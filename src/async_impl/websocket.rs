use std::sync::{Arc, LazyLock};

use base64::Engine;
use http::{HeaderMap, HeaderValue, StatusCode};

use rand::random;
use sha1::Digest;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{Mutex, RwLock},
};
use url::Url;

use crate::{Client, Error, Result, Upgraded};

pub struct WebSocket {
    socket: Upgraded,
    send_lock: Arc<Mutex<i32>>,
    read_lock: Arc<Mutex<i32>>,
    buf: Arc<RwLock<Vec<u8>>>,
}
pub enum WebSocketMessage {
    Byte(Vec<u8>),
    TEXT(String),
}
const HANDSHAKE_HEADERS: LazyLock<HeaderMap> = LazyLock::new(|| {
    let mut header_map = HeaderMap::new();
    header_map.append("upgrade", HeaderValue::from_str("websocket").unwrap());
    header_map.append("connection", HeaderValue::from_str("Upgrade").unwrap());
    header_map.append(
        "sec-websocket-version",
        HeaderValue::from_str("13").unwrap(),
    );
    header_map
});
const CONST_GUID: &'static str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
struct WebSocketFrame {
    data: Vec<u8>,
}
impl WebSocket {
    pub async fn connect(url: Url, client: Client) -> Result<WebSocket> {
        let mut key = [0u8; 16];
        rand::fill(&mut key);
        let sec_key = base64::engine::general_purpose::STANDARD.encode(key);
        let header_map =
            Self::insert_headermap_sec_key(&sec_key, (*HANDSHAKE_HEADERS).clone()).await;
        if url.scheme().eq("ws") || url.scheme().eq("wss") {
            if let Ok(connect_res) = client.get(url).headers(header_map).send().await {
                if connect_res.status().eq(&StatusCode::SWITCHING_PROTOCOLS) {
                    if let Some(v) = connect_res.headers().get("sec-websocket-accept") {
                        if let Ok(()) = Self::check_key_accept(sec_key, v.to_str().unwrap()).await {
                            if let Ok(upgrade) = connect_res.upgrade().await {
                                eprintln!("upgrade success");
                                Ok(Self {
                                    send_lock: Arc::new(Mutex::new(0)),
                                    read_lock: Arc::new(Mutex::new(1)),
                                    socket: upgrade,
                                    buf: Arc::new(RwLock::new(vec![0u8; 1024])),
                                })
                            } else {
                                todo!();
                            }
                        } else {
                            todo!();
                        }
                    } else {
                        todo!();
                    }
                } else {
                    todo!();
                }
            } else {
                todo!();
            }
        } else {
            todo!();
        }
    }
    async fn insert_headermap_sec_key(sec_key: &str, mut header_map: HeaderMap) -> HeaderMap {
        header_map.append(
            "sec-websocket-key",
            HeaderValue::from_str(&sec_key).unwrap(),
        );
        header_map
    }
    async fn check_key_accept(mut sec_key: String, acc_key: &str) -> Result<()> {
        sec_key.push_str(CONST_GUID);
        let mut hasher = sha1::Sha1::new();
        hasher.update(sec_key.as_bytes());
        let finalize = hasher.finalize();
        let result_key = base64::engine::general_purpose::STANDARD.encode(finalize);
        if acc_key.eq(&result_key) {
            Ok(())
        } else {
            todo!();
        }
    }
    async fn make_websocket_frames_bin(&self, mut v: Vec<u8>) -> Vec<WebSocketFrame> {
        let mut res = vec![];
        loop {
            let mut tmp = vec![];
            if v.len() <= 125 {
                tmp.push(0b10000000 | 2u8);
                tmp.push(0b10000000 | v.len() as u8);
                let r1 = random();
                let r2 = random();
                let r3 = random();
                let r4 = random();
                tmp.push(r1);
                tmp.push(r2);
                tmp.push(r3);
                tmp.push(r4);
                for i in (&v).iter().enumerate() {
                    if i.0 % 4 == 0 {
                        tmp.push(*i.1 ^ r1);
                    } else if i.0 % 4 == 1 {
                        tmp.push(*i.1 ^ r2);
                    } else if i.0 % 4 == 2 {
                        tmp.push(*i.1 ^ r3);
                    } else if i.0 % 4 == 3 {
                        tmp.push(*i.1 ^ r4);
                    }
                }
                res.push(WebSocketFrame { data: tmp });
                break;
            } else {
                tmp.push(0b00000000 | 2u8);
                tmp.push(0b10000000 | 125u8);
                let r1 = random();
                let r2 = random();
                let r3 = random();
                let r4 = random();
                tmp.push(r1);
                tmp.push(r2);
                tmp.push(r3);
                tmp.push(r4);
                for i in (&v[0..125]).iter().enumerate() {
                    if i.0 % 4 == 0 {
                        tmp.push(*i.1 ^ r1);
                    } else if i.0 % 4 == 1 {
                        tmp.push(*i.1 ^ r2);
                    } else if i.0 % 4 == 2 {
                        tmp.push(*i.1 ^ r3);
                    } else if i.0 % 4 == 3 {
                        tmp.push(*i.1 ^ r4);
                    }
                }
                v = v[125..v.len()].to_vec();
                res.push(WebSocketFrame { data: tmp });
            }
        }
        res
    }
    async fn make_websocket_frames_text(&self, s: String) -> Vec<WebSocketFrame> {
        let mut res = self.make_websocket_frames_bin(s.as_bytes().to_vec()).await;
        for i in &mut res {
            i.data[0] = i.data[0] & 0b11110000 | 1u8;
        }
        res
    }
}
pub trait WebSocketTrait {
    async fn send_msg(&mut self, message: WebSocketMessage) -> Result<()>;
    async fn receive_msg(&mut self) -> Result<WebSocketMessage>;
}
impl WebSocketTrait for WebSocket {
    async fn send_msg(&mut self, message: WebSocketMessage) -> Result<()> {
        let _s_lk = self.send_lock.lock().await;

        if let WebSocketMessage::Byte(bytes) = message {
            for i in &(self.make_websocket_frames_bin(bytes).await) {
                let so = &mut self.socket;
                if let Ok(()) = so.write_all(&i.data).await {
                } else {
                    todo!();
                }
            }
            Ok(())
        } else if let WebSocketMessage::TEXT(s) = message {
            for i in &self.make_websocket_frames_text(s).await {
                let so = &mut self.socket;
                if let Ok(()) = so.write_all(&i.data).await {
                } else {
                    todo!();
                }
            }
            Ok(())
        } else {
            Err(Error::new::<String>(crate::error::Kind::Upgrade, None))
        }
    }

