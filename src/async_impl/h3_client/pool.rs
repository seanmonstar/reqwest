use std::collections::{HashMap, HashSet};
use std::mem;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use bytes::Bytes;
use tokio::time::Instant;

use h3::client::SendRequest;
use http::{Request, Response, Uri};
use http::uri::{Authority, Scheme};
use hyper::Body;
use crate::error::{Kind, Error};
use bytes::Buf;

pub(super) type Key = (Scheme, Authority); //Arc<String>;

struct Pool {
    inner: Arc<Mutex<PoolInner>>
}

impl Pool {
    fn connecting(&self, key: Key) -> bool {
        let mut inner = self.inner.lock().unwrap();
        inner.connecting.insert(key)
    }
}

struct PoolInner {
    // A flag that a connection is being established, and the connection
    // should be shared. This prevents making multiple HTTP/2 connections
    // to the same host.
    connecting: HashSet<Key>,
    // These are internal Conns sitting in the event loop in the KeepAlive
    // state, waiting to receive a new Request to send on the socket.
    idle: HashMap<Key, Vec<Idle>>,
    max_idle_per_host: usize,
    timeout: Option<Duration>,
}

impl PoolInner {
    fn put(&mut self, key: Key, client: PoolClient) {
        if self.idle.contains_key(&key) {
            eprintln!("connection alread exists for key {:?}", key);
            return;
        }

        let idle_list = self.idle.entry(key).or_default();
        idle_list.push(Idle {
            idle_at: Instant::now(),
            value: client
        });
    }
}

pub(crate) struct PoolClient {
    tx: SendRequest<h3_quinn::OpenStreams, Bytes>
}

impl PoolClient {
    pub fn new(tx: SendRequest<h3_quinn::OpenStreams, Bytes>) -> Self {
        Self {
            tx
        }
    }

    pub async fn send_request(&mut self, req: Request<()>) -> Result<Response<Body>, Error> {
        let mut stream = self.tx.send_request(req).await.unwrap();
        stream.finish().await.unwrap();

        eprintln!("Receiving response ...");
        let resp = stream.recv_response().await.unwrap();
        eprintln!("Response h3 {:?}", resp);

        while let Some(chunk) = stream.recv_data().await.unwrap() {
            eprintln!("Chunk: {:?}", chunk.chunk());
        }

        Ok(resp.map(|_| {
            Body::empty()
        }))
    }
}

struct Idle {
    idle_at: Instant,
    value: PoolClient,
}


fn set_scheme(uri: &mut Uri, scheme: Scheme) {
    debug_assert!(
        uri.scheme().is_none(),
        "set_scheme expects no existing scheme"
    );
    let old = mem::replace(uri, Uri::default());
    let mut parts: ::http::uri::Parts = old.into();
    parts.scheme = Some(scheme);
    parts.path_and_query = Some("/".parse().expect("slash is a valid path"));
    *uri = Uri::from_parts(parts).expect("scheme is valid");
}

pub(crate) fn extract_domain(uri: &mut Uri, is_http_connect: bool) -> Result<Key, Error> {
    let uri_clone = uri.clone();
    match (uri_clone.scheme(), uri_clone.authority()) {
        (Some(scheme), Some(auth)) => Ok((scheme.clone(), auth.clone())),
        (None, Some(auth)) if is_http_connect => {
            let scheme = match auth.port_u16() {
                Some(443) => {
                    set_scheme(uri, Scheme::HTTPS);
                    Scheme::HTTPS
                }
                _ => {
                    set_scheme(uri, Scheme::HTTP);
                    Scheme::HTTP
                }
            };
            Ok((scheme, auth.clone()))
        }
        _ => {
            Err(crate::Error::new(Kind::Request, None::<crate::Error>))
        }
    }
}

pub(crate) fn domain_as_uri((scheme, auth): Key) -> Uri {
    http::uri::Builder::new()
        .scheme(scheme)
        .authority(auth)
        .path_and_query("/")
        .build()
        .expect("domain is valid Uri")
}