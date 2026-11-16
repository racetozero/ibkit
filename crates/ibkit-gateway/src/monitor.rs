//! Metrics and alerts. One task reads the controller's event stream: it
//! counts events for `GET /v1/metrics` (Prometheus text format) and POSTs
//! an [`Alert`] to `alert_webhook_url` when Gateway goes down, comes back,
//! halts for an operator, or the controller fails.
use crate::{
    ErrorCode, Event, EventKind, State, Status,
    api::{Alert, AlertKind},
    recovery::RecoveryState,
};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use std::{
    collections::BTreeMap,
    fmt::Write,
    sync::{Arc, LazyLock},
    time::Duration,
};
use tokio::sync::broadcast;

#[derive(Default)]
struct Counters {
    launches: u64,
    errors: BTreeMap<String, u64>,
    recoveries_scheduled: u64,
    restarts: u64,
    api_unavailable: u64,
    halts: u64,
    alerts_sent: u64,
    alerts_failed: u64,
    events_dropped: u64,
    last_ready: Option<DateTime<Utc>>,
}

/// Counters of one controller process. Prometheus treats the reset at a
/// controller restart as a counter reset.
pub struct Metrics {
    started: DateTime<Utc>,
    counters: Mutex<Counters>,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            started: Utc::now(),
            counters: Mutex::default(),
        }
    }
}

/// Every controller state and recovery phase name, from the types' JSON
/// schemas, so a new variant gets its gauge.
static STATES: LazyLock<Vec<String>> = LazyLock::new(|| {
    let schema = schemars::schema_for!(State).to_value();
    schema["enum"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|name| name.as_str().map(String::from))
        .collect()
});
static PHASES: LazyLock<Vec<String>> = LazyLock::new(|| {
    let schema = schemars::schema_for!(RecoveryState).to_value();
    schema["oneOf"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|variant| {
            variant["properties"]["phase"]["const"]
                .as_str()
                .map(String::from)
        })
        .collect()
});

/// The serde name of a unit variant, or the tag of a tagged one.
fn name_of(value: &impl serde::Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        Ok(serde_json::Value::Object(fields)) => fields
            .get("phase")
            .and_then(|p| p.as_str())
            .unwrap_or_default()
            .into(),
        _ => String::new(),
    }
}

impl Metrics {
    fn record(&self, event: &Event) {
        let mut c = self.counters.lock();
        match &event.kind {
            EventKind::Started => c.launches += 1,
            EventKind::Error { result } => {
                *c.errors.entry(name_of(&result.error_code)).or_default() += 1;
            }
            EventKind::RecoveryScheduled { .. } => c.recoveries_scheduled += 1,
            EventKind::Restarted => c.restarts += 1,
            EventKind::ApiUnavailable => c.api_unavailable += 1,
            EventKind::Halted { .. } => c.halts += 1,
            EventKind::StateChanged {
                state: State::Ready,
            } => c.last_ready = Some(event.at),
            _ => {}
        }
    }

