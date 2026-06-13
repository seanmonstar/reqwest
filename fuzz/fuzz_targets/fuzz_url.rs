#![no_main]
use libfuzzer_sys::fuzz_target;
use bytes::Bytes;
use http::Uri;

fuzz_target!(|data: &[u8]| {
    // reqwest URL parsing — pre-auth for every HTTP request
    let _ = Uri::from_maybe_shared(Bytes::copy_from_slice(data));
});
