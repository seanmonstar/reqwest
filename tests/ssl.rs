#![deny(warnings)]
extern crate hyper;
extern crate hyper_native_tls;
extern crate reqwest;

use hyper::server::{Server, Request, Response, Fresh, Listening};
use hyper_native_tls::NativeTlsServer;

use reqwest::{Client, ClientBuilder, Certificate};

use std::fs::File;
use std::io::Read;
use std::mem;

#[test]
fn test_custom_ca_hostname_verification_disabled() {
    let listening = start_server("tests/certificates/server-wrong.hostname.com.pfx");
    let client = get_client(HostnameVerification::Disabled);
    let mut resp = client.get("https://localhost:12345").send().unwrap();
    let mut body = vec![];
    resp.read_to_end(&mut body).unwrap();
    assert_eq!(body, b"ok");
    mem::forget(listening);
}

#[test]
fn test_custom_ca_hostname_verification_enabled() {
    let listening = start_server("tests/certificates/server-localhost.pfx");
    let client = get_client(HostnameVerification::Enabled);
    let mut resp = client.get("https://localhost:12345").send().unwrap();
    let mut body = vec![];
    resp.read_to_end(&mut body).unwrap();
    assert_eq!(body, b"ok");
    mem::forget(listening);
}

#[derive(PartialEq)]
enum HostnameVerification {
    Enabled,
    Disabled,
}

fn get_client(hostname_verification: HostnameVerification) -> Client {
    let mut buf = Vec::new();
    File::open("tests/certificates/root.der").unwrap().read_to_end(&mut buf).unwrap();
    let cert = Certificate::from_der(&buf).unwrap();

    let mut client_builder = ClientBuilder::new().unwrap();
    client_builder.add_root_certificate(cert).unwrap();
    if hostname_verification == HostnameVerification::Disabled {
        client_builder.danger_disable_hostname_verification();
    }

    client_builder.build().unwrap()
}

fn start_server(cert: &str) -> Listening {
    let ssl = NativeTlsServer::new(cert, "mypass").unwrap();
    let server = Server::https("localhost:12345", ssl).unwrap();
    server.handle(|_: Request, resp: Response<Fresh>| {
        resp.send(b"ok").unwrap();
    }).unwrap()
}
