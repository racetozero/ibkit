use ibkit_fix::ib::{ExecutionReport, cancel_request, new_order_single};
use ibkit_fix::tags::{self, msg_type};
use ibkit_fix::*;
use ibkit_tws::types::{Contract, Order};
use std::time::{Duration, Instant};

fn pair() -> (Session, Session) {
    let i = Session::new(
        SessionConfig::initiator("CLIENT", "IB"),
        Box::new(MemoryStore::default()),
    );
    let a = Session::new(
        SessionConfig::acceptor("IB", "CLIENT"),
        Box::new(MemoryStore::default()),
    );
    (i, a)
}

fn sends(outs: &[Output]) -> Vec<Vec<u8>> {
    outs.iter()
        .filter_map(|o| {
            if let Output::Send(b) = o {
                Some(b.clone())
            } else {
                None
            }
        })
        .collect()
}

fn types(bytes: &[Vec<u8>]) -> Vec<String> {
    bytes
        .iter()
        .map(|b| MessageView::parse(b).unwrap().msg_type().to_string())
        .collect()
}

/// Deliver every message in `bytes` to `to` and return its outputs.
fn deliver(to: &mut Session, bytes: &[Vec<u8>], now: Instant) -> Vec<Output> {
    bytes.iter().flat_map(|b| to.on_raw(b, now)).collect()
}

fn logged_on() -> (Session, Session, Instant) {
    let (mut i, mut a) = pair();
    let now = Instant::now();
    let logon = i.logon(now);
    let outs = a.on_raw(&logon, now);
    assert!(outs.contains(&Output::Event(SessionEvent::LoggedOn)));
    let reply = sends(&outs);
    assert_eq!(types(&reply), ["A"]);
    let outs = deliver(&mut i, &reply, now);
    assert!(outs.contains(&Output::Event(SessionEvent::LoggedOn)));
    assert_eq!((i.state(), a.state()), (State::Active, State::Active));
    (i, a, now)
}

#[test]
fn logon_and_application_messages() {
    let (mut i, mut a, now) = logged_on();
    let mut m = Message::new(msg_type::NEW_ORDER_SINGLE);
    m.push(tags::CL_ORD_ID, "1");
    let outs = a.on_raw(&i.send(m, now), now);
    assert!(matches!(&outs[..], [Output::App(m)] if m.get(tags::CL_ORD_ID) == Some("1")));
    assert_eq!((i.next_out(), a.next_in()), (3, 3));
}

