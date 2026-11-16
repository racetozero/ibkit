#![forbid(unsafe_code)]

pub mod api;
pub mod client;
pub mod config;
pub mod dialog;
pub mod recovery;
pub mod schedule;
pub mod totp;

pub use api::Status;
use chrono::{DateTime, Utc};
pub use config::{Config, Credentials, ExistingSession, IncomingConnection, TradingMode};
pub use dialog::{Action, Dialog, WindowSnapshot};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Values 0..=15 match QuantConnect IBAutomater's ErrorCode values.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub enum ErrorCode {
    #[default]
    None,
    ProcessStartFailed,
    IbGatewayVersionNotInstalled,
    JavaNotFound,
    JavaException,
    LoginFailed,
    ExistingSessionDetected,
    SecurityDialogDetected,
    TwoFactorConfirmationTimeout,
    InitializationTimeout,
    UnsupportedVersion,
    ApiSupportNotAvailable,
    RestartedProcessNotFound,
    UnknownMessageWindowDetected,
    SoftRestartTimeout,
    LoginFailedAccountTasksRequired,
    InvalidConfiguration,
    AgentDisconnected,
    AuthenticationRequired,
    ConnectionLost,
    ApiConnectionUnavailable,
    LogExportTimeout,
    AuthenticationNotAnswered,
    /// IBKR rejected the Mobile Authenticator codes. Never retried: more
    /// wrong codes can lock the account.
    TotpRejected,
    /// Consecutive logins that never reached Ready reached
    /// `max_login_failures`. Never retried automatically.
    LoginRetriesExhausted,
}

