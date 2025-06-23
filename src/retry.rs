//! Retry requests
//!
//! A `Client` has the ability to retry requests, by sending additional copies
//! to the server if a response is considered retryable.
//!
//! The [`Builder`] makes it easier to configure what requests to retry, along
//! with including best practices by default, such as a retry budget.
//!
//! # Defaults
//!
//! The default retry behavior of a `Client` is to only retry requests where an
//! error or low-level protocol NACK is encountered that is known to be safe to
//! retry. Note however that providing a specific retry policy will override
//! the default, and you will need to explicitly include that behavior.
//!
//! All policies default to including a retry budget that permits 20% extra
//! requests to be sent.
//!
//! # Scoped
//!
//! A client's retry policy is scoped. That means that the policy doesn't
//! apply to all requests, but only those within a user-defined scope.
//!
//! Since all policies include a budget by default, it doesn't make sense to
//! apply it on _all_ requests. Rather, the retry history applied by a budget
//! should likely only be applied to the same host.
//!
//! # Classifiers
//!
//! A retry policy needs to be configured with a classifier that determines
//! if a request should be retried. Knowledge of the destination server's
//! behavior is required to make a safe classifier. Requests should not be
//! retried if the server cannot safely handle the same request twice, or if it
//! causes side effects.
//!
//! Some common properties to check include if the request method is
//! idempotent, or if the response status code indicates a transient error.

use std::sync::Arc;
use std::time::Duration;

use tower::retry::budget::{Budget as _, TpsBudget as Budget};

/// Builder to configure retries
///
/// Construct with [`scoped()`].
#[derive(Debug)]
pub struct Builder {
    //backoff: Backoff,
    budget: Option<f32>,
    classifier: classify::Classifier,
    max_retries_per_request: u32,
    scope: scope::Scoped,
}

/// The internal type that we convert the builder into, that implements
/// tower::retry::Policy privately.
#[derive(Clone, Debug)]
pub(crate) struct Policy {
    budget: Option<Arc<Budget>>,
    classifier: classify::Classifier,
    max_retries_per_request: u32,
    retry_cnt: u32,
    scope: scope::Scoped,
}

//#[derive(Debug)]
//struct Backoff;

/// Create a retry builder with a request scope.
///
/// To provide a scope that isn't a closure, use the more general
/// [`Builder::scoped()`].
pub fn scoped<F>(func: F) -> Builder
where
    F: Fn(&Req) -> bool + Send + Sync + 'static,
{
    Builder::scoped(scope::ScopeFn(func))
}

// ===== impl Builder =====

impl Builder {
    /// Create a scoped retry policy.
    pub fn scoped(scope: impl scope::Scope) -> Self {
        Self {
            budget: Some(0.2),
            // XXX: should the default be Never?
            classifier: classify::Classifier::ProtocolNacks,
            max_retries_per_request: 2, // on top of the original
            scope: scope::Scoped::Dyn(Arc::new(scope)),
        }
    }

    /// Set no retry budget.
    pub fn no_budget(mut self) -> Self {
        self.budget = None;
        self
    }
    // pub fn max_extra_load()
    // pub fn max_replay_body

    /// Set the max retries allowed per request.
    ///
    /// For each logical (initial) request, only retry up to `max` times.
    ///
    /// This value is used in combination with a token budget that is applied
    /// to all requests. Even if the budget would allow more requests, this
    /// limit will prevent. Likewise, the budget may prevent retying up to
    /// `max` times.
    ///
    /// Default is currently 2 retries.
    pub fn max_retries_per_request(mut self, max: u32) -> Self {
        self.max_retries_per_request = max;
        self
    }