#[test]
fn gap_is_recovered_with_resend_and_gap_fill() {
    let (mut i, mut a, now) = logged_on();
    let order = |id: &str| {
        let mut m = Message::new(msg_type::NEW_ORDER_SINGLE);
        m.push(tags::CL_ORD_ID, id);
        m
    };
    let _lost_heartbeat = i.send(Message::new(msg_type::HEARTBEAT), now); // seq 2, admin
    let _lost_order = i.send(order("A"), now); // seq 3, application
    let outs = a.on_raw(&i.send(order("B"), now), now); // seq 4 arrives first
    assert!(outs.contains(&Output::Event(SessionEvent::Gap {
        expected: 2,
        received: 4
    })));
    let resend_req = sends(&outs);
    assert_eq!(types(&resend_req), ["2"]);
    let resent = sends(&deliver(&mut i, &resend_req, now));
    assert_eq!(
        types(&resent),
        ["4", "D", "D"],
        "heartbeat becomes a gap fill; orders are resent with PossDupFlag"
    );
    let first = MessageView::parse(&resent[0]).unwrap();
    assert_eq!(
        (
            first.seq_num(),
            first.get(tags::GAP_FILL_FLAG),
            first.get(tags::NEW_SEQ_NO)
        ),
        (2, Some("Y"), Some("3"))
    );
    let dup = MessageView::parse(&resent[1]).unwrap();
    assert!(dup.is_poss_dup() && dup.get(tags::ORIG_SENDING_TIME).is_some());
    let outs = deliver(&mut a, &resent, now);
    let ids: Vec<&str> = outs
        .iter()
        .filter_map(|o| {
            if let Output::App(m) = o {
                m.get(tags::CL_ORD_ID)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        ids,
        ["A", "B"],
        "the queued message is processed after the gap closes; the duplicate is ignored"
    );
    assert_eq!(a.next_in(), 5);
}

#[test]
fn unanswered_resend_request_ends_the_session() {
    let (mut i, mut a, now) = logged_on();
    let _lost = i.send(Message::new(msg_type::HEARTBEAT), now); // seq 2
    a.on_raw(&i.send(Message::new(msg_type::HEARTBEAT), now), now); // seq 3: a gap
    // Later traffic only queues behind the gap; it is no progress.
    a.on_raw(
        &i.send(Message::new(msg_type::HEARTBEAT), now),
        now + Duration::from_secs(20),
    );
    let outs = a.on_timer(now + Duration::from_secs(31));
    assert!(
        matches!(&outs[..], [Output::Disconnect(r)] if r.contains("ResendRequest")),
        "{outs:?}"
    );
}

#[test]
fn application_sends_do_not_extend_the_logon_deadline() {
    let (mut i, _a) = pair();
    let now = Instant::now();
    i.logon(now);
    i.send(
        Message::new(msg_type::NEW_ORDER_SINGLE),
        now + Duration::from_secs(20),
    );
    let outs = i.on_timer(now + Duration::from_secs(31));
    assert!(
        matches!(&outs[..], [Output::Disconnect(r)] if r.contains("no reply to Logon")),
        "{outs:?}"
    );
}

#[test]
fn silence_triggers_test_request_then_disconnect() {
    let (mut i, _a, now) = logged_on();
    let hb = Duration::from_secs(30);
    let outs = i.on_timer(now + hb + hb / 4);
    assert_eq!(
        types(&sends(&outs)),
        ["1"],
        "the TestRequest itself counts as outgoing traffic"
    );
    let outs = i.on_timer(now + hb * 3);
    assert!(matches!(&outs[..], [Output::Disconnect(r)] if r.contains("TestRequest")));
}

#[test]
fn unanswered_logon_and_logout_end_the_session() {
    let (mut i, _a) = pair();
    let now = Instant::now();
    i.logon(now);
    let outs = i.on_timer(now + Duration::from_secs(31));
    assert!(
        matches!(&outs[..], [Output::Disconnect(r)] if r.contains("no reply to Logon")),
        "{outs:?}"
    );
    let (mut i, _a, now) = logged_on();
    i.logout("", now);
    let outs = i.on_timer(now + Duration::from_secs(31));
    assert!(
        matches!(&outs[..], [Output::Disconnect(r)] if r.contains("no reply to Logout")),
        "{outs:?}"
    );
}

#[test]
fn test_request_is_answered_with_its_id() {
    let (mut i, mut a, now) = logged_on();
    let mut tr = Message::new(msg_type::TEST_REQUEST);
    tr.push(tags::TEST_REQ_ID, "X1");
    let reply = sends(&a.on_raw(&i.send(tr, now), now));
    let hb = MessageView::parse(&reply[0]).unwrap();
    assert_eq!(
        (hb.msg_type(), hb.get(tags::TEST_REQ_ID)),
        ("0", Some("X1"))
    );
}

#[test]
fn too_low_sequence_number_logs_out() {
    let (mut i, mut a, now) = logged_on();
    let m = i.send(Message::new("D"), now);
    a.on_raw(&m, now);
    let outs = a.on_raw(&m, now);
    assert!(outs.iter().any(|o| matches!(o, Output::Disconnect(r) if r.contains("MsgSeqNum too low, expecting 3 but received 2"))));
}

#[test]
fn logout_handshake() {
    let (mut i, mut a, now) = logged_on();
    let outs = a.on_raw(&i.logout("bye", now), now);
    assert!(outs.contains(&Output::Event(SessionEvent::LoggedOut("bye".into()))));
    let reply = sends(&outs);
    assert_eq!(types(&reply), ["5"]);
    let outs = deliver(&mut i, &reply, now);
    assert!(outs.iter().any(|o| matches!(o, Output::Disconnect(_))));
}

fn aapl_option() -> Contract {
    Contract {
        symbol: "AAPL".into(),
        sec_type: "OPT".into(),
        last_trade_date_or_contract_month: "20261218".into(),
        strike: 200.0,
        right: "C".into(),
        multiplier: "100".into(),
        exchange: "SMART".into(),
        currency: "USD".into(),
        local_symbol: "AAPL  261218C00200000".into(),
        ..Default::default()
    }
}

#[test]
fn ib_new_order_single_mapping() {
    let o = Order {
        action: "BUY".into(),
        total_quantity: 2.0,
        order_type: "LMT".into(),
        lmt_price: 3.5,
        tif: "GTC".into(),
        account: "U123456".into(),
        ..Default::default()
    };
    let m = new_order_single("6020.0", &aapl_option(), &o).unwrap();
    assert_eq!(m.get(21), Some("2"), "IB accepts only HandlInst 2");
    assert_eq!(m.get(58), None, "IB rejects orders with Text (58)");
    let get = |t| m.get(t).unwrap_or("-");
    assert_eq!(
        [
            get(11),
            get(55),
            get(167),
            get(200),
            get(205),
            get(201),
            get(202),
            get(231),
            get(6035),
            get(100),
            get(15),
            get(54),
            get(38),
            get(40),
            get(44),
            get(59),
            get(1),
            get(204),
            get(77)
        ],
        [
            "6020.0",
            "AAPL",
            "OPT",
            "202612",
            "18",
            "1",
            "200",
            "100",
            "AAPL  261218C00200000",
            "SMART",
            "USD",
            "1",
            "2",
            "2",
            "3.5",
            "1",
            "U123456",
            "0",
            "O"
        ]
    );
    let peg = Order {
        order_type: "PEG MID".into(),
        aux_price: 0.01,
        ..o.clone()
    };
    let m = new_order_single("2", &aapl_option(), &peg).unwrap();
    assert_eq!(
        (m.get(40), m.get(18), m.get(211)),
        (Some("P"), Some("M"), Some("0")),
        "IB needs PegDifference 0 on midpoint pegs"
    );
    let bad = Order {
        order_type: "VWAP".into(),
        ..o
    };
    assert!(
        new_order_single("3", &aapl_option(), &bad)
            .unwrap_err()
            .to_string()
            .contains("has no IB FIX mapping")
    );
    let c = cancel_request("6020.0", "6021.0", "U123456", &aapl_option(), "BUY", 2.0).unwrap();
    assert_eq!(
        (c.msg_type.as_str(), c.get(41), c.get(11), c.get(1)),
        ("F", Some("6020.0"), Some("6021.0"), Some("U123456"))
    );
    assert!(cancel_request("1", "2", "", &aapl_option(), "BUY", 2.0).is_err());
}

#[tokio::test]
async fn tcp_initiator_and_acceptor_exchange_orders_and_reports() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let (handle, mut events) = run(
            sock,
            SessionConfig::acceptor("IB", "CLIENT"),
            Box::new(MemoryStore::default()),
        );
        while let Some(e) = events.recv().await {
            if let FixEvent::App(m) = e
                && m.msg_type == msg_type::NEW_ORDER_SINGLE
            {
                let mut er = Message::new(msg_type::EXECUTION_REPORT);
                er.push(tags::ORDER_ID, "99")
                    .push(tags::CL_ORD_ID, m.get(tags::CL_ORD_ID).unwrap())
                    .push(tags::EXEC_ID, "E1")
                    .push(tags::EXEC_TRANS_TYPE, 0)
                    .push(tags::EXEC_TYPE, 2)
                    .push(tags::ORD_STATUS, 2)
                    .push(tags::SYMBOL, "AAPL")
                    .push(tags::SIDE, 1)
                    .push(tags::LEAVES_QTY, 0)
                    .push(tags::CUM_QTY, 2)
                    .push(tags::AVG_PX, 3.5)
                    .push(tags::LAST_SHARES, 2)
                    .push(tags::LAST_PX, 3.5);
                handle.send(er).unwrap();
            }
        }
    });
    let (handle, mut events) = connect(
        &addr,
        SessionConfig::initiator("CLIENT", "IB"),
        Box::new(MemoryStore::default()),
    )
    .await
    .unwrap();
    let wait = async {
        loop {
            match events.recv().await.unwrap() {
                FixEvent::Session(SessionEvent::LoggedOn) => {
                    let o = Order {
                        action: "BUY".into(),
                        total_quantity: 2.0,
                        order_type: "LMT".into(),
                        lmt_price: 3.5,
                        account: "U1".into(),
                        ..Default::default()
                    };
                    handle
                        .send(new_order_single("C1", &aapl_option(), &o).unwrap())
                        .unwrap();
                }
                FixEvent::App(m) => return ExecutionReport::from_message(&m).unwrap(),
                other => panic!("unexpected {other:?}"),
            }
        }
    };
    let er = tokio::time::timeout(Duration::from_secs(5), wait)
        .await
        .expect("execution report within 5 s");
    assert_eq!(
        (
            er.cl_ord_id.as_str(),
            er.status_name(),
            er.cum_qty,
            er.avg_px
        ),
        ("C1", "Filled", 2.0, 3.5)
    );
    handle.logout("done").unwrap();
}

