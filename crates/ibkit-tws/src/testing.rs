//! An in-process fake TWS server that speaks the real wire protocol.
//!
//! It accepts connections on
//! `127.0.0.1:<random port>`, performs the server side of the handshake,
//! answers START_API with `nextValidId` and `managedAccounts`, records every
//! request, and answers requests through a user supplied responder.
//!
//! From server version 201 frames carry a raw 4-byte message id. A protobuf
//! message is represented as the fields `[id, "protobuf", <payload as hex>]`,
//! both for recorded requests and for responses; [`protobuf`] builds one.

#![allow(clippy::useless_vec)]

use crate::wire::{FieldReader, FrameBuffer};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};

/// Build a field list from displayable values: `tws_msg![9, 1, 1000]`.
#[macro_export]
macro_rules! tws_msg {
    ($($x:expr),* $(,)?) => { vec![$($x.to_string()),*] };
}

/// Maps one request (its fields, message id first) to the frames to send back.
pub type Responder = Arc<dyn Fn(&[String]) -> Vec<Vec<String>> + Send + Sync>;

/// Fields of a protobuf message: `[id, "protobuf", hex]`.
pub fn protobuf(msg_id: i32, build: impl FnOnce(&mut crate::pb::PbWriter)) -> Vec<String> {
    let mut w = crate::pb::PbWriter::default();
    build(&mut w);
    vec![msg_id.to_string(), "protobuf".into(), hex(&w.buf)]
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .filter_map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok())
        .collect()
}

/// Encode fields as a frame for a server of `server_version`.
pub fn frame_for(server_version: i32, fields: &[String]) -> Vec<u8> {
    if server_version < crate::server_versions::PROTOBUF {
        return frame(fields);
    }
    let id: i32 = fields.first().and_then(|f| f.parse().ok()).unwrap_or(0);
    let mut payload = Vec::new();
    if fields.get(1).map(String::as_str) == Some("protobuf") {
        payload.extend_from_slice(&(id + crate::message::PROTOBUF_MSG_ID).to_be_bytes());
        payload.extend(unhex(fields.get(2).map_or("", String::as_str)));
    } else {
        payload.extend_from_slice(&id.to_be_bytes());
        for f in &fields[1.min(fields.len())..] {
            payload.extend_from_slice(f.as_bytes());
            payload.push(0);
        }
    }
    let mut out = (payload.len() as u32).to_be_bytes().to_vec();
    out.extend(payload);
    out
}

/// Fields of a client frame from a server of `server_version`.
fn request_fields(server_version: i32, payload: &[u8]) -> Vec<String> {
    let (mut fields, rest) = match payload.split_first_chunk::<4>() {
        Some((head, rest)) if server_version >= crate::server_versions::PROTOBUF => {
            let id = i32::from_be_bytes(*head);
            if id > crate::message::PROTOBUF_MSG_ID {
                let id = id - crate::message::PROTOBUF_MSG_ID;
                return vec![id.to_string(), "protobuf".into(), hex(rest)];
            }
            (vec![id.to_string()], rest)
        }
        _ => (Vec::new(), payload),
    };
    let mut r = FieldReader::new(rest);
    while !r.is_empty() {
        match r.string() {
            Ok(f) => fields.push(f),
            Err(_) => break,
        }
    }
    fields
}

/// Encode fields into one length-prefixed frame.
pub fn frame(fields: &[String]) -> Vec<u8> {
    let mut payload = Vec::new();
    for f in fields {
        payload.extend_from_slice(f.as_bytes());
        payload.push(0);
    }
    let mut out = (payload.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&payload);
    out
}

/// Settings of the fake server.
#[derive(Clone, Debug)]
pub struct FakeTwsConfig {
    pub server_version: i32,
    pub next_valid_id: i32,
    pub accounts: String,
}

impl Default for FakeTwsConfig {
    fn default() -> Self {
        Self {
            server_version: 178,
            next_valid_id: 1,
            accounts: "DU123456".into(),
        }
    }
}

