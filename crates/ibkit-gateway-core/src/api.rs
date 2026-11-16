//! Request and response bodies of the control API (`/v1`). The server is in
//! the `ibkit-gateway` crate; the client is the `ibkit-gateway-client` crate.
use crate::{AgentStatus, Control, StartResult, State, recovery::RecoveryState};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Controller status, saved to `state_dir/status.json` every 500 ms and
/// returned by `GET /v1/status`.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct Status {
    pub session: String,
    pub controller_pid: u32,
    pub at: DateTime<Utc>,
    pub state: State,
    pub agents: Vec<AgentStatus>,
    pub last_start_result: StartResult,
    /// Automatic recovery phase. With state `halted`, `blocked` waits for an
    /// operator and `exhausted` for the hourly attempt budget.
    pub recovery: Option<RecoveryState>,
    /// Process group of the Gateway launcher that this controller started.
    /// A replacement controller only tests whether the group still exists.
    #[serde(default)]
    pub process_group: Option<u32>,
}

impl Status {
    /// The controller runs: it saved this status in the last 5 seconds (it
    /// saves every 500 ms) and has not exited. A halted controller is live.
    pub fn is_live(&self) -> bool {
        (Utc::now() - self.at).num_seconds() <= 5 && self.state != State::Exited
    }
}

/// `GET /v1/health` and `GET /v1/health/ready` (no token needed).
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct Health {
    /// The controller runs and saved its status in the last 5 seconds.
    pub live: bool,
    /// Gateway is logged in and its API socket answered a handshake.
    pub ready: bool,
    pub state: State,
    pub controller_pid: u32,
}

/// `POST /v1/control`.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRequest {
    pub control: Control,
}

/// `PATCH /v1/config`: configuration keys and their new values. `null`
/// removes a value set through the API, so the file or default applies.
pub type ConfigUpdate = BTreeMap<String, Option<toml::Value>>;

/// `PUT /v1/credentials`.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialsRequest {
    pub username: String,
    pub password: String,
}

/// `PUT /v1/totp`: the base32 Mobile Authenticator secret.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TotpRequest {
    pub secret: String,
}

/// The body of every error response.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ApiError {
    pub error: String,
}

/// Why an alert was sent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    /// A login or a running Gateway failed. Automatic recovery may follow.
    Down,
    /// Gateway is logged in again after `down`, `halted` or `exhausted`.
    Recovered,
    /// Gateway stopped after a fault that a new login cannot repair. An
    /// operator must correct the cause and send `restart`.
    Halted,
    /// The hourly recovery budget is used up; recovery continues when it
    /// permits.
    Exhausted,
    /// The controller itself stopped with an error.
    ControllerFailed,
}

/// The JSON body that the controller POSTs to `alert_webhook_url`.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct Alert {
    pub alert: AlertKind,
    /// One readable line, for chat webhooks (Slack, Mattermost).
    pub text: String,
    pub at: DateTime<Utc>,
    pub session: String,
    pub state: State,
    pub reason: crate::ErrorCode,
    pub message: String,
}
