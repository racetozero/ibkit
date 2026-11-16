//! Metadata for every configuration key. The TUI form, the CLI flags and
//! the IBKIT_* environment overrides use the same keys, help and conversion.
use crate::Config;
use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::LazyLock,
};

/// Settings changed through the control API, in `state_dir`. They apply
/// over the config file and under the CLI flags and IBKIT_* variables.
pub const API_SETTINGS: &str = "api-settings.toml";

/// The effective configuration: defaults, then the file at `path`, then
/// the settings saved through the control API, then `flags`.
pub fn load(path: Option<&Path>, flags: &toml::Table) -> Result<Config> {
    let mut base = Config::load_with(path, flags)?;
    let saved = read_api_settings(&base.state_dir)?;
    if saved.is_empty() {
        fill_paths(&mut base);
        return Ok(base);
    }
    let mut config = merge(path, &saved, flags).with_context(|| format!(
        "A setting saved through the control API is no longer valid. Correct it with PATCH /v1/config, or remove {}.",
        base.state_dir.join(API_SETTINGS).display()
    ))?;
    fill_paths(&mut config);
    Ok(config)
}

/// Replace `executable` and `agent` when they still hold the built-in
/// default: the launcher when no file is there, with the newest Gateway
/// found on this machine, and the agent with the one next to this binary.
/// A path that the user set is never changed.
pub fn fill_paths(config: &mut Config) {
    let defaults = Config::default();
    // A relative default (the macOS agent) is resolved against the working
    // directory when the config loads.
    let is_default = |path: &Path, default: &Path| {
        path == default || (default.is_relative() && path.ends_with(default))
    };
    if is_default(&config.executable, &defaults.executable)
        && !config.executable.is_file()
        && let Some(found) = find_launchers().into_iter().next()
    {
        config.executable = found;
    }
    // The agent must come from the same build as this binary (they share
    // the launch file format), so the one next to it wins over the default
    // even when the default file exists (an older target/release build).
    if is_default(&config.agent, &defaults.agent)
        && let Some(found) = find_agent()
    {
        config.agent = found;
    }
}

/// Installed Gateway launchers, newest version first. IBKR's installers
/// put Gateway in `~/Applications/IB Gateway <version>/` on macOS (also
/// `/Applications`), in `~/Jts/ibgateway/<version>/` on Linux, and in
/// `C:\Jts\ibgateway\<version>\ibgateway.exe` on Windows; the container
/// image has it in `/opt/ibgateway`.
pub fn find_launchers() -> Vec<PathBuf> {
    launchers_in(std::env::home_dir())
}

