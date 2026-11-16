//! Web API tests. OAuth vectors were computed with Python's standard library
//! following the same steps as ibind's `oauth1a.py`.

use ibkit_web::oauth::*;
use ibkit_web::ws::{WsAuth, WsClient, parse_message, topics};
use ibkit_web::{WebClient, WebConfig, WebOrder};
use std::collections::BTreeMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const PRIME: &str = "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7EDEE386BFB5A899FA5AE9F24117C4B1FE649286651ECE65381FFFFFFFFFFFFFFFF";
const RANDOM: &str = "1f2e3d4c5b6a79881f2e3d4c5b6a79881f2e3d4c5b6a79881f2e3d4c5b6a7988";
const B: &str = "ca178765fd7b67fad2e772bb972fac6fb0fb28493fd24d542e51a0e303784f498f9695630c267bf709f302d3a2bf4c2a67657d03df09222a077866b871ecc95a253664e94c13375c00dec40a35ab859c4d36b1d6e1a7946dbd845b0bec58f8c9e56a9a62b2d6a607f759515c731560c7d4eb63a8d76655c3a7e61c4374dd723d";

#[test]
fn quote_plus_matches_python() {
    assert_eq!(
        quote_plus("a b/c=d&e~f_g.h-i+é"),
        "a+b%2Fc%3Dd%26e~f_g.h-i%2B%C3%A9"
    );
}

#[test]
fn live_session_token_matches_python() {
    assert!(
        dh_challenge(PRIME, RANDOM, 2)
            .unwrap()
            .starts_with("cb6532b34345441d394024747ecc8f5036c3e90b")
    );
    let lst = live_session_token(PRIME, RANDOM, B, "0a1b2c3d4e5f").unwrap();
    assert_eq!(lst, "6igHtnmpJdVTqN1ogYYQOEwedPk=");
    assert!(validate_live_session_token(
        &lst,
        "f23dff2fb9e47b50764b04ba5f9ed56337b4c53a",
        "TESTCONS"
    ));
    assert!(!validate_live_session_token(&lst, "00", "TESTCONS"));
}

#[test]
fn base_string_and_hmac_signature_match_python() {
    let mut p = BTreeMap::new();
    for (k, v) in [
        ("oauth_consumer_key", "TESTCONS"),
        ("oauth_nonce", "abc"),
        ("oauth_signature_method", "HMAC-SHA256"),
        ("oauth_timestamp", "1700000000"),
        ("oauth_token", "tok"),
        ("conids", "265598,8314"),
    ] {
        p.insert(k.to_string(), v.to_string());
    }
    let bs = base_string(
        "GET",
        "https://api.ibkr.com/v1/api/iserver/marketdata/snapshot",
        &p,
        None,
    );
    assert_eq!(
        bs,
        "GET&https%3A%2F%2Fapi.ibkr.com%2Fv1%2Fapi%2Fiserver%2Fmarketdata%2Fsnapshot&conids%3D265598%2C8314%26oauth_consumer_key%3DTESTCONS%26oauth_nonce%3Dabc%26oauth_signature_method%3DHMAC-SHA256%26oauth_timestamp%3D1700000000%26oauth_token%3Dtok"
    );
    assert_eq!(
        hmac_sha256_signature(&bs, "6igHtnmpJdVTqN1ogYYQOEwedPk=").unwrap(),
        "8%2ByIg8ekVZGTgyjGEaiGbZKeks6dGmuRF86oQUZ6FK8%3D"
    );
}

#[test]
fn rsa_prepend_and_signature_round_trip() {
    use base64::Engine;
    use rsa::pkcs1::EncodeRsaPrivateKey;
    use rsa::traits::PublicKeyParts;
    let mut rng = rand::thread_rng();
    let key = rsa::RsaPrivateKey::new(&mut rng, 1024).unwrap();
    let pem = key.to_pkcs1_pem(rsa::pkcs8::LineEnding::LF).unwrap();
    let parsed = read_private_key(&pem).unwrap();
    assert_eq!(parsed.n(), key.n());
    let secret = [0xde, 0xad, 0xbe, 0xef];
    let enc = key
        .to_public_key()
        .encrypt(&mut rng, rsa::Pkcs1v15Encrypt, &secret)
        .unwrap();
    let b64 = base64::engine::general_purpose::STANDARD.encode(enc);
    assert_eq!(
        live_session_token_prepend(&b64, &parsed).unwrap(),
        "deadbeef"
    );
    let sig = rsa_sha256_signature("prependPOST&x&y", &parsed);
    assert!(
        !sig.contains('/') && !sig.contains('+'),
        "signature is URL-quoted: {sig}"
    );
    let header = authorization_header(
        &BTreeMap::from([("oauth_signature".to_string(), sig)]),
        "limited_poa",
    );
    assert!(header.starts_with("OAuth realm=\"limited_poa\", oauth_signature=\""));
}