#[test]
fn file_store_persists_sequence_numbers() {
    let dir = std::env::temp_dir().join(format!("ibkit-fix-{}", std::process::id()));
    let mut s = FileStore(dir.clone());
    s.save(17, 42);
    assert_eq!(FileStore(dir.clone()).load(), (17, 42));
    let _ = std::fs::remove_file(dir);
}

#[test]
fn reject_is_reported_and_sequence_reset_moves_the_expected_number() {
    let (mut i, mut a, now) = logged_on();
    let mut rej = Message::new(msg_type::REJECT);
    rej.push(tags::REF_SEQ_NUM, 2).push(tags::TEXT, "bad tag");
    let outs = i.on_raw(&a.send(rej, now), now);
    assert!(
        matches!(&outs[..], [Output::Event(SessionEvent::Reject(m))] if m.get(tags::TEXT) == Some("bad tag"))
    );
    let mut reset = Message::new(msg_type::SEQUENCE_RESET);
    reset.push(tags::NEW_SEQ_NO, 50);
    let outs = i.on_raw(&a.send(reset, now), now);
    assert_eq!(outs, [Output::Event(SessionEvent::SequenceReset(50))]);
    assert_eq!(i.next_in(), 50);
}

#[test]
fn reset_on_logon_restarts_both_sequences() {
    let (mut i, mut a, now) = logged_on();
    for _ in 0..3 {
        a.on_raw(&i.send(Message::new("D"), now), now);
    }
    let mut cfg = SessionConfig::initiator("CLIENT", "IB");
    cfg.reset_on_logon = true;
    let mut i2 = Session::new(cfg, Box::new(MemoryStore(9, 9)));
    let mut a2 = Session::new(
        SessionConfig::acceptor("IB", "CLIENT"),
        Box::new(MemoryStore(5, 5)),
    );
    let logon = i2.logon(now);
    assert_eq!(MessageView::parse(&logon).unwrap().seq_num(), 1);
    let reply = sends(&a2.on_raw(&logon, now));
    assert_eq!(
        MessageView::parse(&reply[0])
            .unwrap()
            .get(tags::RESET_SEQ_NUM_FLAG),
        Some("Y")
    );
    deliver(&mut i2, &reply, now);
    assert_eq!(
        (i2.state(), i2.next_out(), i2.next_in(), a2.next_in()),
        (State::Active, 2, 2, 2)
    );
}

#[test]
fn garbled_input_is_ignored() {
    let (mut i, _a, now) = logged_on();
    assert!(
        i.on_raw(b"8=FIX.4.2\x019=5\x0135=0\x0110=000\x01", now)
            .is_empty()
    );
    assert!(MessageView::parse(b"not fix").is_err());
}

/// Raw message from IB to CLIENT with sequence number `seq` and the given header.
fn raw_from_ib(msg_type: &str, seq: u64, sender: &str, sending_time: Option<&str>) -> Vec<u8> {
    let seq = seq.to_string();
    let mut header = vec![(49, sender), (56, "CLIENT"), (34, seq.as_str())];
    if let Some(t) = sending_time {
        header.push((52, t));
    }
    let mut out = Vec::new();
    Message::new(msg_type).encode("FIX.4.2", &header, &mut out);
    out
}

fn field(bytes: &[u8], tag: u32) -> String {
    MessageView::parse(bytes)
        .unwrap()
        .get(tag)
        .unwrap_or("")
        .to_string()
}

#[test]
fn wrong_comp_id_is_rejected_then_logged_out() {
    let (mut i, _a, now) = logged_on();
    let ts = ibkit_fix::message::utc_timestamp(std::time::SystemTime::now());
    let outs = i.on_raw(&raw_from_ib("0", 2, "SOMEONE", Some(&ts)), now);
    let sent = sends(&outs);
    assert_eq!(types(&sent), ["3", "5"]);
    assert_eq!(
        (
            field(&sent[0], 373),
            field(&sent[0], 371),
            field(&sent[0], 45)
        ),
        ("9".into(), "49".into(), "2".into())
    );
    assert!(matches!(outs.last(), Some(Output::Disconnect(t)) if t.contains("CompID problem")));
    assert_eq!(i.state(), State::Closed);
}

