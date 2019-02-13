//! A server builder helper for the integration tests.

use std::io::{Read, Write};
use std::net;
use std::time::Duration;
use std::sync::mpsc;
use std::thread;

pub struct Server {
    addr: net::SocketAddr,
    panic_rx: mpsc::Receiver<()>,
}

impl Server {
    pub fn addr(&self) -> net::SocketAddr {
        self.addr
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if !::std::thread::panicking() {
            self
                .panic_rx
                .recv_timeout(Duration::from_secs(3))
                .expect("test server should not panic");
        }
    }
}

#[derive(Debug, Default)]
pub struct Txn {
    pub request: Vec<u8>,
    pub response: Vec<u8>,

    pub read_timeout: Option<Duration>,
    pub read_closes: bool,
    pub response_timeout: Option<Duration>,
    pub write_timeout: Option<Duration>,
    pub chunk_size: Option<usize>,
}

static DEFAULT_USER_AGENT: &'static str =
    concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

pub fn spawn(txns: Vec<Txn>) -> Server {
    let listener = net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (panic_tx, panic_rx) = mpsc::channel();
    let tname = format!("test({})-support-server", thread::current().name().unwrap_or("<unknown>"));
    thread::Builder::new().name(tname).spawn(move || {
        'txns: for txn in txns {
            let mut expected = txn.request;
            let reply = txn.response;
            let (mut socket, _addr) = listener.accept().unwrap();

            socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();

            replace_expected_vars(&mut expected, addr.to_string().as_ref(), DEFAULT_USER_AGENT.as_ref());

            if let Some(dur) = txn.read_timeout {
                thread::park_timeout(dur);
            }

            let mut buf = vec![0; expected.len() + 256];

            let mut n = 0;
            while n < expected.len() {
                match socket.read(&mut buf[n..]) {
                    Ok(0) => {
                        if !txn.read_closes {
                            panic!("server unexpected socket closed");
                        } else {
                            continue 'txns;
                        }
                    },
                    Ok(nread) => n += nread,
                    Err(err) => {
                        println!("server read error: {}", err);
                        break;
                    }
                }
            }

            if txn.read_closes {
                socket.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                match socket.read(&mut [0; 256]) {
                    Ok(0) => {
                        continue 'txns
                    },
                    Ok(_) => {
                        panic!("server read expected EOF, found more bytes");
                    },
                    Err(err) => {
                        panic!("server read expected EOF, got error: {}", err);
                    }
                }
            }

            match (::std::str::from_utf8(&expected), ::std::str::from_utf8(&buf[..n])) {
                (Ok(expected), Ok(received)) => {
                    if expected.len() > 300 && ::std::env::var("REQWEST_TEST_BODY_FULL").is_err() {
                        assert_eq!(
                            expected.len(),
                            received.len(),
                            "expected len = {}, received len = {}; to skip length check and see exact contents, re-run with REQWEST_TEST_BODY_FULL=1",
                            expected.len(),
                            received.len(),
                        );
                    }
                    assert_eq!(expected, received)
                },
                _ => {
                    assert_eq!(
                        expected.len(),
                        n,
                        "expected len = {}, received len = {}",
                        expected.len(),
                        n,
                    );
                    assert_eq!(expected, &buf[..n])
                },
            }

            if let Some(dur) = txn.response_timeout {
                thread::park_timeout(dur);
            }

            if let Some(dur) = txn.write_timeout {
                let headers_end = b"\r\n\r\n";
                let headers_end = reply.windows(headers_end.len()).position(|w| w == headers_end).unwrap() + 4;
                socket.write_all(&reply[..headers_end]).unwrap();

                let body = &reply[headers_end..];

                if let Some(chunk_size) = txn.chunk_size {
                    for content in body.chunks(chunk_size) {
                        thread::park_timeout(dur);
                        socket.write_all(&content).unwrap();
                    }
                } else {
                    thread::park_timeout(dur);
                    socket.write_all(&body).unwrap();
                }
            } else {
                socket.write_all(&reply).unwrap();
            }
        }
        let _ = panic_tx.send(());
    }).expect("server thread spawn");

    Server {
        addr,
        panic_rx,
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
    ($($($f:ident: $v:expr),+);*) => ({
        let txns = vec![
            $(__internal__txn! {
                $($f: $v,)+
            }),*
        ];
        ::support::server::spawn(txns)
    })
}

#[macro_export]
macro_rules! __internal__txn {
    ($($field:ident: $val:expr,)+) => (
        ::support::server::Txn {
            $( $field: __internal__prop!($field: $val), )+
            .. Default::default()
        }
    )
}


#[macro_export]
macro_rules! __internal__prop {
    (request: $val:expr) => (
        From::from(&$val[..])
    );
    (response: $val:expr) => (
        From::from(&$val[..])
    );
    ($field:ident: $val:expr) => (
        From::from($val)
    )
}
