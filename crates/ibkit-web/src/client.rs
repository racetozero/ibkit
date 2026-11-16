//! REST client for the IBKR Web API (Client Portal API).
//!
//! Two session modes exist:
//!
//! * **Gateway**: the Client Portal Gateway runs locally (default
//!   `https://localhost:5000/v1/api`) and holds a browser login session. It
//!   uses a self-signed certificate, so certificate checks are off by default
//!   for `localhost`.
//! * **OAuth 1.0a**: requests go to `https://api.ibkr.com/v1/api` and every
//!   request is signed; call [`WebClient::oauth_init`] first.
//! * **OAuth 2.0**: requests go to `https://api.ibkr.com/v1/api` with a bearer
//!   token; call [`WebClient::oauth2_init`] first.
//!
//! Every endpoint method returns the JSON body as [`serde_json::Value`].

use crate::oauth::{self, OAuthConfig, Signing};
use crate::oauth2::{self, OAuth2Config};
pub use reqwest::Method;
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::Mutex;
use std::time::Duration;

/// Default base URL of the local Client Portal Gateway.
pub const GATEWAY_URL: &str = "https://localhost:5000/v1/api";
/// Base URL for OAuth sessions.
pub const OAUTH_URL: &str = "https://api.ibkr.com/v1/api";

/// Errors of the Web API client.
#[derive(Debug, thiserror::Error)]
pub enum WebError {
    #[error("HTTP request to {url} failed: {source}")]
    Http { url: String, source: reqwest::Error },
    #[error("{method} {url} returned HTTP {status}: {body}")]
    Status {
        method: String,
        url: String,
        status: u16,
        body: String,
    },
    #[error(transparent)]
    OAuth(#[from] oauth::OAuthError),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, WebError>;

impl WebError {
    pub fn kind(&self) -> ibkit_tws::ErrorKind {
        match self {
            Self::Http { .. } => ibkit_tws::ErrorKind::Connection,
            Self::Invalid(_) => ibkit_tws::ErrorKind::InvalidArgument,
            Self::Status { .. } | Self::OAuth(_) => ibkit_tws::ErrorKind::Other,
        }
    }
}

/// One of the HTTP methods that the Web API uses, from its name in any case.
pub fn parse_method(name: &str) -> Result<Method> {
    match name.to_ascii_uppercase().as_str() {
        "GET" => Ok(Method::GET),
        "POST" => Ok(Method::POST),
        "PUT" => Ok(Method::PUT),
        "PATCH" => Ok(Method::PATCH),
        "DELETE" => Ok(Method::DELETE),
        _ => Err(WebError::Invalid(format!(
            "You gave the HTTP method {name:?}. Use GET, POST, PUT, PATCH or DELETE."
        ))),
    }
}

/// Settings of a [`WebClient`].
#[derive(Clone, Debug)]
pub struct WebConfig {
    pub base_url: String,
    /// Accept self-signed certificates (the local gateway uses one).
    pub accept_invalid_certs: bool,
    pub timeout: Duration,
    pub oauth: Option<OAuthConfig>,
    pub oauth2: Option<OAuth2Config>,
    /// Wait before a request that would exceed IBKR's rate limits (10
    /// requests per second per session, and the per-endpoint limits).
    /// IBKR answers excess requests with 429 and blocks the IP address for
    /// 10 to 15 minutes.
    pub pacing: bool,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            base_url: GATEWAY_URL.into(),
            accept_invalid_certs: true,
            timeout: Duration::from_secs(30),
            oauth: None,
            oauth2: None,
            pacing: true,
        }
    }
}

/// Sliding-window request pacing with IBKR's documented Web API limits
/// (Trading, "Pacing Limitations").
#[derive(Debug, Default)]
pub(crate) struct Pacer {
    sent: std::collections::HashMap<String, std::collections::VecDeque<std::time::Instant>>,
}

/// `(window key, max requests, period)` rules that apply to `method path`.
fn pacing_rules(method: &str, path: &str) -> Vec<(String, usize, Duration)> {
    const SEC: Duration = Duration::from_secs(1);
    let path = path.trim_start_matches('/').split('?').next().unwrap_or("");
    let mut rules = vec![("*".to_string(), 10, SEC)];
    let endpoint = |max, period| (format!("{method} {path}"), max, period);
    match (method, path) {
        (
            "GET",
            "iserver/account/orders"
            | "iserver/account/trades"
            | "iserver/account/pnl/partitioned"
            | "portfolio/accounts"
            | "portfolio/subaccounts",
        ) => rules.push(endpoint(1, Duration::from_secs(5))),
        ("GET", "iserver/marketdata/history") => rules.push(endpoint(50, Duration::from_secs(60))),
        ("GET", "iserver/scanner/params")
        | ("POST", "pa/performance" | "pa/summary" | "pa/transactions") => {
            rules.push(endpoint(1, Duration::from_secs(900)))
        }
        ("POST", "iserver/scanner/run") | (_, "tickle") => rules.push(endpoint(1, SEC)),
        ("GET", "sso/validate") => rules.push(endpoint(1, Duration::from_secs(60))),
        (_, p) if p.starts_with("fyi/") => {
            // One request per second per FYI endpoint; ids in the path
            // (typecodes, notification and device ids) share the endpoint.
            let mut parts = p.split('/');
            let group = parts.nth(1).unwrap_or("");
            let next = parts.next().unwrap_or("");
            let fixed = matches!(next, "" | "more" | "email" | "device");
            let key = if fixed {
                format!("{method} fyi/{group}/{next}")
            } else {
                format!("{method} fyi/{group}/{{id}}")
            };
            rules.push((key, 1, SEC));
        }
        _ => {}
    }
    rules
}

impl Pacer {
    /// Reserve a slot for `method path` at or after `now`; returns how long
    /// to wait before sending.
    pub(crate) fn reserve(
        &mut self,
        method: &str,
        path: &str,
        now: std::time::Instant,
    ) -> Duration {
        let rules = pacing_rules(method, path);
        let mut at = now;
        for (key, max, period) in &rules {
            let sent = self.sent.entry(key.clone()).or_default();
            while sent.front().is_some_and(|t| *t + *period <= at) {
                sent.pop_front();
            }
            if sent.len() >= *max {
                at = at.max(sent[sent.len() - max] + *period);
            }
        }
        for (key, _, period) in &rules {
            let sent = self.sent.entry(key.clone()).or_default();
            // Reservations can lie in the future; keep the window sorted.
            let pos = sent.partition_point(|t| *t <= at);
            sent.insert(pos, at);
            while sent.front().is_some_and(|t| *t + *period <= now) {
                sent.pop_front();
            }
        }
        at - now
    }
}

impl WebConfig {
    pub fn oauth(cfg: OAuthConfig) -> Self {
        Self {
            base_url: OAUTH_URL.into(),
            accept_invalid_certs: false,
            oauth: Some(cfg),
            ..Default::default()
        }
    }