    /// The Prometheus text exposition of these counters and of `status`
    /// (`None` when the controller has not saved one).
    pub fn render(&self, status: Option<&Status>) -> String {
        let now = Utc::now();
        let mut out = String::new();
        let mut metric = |name: &str, kind: &str, help: &str, samples: &[(String, f64)]| {
            let _ = writeln!(
                out,
                "# HELP ibkit_gateway_{name} {help}\n# TYPE ibkit_gateway_{name} {kind}"
            );
            for (labels, value) in samples {
                let _ = writeln!(out, "ibkit_gateway_{name}{labels} {value}");
            }
        };
        let seconds = |at: DateTime<Utc>| at.timestamp_millis() as f64 / 1000.0;
        let one = |value: f64| [(String::new(), value)];
        let c = self.counters.lock();
        let live = status.is_some_and(Status::is_live);
        metric(
            "up",
            "gauge",
            "1 while the controller runs and saves its status.",
            &one(f64::from(u8::from(live))),
        );
        metric(
            "ready",
            "gauge",
            "1 while Gateway is logged in and configured.",
            &one(f64::from(u8::from(
                status.is_some_and(|s| s.state == State::Ready),
            ))),
        );
        metric(
            "controller_start_time_seconds",
            "gauge",
            "Start time of this controller process.",
            &one(seconds(self.started)),
        );
        if let Some(status) = status {
            metric(
                "status_age_seconds",
                "gauge",
                "Seconds since the controller saved its status.",
                &one((now - status.at).num_milliseconds() as f64 / 1000.0),
            );
            let state = name_of(&status.state);
            let states: Vec<_> = STATES
                .iter()
                .map(|s| {
                    (
                        format!("{{state=\"{s}\"}}"),
                        f64::from(u8::from(*s == state)),
                    )
                })
                .collect();
            metric(
                "state",
                "gauge",
                "1 for the current controller state.",
                &states,
            );
            let phase = status.recovery.as_ref().map_or("idle".into(), name_of);
            let phases: Vec<_> = PHASES
                .iter()
                .map(|p| {
                    (
                        format!("{{phase=\"{p}\"}}"),
                        f64::from(u8::from(*p == phase)),
                    )
                })
                .collect();
            metric(
                "recovery_phase",
                "gauge",
                "1 for the current automatic recovery phase; blocked waits for an operator.",
                &phases,
            );
            if let Some(RecoveryState::Waiting { retry_at, .. }) = status.recovery {
                metric(
                    "recovery_retry_time_seconds",
                    "gauge",
                    "Time of the next automatic login attempt.",
                    &one(seconds(retry_at)),
                );
            }
            let heartbeat: Vec<_> = status
                .agents
                .iter()
                .map(|a| {
                    (
                        format!("{{pid=\"{}\"}}", a.pid),
                        (now - a.at).num_milliseconds() as f64 / 1000.0,
                    )
                })
                .collect();
            metric(
                "agent_heartbeat_age_seconds",
                "gauge",
                "Seconds since each Gateway JVM agent reported.",
                &heartbeat,
            );
            let heap: Vec<_> = status
                .agents
                .iter()
                .filter_map(|a| {
                    Some((
                        format!("{{pid=\"{}\"}}", a.pid),
                        a.max_heap_mb? as f64 * 1048576.0,
                    ))
                })
                .collect();
            metric(
                "jvm_max_heap_bytes",
                "gauge",
                "Maximum heap of each Gateway JVM.",
                &heap,
            );
        }
        if let Some(at) = c.last_ready {
            metric(
                "last_ready_time_seconds",
                "gauge",
                "Time Gateway last became ready.",
                &one(seconds(at)),
            );
        }
        let errors: Vec<_> = c
            .errors
            .iter()
            .map(|(code, n)| (format!("{{code=\"{code}\"}}"), *n as f64))
            .collect();
        metric(
            "errors_total",
            "counter",
            "Login and runtime faults by error code.",
            &errors,
        );
        for (name, help, count) in [
            (
                "gateway_launches_total",
                "Gateway processes started.",
                c.launches,
            ),
            (
                "recoveries_scheduled_total",
                "Automatic login attempts scheduled.",
                c.recoveries_scheduled,
            ),
            (
                "gateway_restarts_total",
                "Completed Gateway auto-restarts.",
                c.restarts,
            ),
            (
                "api_unavailable_total",
                "Times the local API stopped answering probes.",
                c.api_unavailable,
            ),
            (
                "halts_total",
                "Times the controller halted Gateway.",
                c.halts,
            ),
            (
                "alerts_sent_total",
                "Alerts that the webhook accepted.",
                c.alerts_sent,
            ),
            (
                "alerts_failed_total",
                "Alerts that failed after all retries.",
                c.alerts_failed,
            ),
            (
                "events_dropped_total",
                "Events the metrics task missed because it lagged.",
                c.events_dropped,
            ),
        ] {
            metric(name, "counter", help, &one(count as f64));
        }
        out
    }
}