/// Handle to a running fake server.
pub struct FakeTws {
    pub addr: SocketAddr,
    requests: mpsc::UnboundedReceiver<Vec<String>>,
    push: broadcast::Sender<Vec<u8>>,
    server_version: i32,
}

impl FakeTws {
    /// Start a server with default settings and no automatic responses.
    pub async fn start() -> FakeTws {
        Self::with(FakeTwsConfig::default(), Arc::new(|_| Vec::new())).await
    }

    /// Start a server with the given settings and responder.
    pub async fn with(config: FakeTwsConfig, responder: Responder) -> FakeTws {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake TWS");
        let addr = listener.local_addr().expect("fake TWS address");
        let (req_tx, requests) = mpsc::unbounded_channel();
        let (push, _) = broadcast::channel::<Vec<u8>>(4096);
        let push2 = push.clone();
        let server_version = config.server_version;
        tokio::spawn(async move {
            while let Ok((sock, _)) = listener.accept().await {
                let config = config.clone();
                let responder = responder.clone();
                let req_tx = req_tx.clone();
                let mut push_rx = push2.subscribe();
                tokio::spawn(async move {
                    let _ = sock.set_nodelay(true);
                    let (mut rd, mut wr) = sock.into_split();
                    let mut greeting = [0u8; 4];
                    if rd.read_exact(&mut greeting).await.is_err() || &greeting != b"API\0" {
                        return;
                    }
                    let mut fb = FrameBuffer::new();
                    let mut buf = vec![0u8; 65536];
                    let mut started = false;
                    loop {
                        // Drain complete frames.
                        while let Ok(Some(p)) = fb.next_frame() {
                            let payload = p.to_vec();
                            if !started && payload.starts_with(b"v") {
                                let hello = frame(&crate::tws_msg![
                                    config.server_version,
                                    "20260108 12:00:00 EST"
                                ]);
                                if wr.write_all(&hello).await.is_err() {
                                    return;
                                }
                                continue;
                            }
                            let v = config.server_version;
                            let fields = request_fields(v, &payload);
                            let mut out = Vec::new();
                            if !started && fields.first().map(String::as_str) == Some("71") {
                                started = true;
                                out.extend(frame_for(
                                    v,
                                    &crate::tws_msg![9, 1, config.next_valid_id],
                                ));
                                out.extend(frame_for(v, &crate::tws_msg![15, 1, config.accounts]));
                            } else {
                                for resp in responder(&fields) {
                                    out.extend(frame_for(v, &resp));
                                }
                            }
                            let _ = req_tx.send(fields);
                            if !out.is_empty() && wr.write_all(&out).await.is_err() {
                                return;
                            }
                        }
                        tokio::select! {
                            n = rd.read(&mut buf) => match n {
                                Ok(0) | Err(_) => return,
                                Ok(n) => fb.extend(&buf[..n]),
                            },
                            pushed = push_rx.recv() => match pushed {
                                Ok(bytes) => if wr.write_all(&bytes).await.is_err() { return },
                                Err(broadcast::error::RecvError::Lagged(_)) => {}
                                Err(_) => return,
                            },
                        }
                    }
                });
            }
        });
        FakeTws {
            addr,
            requests,
            push,
            server_version,
        }
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Send one message to every connected client.
    pub fn send(&self, fields: Vec<String>) {
        let _ = self.push.send(frame_for(self.server_version, &fields));
    }

    /// Send raw bytes to every connected client.
    pub fn send_raw(&self, bytes: Vec<u8>) {
        let _ = self.push.send(bytes);
    }

    /// Next request received from a client (START_API included).
    pub async fn next_request(&mut self) -> Vec<String> {
        tokio::time::timeout(std::time::Duration::from_secs(5), self.requests.recv())
            .await
            .expect("fake TWS waited 5 s for a request")
            .expect("fake TWS request channel closed")
    }

    /// Next request whose message id is `msg_id`, skipping others.
    pub async fn next_request_with_id(&mut self, msg_id: i32) -> Vec<String> {
        let id = msg_id.to_string();
        loop {
            let r = self.next_request().await;
            if r.first() == Some(&id) {
                return r;
            }
        }
    }
}
