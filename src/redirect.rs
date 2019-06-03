use std::fmt;

use header::{
    HeaderMap,
    AUTHORIZATION,
    COOKIE,
    PROXY_AUTHORIZATION,
    WWW_AUTHENTICATE,

};
use hyper::StatusCode;

use Url;

/// A type that controls the policy on how to handle the following of redirects.
///
/// The default value will catch redirect loops, and has a maximum of 10
/// redirects it will follow in a chain before returning an error.
///
/// - `limited` can be used have the same as the default behavior, but adjust
///   the allowed maximum redirect hops in a chain.
/// - `none` can be used to disable all redirect behavior.
/// - `custom` can be used to create a customized policy.
#[derive(Debug)]
pub struct RedirectPolicy {
    inner: Policy,
}

/// A type that holds information on the next request and previous requests
/// in redirect chain.
#[derive(Debug)]
pub struct RedirectAttempt<'a> {
    status: StatusCode,
    next: &'a Url,
    previous: &'a [Url],
}

/// An action to perform when a redirect status code is found.
#[derive(Debug)]
pub struct RedirectAction {
    inner: Action,
}

impl RedirectPolicy {
    /// Create a RedirectPolicy with a maximum number of redirects.
    ///
    /// An `Error` will be returned if the max is reached.
    pub fn limited(max: usize) -> RedirectPolicy {
        RedirectPolicy {
            inner: Policy::Limit(max),
        }
    }

    /// Create a RedirectPolicy that does not follow any redirect.
    pub fn none() -> RedirectPolicy {
        RedirectPolicy {
            inner: Policy::None,
        }
    }

    /// Create a custom RedirectPolicy using the passed function.
    ///
    /// # Note
    ///
    /// The default RedirectPolicy handles redirect loops and a maximum loop
    /// chain, but the custom variant does not do that for you automatically.
    /// The custom policy should have some way of handling those.
    ///
    /// Information on the next request and previous requests can be found
    /// on the [`RedirectAttempt`] argument passed to the closure.
    ///
    /// Actions can be conveniently created from methods on the
    /// [`RedirectAttempt`].
    ///
    /// # Example
    ///
    /// ```rust
    /// # use reqwest::{Error, RedirectPolicy};
    /// #
    /// # fn run() -> Result<(), Error> {
    /// let custom = RedirectPolicy::custom(|attempt| {
    ///     if attempt.previous().len() > 5 {
    ///         attempt.too_many_redirects()
    ///     } else if attempt.url().host_str() == Some("example.domain") {
    ///         // prevent redirects to 'example.domain'
    ///         attempt.stop()
    ///     } else {
    ///         attempt.follow()
    ///     }
    /// });
    /// let client = reqwest::Client::builder()
    ///     .redirect(custom)
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`RedirectAttempt`]: struct.RedirectAttempt.html
    pub fn custom<T>(policy: T) -> RedirectPolicy
    where
        T: Fn(RedirectAttempt) -> RedirectAction + Send + Sync + 'static,
    {
        RedirectPolicy {
            inner: Policy::Custom(Box::new(policy)),
        }
    }

    /// Apply this policy to a given [`RedirectAttempt`] to produce a [`RedirectAction`].
    ///
    /// # Note
    ///
    /// This method can be used together with RedirectPolicy::custom()
    /// to construct one RedirectPolicy that wraps another.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use reqwest::{Error, RedirectPolicy};
    /// #
    /// # fn run() -> Result<(), Error> {
    /// let custom = RedirectPolicy::custom(|attempt| {
    ///     eprintln!("{}, Location: {:?}", attempt.status(), attempt.url());
    ///     RedirectPolicy::default().redirect(attempt)
    /// });
    /// # Ok(())
    /// # }
    /// ```
    pub fn redirect(&self, attempt: RedirectAttempt) -> RedirectAction {
        match self.inner {
            Policy::Custom(ref custom) => custom(attempt),
            Policy::Limit(max) => {
                if attempt.previous.len() == max {
                    attempt.too_many_redirects()
                } else if attempt.previous.contains(attempt.next) {
                    attempt.loop_detected()
                } else {
                    attempt.follow()
                }
            }
            Policy::None => attempt.stop(),
        }
    }

    pub(crate) fn check(
        &self,
        status: StatusCode,
        next: &Url,
        previous: &[Url],
    ) -> Action {
        self
            .redirect(RedirectAttempt {
                status: status,
                next: next,
                previous: previous,
            })
            .inner
    }
}

impl Default for RedirectPolicy {
    fn default() -> RedirectPolicy {
        RedirectPolicy::limited(10)
    }
}

impl<'a> RedirectAttempt<'a> {
    /// Get the type of redirect.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Get the next URL to redirect to.
    pub fn url(&self) -> &Url {
        self.next
    }