/// Sends alerts to `alert_webhook_url`.
#[derive(Clone)]
pub struct Alerts {
    url: Arc<str>,
    http: reqwest::Client,
    metrics: Arc<Metrics>,
}

impl Alerts {
    pub fn new(url: &str, metrics: Arc<Metrics>) -> anyhow::Result<Self> {
        Ok(Self {
            url: url.into(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()?,
            metrics,
        })
    }

    /// POST `alert`, retrying after 2, 10 and 30 seconds. A webhook that
    /// never answers costs at most 80 seconds and does not stop the controller.
    pub async fn send(&self, alert: Alert) {
        let mut last = String::new();
        for delay in [0, 2, 10, 30] {
            tokio::time::sleep(Duration::from_secs(delay)).await;
            match self.http.post(&*self.url).json(&alert).send().await {
                Ok(response) if response.status().is_success() => {
                    self.metrics.counters.lock().alerts_sent += 1;
                    return;
                }
                Ok(response) => last = format!("the webhook answered {}", response.status()),
                Err(error) => last = error.without_url().to_string(),
            }
        }
        self.metrics.counters.lock().alerts_failed += 1;
        eprintln!(
            "The {:?} alert was not delivered ({last}). Check alert_webhook_url and the network path to it.",
            alert.alert
        );
    }
}

/// Build the alert of `kind` for an event of `session`.
pub fn alert(
    kind: AlertKind,
    session: &str,
    state: State,
    reason: ErrorCode,
    message: String,
) -> Alert {
    let summary = match kind {
        AlertKind::Down => "Gateway is down",
        AlertKind::Recovered => "Gateway is logged in again",
        AlertKind::Halted => "Gateway is halted and needs an operator",
        AlertKind::Exhausted => "Gateway recovery is paused by the hourly attempt budget",
        AlertKind::ControllerFailed => "The ibkit controller stopped with an error",
    };
    Alert {
        alert: kind,
        text: if message.is_empty() {
            format!("ibkit: {summary}.")
        } else {
            format!("ibkit: {summary}: {message}")
        },
        at: Utc::now(),
        session: session.into(),
        state,
        reason,
        message,
    }
}

/// Count events and send alerts until the event stream closes.
/// Record metrics for every event and send alerts through the webhook that
/// `alerts` holds (a reload can change it).
pub async fn run(
    mut events: broadcast::Receiver<Event>,
    metrics: Arc<Metrics>,
    alerts: tokio::sync::watch::Receiver<Option<Alerts>>,
) {
    // Set by a fault, cleared when Gateway is ready again.
    let mut down = false;
    loop {
        let event = match events.recv().await {
            Ok(event) => event,
            Err(broadcast::error::RecvError::Lagged(n)) => {
                metrics.counters.lock().events_dropped += n;
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => return,
        };
        metrics.record(&event);
        let Some(alerts) = alerts.borrow().clone() else {
            continue;
        };
        let alert = match &event.kind {
            EventKind::Error { result } if !down => {
                down = true;
                alert(
                    AlertKind::Down,
                    &event.session,
                    State::Failed,
                    result.error_code,
                    result.error_message.clone(),
                )
            }
            EventKind::Halted {
                reason,
                exhausted,
                message,
            } => {
                down = true;
                let kind = if *exhausted {
                    AlertKind::Exhausted
                } else {
                    AlertKind::Halted
                };
                alert(
                    kind,
                    &event.session,
                    State::Halted,
                    *reason,
                    message.clone(),
                )
            }
            EventKind::StateChanged {
                state: State::Ready,
            } if down => {
                down = false;
                alert(
                    AlertKind::Recovered,
                    &event.session,
                    State::Ready,
                    ErrorCode::None,
                    String::new(),
                )
            }
            _ => continue,
        };
        tokio::spawn(async move { alerts.send(alert).await });
    }
}
