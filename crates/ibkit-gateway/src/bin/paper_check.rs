#![forbid(unsafe_code)]
use anyhow::{Context, Result, bail};
use clap::Parser;
use ibapi::{accounts::PositionUpdate, orders::Executions, prelude::*};
use ibkit_gateway::client::{ClientAction, ClientRecovery, Snapshot, reconcile_order_ids};
use ibkit_gateway::{Config, Control, ErrorCode, State, TradingMode, read_status, send_control};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    time::Duration,
};
use tokio::time::{Instant, sleep, timeout_at};

#[derive(Parser)]
#[command(about = "Read-only checks for a dedicated paper Gateway; never sends an order")]
struct Args {
    #[arg(long)]
    config: PathBuf,
    #[arg(long, env = "IBKIT_PAPER_ACCOUNT")]
    paper_account: String,
    #[arg(long, default_value_t = 19731, value_parser = clap::value_parser!(i32).range(1..))]
    client_id: i32,
    /// Private JSON file: expected_perm_ids and unacknowledged_intents.
    #[arg(long)]
    intents: PathBuf,
    /// A new private JSONL file. Existing files are not overwritten.
    #[arg(long)]
    report: PathBuf,
    #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..=691200))]
    duration_secs: u64,
    #[arg(long, default_value = "SPY")]
    symbol: String,
    /// Disconnect only this checker's API socket once, after baseline checks.
    #[arg(long)]
    disconnect_after_secs: Option<u64>,
    /// Ask the dedicated controller for a soft restart after baseline checks.
    #[arg(long)]
    request_soft_restart: bool,
    #[arg(long)]
    expect_restart: bool,
    #[arg(long)]
    expect_weekly_renewal: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Intents {
    expected_perm_ids: Vec<i64>,
    unacknowledged_intents: usize,
}

#[derive(Default, Serialize)]
struct Results {
    api_ready_at_end: bool,
    reconciled_sessions: usize,
    socket_outage_recovered: bool,
    restart_handoff_observed: bool,
    restart_reconciled: bool,
    authentication_required_observed: bool,
    weekly_renewal_observed: bool,
    #[serde(skip)]
    observed_session: String,
    #[serde(skip)]
    ready_pids: BTreeSet<u32>,
    #[serde(skip)]
    authentication_at: Option<Instant>,
    #[serde(skip)]
    restart_at: Option<Instant>,
}

impl Results {
    fn observe_controller(&mut self, config: &Config) {
        if let Ok(status) = read_status(&config.state_dir) {
            if !(0..=5000).contains(&(chrono::Utc::now() - status.at).num_milliseconds()) {
                return;
            }
            if status.last_start_result.error_code == ErrorCode::AuthenticationRequired
                && self.authentication_at.is_none()
            {
                self.authentication_at = Some(Instant::now());
                self.authentication_required_observed = true;
            }
            if status.state == State::Ready {
                if self.observed_session != status.session {
                    self.observed_session = status.session;
                    self.ready_pids.clear();
                }
                let baseline = self.ready_pids.is_empty();
                for agent in status
                    .agents
                    .iter()
                    .filter(|agent| agent.state == State::Ready)
                {
                    if !self.ready_pids.contains(&agent.pid) && !baseline {
                        self.restart_handoff_observed = true;
                        self.restart_reconciled = false;
                        self.restart_at = Some(Instant::now());
                    }
                    self.ready_pids.insert(agent.pid);
                }
            }
        }
    }

    fn reconciled(
        &mut self,
        round_started: Instant,
        controller_ready: bool,
        outage_requested: bool,
    ) {
        self.reconciled_sessions += 1;
        self.api_ready_at_end = true;
        self.socket_outage_recovered |= outage_requested;
        if controller_ready {
            self.weekly_renewal_observed |=
                self.authentication_at.is_some_and(|at| round_started > at);
            self.restart_reconciled |= self.restart_at.is_some_and(|at| round_started > at);
        }
    }

    fn passed(&self, args: &Args) -> bool {
        self.reconciled_sessions > 0
            && self.api_ready_at_end
            && (args.disconnect_after_secs.is_none() || self.socket_outage_recovered)
            && (!(args.expect_restart || args.request_soft_restart) || self.restart_reconciled)
            && (!args.expect_weekly_renewal || self.weekly_renewal_observed)
    }
}

#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum CheckEvent<'a> {
    Reconciled {
        open_orders: usize,
        executions: usize,
        positions: usize,
    },
    Notice {
        code: i32,
    },
    SocketDisconnected {
        requested: bool,
    },
    Retrying,
    RestartRequested,
    Summary {
        passed: bool,
        results: &'a Results,
    },
    Failed,
}

