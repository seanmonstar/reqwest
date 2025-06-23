//! Retry requests

use std::sync::Arc;
use std::time::Duration;

use tower::retry::budget::{Budget as _, TpsBudget as Budget};

/// dox
#[derive(Debug)]
pub struct Builder {
    //backoff: Backoff,
    budget: Option<f32>,
    classifier: classify::Classifier,
    scope: scope::Scoped,
}

/// The internal type that we convert the builder into, that implements
/// tower::retry::Policy privately.
#[derive(Clone, Debug)]
pub(crate) struct Policy {
    budget: Option<Arc<Budget>>,
    classifier: classify::Classifier,
    scope: scope::Scoped,
}

//#[derive(Debug)]
//struct Backoff;

// ===== impl Builder =====

impl Builder {
    /// Create a scoped retry policy.
    pub fn scoped(scope: impl scope::Scope) -> Self {
        Self {
            budget: Some(0.2),
            // XXX: should the default be Never?
            classifier: classify::Classifier::ProtocolNacks,
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

    /// Provide a classifier to determine if a request should be retried.
    ///
    /// # Example
    ///
    /// ```rust
    /// # fn with_builder(builder: reqwest::retry::Builder) -> reqwest::retry::Builder {
    /// builder.classify(|req_rep| {
    ///     match (req_req.method(), req_rep.status()) {
    ///         (http::Method::GET, http::StatusCode::SERVICE_UNAVAILABLE) => {
    ///             req_rep.retryable()
    ///         },
    ///         _ => req_rep.success()
    ///     }
    /// })
    /// # }
    /// ```
    pub fn classify<C>(mut self, classifier: impl classify::Classify) -> Self {
        self.classifier = classify::Classifier::Dyn(Arc::new(classifier));
        self
    }

    pub(crate) fn default() -> Builder {
        Self {
            budget: Some(0.2),
            classifier: classify::Classifier::ProtocolNacks,
            scope: scope::Scoped::Unscoped,
        }
    }

    pub(crate) fn into_policy(self) -> Policy {
        let budget = self.budget.map(|p| {
            Arc::new(Budget::new(Duration::from_secs(10), 0, p))
        });
        Policy {
            budget,
            classifier: self.classifier,
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
                if let Some(ref budget) = self.budget {
                    budget.deposit();
                }
                None
            },
            classify::Action::Retryable => {
                log::trace!("can retry");
                if self.budget.as_ref().map(|b| b.withdraw()).unwrap_or(true) {
                    Some(std::future::ready(()))
                } else {
                    log::debug!("retryable but could not withdraw from budget");
                    None
                }
            }
        }
    }

    fn clone_request(&mut self, req: &Req) -> Option<Req> {
        if !self.scope.applies_to(req) {
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
    pub trait Scope: Send + Sync + 'static {}

    #[derive(Clone)]
    pub(super) enum Scoped {
        Unscoped,
        Dyn(std::sync::Arc<dyn Scope>),
    }

    impl Scoped {
        pub(super) fn applies_to(&self, _req: &super::Req) -> bool {
            match self {
                Self::Unscoped => true,
                Self::Dyn(_s) => todo!("dyn scoped applies to"),
            }
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

    // blanket impl for closures
    impl<F> Classify for F
    where
        F: Fn(ReqRep<'_>) -> Action + Send + Sync + 'static,
    {
        fn classify(&self, req_rep: ReqRep<'_>) -> Action {
            (self)(req_rep)
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
