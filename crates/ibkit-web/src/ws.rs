//! WebSocket streaming of the IBKR Web API (`wss://<host>/v1/api/ws`).
//!
//! Requests are text frames such as `smd+265598+{"fields":["31","84"]}`
//! (subscribe market data) and `umd+265598+{}` (unsubscribe). Server frames
//! are JSON objects; most carry a `topic` such as `smd+265598`, `sor`, `spl`,
//! `str`, `act`, `sts`, `blt`, `ntf`, `system` or `tic`.

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

/// WebSocket errors.
#[derive(Debug, thiserror::Error)]
pub enum WsError {
    #[error("WebSocket connection to {url} failed: {source}")]
    Connect {
        url: String,
        source: Box<tokio_tungstenite::tungstenite::Error>,
    },
    #[error("the WebSocket is closed")]
    Closed,
    #[error("{0}")]
    Invalid(String),
}

impl WsError {
    pub fn kind(&self) -> ibkit_tws::ErrorKind {
        match self {
            Self::Connect { .. } | Self::Closed => ibkit_tws::ErrorKind::Connection,
            Self::Invalid(_) => ibkit_tws::ErrorKind::InvalidArgument,
        }
    }
}

/// One message from the server.
#[derive(Clone, Debug, PartialEq)]
pub struct WsMessage {
    /// Topic kind, for example `"smd"`, `"sor"`, `"system"`; `None` without a topic.
    pub topic: Option<String>,
    /// Arguments after the topic kind, for example the conid of `smd+265598`.
    pub args: Vec<String>,
    pub data: Value,
}

/// Parse one server frame.
pub fn parse_message(text: &str) -> WsMessage {
    let data: Value =
        serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()));
    let (topic, args) = match data.get("topic").and_then(|t| t.as_str()) {
        Some(t) => {
            let mut parts = t.split('+');
            let kind = parts.next().map(String::from);
            (kind, parts.map(String::from).collect())
        }
        None => (None, Vec::new()),
    };
    WsMessage { topic, args, data }
}

/// Builders for subscription request frames.
pub mod topics {
    use serde_json::json;