    async fn receive_msg(&mut self) -> Result<WebSocketMessage> {
        let _r_lock = self.read_lock.lock().await;
        let so = &mut self.socket;
        let mut buf = self.buf.write().await;
        let mut res = vec![];
        loop {
            if let Ok(_size) = so.read_exact(&mut buf[0..2]).await {
                let first_byte_fin = buf[0];
                let payload_len = buf[1];
                if let Ok(_size) = so.read_exact(&mut buf[0..payload_len as usize]).await {
                    res.extend(&buf[0..payload_len as usize]);
                    if first_byte_fin >> 7 == 1 {
                        break;
                    }
                } else {
                    todo!();
                }
            } else {
                todo!();
            }
        }
        Ok(WebSocketMessage::Byte(res))
    }
}
pub trait WebSocketExt {
    async fn to_websocket(self, url: Url) -> Result<WebSocket>;
}
impl WebSocketExt for Client {
    async fn to_websocket(self, url: Url) -> Result<WebSocket> {
        WebSocket::connect(url, self).await
    }
}
#[cfg(test)]
mod test {
    #[tokio::test]
    async fn test_websocket() {
        use crate::async_impl::websocket::WebSocketTrait;
        use std::str::FromStr;
        use url::Url;
        let client = crate::Client::new();
        if let Ok(mut websocket) = crate::async_impl::websocket::WebSocketExt::to_websocket(
            client,
            Url::from_str("ws://xxxx/xxxx/xxxx").unwrap(),
        )
        .await
        {
            websocket.send_msg( crate::async_impl::websocket::WebSocketMessage::TEXT("{\"senderId\":\"jkl\",\"senderName\":\"sdsd\",\"receiverType\":\"server\",\"msg\":\"hello\"}".to_string(),))
            .await
            .unwrap();
            websocket.send_msg( crate::async_impl::websocket::WebSocketMessage::TEXT("{\"senderId\":\"jkl\",\"senderName\":\"sdsd\",\"receiverType\":\"server\",\"msg\":\"hello world!\"}".to_string(),))
            .await
            .unwrap();
            // sleep(Duration::from_millis(200)).await;
            if let Ok(receive_msg) =
                crate::async_impl::websocket::WebSocketTrait::receive_msg(&mut websocket).await
            {
                if let crate::async_impl::websocket::WebSocketMessage::Byte(bytes) = receive_msg {
                    eprintln!("{:?}", String::from_utf8(bytes).unwrap());
                }
            }
            if let Ok(receive_msg) =
                crate::async_impl::websocket::WebSocketTrait::receive_msg(&mut websocket).await
            {
                if let crate::async_impl::websocket::WebSocketMessage::Byte(bytes) = receive_msg {
                    eprintln!("{:?}", String::from_utf8(bytes).unwrap());
                }
            }
            let mut buf = String::new();
            std::io::stdin().read_line(&mut buf).unwrap();
        }
    }
}