    /// Provide a classifier to determine if a request should be retried.
    ///
    /// # Example
    ///
    /// ```rust
    /// # fn with_builder(builder: reqwest::retry::Builder) -> reqwest::retry::Builder {
    /// builder.classify_fn(|req_rep| {
    ///     match (req_req.method(), req_rep.status()) {
    ///         (http::Method::GET, Some(http::StatusCode::SERVICE_UNAVAILABLE)) => {
    ///             req_rep.retryable()
    ///         },
    ///         _ => req_rep.success()
    ///     }
    /// })
    /// # }
    /// ```
    pub fn classify_fn<F>(self, func: F) -> Self 
    where
        F: Fn(classify::ReqRep<'_>) -> classify::Action + Send + Sync + 'static,
    {
        self.classify(classify::ClassifyFn(func))
    }

    /// Provide a classifier to determine if a request should be retried.
    pub fn classify(mut self, classifier: impl classify::Classify) -> Self {
        self.classifier = classify::Classifier::Dyn(Arc::new(classifier));
        self
    }

    pub(crate) fn default() -> Builder {
        Self {
            // unscoped protocols nacks doesn't need a budget
            budget: None,
            classifier: classify::Classifier::ProtocolNacks,
            max_retries_per_request: 2, // on top of the original
            scope: scope::Scoped::Unscoped,
        }
    }

    pub(crate) fn into_policy(self) -> Policy {
        let budget = self.budget.map(|p| {
            Arc::new(Budget::new(Duration::from_secs(10), 10, p))
        });
        Policy {
            budget,
            classifier: self.classifier,
            max_retries_per_request: self.max_retries_per_request,
            retry_cnt: 0,
            scope: self.scope,
        }
    }
}

// ===== internal ======

type Req = http::Request<crate::async_impl::body::Body>;
type Rep = http::Response<hyper::body::Incoming>;

impl tower::retry::Policy<
    Req,
    Rep,
    crate::Error,
> for Policy {

    // TODO? backoff futures...
    type Future = std::future::Ready<()>;

    fn retry(
        &mut self,
        req: &mut Req,
        result: &mut crate::Result<Rep>,
    ) -> Option<Self::Future> {
        match self.classifier.classify(req, result) {
            classify::Action::Success => {
                log::trace!("shouldn't retry!");
                if let Some(ref budget) = self.budget {
                    budget.deposit();
                }
                None
            },
            classify::Action::Retryable => {
                log::trace!("could retry!");
                if self.budget.as_ref().map(|b| b.withdraw()).unwrap_or(true) {
                    self.retry_cnt += 1;
                    Some(std::future::ready(()))
                } else {
                    log::debug!("retryable but could not withdraw from budget");
                    None
                }
            }
        }
    }

    fn clone_request(&mut self, req: &Req) -> Option<Req> {
        if self.retry_cnt > 0 && !self.scope.applies_to(req) {
            return None;
        }
        if self.retry_cnt >= self.max_retries_per_request {
            log::trace!("max_retries_per_request hit");
            return None;
        }
        let body = req.body().try_clone()?;
        let mut new = http::Request::new(body);
        *new.method_mut() = req.method().clone();
        *new.uri_mut() = req.uri().clone();
        *new.version_mut() = req.version();
        *new.headers_mut() = req.headers().clone();
        *new.extensions_mut() = req.extensions().clone();

        Some(new)
    }
}

fn is_retryable_error(err: &crate::Error) -> bool {
    use std::error::Error as _;

    // pop the reqwest::Error
    let err = if let Some(err) = err.source() {
        err
    } else {
        return false;
    };
    // pop the legacy::Error
    let err = if let Some(err) = err.source() {
        err
    } else {
        return false;
    };

    #[cfg(feature = "http3")]
    if let Some(cause) = err.source() {
        if let Some(err) = cause.downcast_ref::<h3::error::ConnectionError>() {
            log::debug!("determining if HTTP/3 error {err} can be retried");
            // TODO: Does h3 provide an API for checking the error?
            return err.to_string().as_str() == "timeout";
        }
    }

    #[cfg(feature = "http2")]
    if let Some(cause) = err.source() {
        if let Some(err) = cause.downcast_ref::<h2::Error>() {
            // They sent us a graceful shutdown, try with a new connection!
            if err.is_go_away() && err.is_remote() && err.reason() == Some(h2::Reason::NO_ERROR) {
                return true;
            }

            // REFUSED_STREAM was sent from the server, which is safe to retry.
            // https://www.rfc-editor.org/rfc/rfc9113.html#section-8.7-3.2
            if err.is_reset() && err.is_remote() && err.reason() == Some(h2::Reason::REFUSED_STREAM)
            {
                return true;
            }
        }
    }
    false
}

// sealed types and traits on purpose while exploring design space
mod scope {
    pub trait Scope: Send + Sync + 'static {
        fn applies_to(&self, req: &super::Req) -> bool;
    }