#[test]
fn stale_sending_time_is_rejected_then_logged_out() {
    let (mut i, _a, now) = logged_on();
    let old = std::time::SystemTime::now() - Duration::from_secs(600);
    let ts = ibkit_fix::message::utc_timestamp(old);
    let outs = i.on_raw(&raw_from_ib("0", 2, "IB", Some(&ts)), now);
    let sent = sends(&outs);
    assert_eq!(types(&sent), ["3", "5"]);
    assert_eq!(field(&sent[0], 373), "10");
    assert_eq!(i.state(), State::Closed);
}

#[test]
fn missing_sending_time_is_rejected_and_uses_up_the_sequence_number() {
    let (mut i, _a, now) = logged_on();
    let outs = i.on_raw(&raw_from_ib("8", 2, "IB", None), now);
    let sent = sends(&outs);
    assert_eq!(types(&sent), ["3"]);
    assert_eq!(
        (field(&sent[0], 373), field(&sent[0], 371)),
        ("1".into(), "52".into())
    );
    assert!(!outs.iter().any(|o| matches!(o, Output::App(_))));
    assert_eq!((i.state(), i.next_in()), (State::Active, 3));
}

#[test]
fn unsupported_application_message_gets_business_reject() {
    let (mut i, mut a, now) = logged_on();
    a.cfg.app_msg_types = Some(vec!["D".into()]);
    let outs = a.on_raw(&i.send(Message::new("AF"), now), now);
    let sent = sends(&outs);
    assert_eq!(types(&sent), ["j"]);
    assert_eq!(
        (
            field(&sent[0], 372),
            field(&sent[0], 380),
            field(&sent[0], 45)
        ),
        ("AF".into(), "3".into(), "2".into())
    );
    assert!(!outs.iter().any(|o| matches!(o, Output::App(_))));
    let outs = a.on_raw(&i.send(Message::new("D"), now), now);
    assert!(matches!(&outs[..], [Output::App(m)] if m.msg_type == "D"));
}

#[test]
fn file_store_resends_original_messages_after_restart() {
    let path = std::env::temp_dir().join(format!("ibkit-fix-resend-{}", std::process::id()));
    let mut store = FileStore(path.clone());
    store.reset();
    let now = Instant::now();
    let mut a = Session::new(
        SessionConfig::acceptor("IB", "CLIENT"),
        Box::new(MemoryStore::default()),
    );
    let mut i = Session::new(
        SessionConfig::initiator("CLIENT", "IB"),
        Box::new(store.clone()),
    );
    let reply = sends(&a.on_raw(&i.logon(now), now));
    deliver(&mut i, &reply, now);
    let mut order = Message::new(msg_type::NEW_ORDER_SINGLE);
    order.push(tags::CL_ORD_ID, "keep-me");
    i.send(order, now);
    drop(i);

    // Restarted process: same file, asked to resend from 2.
    let mut i = Session::new(
        SessionConfig::initiator("CLIENT", "IB"),
        Box::new(store.clone()),
    );
    assert_eq!(i.next_out(), 3);
    let mut resend = Message::new(msg_type::RESEND_REQUEST);
    resend.push(tags::BEGIN_SEQ_NO, 2).push(tags::END_SEQ_NO, 0);
    let outs = i.on_raw(&a.send(resend, now), now);
    let sent = sends(&outs);
    assert_eq!(types(&sent), ["D"]);
    assert_eq!(
        (
            field(&sent[0], 11),
            field(&sent[0], 34),
            field(&sent[0], 43)
        ),
        ("keep-me".into(), "2".into(), "Y".into())
    );
    assert!(!field(&sent[0], 122).is_empty());
    store.reset();
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn tls_initiator_logs_on_to_a_tls_acceptor() {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use std::sync::Arc;
    const CERT: &[u8] = include_bytes!("data/cert.pem");
    const KEY: &[u8] = include_bytes!("data/key.pem");
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cert = CertificateDer::from_pem_slice(CERT).unwrap();
    let server = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.clone()],
            PrivateKeyDer::from_pem_slice(KEY).unwrap(),
        )
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert).unwrap();
    let client = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let tls = acceptor.accept(sock).await.unwrap();
        let (_handle, mut events) = run(
            tls,
            SessionConfig::acceptor("IB", "CLIENT"),
            Box::new(MemoryStore::default()),
        );
        while events.recv().await.is_some() {}
    });
    let (handle, mut events) = connect_tls(
        &addr,
        "localhost",
        Arc::new(client),
        SessionConfig::initiator("CLIENT", "IB"),
        Box::new(MemoryStore::default()),
    )
    .await
    .unwrap();
    let first = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("logon within 5 s");
    assert_eq!(first, Some(FixEvent::Session(SessionEvent::LoggedOn)));
    handle.logout("done").unwrap();

    let err = connect_tls(
        &addr,
        "not a host name",
        Arc::new(
            rustls::ClientConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth(),
        ),
        SessionConfig::initiator("CLIENT", "IB"),
        Box::new(MemoryStore::default()),
    )
    .await
    .err()
    .unwrap();
    assert!(err.to_string().contains("pass the host name"), "{err}");
}

/// (tag, value) pairs of `m` in order, as "tag=value" strings.
fn pairs(m: &Message) -> Vec<String> {
    m.fields.iter().map(|(t, v)| format!("{t}={v}")).collect()
}

fn limit(account: &str) -> Order {
    Order {
        action: "BUY".into(),
        total_quantity: 100.0,
        order_type: "LMT".into(),
        lmt_price: 150.0,
        account: account.into(),
        ..Default::default()
    }
}

