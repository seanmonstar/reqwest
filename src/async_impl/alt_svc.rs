//! Minimal RFC 7838 `Alt-Svc` discovery for HTTP/3.
//!
//! Only authenticated HTTPS origins are trusted, and only an `h3` alternative
//! on the origin's own host and port is accepted. This matches what
//! [`H3Connector`](crate::async_impl::h3_client::connect::H3Connector) is able
//! to dial today: it uses the URL's authority for both SNI and the UDP
//! destination. Accepting a different host would require origin-host
//! certificate verification and a second DNS lookup, which is deliberately
//! left out of this minimal implementation.

#![cfg(feature = "http3")]

use http::header::AGE;
use http::{HeaderMap, StatusCode};
use std::collections::HashMap;
use std::sync::{Arc, RwLock, RwLockWriteGuard};
use std::time::{Duration, Instant};
use url::Url;

/// Maximum number of cached alternatives. Bounds memory use from hostile servers.
const MAX_ENTRIES: usize = 64;
/// Maximum total `Alt-Svc` header bytes processed from a single response.
const MAX_HEADER_BYTES: usize = 8 * 1024;
/// RFC 7838 §3.1 default freshness, also used as a hard cap (`ma` can be much larger).
const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
/// Suppress repeated H3 attempts for a short time after a failed connect.
const FAILURE_BACKOFF: Duration = Duration::from_secs(5 * 60);

/// A cache of `Alt-Svc` advertisements, keyed by RFC 6454 origin.
///
/// Cheap to clone; all clones share the same entries.
#[derive(Clone, Default)]
pub(crate) struct AltSvcCache {
    inner: Arc<RwLock<Inner>>,
}

#[derive(Default)]
struct Inner {
    entries: HashMap<Origin, Entry>,
    backoff: HashMap<Origin, Instant>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct Origin {
    host: String,
    port: u16,
}

#[derive(Clone, Debug)]
struct Entry {
    port: u16,
    expires: Instant,
    last_used: Instant,
}

/// The result of parsing a single acceptable `h3` alt-value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Update {
    /// `ma=0`: invalidate the cached alternative.
    Remove,
    /// Advertise HTTP/3 on the given port for the given freshness lifetime.
    Set { port: u16, max_age: Duration },
}

impl AltSvcCache {
    /// Process all `Alt-Svc` fields from a response for its origin.
    ///
    /// Per RFC 7838 §3.1, a valid `Alt-Svc` replaces all cached alternatives
    /// for that origin. `clear` invalidates them. Responses to `421` are
    /// ignored (§6), as are plaintext HTTP responses (no authenticated
    /// binding to the origin).
    pub(crate) fn observe(&self, url: &Url, status: StatusCode, headers: &HeaderMap) {
        if !is_secure(url) || status == StatusCode::MISDIRECTED_REQUEST {
            return;
        }
        let origin = match origin_of(url) {
            Some(origin) => origin,
            None => return,
        };

        let values = headers.get_all(http::header::ALT_SVC);
        let mut bytes = 0usize;
        for value in values.iter() {
            bytes = bytes.saturating_add(value.as_bytes().len());
        }
        if bytes > MAX_HEADER_BYTES {
            return;
        }

        let now = Instant::now();
        let mut inner = write_lock(&self.inner);
        inner.purge(now);
        if is_backed_off(&inner, &origin, now) {
            return;
        }

        if values.iter().any(|value| has_clear(value.as_bytes())) {
            inner.entries.remove(&origin);
            return;
        }

        let age = parse_age(headers);
        let update = values
            .iter()
            .find_map(|value| parse_h3(value.as_bytes(), origin.port, age));
        match update {
            Some(Update::Remove) => {
                inner.entries.remove(&origin);
            }
            Some(Update::Set { port, max_age }) => {
                if inner.entries.len() >= MAX_ENTRIES && !inner.entries.contains_key(&origin) {
                    evict_oldest(&mut inner.entries);
                }
                inner.entries.insert(
                    origin,
                    Entry {
                        port,
                        expires: now + max_age.min(MAX_AGE),
                        last_used: now,
                    },
                );
            }
            None => {}
        }
    }

