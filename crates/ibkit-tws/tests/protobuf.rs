//! Protobuf protocol (server versions 201 to 226): request encoding, message
//! decoding, and a client session against the fake server at version 226.

use ibkit_tws::consts::UNSET_DOUBLE;
use ibkit_tws::decode;
use ibkit_tws::encode_pb;
use ibkit_tws::pb::{PbMsg, PbWriter, to_json};
use ibkit_tws::proto;
use ibkit_tws::testing::{FakeTws, FakeTwsConfig, protobuf, unhex};
use ibkit_tws::*;
use std::sync::Arc;
use std::time::Duration;

const V: i32 = 226;

/// Message id and protobuf payload of a frame.
fn split(frame: &[u8]) -> (i32, &[u8]) {
    let len = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
    assert_eq!(len + 4, frame.len(), "length prefix");
    (
        i32::from_be_bytes(frame[4..8].try_into().unwrap()),
        &frame[8..],
    )
}

fn aapl() -> Contract {
    Contract {
        con_id: 265598,
        multiplier: "100".into(),
        ..Contract::stock("AAPL", "SMART", "USD")
    }
}

#[test]
fn requests_switch_to_protobuf_at_their_server_version() {
    // reqMktData is protobuf from 206; below it the text encoder is used.
    assert!(encode_pb::req_mkt_data(205, 1, &aapl(), "", false, false, &[]).is_none());
    let frame = encode_pb::req_mkt_data(
        206,
        7,
        &aapl(),
        "233",
        true,
        false,
        &[TagValue {
            tag: "k".into(),
            value: "v".into(),
        }],
    )
    .unwrap();
    let (id, body) = split(&frame);
    assert_eq!(id, 201);
    let json = to_json("MarketDataRequest", &PbMsg::parse(body).unwrap());
    assert_eq!(
        json,
        serde_json::json!({
            "reqId": 7,
            "contract": {"conId": 265598, "symbol": "AAPL", "secType": "STK", "strike": 0.0, "multiplier": 100.0, "exchange": "SMART", "currency": "USD"},
            "genericTickList": "233",
            "snapshot": true,
            "marketDataOptions": {"k": "v"}
        })
    );
    // reqExecutions is protobuf from 201 already.
    assert!(encode_pb::req_executions(201, 1, &ExecutionFilter::default()).is_some());
    assert!(encode_pb::start_api(212, 1, "").is_none());
    assert_eq!(split(&encode_pb::start_api(213, 1, "").unwrap()).0, 271);
}

#[test]
fn place_order_carries_order_attached_orders_and_conditions() {
    let order = Order {
        action: "BUY".into(),
        total_quantity: 10.0,
        order_type: "LMT".into(),
        lmt_price: 150.5,
        tif: "GTC".into(),
        post_only: true,
        seek_price_improvement: Some(false),
        sl_order_id: 9,
        sl_order_type: "STP".into(),
        hedge_type: "B".into(),
        hedge_max_size: 300,
        conditions_include_overnight: true,
        conditions: vec![OrderCondition::Price {
            conjunction: "a".into(),
            is_more: true,
            price: 160.0,
            con_id: 265598,
            exch: "SMART".into(),
            trigger_method: 0,
        }],
        ..Default::default()
    };
    let frame = encode_pb::place_order(V, 42, &aapl(), &order).unwrap();
    let (id, body) = split(&frame);
    assert_eq!(id, 203);
    let json = to_json("PlaceOrderRequest", &PbMsg::parse(body).unwrap());
    assert_eq!(json["orderId"], 42);
    let o = &json["order"];
    assert_eq!(
        (
            o["action"].as_str(),
            o["totalQuantity"].as_str(),
            o["lmtPrice"].as_f64(),
            o["tif"].as_str()
        ),
        (Some("BUY"), Some("10"), Some(150.5), Some("GTC"))
    );
    assert_eq!(
        (o["postOnly"].as_bool(), o["seekPriceImprovement"].as_i64()),
        (Some(true), Some(0))
    );
    assert_eq!(
        (
            o["hedgeMaxSize"].as_i64(),
            o["conditionsIncludeOvernight"].as_bool()
        ),
        (Some(300), Some(true))
    );
    assert_eq!(o.get("auxPrice"), None, "unset values are not sent");
    assert_eq!(
        o["conditions"][0],
        serde_json::json!({"type": 1, "isConjunctionConnection": true, "isMore": true, "conId": 265598, "exchange": "SMART", "price": 160.0, "triggerMethod": 0})
    );
    assert_eq!(
        json["attachedOrders"],
        serde_json::json!({"slOrderId": 9, "slOrderType": "STP"})
    );
}