fn ibm() -> Contract {
    Contract {
        primary_exchange: "NYSE".into(),
        ..Contract::stock("IBM", "SMART", "USD")
    }
}

#[test]
fn order_types_use_ib_codes() {
    use ibkit_tws::consts::UNSET_DOUBLE;
    let base = Order {
        aux_price: 0.5,
        ..limit("U1")
    };
    type Case<'a> = (&'a str, Order, &'a [(u32, &'a str)]);
    let cases: [Case; 12] = [
        ("MIT", base.clone(), &[(40, "J"), (99, "0.5")]),
        ("LIT", base.clone(), &[(40, "LT"), (44, "150"), (99, "0.5")]),
        (
            "REL",
            base.clone(),
            &[(40, "P"), (18, "R"), (211, "0.5"), (44, "150")],
        ),
        (
            "PEG MKT",
            base.clone(),
            &[(40, "P"), (18, "P"), (211, "0.5")],
        ),
        ("TRAIL", base.clone(), &[(40, "P"), (18, "a"), (211, "0.5")]),
        (
            "TRAIL",
            Order {
                trailing_percent: 5.0,
                aux_price: UNSET_DOUBLE,
                ..base.clone()
            },
            &[(40, "P"), (18, "a"), (211, "5"), (6268, "100")],
        ),
        (
            "TRAIL LIMIT",
            Order {
                lmt_price_offset: -0.1,
                ..base.clone()
            },
            &[(40, "TSL"), (44, "150"), (211, "0.5"), (6370, "-0.1")],
        ),
        ("MIDPRICE", base.clone(), &[(40, "MIDPX"), (44, "150")]),
        ("PASSV REL", base.clone(), &[(40, "PSVR"), (211, "0.5")]),
        ("SNAP MKT", base.clone(), &[(40, "SMKT"), (211, "0.5")]),
        ("SNAP MID", base.clone(), &[(40, "SMID"), (211, "0.5")]),
        (
            "PEG BEST",
            Order {
                min_compete_size: 100,
                compete_against_best_offset: f64::INFINITY,
                ..base.clone()
            },
            &[(40, "E2M"), (8411, "100"), (8412, "UpToMid")],
        ),
    ];
    for (order_type, o, want) in cases {
        let o = Order {
            order_type: order_type.into(),
            ..o
        };
        let m = ib::new_order_single("1", &ibm(), &o).unwrap();
        for (tag, value) in want {
            assert_eq!(m.get(*tag), Some(*value), "{order_type} tag {tag}");
        }
    }
    let stk = Order {
        order_type: "PEG STK".into(),
        stock_ref_price: 140.0,
        delta: 50.0,
        starting_price: 2.0,
        ..limit("U1")
    };
    let ats = Contract {
        exchange: "IBKRATS".into(),
        ..ibm()
    };
    let best = Order {
        order_type: "PEG BEST".into(),
        ..limit("U1")
    };
    let m = ib::new_order_single("3", &ats, &best).unwrap();
    assert_eq!((m.get(40), m.get(8032)), (Some("E2M"), Some("1")));
    let m = ib::new_order_single("2", &aapl_option(), &stk).unwrap();
    assert_eq!(
        (m.get(18), m.get(6151), m.get(6154), m.get(99), m.get(6271)),
        (Some("s"), Some("140"), Some("50"), Some("2"), Some("1"))
    );
}

#[test]
fn order_fields_map_to_ib_custom_tags() {
    use ibkit_tws::types::{OrderCondition, TagValue};
    let o = Order {
        tif: "GTD".into(),
        good_till_date: "20261231 16:00:00 US/Eastern".into(),
        good_after_time: "20261230-14:30:00".into(),
        oca_group: "g1".into(),
        oca_type: 3,
        hidden: true,
        discretionary_amt: 0.05,
        order_ref: "ref-7".into(),
        trigger_method: 2,
        what_if: true,
        all_or_none: true,
        manual_order_indicator: 0,
        ext_operator: "bot1".into(),
        mifid2_decision_maker: "DM1".into(),
        use_price_mgmt_algo: Some(true),
        conditions_cancel_order: true,
        conditions: vec![
            OrderCondition::Price {
                conjunction: "o".into(),
                is_more: true,
                price: 200.0,
                con_id: 8314,
                exch: "SMART".into(),
                trigger_method: 1,
            },
            OrderCondition::Margin {
                conjunction: "a".into(),
                is_more: false,
                percent: 30,
            },
        ],
        ..limit("U1")
    };
    let m = ib::new_order_single("C1", &ibm(), &o).unwrap();
    let want: &[(u32, &str)] = &[
        (59, "6"),
        (126, "20261231-21:00:00"),
        (168, "20261230-14:30:00"),
        (583, "IBOCAGROUP g1"),
        (6209, "ReduceOnFillNonBlock"),
        (111, "0"),
        (388, "0"),
        (389, "0.05"),
        (6010, "ref-7"),
        (6115, "2"),
        (6091, "1"),
        (18, "G"),
        (1028, "N"),
        (8089, "bot1"),
        (8237, "DM1"),
        (8339, "1"),
        (6205, "1"),
        (6136, "2"),
        (6579, "1"),
    ];
    for (tag, value) in want {
        assert_eq!(m.get(*tag), Some(*value), "tag {tag}");
    }
    let p = pairs(&m);
    let conds = p
        .iter()
        .skip_while(|f| !f.starts_with("6222="))
        .cloned()
        .collect::<Vec<_>>()
        .join("|");
    assert!(
        conds.starts_with("6222=1|6123=8314|6124=SMART|6125=200|6126=>=|6127=1|6137=o|6222=4|6126=<=|6245=30|6137=n"),
        "{conds}"
    );

    let algo = Order {
        algo_strategy: "Vwap".into(),
        algo_params: vec![
            TagValue {
                tag: "maxPctVol".into(),
                value: "0.1".into(),
            },
            TagValue {
                tag: "startTime".into(),
                value: "20261230 09:45:00 US/Eastern".into(),
            },
            TagValue {
                tag: "noTakeLiq".into(),
                value: "true".into(),
            },
        ],
        outside_rth: true,
        ..limit("U1")
    };
    let m = ib::new_order_single("C2", &ibm(), &algo).unwrap();
    assert_eq!(
        (
            m.get(18),
            m.get(847),
            m.get(849),
            m.get(8708),
            m.get(8704),
            m.get(6205)
        ),
        (
            Some("e"),
            Some("Vwap"),
            Some("0.1"),
            Some("20261230-14:45:00"),
            Some("1"),
            Some("1")
        )
    );
    let bad = Order {
        algo_params: vec![TagValue {
            tag: "bogus".into(),
            value: "1".into(),
        }],
        ..algo
    };
    assert!(
        ib::new_order_single("C3", &ibm(), &bad)
            .unwrap_err()
            .to_string()
            .contains("has no IB FIX tag")
    );
    let local = Order {
        tif: "GTD".into(),
        good_till_date: "20261231 16:00:00".into(),
        ..limit("U1")
    };
    assert!(
        ib::new_order_single("C4", &ibm(), &local)
            .unwrap_err()
            .to_string()
            .contains("convert to UTC")
    );
    let by_date = Order {
        tif: "GTD".into(),
        good_till_date: "20261231".into(),
        ..limit("U1")
    };
    let m = ib::new_order_single("C5", &ibm(), &by_date).unwrap();
    assert_eq!((m.get(432), m.get(126)), (Some("20261231"), None));
}

