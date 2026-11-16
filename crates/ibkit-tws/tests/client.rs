//! End-to-end tests of the socket client against the in-process fake TWS.

use ibkit_tws::testing::{FakeTws, FakeTwsConfig};
use ibkit_tws::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::test]
async fn connects_and_waits_for_next_valid_id_and_accounts() {
    let mut tws = FakeTws::with(
        FakeTwsConfig {
            server_version: 190,
            next_valid_id: 100,
            accounts: "DU1,DU2".into(),
        },
        Arc::new(|_| vec![]),
    )
    .await;
    let (client, mut events) = Client::connect(ConnectOptions::new("127.0.0.1", tws.port(), 7))
        .await
        .unwrap();
    assert_eq!(client.server_version(), 190);
    assert_eq!(client.connection_time(), "20260108 12:00:00 EST");
    assert_eq!(client.managed_accounts(), vec!["DU1", "DU2"]);
    assert_eq!(client.next_req_id(), 100);
    assert_eq!(client.next_req_id(), 101);
    assert_eq!(tws.next_request().await, vec!["71", "2", "7", ""]);
    assert_eq!(
        events.recv().await,
        Some(Incoming::NextValidId { order_id: 100 })
    );
    assert_eq!(
        events.recv().await,
        Some(Incoming::ManagedAccounts {
            accounts: "DU1,DU2".into()
        })
    );
}

#[tokio::test]
async fn sends_requests_and_streams_messages() {
    let responder = Arc::new(|req: &[String]| {
        if req[0] == "1" {
            let id = req[2].clone();
            vec![
                tws_msg![1, 6, id, 1, 101.5, 10, 0],
                tws_msg![2, 6, id, 0, 10],
            ]
        } else {
            vec![]
        }
    });
    let mut tws = FakeTws::with(FakeTwsConfig::default(), responder).await;
    let (client, mut events) = Client::connect(ConnectOptions::new("127.0.0.1", tws.port(), 1))
        .await
        .unwrap();
    let c = Contract {
        symbol: "AAPL".into(),
        sec_type: "STK".into(),
        exchange: "SMART".into(),
        currency: "USD".into(),
        ..Default::default()
    };
    let id = client.next_req_id();
    client.req_mkt_data(id, &c, "", false, false, &[]).unwrap();
    let req = tws.next_request_with_id(1).await;
    assert_eq!(&req[..5], ["1", "11", &id.to_string(), "0", "AAPL"]);
    let mut got = Vec::new();
    while got.len() < 2 {
        let m = events.recv().await.unwrap();
        if matches!(m, Incoming::TickPrice { .. } | Incoming::TickSize { .. }) {
            got.push(m);
        }
    }
    assert!(
        matches!(got[0], Incoming::TickPrice { price, size, .. } if price == 101.5 && size == 10.0)
    );
    tws.send(tws_msg![
        4,
        2,
        -1,
        2104,
        "Market data farm connection is OK:usfarm",
        ""
    ]);
    let wait = async {
        loop {
            if let Some(Incoming::Error(e)) = events.recv().await {
                return e;
            }
        }
    };
    let e = tokio::time::timeout(Duration::from_secs(5), wait)
        .await
        .expect("error message within 5 s");
    assert_eq!(e.code, 2104);
    let stats = client.connection_stats();
    assert!(
        stats.num_msg_sent >= 2 && stats.num_msg_recv >= 5,
        "{stats:?}"
    );
}

#[tokio::test]
async fn throttles_requests() {
    let mut tws = FakeTws::start().await;
    let opts = ConnectOptions {
        max_requests: 5,
        requests_interval: Duration::from_millis(300),
        ..ConnectOptions::new("127.0.0.1", tws.port(), 1)
    };
    let (client, _events) = Client::connect(opts).await.unwrap();
    tws.next_request().await; // START_API
    let t0 = Instant::now();
    for _ in 0..12 {
        client.req_current_time().unwrap();
    }
    for _ in 0..12 {
        tws.next_request_with_id(49).await;
    }
    let elapsed = t0.elapsed();
    assert!(
        elapsed >= Duration::from_millis(550),
        "12 requests at 5 per 300 ms need two waits, took {elapsed:?}"
    );
}

#[tokio::test]
async fn reports_closed_connection() {
    let tws = FakeTws::start().await;
    let (client, mut events) = Client::connect(ConnectOptions::new("127.0.0.1", tws.port(), 1))
        .await
        .unwrap();
    client.disconnect();
    assert!(!client.is_connected());
    assert!(client.req_current_time().is_err());
    drop(tws);
    while events.recv().await.is_some() {}
}

#[tokio::test]
async fn connect_error_explains_the_fix() {
    let port = {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    };
    let err = Client::connect(ConnectOptions::new("127.0.0.1", port, 1))
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("Check that TWS or IB Gateway runs"),
        "{err}"
    );
}

#[tokio::test]
async fn rejects_too_old_server() {
    let tws = FakeTws::with(
        FakeTwsConfig {
            server_version: 150,
            ..Default::default()
        },
        Arc::new(|_| vec![]),
    )
    .await;
    let err = Client::connect(ConnectOptions::new("127.0.0.1", tws.port(), 1))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::ServerVersion {
                server: 150,
                min: 157
            }
        ),
        "{err}"
    );
}