    pub fn oauth2(cfg: OAuth2Config) -> Self {
        Self {
            base_url: OAUTH_URL.into(),
            accept_invalid_certs: false,
            oauth2: Some(cfg),
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, Default)]
struct LiveSession {
    token: String,
    expires_ms: i64,
}

/// Web API client.
pub struct WebClient {
    pub(crate) http: reqwest::Client,
    cfg: WebConfig,
    session: Mutex<LiveSession>,
    /// OAuth 2.0 access token for `/gw/api` requests.
    access: Mutex<LiveSession>,
    pacer: Mutex<Pacer>,
}

/// An order for `POST /iserver/account/{accountId}/orders`.
#[derive(Clone, Debug, Default, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebOrder {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acct_id: Option<String>,
    pub conid: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conidex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sec_type: Option<String>,
    #[serde(rename = "cOID", skip_serializing_if = "Option::is_none")]
    pub c_oid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub order_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing_exchange: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_single_group: Option<bool>,
    /// Sent as `outsideRTH`, the name in IBKR's order schema.
    #[serde(rename = "outsideRTH", alias = "outsideRth")]
    pub outside_rth: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_or_none: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aux_price: Option<f64>,
    pub side: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticker: Option<String>,
    pub tif: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trailing_amt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trailing_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub referrer: Option<String>,
    pub quantity: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cash_qty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fx_qty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_adaptive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_ccy_conv: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocation_method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy_parameters: Option<Value>,
    /// Required for US futures and futures options (CME Rule 536-B): true
    /// when a person entered the order, false for an automated system.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manual_indicator: Option<bool>,
    /// Person or system operating the API session (CME Rule 536-B).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ext_operator: Option<String>,
    /// Tax lots selected for gains and losses management.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tax_optimizer_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_payload: Option<Value>,
}

/// Send `req`: the body of a 2xx answer, otherwise [`WebError::Status`]
/// with the body as text.
pub(crate) async fn fetch(
    method: &str,
    url: &str,
    req: reqwest::RequestBuilder,
) -> Result<Vec<u8>> {
    let http_err = |source| WebError::Http {
        url: url.into(),
        source,
    };
    let resp = req.send().await.map_err(http_err)?;
    let status = resp.status();
    let body = resp.bytes().await.map_err(http_err)?.to_vec();
    if !status.is_success() {
        return Err(WebError::Status {
            method: method.into(),
            url: url.into(),
            status: status.as_u16(),
            body: String::from_utf8_lossy(&body).into_owned(),
        });
    }
    Ok(body)
}