    /// Whether a fresh, non-backed-off H3 alternative is known for this URL.
    pub(crate) fn should_try_h3(&self, url: &Url) -> bool {
        if !is_secure(url) {
            return false;
        }
        let origin = match origin_of(url) {
            Some(origin) => origin,
            None => return false,
        };

        let now = Instant::now();
        let mut inner = write_lock(&self.inner);
        inner.purge(now);
        if is_backed_off(&inner, &origin, now) {
            return false;
        }
        match inner.entries.get_mut(&origin) {
            Some(entry) if entry.port == origin.port && entry.expires > now => {
                entry.last_used = now;
                true
            }
            _ => false,
        }
    }

    /// Record a failed H3 attempt: drop the advertisement and back off.
    pub(crate) fn invalidate(&self, url: &Url) {
        if !is_secure(url) {
            return;
        }
        let origin = match origin_of(url) {
            Some(origin) => origin,
            None => return,
        };

        let now = Instant::now();
        let mut inner = write_lock(&self.inner);
        inner.entries.remove(&origin);
        inner.backoff.insert(origin, now + FAILURE_BACKOFF);
        inner.purge(now);
    }

    #[cfg(test)]
    fn expire_for_test(&self, url: &Url) {
        let origin = origin_of(url).unwrap();
        let mut inner = write_lock(&self.inner);
        if let Some(entry) = inner.entries.get_mut(&origin) {
            entry.expires = Instant::now() - Duration::from_secs(1);
        }
    }
}

impl Inner {
    fn purge(&mut self, now: Instant) {
        self.entries.retain(|_, entry| entry.expires > now);
        self.backoff.retain(|_, until| *until > now);
        while self.backoff.len() > MAX_ENTRIES {
            let key = self.backoff.keys().next().expect("non-empty").clone();
            self.backoff.remove(&key);
        }
    }
}

fn write_lock(lock: &RwLock<Inner>) -> RwLockWriteGuard<'_, Inner> {
    // A poisoned lock contains no invariant that could cause memory unsafety;
    // recover rather than turning a request into a panic.
    lock.write().unwrap_or_else(|err| err.into_inner())
}

fn is_backed_off(inner: &Inner, origin: &Origin, now: Instant) -> bool {
    inner
        .backoff
        .get(origin)
        .is_some_and(|until| *until > now)
}

fn is_secure(url: &Url) -> bool {
    url.scheme() == "https"
}

fn origin_of(url: &Url) -> Option<Origin> {
    Some(Origin {
        host: url.host_str()?.to_ascii_lowercase(),
        port: url.port_or_known_default()?,
    })
}

fn evict_oldest(entries: &mut HashMap<Origin, Entry>) {
    let oldest = entries
        .iter()
        .min_by_key(|(_, entry)| entry.last_used)
        .map(|(origin, _)| origin.clone());
    if let Some(origin) = oldest {
        entries.remove(&origin);
    }
}