    // I think scopes likely make the most sense being to hosts.
    // If that's the case, then it should probably be easiest to check for
    // the host. Perhaps also considering the ability to add more things
    // to scope off in the future...

    // For Future Whoever: making a blanket impl for any closure sounds nice,
    // but it causes inference issues at the call site. Every closure would
    // need to include `: ReqRep` in the arguments.
    //
    // An alternative is to make things like `ScopeFn`. Slightly more annoying,
    // but also more forwards-compatible. :shrug:

    pub struct ScopeFn<F>(pub(super) F);

    impl<F> Scope for ScopeFn<F>
    where
        F: Fn(&super::Req) -> bool + Send + Sync + 'static,
    {
        fn applies_to(&self, req: &super::Req) -> bool {
            (self.0)(req)
        }
    }

    #[derive(Clone)]
    pub(super) enum Scoped {
        Unscoped,
        Dyn(std::sync::Arc<dyn Scope>),
    }

    impl Scoped {
        pub(super) fn applies_to(&self, req: &super::Req) -> bool {
            let ret = match self {
                Self::Unscoped => true,
                Self::Dyn(s) => s.applies_to(req),
            };
            log::trace!("retry in scope: {ret}");
            ret
        }
    }

    impl std::fmt::Debug for Scoped {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Unscoped => f.write_str("Unscoped"),
                Self::Dyn(_) => f.write_str("Scoped"),
            }
        }
    }
}

// sealed types and traits on purpose while exploring design space
mod classify {
    pub trait Classify: Send + Sync + 'static {
        fn classify(&self, req_rep: ReqRep<'_>) -> Action;
    }

    pub struct ClassifyFn<F>(pub(super) F);

    // blanket impl for closures
    impl<F> Classify for ClassifyFn<F>
    where
        F: Fn(ReqRep<'_>) -> Action + Send + Sync + 'static,
    {
        fn classify(&self, req_rep: ReqRep<'_>) -> Action {
            (self.0)(req_rep)
        }
    }

    #[derive(Debug)]
    pub struct ReqRep<'a>(&'a super::Req, &'a Result<super::Rep, crate::Error>);

    impl ReqRep<'_> {
        pub fn method(&self) -> &http::Method {
            self.0.method()
        }

        pub fn uri(&self) -> &http::Uri {
            self.0.uri()
        }

        pub fn status(&self) -> Option<http::StatusCode> {
            self.1.as_ref().ok().map(|r| r.status())
        }

        pub fn error(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.1.as_ref().err().map(|e| &*e as _)
        }

        pub fn retryable(self) -> Action {
            Action::Retryable
        }

        pub fn success(self) -> Action {
            Action::Success
        }

        fn is_protocol_nack(&self) -> bool {
            self.1.as_ref().err().map(super::is_retryable_error).unwrap_or(false)
        }
    }

    #[must_use]
    #[derive(Debug)]
    pub enum Action {
        Success,
        Retryable,
    }

    #[derive(Clone)]
    pub(super) enum Classifier {
        ProtocolNacks,
        Dyn(std::sync::Arc<dyn Classify>),
    }

    impl Classifier {
        pub(super) fn classify(&self, req: &super::Req, res: &Result<super::Rep, crate::Error>) -> Action {
            match self {
                Self::ProtocolNacks => {
                    if ReqRep(req, res).is_protocol_nack() {
                        Action::Retryable
                    } else {
                        Action::Success
                    }
                },
                Self::Dyn(c) => c.classify(ReqRep(req, res)),
            }
        }
    }

    impl std::fmt::Debug for Classifier {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::ProtocolNacks => f.write_str("ProtocolNacks"),
                Self::Dyn(_) => f.write_str("Classifier"),
            }
        }
    }
}