    pub fn market_data(conid: i64, fields: &[&str]) -> String {
        format!("smd+{conid}+{}", json!({ "fields": fields }))
    }
    pub fn market_data_unsubscribe(conid: i64) -> String {
        format!("umd+{conid}+{{}}")
    }
    /// `params` may hold exchange, period, bar, outsideRth, source, format.
    pub fn market_history(conid: i64, params: &serde_json::Value) -> String {
        format!("smh+{conid}+{params}")
    }
    pub fn market_history_unsubscribe(server_id: &str) -> String {
        format!("umh+{server_id}")
    }
    /// Live order updates; `filters` are statuses such as `"submitted"`,
    /// `"filled"`, `"cancelled"` or `"sort_by_time"` (none: all orders).
    pub fn orders(filters: &[&str]) -> String {
        if filters.is_empty() {
            "sor+{}".into()
        } else {
            format!("sor+{}", json!({ "filters": filters }))
        }
    }
    pub fn orders_unsubscribe() -> String {
        "uor+{}".into()
    }
    pub fn pnl() -> String {
        "spl+{}".into()
    }
    pub fn pnl_unsubscribe() -> String {
        "upl+{}".into()
    }
    /// `days`: 1 to 7 days of trades; `realtime_updates_only` skips history.
    pub fn trades(realtime_updates_only: bool, days: Option<u8>) -> String {
        let mut v = json!({ "realtimeUpdatesOnly": realtime_updates_only });
        if let Some(d) = days {
            v["days"] = json!(d);
        }
        format!("str+{v}")
    }
    pub fn trades_unsubscribe() -> String {
        "utr".into()
    }
    pub fn account_summary(
        account_id: &str,
        keys: Option<&[&str]>,
        fields: Option<&[&str]>,
    ) -> String {
        format!("ssd+{account_id}+{}", key_fields(keys, fields))
    }
    pub fn account_summary_unsubscribe(account_id: &str) -> String {
        format!("usd+{account_id}+{{}}")
    }
    pub fn account_ledger(
        account_id: &str,
        keys: Option<&[&str]>,
        fields: Option<&[&str]>,
    ) -> String {
        format!("sld+{account_id}+{}", key_fields(keys, fields))
    }
    pub fn account_ledger_unsubscribe(account_id: &str) -> String {
        format!("uld+{account_id}+{{}}")
    }
    pub fn book_trader(account_id: &str, conid: i64, exchange: Option<&str>) -> String {
        match exchange {
            Some(e) => format!("sbd+{account_id}+{conid}+{e}"),
            None => format!("sbd+{account_id}+{conid}"),
        }
    }
    pub fn book_trader_unsubscribe(account_id: &str) -> String {
        format!("ubd+{account_id}")
    }
    /// Start an option exercise or lapse for the option `conid`; the server
    /// answers on `shs+exercise` with the available actions and an `id`.
    pub fn exercise_options(conid: i64) -> String {
        format!("shs+exercise+{}", json!({ "CEX": conid.to_string() }))
    }
    /// Answer an exercise prompt: `user_action` is `"submit"` (with
    /// `quantity` and `make_final`), `"continue"` after a warning, or
    /// `"cancel"`; `id` is the one of the last `shs+exercise`/`inp+exercise`
    /// message.
    pub fn exercise_options_input(
        id: &str,
        user_action: &str,
        quantity: Option<f64>,
        make_final: bool,
    ) -> String {
        let mut data = json!({ "id": id, "user_action": user_action });
        if let Some(qty) = quantity {
            data["exercise"] =
                json!({ "allowed": "not_shown", "make_final": make_final, "value": qty });
        }
        format!(
            "inp+exercise+{}",
            json!({ "action": "user_input", "data": data })
        )
    }
    pub fn heartbeat() -> String {
        "tic".into()
    }

    fn key_fields(keys: Option<&[&str]>, fields: Option<&[&str]>) -> String {
        let mut v = json!({});
        if let Some(k) = keys {
            v["keys"] = json!(k);
        }
        if let Some(f) = fields {
            v["fields"] = json!(f);
        }
        v.to_string()
    }
}

/// How to authenticate the WebSocket and keep it alive.
#[derive(Clone, Debug)]
pub struct WsAuth {
    /// Session id from `POST /tickle` (`session` field). With the gateway it is
    /// sent as the `api` cookie; with OAuth it is sent as `{"session": ..}`.
    pub session: Option<String>,
    /// OAuth 1.0a access token, appended to the URL as `oauth_token`.
    pub oauth_token: Option<String>,
    /// OAuth 2.0 bearer token (from `WebClient::oauth2_init`), appended to
    /// the URL as `bearer_token`.
    pub bearer_token: Option<String>,
    pub accept_invalid_certs: bool,
    /// Interval of `tic` heartbeats; `None` disables them.
    pub heartbeat: Option<Duration>,
    /// Wait this long for the `system` message with `success` before
    /// `connect` returns; a Client Portal Gateway silently drops frames sent
    /// earlier. `None` skips the wait.
    pub confirm_timeout: Option<Duration>,
    /// Re-send every market data subscription this often; IBKR ends a stream
    /// 15 minutes after it was requested. `None` disables renewal.
    pub renew_market_data: Option<Duration>,
}

impl Default for WsAuth {
    fn default() -> Self {
        Self {
            session: None,
            oauth_token: None,
            bearer_token: None,
            accept_invalid_certs: false,
            heartbeat: Some(Duration::from_secs(30)),
            confirm_timeout: Some(Duration::from_secs(10)),
            renew_market_data: Some(Duration::from_secs(600)),
        }
    }
}

