#![deny(warnings)]
extern crate hyper;
extern crate hyper_native_tls;
extern crate reqwest;

use hyper::server::{Server, Request, Response, Fresh};
use hyper_native_tls::NativeTlsServer;

use reqwest::{Client, ClientBuilder, Certificate};

use std::fs::File;
use std::io::Read;
use std::mem;

#[test]
fn test_custom_ca_hostname_verification_disabled() {
    let ssl = NativeTlsServer::new("tests/certificates/server-wrong.hostname.com.pfx", "mypass").unwrap();
    let server = Server::https("localhost:12345", ssl).unwrap();
    let listening = server.handle(|_: Request, resp: Response<Fresh>| {
        resp.send(b"ok").unwrap();
    }).unwrap();
    mem::forget(listening);

    let client = get_client(true);

    let mut resp = client.get("https://localhost:12345").send().unwrap();
    let mut body = vec![];
    resp.read_to_end(&mut body).unwrap();
    assert_eq!(body, b"ok");
}

#[test]
fn test_custom_ca_hostname_verification_enabled() {
    let ssl = NativeTlsServer::new("tests/certificates/server-localhost.pfx", "mypass").unwrap();
    let server = Server::https("localhost:12344", ssl).unwrap();
    let listening = server.handle(|_: Request, resp: Response<Fresh>| {
        resp.send(b"ok").unwrap();
    }).unwrap();
    mem::forget(listening);

    let client = get_client(false);
    let mut resp = client.get("https://localhost:12344").send().unwrap();
    let mut body = vec![];
    resp.read_to_end(&mut body).unwrap();
    assert_eq!(body, b"ok");
}

fn get_client(disable_hostname_verification: bool) -> Client {
    let mut client_builder = ClientBuilder::new().unwrap();
    let mut buf = Vec::new();
    File::open("tests/certificates/root.der").unwrap().read_to_end(&mut buf).unwrap();
    let cert = Certificate::from_der(&buf).unwrap();
    client_builder.add_root_certificate(cert).unwrap();
    if disable_hostname_verification {
        client_builder.danger_disable_hostname_verification();
    }
    client_builder.build().unwrap()
}
