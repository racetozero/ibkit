//! Client for the `ibkit` control API: health, status, commands,
//! configuration, credentials and the event stream.
//!
//! ```no_run
//! # async fn run() -> anyhow::Result<()> {
//! use ibkit_gateway_client::{Control, GatewayClient};
//! let client = GatewayClient::new("http://127.0.0.1:5800", std::env::var("IBKIT_CONTROL_TOKEN")?)?;
//! println!("{:?}", client.status().await?.state);
//! client.control(Control::ReconnectAccount).await?;
//! let mut events = client.events().await?;
//! while let Some(event) = events.next().await? {
//!     println!("{:?}", event.kind);
//! }
//! # Ok(()) }
//! ```
#![forbid(unsafe_code)]
use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
pub use ibkit_gateway_core::{
    Config, Control, ErrorCode, Event, EventKind, State,
    api::{
        Alert, AlertKind, ApiError, ConfigUpdate, ControlRequest, CredentialsRequest, Health,
        Status, TotpRequest,
    },
};
use reqwest::{Method, StatusCode};
use serde::{Serialize, de::DeserializeOwned};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest, http::HeaderValue},
};

/// The control API answered with an error status. Errors from this client
/// carry it, so `error.downcast_ref::<StatusError>()` gives the status.
#[derive(Debug, thiserror::Error)]
#[error("The control API answered {status}: {message}")]
pub struct StatusError {
    pub status: u16,
    pub message: String,
}

/// The control API or its event stream cannot be reached. Errors from this
/// client carry it as context: test with `error.is::<ConnectError>()`.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ConnectError(pub String);

#[derive(Clone, Debug)]
pub struct GatewayClient {
    base: String,
    token: String,
    http: reqwest::Client,
}

impl GatewayClient {
    /// `base_url` is the control API root, for example
    /// `http://127.0.0.1:5800`; `token` is its bearer token.
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Result<Self> {
        let base = base_url.into().trim_end_matches('/').to_owned();
        if !(base.starts_with("http://") || base.starts_with("https://")) {
            bail!(
                "You gave the control API address {base:?}. Use an http:// or https:// URL, for example http://127.0.0.1:5800."
            );
        }
        Ok(Self {
            base,
            token: token.into(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
        })
    }

    /// Send a request; `accept` lists the error statuses whose response is
    /// returned instead of an error.
    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
        accept: &[StatusCode],
    ) -> Result<reqwest::Response> {
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(&self.token);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .await
            .with_context(|| ConnectError(format!("Cannot reach the control API at {}. Check that the controller runs with control_listen set to this address.", self.base)))?;
        let status = response.status();
        if status.is_success() || accept.contains(&status) {
            return Ok(response);
        }
        let text = response.text().await.unwrap_or_default();
        let message = serde_json::from_str::<ApiError>(&text).map_or(text, |error| error.error);
        Err(StatusError {
            status: status.as_u16(),
            message,
        }
        .into())
    }

    /// Send a request and decode its JSON body (`None` when it is empty).
    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
        accept: &[StatusCode],
    ) -> Result<Option<T>> {
        let bytes = self.send(method, path, body, accept).await?.bytes().await?;
        if bytes.is_empty() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_slice(&bytes)?))
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, accept: &[StatusCode]) -> Result<T> {
        self.call(Method::GET, path, None::<&()>, accept)
            .await?
            .context("The control API returned an empty body")
    }

    /// Controller liveness. A halted controller is live.
    pub async fn health(&self) -> Result<Health> {
        self.get("/v1/health", &[StatusCode::SERVICE_UNAVAILABLE])
            .await
    }

    /// Readiness: logged in, and the IB API socket answers a handshake.
    pub async fn ready(&self) -> Result<Health> {
        self.get("/v1/health/ready", &[StatusCode::SERVICE_UNAVAILABLE])
            .await
    }

    pub async fn status(&self) -> Result<Status> {
        self.get("/v1/status", &[]).await
    }

    /// Prometheus text metrics.
    pub async fn metrics(&self) -> Result<String> {
        let response = self
            .send(Method::GET, "/v1/metrics", None::<&()>, &[])
            .await?;
        Ok(response.text().await?)
    }

    /// The OpenAPI 3.1 document of the control API.
    pub async fn openapi(&self) -> Result<serde_json::Value> {
        self.get("/v1/openapi.json", &[]).await
    }

    /// Queue a command: stop, restart, soft_restart, export_logs,
    /// reconnect_account, reconnect_data or reload.
    pub async fn control(&self, control: Control) -> Result<()> {
        self.call::<()>(
            Method::POST,
            "/v1/control",
            Some(&ControlRequest { control }),
            &[],
        )
        .await
        .map(drop)
    }

    /// The effective configuration.
    pub async fn config(&self) -> Result<Config> {
        self.get("/v1/config", &[]).await
    }

    /// Save settings (`None` removes an API value) and start a new session
    /// with them. Returns the new effective configuration.
    pub async fn update_config(&self, update: &ConfigUpdate) -> Result<Config> {
        self.call(Method::PATCH, "/v1/config", Some(update), &[])
            .await?
            .context("The control API returned an empty body")
    }

    /// Remove every setting saved through the API (the file, flags and
    /// variables apply again) and reload. Returns the new settings.
    pub async fn clear_config(&self) -> Result<Config> {
        self.call(Method::DELETE, "/v1/config", None::<&()>, &[])
            .await?
            .context("The control API returned an empty body")
    }

    /// Save the IBKR username and password in private files in state_dir.
    pub async fn set_credentials(&self, username: &str, password: &str) -> Result<()> {
        let body = CredentialsRequest {
            username: username.into(),
            password: password.into(),
        };
        self.call::<()>(Method::PUT, "/v1/credentials", Some(&body), &[])
            .await
            .map(drop)
    }

    /// Save the base32 Mobile Authenticator secret; codes are then entered
    /// automatically at login.
    pub async fn set_totp_secret(&self, secret: &str) -> Result<()> {
        let body = TotpRequest {
            secret: secret.into(),
        };
        self.call::<()>(Method::PUT, "/v1/totp", Some(&body), &[])
            .await
            .map(drop)
    }

    /// Remove the saved secret and return to IB Key approval on the phone.
    pub async fn clear_totp_secret(&self) -> Result<()> {
        self.call::<()>(Method::DELETE, "/v1/totp", None::<&()>, &[])
            .await
            .map(drop)
    }

    /// Open the event stream. Events start from the moment of connection.
    pub async fn events(&self) -> Result<Events> {
        let url = format!("ws{}/v1/events", &self.base["http".len()..]);
        let mut request = url.into_client_request()?;
        request.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {}", self.token))?,
        );
        let (socket, _) = tokio_tungstenite::connect_async(request)
            .await
            .context(ConnectError(
                "Cannot open the control API event stream. Check the address and the bearer token."
                    .into(),
            ))?;
        Ok(Events(socket))
    }
}

/// Controller events, one per WebSocket message.
pub struct Events(WebSocketStream<MaybeTlsStream<TcpStream>>);

impl Events {
    /// The next event, or `None` when the controller closed the stream.
    pub async fn next(&mut self) -> Result<Option<Event>> {
        while let Some(message) = self.0.next().await {
            match message? {
                Message::Text(text) => return Ok(Some(serde_json::from_str(&text)?)),
                Message::Ping(data) => self.0.send(Message::Pong(data)).await?,
                Message::Close(_) => return Ok(None),
                _ => {}
            }
        }
        Ok(None)
    }

    pub async fn close(mut self) -> Result<()> {
        self.0.close(None).await?;
        Ok(())
    }
}