fn pb_frame(msg_id: i32, build: impl FnOnce(&mut PbWriter)) -> Vec<u8> {
    let mut w = PbWriter::default();
    build(&mut w);
    let mut payload = (msg_id + 200).to_be_bytes().to_vec();
    payload.extend(w.buf);
    payload
}

#[test]
fn protobuf_messages_decode_into_the_same_values_as_text() {
    use proto::*;
    let tick = pb_frame(1, |w| {
        w.i32(tick_price::REQ_ID, 5);
        w.i32(tick_price::TICK_TYPE, 1);
        w.f64(tick_price::PRICE, 150.25);
        w.str(tick_price::SIZE, "300");
        w.i32(tick_price::ATTR_MASK, 3);
    });
    assert_eq!(
        decode(V, &tick).unwrap(),
        Incoming::TickPrice {
            req_id: 5,
            tick_type: 1,
            price: 150.25,
            size: 300.0,
            attrib: TickAttrib {
                can_auto_execute: true,
                past_limit: true,
                pre_open: false
            },
        }
    );
    let status = pb_frame(3, |w| {
        w.i32(order_status::ORDER_ID, 7);
        w.str(order_status::STATUS, "Filled");
        w.str(order_status::FILLED, "100");
        w.str(order_status::REMAINING, "0");
        w.f64(order_status::AVG_FILL_PRICE, 150.0);
        w.i64(order_status::PERM_ID, 1 << 40);
    });
    let Incoming::OrderStatus(s) = decode(V, &status).unwrap() else {
        panic!("order status")
    };
    assert_eq!(
        (
            s.order_id,
            s.status.as_str(),
            s.filled,
            s.perm_id,
            s.parent_id,
            s.last_fill_price
        ),
        (
            7,
            "Filled",
            100.0,
            1 << 40,
            consts::UNSET_INTEGER,
            UNSET_DOUBLE
        )
    );

    let open = pb_frame(5, |w| {
        w.i32(open_order::ORDER_ID, 7);
        w.msg(open_order::CONTRACT, |w| {
            w.i32(contract::CON_ID, 265598);
            w.str(contract::SYMBOL, "AAPL");
            w.f64(contract::MULTIPLIER, 100.0);
            w.msg(contract::COMBO_LEGS, |w| {
                w.i32(combo_leg::CON_ID, 1);
                w.f64(combo_leg::PER_LEG_PRICE, 2.5);
            });
        });
        w.msg(open_order::ORDER, |w| {
            w.str(order::ACTION, "SELL");
            w.str(order::TOTAL_QUANTITY, "5");
            w.i32(order::USE_PRICE_MGMT_ALGO, 1);
            w.str(order::ALGO_STRATEGY, "Adaptive");
            w.str(order::HEDGE_TYPE, "B");
            w.i32(order::HEDGE_MAX_SIZE, 300);
            w.bool(order::CONDITIONS_INCLUDE_OVERNIGHT, true);
            w.map(
                order::ALGO_PARAMS,
                &[TagValue {
                    tag: "adaptivePriority".into(),
                    value: "Normal".into(),
                }],
            );
        });
        w.msg(open_order::ORDER_STATE, |w| {
            w.str(order_state::STATUS, "Submitted");
            w.f64(order_state::INIT_MARGIN_CHANGE, 2500.5);
            w.f64(order_state::COMMISSION_AND_FEES, 1.25);
        });
    });
    let Incoming::OpenOrder {
        order_id,
        contract,
        order,
        state,
    } = decode(V, &open).unwrap()
    else {
        panic!("open order")
    };
    assert_eq!(
        (
            order_id,
            order.order_id,
            contract.multiplier.as_str(),
            contract.combo_legs[0].con_id
        ),
        (7, 7, "100", 1)
    );
    assert_eq!(
        (
            order.action.as_str(),
            order.total_quantity,
            order.use_price_mgmt_algo,
            order.order_combo_legs[0].price
        ),
        ("SELL", 5.0, Some(true), 2.5)
    );
    assert_eq!(order.algo_params[0].value, "Normal");
    assert_eq!(
        (order.hedge_max_size, order.conditions_include_overnight),
        (300, true)
    );
    assert_eq!(
        (
            state.status.as_str(),
            state.init_margin_change.as_str(),
            state.commission
        ),
        ("Submitted", "2500.5", 1.25)
    );

    let details = pb_frame(10, |w| {
        w.i32(contract_data::REQ_ID, 3);
        w.msg(contract_data::CONTRACT, |w| {
            w.str(contract::SYMBOL, "AAPL");
            w.str(
                contract::LAST_TRADE_DATE_OR_CONTRACT_MONTH,
                "20261218 16:00",
            );
        });
        w.msg(contract_data::CONTRACT_DETAILS, |w| {
            w.str(contract_details::MIN_TICK, "0.01");
            w.str(contract_details::LONG_NAME, "APPLE INC");
            w.str(contract_details::EVENT_CONTRACT1, "EC1");
            w.str(contract_details::SETTLEMENT_METHOD, "Cash");
            w.map(
                contract_details::SEC_ID_LIST,
                &[TagValue {
                    tag: "ISIN".into(),
                    value: "US0378331005".into(),
                }],
            );
        });
    });
    let Incoming::ContractDetails { req_id, details } = decode(V, &details).unwrap() else {
        panic!("details")
    };
    assert_eq!(
        (
            req_id,
            details.min_tick,
            details.long_name.as_str(),
            details.event_contract1.as_str()
        ),
        (3, 0.01, "APPLE INC", "EC1")
    );
    assert_eq!(
        (
            details.contract.last_trade_date_or_contract_month.as_str(),
            details.last_trade_time.as_str()
        ),
        ("20261218", "16:00")
    );
    assert_eq!(details.sec_id_list[0].value, "US0378331005");
    assert_eq!(details.settlement_method, "Cash");

    let greeks = pb_frame(21, |w| {
        w.i32(tick_option_computation::REQ_ID, 1);
        w.i32(tick_option_computation::TICK_TYPE, 13);
        w.f64(tick_option_computation::IMPLIED_VOL, -1.0);
        w.f64(tick_option_computation::DELTA, 0.5);
        w.f64(tick_option_computation::GAMMA, -2.0);
    });
    let Incoming::TickOptionComputation { computation: c, .. } = decode(V, &greeks).unwrap() else {
        panic!("greeks")
    };
    assert_eq!(
        (c.implied_vol, c.delta, c.gamma, c.vega),
        (None, Some(0.5), None, None)
    );

    let bars = pb_frame(17, |w| {
        w.i32(historical_data::REQ_ID, 4);
        for close in [1.0, 2.0] {
            w.msg(historical_data::HISTORICAL_DATA_BARS, |w| {
                w.str(historical_data_bar::DATE, "20260102");
                w.f64(historical_data_bar::CLOSE, close);
                w.str(historical_data_bar::WAP, "1.5");
            });
        }
    });
    let Incoming::HistoricalData { bars, end, .. } = decode(V, &bars).unwrap() else {
        panic!("bars")
    };
    assert_eq!(
        (bars.len(), bars[1].close, bars[0].average, end),
        (2, 2.0, 1.5, None)
    );

    // A text message keeps working; its id is a raw integer from 201.
    let mut text = 49i32.to_be_bytes().to_vec();
    text.extend(b"1\x001767225600\x00");
    assert_eq!(
        decode(V, &text).unwrap(),
        Incoming::CurrentTime { time: 1767225600 }
    );
    assert!(matches!(
        decode(V, &pb_frame(5, |w| w.i32(1, 1))),
        Err(wire::DecodeError::Protobuf(_))
    ));
}

