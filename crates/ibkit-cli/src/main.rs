#![forbid(unsafe_code)]
mod tui;
mod update;
use anyhow::{Context, Result, bail};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum, parser::ValueSource};
use ibkit_gateway::{Control, Gateway, State, api_handshake, read_status, send_control};
use std::{path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(name = "ibkit", version, about = "Interactive Brokers toolkit")]
struct Cli {
    #[command(subcommand)]
    command: Top,
}

#[derive(Subcommand)]
enum Top {
    /// Start, supervise and control IB Gateway with a native Rust agent.
    Gateway(GatewayArgs),
    /// Replace this ibkit binary and its agent with the latest release.
    Update,
}

#[derive(clap::Args)]
struct GatewayArgs {
    /// TOML settings file. Without it, ./config.toml is read when it exists;
    /// otherwise the settings come from defaults, flags and IBKIT_* variables.
    #[arg(long, global = true, env = "IBKIT_CONFIG")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Edit settings, enter credentials, and control Gateway in a terminal.
    Tui,
    /// Run in the foreground. Output one JSON event per line.
    Run,
    /// Print the effective settings (file, then flags and IBKIT_* variables) as TOML.
    Config,
    /// Print the OpenAPI 3.1 document of the control API.
    Openapi,
    /// Check settings. Add --credentials to check the credential and TOTP
    /// sources, and --files to check that the launcher and agent exist.
    Validate {
        #[arg(long)]
        credentials: bool,
        #[arg(long)]
        files: bool,
    },
    /// Print the controller's status (state, session, agents, recovery) as JSON.
    Status,
    /// Clear a saved recovery block after operator review. The controller must be stopped.
    ResetRecovery,
    /// Stop Gateway and end the controller.
    Stop,
    /// Stop Gateway, then start a new login session.
    Restart,
    /// Restart Gateway through its own daily restart, without a new login.
    SoftRestart,
    /// Export the private Gateway and API logs through Gateway's log viewer.
    ExportLogs,
    /// Reconnect Gateway's server connection without a restart (IBC RECONNECTACCOUNT).
    ReconnectAccount,
    /// Reconnect market data farms (IBC RECONNECTDATA).
    ReconnectData,
    /// Read the configuration file again and apply it (a new session only
    /// when a Gateway setting changed).
    Reload,
    /// Stop Gateway and keep the controller up until restart, also across
    /// controller restarts.
    Pause,
    /// Exit 0 when the controller runs (--kind live) or Gateway is ready and
    /// its API answers (--kind ready, the default).
    Health {
        #[arg(long, value_enum, default_value = "ready")]
        kind: HealthKind,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum HealthKind {
    Live,
    Ready,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Every configuration key is also a flag of `ibkit gateway` and its
    // commands, and an IBKIT_* variable.
    let command = Cli::command().mut_subcommand("gateway", |mut gateway| {
        for (key, label, help) in ibkit_gateway::settings::config_fields() {
            gateway = gateway.arg(
                clap::Arg::new(*key)
                    .long(key.replace('_', "-"))
                    .env(format!("IBKIT_{}", key.to_uppercase()))
                    .value_name("VALUE")
                    .help(label.to_string())
                    .long_help(format!("{label}. {help}"))
                    .global(true)
                    .help_heading("Settings (override the config file)")
                    // --help must not print secrets such as the webhook URL.
                    .hide_env_values(true),
            );
        }
        gateway
    });
    let all = command.get_matches();
    let cli = match Cli::from_arg_matches(&all)?.command {
        Top::Update => {
            println!("{}", update::run().await?);
            return Ok(());
        }
        Top::Gateway(cli) => cli,
    };
    let matches = all
        .subcommand_matches("gateway")
        .expect("the gateway command was selected");
    let mut overrides = toml::Table::new();
    for (key, _, _) in ibkit_gateway::settings::config_fields() {
        let Some(raw) = matches.get_one::<String>(key) else {
            continue;
        };
        let source = if matches.value_source(key) == Some(ValueSource::EnvVariable) {
            format!("IBKIT_{}", key.to_uppercase())
        } else {
            format!("--{}", key.replace('_', "-"))
        };
        // An empty value of an optional key clears it (an empty array marks
        // a removal for Config::load_with).
        let value = ibkit_gateway::settings::parse_value(key, raw)
            .with_context(|| format!("The value of {source} is invalid."))?
            .unwrap_or(toml::Value::Array(Vec::new()));
        overrides.insert(key.to_string(), value);
    }
    if matches!(cli.command, Commands::Openapi) {
        println!(
            "{}",
            serde_json::to_string_pretty(&ibkit_gateway::openapi::document())?
        );
        return Ok(());
    }
    if matches!(cli.command, Commands::Tui) {
        if !overrides.is_empty() {
            bail!(
                "You gave setting flags or IBKIT_* variables to tui. The TUI edits the config file; give the flags to run instead, or remove them."
            );
        }
        return tui::run(cli.config.unwrap_or_else(|| "config.toml".into())).await;
    }
    let path = cli.config.clone().or_else(|| {
        let default = PathBuf::from("config.toml");
        default.is_file().then_some(default)
    });
    let config = ibkit_gateway::settings::load(path.as_deref(), &overrides)?;
    match cli.command {
        Commands::Tui | Commands::Openapi => unreachable!("dispatched before config loading"),
        Commands::Config => print!("{}", toml::to_string_pretty(&config)?),
        Commands::Validate { credentials, files } => {
            if credentials {
                let _ = config.credentials()?;
                config.totp_key()?;
            }
            if files {
                let mut checked = config.clone();
                checked.auto_login = false;
                ibkit_gateway::gateway::check_config(&mut checked)?;
            }
            println!("Configuration is valid.");
        }
        Commands::Status => println!(
            "{}",
            serde_json::to_string_pretty(&read_status(&config.state_dir)?)?
        ),
        Commands::ResetRecovery => Gateway::new(config)?.reset_recovery()?,
        Commands::Stop => send_control(&config.state_dir, Control::Stop)?,
        Commands::Restart => send_control(&config.state_dir, Control::Restart)?,
        Commands::SoftRestart => send_control(&config.state_dir, Control::SoftRestart)?,
        Commands::ExportLogs => send_control(&config.state_dir, Control::ExportLogs)?,
        Commands::ReconnectAccount => send_control(&config.state_dir, Control::ReconnectAccount)?,
        Commands::ReconnectData => send_control(&config.state_dir, Control::ReconnectData)?,
        Commands::Reload => send_control(&config.state_dir, Control::Reload)?,
        Commands::Pause => send_control(&config.state_dir, Control::Pause)?,
        Commands::Health { kind } => {
            let status = read_status(&config.state_dir)?;
            // Live: the controller runs (a halted controller is live, so a
            // liveness probe does not restart it into the same fault).
            if !status.is_live() {
                bail!(
                    "The controller is inactive. Check its event log and start the controller again."
                );
            }
            if matches!(kind, HealthKind::Ready) {
                if status.state != State::Ready {
                    bail!(
                        "Gateway has not completed login and configuration. Complete mobile approval or account tasks, then check again."
                    );
                }
                api_handshake(config.api_port).await?;
            }
        }
        Commands::Run => {
            let mut signals = ibkit_gateway::platform::Signals::new()?;
            let mut gateway = Gateway::new(config)?;
            let metrics = std::sync::Arc::new(ibkit_gateway::monitor::Metrics::default());
            let webhook =
                |config: &ibkit_gateway::Config| -> Result<Option<ibkit_gateway::monitor::Alerts>> {
                    config
                        .alert_webhook_url
                        .as_deref()
                        .map(|url| ibkit_gateway::monitor::Alerts::new(url, metrics.clone()))
                        .transpose()
                };
            let (alerts, alerts_rx) = tokio::sync::watch::channel(webhook(gateway.config())?);
            let monitor = tokio::spawn(ibkit_gateway::monitor::run(
                gateway.events().subscribe(),
                metrics.clone(),
                alerts_rx,
            ));
            let control_api = match gateway.config().control_listen {
                Some(address) => {
                    let token = gateway.config().control_token()?;
                    let listener = tokio::net::TcpListener::bind(address).await.with_context(|| format!("Cannot listen on control_listen {address}. Free the address or change control_listen."))?;
                    Some(tokio::spawn(ibkit_gateway::control::serve(
                        listener,
                        ibkit_gateway::control::ControlApi {
                            token: token.into(),
                            state_dir: gateway.config().state_dir.clone(),
                            config_path: path.clone(),
                            flags: std::sync::Arc::new(overrides.clone()),
                            events: gateway.events(),
                            metrics: metrics.clone(),
                        },
                    )))
                }
                None => None,
            };
            type Forwarder = (
                (std::net::SocketAddr, u16, Option<String>),
                tokio::task::JoinHandle<()>,
            );
            let mut forwarder: Option<Forwarder> = None;
            let mut recovery = ibkit_gateway::recovery::Recovery::default();
            let mut events = gateway.events().subscribe();
            let output = tokio::spawn(async move {
                loop {
                    match events.recv().await {
                        Ok(event) => {
                            println!("{}", serde_json::to_string(&event).unwrap_or_default());
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            // Settings saved through the control API take precedence over the
            // file; say which ones, so a changed file or chart value that has
            // no effect is not a mystery.
            let shadowed = ibkit_gateway::settings::read_api_settings(&gateway.config().state_dir)
                .map(|saved| saved.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            if !shadowed.is_empty() {
                eprintln!(
                    "Settings saved through the control API override the config file: {}. Remove them with DELETE /v1/config (or PATCH a key to null).",
                    shadowed.join(", ")
                );
            }
            gateway.resume(&mut recovery).await?;
            let mut by_signal = false;
            // Windows console close, logoff or shutdown: end at once and leave
            // Gateway for the next controller to adopt.
            let mut leave = false;
            // Consecutive failed steps (for example a full disk): a short run is
            // retried; 20 in a row (10 s) end the controller.
            let mut failures = 0u32;
            let mut last_tick = chrono::Local::now();
            let failure = loop {
                tokio::select! {
                    signal = signals.next() => match signal {
                        ibkit_gateway::platform::Signal::Reload => gateway.request_reload(),
                        ibkit_gateway::platform::Signal::Terminate | ibkit_gateway::platform::Signal::Interrupt => { by_signal = true; break None; }
                        ibkit_gateway::platform::Signal::Leave => { by_signal = true; leave = true; break None; }
                    },
                    _ = tokio::time::sleep(Duration::from_millis(500)) => {
                        // One forwarder per (api_listen, api_port, sources); a
                        // reload that changes one starts a new one.
                        let wanted = gateway.config().api_listen.map(|listen| {
                            (listen, gateway.config().api_port, gateway.config().api_allowed_sources.clone())
                        });
                        if forwarder.as_ref().map(|(key, _)| key) != wanted.as_ref() {
                            if let Some((_, task)) = forwarder.take() {
                                task.abort();
                            }
                            forwarder = match wanted {
                                Some(key) => {
                                    let sources = gateway.config().allowed_sources()?;
                                    let task = tokio::spawn(ibkit_gateway::forwarder::supervise(key.0, key.1, sources));
                                    Some((key, task))
                                }
                                None => None,
                            };
                        }
                        let now = chrono::Local::now();
                        let weekly = |value: &Option<String>| {
                            value
                                .as_deref()
                                .and_then(|value| ibkit_gateway::schedule::Weekly::parse(value).ok())
                                .is_some_and(|weekly| weekly.occurs_between(last_tick, now))
                        };
                        let closedown = weekly(&gateway.config().closedown_at);
                        let cold_restart = weekly(&gateway.config().cold_restart_at);
                        last_tick = now;
                        // Ok(true) ends the controller normally.
                        let step: Result<bool> = async {
                            if closedown && !gateway.is_paused() {
                                gateway.emit_output("Scheduled closedown (closedown_at): pausing Gateway until restart.");
                                gateway.pause().await?;
                            }
                            if cold_restart {
                                gateway.cold_restart(&mut recovery).await?;
                            }
                            if gateway.is_paused() {
                                return Ok(gateway.poll_paused(&mut recovery).await? == State::Exited);
                            }
                            // A halted controller serves commands only.
                            if gateway.status().state == State::Halted {
                                return Ok(gateway.poll_halted(&mut recovery).await? == State::Exited);
                            }
                            let state = gateway.poll_recovery(&mut recovery).await?;
                            if state == State::Paused {
                                return Ok(false);
                            }
                            if matches!(state, State::Failed | State::Exited) && !gateway.recover(&mut recovery).await? {
                                // No error code: an operator stop.
                                if !gateway.last_start_result().has_error() {
                                    return Ok(true);
                                }
                                gateway.halt(&recovery).await?;
                            }
                            Ok(false)
                        }
                        .await;
                        match step {
                            Ok(true) => break None,
                            Ok(false) => failures = 0,
                            Err(error) => {
                                failures += 1;
                                eprintln!("A controller step failed ({failures} of 20 in a row): {error:#}");
                                if failures >= 20 {
                                    break Some(format!("Controller operation failed: {error:#}. Check that state_dir is writable and has free space, then start again."));
                                }
                            }
                        }
                        if gateway.take_reload() {
                            let reloaded = match ibkit_gateway::settings::load(path.as_deref(), &overrides) {
                                Ok(config) => gateway.reconfigure(config, &mut recovery).await,
                                Err(error) => Err(error),
                            };
                            match reloaded.and_then(|()| webhook(gateway.config())) {
                                Ok(webhook) => {
                                    alerts.send_replace(webhook);
                                }
                                Err(error) => eprintln!("The reload failed and the current configuration stays in use: {error:#}"),
                            }
                        }
                    }
                }
            };
            // Send the alert before the final stop: a stop that fails too (a
            // full disk) must not lose it.
            let session = gateway.status().session.clone();
            if let Some(message) = &failure
                && let Some(alerts) = alerts.borrow().clone()
            {
                alerts
                    .send(ibkit_gateway::monitor::alert(
                        ibkit_gateway::api::AlertKind::ControllerFailed,
                        &session,
                        State::Exited,
                        ibkit_gateway::ErrorCode::None,
                        message.clone(),
                    ))
                    .await;
            }
            if leave {
                // Exit without destructors: dropping the Gateway would ask
                // the agent to stop and kill the launcher.
                std::process::exit(0);
            }
            let stopped = gateway.stop().await;
            // A pending retry survives a signal, so a replacement controller
            // keeps its delay (for example the wait after an unanswered login).
            if failure.is_none()
                && !(by_signal
                    && matches!(
                        recovery.state,
                        ibkit_gateway::recovery::RecoveryState::Waiting { .. }
                    ))
            {
                gateway.cancel_recovery(&mut recovery)?;
            }
            if let Some((_, task)) = forwarder.take() {
                task.abort();
                let _ = task.await;
            }
            if let Some(task) = control_api {
                task.abort();
                let _ = task.await;
            }
            drop(gateway);
            output.await?;
            let _ = monitor.await;
            if let Some(message) = failure {
                bail!("{message}");
            }
            stopped?;
        }
    }
    Ok(())
}