    /// Get the list of previous URLs that have already been requested in this chain.
    pub fn previous(&self) -> &[Url] {
        self.previous
    }
    /// Returns an action meaning reqwest should follow the next URL.
    pub fn follow(self) -> RedirectAction {
        RedirectAction {
            inner: Action::Follow,
        }
    }

    /// Returns an action meaning reqwest should not follow the next URL.
    ///
    /// The 30x response will be returned as the `Ok` result.
    pub fn stop(self) -> RedirectAction {
        RedirectAction {
            inner: Action::Stop,
        }
    }

    /// Returns an action meaning there was a loop of redirects found.
    ///
    /// An `Error` will be returned for the result of the sent request.
    pub fn loop_detected(self) -> RedirectAction {
        RedirectAction {
            inner: Action::LoopDetected,
        }
    }

    /// Returns an action meaning there was a loop of redirects found.
    ///
    /// An `Error` will be returned for the result of the sent request.
    pub fn too_many_redirects(self) -> RedirectAction {
        RedirectAction {
            inner: Action::TooManyRedirects,
        }
    }
}

enum Policy {
    Custom(Box<dyn Fn(RedirectAttempt) -> RedirectAction + Send + Sync + 'static>),
    Limit(usize),
    None,
}

impl fmt::Debug for Policy {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Policy::Custom(..) => f.pad("Custom"),
            Policy::Limit(max) => f.debug_tuple("Limit").field(&max).finish(),
            Policy::None => f.pad("None"),
        }
    }
}

// pub(crate)

#[derive(Debug, PartialEq)]
pub(crate) enum Action {
    Follow,
    Stop,
    LoopDetected,
    TooManyRedirects,
}


pub(crate) fn remove_sensitive_headers(headers: &mut HeaderMap, next: &Url, previous: &[Url]) {
    if let Some(previous) = previous.last() {
        let cross_host = next.host_str() != previous.host_str() ||
                         next.port_or_known_default() != previous.port_or_known_default();
        if cross_host {
            headers.remove(AUTHORIZATION);
            headers.remove(COOKIE);
            headers.remove("cookie2");
            headers.remove(PROXY_AUTHORIZATION);
            headers.remove(WWW_AUTHENTICATE);
        }
    }
}

/*
This was the desired way of doing it, but ran in to inference issues when
using closures, since the arguments received are references (&Url and &[Url]),
and the compiler could not infer the lifetimes of those references. That means
people would need to annotate the closure's argument types, which is garbase.

pub trait Redirect {
    fn redirect(&self, next: &Url, previous: &[Url]) -> ::Result<bool>;
}

impl<F> Redirect for F
where F: Fn(&Url, &[Url]) -> ::Result<bool> {
    fn redirect(&self, next: &Url, previous: &[Url]) -> ::Result<bool> {
        self(next, previous)
    }
}
*/

#[test]
fn test_redirect_policy_limit() {
    let policy = RedirectPolicy::default();
    let next = Url::parse("http://x.y/z").unwrap();
    let mut previous = (0..9)
        .map(|i| Url::parse(&format!("http://a.b/c/{}", i)).unwrap())
        .collect::<Vec<_>>();

    assert_eq!(
        policy.check(StatusCode::FOUND, &next, &previous),
        Action::Follow
    );

    previous.push(Url::parse("http://a.b.d/e/33").unwrap());

    assert_eq!(
        policy.check(StatusCode::FOUND, &next, &previous),
        Action::TooManyRedirects
    );
}

#[test]
fn test_redirect_policy_custom() {
    let policy = RedirectPolicy::custom(|attempt| {
        if attempt.url().host_str() == Some("foo") {
            attempt.stop()
        } else {
            attempt.follow()
        }
    });

    let next = Url::parse("http://bar/baz").unwrap();
    assert_eq!(
        policy.check(StatusCode::FOUND, &next, &[]),
        Action::Follow
    );

    let next = Url::parse("http://foo/baz").unwrap();
    assert_eq!(
        policy.check(StatusCode::FOUND, &next, &[]),
        Action::Stop
    );
}

#[test]
fn test_remove_sensitive_headers() {
    use hyper::header::{ACCEPT, AUTHORIZATION, COOKIE, HeaderValue};

    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
    headers.insert(AUTHORIZATION, HeaderValue::from_static("let me in"));
    headers.insert(COOKIE, HeaderValue::from_static("foo=bar"));

    let next = Url::parse("http://initial-domain.com/path").unwrap();
    let mut prev = vec![Url::parse("http://initial-domain.com/new_path").unwrap()];
    let mut filtered_headers = headers.clone();

    remove_sensitive_headers(&mut headers, &next, &prev);
    assert_eq!(headers, filtered_headers);

    prev.push(Url::parse("http://new-domain.com/path").unwrap());
    filtered_headers.remove(AUTHORIZATION);
    filtered_headers.remove(COOKIE);

    remove_sensitive_headers(&mut headers, &next, &prev);
    assert_eq!(headers, filtered_headers);
}
