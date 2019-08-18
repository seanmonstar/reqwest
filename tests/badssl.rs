extern crate reqwest;


#[cfg(feature = "tls")]
#[test]
fn test_badssl_modern() {
    let text = reqwest::get("https://mozilla-modern.badssl.com/").unwrap()
        .text().unwrap();

    assert!(text.contains("<title>mozilla-modern.badssl.com</title>"));
}

#[cfg(feature = "rustls-tls")]
#[test]
fn test_rustls_badssl_modern() {
    let text = reqwest::Client::builder()
        .use_rustls_tls()
        .build().unwrap()
        .get("https://mozilla-modern.badssl.com/")
        .send().unwrap()
        .text().unwrap();

    assert!(text.contains("<title>mozilla-modern.badssl.com</title>"));
}

#[cfg(feature = "tls")]
#[test]
fn test_badssl_self_signed() {
    let text = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build().unwrap()
        .get("https://self-signed.badssl.com/")
        .send().unwrap()
        .text().unwrap();

    assert!(text.contains("<title>self-signed.badssl.com</title>"));
}

#[cfg(feature = "default-tls")]
#[test]
fn test_badssl_wrong_host() {
    let text = reqwest::Client::builder()
        .danger_accept_invalid_hostnames(true)
        .build().unwrap()
        .get("https://wrong.host.badssl.com/")
        .send().unwrap()
        .text().unwrap();

    assert!(text.contains("<title>wrong.host.badssl.com</title>"));


    let result = reqwest::Client::builder()
        .danger_accept_invalid_hostnames(true)
        .build().unwrap()
        .get("https://self-signed.badssl.com/")
        .send();

    assert!(result.is_err());
}
