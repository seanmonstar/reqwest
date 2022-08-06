use bytes::Bytes;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;

use crate::error::{BoxError, Error, Kind};
use bytes::Buf;
use h3::client::SendRequest;
use http::uri::{Authority, Scheme};
use http::{Request, Response, Uri};
use hyper::Body;
use log::debug;

pub(super) type Key = (Scheme, Authority);

#[derive(Clone)]
pub struct Pool {
    inner: Arc<Mutex<PoolInner>>,
}

impl Pool {
    pub fn new(max_idle_per_host: usize, timeout: Option<Duration>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(PoolInner {
                idle: HashMap::new(),
                max_idle_per_host,
                timeout,
            })),
        }
    }

    pub fn put(&self, key: Key, client: PoolClient) {
        let mut inner = self.inner.lock().unwrap();
        inner.put(key, client)
    }

    pub fn try_pool(&self, key: &Key) -> Option<PoolClient> {
        let mut inner = self.inner.lock().unwrap();
        let timeout = inner.timeout;
        inner.idle.get_mut(&key).and_then(|list| match list.pop() {
            Some(idle) => {
                if let Some(duration) = timeout {
                    if Instant::now().saturating_duration_since(idle.idle_at) > duration {
                        debug!("pooled client expired");
                        return None;
                    }
                }
                Some(idle.value)
            }
            None => None,
        })
    }
}

struct PoolInner {
    // These are internal Conns sitting in the event loop in the KeepAlive
    // state, waiting to receive a new Request to send on the socket.
    idle: HashMap<Key, Vec<Idle>>,
    max_idle_per_host: usize,
    timeout: Option<Duration>,
}

impl PoolInner {
    fn put(&mut self, key: Key, client: PoolClient) {
        if self.idle.contains_key(&key) {
            debug!("connection already exists for key {:?}", key);
            return;
        }

        let idle_list = self.idle.entry(key.clone()).or_default();

        if idle_list.len() >= self.max_idle_per_host {
            debug!("max idle per host for {:?}, dropping connection", key);
            return;
        }

        idle_list.push(Idle {
            idle_at: Instant::now(),
            value: client,
        });
    }
}

#[derive(Clone)]
pub struct PoolClient {
    tx: SendRequest<h3_quinn::OpenStreams, Bytes>,
}

impl PoolClient {
    pub fn new(tx: SendRequest<h3_quinn::OpenStreams, Bytes>) -> Self {
        Self { tx }
    }

    // TODO: add support for sending data.
    pub async fn send_request(&mut self, req: Request<()>) -> Result<Response<Body>, BoxError> {
        let mut stream = self.tx.send_request(req).await?;
        stream.finish().await?;

        let resp = stream.recv_response().await?;

        let mut body = Vec::new();
        while let Some(chunk) = stream.recv_data().await? {
            body.extend(chunk.chunk())
        }

        Ok(resp.map(|_| Body::from(body)))
    }
}

struct Idle {
    idle_at: Instant,
    value: PoolClient,
}

pub(crate) fn extract_domain(uri: &mut Uri) -> Result<Key, Error> {
    let uri_clone = uri.clone();
    match (uri_clone.scheme(), uri_clone.authority()) {
        (Some(scheme), Some(auth)) => Ok((scheme.clone(), auth.clone())),
        _ => Err(Error::new(Kind::Request, None::<Error>)),
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