fn parse_age(headers: &HeaderMap) -> u64 {
    headers
        .get(AGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// `clear` is a case-sensitive standalone alt-value (RFC 7838 §3).
fn has_clear(value: &[u8]) -> bool {
    split_values(value).iter().any(|part| *part == b"clear")
}

/// Find the first acceptable `h3` alt-value in a field value.
fn parse_h3(value: &[u8], origin_port: u16, age: u64) -> Option<Update> {
    split_values(value)
        .iter()
        .find_map(|part| parse_alt_value(part, origin_port, age))
}

fn parse_alt_value(part: &[u8], origin_port: u16, age: u64) -> Option<Update> {
    let eq = part.iter().position(|byte| *byte == b'=')?;
    if part[..eq] != b"h3"[..] {
        // Protocol IDs are ALPN tokens and are case-sensitive.
        return None;
    }

    let quoted = &part[eq + 1..];
    if quoted.first() != Some(&b'"') {
        return None;
    }
    let end = find_closing_quote(quoted)?;
    let authority = &quoted[1..end];

    // Same-host only: `":port"`, no explicit host, IP literal or userinfo.
    let port_digits = authority.strip_prefix(b":")?;
    if port_digits.is_empty() || !port_digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let port = std::str::from_utf8(port_digits)
        .ok()?
        .parse::<u16>()
        .ok()?;
    if port == 0 || port != origin_port {
        // The connector dials the URL port, so another port would be wrong.
        return None;
    }

    let mut max_age = None;
    for param in split_params(&quoted[end + 1..]) {
        let param = trim(param);
        if param.is_empty() {
            continue;
        }
        let Some(eq) = param.iter().position(|byte| *byte == b'=') else {
            continue;
        };
        let name = trim(&param[..eq]);
        let value = trim_param_value(&param[eq + 1..]);
        if !name.eq_ignore_ascii_case(b"ma") {
            // Unknown parameters MUST be ignored (RFC 7838 §3).
            continue;
        }
        if value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let seconds = parse_u64_saturating(value);
        if let Some(previous) = max_age {
            if previous != seconds {
                // Conflicting `ma` values: fail closed.
                return None;
            }
        }
        max_age = Some(seconds);
    }

    let seconds = max_age.unwrap_or(MAX_AGE.as_secs());
    if seconds == 0 {
        return Some(Update::Remove);
    }
    Some(Update::Set {
        port,
        max_age: Duration::from_secs(seconds.saturating_sub(age)).min(MAX_AGE),
    })
}

fn find_closing_quote(input: &[u8]) -> Option<usize> {
    let mut escaped = false;
    for (index, byte) in input.iter().enumerate().skip(1) {
        if escaped {
            escaped = false;
        } else if *byte == b'\\' {
            escaped = true;
        } else if *byte == b'"' {
            return Some(index);
        }
    }
    None
}

fn split_values(input: &[u8]) -> Vec<&[u8]> {
    split_unquoted(input, b',')
}

fn split_params(input: &[u8]) -> Vec<&[u8]> {
    split_unquoted(input, b';')
}

fn split_unquoted(input: &[u8], delimiter: u8) -> Vec<&[u8]> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in input.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted {
            match byte {
                b'\\' => escaped = true,
                b'"' => quoted = false,
                _ => {}
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b if *b == delimiter => {
                    parts.push(trim(&input[start..index]));
                    start = index + 1;
                }
                _ => {}
            }
        }
    }
    parts.push(trim(&input[start..]));
    parts
}

fn trim(mut input: &[u8]) -> &[u8] {
    while input.first().is_some_and(u8::is_ascii_whitespace) {
        input = &input[1..];
    }
    while input.last().is_some_and(u8::is_ascii_whitespace) {
        input = &input[..input.len() - 1];
    }
    input
}

fn trim_param_value(input: &[u8]) -> &[u8] {
    let input = trim(input);
    if input.len() >= 2 && input[0] == b'"' && input[input.len() - 1] == b'"' {
        &input[1..input.len() - 1]
    } else {
        input
    }
}

