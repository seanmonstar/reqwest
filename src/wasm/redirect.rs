//! Redirect Handling
//!
//! By default, a `Client` will automatically follow HTTP redirects. To customize this behavior, a
//! `redirect::Policy` can be used with a `ClientBuilder`.

/// A type that controls the policy on how to handle the following of redirects.
///
/// The default value follow redirects https://developer.mozilla.org/en-US/docs/Web/API/Request/redirect
///
/// - `none` can be used to disable all redirect behavior. This sets the redirect value to "manual".
#[derive(Debug, PartialEq, Clone)]
pub struct Policy {
    inner: PolicyKind,
}

impl Policy {
    /// Create a `Policy` that does not follow any redirect.
    pub fn none() -> Self {
        Self {
            inner: PolicyKind::None,
        }
    }

    fn not_set() -> Self {
        Self {
            inner: PolicyKind::NotSet,
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
enum PolicyKind {
    None,
    NotSet,
}

impl Default for Policy {
    fn default() -> Policy {
        Policy::not_set()
    }
}