/// Responder of a server at version 226 that speaks protobuf.
fn responder(req: &[String]) -> Vec<Vec<String>> {
    use proto::*;
    let id: i32 = req[0].parse().unwrap();
    if req.get(1).map(String::as_str) != Some("protobuf") {
        return vec![];
    }
    let body = unhex(&req[2]);
    let m = PbMsg::parse(&body).unwrap();
    match id {
        9 => {
            let r = m.int(contract_data_request::REQ_ID);
            let symbol = m
                .msg(contract_data_request::CONTRACT)
                .unwrap()
                .string(contract::SYMBOL);
            vec![
                protobuf(10, |w| {
                    w.i32(contract_data::REQ_ID, r);
                    w.msg(contract_data::CONTRACT, |w| {
                        w.i32(contract::CON_ID, 265598);
                        w.str(contract::SYMBOL, &symbol);
                        w.str(contract::SEC_TYPE, "STK");
                        w.str(contract::EXCHANGE, "SMART");
                        w.str(contract::CURRENCY, "USD");
                    });
                    w.msg(contract_data::CONTRACT_DETAILS, |w| {
                        w.str(contract_details::MIN_TICK, "0.01")
                    });
                }),
                protobuf(52, |w| w.i32(contract_data_end::REQ_ID, r)),
            ]
        }
        108 => vec![protobuf(110, |w| {
            w.i32(config_response::REQ_ID, m.int(config_request::REQ_ID));
            w.msg(config_response::LOCK_AND_EXIT, |w| {
                w.str(lock_and_exit_config::AUTO_LOGOFF_TIME, "11:45 PM")
            });
        })],
        49 => vec![protobuf(49, |w| {
            w.i64(current_time::CURRENT_TIME, 1767225600)
        })],
        _ => vec![],
    }
}