/// Text of a data frame; `Ok(None)` for control frames, `Err` when the
/// socket closed.
fn frame_text(
    frame: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
) -> Result<Option<String>, ()> {
    match frame {
        Some(Ok(Message::Text(t))) => Ok(Some(t.to_string())),
        Some(Ok(Message::Binary(b))) => Ok(Some(String::from_utf8_lossy(&b).into_owned())),
        Some(Ok(Message::Close(_)) | Err(_)) | None => Err(()),
        Some(Ok(_)) => Ok(None),
    }
}

/// Connected WebSocket.
pub struct WsClient {
    sender: WsSender,
    rx: mpsc::UnboundedReceiver<WsMessage>,
}

/// Sending half of a [`WsClient`]; cheap to clone.
#[derive(Clone)]
pub struct WsSender {
    tx: mpsc::UnboundedSender<String>,
    /// Market data subscriptions to renew: conid -> fields.
    market_data: Arc<std::sync::Mutex<std::collections::HashMap<i64, Vec<String>>>>,
}

impl std::ops::Deref for WsClient {
    type Target = WsSender;
    fn deref(&self) -> &WsSender {
        &self.sender
    }
}

#[derive(Debug)]
struct NoVerify;

impl rustls::client::danger::ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &[rustls::pki_types::CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

impl WsClient {
    /// Connect to `url` (for example `wss://localhost:5000/v1/api/ws`).
    pub async fn connect(url: &str, auth: WsAuth) -> Result<WsClient, WsError> {
        let credential = match (&auth.oauth_token, &auth.bearer_token) {
            (Some(t), _) => Some(("oauth_token", t)),
            (None, Some(t)) => Some(("bearer_token", t)),
            (None, None) => None,
        };
        let full = match credential {
            Some((name, t)) => format!(
                "{url}{}{name}={t}",
                if url.contains('?') { '&' } else { '?' }
            ),
            None => url.to_string(),
        };
        let mut request = full
            .clone()
            .into_client_request()
            .map_err(|e| WsError::Connect {
                url: full.clone(),
                source: Box::new(e),
            })?;
        if let (Some(s), None) = (&auth.session, &auth.oauth_token) {
            let cookie = format!("api={s}").parse().map_err(|_| {
                WsError::Invalid(format!("session id {s:?} is not a valid cookie value"))
            })?;
            request.headers_mut().insert("Cookie", cookie);
        }
        let connector = if auth.accept_invalid_certs {
            let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .map_err(|e| WsError::Invalid(e.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerify))
            .with_no_client_auth();
            Some(tokio_tungstenite::Connector::Rustls(Arc::new(cfg)))
        } else {
            None
        };
        let (stream, _) =
            tokio_tungstenite::connect_async_tls_with_config(request, None, false, connector)
                .await
                .map_err(|e| WsError::Connect {
                    url: full.clone(),
                    source: Box::new(e),
                })?;
        let (mut sink, mut source) = stream.split();
        if let (Some(s), Some(_)) = (&auth.session, &auth.oauth_token) {
            let hello = Message::Text(json!({ "session": s }).to_string().into());
            sink.send(hello).await.map_err(|e| WsError::Connect {
                url: full.clone(),
                source: Box::new(e),
            })?;
        }
        let (in_tx, rx) = mpsc::unbounded_channel::<WsMessage>();
        if let Some(limit) = auth.confirm_timeout {
            let confirm = async {
                loop {
                    let Some(text) =
                        frame_text(source.next().await).map_err(|_| WsError::Closed)?
                    else {
                        continue;
                    };
                    let m = parse_message(&text);
                    let ok =
                        m.topic.as_deref() == Some("system") && m.data.get("success").is_some();
                    let _ = in_tx.send(m);
                    if ok {
                        return Ok(());
                    }
                }
            };
            tokio::time::timeout(limit, confirm).await.map_err(|_| {
                WsError::Invalid(format!(
                    "the WebSocket session was not confirmed within {limit:?}; check that the brokerage session is authenticated (POST /iserver/auth/status) and that the session id or token is current"
                ))
            })??;
        }
        let (tx, mut out_rx) = mpsc::unbounded_channel::<String>();
        let market_data: Arc<std::sync::Mutex<std::collections::HashMap<i64, Vec<String>>>> =
            Arc::default();
        let subs = market_data.clone();
        let interval = |period: Option<Duration>| {
            period.map(|p| tokio::time::interval_at(tokio::time::Instant::now() + p, p))
        };
        let (mut heartbeat, mut renew) =
            (interval(auth.heartbeat), interval(auth.renew_market_data));
        // The only writer: it also sends the heartbeats and renewals, so the
        // socket closes once every WsClient and WsSender is dropped.
        tokio::spawn(async move {
            loop {
                let frames = tokio::select! {
                    text = out_rx.recv() => match text {
                        Some(text) => vec![text],
                        None => break,
                    },
                    _ = async { heartbeat.as_mut().unwrap().tick().await }, if heartbeat.is_some() => {
                        vec![topics::heartbeat()]
                    }
                    _ = async { renew.as_mut().unwrap().tick().await }, if renew.is_some() => {
                        let subs = subs.lock().expect("market data lock");
                        subs.iter()
                            .map(|(c, f)| {
                                topics::market_data(*c, &f.iter().map(String::as_str).collect::<Vec<_>>())
                            })
                            .collect()
                    }
                };
                for frame in frames {
                    if sink.send(Message::Text(frame.into())).await.is_err() {
                        return;
                    }
                }
            }
            let _ = sink.close().await;
        });
        tokio::spawn(async move {
            loop {
                match frame_text(source.next().await) {
                    Ok(Some(text)) => {
                        if in_tx.send(parse_message(&text)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(()) => break,
                }
            }
        });
        Ok(WsClient {
            sender: WsSender { tx, market_data },
            rx,
        })
    }

    /// Next server message; `None` after the socket closed.
    pub async fn recv(&mut self) -> Option<WsMessage> {
        self.rx.recv().await
    }

    /// A handle for sending while another task receives.
    pub fn sender(&self) -> WsSender {
        self.sender.clone()
    }
}

impl WsSender {
    /// Send one raw request frame.
    pub fn send(&self, text: impl Into<String>) -> Result<(), WsError> {
        self.tx.send(text.into()).map_err(|_| WsError::Closed)
    }

    /// Subscribe to market data; the subscription is renewed every
    /// `WsAuth::renew_market_data` until unsubscribed.
    pub fn subscribe_market_data(&self, conid: i64, fields: &[&str]) -> Result<(), WsError> {
        self.market_data
            .lock()
            .expect("market data lock")
            .insert(conid, fields.iter().map(|f| f.to_string()).collect());
        self.send(topics::market_data(conid, fields))
    }
    pub fn unsubscribe_market_data(&self, conid: i64) -> Result<(), WsError> {
        self.market_data
            .lock()
            .expect("market data lock")
            .remove(&conid);
        self.send(topics::market_data_unsubscribe(conid))
    }
    pub fn subscribe_orders(&self, filters: &[&str]) -> Result<(), WsError> {
        self.send(topics::orders(filters))
    }
    pub fn subscribe_pnl(&self) -> Result<(), WsError> {
        self.send(topics::pnl())
    }
    pub fn subscribe_trades(
        &self,
        realtime_updates_only: bool,
        days: Option<u8>,
    ) -> Result<(), WsError> {
        self.send(topics::trades(realtime_updates_only, days))
    }
    pub fn subscribe_account_summary(&self, account_id: &str) -> Result<(), WsError> {
        self.send(topics::account_summary(account_id, None, None))
    }
    pub fn subscribe_account_ledger(&self, account_id: &str) -> Result<(), WsError> {
        self.send(topics::account_ledger(account_id, None, None))
    }
    pub fn exercise_options(&self, conid: i64) -> Result<(), WsError> {
        self.send(topics::exercise_options(conid))
    }
    pub fn subscribe_book_trader(
        &self,
        account_id: &str,
        conid: i64,
        exchange: Option<&str>,
    ) -> Result<(), WsError> {
        self.send(topics::book_trader(account_id, conid, exchange))
    }
}
