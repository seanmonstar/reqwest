#![cfg(not(target_arch = "wasm32"))]
#![cfg(not(feature = "rustls-tls-manual-roots-no-provider"))]
mod support;

use std::sync::{Arc, Mutex};

use bytes::Bytes;
use support::server;

use reqwest::{
    hooks::{RequestHook, ResponseBodyHook, ResponseHook},
    Client,
};

#[derive(Default)]
struct MyHook {
    pub req_visited: Arc<Mutex<bool>>,
    pub res_visited: Arc<Mutex<bool>>,
    pub body_visited: Arc<Mutex<bool>>,
}

impl RequestHook for MyHook {
    fn intercept(&self, req: reqwest::Request) -> reqwest::Request {
        *self.req_visited.lock().unwrap() = true;
        req
    }
}

impl ResponseHook for MyHook {
    fn intercept(&self, res: reqwest::Response) -> reqwest::Response {
        *self.res_visited.lock().unwrap() = true;
        res
    }
}

impl ResponseBodyHook for MyHook {
    fn intercept(&self, body: Bytes) -> Bytes {
        *self.body_visited.lock().unwrap() = true;
        body
    }
}

#[tokio::test]
async fn full_hook_chain() {
    let _ = env_logger::try_init();

    let server = server::http(move |_req| async { http::Response::new("Hello".into()) });

    let hook = Arc::new(MyHook::default());

    let client = Client::builder()
        .request_hook(hook.clone())
        .response_hook(hook.clone())
        .response_body_hook(hook.clone())
        .build()
        .unwrap();

    let res = client
        .get(&format!("http://{}/text", server.addr()))
        .send()
        .await
        .expect("Failed to get");
    assert_eq!(res.content_length(), Some(5));
    let text = res.text().await.expect("Failed to get text");
    assert_eq!("Hello", text);
    assert!(*hook.req_visited.lock().unwrap());
    assert!(*hook.res_visited.lock().unwrap());
    assert!(*hook.body_visited.lock().unwrap());
}