#[test]
fn bad_key_error_explains_the_fix() {
    let err = read_private_key("not a key").unwrap_err();
    assert!(err.to_string().contains("Pass the PEM text of the key"));
}

/// Minimal HTTP/1.1 server answering each request with `body`; returns the
/// base URL and a receiver of raw requests.
async fn http_server(body: &'static str) -> (String, tokio::sync::mpsc::UnboundedReceiver<String>) {
    let (root, rx) = http_server_seq(vec![body.into()]).await;
    (format!("{root}/v1/api"), rx)
}

/// HTTP server answering the n-th request with `bodies[n]` (the last one
/// repeats); `{root}` in a body becomes the server's `http://host:port`.
async fn http_server_seq(
    bodies: Vec<String>,
) -> (String, tokio::sync::mpsc::UnboundedReceiver<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let root = format!("http://{}", listener.local_addr().unwrap());
    let bodies: Vec<String> = bodies.iter().map(|b| b.replace("{root}", &root)).collect();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut n = 0;
        while let Ok((mut sock, _)) = listener.accept().await {
            let tx = tx.clone();
            let body = bodies[n.min(bodies.len() - 1)].clone();
            n += 1;
            tokio::spawn(async move {
                let mut req = Vec::new();
                let mut buf = vec![0u8; 65536];
                loop {
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    req.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&req).to_lowercase();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let len = text
                            .lines()
                            .find_map(|l| {
                                l.strip_prefix("content-length: ")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if req.len() >= end + 4 + len {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                let _ = tx.send(String::from_utf8_lossy(&req).into_owned());
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    (root, rx)
}

#[tokio::test]
async fn rest_client_sends_paths_queries_and_bodies() {
    let (url, mut requests) = http_server(r#"[{"id":"abc","message":["confirm?"]}]"#).await;
    let web = WebClient::new(WebConfig {
        base_url: url,
        ..Default::default()
    })
    .unwrap();
    let snap = web
        .live_marketdata_snapshot(&[265598, 8314], &["31", "84"])
        .await
        .unwrap();
    assert_eq!(snap[0]["id"], "abc");
    let req = requests.recv().await.unwrap();
    assert!(
        req.starts_with(
            "GET /v1/api/iserver/marketdata/snapshot?conids=265598%2C8314&fields=31%2C84 HTTP/1.1"
        ),
        "{req}"
    );

    let order = WebOrder {
        conid: 265598,
        order_type: "LMT".into(),
        price: Some(150.0),
        side: "BUY".into(),
        tif: "DAY".into(),
        quantity: 10.0,
        manual_indicator: Some(false),
        ext_operator: Some("algo1".into()),
        ..Default::default()
    };
    web.place_order("DU1", &[order]).await.unwrap();
    let req = requests.recv().await.unwrap();
    assert!(
        req.starts_with("POST /v1/api/iserver/account/DU1/orders HTTP/1.1"),
        "{req}"
    );
    let body: serde_json::Value =
        serde_json::from_str(req.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(
        body,
        serde_json::json!({"orders": [{"conid": 265598, "orderType": "LMT", "outsideRTH": false, "price": 150.0, "quantity": 10.0, "side": "BUY", "tif": "DAY", "manualIndicator": false, "extOperator": "algo1"}]}),
        "{req}"
    );

    web.cancel_order("DU1", 42, Some(false), Some("algo1"), None)
        .await
        .unwrap();
    let req = requests.recv().await.unwrap();
    assert!(
        req.starts_with(
            "DELETE /v1/api/iserver/account/DU1/order/42?manualIndicator=false&extOperator=algo1 HTTP/1.1"
        ),
        "{req}"
    );

    web.marketdata_history_by_conid(
        265598,
        "1h",
        None,
        Some("1d"),
        None,
        None,
        Some(1),
        Some("Bid_Ask"),
    )
    .await
    .unwrap();
    let req = requests.recv().await.unwrap();
    assert!(
        req.starts_with(
            "GET /v1/api/iserver/marketdata/history?conid=265598&bar=1h&period=1d&direction=1&source=Bid_Ask HTTP/1.1"
        ),
        "{req}"
    );
    let old: WebOrder = serde_json::from_value(serde_json::json!({"conid": 1, "orderType": "MKT", "outsideRth": true, "side": "BUY", "tif": "DAY", "quantity": 1.0})).unwrap();
    assert!(old.outside_rth, "the outsideRth spelling is still read");
}

#[tokio::test]
async fn oauth_requests_need_a_live_session_token() {
    let cfg = OAuthConfig::new("CONS", "TOKEN", "c2VjcmV0", "", "", PRIME);
    let web = WebClient::new(WebConfig {
        base_url: "http://127.0.0.1:9/v1/api".into(),
        ..WebConfig::oauth(cfg)
    })
    .unwrap();
    let err = web.portfolio_accounts().await.unwrap_err();
    assert!(err.to_string().contains("call oauth_init"), "{err}");
}

#[test]
fn ws_topics_and_parsing() {
    assert_eq!(
        topics::market_data(265598, &["31", "84"]),
        r#"smd+265598+{"fields":["31","84"]}"#
    );
    assert_eq!(topics::market_data_unsubscribe(265598), "umd+265598+{}");
    assert_eq!(
        topics::orders(&["submitted", "filled"]),
        r#"sor+{"filters":["submitted","filled"]}"#
    );
    assert_eq!(topics::orders(&[]), "sor+{}");
    // Unsubscribe strings exactly as IBKR documents them.
    assert_eq!(topics::pnl_unsubscribe(), "upl+{}");
    assert_eq!(topics::account_summary_unsubscribe("DU1"), "usd+DU1+{}");
    assert_eq!(topics::account_ledger_unsubscribe("DU1"), "uld+DU1+{}");
    assert_eq!(topics::book_trader_unsubscribe("DU1"), "ubd+DU1");
    assert_eq!(topics::market_history_unsubscribe("7"), "umh+7");
    assert_eq!(topics::orders_unsubscribe(), "uor+{}");
    assert_eq!(topics::trades_unsubscribe(), "utr");
    assert_eq!(
        topics::exercise_options(497222760),
        r#"shs+exercise+{"CEX":"497222760"}"#
    );
    let input: serde_json::Value = serde_json::from_str(
        topics::exercise_options_input("5", "submit", Some(5.0), true)
            .strip_prefix("inp+exercise+")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        input,
        serde_json::json!({"action": "user_input", "data": {"id": "5", "user_action": "submit", "exercise": {"allowed": "not_shown", "make_final": true, "value": 5.0}}})
    );
    assert_eq!(
        topics::book_trader("DU1", 265598, Some("NASDAQ")),
        "sbd+DU1+265598+NASDAQ"
    );
    let m = parse_message(r#"{"topic":"smd+265598","31":"150.25","conid":265598}"#);
    assert_eq!(m.topic.as_deref(), Some("smd"));
    assert_eq!(m.args, vec!["265598"]);
    assert_eq!(m.data["31"], "150.25");
    assert_eq!(
        parse_message(r#"{"message":"waiting for session"}"#).topic,
        None
    );
}

/// WebSocket server that confirms the session after `delay`, records each
/// received text frame with the time since accept, and answers `smd`.
async fn ws_server(
    delay: std::time::Duration,
) -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<(std::time::Duration, String)>,
) {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let start = std::time::Instant::now();
        let ws = tokio_tungstenite::accept_async(sock).await.unwrap();
        let (mut sink, mut source) = ws.split();
        let (reply_tx, mut reply_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            sink.send(Message::Text(
                r#"{"topic":"system","success":"user1"}"#.into(),
            ))
            .await
            .unwrap();
            while let Some(r) = reply_rx.recv().await {
                if sink.send(Message::Text(r.into())).await.is_err() {
                    break;
                }
            }
        });
        while let Some(Ok(msg)) = source.next().await {
            if let Message::Text(t) = msg {
                if t.starts_with("smd+") {
                    let _ = reply_tx.send(r#"{"topic":"smd+265598","31":"150.25"}"#.to_string());
                }
                let _ = tx.send((start.elapsed(), t.to_string()));
            }
        }
    });
    (format!("ws://{addr}/v1/api/ws"), rx)
}

#[tokio::test]
async fn ws_client_waits_for_the_session_then_subscribes_and_receives() {
    let delay = std::time::Duration::from_millis(300);
    let (url, mut frames) = ws_server(delay).await;
    let auth = WsAuth {
        session: Some("s1".into()),
        heartbeat: None,
        ..Default::default()
    };
    let mut ws = WsClient::connect(&url, auth).await.unwrap();
    let system = ws.recv().await.unwrap();
    assert_eq!(
        system.topic.as_deref(),
        Some("system"),
        "the confirmation is delivered too"
    );
    ws.subscribe_market_data(265598, &["31"]).unwrap();
    let (at, frame) = tokio::time::timeout(std::time::Duration::from_secs(5), frames.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(frame.starts_with("smd+265598"), "{frame}");
    assert!(
        at >= delay,
        "the subscription was sent {at:?} after accept, before the session was confirmed"
    );
    let m = tokio::time::timeout(std::time::Duration::from_secs(5), ws.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (m.topic.as_deref(), m.data["31"].as_str()),
        (Some("smd"), Some("150.25"))
    );
}

#[tokio::test]
async fn ws_client_renews_market_data_and_stops_after_unsubscribe() {
    let (url, mut frames) = ws_server(std::time::Duration::ZERO).await;
    let auth = WsAuth {
        heartbeat: None,
        renew_market_data: Some(std::time::Duration::from_millis(100)),
        ..Default::default()
    };
    let ws = WsClient::connect(&url, auth).await.unwrap();
    ws.subscribe_market_data(265598, &["31"]).unwrap();
    let mut smd = 0;
    while smd < 2 {
        let (_, f) = tokio::time::timeout(std::time::Duration::from_secs(5), frames.recv())
            .await
            .unwrap()
            .unwrap();
        smd += usize::from(f.starts_with("smd+265598"));
    }
    ws.unsubscribe_market_data(265598).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
    let mut late = Vec::new();
    while let Ok((_, f)) = frames.try_recv() {
        late.push(f);
    }
    let after_unsubscribe: Vec<&String> = late
        .iter()
        .skip_while(|f| !f.starts_with("umd+"))
        .filter(|f| f.starts_with("smd+"))
        .collect();
    assert!(
        after_unsubscribe.is_empty(),
        "renewal continued after unsubscribe: {late:?}"
    );
}

#[tokio::test]
async fn ws_client_fails_when_the_session_is_not_confirmed() {
    let (url, _frames) = ws_server(std::time::Duration::from_secs(60)).await;
    let auth = WsAuth {
        confirm_timeout: Some(std::time::Duration::from_millis(200)),
        ..Default::default()
    };
    let err = WsClient::connect(&url, auth)
        .await
        .err()
        .expect("no confirmation");
    assert!(err.to_string().contains("was not confirmed"), "{err}");
}

#[tokio::test]
async fn order_confirmation_stops_after_the_reply_limit() {
    let (url, _requests) = http_server(r#"[{"id":"q1","message":["Are you sure?"]}]"#).await;
    let web = WebClient::new(WebConfig {
        base_url: url,
        ..Default::default()
    })
    .unwrap();
    let order = WebOrder {
        conid: 1,
        order_type: "MKT".into(),
        side: "BUY".into(),
        tif: "DAY".into(),
        quantity: 1.0,
        ..Default::default()
    };
    let err = web
        .place_order_confirmed("DU1", &[order], 2)
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("still need confirmation after 2 replies"),
        "{err}"
    );
}

#[tokio::test]
async fn http_errors_name_the_method_url_and_status() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let _ = sock.read(&mut buf).await;
        let body = "not authenticated";
        let resp = format!(
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = sock.write_all(resp.as_bytes()).await;
    });
    let web = WebClient::new(WebConfig {
        base_url: format!("http://{addr}/v1/api"),
        ..Default::default()
    })
    .unwrap();
    let err = web.iserver_accounts().await.unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("GET")
            && text.contains("iserver/accounts")
            && text.contains("401")
            && text.contains("not authenticated"),
        "{text}"
    );
}

#[tokio::test]
async fn oauth2_exchanges_assertions_for_a_bearer_token() {
    use ibkit_web::OAuth2Config;
    use rsa::pkcs1::EncodeRsaPrivateKey;
    let key = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 1024).unwrap();
    let pem = key.to_pkcs1_pem(rsa::pkcs8::LineEnding::LF).unwrap();
    let (root, mut requests) = http_server_seq(vec![
        r#"{"access_token":"tok-1","authenticated":true}"#.into(),
    ])
    .await;
    let mut cfg = OAuth2Config::new("client", "kid", "user1", &pem, "1.2.3.4");
    cfg.auth_url = root.clone();
    let web = WebClient::new(WebConfig {
        base_url: format!("{root}/v1/api"),
        ..WebConfig::oauth2(cfg)
    })
    .unwrap();
    let err = web.portfolio_accounts().await.unwrap_err();
    assert!(err.to_string().contains("call oauth2_init"), "{err}");

    web.oauth2_init(true).await.unwrap();
    let token = requests.recv().await.unwrap();
    assert!(
        token.starts_with("POST /oauth2/api/v1/token HTTP/1.1"),
        "{token}"
    );
    let form = token.split("\r\n\r\n").nth(1).unwrap();
    for field in [
        "grant_type=client_credentials",
        "client_assertion_type=urn%3Aietf%3Aparams%3Aoauth%3Aclient-assertion-type%3Ajwt-bearer",
        "scope=sso-sessions.write",
        "client_assertion=",
    ] {
        assert!(form.contains(field), "{form}");
    }
    let session = requests.recv().await.unwrap().to_lowercase();
    assert!(
        session.starts_with("post /gw/api/v1/sso-sessions http/1.1"),
        "{session}"
    );
    assert!(session.contains("authorization: bearer tok-1"), "{session}");
    assert!(
        session.contains("content-type: application/jwt"),
        "{session}"
    );
    assert_eq!(
        session.split("\r\n\r\n").nth(1).unwrap().split('.').count(),
        3
    );
    let init = requests.recv().await.unwrap();
    assert!(
        init.starts_with("POST /v1/api/iserver/auth/ssodh/init"),
        "{init}"
    );
    assert!(
        init.to_lowercase().contains("authorization: bearer tok-1"),
        "{init}"
    );
    assert_eq!(web.live_session_token().as_deref(), Some("tok-1"));
}

#[tokio::test]
async fn flex_download_polls_until_the_statement_is_ready() {
    let (root, mut requests) = http_server_seq(vec![
        "<FlexStatementResponse><Status>Success</Status><ReferenceCode>R1</ReferenceCode><Url>{root}/GetStatement</Url></FlexStatementResponse>".into(),
        "<FlexStatementResponse><Status>Warn</Status><ErrorCode>1019</ErrorCode></FlexStatementResponse>".into(),
        r#"<FlexQueryResponse queryName="q"><FlexStatements count="1"/></FlexQueryResponse>"#.into(),
    ])
    .await;
    let range = ibkit_web::flex::FlexPeriod::Range {
        from: "20260101",
        to: "20260131",
    };
    let xml =
        ibkit_web::flex::download_from(&root, "TOKEN", "123", range, 3, std::time::Duration::ZERO)
            .await
            .unwrap();
    assert!(xml.starts_with("<FlexQueryResponse"), "{xml}");
    assert!(
        requests
            .recv()
            .await
            .unwrap()
            .starts_with("GET /SendRequest?t=TOKEN&q=123&v=3&fd=20260101&td=20260131 ")
    );
    assert!(
        requests
            .recv()
            .await
            .unwrap()
            .starts_with("GET /GetStatement?q=R1&t=TOKEN&v=3 ")
    );

    let (root, _) = http_server_seq(vec![
        "<FlexStatementResponse><Status>Fail</Status><ErrorCode>1012</ErrorCode><ErrorMessage>Token has expired.</ErrorMessage></FlexStatementResponse>".into(),
    ])
    .await;
    let err = ibkit_web::flex::download_from(
        &root,
        "OLD",
        "123",
        ibkit_web::flex::FlexPeriod::Days(30),
        3,
        std::time::Duration::ZERO,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("1012 Token has expired."), "{err}");
}

#[tokio::test]
async fn alert_event_contract_and_fa_endpoints_use_documented_paths() {
    let (url, mut requests) = http_server("{}").await;
    let web = WebClient::gateway(&url).unwrap();
    web.activate_alert("DU1", 9, true).await.unwrap();
    web.alert_details(9).await.unwrap();
    web.forecast_market(766914406, Some("FORECASTX"))
        .await
        .unwrap();
    web.fa_allocation_group_delete("g1").await.unwrap();
    web.fyi_set_email_delivery(false).await.unwrap();
    web.server_prompt_reply(5, "r1", "Yes").await.unwrap();
    let mut next = async || requests.recv().await.unwrap();
    let r = next().await;
    assert!(
        r.starts_with("POST /v1/api/iserver/account/DU1/alert/activate "),
        "{r}"
    );
    let body: serde_json::Value =
        serde_json::from_str(r.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body, serde_json::json!({"alertId": 9, "alertActive": 1}));
    assert!(
        next()
            .await
            .starts_with("GET /v1/api/iserver/account/alert/9?type=Q ")
    );
    assert!(next().await.starts_with(
        "GET /v1/api/forecast/contract/market?underlyingConid=766914406&exchange=FORECASTX "
    ));
    assert!(
        next()
            .await
            .starts_with("POST /v1/api/iserver/account/allocation/group/delete ")
    );
    assert!(
        next()
            .await
            .starts_with("PUT /v1/api/fyi/deliveryoptions/email?enabled=false ")
    );
    let r = next().await;
    assert!(r.starts_with("POST /v1/api/iserver/notification "), "{r}");
    let body: serde_json::Value =
        serde_json::from_str(r.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(
        body,
        serde_json::json!({"orderId": 5, "reqId": "r1", "text": "Yes"})
    );

    web.pa_allocation(&["U1".into()], "ASSET_CLASS", Some("EUR"), None, None)
        .await
        .unwrap();
    let r = next().await;
    assert!(r.starts_with("POST /v1/api/pa/allocation "), "{r}");
    let body: serde_json::Value =
        serde_json::from_str(r.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(
        body,
        serde_json::json!({"acctIds": ["U1"], "type": "ASSET_CLASS", "currency": "EUR"})
    );
    web.fa_model_rebalance_to_existing_targets(&serde_json::json!({"reqID": 1, "model": "m1"}))
        .await
        .unwrap();
    assert!(
        next()
            .await
            .starts_with("POST /v1/api/fa/model/rebalance/to-existing-targets ")
    );
    web.fa_allocation_models().await.unwrap();
    assert!(
        next()
            .await
            .starts_with("GET /v1/api/iserver/account/allocation/models ")
    );
}

/// Claims of a compact JWT (no signature check).
fn jwt_claims(jwt: &str) -> serde_json::Value {
    use base64::Engine;
    let part = jwt.split('.').nth(1).expect("claims part");
    serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(part)
            .expect("base64url"),
    )
    .expect("json claims")
}

#[tokio::test]
async fn gw_endpoints_use_the_access_token_and_signed_jwt_bodies() {
    use ibkit_web::OAuth2Config;
    use rsa::pkcs1::EncodeRsaPrivateKey;
    let key = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 1024).unwrap();
    let pem = key.to_pkcs1_pem(rsa::pkcs8::LineEnding::LF).unwrap();
    let (root, mut requests) =
        http_server_seq(vec![r#"{"access_token":"acc-1","expires_in":600}"#.into()]).await;
    let mut cfg = OAuth2Config::new("client", "kid", "user1", &pem, "1.2.3.4");
    cfg.auth_url = root.clone();
    cfg.scope = "accounts.read bank-instructions.write restrictions.read".into();
    let web = WebClient::new(WebConfig::oauth2(cfg)).unwrap();

    web.am_account_status("U123").await.unwrap();
    let token = requests.recv().await.unwrap();
    assert!(token.starts_with("POST /oauth2/api/v1/token "), "{token}");
    assert!(
        token.contains("scope=accounts.read+bank-instructions.write+restrictions.read"),
        "{token}"
    );
    let get = requests.recv().await.unwrap();
    assert!(
        get.starts_with("GET /gw/api/v1/accounts/U123/status "),
        "{get}"
    );
    assert!(
        get.to_lowercase().contains("authorization: bearer acc-1"),
        "{get}"
    );

    // The access token is reused; a JSON query object is sent as one string.
    web.am_accounts_status(&serde_json::json!({"startDate": "2026-01-15"}))
        .await
        .unwrap();
    let get = requests.recv().await.unwrap();
    assert!(
        get.starts_with(
            "GET /gw/api/v1/accounts/status?accountStatusRequest=%7B%22startDate%22%3A%222026-01-15%22%7D "
        ),
        "{get}"
    );

    web.am_bank_instruction(&serde_json::json!({"instructionType": "ACH_INSTRUCTION", "instruction": {"clientInstructionId": 7}}))
        .await
        .unwrap();
    let post = requests.recv().await.unwrap();
    assert!(
        post.starts_with("POST /gw/api/v1/bank-instructions "),
        "{post}"
    );
    let lower = post.to_lowercase();
    assert!(lower.contains("content-type: application/jwt"), "{post}");
    assert!(lower.contains("authorization: bearer acc-1"), "{post}");
    let claims = jwt_claims(post.split("\r\n\r\n").nth(1).unwrap());
    assert_eq!(claims["instructionType"], "ACH_INSTRUCTION");
    assert_eq!(claims["instruction"]["clientInstructionId"], 7);
    assert_eq!(claims["iss"], "client");
    assert_eq!(
        claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap(),
        60
    );

    web.restrictions_for_user("F100", "master1", "trader2", true)
        .await
        .unwrap();
    let get = requests.recv().await.unwrap();
    assert!(
        get.starts_with(
            "GET /gw/api/v1/restrictions/user?masterUserName=master1&isEmpTrack=T&userName=trader2 "
        ),
        "{get}"
    );
    let auth = get
        .lines()
        .find_map(|l| l.strip_prefix("authorization: Bearer "))
        .expect("bearer JWT");
    assert_eq!(jwt_claims(auth)["accountId"], "F100");

    let file = web.tax_voucher_download("R1").await.unwrap();
    assert_eq!(
        file,
        br#"{"access_token":"acc-1","expires_in":600}"#.to_vec()
    );
    let get = requests.recv().await.unwrap();
    assert!(
        get.starts_with("GET /gw/api/v1/tax-vouchers/R1/download "),
        "{get}"
    );

    let err = web
        .am_bank_instruction(&serde_json::json!(["not", "an", "object"]))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("must be a JSON object"), "{err}");
}

#[tokio::test]
async fn oauth1_third_party_token_requests_are_rsa_signed() {
    use rsa::pkcs1::EncodeRsaPrivateKey;
    let key = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 1024).unwrap();
    let pem = key.to_pkcs1_pem(rsa::pkcs8::LineEnding::LF).unwrap();
    let (root, mut requests) = http_server_seq(vec![
        r#"{"oauth_token":"a1b2c3","oauth_token_secret":"c2VjcmV0","is_true":false}"#.into(),
    ])
    .await;
    let cfg = OAuthConfig::new("CONS", "", "", "", &pem, PRIME);
    let web = WebClient::new(WebConfig {
        base_url: format!("{root}/v1/api"),
        ..WebConfig::oauth(cfg)
    })
    .unwrap();
    assert_eq!(web.oauth_request_token().await.unwrap(), "a1b2c3");
    let req = requests.recv().await.unwrap();
    assert!(
        req.starts_with("POST /v1/api/oauth/request_token "),
        "{req}"
    );
    assert!(req.contains("oauth_callback=\"oob\""), "{req}");
    assert!(
        req.contains("oauth_signature_method=\"RSA-SHA256\""),
        "{req}"
    );
    assert!(!req.contains("oauth_token="), "no access token yet: {req}");

    let answer = web.oauth_access_token("a1b2c3", "v9").await.unwrap();
    assert_eq!(answer["oauth_token_secret"], "c2VjcmV0");
    let req = requests.recv().await.unwrap();
    assert!(req.starts_with("POST /v1/api/oauth/access_token "), "{req}");
    assert!(
        req.contains("oauth_token=\"a1b2c3\"") && req.contains("oauth_verifier=\"v9\""),
        "{req}"
    );
}

#[test]
fn http_methods_parse_in_any_case_and_unknown_ones_are_refused() {
    use ibkit_web::client::{Method, parse_method};
    assert_eq!(parse_method("patch").unwrap(), Method::PATCH);
    assert_eq!(parse_method("Get").unwrap(), Method::GET);
    let error = parse_method("FOO").unwrap_err().to_string();
    assert!(
        error.contains("\"FOO\"") && error.contains("PATCH"),
        "{error}"
    );
}
