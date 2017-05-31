//! A server builder helper for the integration tests.

use std::io::{Read, Write};
use std::net;
use std::thread;

pub struct Server {
    addr: net::SocketAddr,
}

impl Server {
    pub fn addr(&self) -> net::SocketAddr {
        self.addr
    }
}

static DEFAULT_USER_AGENT: &'static str =
    concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

pub fn spawn(txns: Vec<(Vec<u8>, Vec<u8>)>) -> Server {
    let listener = net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for (mut expected, reply) in txns {
            let (mut socket, _addr) = listener.accept().unwrap();
            replace_expected_vars(&mut expected, addr.to_string().as_ref(), DEFAULT_USER_AGENT.as_ref());
            let mut buf = [0; 4096];
            let n = socket.read(&mut buf).unwrap();

            match (::std::str::from_utf8(&expected), ::std::str::from_utf8(&buf[..n])) {
                (Ok(expected), Ok(received)) => assert_eq!(expected, received),
                _ => assert_eq!(expected, &buf[..n]),
            }
            socket.write_all(&reply).unwrap();
        }
    });

    Server {
        addr: addr,
    }
}

fn replace_expected_vars(bytes: &mut Vec<u8>, host: &[u8], ua: &[u8]) {
    // plenty horrible, but these are just tests, and gets the job done
    let mut index = 0;
    loop {
        if index == bytes.len() {
            return;
        }

        for b in (&bytes[index..]).iter() {
            index += 1;
            if *b == b'$' {
                break;
            }
        }

        let has_host = (&bytes[index..]).starts_with(b"HOST");
        if has_host {
            bytes.drain(index - 1..index + 4);
            for (i, b) in host.iter().enumerate() {
                bytes.insert(index - 1 + i, *b);
            }
        } else {
            let has_ua = (&bytes[index..]).starts_with(b"USERAGENT");
            if has_ua {
                bytes.drain(index - 1..index + 9);
                for (i, b) in ua.iter().enumerate() {
                    bytes.insert(index - 1 + i, *b);
                }
            }
        }
    }
}

#[macro_export]
macro_rules! server {
    ($(request: $req:expr, response: $res:expr),*) => ({
        let txns = vec![
            $(((&$req[..]).into(), (&$res[..]).into()),)*
        ];
        ::server::spawn(txns)
    })
}