#[test]
fn ib_order_rules_are_checked() {
    let no_account = ib::new_order_single("1", &ibm(), &limit("")).unwrap_err();
    assert!(
        no_account.to_string().contains("Account (tag 1)"),
        "{no_account}"
    );
    let smart_only = Contract::stock("IBM", "SMART", "");
    assert!(
        ib::new_order_single("2", &smart_only, &limit("U1"))
            .unwrap_err()
            .to_string()
            .contains("tag 15 or 207")
    );
    let es = Contract {
        sec_type: "FUT".into(),
        last_trade_date_or_contract_month: "202612".into(),
        ..Contract::stock("ES", "SMART", "USD")
    };
    assert!(
        ib::new_order_single("3", &es, &limit("U1"))
            .unwrap_err()
            .to_string()
            .contains("no SMART routing for futures")
    );
    let bond = Contract {
        sec_type: "BOND".into(),
        sec_id_type: "ISIN".into(),
        sec_id: "US912828XX00".into(),
        ..Contract::stock("T", "SMART", "USD")
    };
    let m = ib::new_order_single("4", &bond, &limit("U1")).unwrap();
    assert_eq!(
        (m.get(48), m.get(22), m.get(167)),
        (Some("US912828XX00"), Some("4"), Some("BOND"))
    );
    let cash = Order {
        cash_qty: 500.0,
        total_quantity: 0.0,
        order_type: "MKT".into(),
        ..limit("U1")
    };
    let m = ib::new_order_single("5", &ibm(), &cash).unwrap();
    assert_eq!((m.get(152), m.get(38)), (Some("500"), None));
    let short = Order {
        action: "SSHORT".into(),
        short_sale_slot: 2,
        designated_location: "ABCD".into(),
        ..limit("U1")
    };
    let m = ib::new_order_single("6", &ibm(), &short).unwrap();
    assert_eq!(
        (m.get(54), m.get(6086), m.get(114), m.get(5700)),
        (Some("5"), Some("2"), Some("N"), Some("ABCD"))
    );
    let status = ib::order_status_request("U1", "*");
    assert_eq!(
        (status.msg_type.as_str(), pairs(&status)),
        ("H", vec!["1=U1".to_string(), "11=*".into()])
    );
    let paused = Order {
        deactivate: true,
        ..limit("U1")
    };
    let g = ib::cancel_replace("1", "2", &ibm(), &paused).unwrap();
    assert_eq!((g.get(21), g.get(6521)), (Some("2"), Some("1")));
}