impl ErrorCode {
    pub fn message(self) -> &'static str {
        match self {
            Self::None => "Gateway completed the operation.",
            Self::ProcessStartFailed => {
                "Gateway did not start. Select an executable Gateway launcher and check its file permissions."
            }
            Self::IbGatewayVersionNotInstalled => {
                "The selected Gateway installation is absent. Install Gateway and set executable to its launcher path."
            }
            Self::JavaNotFound => {
                "The Gateway launcher did not find Java. Use the runtime supplied with Gateway or set its supported Java path."
            }
            Self::JavaException => {
                "A call to the Gateway UI failed. Use a supported Gateway version and check the agent event log."
            }
            Self::LoginFailed => {
                "Gateway rejected the login. Supply valid IBKR credentials and check the account status before you try again."
            }
            Self::ExistingSessionDetected => {
                "Gateway found an existing session. Close that session or select the explicit takeover policy before you try again."
            }
            Self::SecurityDialogDetected => {
                "Gateway requested a second factor that automatic login cannot complete (security card, passkey, or a code without a configured TOTP secret). Use IB Key approval, set the Mobile Authenticator secret in totp_secret_env or totp_secret_file, set passkey_authenticate with an authenticator, or complete login manually."
            }
            Self::TwoFactorConfirmationTimeout => {
                "Gateway did not receive mobile approval in time. Approve an IB Key request within the configured time and start again."
            }
            Self::InitializationTimeout => {
                "Gateway did not complete setup in time. Check its network access and account tasks, then start again or increase initialization_timeout_secs."
            }
            Self::UnsupportedVersion => {
                "Gateway reported an unsupported version. Install a current standalone Gateway release and start again."
            }
            Self::ApiSupportNotAvailable => {
                "Gateway rejected API access for this account. Use an IBKR account with API support, such as IBKR Pro."
            }
            Self::RestartedProcessNotFound => {
                "Gateway did not report its restart child. Check agent loading in the launcher and start Gateway again."
            }
            Self::UnknownMessageWindowDetected => {
                "Gateway showed an unknown dialog. Review the private dialog report and resolve it manually before you start again."
            }
            Self::SoftRestartTimeout => {
                "Gateway did not complete the requested soft restart in time. Check its restart settings and network access, then start again."
            }
            Self::LoginFailedAccountTasksRequired => {
                "Gateway requires account tasks. Open Gateway manually, complete the tasks, and start again."
            }
            Self::InvalidConfiguration => {
                "The configuration is invalid. Use the documented TOML keys and valid paths, ports, and time limits, then validate again."
            }
            Self::AgentDisconnected => {
                "The Gateway agent stopped reporting. Check the Gateway process and agent path, then start again."
            }
            Self::AuthenticationRequired => {
                "Gateway's restart token expired. Start a new login and approve IB Key on your phone."
            }
            Self::ConnectionLost => {
                "Gateway lost its server connection. Check network access and the maintenance schedule before you start again."
            }
            Self::ApiConnectionUnavailable => {
                "Gateway did not answer API probes within the configured limit. Check its API port and connection settings before you start again."
            }
            Self::LogExportTimeout => {
                "Gateway did not complete log export within 60 seconds. Close the log dialog manually and check the export directory before you start again."
            }
            Self::TotpRejected => {
                "IBKR rejected the Mobile Authenticator codes. Check that the TOTP secret is the one from this user's enrollment and that the host clock is correct (for example with NTP), then send restart."
            }
            Self::LoginRetriesExhausted => {
                "Consecutive logins failed max_login_failures times without reaching Ready, so automatic retries stopped to protect the account from a lock-out. Read the earlier errors, correct the cause, then send restart."
            }
            Self::AuthenticationNotAnswered => {
                "IBKR did not answer the login request. Usually another TWS, Gateway, or IBKR Mobile session holds this user's session, or an unclean stop left one open. Stop other sessions, log in to IBKR Mobile as this user to end them, then start again."
            }
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
pub struct StartResult {
    pub error_code: ErrorCode,
    pub error_message: String,
}

impl StartResult {
    pub fn new(error_code: ErrorCode) -> Self {
        Self {
            error_code,
            error_message: error_code.message().into(),
        }
    }
    pub fn has_error(&self) -> bool {
        self.error_code != ErrorCode::None
    }
}

/// Controller and Gateway state. `failed`: Gateway stopped on a fault in
/// one session, and recovery may follow. `halted`: Gateway is stopped after
/// a fault that automatic recovery must not retry; the controller stays up
/// and waits for restart, reload or stop. `paused`: an operator (the `pause`
/// command or `closedown_at`) stopped Gateway; the controller stays up,
/// also across its own restarts, until `restart`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    #[default]
    Starting,
    LoggingIn,
    AwaitingSecondFactor,
    Configuring,
    Ready,
    Restarting,
    Stopping,
    Exited,
    Failed,
    Recovering,
    Halted,
    Paused,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EventKind {
    Started,
    StateChanged {
        state: State,
    },
    Output {
        message: String,
    },
    Error {
        result: StartResult,
    },
    Restarted,
    Exited {
        code: Option<i32>,
    },
    Dialog {
        kind: Dialog,
        title: String,
    },
    RecoveryScheduled {
        attempt: u32,
        retry_at: DateTime<Utc>,
        reason: ErrorCode,
    },
    ApiUnavailable,
    ApiRestored,
    /// Gateway stopped and the controller waits: for an operator after a
    /// fault that a new login cannot repair, or for the hourly attempt
    /// budget (`exhausted`).
    Halted {
        reason: ErrorCode,
        exhausted: bool,
        message: String,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct Event {
    pub at: DateTime<Utc>,
    pub session: String,
    pub pid: u32,
    #[serde(flatten)]
    pub kind: EventKind,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentLaunch {
    pub session: String,
    pub config: Config,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct AgentStatus {
    pub session: String,
    pub pid: u32,
    pub at: DateTime<Utc>,
    pub state: State,
    pub result: StartResult,
    /// The JVM's maximum heap (Runtime.maxMemory) in MiB.
    pub max_heap_mb: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Control {
    Stop,
    Restart,
    SoftRestart,
    ExportLogs,
    /// Reconnect Gateway's account (server) connection without a restart.
    ReconnectAccount,
    /// Reconnect market data farms (TWS; Gateway ignores it).
    ReconnectData,
    /// Read the configuration again and start a new session with it.
    Reload,
    /// Stop Gateway and keep the controller up until `restart`, also after
    /// the controller itself restarts (for example a container restart).
    Pause,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Command {
    pub session: String,
    pub control: Control,
}