fn record(file: &mut File, event: CheckEvent<'_>) -> Result<()> {
    #[derive(Serialize)]
    struct Record<'a> {
        at: chrono::DateTime<chrono::Utc>,
        #[serde(flatten)]
        event: CheckEvent<'a>,
    }
    serde_json::to_writer(
        &mut *file,
        &Record {
            at: chrono::Utc::now(),
            event,
        },
    )?;
    file.write_all(b"\n")?;
    file.flush()?;
    Ok(())
}

struct Snapshots {
    open: Vec<i64>,
    executions: Vec<i64>,
    positions: usize,
    quotes: Subscription<TickTypes>,
}

async fn snapshots(client: &Client, args: &Args) -> Result<Snapshots> {
    let orders = async {
        let mut stream = client.all_open_orders().await?;
        let rows = stream
            .collect_to_end_within(Duration::from_secs(45))
            .await?;
        Ok::<_, ibapi::Error>(
            rows.into_iter()
                .filter_map(|row| match row {
                    Orders::OrderData(data) if data.order.account == args.paper_account => {
                        Some(data.order.perm_id)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>(),
        )
    };
    let executions = async {
        let mut stream = client
            .executions(ExecutionFilter {
                account_code: args.paper_account.clone(),
                last_n_days: 7,
                ..Default::default()
            })
            .await?;
        let rows = stream
            .collect_to_end_within(Duration::from_secs(45))
            .await?;
        Ok::<_, ibapi::Error>(
            rows.into_iter()
                .filter_map(|row| match row {
                    Executions::ExecutionData(data)
                        if data.execution.account_number == args.paper_account =>
                    {
                        Some(data.execution.perm_id)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>(),
        )
    };
    let positions = async {
        let mut stream = client.positions().await?;
        let mut count = 0;
        while let Some(item) = stream.next().await {
            match item? {
                SubscriptionItem::Data(PositionUpdate::Position(position))
                    if position.account == args.paper_account =>
                {
                    count += 1
                }
                SubscriptionItem::Data(PositionUpdate::PositionEnd) => return Ok(count),
                SubscriptionItem::Notice(notice) if notice.code == ibapi::SUBSCRIPTION_LAG_CODE => {
                    bail!(
                        "Position callbacks were lost. Repeat this check with a complete position snapshot."
                    )
                }
                _ => {}
            }
        }
        bail!(
            "The position stream closed before PositionEnd. Restore the API connection and repeat the snapshot."
        )
    };
    let (open, executions, positions) = tokio::try_join!(
        async { orders.await.map_err(anyhow::Error::from) },
        async { executions.await.map_err(anyhow::Error::from) },
        positions,
    )?;
    let contract = Contract::stock(&args.symbol).build();
    let mut quotes = client.market_data(&contract).subscribe().await?;
    loop {
        match quotes.next().await.context(
            "The quote stream closed. Check market-data permissions and repeat the check.",
        )?? {
            SubscriptionItem::Data(TickTypes::Price(tick)) if tick.price > 0.0 => break,
            SubscriptionItem::Data(TickTypes::PriceSize(tick)) if tick.price > 0.0 => break,
            SubscriptionItem::Notice(notice) if notice.code == ibapi::SUBSCRIPTION_LAG_CODE => {
                bail!("Quote callbacks were lost. Repeat this check with an intact subscription.")
            }
            _ => {}
        }
    }
    Ok(Snapshots {
        open,
        executions,
        positions,
        quotes,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = Config::load(&args.config)?;
    if config.trading_mode != TradingMode::Paper
        || !config.read_only_api
        || !args.paper_account.starts_with("DU")
    {
        bail!(
            "You selected an account or config that is not approved for this check. Use a dedicated DU paper account, trading_mode=paper, and read_only_api=true."
        );
    }
    if args
        .disconnect_after_secs
        .is_some_and(|delay| delay >= args.duration_secs)
        || args.request_soft_restart && args.duration_secs < 300
    {
        bail!(
            "You supplied time limits that cannot complete the check. Keep disconnect_after_secs below duration_secs and allow at least 300 seconds for a requested soft restart."
        );
    }
    let intents: Intents = serde_json::from_slice(&fs::read(&args.intents).context("Cannot read the intent ledger. Supply a private JSON file with expected_perm_ids and unacknowledged_intents.")?)?;
    reconcile_order_ids(
        &intents.expected_perm_ids,
        &[],
        &[],
        intents.unacknowledged_intents,
    )?;
    let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&args.report)
        .context("Cannot create the report. Select a new file in a writable private directory; existing reports are preserved.")?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut results = Results::default();
    let (result, interrupted) = {
        tokio::select! {
            result = run(&args, &config, &intents, &mut file, &mut results) => (result, false),
            _ = interrupt.recv() => (Err(anyhow::anyhow!("You interrupted the paper check. Its report is incomplete; start a new check to collect the required observations.")), true),
            _ = terminate.recv() => (Err(anyhow::anyhow!("You stopped the paper check. Its report is incomplete; start a new check to collect the required observations.")), true),
        }
    };
    if interrupted {
        record(
            &mut file,
            CheckEvent::Summary {
                passed: false,
                results: &results,
            },
        )?;
        file.sync_all()?;
    }
    if result.is_err() {
        record(&mut file, CheckEvent::Failed)?;
    }
    result
}

async fn run(
    args: &Args,
    config: &Config,
    intents: &Intents,
    file: &mut File,
    results: &mut Results,
) -> Result<()> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(args.duration_secs);
    let address = format!("127.0.0.1:{}", config.api_port);
    let mut gate = ClientRecovery::default();
    gate.require_subscription("quotes")?;
    let mut outage_requested = false;
    let mut restart_requested = false;
    while Instant::now() < deadline {
        results.api_ready_at_end = false;
        results.observe_controller(config);
        let connection = timeout_at(
            deadline.min(Instant::now() + Duration::from_secs(10)),
            Client::builder()
                .address(&address)
                .client_id(args.client_id)
                .max_reconnect_attempts(0)
                .connect_with_notice_stream(),
        )
        .await;
        let (client, mut notices) = match connection {
            Ok(Ok(connection)) => connection,
            _ => {
                record(file, CheckEvent::Retrying)?;
                sleep_until_retry(deadline).await;
                continue;
            }
        };
        let round_started = Instant::now();
        let accounts = timeout_at(
            deadline.min(Instant::now() + Duration::from_secs(10)),
            client.managed_accounts(),
        )
        .await??;
        if !accounts
            .iter()
            .any(|account| account == &args.paper_account)
            || accounts.iter().any(|account| !account.starts_with("DU"))
        {
            client.disconnect().await;
            bail!(
                "The API account list does not match the dedicated paper account. Stop this check and select the correct paper Gateway and account."
            );
        }
        let generation = gate.connected();
        let snapshot_result = {
            let requests = timeout_at(
                deadline.min(Instant::now() + Duration::from_secs(60)),
                snapshots(&client, args),
            );
            tokio::pin!(requests);
            loop {
                tokio::select! {
                    result = &mut requests => break result.ok().and_then(Result::ok),
                    notice = notices.next() => {
                        let Some(notice) = notice else { break None; };
                        record(file, CheckEvent::Notice { code: notice.code })?;
                        if notice.code == ibapi::NOTICE_STREAM_LAG_CODE || gate.notice(notice.code) != ClientAction::None { break None; }
                    }
                }
            }
        };
        let Some(mut snapshot) = snapshot_result else {
            gate.disconnected();
            client.disconnect().await;
            record(file, CheckEvent::Retrying)?;
            sleep_until_retry(deadline).await;
            continue;
        };
        for kind in [
            Snapshot::OpenOrders,
            Snapshot::Executions,
            Snapshot::Positions,
        ] {
            gate.snapshot_complete(generation, kind);
        }
        let report = reconcile_order_ids(
            &intents.expected_perm_ids,
            &snapshot.open,
            &snapshot.executions,
            intents.unacknowledged_intents,
        )?;
        gate.orders_reviewed(generation, &report)?;
        gate.subscription_received(generation, "quotes")?;
        if !gate.can_submit_orders() {
            client.disconnect().await;
            bail!(
                "The paper snapshot contains unresolved order intents. Compare the durable ledger with broker open orders, executions, and terminal order records; do not replay missing orders."
            );
        }
        results.reconciled(
            round_started,
            read_status(&config.state_dir).is_ok_and(|status| {
                status.state == State::Ready
                    && (0..=5000).contains(&(chrono::Utc::now() - status.at).num_milliseconds())
            }),
            outage_requested,
        );
        record(
            file,
            CheckEvent::Reconciled {
                open_orders: snapshot.open.len(),
                executions: snapshot.executions.len(),
                positions: snapshot.positions,
            },
        )?;
        if args.request_soft_restart && !restart_requested {
            send_control(&config.state_dir, Control::SoftRestart)?;
            restart_requested = true;
            record(file, CheckEvent::RestartRequested)?;
        }
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => break,
                notice = notices.next() => {
                    let Some(notice) = notice else { break; };
                    record(file, CheckEvent::Notice { code: notice.code })?;
                    if notice.code == 1300 { client.disconnect().await; bail!("Gateway changed its API port. Set api_port to the new port and start a new paper check; no port is inferred from an unstructured broker message."); }
                    if notice.code == ibapi::NOTICE_STREAM_LAG_CODE { break; }
                    if matches!(gate.notice(notice.code), ClientAction::Reconcile | ClientAction::Reconnect) { break; }
                }
                quote = snapshot.quotes.next() => {
                    if !matches!(quote, Some(Ok(_))) { break; }
                }
                _ = tick.tick() => {
                    results.observe_controller(config);
                    if results.restart_handoff_observed && !results.restart_reconciled { break; }
                    if !client.is_connected() { break; }
                    if !outage_requested && args.disconnect_after_secs.is_some_and(|delay| started.elapsed().as_secs() >= delay) {
                        outage_requested = true;
                        record(file, CheckEvent::SocketDisconnected { requested: true })?;
                        break;
                    }
                }
            }
        }
        results.api_ready_at_end =
            Instant::now() >= deadline && gate.can_submit_orders() && client.is_connected();
        gate.disconnected();
        client.disconnect().await;
        if Instant::now() < deadline {
            record(file, CheckEvent::SocketDisconnected { requested: false })?;
            sleep_until_retry(deadline).await;
        }
    }
    let passed = results.passed(args);
    record(file, CheckEvent::Summary { passed, results })?;
    file.sync_all()?;
    if !passed {
        bail!(
            "The requested paper checks were not all observed. Read the private report, correct connection or account settings, and allow enough time for the requested outage or renewal."
        );
    }
    println!("The requested paper checks passed. Read the private report for observed results.");
    Ok(())
}

async fn sleep_until_retry(deadline: Instant) {
    let _ = timeout_at(deadline, sleep(Duration::from_secs(5))).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handoff_requires_api_reconciliation_after_the_new_gateway_is_ready() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            state_dir: dir.path().into(),
            ..Default::default()
        };
        let args = Args::try_parse_from([
            "paper-check",
            "--config",
            "unused",
            "--paper-account",
            "DU123",
            "--intents",
            "unused",
            "--report",
            "unused",
            "--expect-restart",
        ])
        .unwrap();
        let mut status = ibkit_gateway::Status {
            session: "fixture".into(),
            controller_pid: std::process::id(),
            at: chrono::Utc::now(),
            state: State::Ready,
            agents: vec![ibkit_gateway::AgentStatus {
                session: "fixture".into(),
                pid: 1001,
                at: chrono::Utc::now(),
                state: State::Ready,
                result: Default::default(),
                max_heap_mb: None,
            }],
            last_start_result: Default::default(),
            process_group: None,
            recovery: None,
        };
        let path = dir.path().join("status.json");
        fs::write(&path, serde_json::to_vec(&status).unwrap()).unwrap();
        let mut results = Results::default();
        results.observe_controller(&config);
        let old_round = Instant::now();
        results.reconciled(old_round, true, false);
        assert!(!results.passed(&args));
        status.agents[0].pid = 1002;
        fs::write(&path, serde_json::to_vec(&status).unwrap()).unwrap();
        results.observe_controller(&config);
        assert!(results.restart_handoff_observed);
        assert!(
            !results.passed(&args),
            "A new UI PID does not prove API recovery"
        );
        results.reconciled(old_round, true, false);
        assert!(
            !results.passed(&args),
            "A snapshot started before the handoff is stale"
        );
        results.reconciled(Instant::now(), false, false);
        assert!(!results.passed(&args), "The new controller must be ready");
        results.reconciled(Instant::now(), true, false);
        assert!(results.passed(&args));
        results.api_ready_at_end = false;
        assert!(
            !results.passed(&args),
            "A later disconnect must not pass on an earlier baseline"
        );
    }
}