#[test]
fn multileg_orders_follow_ibs_leg_block() {
    use ibkit_fix::ib::*;
    use ibkit_tws::types::TagValue;
    let put = Contract {
        right: "P".into(),
        strike: 190.0,
        local_symbol: String::new(),
        ..aapl_option()
    };
    let call = Contract {
        local_symbol: String::new(),
        ..aapl_option()
    };
    let combo = Contract {
        symbol: "AAPL".into(),
        sec_type: "BAG".into(),
        exchange: "SMART".into(),
        primary_exchange: "CBOE".into(),
        currency: "USD".into(),
        ..Default::default()
    };
    let o = Order {
        total_quantity: 1.0,
        lmt_price: 1.25,
        smart_combo_routing_params: vec![TagValue {
            tag: "NonGuaranteed".into(),
            value: "1".into(),
        }],
        ..limit("U1")
    };
    let legs = [
        Leg {
            contract: &call,
            ratio: 1.0,
            action: "BUY",
            open_close: "",
        },
        Leg {
            contract: &put,
            ratio: 1.0,
            action: "SELL",
            open_close: "C",
        },
    ];
    let m = new_order_multileg("C9", &combo, &legs, &o).unwrap();
    assert_eq!(m.msg_type, "AB");
    let p = pairs(&m);
    assert_eq!(
        &p[..8],
        [
            "11=C9",
            "21=2",
            "55=AAPL",
            "167=MULTILEG",
            "207=CBOE",
            "15=USD",
            "100=SMART",
            "6248=1"
        ]
    );
    let legs_text = p
        .iter()
        .skip_while(|f| !f.starts_with("555="))
        .take(23)
        .cloned()
        .collect::<Vec<_>>()
        .join("|");
    assert_eq!(
        legs_text,
        "555=2|600=AAPL|609=OPT|608=OCXXXX|611=20261218|612=200|614=100|616=SMART|564=O|623=1|624=1|654=1|600=AAPL|609=OPT|608=OPXXXX|611=20261218|612=190|614=100|616=SMART|564=C|623=1|624=2|654=2"
    );
    assert!(
        new_order_multileg("C10", &combo, &legs[..1], &o)
            .unwrap_err()
            .to_string()
            .contains("at least two")
    );
    let stock = Contract::stock("AAPL", "SMART", "USD");
    let stock_first = [
        Leg {
            contract: &stock,
            ratio: 100.0,
            action: "BUY",
            open_close: "",
        },
        legs[0].clone(),
    ];
    assert!(
        new_order_multileg("C11", &combo, &stock_first, &o)
            .unwrap_err()
            .to_string()
            .contains("stock leg last")
    );

    let ac = multileg_cancel_replace(
        "C9",
        "C12",
        &combo,
        &Order {
            lmt_price: 1.3,
            ..o.clone()
        },
    )
    .unwrap();
    assert_eq!(
        (
            ac.msg_type.as_str(),
            ac.get(41),
            ac.get(11),
            ac.get(44),
            ac.get(555)
        ),
        ("AC", Some("C9"), Some("C12"), Some("1.3"), None)
    );

    let j = allocation_instruction(
        "A1",
        AllocTransType::New,
        None,
        "F1",
        &Contract::stock("LCID", "SMART", "USD"),
        "BUY",
        2.0,
        2.535,
        "20240624",
        &[("U234567", 1.0), ("U345678", 1.0)],
    )
    .unwrap();
    let p = pairs(&j);
    assert_eq!(&p[..4], ["70=A1", "71=0", "1=F1", "54=1"]);
    assert!(
        p.join("|").contains(
            "53=2|6=2.535|75=20240624|78=2|79=U234567|80=1|153=2.535|79=U345678|80=1|153=2.535"
        ),
        "{p:?}"
    );
    assert!(
        allocation_instruction(
            "A2",
            AllocTransType::Cancel,
            None,
            "F1",
            &Contract::stock("LCID", "SMART", "USD"),
            "BUY",
            2.0,
            2.535,
            "20240624",
            &[]
        )
        .unwrap_err()
        .to_string()
        .contains("ref_alloc_id")
    );
}

#[test]
fn typed_views_read_ib_messages() {
    use ibkit_fix::ib::*;
    let mut m = Message::new("8");
    for (t, v) in [
        (37, "00015c22.1"),
        (11, "96014709.0"),
        (150, "A"),
        (20, "3"),
        (39, "A"),
        (6091, "1"),
        (6092, "21603.94"),
        (6094, "6000329.05"),
        (8130, "USD"),
        (6379, "0.1"),
        (442, "2"),
        (654, "1"),
        (6013, "0:2"),
        (6959, "1"),
    ] {
        m.push(t, v);
    }
    let er = ExecutionReport::from_message(&m).unwrap();
    assert_eq!(
        (
            er.what_if.init_margin,
            er.what_if.equity_with_loan,
            er.what_if.min_commission
        ),
        (Some(21603.94), Some(6000329.05), Some(0.1))
    );
    assert_eq!(
        (
            er.multi_leg_reporting_type.as_str(),
            er.leg_ref_id.as_str(),
            er.combo_leg_info.as_str(),
            er.liquidation
        ),
        ("2", "1", "0:2", true)
    );
    assert_eq!(er.status_name(), "PendingNew");

    let mut b = Message::new("B");
    b.push(61, 1)
        .push(148, "Important Bulletin Message")
        .push(6143, 42805703)
        .push(207, "ALL");
    let b = Bulletin::from_message(&b).unwrap();
    assert_eq!(
        (b.urgency.as_str(), b.daily_new_id.as_str()),
        ("1", "42805703")
    );

    let mut p = Message::new("P");
    p.push(70, "ptaorder#99").push(75, "20240624").push(87, 3);
    let ack = AllocationAck::from_message(&p).unwrap();
    assert_eq!(
        (ack.alloc_id.as_str(), ack.status_name()),
        ("ptaorder#99", "Received")
    );

    let mut r = Message::new("9");
    r.push(37, "X")
        .push(11, "2")
        .push(41, "1")
        .push(102, 1)
        .push(434, 1)
        .push(58, "No such order");
    let r = OrderCancelReject::from_message(&r).unwrap();
    assert_eq!(
        (r.cxl_rej_reason.as_str(), r.text.as_str()),
        ("1", "No such order")
    );
}

#[test]
fn initiator_adopts_the_heartbeat_interval_of_the_logon_reply() {
    let now = Instant::now();
    let mut client = Session::new(
        SessionConfig::initiator("CLIENT", "IB"),
        Box::new(MemoryStore::default()),
    );
    let logon = client.logon(now);
    let mut reply = Vec::new();
    let mut m = Message::new(msg_type::LOGON);
    m.push(98, 0).push(108, 10);
    m.encode(
        "FIX.4.2",
        &[
            (49, "IB"),
            (56, "CLIENT"),
            (34, "1"),
            (
                52,
                &ibkit_fix::message::utc_timestamp(std::time::SystemTime::now()),
            ),
        ],
        &mut reply,
    );
    assert!(!logon.is_empty());
    client.on_raw(&reply, now);
    assert_eq!(client.state(), State::Active);
    assert_eq!(client.cfg.heartbeat_interval, Duration::from_secs(10));
}