/// `base` and `path` with one slash between them.
pub(crate) fn join(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

pub(crate) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

pub(crate) fn q(pairs: &[(&str, Option<String>)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .filter_map(|(k, v)| v.as_ref().map(|v| (k.to_string(), v.clone())))
        .collect()
}

pub(crate) fn s<T: ToString>(v: T) -> Option<String> {
    Some(v.to_string())
}

impl WebClient {
    pub fn new(cfg: WebConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .cookie_store(true)
            .danger_accept_invalid_certs(cfg.accept_invalid_certs)
            .timeout(cfg.timeout)
            .user_agent("ibkit")
            .build()
            .map_err(|e| WebError::Http {
                url: cfg.base_url.clone(),
                source: e,
            })?;
        Ok(Self {
            http,
            cfg,
            session: Mutex::new(LiveSession::default()),
            access: Mutex::new(LiveSession::default()),
            pacer: Mutex::new(Pacer::default()),
        })
    }

    /// Client for the local gateway at `base_url` (for example `GATEWAY_URL`).
    pub fn gateway(base_url: &str) -> Result<Self> {
        Self::new(WebConfig {
            base_url: base_url.into(),
            ..Default::default()
        })
    }

    pub fn base_url(&self) -> &str {
        &self.cfg.base_url
    }

    /// Current OAuth 1.0a live session token or OAuth 2.0 bearer token, if one
    /// was obtained and has not expired.
    pub fn live_session_token(&self) -> Option<String> {
        let s = self.session.lock().expect("session lock");
        let expired = s.expires_ms > 0 && s.expires_ms <= unix_now() * 1000;
        (!s.token.is_empty() && !expired).then(|| s.token.clone())
    }

    /// Expiry of that token in milliseconds since the epoch.
    pub fn live_session_token_expiration(&self) -> i64 {
        self.session.lock().expect("session lock").expires_ms
    }

    fn url(&self, path: &str) -> String {
        join(&self.cfg.base_url, path)
    }

    /// Obtain an OAuth live session token and open a brokerage session.
    pub async fn oauth_init(&self, compete: bool) -> Result<Value> {
        let cfg = self.cfg.oauth.clone().ok_or_else(|| {
            WebError::Invalid("oauth_init needs WebConfig::oauth credentials".into())
        })?;
        let enc = oauth::read_private_key(&cfg.encryption_key_pem)?;
        let sig = oauth::read_private_key(&cfg.signature_key_pem)?;
        let prepend = oauth::live_session_token_prepend(&cfg.access_token_secret, &enc)?;
        let random = oauth::dh_random();
        let challenge = oauth::dh_challenge(&cfg.dh_prime, &random, cfg.dh_generator)?;
        let resp = self
            .rsa_post(
                &cfg,
                "oauth/live_session_token",
                ("diffie_hellman_challenge", challenge),
                &sig,
                &prepend,
            )
            .await?;
        let dh_response = resp["diffie_hellman_response"].as_str().unwrap_or_default();
        let token = oauth::live_session_token(&cfg.dh_prime, &random, dh_response, &prepend)?;
        let signature = resp["live_session_token_signature"]
            .as_str()
            .unwrap_or_default();
        if !oauth::validate_live_session_token(&token, signature, &cfg.consumer_key) {
            return Err(WebError::Invalid(
                "the live session token does not match the server signature; check the consumer key, access token secret and DH prime".into(),
            ));
        }
        *self.session.lock().expect("session lock") = LiveSession {
            token,
            expires_ms: resp["live_session_token_expiration"].as_i64().unwrap_or(0),
        };
        self.ssodh_init(compete).await
    }

    /// Third-party OAuth 1.0a, step 1: `POST oauth/request_token` signed with
    /// RSA-SHA256 by the signature key (`oauth_callback=oob`). Returns the
    /// temporary `oauth_token` the user authorizes at IBKR; the access token
    /// in [`OAuthConfig`] is not used.
    pub async fn oauth_request_token(&self) -> Result<String> {
        let mut cfg = self.cfg.oauth.clone().ok_or_else(|| {
            WebError::Invalid("oauth_request_token needs WebConfig::oauth with the consumer key and signature key".into())
        })?;
        cfg.access_token.clear();
        let key = oauth::read_private_key(&cfg.signature_key_pem)?;
        let resp = self
            .rsa_post(
                &cfg,
                "oauth/request_token",
                ("oauth_callback", "oob".into()),
                &key,
                "",
            )
            .await?;
        resp["oauth_token"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| {
                WebError::Invalid(format!(
                    "POST oauth/request_token returned no oauth_token: {resp}; check the consumer key and that the signature key is the one registered for it"
                ))
            })
    }

    /// Third-party OAuth 1.0a, step 3: exchange the authorized
    /// `request_token` and the `verifier` IBKR showed the user for a
    /// permanent access token. Returns `oauth_token`, `oauth_token_secret`
    /// and `is_true` (whether the user is a paper account).
    pub async fn oauth_access_token(&self, request_token: &str, verifier: &str) -> Result<Value> {
        let mut cfg = self.cfg.oauth.clone().ok_or_else(|| {
            WebError::Invalid(
                "oauth_access_token needs WebConfig::oauth with the consumer key and signature key"
                    .into(),
            )
        })?;
        cfg.access_token = request_token.into();
        let key = oauth::read_private_key(&cfg.signature_key_pem)?;
        self.rsa_post(
            &cfg,
            "oauth/access_token",
            ("oauth_verifier", verifier.into()),
            &key,
            "",
        )
        .await
    }

    /// POST to an OAuth 1.0a token endpoint, signed with RSA-SHA256.
    async fn rsa_post(
        &self,
        cfg: &OAuthConfig,
        path: &str,
        extra: (&str, String),
        key: &rsa::RsaPrivateKey,
        prepend: &str,
    ) -> Result<Value> {
        let url = self.url(path);
        let auth = oauth::sign_request(
            cfg,
            "POST",
            &url,
            &[],
            &[extra],
            Signing::Rsa { key, prepend },
        )?;
        self.send_raw(Method::POST, &url, &[], None, Some(auth))
            .await
    }

    /// Obtain an OAuth 2.0 bearer token (token request, then SSO session) and
    /// open a brokerage session. Returns the `ssodh/init` answer.
    pub async fn oauth2_init(&self, compete: bool) -> Result<Value> {
        let cfg = self.oauth2_config()?;
        let session = self
            .gw_request(
                Method::POST,
                "gw/api/v1/sso-sessions",
                &[],
                Some(&json!({"ip": cfg.ip, "credential": cfg.credential})),
            )
            .await?;
        let bearer = session["access_token"].as_str().unwrap_or_default();
        if bearer.is_empty() {
            return Err(WebError::Invalid(format!(
                "POST gw/api/v1/sso-sessions returned no access_token: {session}; check the username (credential) and that ip is the public address this request comes from"
            )));
        }
        *self.session.lock().expect("session lock") = LiveSession {
            token: bearer.into(),
            expires_ms: (unix_now() + 86400) * 1000,
        };
        self.ssodh_init(compete).await
    }

    pub(crate) fn oauth2_config(&self) -> Result<&OAuth2Config> {
        self.cfg.oauth2.as_ref().ok_or_else(|| {
            WebError::Invalid(
                "this request needs OAuth 2.0 credentials; build the client with WebClient::new(WebConfig::oauth2(OAuth2Config::new(...)))".into(),
            )
        })
    }

    /// OAuth 2.0 access token (`POST {auth_url}/oauth2/api/v1/token` with a
    /// signed client assertion) for the scopes in [`OAuth2Config::scope`];
    /// kept until shortly before it expires.
    pub async fn oauth2_access_token(&self) -> Result<String> {
        let now = unix_now();
        {
            let a = self.access.lock().expect("access token lock");
            if !a.token.is_empty() && a.expires_ms > (now + 5) * 1000 {
                return Ok(a.token.clone());
            }
        }
        let cfg = self.oauth2_config()?;
        let key = cfg.private_key()?;
        let url = join(&cfg.auth_url, "oauth2/api/v1/token");
        let assertion = oauth2::access_token_assertion(cfg, &key, now);
        let req = self
            .http
            .post(&url)
            .form(&oauth2::token_form(cfg, &assertion));
        let answer = self.send("POST", &url, req).await?;
        let token = answer["access_token"].as_str().unwrap_or_default();
        if token.is_empty() {
            return Err(WebError::Invalid(format!(
                "POST {url} returned no access_token: {answer}; check the client id, key id, scope and that the private key matches the registered public key"
            )));
        }
        *self.access.lock().expect("access token lock") = LiveSession {
            token: token.into(),
            expires_ms: (now + answer["expires_in"].as_i64().unwrap_or(60)) * 1000,
        };
        Ok(token.into())
    }

    /// Send one request to a `/gw/api` path (Account Management, third-party,
    /// echo and SSO endpoints) on [`OAuth2Config::auth_url`] with the OAuth
    /// 2.0 access token. A body is sent as a signed JWT (`application/jwt`,
    /// valid 60 s; 24 h for `sso-sessions` as IBKR documents), as the "Signed
    /// JWT" security policy requires for every POST and PATCH there.
    pub async fn gw_request(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<&Value>,
    ) -> Result<Value> {
        let token = self.oauth2_access_token().await?;
        let cfg = self.oauth2_config()?;
        let url = join(&cfg.auth_url, path);
        let mut req = self
            .http
            .request(method.clone(), &url)
            .query(query)
            .bearer_auth(token);
        if method != Method::GET && method != Method::DELETE {
            let lifetime = if path.ends_with("sso-sessions") {
                86400
            } else {
                60
            };
            let jwt = oauth2::signed_body(
                cfg,
                &cfg.private_key()?,
                body.unwrap_or(&Value::Null),
                unix_now(),
                lifetime,
            )?;
            req = req.header("Content-Type", "application/jwt").body(jwt);
        }
        self.send(method.as_str(), &url, req).await
    }

    async fn send_raw(
        &self,
        method: Method,
        url: &str,
        query: &[(String, String)],
        body: Option<&Value>,
        auth: Option<String>,
    ) -> Result<Value> {
        let mut req = self.http.request(method.clone(), url).query(query);
        if let Some(a) = auth {
            req = req.header("Authorization", a);
        }
        if let Some(b) = body {
            req = req.json(b);
        } else if method == Method::POST {
            req = req.header("Content-Length", "0");
        }
        self.send(method.as_str(), url, req).await
    }

    /// Send `req` and parse the answer as JSON (an empty body is `Null`,
    /// a body that is not JSON a `String`).
    pub(crate) async fn send(
        &self,
        method: &str,
        url: &str,
        req: reqwest::RequestBuilder,
    ) -> Result<Value> {
        let text = String::from_utf8(fetch(method, url, req).await?)
            .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    /// Send one request to `path` (relative to the base URL).
    pub async fn request(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<&Value>,
    ) -> Result<Value> {
        let url = self.url(path);
        // Wait first: an OAuth 1.0a signature carries a timestamp and a nonce.
        if self.cfg.pacing {
            let wait = self.pacer.lock().expect("pacer lock").reserve(
                method.as_str(),
                path,
                std::time::Instant::now(),
            );
            if !wait.is_zero() {
                tracing::debug!(?wait, path, "waiting for IBKR's Web API rate limit");
                tokio::time::sleep(wait).await;
            }
        }
        let auth = match (&self.cfg.oauth, &self.cfg.oauth2) {
            (_, Some(_)) => {
                let token = self.live_session_token().ok_or_else(|| {
                    WebError::Invalid("no current OAuth 2.0 bearer token: call oauth2_init first, and again after the token expires (live_session_token_expiration)".into())
                })?;
                Some(format!("Bearer {token}"))
            }
            (Some(cfg), None) => {
                let token = self.live_session_token().ok_or_else(|| {
                    WebError::Invalid("no current live session token: call oauth_init first, and again after the token expires (live_session_token_expiration)".into())
                })?;
                Some(oauth::sign_request(
                    cfg,
                    method.as_str(),
                    &url,
                    query,
                    &[],
                    Signing::Hmac {
                        live_session_token: &token,
                    },
                )?)
            }
            (None, None) => None,
        };
        self.send_raw(method, &url, query, body, auth).await
    }

    pub async fn get(&self, path: &str, query: &[(String, String)]) -> Result<Value> {
        self.request(Method::GET, path, query, None).await
    }

    pub async fn post(&self, path: &str, body: Option<&Value>) -> Result<Value> {
        self.request(Method::POST, path, &[], body).await
    }

    pub async fn delete(&self, path: &str) -> Result<Value> {
        self.request(Method::DELETE, path, &[], None).await
    }

    // ---- session ---------------------------------------------------------

    pub async fn auth_status(&self) -> Result<Value> {
        self.post("iserver/auth/status", None).await
    }
    pub async fn ssodh_init(&self, compete: bool) -> Result<Value> {
        self.post(
            "iserver/auth/ssodh/init",
            Some(&json!({"publish": true, "compete": compete})),
        )
        .await
    }
    #[deprecated(note = "IBKR deprecated POST iserver/reauthenticate; call ssodh_init")]
    pub async fn reauthenticate(&self) -> Result<Value> {
        self.post("iserver/reauthenticate", None).await
    }
    pub async fn validate(&self) -> Result<Value> {
        self.get("sso/validate", &[]).await
    }
    /// Keep the session alive; call about once a minute.
    pub async fn tickle(&self) -> Result<Value> {
        self.post("tickle", None).await
    }
    pub async fn logout(&self) -> Result<Value> {
        self.post("logout", None).await
    }

    // ---- accounts --------------------------------------------------------

    pub async fn iserver_accounts(&self) -> Result<Value> {
        self.get("iserver/accounts", &[]).await
    }
    pub async fn switch_account(&self, account_id: &str) -> Result<Value> {
        self.post("iserver/account", Some(&json!({"acctId": account_id})))
            .await
    }
    pub async fn set_dynamic_account(&self, account_id: &str) -> Result<Value> {
        self.post("iserver/dynaccount", Some(&json!({"acctId": account_id})))
            .await
    }
    pub async fn search_dynamic_account(&self, pattern: &str) -> Result<Value> {
        self.get(&format!("iserver/account/search/{pattern}"), &[])
            .await
    }
    pub async fn signatures_and_owners(&self, account_id: &str) -> Result<Value> {
        self.get(&format!("acesws/{account_id}/signatures-and-owners"), &[])
            .await
    }
    pub async fn account_summary(&self, account_id: &str) -> Result<Value> {
        self.get(&format!("iserver/account/{account_id}/summary"), &[])
            .await
    }
    pub async fn account_summary_available_funds(&self, account_id: &str) -> Result<Value> {
        self.get(
            &format!("iserver/account/{account_id}/summary/available_funds"),
            &[],
        )
        .await
    }
    pub async fn account_summary_balances(&self, account_id: &str) -> Result<Value> {
        self.get(
            &format!("iserver/account/{account_id}/summary/balances"),
            &[],
        )
        .await
    }
    pub async fn account_summary_margins(&self, account_id: &str) -> Result<Value> {
        self.get(
            &format!("iserver/account/{account_id}/summary/margins"),
            &[],
        )
        .await
    }
    pub async fn account_summary_market_value(&self, account_id: &str) -> Result<Value> {
        self.get(
            &format!("iserver/account/{account_id}/summary/market_value"),
            &[],
        )
        .await
    }
    pub async fn account_profit_and_loss(&self) -> Result<Value> {
        self.get("iserver/account/pnl/partitioned", &[]).await
    }

    // ---- portfolio -------------------------------------------------------

    pub async fn portfolio_accounts(&self) -> Result<Value> {
        self.get("portfolio/accounts", &[]).await
    }
    pub async fn portfolio_subaccounts(&self) -> Result<Value> {
        self.get("portfolio/subaccounts", &[]).await
    }
    pub async fn portfolio_subaccounts_large(
        &self,
        page: i32,
        page_size: Option<i32>,
        nocache: Option<bool>,
    ) -> Result<Value> {
        let query = q(&[
            ("page", s(page)),
            ("pageSize", page_size.map(|v| v.to_string())),
            ("nocache", nocache.map(|b| b.to_string())),
        ]);
        self.get("portfolio/subaccounts2", &query).await
    }
    pub async fn portfolio_account_info(&self, account_id: &str) -> Result<Value> {
        self.get(&format!("portfolio/{account_id}/meta"), &[]).await
    }
    pub async fn portfolio_allocation(
        &self,
        account_id: &str,
        model: Option<&str>,
    ) -> Result<Value> {
        self.get(
            &format!("portfolio/{account_id}/allocation"),
            &q(&[("model", model.map(String::from))]),
        )
        .await
    }
    pub async fn portfolio_allocations(&self, account_ids: &[String]) -> Result<Value> {
        self.post(
            "portfolio/allocation",
            Some(&json!({"acctIds": account_ids})),
        )
        .await
    }
    pub async fn positions(
        &self,
        account_id: &str,
        page: i32,
        model: Option<&str>,
        sort: Option<&str>,
        direction: Option<&str>,
        wait_for_sec_def: Option<bool>,
    ) -> Result<Value> {
        let query = q(&[
            ("model", model.map(String::from)),
            ("sort", sort.map(String::from)),
            ("direction", direction.map(String::from)),
            ("waitForSecDef", wait_for_sec_def.map(|b| b.to_string())),
        ]);
        self.get(&format!("portfolio/{account_id}/positions/{page}"), &query)
            .await
    }
    pub async fn positions2(
        &self,
        account_id: &str,
        model: Option<&str>,
        sort: Option<&str>,
        direction: Option<&str>,
    ) -> Result<Value> {
        let query = q(&[
            ("model", model.map(String::from)),
            ("sort", sort.map(String::from)),
            ("direction", direction.map(String::from)),
        ]);
        self.get(&format!("portfolio2/{account_id}/positions"), &query)
            .await
    }
    pub async fn position_by_conid(
        &self,
        account_id: &str,
        conid: i64,
        model: Option<&str>,
    ) -> Result<Value> {
        self.get(
            &format!("portfolio/{account_id}/position/{conid}"),
            &q(&[("model", model.map(String::from))]),
        )
        .await
    }
    pub async fn positions_by_conid(&self, conid: i64) -> Result<Value> {
        self.get(&format!("portfolio/positions/{conid}"), &[]).await
    }
    pub async fn invalidate_positions_cache(&self, account_id: &str) -> Result<Value> {
        self.post(
            &format!("portfolio/{account_id}/positions/invalidate"),
            None,
        )
        .await
    }
    pub async fn portfolio_summary(&self, account_id: &str, model: Option<&str>) -> Result<Value> {
        self.get(
            &format!("portfolio/{account_id}/summary"),
            &q(&[("model", model.map(String::from))]),
        )
        .await
    }
    pub async fn portfolio_ledger(&self, account_id: &str, model: Option<&str>) -> Result<Value> {
        self.get(
            &format!("portfolio/{account_id}/ledger"),
            &q(&[("model", model.map(String::from))]),
        )
        .await
    }
    pub async fn combo_positions(&self, account_id: &str, nocache: bool) -> Result<Value> {
        self.get(
            &format!("portfolio/{account_id}/combo/positions"),
            &q(&[("nocache", s(nocache))]),
        )
        .await
    }

    // ---- portfolio analyst -------------------------------------------------

    pub async fn pa_performance(&self, account_ids: &[String], period: &str) -> Result<Value> {
        self.post(
            "pa/performance",
            Some(&json!({"acctIds": account_ids, "period": period})),
        )
        .await
    }
    pub async fn pa_all_periods(&self, account_ids: &[String]) -> Result<Value> {
        self.post("pa/allperiods", Some(&json!({"acctIds": account_ids})))
            .await
    }
    pub async fn pa_transactions(
        &self,
        account_ids: &[String],
        conids: &[i64],
        currency: &str,
        days: Option<i32>,
    ) -> Result<Value> {
        let mut body = json!({"acctIds": account_ids, "conids": conids, "currency": currency});
        if let Some(d) = days {
            body["days"] = json!(d);
        }
        self.post("pa/transactions", Some(&body)).await
    }

    // ---- contracts ---------------------------------------------------------

    /// `GET iserver/secdef/search`. `more` returns more results, `fund`
    /// searches mutual funds (`fund_family_conid_ex` narrows to one family),
    /// `pattern` treats `symbol` as a pattern.
    pub async fn search_contract_by_symbol(
        &self,
        symbol: &str,
        name: Option<bool>,
        sec_type: Option<&str>,
        more: Option<bool>,
        fund: Option<bool>,
        fund_family_conid_ex: Option<&str>,
        pattern: Option<bool>,
        referrer: Option<&str>,
    ) -> Result<Value> {
        let query = q(&[
            ("symbol", s(symbol)),
            ("name", name.map(|b| b.to_string())),
            ("secType", sec_type.map(String::from)),
            ("more", more.map(|b| b.to_string())),
            ("fund", fund.map(|b| b.to_string())),
            ("fundFamilyConidEx", fund_family_conid_ex.map(String::from)),
            ("pattern", pattern.map(|b| b.to_string())),
            ("referrer", referrer.map(String::from)),
        ]);
        self.get("iserver/secdef/search", &query).await
    }
    pub async fn search_secdef_info(
        &self,
        conid: i64,
        sec_type: &str,
        month: Option<&str>,
        exchange: Option<&str>,
        strike: Option<f64>,
        right: Option<&str>,
        issuer_id: Option<&str>,
        filters: Option<&str>,
    ) -> Result<Value> {
        let query = q(&[
            ("conid", s(conid)),
            ("sectype", s(sec_type)),
            ("month", month.map(String::from)),
            ("exchange", exchange.map(String::from)),
            ("strike", strike.map(|v| v.to_string())),
            ("right", right.map(String::from)),
            ("issuerId", issuer_id.map(String::from)),
            ("filters", filters.map(String::from)),
        ]);
        self.get("iserver/secdef/info", &query).await
    }
    pub async fn search_strikes(
        &self,
        conid: i64,
        sec_type: &str,
        month: &str,
        exchange: Option<&str>,
    ) -> Result<Value> {
        let query = q(&[
            ("conid", s(conid)),
            ("sectype", s(sec_type)),
            ("month", s(month)),
            ("exchange", exchange.map(String::from)),
        ]);
        self.get("iserver/secdef/strikes", &query).await
    }
    pub async fn bond_filters(&self, issuer_id: &str) -> Result<Value> {
        self.get(
            "iserver/secdef/bond-filters",
            &q(&[("symbol", s("BOND")), ("issuerId", s(issuer_id))]),
        )
        .await
    }
    pub async fn contract_info(&self, conid: i64) -> Result<Value> {
        self.get(&format!("iserver/contract/{conid}/info"), &[])
            .await
    }
    pub async fn contract_info_and_rules(&self, conid: i64, is_buy: Option<bool>) -> Result<Value> {
        self.get(
            &format!("iserver/contract/{conid}/info-and-rules"),
            &q(&[("isBuy", is_buy.map(|b| b.to_string()))]),
        )
        .await
    }
    pub async fn contract_rules(
        &self,
        conid: i64,
        exchange: Option<&str>,
        is_buy: bool,
        modify_order: bool,
        order_id: Option<i64>,
    ) -> Result<Value> {
        let mut body = json!({"conid": conid, "isBuy": is_buy, "modifyOrder": modify_order});
        if let Some(e) = exchange {
            body["exchange"] = json!(e);
        }
        if let Some(o) = order_id {
            body["orderId"] = json!(o);
        }
        self.post("iserver/contract/rules", Some(&body)).await
    }
    pub async fn algo_params(
        &self,
        conid: i64,
        algos: Option<&str>,
        add_description: bool,
        add_params: bool,
    ) -> Result<Value> {
        let query = q(&[
            ("algos", algos.map(String::from)),
            ("addDescription", s(u8::from(add_description))),
            ("addParams", s(u8::from(add_params))),
        ]);
        self.get(&format!("iserver/contract/{conid}/algos"), &query)
            .await
    }
    pub async fn currency_pairs(&self, currency: &str) -> Result<Value> {
        self.get("iserver/currency/pairs", &q(&[("currency", s(currency))]))
            .await
    }
    pub async fn currency_exchange_rate(&self, source: &str, target: &str) -> Result<Value> {
        self.get(
            "iserver/exchangerate",
            &q(&[("source", s(source)), ("target", s(target))]),
        )
        .await
    }
    pub async fn security_definition_by_conid(&self, conids: &[i64]) -> Result<Value> {
        let ids: Vec<String> = conids.iter().map(|c| c.to_string()).collect();
        self.get("trsrv/secdef", &q(&[("conids", s(ids.join(",")))]))
            .await
    }
    pub async fn security_future_by_symbol(
        &self,
        symbols: &[&str],
        exchange: Option<&str>,
    ) -> Result<Value> {
        let query = q(&[
            ("symbols", s(symbols.join(","))),
            ("exchange", exchange.map(String::from)),
        ]);
        self.get("trsrv/futures", &query).await
    }
    pub async fn security_stocks_by_symbol(&self, symbols: &[&str]) -> Result<Value> {
        self.get("trsrv/stocks", &q(&[("symbols", s(symbols.join(",")))]))
            .await
    }
    pub async fn trading_schedule(
        &self,
        asset_class: &str,
        symbol: &str,
        exchange: Option<&str>,
        exchange_filter: Option<&str>,
    ) -> Result<Value> {
        let query = q(&[
            ("assetClass", s(asset_class)),
            ("symbol", s(symbol)),
            ("exchange", exchange.map(String::from)),
            ("exchangeFilter", exchange_filter.map(String::from)),
        ]);
        self.get("trsrv/secdef/schedule", &query).await
    }
    pub async fn all_conids(&self, exchange: &str, asset_class: Option<&str>) -> Result<Value> {
        self.get(
            "trsrv/all-conids",
            &q(&[
                ("exchange", s(exchange)),
                ("assetClass", asset_class.map(String::from)),
            ]),
        )
        .await
    }

    // ---- market data -------------------------------------------------------

    /// Field ids are the documented market data field tags, for example
    /// `31` last price, `84` bid, `86` ask.
    pub async fn live_marketdata_snapshot(&self, conids: &[i64], fields: &[&str]) -> Result<Value> {
        let ids: Vec<String> = conids.iter().map(|c| c.to_string()).collect();
        self.get(
            "iserver/marketdata/snapshot",
            &q(&[
                ("conids", s(ids.join(","))),
                ("fields", s(fields.join(","))),
            ]),
        )
        .await
    }
    pub async fn regulatory_snapshot(&self, conid: i64) -> Result<Value> {
        self.get("md/regsnapshot", &q(&[("conid", s(conid))])).await
    }
    pub async fn marketdata_history_by_conid(
        &self,
        conid: i64,
        bar: &str,
        exchange: Option<&str>,
        period: Option<&str>,
        outside_rth: Option<bool>,
        start_time: Option<&str>,
        direction: Option<i32>,
        source: Option<&str>,
    ) -> Result<Value> {
        let query = q(&[
            ("conid", s(conid)),
            ("bar", s(bar)),
            ("exchange", exchange.map(String::from)),
            ("period", period.map(String::from)),
            ("outsideRth", outside_rth.map(|b| b.to_string())),
            ("startTime", start_time.map(String::from)),
            ("direction", direction.map(|d| d.to_string())),
            ("source", source.map(String::from)),
        ]);
        self.get("iserver/marketdata/history", &query).await
    }
    #[deprecated(
        note = "GET hmds/history is not in IBKR's current Web API reference; it may stop working; use marketdata_history_by_conid"
    )]
    pub async fn historical_marketdata_beta(
        &self,
        conid: i64,
        period: &str,
        bar: &str,
        outside_rth: Option<bool>,
        start_time: Option<&str>,
        direction: Option<&str>,
        bar_type: Option<&str>,
    ) -> Result<Value> {
        let query = q(&[
            ("conid", s(conid)),
            ("period", s(period)),
            ("bar", s(bar)),
            ("outsideRth", outside_rth.map(|b| b.to_string())),
            ("startTime", start_time.map(String::from)),
            ("direction", direction.map(String::from)),
            ("barType", bar_type.map(String::from)),
        ]);
        self.get("hmds/history", &query).await
    }
    pub async fn marketdata_unsubscribe(&self, conid: i64) -> Result<Value> {
        self.post(
            "iserver/marketdata/unsubscribe",
            Some(&json!({"conid": conid})),
        )
        .await
    }
    pub async fn marketdata_unsubscribe_all(&self) -> Result<Value> {
        self.get("iserver/marketdata/unsubscribeall", &[]).await
    }

    // ---- orders ------------------------------------------------------------

    pub async fn live_orders(
        &self,
        filters: Option<&str>,
        force: bool,
        account_id: Option<&str>,
    ) -> Result<Value> {
        let query = q(&[
            ("filters", filters.map(String::from)),
            ("force", s(force)),
            ("accountId", account_id.map(String::from)),
        ]);
        self.get("iserver/account/orders", &query).await
    }
    pub async fn order_status(&self, order_id: i64) -> Result<Value> {
        self.get(&format!("iserver/account/order/status/{order_id}"), &[])
            .await
    }
    pub async fn trades(&self, days: Option<i32>, account_id: Option<&str>) -> Result<Value> {
        self.get(
            "iserver/account/trades",
            &q(&[
                ("days", days.map(|d| d.to_string())),
                ("accountId", account_id.map(String::from)),
            ]),
        )
        .await
    }
    /// Submit orders. The answer may contain questions (`id` + `message`) that
    /// must be confirmed with [`WebClient::reply`].
    pub async fn place_order(&self, account_id: &str, orders: &[WebOrder]) -> Result<Value> {
        self.post(
            &format!("iserver/account/{account_id}/orders"),
            Some(&json!({"orders": orders})),
        )
        .await
    }
    /// Submit orders and confirm every question automatically, up to `max_replies`.
    pub async fn place_order_confirmed(
        &self,
        account_id: &str,
        orders: &[WebOrder],
        max_replies: usize,
    ) -> Result<Value> {
        let mut resp = self.place_order(account_id, orders).await?;
        for _ in 0..max_replies {
            let Some(id) = resp
                .get(0)
                .and_then(|r| r.get("id"))
                .and_then(|v| v.as_str())
                .filter(|_| resp[0].get("message").is_some())
                .map(String::from)
            else {
                return Ok(resp);
            };
            resp = self.reply(&id, true).await?;
        }
        Err(WebError::Invalid(format!(
            "orders still need confirmation after {max_replies} replies: {resp}"
        )))
    }
    pub async fn reply(&self, reply_id: &str, confirmed: bool) -> Result<Value> {
        self.post(
            &format!("iserver/reply/{reply_id}"),
            Some(&json!({"confirmed": confirmed})),
        )
        .await
    }
    pub async fn whatif_order(&self, account_id: &str, orders: &[WebOrder]) -> Result<Value> {
        self.post(
            &format!("iserver/account/{account_id}/orders/whatif"),
            Some(&json!({"orders": orders})),
        )
        .await
    }
    pub async fn modify_order(
        &self,
        account_id: &str,
        order_id: i64,
        order: &WebOrder,
    ) -> Result<Value> {
        let body = serde_json::to_value(order).map_err(|e| WebError::Invalid(e.to_string()))?;
        self.post(
            &format!("iserver/account/{account_id}/order/{order_id}"),
            Some(&body),
        )
        .await
    }
    /// Cancel an order (`order_id` -1 cancels all open orders). US futures
    /// need `manual_indicator` and `ext_operator` (CME Rule 536-B);
    /// `manual_cancel_time` is the time a person cancelled (YYYYMMDD-hh:mm:ss).
    pub async fn cancel_order(
        &self,
        account_id: &str,
        order_id: i64,
        manual_indicator: Option<bool>,
        ext_operator: Option<&str>,
        manual_cancel_time: Option<&str>,
    ) -> Result<Value> {
        let query = q(&[
            ("manualIndicator", manual_indicator.map(|b| b.to_string())),
            ("extOperator", ext_operator.map(String::from)),
            ("manualCancelTime", manual_cancel_time.map(String::from)),
        ]);
        self.request(
            Method::DELETE,
            &format!("iserver/account/{account_id}/order/{order_id}"),
            &query,
            None,
        )
        .await
    }
    /// Suppress order reply messages by message id (for example `"o163"`).
    pub async fn suppress_messages(&self, message_ids: &[&str]) -> Result<Value> {
        self.post(
            "iserver/questions/suppress",
            Some(&json!({"messageIds": message_ids})),
        )
        .await
    }
    pub async fn reset_suppressed_messages(&self) -> Result<Value> {
        self.post("iserver/questions/suppress/reset", None).await
    }

    // ---- scanners ------------------------------------------------------------

    pub async fn scanner_params(&self) -> Result<Value> {
        self.get("iserver/scanner/params", &[]).await
    }
    pub async fn scanner_run(
        &self,
        instrument: &str,
        scan_type: &str,
        location: &str,
        filters: &Value,
    ) -> Result<Value> {
        self.post("iserver/scanner/run", Some(&json!({"instrument": instrument, "type": scan_type, "location": location, "filter": filters}))).await
    }
    #[deprecated(
        note = "GET hmds/scanner/params is not in IBKR's current Web API reference; it may stop working; use scanner_params"
    )]
    pub async fn hmds_scanner_params(&self) -> Result<Value> {
        self.get("hmds/scanner/params", &[]).await
    }
    #[deprecated(
        note = "POST hmds/scanner/run is not in IBKR's current Web API reference; it may stop working; use scanner_run"
    )]
    pub async fn hmds_scanner_run(&self, body: &Value) -> Result<Value> {
        self.post("hmds/scanner/run", Some(body)).await
    }

    // ---- watchlists ------------------------------------------------------------

    pub async fn create_watchlist(&self, id: &str, name: &str, rows: &[Value]) -> Result<Value> {
        self.post(
            "iserver/watchlist",
            Some(&json!({"id": id, "name": name, "rows": rows})),
        )
        .await
    }
    pub async fn get_all_watchlists(&self, sc: &str) -> Result<Value> {
        self.get("iserver/watchlists", &q(&[("SC", s(sc))])).await
    }
    pub async fn get_watchlist_information(&self, id: &str) -> Result<Value> {
        self.get("iserver/watchlist", &q(&[("id", s(id))])).await
    }
    pub async fn delete_watchlist(&self, id: &str) -> Result<Value> {
        self.request(
            Method::DELETE,
            "iserver/watchlist",
            &q(&[("id", s(id))]),
            None,
        )
        .await
    }

    // ---- financial advisors ------------------------------------------------------

    pub async fn fa_model_list(&self, body: &Value) -> Result<Value> {
        self.post("fa/model/list", Some(body)).await
    }
    pub async fn fa_model_positions(&self, body: &Value) -> Result<Value> {
        self.post("fa/model/positions", Some(body)).await
    }
    pub async fn fa_model_summary(&self, body: &Value) -> Result<Value> {
        self.post("fa/model/summary", Some(body)).await
    }
    pub async fn fa_model_save(&self, body: &Value) -> Result<Value> {
        self.post("fa/model/save", Some(body)).await
    }
    pub async fn fa_model_invest_divest(&self, body: &Value) -> Result<Value> {
        self.post("fa/model/invest-divest", Some(body)).await
    }
    pub async fn fa_model_invest_divest_positions(&self, body: &Value) -> Result<Value> {
        self.post("fa/model/invest-divest-positions", Some(body))
            .await
    }
    pub async fn fa_model_accounts_details(&self, body: &Value) -> Result<Value> {
        self.post("fa/model/accounts-details", Some(body)).await
    }
    pub async fn fa_model_submit_transfers(&self, body: &Value) -> Result<Value> {
        self.post("fa/model/submit-transfers", Some(body)).await
    }
    pub async fn fa_preset_get(&self, body: &Value) -> Result<Value> {
        self.post("fa/fa-preset/get", Some(body)).await
    }
    pub async fn fa_preset_save(&self, body: &Value) -> Result<Value> {
        self.post("fa/fa-preset/save", Some(body)).await
    }
    pub async fn fa_allocation_groups(&self) -> Result<Value> {
        self.get("iserver/account/allocation/group", &[]).await
    }
    pub async fn fa_allocation_accounts(&self) -> Result<Value> {
        self.get("iserver/account/allocation/accounts", &[]).await
    }
    pub async fn fa_allocation_presets(&self) -> Result<Value> {
        self.get("iserver/account/allocation/presets", &[]).await
    }
    /// `GET iserver/account/allocation/models`: model portfolios usable for allocation.
    pub async fn fa_allocation_models(&self) -> Result<Value> {
        self.get("iserver/account/allocation/models", &[]).await
    }
    /// `POST fa/is-full-master` (`{"reqID", "subscriptionKey"}`).
    pub async fn fa_is_full_master(&self, body: &Value) -> Result<Value> {
        self.post("fa/is-full-master", Some(body)).await
    }
    /// `POST fa/model/cash-analyzer` (`{"reqID", "subscriptionKey"}`).
    pub async fn fa_model_cash_analyzer(&self, body: &Value) -> Result<Value> {
        self.post("fa/model/cash-analyzer", Some(body)).await
    }
    /// `POST fa/model/rebalance/to-existing-targets` (`{"reqID", "model", "subscriptionKey"}`).
    pub async fn fa_model_rebalance_to_existing_targets(&self, body: &Value) -> Result<Value> {
        self.post("fa/model/rebalance/to-existing-targets", Some(body))
            .await
    }
    /// `POST fa/model/rebalance/to-new-targets` (adds `positionTargets`, `cashTargets`).
    pub async fn fa_model_rebalance_to_new_targets(&self, body: &Value) -> Result<Value> {
        self.post("fa/model/rebalance/to-new-targets", Some(body))
            .await
    }
    /// `POST fa/model/rebalance/to-specific-targets` (adds `positionTargets`).
    pub async fn fa_model_rebalance_to_specific_targets(&self, body: &Value) -> Result<Value> {
        self.post("fa/model/rebalance/to-specific-targets", Some(body))
            .await
    }
    /// `POST fa/model/tws-invest-divest` (`{"reqID", "modelList", "account" | "accountList" | "group"}`).
    pub async fn fa_model_tws_invest_divest(&self, body: &Value) -> Result<Value> {
        self.post("fa/model/tws-invest-divest", Some(body)).await
    }

    // ---- notifications (FYI) -------------------------------------------------------

    pub async fn fyi_unread_number(&self) -> Result<Value> {
        self.get("fyi/unreadnumber", &[]).await
    }
    pub async fn fyi_settings(&self) -> Result<Value> {
        self.get("fyi/settings", &[]).await
    }
    pub async fn fyi_disclaimer(&self, typecode: &str) -> Result<Value> {
        self.get(&format!("fyi/disclaimer/{typecode}"), &[]).await
    }
    pub async fn fyi_delivery_options(&self) -> Result<Value> {
        self.get("fyi/deliveryoptions", &[]).await
    }
    pub async fn fyi_notifications(
        &self,
        max: i32,
        include: Option<&str>,
        exclude: Option<&str>,
    ) -> Result<Value> {
        self.get(
            "fyi/notifications",
            &q(&[
                ("max", s(max)),
                ("include", include.map(String::from)),
                ("exclude", exclude.map(String::from)),
            ]),
        )
        .await
    }
    pub async fn fyi_mark_read(&self, notification_id: &str) -> Result<Value> {
        self.request(
            Method::PUT,
            &format!("fyi/notifications/{notification_id}"),
            &[],
            None,
        )
        .await
    }

    pub async fn fyi_notifications_more(&self, id: &str) -> Result<Value> {
        self.get("fyi/notifications/more", &q(&[("id", s(id))]))
            .await
    }
    /// Turn a notification type on or off.
    pub async fn fyi_set_setting(&self, typecode: &str, enabled: bool) -> Result<Value> {
        self.post(
            &format!("fyi/settings/{typecode}"),
            Some(&json!({"enabled": enabled})),
        )
        .await
    }
    pub async fn fyi_mark_disclaimer_read(&self, typecode: &str) -> Result<Value> {
        self.request(
            Method::PUT,
            &format!("fyi/disclaimer/{typecode}"),
            &[],
            None,
        )
        .await
    }
    pub async fn fyi_set_email_delivery(&self, enabled: bool) -> Result<Value> {
        self.request(
            Method::PUT,
            "fyi/deliveryoptions/email",
            &q(&[("enabled", s(enabled))]),
            None,
        )
        .await
    }
    pub async fn fyi_set_device_delivery(
        &self,
        device_name: &str,
        device_id: &str,
        ui_name: &str,
        enabled: bool,
    ) -> Result<Value> {
        self.post(
            "fyi/deliveryoptions/device",
            Some(&json!({"devicename": device_name, "deviceId": device_id, "uiName": ui_name, "enabled": enabled})),
        )
        .await
    }
    pub async fn fyi_delete_device(&self, device_id: &str) -> Result<Value> {
        self.delete(&format!("fyi/deliveryoptions/{device_id}"))
            .await
    }

    // ---- alerts --------------------------------------------------------------------

    /// Create an alert, or modify one when `alert` has `orderId`. The fields
    /// are the documented `alert-request` object (`alertName`, `conditions`, ...).
    pub async fn create_alert(&self, account_id: &str, alert: &Value) -> Result<Value> {
        self.post(&format!("iserver/account/{account_id}/alert"), Some(alert))
            .await
    }
    pub async fn alerts(&self, account_id: &str) -> Result<Value> {
        self.get(&format!("iserver/account/{account_id}/alerts"), &[])
            .await
    }
    pub async fn alert_details(&self, alert_id: i64) -> Result<Value> {
        self.get(
            &format!("iserver/account/alert/{alert_id}"),
            &q(&[("type", s("Q"))]),
        )
        .await
    }
    pub async fn activate_alert(
        &self,
        account_id: &str,
        alert_id: i64,
        active: bool,
    ) -> Result<Value> {
        self.post(
            &format!("iserver/account/{account_id}/alert/activate"),
            Some(&json!({"alertId": alert_id, "alertActive": i32::from(active)})),
        )
        .await
    }
    pub async fn delete_alert(&self, account_id: &str, alert_id: i64) -> Result<Value> {
        self.delete(&format!("iserver/account/{account_id}/alert/{alert_id}"))
            .await
    }
    /// The Mobile Trading Assistant alert.
    pub async fn mta_alert(&self) -> Result<Value> {
        self.get("iserver/account/mta", &[]).await
    }

    // ---- event contracts (ForecastTrader) --------------------------------------------

    pub async fn forecast_category_tree(&self) -> Result<Value> {
        self.get("forecast/category/tree", &[]).await
    }
    pub async fn forecast_market(
        &self,
        underlying_conid: i64,
        exchange: Option<&str>,
    ) -> Result<Value> {
        self.get(
            "forecast/contract/market",
            &q(&[
                ("underlyingConid", s(underlying_conid)),
                ("exchange", exchange.map(String::from)),
            ]),
        )
        .await
    }
    pub async fn forecast_contract_details(&self, conid: i64) -> Result<Value> {
        self.get("forecast/contract/details", &q(&[("conid", s(conid))]))
            .await
    }
    pub async fn forecast_contract_rules(&self, conid: i64) -> Result<Value> {
        self.get("forecast/contract/rules", &q(&[("conid", s(conid))]))
            .await
    }
    pub async fn forecast_contract_schedules(&self, conid: i64) -> Result<Value> {
        self.get("forecast/contract/schedules", &q(&[("conid", s(conid))]))
            .await
    }

    // ---- FA allocation groups --------------------------------------------------------

    /// Add an allocation group: `{"name", "accounts": [{"name", "amount"}], "default_method"}`.
    pub async fn fa_allocation_group_add(&self, group: &Value) -> Result<Value> {
        self.post("iserver/account/allocation/group", Some(group))
            .await
    }
    /// Modify an allocation group (same fields as add, plus `prev_name` to rename).
    pub async fn fa_allocation_group_modify(&self, group: &Value) -> Result<Value> {
        self.request(
            Method::PUT,
            "iserver/account/allocation/group",
            &[],
            Some(group),
        )
        .await
    }
    pub async fn fa_allocation_group(&self, name: &str) -> Result<Value> {
        self.post(
            "iserver/account/allocation/group/single",
            Some(&json!({"name": name})),
        )
        .await
    }
    pub async fn fa_allocation_group_delete(&self, name: &str) -> Result<Value> {
        self.post(
            "iserver/account/allocation/group/delete",
            Some(&json!({"name": name})),
        )
        .await
    }
    pub async fn fa_allocation_presets_set(&self, presets: &Value) -> Result<Value> {
        self.post("iserver/account/allocation/presets", Some(presets))
            .await
    }

    // ---- other ---------------------------------------------------------------------

    /// Market data snapshot without a brokerage session (OAuth only).
    #[deprecated(
        note = "GET md/snapshot is not in IBKR's current Web API reference; it may stop working; use live_marketdata_snapshot"
    )]
    pub async fn md_snapshot(&self, conids: &[i64], fields: &[&str]) -> Result<Value> {
        let conids: Vec<String> = conids.iter().map(ToString::to_string).collect();
        self.get(
            "md/snapshot",
            &q(&[
                ("conids", s(conids.join(","))),
                ("fields", s(fields.join(","))),
            ]),
        )
        .await
    }
    /// `POST pa/allocation`: allocation by `allocation_type` (for example
    /// `ASSET_CLASS`, `SECTOR`), optionally in `currency`, at `date`
    /// (yyyymmdd) or for a model portfolio.
    pub async fn pa_allocation(
        &self,
        account_ids: &[String],
        allocation_type: &str,
        currency: Option<&str>,
        date: Option<&str>,
        model: Option<&str>,
    ) -> Result<Value> {
        let mut body = json!({"acctIds": account_ids, "type": allocation_type});
        for (k, v) in [("currency", currency), ("date", date), ("model", model)] {
            if let Some(v) = v {
                body[k] = json!(v);
            }
        }
        self.post("pa/allocation", Some(&body)).await
    }
    pub async fn pa_summary(&self, account_ids: &[String]) -> Result<Value> {
        self.post("pa/summary", Some(&json!({"acctIds": account_ids})))
            .await
    }
    /// Bond scanner (`POST /hmds/scanner`): `{"instrument", "locations", "scanCode", "secType", "filters"}`.
    #[deprecated(
        note = "POST hmds/scanner is not in IBKR's current Web API reference; it may stop working; use scanner_run"
    )]
    pub async fn hmds_scanner(&self, body: &Value) -> Result<Value> {
        self.post("hmds/scanner", Some(body)).await
    }
    /// Stop market data of one conid (`GET /iserver/marketdata/{conid}/unsubscribe`).
    #[deprecated(
        note = "GET iserver/marketdata/{conid}/unsubscribe is not in IBKR's current Web API reference; it may stop working; use marketdata_unsubscribe"
    )]
    pub async fn marketdata_unsubscribe_conid(&self, conid: i64) -> Result<Value> {
        self.get(&format!("iserver/marketdata/{conid}/unsubscribe"), &[])
            .await
    }
    /// Answer a server prompt from an `ntf` WebSocket message.
    pub async fn server_prompt_reply(
        &self,
        order_id: i64,
        req_id: &str,
        text: &str,
    ) -> Result<Value> {
        self.post(
            "iserver/notification",
            Some(&json!({"orderId": order_id, "reqId": req_id, "text": text})),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn pacer_follows_ibkrs_rate_limits() {
        let mut p = Pacer::default();
        let t0 = Instant::now();
        // Ten requests per second overall; the eleventh waits.
        for _ in 0..10 {
            assert_eq!(p.reserve("GET", "iserver/accounts", t0), Duration::ZERO);
        }
        assert_eq!(
            p.reserve("GET", "iserver/accounts", t0),
            Duration::from_secs(1)
        );
        // One request per 5 s on GET iserver/account/orders.
        let mut p = Pacer::default();
        assert_eq!(
            p.reserve("GET", "/iserver/account/orders", t0),
            Duration::ZERO
        );
        assert_eq!(
            p.reserve("GET", "iserver/account/orders?force=true", t0),
            Duration::from_secs(5)
        );
        assert_eq!(p.reserve("GET", "iserver/accounts", t0), Duration::ZERO);
        // FYI endpoints with ids share one limit; different endpoints do not.
        let mut p = Pacer::default();
        assert_eq!(p.reserve("PUT", "fyi/notifications/1", t0), Duration::ZERO);
        assert_eq!(
            p.reserve("PUT", "fyi/notifications/2", t0),
            Duration::from_secs(1)
        );
        assert_eq!(
            p.reserve("GET", "fyi/notifications/more", t0),
            Duration::ZERO
        );
        // Once the window passes, no wait.
        let mut p = Pacer::default();
        assert_eq!(p.reserve("POST", "iserver/scanner/run", t0), Duration::ZERO);
        let later = t0 + Duration::from_secs(1);
        assert_eq!(
            p.reserve("POST", "iserver/scanner/run", later),
            Duration::ZERO
        );
    }
}
