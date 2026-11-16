use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    env, fmt, fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TradingMode {
    Live,
    #[default]
    Paper,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExistingSession {
    #[default]
    Refuse,
    Takeover,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IncomingConnection {
    #[default]
    Manual,
    Accept,
    Reject,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub executable: PathBuf,
    pub agent: PathBuf,
    pub state_dir: PathBuf,
    pub settings_dir: PathBuf,
    pub trading_mode: TradingMode,
    pub auto_login: bool,
    pub api_port: u16,
    pub read_only_api: bool,
    pub api_log: bool,
    pub allow_remote_api: bool,
    pub incoming_connection: IncomingConnection,
    pub api_listen: Option<SocketAddr>,
    pub auto_recover: bool,
    /// Retry limit for automatic recovery; 0 retries without limit.
    pub recovery_attempts: u32,
    /// First retry delay; each further retry doubles it up to
    /// `recovery_max_delay_secs`.
    pub recovery_delay_secs: u64,
    pub recovery_max_delay_secs: u64,
    pub api_probe_interval_secs: u64,
    pub api_failure_timeout_secs: u64,
    pub region: crate::schedule::Region,
    pub bypass_order_precautions: bool,
    pub accept_order_warnings: bool,
    pub existing_session: ExistingSession,
    pub export_logs: bool,
    /// 24-hour local system time. The container sets TZ explicitly.
    pub restart_time: String,
    pub initialization_timeout_secs: u64,
    pub soft_restart_timeout_secs: u64,
    pub shutdown_timeout_secs: u64,
    pub second_factor_timeout_secs: u64,
    pub second_factor_attempts: u8,
    /// How long an unknown modal dialog may block authentication before
    /// the login fails. Unknown dialogs are never clicked.
    pub unknown_dialog_timeout_secs: u64,
    pub username_env: String,
    pub password_env: String,
    pub username_file: Option<PathBuf>,
    pub password_file: Option<PathBuf>,
    /// Environment variable with the base32 Mobile Authenticator (TOTP)
    /// secret. When it is absent and no file is set, second-factor login
    /// waits for IB Key approval on the phone.
    pub totp_secret_env: String,
    pub totp_secret_file: Option<PathBuf>,
    /// Press Authenticate on a passkey (WebAuthn) prompt. An authenticator
    /// beside Gateway must complete it; otherwise a passkey prompt fails.
    pub passkey_authenticate: bool,
    /// Maximum Java heap in MiB (Gateway's default is 768).
    pub java_heap_mb: Option<u32>,
    /// Master API client ID: the client that receives all orders.
    pub master_client_id: Option<u32>,
    pub manage_display: bool,
    /// Control API (REST and WebSocket) listener; `None` disables it.
    pub control_listen: Option<SocketAddr>,
    /// Environment variable with the control API bearer token.
    pub control_token_env: String,
    pub control_token_file: Option<PathBuf>,
    /// HTTP(S) URL that receives a JSON POST when Gateway goes down, comes
    /// back, halts for an operator, or the controller fails. The body has
    /// a `text` field, so Slack and Mattermost incoming webhooks accept it.
    pub alert_webhook_url: Option<String>,
    /// Click OK on "Your password will expire in N day(s)" and report it as
    /// an event. Off: the warning fails the login, as before IBC's
    /// DismissPasswordExpiryWarning.
    pub dismiss_password_expiry_warning: bool,
    /// Consecutive logins that end without Ready, for a fault where another
    /// login can lock the account (an unknown login dialog, unanswered or
    /// rejected second factor), before retries stop; 0 has no limit.
    pub max_login_failures: u32,
    /// Comma-separated source addresses or CIDR ranges that may connect to
    /// `api_listen`, for example `10.0.0.0/8,192.168.1.20`. Absent: any.
    /// Behind Docker port publishing the source can be the bridge address.
    pub api_allowed_sources: Option<String>,
    /// Let the control API set risky values: live trading, remote API
    /// access, accepting incoming connections, a writable API, bypassed
    /// order precautions, and `api_listen`. Only the file, flags and
    /// variables can turn this on.
    pub control_api_full_access: bool,
    /// Serve the virtual display over VNC (x11vnc) on this address, for
    /// account tasks and unknown dialogs. Needs `manage_display` and
    /// `vnc_password_file`. Keep it on loopback and use a tunnel.
    pub vnc_listen: Option<SocketAddr>,
    pub vnc_password_file: Option<PathBuf>,
    /// Weekly time (local system time) at which the controller pauses
    /// Gateway, for example `Fri 23:00` (IBC ClosedownAt).
    pub closedown_at: Option<String>,
    /// Weekly time (local system time) of a cold restart with a new login,
    /// for example `Sun 07:05` (IBC ColdRestartTime): it moves the weekly
    /// re-authentication to a time when an operator can approve it.
    pub cold_restart_at: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // The controller replaces these when no file is there and it
            // finds Gateway or the agent elsewhere (settings::fill_paths).
            executable: if cfg!(windows) {
                r"C:\Jts\ibgateway\ibgateway.exe"
            } else {
                "/opt/ibgateway/ibgateway"
            }
            .into(),
            agent: if cfg!(target_os = "macos") {
                "target/release/libibkit_gateway_agent.dylib"
            } else if cfg!(windows) {
                "ibkit_gateway_agent.dll"
            } else {
                "/usr/local/lib/libibkit_gateway_agent.so"
            }
            .into(),
            state_dir: "state".into(),
            settings_dir: "state/settings".into(),
            trading_mode: TradingMode::Paper,
            auto_login: true,
            api_port: 4002,
            read_only_api: false,
            api_log: true,
            allow_remote_api: false,
            incoming_connection: IncomingConnection::Manual,
            api_listen: None,
            auto_recover: true,
            recovery_attempts: 0,
            recovery_delay_secs: 60,
            recovery_max_delay_secs: 1800,
            api_probe_interval_secs: 30,
            api_failure_timeout_secs: 180,
            region: crate::schedule::Region::America,
            bypass_order_precautions: true,
            accept_order_warnings: true,
            existing_session: ExistingSession::Refuse,
            export_logs: false,
            restart_time: "23:45".into(),
            initialization_timeout_secs: 900,
            soft_restart_timeout_secs: 600,
            shutdown_timeout_secs: 30,
            second_factor_timeout_secs: 150,
            second_factor_attempts: 3,
            unknown_dialog_timeout_secs: 120,
            username_env: "IB_USERNAME".into(),
            password_env: "IB_PASSWORD".into(),
            username_file: None,
            password_file: None,
            totp_secret_env: "IB_TOTP_SECRET".into(),
            totp_secret_file: None,
            passkey_authenticate: false,
            java_heap_mb: None,
            master_client_id: None,
            manage_display: false,
            control_listen: None,
            control_token_env: "IBKIT_CONTROL_TOKEN".into(),
            control_token_file: None,
            alert_webhook_url: None,
            dismiss_password_expiry_warning: true,
            max_login_failures: 10,
            api_allowed_sources: None,
            control_api_full_access: false,
            vnc_listen: None,
            vnc_password_file: None,
            closedown_at: None,
            cold_restart_at: None,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        Self::load_with(Some(path), &toml::Table::new())
    }

    /// Read the TOML file at `path` (none: all defaults), apply `overrides`
    /// (from CLI flags or IBKIT_* variables) over it, and validate the result.
    pub fn load_with(path: Option<&Path>, overrides: &toml::Table) -> Result<Self> {
        let mut table: toml::Table = match path {
            Some(path) => toml::from_str(&fs::read_to_string(path).with_context(|| format!(
                "Cannot read the config file {}. Supply a readable TOML file with --config, or omit --config and give settings as flags.",
                path.display()
            ))?)
            .context("Cannot parse the selected config. Use the keys and value types in config.example.toml (in the repository at crates/ibkit-gateway/, in the image at /etc/ibkit/config.toml) or `ibkit gateway --help`.")?,
            None => toml::Table::new(),
        };
        for (key, value) in overrides {
            // An empty array (no key has an array value) removes the key, so
            // an empty flag or variable clears a value set in the file.
            if value.as_array().is_some_and(Vec::is_empty) {
                table.remove(key);
            } else {
                table.insert(key.clone(), value.clone());
            }
        }
        let mut config: Self = table.try_into().context("A setting has an unknown key or a wrong value type. Use the keys and value types in config.example.toml (in the repository at crates/ibkit-gateway/, in the image at /etc/ibkit/config.toml) or `ibkit gateway --help`.")?;
        config.validate()?;
        config.resolve_paths()?;
        Ok(config)
    }

    pub fn resolve_paths(&mut self) -> Result<()> {
        let base = env::current_dir()?;
        for value in [
            &mut self.executable,
            &mut self.agent,
            &mut self.state_dir,
            &mut self.settings_dir,
        ] {
            if value.is_relative() {
                *value = base.join(&value);
            }
        }
        for value in [
            &mut self.username_file,
            &mut self.password_file,
            &mut self.totp_secret_file,
            &mut self.control_token_file,
            &mut self.vnc_password_file,
        ]
        .into_iter()
        .flatten()
        {
            if value.is_relative() {
                *value = base.join(&value);
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        if self
            .api_listen
            .is_some_and(|address| address.port() == 0 || address.port() == self.api_port)
        {
            bail!(
                "You selected an invalid API forwarding port. Use a port from 1 to 65535 different from api_port, then validate again."
            );
        }
        if self.max_login_failures > 1000 {
            bail!(
                "You set max_login_failures to {}. Use 0 (no limit) to 1000 consecutive failed logins, then validate again.",
                self.max_login_failures
            );
        }
        if self.recovery_attempts > 100_000
            || self.recovery_delay_secs == 0
            || self.recovery_delay_secs > 3600
            || self.recovery_max_delay_secs < self.recovery_delay_secs
            || self.recovery_max_delay_secs > 86400
            || self.api_probe_interval_secs == 0
            || self.api_probe_interval_secs > 3600
            || self.api_failure_timeout_secs < self.api_probe_interval_secs
            || self.api_failure_timeout_secs > 86400
        {
            bail!(
                "You supplied invalid recovery limits. Use 0 (no limit) to 100000 attempts, a delay and probe interval from 1 to 3600 seconds, a maximum delay from the delay to 86400 seconds, and a failure timeout from the probe interval to 86400 seconds."
            );
        }
        if let Some(address) = self.control_listen
            && (address.port() == 0
                || address.port() == self.api_port
                || self
                    .api_listen
                    .is_some_and(|api| api.port() == address.port()))
        {
            bail!(
                "You selected an invalid control_listen port. Use a port from 1 to 65535 that differs from api_port and api_listen, then validate again."
            );
        }
        if let Some(url) = &self.alert_webhook_url
            && (!(url.starts_with("https://") || url.starts_with("http://"))
                || url.len() < 10
                || url.contains(char::is_whitespace))
        {
            bail!(
                "You set alert_webhook_url to a value that is not an HTTP(S) URL. Use a URL such as https://hooks.example.com/ibkit, or remove the key to turn alerts off."
            );
        }
        if self.api_port == 0 {
            bail!("You set api_port to 0. Use a port from 1 to 65535 and validate again.");
        }
        self.allowed_sources()?;
        if cfg!(windows) && (self.manage_display || self.vnc_listen.is_some()) {
            bail!(
                "You set manage_display or vnc_listen on Windows. Gateway uses the Windows desktop there; remove both keys (they start Xvfb and x11vnc on Linux)."
            );
        }
        if self.vnc_listen.is_some() && (!self.manage_display || self.vnc_password_file.is_none()) {
            bail!(
                "You set vnc_listen without manage_display = true and a vnc_password_file. Set both (a VNC server without a password would expose the Gateway screen), or remove vnc_listen."
            );
        }
        for (key, value) in [
            ("closedown_at", &self.closedown_at),
            ("cold_restart_at", &self.cold_restart_at),
        ] {
            if let Some(value) = value {
                crate::schedule::Weekly::parse(value).with_context(|| {
                    format!("You set {key} = {value:?}. Use a weekday and a 24-hour time, for example \"Sun 07:05\", or remove the key.")
                })?;
            }
        }
        let bytes = self.restart_time.as_bytes();
        if bytes.len() != 5
            || bytes[2] != b':'
            || !bytes[..2].iter().chain(&bytes[3..]).all(u8::is_ascii_digit)
            || self.restart_time[..2].parse::<u8>().unwrap_or(255) > 23
            || self.restart_time[3..].parse::<u8>().unwrap_or(255) > 59
        {
            bail!(
                "You supplied an invalid restart_time. Use HH:MM from 00:00 to 23:59 and validate again."
            );
        }
        if self.second_factor_attempts == 0
            || self.second_factor_attempts > 3
            || self.second_factor_timeout_secs == 0
            || self.second_factor_timeout_secs > 180
        {
            bail!(
                "You supplied invalid second factor limits. Use 1 to 3 attempts and 1 to 180 seconds, then validate again."
            );
        }
        if self.initialization_timeout_secs == 0
            || self.initialization_timeout_secs > 86400
            || self.soft_restart_timeout_secs < 120
            || self.soft_restart_timeout_secs > 86400
            || self.shutdown_timeout_secs == 0
            || self.shutdown_timeout_secs > 3600
        {
            bail!(
                "You supplied invalid time limits. Use 1 to 86400 seconds for initialization, 120 to 86400 seconds for soft restart, and 1 to 3600 seconds for shutdown."
            );
        }
        for path in [
            &self.agent,
            &self.state_dir,
            &self.settings_dir,
            &self.executable,
        ] {
            if path.as_os_str().is_empty()
                || path.to_str().is_none()
                || path.to_string_lossy().contains(['\n', '\r', '\0'])
            {
                bail!(
                    "You supplied an empty or invalid path. Use a nonempty UTF-8 path without control characters and validate again."
                );
            }
        }
        if !(1..=3600).contains(&self.unknown_dialog_timeout_secs) {
            bail!(
                "You supplied an invalid unknown_dialog_timeout_secs. Use 1 to 3600 seconds and validate again."
            );
        }
        if self
            .java_heap_mb
            .is_some_and(|mb| !(256..=65536).contains(&mb))
        {
            bail!(
                "You supplied an invalid java_heap_mb. Use 256 to 65536 MiB, or remove the key for Gateway's default, then validate again."
            );
        }
        for name in [
            &self.username_env,
            &self.password_env,
            &self.totp_secret_env,
            &self.control_token_env,
        ] {
            if name.is_empty() || name.contains(['=', '\0']) {
                bail!(
                    "You supplied an invalid credential environment name. Use a nonempty name without '=' or NUL and validate again."
                );
            }
        }
        Ok(())
    }

    pub fn credentials(&self) -> Result<Credentials> {
        if !self.auto_login {
            return Ok(Credentials {
                username: String::new(),
                password: String::new(),
            });
        }
        let read = |file: &Option<PathBuf>, name: &str| -> Result<String> {
            let value = if let Some(path) = file {
                fs::read_to_string(path).with_context(|| format!("Cannot read the credential file {}. Mount a readable UTF-8 file there, or correct username_file/password_file.", path.display()))?.trim_end_matches(['\r', '\n']).to_owned()
            } else {
                env::var(name).with_context(|| format!("The credential variable {name} is not set. Set {} and {} (the username_env and password_env keys name them), set username_file and password_file, or PUT /v1/credentials.", self.username_env, self.password_env))?
            };
            if value.is_empty() || value.contains(['\n', '\r', '\0']) {
                bail!(
                    "You supplied an empty or multiline credential. Use one nonempty line per credential and start again."
                );
            }
            Ok(value)
        };
        Ok(Credentials {
            username: read(&self.username_file, &self.username_env)?,
            password: read(&self.password_file, &self.password_env)?,
        })
    }
}

impl Config {
    /// Bearer token of the control API, from `control_token_file` or the
    /// `control_token_env` variable.
    pub fn control_token(&self) -> Result<String> {
        let token = match &self.control_token_file {
            Some(path) => fs::read_to_string(path).context("Cannot read control_token_file. Mount a readable file that holds the control API token.")?,
            None => env::var(&self.control_token_env).with_context(|| format!(
                "The control API is enabled but has no token. Set {} to a random string of at least 16 characters (for example `openssl rand -hex 32`), or set control_token_file.",
                self.control_token_env
            ))?,
        };
        let token = token.trim().to_owned();
        if token.len() < 16 || token.contains(char::is_whitespace) {
            bail!(
                "The control API token is shorter than 16 characters or contains spaces. Use a random string of at least 16 characters without spaces, for example `openssl rand -hex 32`."
            );
        }
        Ok(token)
    }

    /// The parsed `api_allowed_sources`: (network, prefix length) pairs, or
    /// `None` when every source may connect.
    pub fn allowed_sources(&self) -> Result<Option<Vec<(std::net::IpAddr, u8)>>> {
        let Some(list) = &self.api_allowed_sources else {
            return Ok(None);
        };
        list.split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(|item| {
                let (address, prefix) = item.split_once('/').unwrap_or((item, ""));
                let address: std::net::IpAddr = address.parse().ok()?;
                let full = if address.is_ipv4() { 32 } else { 128 };
                let prefix = if prefix.is_empty() { full } else { prefix.parse().ok()? };
                (prefix <= full).then_some((address, prefix))
            })
            .collect::<Option<Vec<_>>>()
            .map(Some)
            .with_context(|| format!(
                "You set api_allowed_sources = {list:?}. Use comma-separated addresses or CIDR ranges, for example \"127.0.0.1,10.0.0.0/8,fd00::/8\", or remove the key to allow every source."
            ))
    }

    /// Decoded Mobile Authenticator secret, or `None` for IB Key approval.
    pub fn totp_key(&self) -> Result<Option<Vec<u8>>> {
        if !self.auto_login {
            return Ok(None);
        }
        let secret = if let Some(path) = &self.totp_secret_file {
            fs::read_to_string(path).context("Cannot read the TOTP secret file. Mount a readable file with the base32 secret or remove totp_secret_file.")?
        } else {
            match env::var(&self.totp_secret_env) {
                Ok(value) => value,
                Err(env::VarError::NotPresent) => return Ok(None),
                Err(env::VarError::NotUnicode(_)) => bail!(
                    "The TOTP secret variable is not UTF-8. Set it to the base32 secret from IBKR's Mobile Authenticator enrollment."
                ),
            }
        };
        let secret = secret.trim();
        if secret.is_empty() {
            return Ok(None);
        }
        crate::totp::decode_secret(secret).map(Some)
    }
}

pub struct Credentials {
    pub username: String,
    pub password: String,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Credentials([REDACTED])")
    }
}

impl Credentials {
    pub fn redact(&self, input: &str) -> String {
        let (first, second) = if self.username.len() >= self.password.len() {
            (&self.username, &self.password)
        } else {
            (&self.password, &self.username)
        };
        let mut result = String::with_capacity(input.len());
        let mut rest = input;
        while !rest.is_empty() {
            if let Some(secret) = [first, second]
                .into_iter()
                .find(|value| !value.is_empty() && rest.starts_with(value.as_str()))
            {
                result.push_str("[REDACTED]");
                rest = &rest[secret.len()..];
            } else {
                let value = rest.chars().next().unwrap();
                result.push(value);
                rest = &rest[value.len_utf8()..];
            }
        }
        result
    }
}