fn parse_u64_saturating(input: &[u8]) -> u64 {
    let text = std::str::from_utf8(input).expect("ASCII digits");
    text.parse::<u64>().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::header::HeaderValue;

    fn header(value: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.append(http::header::ALT_SVC, HeaderValue::from_static(value));
        headers
    }

    fn url() -> Url {
        Url::parse("https://example.com/").unwrap()
    }

    #[test]
    fn parses_h3_same_host_port() {
        assert_eq!(
            parse_h3(b"h3=\":443\"; ma=60", 443, 0),
            Some(Update::Set {
                port: 443,
                max_age: Duration::from_secs(60),
            })
        );
        assert_eq!(
            parse_h3(b"h2=\":80\", h3=\":443\"", 443, 0),
            Some(Update::Set {
                port: 443,
                max_age: MAX_AGE,
            })
        );
    }

    #[test]
    fn subtracts_age_and_caps_max_age() {
        assert_eq!(
            parse_h3(b"h3=\":443\"; ma=60", 443, 30),
            Some(Update::Set {
                port: 443,
                max_age: Duration::from_secs(30),
            })
        );
        assert_eq!(
            parse_h3(b"h3=\":443\"; ma=2592000", 443, 0),
            Some(Update::Set {
                port: 443,
                max_age: MAX_AGE,
            })
        );
        assert_eq!(
            parse_h3(b"h3=\":443\"; ma=10", 443, 30),
            Some(Update::Set {
                port: 443,
                max_age: Duration::ZERO,
            })
        );
    }

    #[test]
    fn rejects_unsafe_or_malformed_forms() {
        assert_eq!(parse_h3(b"h3=\":8443\"", 443, 0), None);
        assert_eq!(parse_h3(b"h3=\"alt.example:443\"", 443, 0), None);
        assert_eq!(parse_h3(b"h3=\"[::1]:443\"", 443, 0), None);
        assert_eq!(parse_h3(b"h3=\":0\"", 443, 0), None);
        assert_eq!(parse_h3(b"H3=\":443\"", 443, 0), None);
        assert_eq!(parse_h3(b"h3-29=\":443\"", 443, 0), None);
        assert_eq!(parse_h3(b"h3=\":abc\"", 443, 0), None);
        assert_eq!(parse_h3(b"h3=\":443\"; ma=abc", 443, 0), None);
        assert_eq!(parse_h3(b"h3=\":443\"; ma=1; ma=2", 443, 0), None);
        assert!(!has_clear(b"Clear"));
        assert!(has_clear(b"clear"));
        assert!(has_clear(b"h3=\":443\", clear"));
    }

    #[test]
    fn ma_zero_and_clear_remove_entries() {
        let cache = AltSvcCache::default();
        let url = url();
        cache.observe(&url, StatusCode::OK, &header("h3=\":443\"; ma=60"));
        assert!(cache.should_try_h3(&url));

        cache.observe(&url, StatusCode::OK, &header("h3=\":443\"; ma=0"));
        assert!(!cache.should_try_h3(&url));

        cache.observe(&url, StatusCode::OK, &header("h3=\":443\"; ma=60"));
        cache.observe(&url, StatusCode::OK, &header("clear"));
        assert!(!cache.should_try_h3(&url));
    }

    #[test]
    fn ignores_insecure_redirect_and_421() {
        let cache = AltSvcCache::default();
        let alt = header("h3=\":80\"; ma=60");
        cache.observe(
            &Url::parse("http://example.com/").unwrap(),
            StatusCode::OK,
            &alt,
        );
        assert!(!cache.should_try_h3(&Url::parse("http://example.com/").unwrap()));

        let url = url();
        cache.observe(&url, StatusCode::MISDIRECTED_REQUEST, &header("h3=\":443\""));
        assert!(!cache.should_try_h3(&url));
    }

    #[test]
    fn expiry_and_failure_backoff() {
        let cache = AltSvcCache::default();
        let url = url();
        cache.observe(&url, StatusCode::OK, &header("h3=\":443\"; ma=60"));
        cache.expire_for_test(&url);
        assert!(!cache.should_try_h3(&url));

        cache.observe(&url, StatusCode::OK, &header("h3=\":443\"; ma=60"));
        cache.invalidate(&url);
        assert!(!cache.should_try_h3(&url));
        // A fresh advertisement cannot resurrect the entry during backoff.
        cache.observe(&url, StatusCode::OK, &header("h3=\":443\"; ma=60"));
        assert!(!cache.should_try_h3(&url));
    }

    #[test]
    fn entries_are_per_origin() {
        let cache = AltSvcCache::default();
        cache.observe(
            &Url::parse("https://a.example/").unwrap(),
            StatusCode::OK,
            &header("h3=\":443\""),
        );
        assert!(!cache.should_try_h3(&Url::parse("https://b.example/").unwrap()));
        assert!(cache.should_try_h3(&Url::parse("https://a.example/").unwrap()));
    }
}