#[tokio::test]
async fn client_session_at_version_226() {
    let mut tws = FakeTws::with(
        FakeTwsConfig {
            server_version: V,
            next_valid_id: 1,
            accounts: "DU1".into(),
        },
        Arc::new(responder),
    )
    .await;
    let (client, mut events) = Client::connect(ConnectOptions::new("127.0.0.1", tws.port(), 3))
        .await
        .unwrap();
    assert_eq!(client.server_version(), V);
    let start = tws.next_request().await;
    assert_eq!((start[0].as_str(), start[1].as_str()), ("71", "protobuf"));
    let start = PbMsg::parse(&unhex(&start[2]))
        .unwrap()
        .int(proto::start_api_request::CLIENT_ID);
    assert_eq!(start, 3);

    client
        .req_contract_details(5, &Contract::stock("AAPL", "SMART", "USD"))
        .unwrap();
    client.req_config(6).unwrap();
    client.cancel_contract_data(5).unwrap();
    client.req_current_time().unwrap();
    let mut got = Vec::new();
    let wait = async {
        while got.len() < 4 {
            for m in events.recv_batch().await.unwrap() {
                if !matches!(
                    m,
                    Incoming::NextValidId { .. } | Incoming::ManagedAccounts { .. }
                ) {
                    got.push(m);
                }
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), wait)
        .await
        .expect("answers within 5 s");
    let Incoming::ContractDetails { req_id: 5, details } = &got[0] else {
        panic!("{:?}", got[0])
    };
    assert_eq!((details.contract.con_id, details.min_tick), (265598, 0.01));
    assert_eq!(got[1], Incoming::ContractDetailsEnd { req_id: 5 });
    let Incoming::ConfigResponse { req_id: 6, config } = &got[2] else {
        panic!("{:?}", got[2])
    };
    assert_eq!(config["lockAndExit"]["autoLogoffTime"], "11:45 PM");
    assert_eq!(got[3], Incoming::CurrentTime { time: 1767225600 });
    let cancel = tws.next_request_with_id(106).await;
    assert_eq!(cancel[1], "protobuf");

    // Text-only requests carry a raw integer id at this version.
    client.verify_and_auth_request("app", "1", "key").unwrap();
    assert_eq!(
        tws.next_request_with_id(72).await,
        ["72", "1", "app", "1", "key"]
    );
}

#[tokio::test]
async fn protobuf_only_requests_are_refused_by_older_servers() {
    let tws = FakeTws::with(
        FakeTwsConfig {
            server_version: 200,
            ..Default::default()
        },
        Arc::new(|_| vec![]),
    )
    .await;
    let (client, _events) = Client::connect(ConnectOptions::new("127.0.0.1", tws.port(), 1))
        .await
        .unwrap();
    let err = client.req_config(1).unwrap_err().to_string();
    assert!(err.contains("reqConfig needs server version 219"), "{err}");
}
