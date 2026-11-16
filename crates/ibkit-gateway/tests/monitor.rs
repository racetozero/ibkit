//! Alerts and metrics from the controller's event stream, with a local
//! webhook receiver.
use axum::{Json, Router, routing::post};
use chrono::Utc;
use ibkit_gateway::{
    ErrorCode, Event, EventKind, StartResult, State, Status,
    api::{Alert, AlertKind},
    monitor::{Alerts, Metrics, run},
    recovery::RecoveryState,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc};

fn event(kind: EventKind) -> Event {
    Event {
        at: Utc::now(),
        session: "s1".into(),
        pid: 1,
        kind,
    }
}

#[tokio::test]
async fn faults_and_recovery_send_alerts_and_count_in_metrics() {
    let (seen, mut received) = mpsc::unbounded_channel::<Alert>();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    let app = Router::new().route(
        "/hook",
        post(move |Json(alert): Json<Alert>| {
            let seen = seen.clone();
            async move {
                seen.send(alert).unwrap();
            }
        }),
    );
    tokio::spawn(async move { axum::serve(listener, app).await });

    let metrics = Arc::new(Metrics::default());
    let (events, receiver) = broadcast::channel(16);
    let task = tokio::spawn(run(
        receiver,
        metrics.clone(),
        tokio::sync::watch::channel(Some(Alerts::new(&url, metrics.clone()).unwrap())).1,
    ));
    let lost = StartResult::new(ErrorCode::ConnectionLost);
    for kind in [
        EventKind::Started,
        EventKind::StateChanged {
            state: State::Ready,
        },
        EventKind::Error {
            result: lost.clone(),
        },
        // A second fault while down sends no second alert.
        EventKind::Error { result: lost },
        EventKind::RecoveryScheduled {
            attempt: 1,
            retry_at: Utc::now(),
            reason: ErrorCode::ConnectionLost,
        },
        EventKind::StateChanged {
            state: State::Ready,
        },
        EventKind::Halted {
            reason: ErrorCode::LoginFailed,
            exhausted: false,
            message: "Gateway is halted: wrong password".into(),
        },
    ] {
        events.send(event(kind)).unwrap();
    }
    let mut kinds = Vec::new();
    for _ in 0..3 {
        let alert = tokio::time::timeout(Duration::from_secs(5), received.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(alert.text.starts_with("ibkit: "), "{}", alert.text);
        assert_eq!(alert.session, "s1");
        kinds.push((alert.alert, alert.reason));
    }
    kinds.sort_by_key(|(kind, _)| format!("{kind:?}"));
    assert_eq!(
        kinds,
        [
            (AlertKind::Down, ErrorCode::ConnectionLost),
            (AlertKind::Halted, ErrorCode::LoginFailed),
            (AlertKind::Recovered, ErrorCode::None),
        ]
    );
    drop(events);
    task.await.unwrap();
    // The counter moves after the webhook answers.
    tokio::time::timeout(Duration::from_secs(5), async {
        while !metrics
            .render(None)
            .contains("ibkit_gateway_alerts_sent_total 3")
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let text = metrics.render(None);
    for line in [
        "ibkit_gateway_up 0",
        "ibkit_gateway_gateway_launches_total 1",
        "ibkit_gateway_errors_total{code=\"ConnectionLost\"} 2",
        "ibkit_gateway_recoveries_scheduled_total 1",
        "ibkit_gateway_halts_total 1",
        "ibkit_gateway_alerts_sent_total 3",
    ] {
        assert!(text.contains(line), "{line} is absent from:\n{text}");
    }
}

#[test]
fn metrics_list_every_state_and_recovery_phase() {
    let status = Status {
        session: "s1".into(),
        controller_pid: 1,
        at: Utc::now(),
        state: State::AwaitingSecondFactor,
        agents: Vec::new(),
        last_start_result: StartResult::new(ErrorCode::None),
        recovery: Some(RecoveryState::Blocked {
            reason: ErrorCode::LoginFailed,
        }),
        process_group: None,
    };
    let text = Metrics::default().render(Some(&status));
    for line in [
        "ibkit_gateway_up 1",
        "ibkit_gateway_state{state=\"awaiting_second_factor\"} 1",
        "ibkit_gateway_state{state=\"recovering\"} 0",
        "ibkit_gateway_recovery_phase{phase=\"blocked\"} 1",
        "ibkit_gateway_recovery_phase{phase=\"awaiting_agent\"} 0",
    ] {
        assert!(text.contains(line), "{line} is absent from:\n{text}");
    }
    assert_eq!(text.matches("ibkit_gateway_state{").count(), 12);
    assert_eq!(text.matches("ibkit_gateway_recovery_phase{").count(), 6);
}