#[test]
fn sequence_reset_that_decreases_the_expected_number_is_rejected() {
    let (_, mut b, now) = logged_on();
    let expected = b.next_in();
    let mut m = Message::new(msg_type::SEQUENCE_RESET);
    m.push(36, 1);
    let mut raw = Vec::new();
    m.encode(
        "FIX.4.2",
        &[
            (49, "CLIENT"),
            (56, "IB"),
            (34, "99"),
            (
                52,
                &ibkit_fix::message::utc_timestamp(std::time::SystemTime::now()),
            ),
        ],
        &mut raw,
    );
    let out = b.on_raw(&raw, now);
    let sent = sends(&out);
    assert_eq!(types(&sent), ["3"]);
    assert_eq!(field(&sent[0], 373), "5");
    assert!(field(&sent[0], 58).contains("decrease receive sequence number"));
    assert_eq!(b.next_in(), expected);
}

#[tokio::test]
async fn compressed_sessions_exchange_fixcomp_blocks() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let (raw_tx, mut raw_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let (mut sock, _) = listener.accept().await.unwrap();
        // Read the first block raw, then run an acceptor over a replay of it.
        let mut first = vec![0u8; 4096];
        let n = sock.read(&mut first).await.unwrap();
        raw_tx.send(first[..n].to_vec()).unwrap();
        let (client, server) = tokio::io::duplex(1 << 16);
        let (mut cr, mut cw) = tokio::io::split(client);
        let (mut sr, mut sw) = tokio::io::split(sock);
        use tokio::io::AsyncWriteExt;
        cw.write_all(&first[..n]).await.unwrap();
        tokio::spawn(async move { tokio::io::copy(&mut sr, &mut cw).await });
        tokio::spawn(async move { tokio::io::copy(&mut cr, &mut sw).await });
        let mut cfg = SessionConfig::acceptor("IB", "CLIENT");
        cfg.compression = true;
        let (_h, mut events) = run(server, cfg, Box::new(MemoryStore::default()));
        while events.recv().await.is_some() {}
    });
    let mut cfg = SessionConfig::initiator("CLIENT", "IB");
    cfg.compression = true;
    let (handle, mut events) = connect(&addr, cfg, Box::new(MemoryStore::default()))
        .await
        .unwrap();
    let first = raw_rx.recv().await.unwrap();
    assert!(
        first.starts_with(b"8=FIXCOMP\x019="),
        "the Logon is compressed"
    );
    let logged_on = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let FixEvent::Session(SessionEvent::LoggedOn) = events.recv().await.unwrap() {
                return;
            }
        }
    })
    .await;
    assert!(logged_on.is_ok(), "compressed logon reply is inflated");
    handle.logout("done").unwrap();
}

#[test]
fn extras_bench_pair_auction_and_exercise_follow_ib_tags() {
    use ibkit_fix::ib::*;
    let es = Contract {
        sec_type: "FUT".into(),
        last_trade_date_or_contract_month: "202612".into(),
        ..Contract::stock("ES", "CME", "USD")
    };
    let fut = Order {
        manual_order_indicator: 0,
        ext_operator: "bot1".into(),
        ..limit("U1")
    };
    let missing = new_order_single("F1", &es, &fut).unwrap_err();
    assert!(
        missing.to_string().contains("sender_location_id"),
        "{missing}"
    );
    let extras = OrderExtras {
        parent_cl_ord_id: Some("P1"),
        sender_location_id: Some("US,NY"),
        allocations: &[("U2", 60.0), ("U3", 40.0)],
        disable_fa_close_first: true,
        ..Default::default()
    };
    let m = new_order_single_with("F2", &es, &fut, &extras).unwrap();
    assert_eq!(
        (
            m.get(6107),
            m.get(142),
            m.get(1028),
            m.get(8089),
            m.get(6806)
        ),
        (
            Some("P1"),
            Some("US,NY"),
            Some("N"),
            Some("bot1"),
            Some("1")
        )
    );
    assert!(
        pairs(&m).join("|").contains("78=2|79=U2|80=60|79=U3|80=40"),
        "{:?}",
        pairs(&m)
    );

    let bench = Order {
        order_type: "PEG BENCH".into(),
        lmt_price: 151.0,
        stock_ref_price: 300.0,
        pegged_change_amount: 0.2,
        reference_change_amount: 1.0,
        is_pegged_change_amount_decrease: true,
        ..limit("U1")
    };
    let m = new_order_single("B1", &ibm(), &bench).unwrap();
    assert_eq!(
        (
            m.get(40),
            m.get(18),
            m.get(99),
            m.get(6580),
            m.get(6938),
            m.get(6939),
            m.get(6151)
        ),
        (
            Some("PB"),
            Some("R"),
            Some("151"),
            Some("300"),
            Some("-0.2"),
            Some("1"),
            None
        )
    );

    let pair = Order {
        hedge_type: "P".into(),
        hedge_param: "1.8".into(),
        auction_strategy: 2,
        ..limit("U1")
    };
    let m = new_order_single("H1", &ibm(), &pair).unwrap();
    assert_eq!(
        (m.get(6665), m.get(6666), m.get(6173)),
        (Some("3"), Some("1.8"), Some("2"))
    );
    let beta = Order {
        hedge_type: "B".into(),
        ..limit("U1")
    };
    assert!(new_order_single("H2", &ibm(), &beta).is_err());

    let ex = option_exercise(
        "X1",
        &aapl_option(),
        &limit("U1"),
        ExerciseType::Exercise,
        true,
    )
    .unwrap();
    assert_eq!((ex.get(6809), ex.get(8012)), (Some("1"), Some("1")));
    assert!(
        option_exercise(
            "X2",
            &aapl_option(),
            &limit("U1"),
            ExerciseType::Lapse,
            true
        )
        .is_err()
    );
    assert!(option_exercise("X3", &ibm(), &limit("U1"), ExerciseType::Exercise, false).is_err());
}