/// A place to look for Gateway: the root directory, the prefix of its
/// version directories, and the launcher inside a version directory.
type InstallRoot = (PathBuf, &'static str, fn(&str) -> PathBuf);

fn launchers_in(home: Option<PathBuf>) -> Vec<PathBuf> {
    let roots: Vec<InstallRoot> = if cfg!(target_os = "macos") {
        let app = |name: &str| {
            PathBuf::from(format!("{name}.app")).join("Contents/MacOS/JavaApplicationStub")
        };
        home.iter()
            .map(|home| home.join("Applications"))
            .chain([PathBuf::from("/Applications")])
            .map(|root| (root, "IB Gateway ", app as fn(&str) -> PathBuf))
            .collect()
    } else if cfg!(windows) {
        // IBC and IBAutomater: %SYSTEMDRIVE%\Jts\ibgateway\<version>\ibgateway.exe.
        let drive = std::env::var("SYSTEMDRIVE").unwrap_or_else(|_| "C:".into());
        [PathBuf::from(format!("{drive}\\Jts"))]
            .into_iter()
            .chain(home.iter().map(|home| home.join("Jts")))
            .map(|root| {
                (
                    root.join("ibgateway"),
                    "",
                    (|_: &str| PathBuf::from("ibgateway.exe")) as fn(&str) -> PathBuf,
                )
            })
            .collect()
    } else {
        home.iter()
            .map(|home| {
                (
                    home.join("Jts/ibgateway"),
                    "",
                    (|_: &str| PathBuf::from("ibgateway")) as fn(&str) -> PathBuf,
                )
            })
            .collect()
    };
    let mut found: Vec<(Vec<u32>, PathBuf)> = Vec::new();
    for (root, prefix, launcher) in roots {
        for entry in fs::read_dir(&root).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path().join(launcher(&name));
            if name.starts_with(prefix) && path.is_file() {
                // The version from "IB Gateway 10.52" or "1052".
                let version = name[prefix.len()..]
                    .split('.')
                    .filter_map(|part| part.parse().ok())
                    .collect();
                found.push((version, path));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    let mut launchers: Vec<PathBuf> = found.into_iter().map(|(_, path)| path).collect();
    if cfg!(target_os = "linux") {
        launchers.extend(
            ["/opt/ibgateway/ibgateway", "/opt/ibkr/ibgateway"]
                .map(PathBuf::from)
                .into_iter()
                .filter(|path| path.is_file()),
        );
    }
    launchers
}

/// The native agent library that ships with this `ibkit` binary: next
/// to it, in `../lib`, or in `/usr/local/lib` (Unix).
pub fn find_agent() -> Option<PathBuf> {
    let name = crate::platform::AGENT_LIBRARY;
    // Resolved, as Homebrew links the binary from its Cellar directory.
    let binary = std::fs::canonicalize(std::env::current_exe().ok()?).ok()?;
    let directory = binary.parent()?;
    [
        Some(directory.join(name)),
        Some(directory.join("../lib").join(name)),
        cfg!(unix).then(|| Path::new("/usr/local/lib").join(name)),
    ]
    .into_iter()
    .flatten()
    .find(|path| path.is_file())
}

/// The file at `path` with `saved` (the API settings) over it, and `flags`
/// over both.
pub fn merge(path: Option<&Path>, saved: &toml::Table, flags: &toml::Table) -> Result<Config> {
    let mut layer = saved.clone();
    layer.extend(flags.clone());
    Config::load_with(path, &layer)
}

/// The settings saved through the control API; empty when there are none.
pub fn read_api_settings(state_dir: &Path) -> Result<toml::Table> {
    let path = state_dir.join(API_SETTINGS);
    match fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).with_context(|| format!(
            "{} is not valid TOML. Correct or remove it; it holds settings saved through the control API.",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(toml::Table::new()),
        Err(error) => Err(error).with_context(|| format!(
            "Cannot read {}. Correct the permissions of the private state directory.",
            path.display()
        )),
    }
}

pub const SECTIONS: [&str; 6] = ["Login", "Gateway", "API", "Control", "Recovery", "Schedule"];

/// Every configuration key in TUI order: (key, label, help). Keys that
/// start with `_` are TUI inputs that never enter the TOML file; use
/// [`config_fields`] for the configuration keys alone.
pub const FIELDS: &[&[(&str, &str, &str)]] = &[
    &[
        (
            "_username",
            "Username",
            "Enter both credentials to use them for this run. Existing files or environment sources are used when both fields are empty.",
        ),
        (
            "_password",
            "Password",
            "Masked input. Mobile IB Key approval is still required.",
        ),
        (
            "_totp_secret",
            "Mobile Authenticator secret",
            "Optional masked base32 secret from IBKR's Mobile Authenticator enrollment. Codes are then entered automatically; leave empty to approve IB Key on the phone. Save credentials stores it in a private file.",
        ),
        (
            "_remember",
            "Save credentials",
            "Off by default. s saves entered credentials in private files beside the config. Passwords never enter TOML.",
        ),
        (
            "trading_mode",
            "Trading mode",
            "paper or live. Starting live mode requires confirmation.",
        ),
        (
            "auto_login",
            "Automatic login",
            "Submit configured credentials. Turn off for manual Gateway login.",
        ),
        (
            "username_file",
            "Username file",
            "Optional private UTF-8 file. Leave empty to use the environment source.",
        ),
        (
            "password_file",
            "Password file",
            "Optional private UTF-8 file. Leave empty to use the environment source.",
        ),
        (
            "username_env",
            "Username environment",
            "Environment variable name, not its value.",
        ),
        (
            "password_env",
            "Password environment",
            "Environment variable name, not its value.",
        ),
        (
            "totp_secret_file",
            "TOTP secret file",
            "Optional private file with the base32 Mobile Authenticator secret. Leave empty to use the environment source.",
        ),
        (
            "totp_secret_env",
            "TOTP secret environment",
            "Environment variable name, not its value. When it is unset, login waits for IB Key approval.",
        ),
        (
            "passkey_authenticate",
            "Passkey prompts",
            "On presses Authenticate on a passkey prompt; an authenticator beside Gateway must complete it. Off fails the login for review.",
        ),
        (
            "existing_session",
            "Existing session",
            "refuse preserves another session; takeover explicitly disconnects it.",
        ),
        (
            "dismiss_password_expiry_warning",
            "Dismiss password expiry warning",
            "On: click OK on \"Your password will expire in N day(s)\" and report it; off: the warning fails the login.",
        ),
    ],
    &[
        (
            "executable",
            "Gateway launcher",
            "The program that starts IB Gateway. Found automatically in ~/Applications/IB Gateway <version> (macOS), ~/Jts/ibgateway/<version> (Linux) or C:\\Jts\\ibgateway\\<version>\\ibgateway.exe (Windows), newest first; set it only for another location. On macOS it is the app's Contents/MacOS/JavaApplicationStub.",
        ),
        (
            "agent",
            "Native agent",
            "This project's automation library (libibkit_gateway_agent.dylib on macOS, .so on Linux, ibkit_gateway_agent.dll on Windows). The controller loads it into Gateway's Java process, where it fills in the login, answers dialogs and sets the API settings. It ships with ibkit and is found next to the binary; you rarely need to set it.",
        ),
        (
            "state_dir",
            "State directory",
            "Private persistent state, recovery budget, and logs. One controller per directory.",
        ),
        (
            "settings_dir",
            "Gateway settings",
            "Private Gateway settings and restart tokens. Use a separate directory for each account.",
        ),
        (
            "manage_display",
            "Start Xvfb",
            "Linux servers only. Turn off on a local desktop with a display.",
        ),
        (
            "java_heap_mb",
            "Java heap (MiB)",
            "Optional 256 to 65536. Leave empty for Gateway's default (768).",
        ),
        (
            "vnc_listen",
            "VNC address",
            "Optional. Serve the virtual display with x11vnc (needs manage_display and a password file), for account tasks. Keep it on loopback and use a tunnel.",
        ),
        (
            "vnc_password_file",
            "VNC password file",
            "A file made with `x11vnc -storepasswd`. Needed with VNC address.",
        ),
    ],
    &[
        (
            "api_port",
            "Gateway API port",
            "1 to 65535. Paper commonly uses 4002; live commonly uses 4001. Changing mode does not change this port.",
        ),
        (
            "api_listen",
            "Forwarding listener",
            "Optional IP:port, for example 127.0.0.1:4004. Leave empty to disable. Use a different port from Gateway.",
        ),
        (
            "api_allowed_sources",
            "API allowed sources",
            "Optional comma-separated addresses or CIDR ranges that may use the API address. Empty allows every source; behind Docker port publishing the source can be the bridge address.",
        ),
        (
            "read_only_api",
            "Read-only API",
            "On prevents order changes through the Gateway API.",
        ),
        (
            "api_log",
            "API logs",
            "Create Gateway API logs in the private settings directory.",
        ),
        (
            "allow_remote_api",
            "Remote Gateway API",
            "Off restricts Gateway to localhost. The separate forwarder does not need this enabled.",
        ),
        (
            "incoming_connection",
            "Incoming connections",
            "manual, accept, or reject. Approve only intended API clients.",
        ),
        (
            "master_client_id",
            "Master client ID",
            "Optional API client ID that also receives orders from other clients. Leave empty for none.",
        ),
        (
            "bypass_order_precautions",
            "Bypass precautions",
            "Permit API orders to bypass Gateway order precautions.",
        ),
        (
            "accept_order_warnings",
            "Accept order warnings",
            "Automatically accept recognized order-warning dialogs.",
        ),
        (
            "export_logs",
            "Export logs at startup",
            "Export private Gateway and API logs after configuration.",
        ),
    ],
    &[
        (
            "control_listen",
            "Control API listener",
            "Optional IP:port for the REST and WebSocket control API, for example 127.0.0.1:5800. Leave empty to disable.",
        ),
        (
            "control_token_file",
            "Control token file",
            "Optional private file with the bearer token (16 or more characters). Leave empty to use the environment source.",
        ),
        (
            "control_token_env",
            "Control token environment",
            "Environment variable name, not its value. Default IBKIT_CONTROL_TOKEN.",
        ),
        (
            "alert_webhook_url",
            "Alert webhook URL",
            "Optional HTTP(S) URL that receives a JSON POST when Gateway goes down, comes back, halts for an operator, or the controller fails. The body has a text field for Slack and Mattermost. Leave empty to turn alerts off.",
        ),
        (
            "control_api_full_access",
            "Control API full access",
            "Off: the control API cannot turn on live trading, remote or writable API access, accepted incoming connections, bypassed precautions or the API address. Only the file, flags and variables can turn this on.",
        ),
    ],
    &[
        (
            "auto_recover",
            "Automatic recovery",
            "On by default. Retries transient faults; faults that need you (wrong password, account tasks) halt Gateway until you restart it.",
        ),
        (
            "max_login_failures",
            "Failed login limit",
            "0 to 1000 consecutive logins that end without Ready after an unknown login dialog or an unanswered second factor, before retries stop to prevent a lock-out; 0 has no limit.",
        ),
        (
            "recovery_attempts",
            "Recovery attempts",
            "0 (default) retries without a limit. 1 to 100000 limits attempts per hour; the count survives controller replacement.",
        ),
        (
            "recovery_delay_secs",
            "Retry delay (seconds)",
            "1 to 3600. The delay doubles per attempt up to the maximum delay.",
        ),
        (
            "recovery_max_delay_secs",
            "Maximum retry delay (seconds)",
            "From the retry delay to 86400. Default 1800.",
        ),
        (
            "api_probe_interval_secs",
            "API probe (seconds)",
            "1 to 3600. A local API greeting does not prove upstream IBKR connectivity.",
        ),
        (
            "api_failure_timeout_secs",
            "API failure grace",
            "At least the probe interval and at most 86400 seconds.",
        ),
        (
            "region",
            "Maintenance region",
            "america, europe, or asia. Recovery waits through the selected server-maintenance window.",
        ),
        (
            "initialization_timeout_secs",
            "Login/setup timeout",
            "1 to 86400 seconds, including manual login and a missing agent report.",
        ),
        (
            "soft_restart_timeout_secs",
            "Soft restart timeout",
            "120 to 86400 seconds.",
        ),
        (
            "shutdown_timeout_secs",
            "Shutdown timeout",
            "1 to 3600 seconds before owned processes are stopped.",
        ),
        (
            "second_factor_timeout_secs",
            "Mobile approval timeout",
            "1 to 180 seconds per IB Key approval.",
        ),
        (
            "second_factor_attempts",
            "Mobile approval attempts",
            "1 to 3 mobile requests before operator review is required.",
        ),
        (
            "unknown_dialog_timeout_secs",
            "Unknown dialog grace",
            "1 to 3600 seconds an unrecognized dialog may block login before the login fails. Unknown dialogs are never clicked.",
        ),
    ],
    &[
        (
            "restart_time",
            "Daily restart time",
            "HH:MM from 00:00 to 23:59 in the system time zone. Match the saved Gateway account time zone.",
        ),
        (
            "closedown_at",
            "Weekly closedown",
            "Optional weekday and time, for example \"Fri 23:00\": pause Gateway until restart.",
        ),
        (
            "cold_restart_at",
            "Weekly cold restart",
            "Optional weekday and time, for example \"Sun 07:05\": log in again then, when someone can approve IB Key.",
        ),
    ],
];

/// The configuration keys with their label and help, in TUI order.
pub fn config_fields() -> impl Iterator<Item = &'static (&'static str, &'static str, &'static str)>
{
    FIELDS
        .iter()
        .copied()
        .flatten()
        .filter(|(key, _, _)| !key.starts_with('_'))
}

/// The JSON schema of each configuration key, by key.
static KEY_SCHEMAS: LazyLock<serde_json::Map<String, serde_json::Value>> =
    LazyLock::new(
        || match schemars::schema_for!(Config).to_value()["properties"].take() {
            serde_json::Value::Object(properties) => properties,
            _ => serde_json::Map::new(),
        },
    );

/// Convert text from a flag, an environment variable or a form field to
/// the TOML value of `key`, by the key's type in the `Config` schema.
/// `None` means an empty value for an optional key, which leaves it unset.
pub fn parse_value(key: &str, raw: &str) -> Result<Option<toml::Value>> {
    let types: Vec<&str> = match KEY_SCHEMAS.get(key).map(|schema| &schema["type"]) {
        Some(serde_json::Value::String(name)) => vec![name],
        Some(serde_json::Value::Array(names)) => names.iter().filter_map(|n| n.as_str()).collect(),
        _ => Vec::new(),
    };
    if raw.is_empty() && types.contains(&"null") {
        return Ok(None);
    }
    Ok(Some(if types.contains(&"boolean") {
        toml::Value::Boolean(match raw {
            "true" | "1" | "yes" | "on" => true,
            "false" | "0" | "no" | "off" => false,
            _ => bail!("You set {key} to {raw:?}. Use true or false (also 1/0, yes/no, on/off)."),
        })
    } else if types.contains(&"integer") {
        toml::Value::Integer(raw.parse().with_context(|| {
            format!("You set {key} to {raw:?}. Use a whole number in the range that `ibkit gateway --help` gives for it.")
        })?)
    } else {
        toml::Value::String(raw.into())
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_installed_gateway_is_found_and_user_paths_stay() {
        let home = tempfile::tempdir().unwrap();
        let versions = ["10.37", "10.52", "10.9"];
        for version in versions {
            let (directory, file) = if cfg!(target_os = "macos") {
                (
                    home.path().join(format!(
                        "Applications/IB Gateway {version}/IB Gateway {version}.app/Contents/MacOS"
                    )),
                    "JavaApplicationStub",
                )
            } else {
                (
                    home.path()
                        .join("Jts/ibgateway")
                        .join(version.replace('.', "")),
                    if cfg!(windows) {
                        "ibgateway.exe"
                    } else {
                        "ibgateway"
                    },
                )
            };
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join(file), "").unwrap();
        }
        // Only the test's home: a real C:\Jts on the machine does not count.
        let found: Vec<_> = launchers_in(Some(home.path().into()))
            .into_iter()
            .filter(|path| path.starts_with(home.path()))
            .collect();
        assert_eq!(found.len(), 3, "{found:?}");
        let newest = if cfg!(target_os = "macos") {
            "10.52"
        } else {
            "1052"
        };
        assert!(found[0].to_string_lossy().contains(newest), "{found:?}");
        // A path that the user set is never replaced.
        let mut config = Config {
            executable: "/somewhere/else/ibgateway".into(),
            agent: "/somewhere/else/agent.so".into(),
            ..Config::default()
        };
        fill_paths(&mut config);
        assert_eq!(
            config.executable,
            PathBuf::from("/somewhere/else/ibgateway")
        );
        assert_eq!(config.agent, PathBuf::from("/somewhere/else/agent.so"));
    }

    #[test]
    fn fields_cover_every_configuration_key() {
        let all = Config {
            api_listen: Some("127.0.0.1:4004".parse().unwrap()),
            username_file: Some("u".into()),
            password_file: Some("p".into()),
            totp_secret_file: Some("t".into()),
            java_heap_mb: Some(1024),
            master_client_id: Some(0),
            control_listen: Some("127.0.0.1:5800".parse().unwrap()),
            control_token_file: Some("c".into()),
            alert_webhook_url: Some("https://hooks.example.com/x".into()),
            api_allowed_sources: Some("10.0.0.0/8".into()),
            vnc_listen: Some("127.0.0.1:5900".parse().unwrap()),
            vnc_password_file: Some("v".into()),
            closedown_at: Some("Fri 23:00".into()),
            cold_restart_at: Some("Sun 07:05".into()),
            ..Config::default()
        };
        let mut keys: Vec<String> = toml::Table::try_from(all)
            .unwrap()
            .keys()
            .cloned()
            .collect();
        let mut fields: Vec<String> = config_fields().map(|(key, _, _)| key.to_string()).collect();
        keys.sort();
        fields.sort();
        assert_eq!(keys, fields);
    }

    #[test]
    fn values_follow_the_key_type_and_errors_name_the_key() {
        assert_eq!(
            parse_value("api_port", "4001").unwrap(),
            Some(toml::Value::Integer(4001))
        );
        assert_eq!(
            parse_value("auto_login", "off").unwrap(),
            Some(toml::Value::Boolean(false))
        );
        assert_eq!(
            parse_value("restart_time", "05:00").unwrap(),
            Some(toml::Value::String("05:00".into()))
        );
        assert_eq!(
            parse_value("java_heap_mb", "2048").unwrap(),
            Some(toml::Value::Integer(2048))
        );
        assert_eq!(parse_value("api_listen", "").unwrap(), None);
        assert_eq!(
            parse_value("master_client_id", "0").unwrap(),
            Some(toml::Value::Integer(0))
        );
        assert_eq!(
            parse_value("auto_login", "off").unwrap(),
            Some(toml::Value::Boolean(false))
        );
        assert_eq!(
            parse_value("trading_mode", "live").unwrap(),
            Some(toml::Value::String("live".into()))
        );
        let error = parse_value("api_port", "fast").unwrap_err().to_string();
        assert!(
            error.contains("api_port") && error.contains("whole number"),
            "{error}"
        );
    }
}
