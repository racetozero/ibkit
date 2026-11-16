//! Wire layout tests. Expected field lists follow IB's official client and,
//! for server version 178, ib_async's independent decoder.

use ibkit_tws::consts::{UNSET_DOUBLE, UNSET_INTEGER};
use ibkit_tws::encode;
use ibkit_tws::wire::FieldWriter;
use ibkit_tws::*;

fn fields(w: &mut FieldWriter) -> Vec<String> {
    let frame = w.finish().to_vec();
    let len = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
    assert_eq!(
        len,
        frame.len() - 4,
        "length prefix must equal payload size"
    );
    let payload = &frame[4..];
    assert_eq!(
        *payload.last().unwrap(),
        0,
        "payload must end with a NUL terminator"
    );
    payload[..payload.len() - 1]
        .split(|b| *b == 0)
        .map(|f| String::from_utf8(f.to_vec()).unwrap())
        .collect()
}

fn payload(fields: &[&str]) -> Vec<u8> {
    let mut p = Vec::new();
    for f in fields {
        p.extend_from_slice(f.as_bytes());
        p.push(0);
    }
    p
}

fn aapl() -> Contract {
    Contract {
        symbol: "AAPL".into(),
        sec_type: "STK".into(),
        exchange: "SMART".into(),
        currency: "USD".into(),
        ..Default::default()
    }
}

fn limit(qty: f64, price: f64) -> Order {
    Order {
        action: "BUY".into(),
        total_quantity: qty,
        order_type: "LMT".into(),
        lmt_price: price,
        ..Default::default()
    }
}

const MAX: &str = "1.7976931348623157e308";

fn place_order_expected(v: i32) -> Vec<&'static str> {
    let mut e = vec![
        "3", "7", // msg id, order id
        "0", "AAPL", "STK", "", "0", "", "", "SMART", "", "USD", "", "", "",
        "", // contract + secIdType/secId
        "BUY", "100", "LMT", "150.5", "", // action qty type lmt aux
        "", "", "", "O", "0", "", "1", "0", "0", "0", "0", "0", "0", "0", // tif .. hidden
        "", "0", "", "", "", "", "", // sharesAllocation .. faPercentage
    ];
    if v < 177 {
        e.push(""); // faProfile
    }
    e.extend([
        "", "0", "", "-1", "0", "", "", "0", "", "", "0", "0",
        "", // modelCode .. nbboPriceCap
        "0", "", "", "", "", "", "0", "", "", "",
        "", // auctionStrategy .. deltaNeutralAuxPrice
        "0", "", "", "", "", "", "", // continuousUpdate .. scalePriceIncrement
        "", "", "", "", "0", "", "", "0", "0", "", "", "0", "", "0", "0", "0",
        "0", // scaleTable .. conditions count
        "", MAX, MAX, MAX, MAX, MAX, "0", "", "", "", MAX, // adjusted .. cashQty
        "", "", "", "", "0", "0", "0", "", // mifid .. usePriceMgmtAlgo
    ]);
    if v >= 158 {
        e.push("2147483647"); // duration
    }
    if v >= 160 {
        e.push("2147483647"); // postToAts
    }
    if v >= 162 {
        e.push("0"); // autoCancelParent
    }
    if v >= 166 {
        e.push(""); // advancedErrorOverride
    }
    if v >= 169 {
        e.push(""); // manualOrderTime
    }
    if v >= 183 {
        e.push(""); // customerAccount
    }
    if v >= 184 {
        e.push("0"); // professionalCustomer
    }
    if (187..190).contains(&v) {
        e.extend(["", "2147483647"]);
    }
    if v >= 189 {
        e.push("0"); // includeOvernight
    }
    if v >= 192 {
        e.push("2147483647"); // manualOrderIndicator
    }
    if v >= 199 {
        e.push("0"); // imbalanceOnly
    }
    e
}

#[test]
fn place_order_layout_matches_official_client_at_each_version() {
    for v in [157, 165, 170, 178, 188, 192, 200] {
        let mut w = FieldWriter::new();
        encode::place_order(&mut w, v, 7, &aapl(), &limit(100.0, 150.5)).unwrap();
        assert_eq!(
            fields(&mut w),
            place_order_expected(v),
            "server version {v}"
        );
    }
}

#[test]
fn place_order_rejects_fields_the_server_cannot_accept() {
    let mut w = FieldWriter::new();
    let o = Order {
        customer_account: "X".into(),
        ..limit(1.0, 1.0)
    };
    let err = encode::place_order(&mut w, 178, 1, &aapl(), &o).unwrap_err();
    assert_eq!(err.min_version, 183);
    assert!(
        err.to_string()
            .contains("Order.customerAccount needs server version 183")
    );
}

#[test]
fn place_order_encodes_conditions_algo_and_peg_mid() {
    let mut o = limit(10.0, 1.25);
    o.order_type = "PEG MID".into();
    o.mid_offset_at_whole = 0.01;
    o.algo_strategy = "Adaptive".into();
    o.algo_params = vec![TagValue::new("adaptivePriority", "Normal")];
    o.conditions = vec![
        OrderCondition::Price {
            conjunction: "a".into(),
            is_more: true,
            price: 100.0,
            con_id: 265598,
            exch: "SMART".into(),
            trigger_method: 0,
        },
        OrderCondition::Time {
            conjunction: "o".into(),
            is_more: false,
            time: "20260101 10:00:00".into(),
        },
    ];
    let mut w = FieldWriter::new();
    encode::place_order(&mut w, 178, 1, &aapl(), &o).unwrap();
    let f = fields(&mut w);
    let joined = f.join("|");
    assert!(
        joined.contains("|Adaptive|1|adaptivePriority|Normal|"),
        "{joined}"
    );
    assert!(
        joined.contains("|2|1|a|1|100|265598|SMART|0|3|o|0|20260101 10:00:00|0|0|"),
        "{joined}"
    );
    assert_eq!(
        &f[f.len() - 2..],
        ["0.01", ""],
        "PEG MID sends midOffsetAtWhole and midOffsetAtHalf last"
    );
}

#[test]
fn req_mkt_data_layout() {
    let mut w = FieldWriter::new();
    let opts = [TagValue::new("a", "b")];
    encode::req_mkt_data(&mut w, 5, &aapl(), "233,236", false, false, &opts);
    assert_eq!(
        fields(&mut w),
        [
            "1", "11", "5", "0", "AAPL", "STK", "", "0", "", "", "SMART", "", "USD", "", "", "0",
            "233,236", "0", "0", "a=b;"
        ]
    );
}

#[test]
fn req_historical_data_layout_with_bag_legs() {
    let mut c = aapl();
    c.sec_type = "BAG".into();
    c.combo_legs = vec![ComboLeg {
        con_id: 1,
        ratio: 2,
        action: "BUY".into(),
        exchange: "SMART".into(),
        ..Default::default()
    }];
    let mut w = FieldWriter::new();
    encode::req_historical_data(
        &mut w,
        3,
        &c,
        "",
        "1 D",
        "1 min",
        "TRADES",
        true,
        1,
        false,
        &[],
    );
    assert_eq!(
        fields(&mut w),
        [
            "20", "3", "0", "AAPL", "BAG", "", "0", "", "", "SMART", "", "USD", "", "", "0", "",
            "1 min", "1 D", "1", "TRADES", "1", "1", "1", "2", "BUY", "SMART", "0", ""
        ]
    );
}

#[test]
fn cancel_order_layout_by_version() {
    let oc = OrderCancel::default();
    let mut w = FieldWriter::new();
    encode::cancel_order(&mut w, 178, 9, &oc);
    assert_eq!(fields(&mut w), ["4", "1", "9", ""]);
    encode::cancel_order(&mut w, 188, 9, &oc);
    assert_eq!(fields(&mut w), ["4", "1", "9", "", "", "", "2147483647"]);
    encode::cancel_order(&mut w, 200, 9, &oc);
    assert_eq!(fields(&mut w), ["4", "9", "", "", "2147483647"]);
}

#[test]
fn req_executions_layout_by_version() {
    let f = ExecutionFilter {
        client_id: 3,
        ..Default::default()
    };
    let mut w = FieldWriter::new();
    encode::req_executions(&mut w, 178, 11, &f).unwrap();
    assert_eq!(
        fields(&mut w),
        ["7", "3", "11", "3", "", "", "", "", "", ""]
    );
    let f = ExecutionFilter {
        last_n_days: 2,
        specific_dates: vec![20260101],
        ..f
    };
    encode::req_executions(&mut w, 200, 11, &f).unwrap();
    assert_eq!(
        fields(&mut w),
        [
            "7", "3", "11", "3", "", "", "", "", "", "", "2", "1", "20260101"
        ]
    );
    assert!(encode::req_executions(&mut w, 199, 11, &f).is_err());
}

#[test]
fn handshake_bytes() {
    assert_eq!(encode::handshake(157, 200, ""), b"API\0\0\0\0\x09v157..200");
    assert_eq!(
        encode::handshake(100, 178, "+PACEAPI"),
        b"API\0\0\0\0\x12v100..178 +PACEAPI"
    );
}

#[test]
fn decode_tick_price_and_size() {
    let m = decode(178, &payload(&["1", "6", "4", "1", "185.25", "300", "3"])).unwrap();
    assert_eq!(
        m,
        Incoming::TickPrice {
            req_id: 4,
            tick_type: 1,
            price: 185.25,
            size: 300.0,
            attrib: TickAttrib {
                can_auto_execute: true,
                past_limit: true,
                pre_open: false
            }
        }
    );
    let m = decode(178, &payload(&["2", "6", "4", "8", "12345"])).unwrap();
    assert_eq!(
        m,
        Incoming::TickSize {
            req_id: 4,
            tick_type: 8,
            size: 12345.0
        }
    );
}

#[test]
fn decode_error_by_version() {
    let m = decode(
        178,
        &payload(&["4", "2", "7", "200", "No security definition", ""]),
    )
    .unwrap();
    let Incoming::Error(e) = m else { panic!() };
    assert_eq!(
        (e.req_id, e.code, e.message.as_str(), e.error_time),
        (7, 200, "No security definition", 0)
    );
    let m = decode(
        200,
        &payload(&["4", "7", "200", r"Café", "{}", "1767225600000"]),
    )
    .unwrap();
    let Incoming::Error(e) = m else { panic!() };
    assert_eq!(
        (
            e.message.as_str(),
            e.advanced_order_reject_json.as_str(),
            e.error_time
        ),
        ("Café", "{}", 1767225600000)
    );
}

#[test]
fn decode_historical_data_by_version() {
    let bar = [
        "20260102 09:30:00",
        "1",
        "2",
        "0.5",
        "1.5",
        "100",
        "1.25",
        "7",
    ];
    let mut f = vec!["17", "9", "20260101", "20260102", "1"];
    f.extend(bar);
    let Incoming::HistoricalData { req_id, bars, end } = decode(178, &payload(&f)).unwrap() else {
        panic!()
    };
    assert_eq!(
        (req_id, bars.len(), end),
        (9, 1, Some(("20260101".into(), "20260102".into())))
    );
    assert_eq!(bars[0].close, 1.5);
    assert_eq!(bars[0].bar_count, 7);
    let mut f = vec!["17", "9", "1"];
    f.extend(bar);
    let Incoming::HistoricalData { end, .. } = decode(200, &payload(&f)).unwrap() else {
        panic!()
    };
    assert_eq!(end, None);
    assert_eq!(
        decode(200, &payload(&["108", "9", "a", "b"])).unwrap(),
        Incoming::HistoricalDataEnd {
            req_id: 9,
            start: "a".into(),
            end: "b".into()
        }
    );
}

#[test]
fn decode_exec_details_by_version() {
    let base = [
        "11",
        "5",
        "12",
        "265598",
        "AAPL",
        "STK",
        "",
        "0",
        "",
        "",
        "ISLAND",
        "USD",
        "AAPL",
        "NMS",
        "0001f4e8.6574.01.01",
        "20260102 10:00:00 US/Eastern",
        "DU123456",
        "ISLAND",
        "BOT",
        "100",
        "150.5",
        "987654321012",
        "1",
        "0",
        "100",
        "150.5",
        "ref",
        "",
        "",
        "",
        "1",
    ];
    let mut f178: Vec<&str> = base.to_vec();
    f178.push("1");
    let Incoming::ExecDetails {
        req_id,
        contract,
        execution,
    } = decode(178, &payload(&f178)).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        (
            req_id,
            contract.con_id,
            execution.order_id,
            execution.perm_id
        ),
        (5, 265598, 12, 987654321012)
    );
    assert_eq!(
        (execution.shares, execution.price, execution.last_liquidity),
        (100.0, 150.5, 1)
    );
    assert!(execution.pending_price_revision);
    let mut f200 = f178.clone();
    f200.push("ME");
    let Incoming::ExecDetails { execution, .. } = decode(200, &payload(&f200)).unwrap() else {
        panic!()
    };
    assert_eq!(execution.submitter, "ME");
}

#[test]
fn decode_contract_details_178() {
    let f = [
        "10",
        "3",
        "AAPL",
        "STK",
        "",
        "0",
        "",
        "SMART",
        "USD",
        "AAPL",
        "NMS",
        "NMS",
        "265598",
        "0.01",
        "",
        "ACTIVETIM,LMT",
        "SMART,ISLAND",
        "1",
        "0",
        r"APPLE INC é",
        "NASDAQ",
        "",
        "Technology",
        "Computers",
        "Computers",
        "US/Eastern",
        "20260102:0400-20260102:2000",
        "20260102:0930-20260102:1600",
        "",
        "",
        "1",
        "ISIN",
        "US0378331005",
        "1",
        "AAPL",
        "",
        "26,26",
        "",
        "COMMON",
        "0.0001",
        "0.0001",
        "100",
    ];
    let Incoming::ContractDetails { req_id, details } = decode(178, &payload(&f)).unwrap() else {
        panic!()
    };
    assert_eq!(req_id, 3);
    assert_eq!(details.contract.con_id, 265598);
    assert_eq!(details.long_name, "APPLE INC é");
    assert_eq!(details.contract.primary_exchange, "NASDAQ");
    assert_eq!(
        details.sec_id_list,
        vec![TagValue::new("ISIN", "US0378331005")]
    );
    assert_eq!(details.stock_type, "COMMON");
    assert_eq!(details.suggested_size_increment, 100.0);
}

#[test]
fn decode_open_order_178_in_ib_async_field_order() {
    #[rustfmt::skip]
    let f = [
        "5", "42",
        "265598", "AAPL", "STK", "", "0", "?", "", "SMART", "USD", "AAPL", "NMS",
        "BUY", "100", "LMT", "150.5", "0.0", "DAY", "", "DU123456", "", "0", "myref", "1", "1234567890", "0", "0", "0", "", "",
        "", "", "", // faGroup faMethod faPercentage
        "", "", "", "", "", "0", "", "-1", // modelCode goodTillDate rule80A percentOffset settlingFirm shortSaleSlot designatedLocation exemptCode
        "0", "", "", "", "", "", "0", "0", "0", "0", "", "3", "0", "0", "", "0", "0", // auctionStrategy .. triggerMethod
        "", "0", "", "", // volatility volatilityType dnOrderType dnAuxPrice
        "0", "0", "", "", "", "", // continuousUpdate referencePriceType trailStopPrice trailingPercent basisPoints basisPointsType
        "", "0", "0", "0", // comboLegsDescrip numLegs numOrderLegs numParams
        "", "", "", // scaleInitLevelSize scaleSubsLevelSize increment
        "", "0", "", "", "0", "0", "", // hedgeType optOutSmartRouting clearingAccount clearingIntent notHeld dncPresent algoStrategy
        "0", "0", "Submitted", "", "", "", "", "", "", "", "", "", "", "", "", "", "", // solicited whatIf status margins commissions
        "0", "0", "0", // randomizeSize randomizePrice numConditions
        "", MAX, MAX, MAX, MAX, MAX, MAX, "0", // adjusted
        "", "", "", MAX, "0", "0", "0", "0", // softDollarTier cashQty ... usePriceMgmtAlgo
        "", "", "0", // duration postToAts autoCancelParent
        "", "", "", "", "", // minTradeQty minCompeteSize competeAgainstBestOffset midOffsetAtWhole midOffsetAtHalf
    ];
    let Incoming::OpenOrder {
        order_id,
        contract,
        order,
        state,
    } = decode(178, &payload(&f)).unwrap()
    else {
        panic!()
    };
    assert_eq!(order_id, 42);
    assert_eq!(contract.symbol, "AAPL");
    assert_eq!(contract.right, "?");
    assert_eq!(
        (order.action.as_str(), order.total_quantity, order.lmt_price),
        ("BUY", 100.0, 150.5)
    );
    assert_eq!(
        (order.client_id, order.perm_id, order.order_ref.as_str()),
        (1, 1234567890, "myref")
    );
    assert_eq!(order.oca_type, 3);
    assert_eq!(order.min_qty, UNSET_INTEGER);
    assert_eq!(order.trail_stop_price, UNSET_DOUBLE);
    assert_eq!(state.status, "Submitted");
    assert_eq!(order.duration, UNSET_INTEGER);
}

#[test]
fn decode_option_computation_marks_not_computed_values() {
    let f = [
        "21", "4", "13", "1", "-1", "0.5", "2.5", "-1", "-2", "0.1", "-0.05", "150",
    ];
    let Incoming::TickOptionComputation {
        req_id,
        tick_type,
        computation,
    } = decode(178, &payload(&f)).unwrap()
    else {
        panic!()
    };
    assert_eq!((req_id, tick_type), (4, 13));
    assert_eq!(computation.implied_vol, None);
    assert_eq!(computation.delta, Some(0.5));
    assert_eq!(computation.pv_dividend, None);
    assert_eq!(computation.gamma, None);
    assert_eq!(computation.und_price, Some(150.0));
}

#[test]
fn decode_unknown_message_keeps_fields() {
    let m = decode(178, &payload(&["999", "a", "b"])).unwrap();
    assert_eq!(
        m,
        Incoming::Unknown {
            msg_id: 999,
            fields: vec!["999".into(), "a".into(), "b".into()]
        }
    );
}

#[test]
fn decode_reports_missing_fields() {
    let err = decode(178, &payload(&["9", "1"])).unwrap_err();
    assert_eq!(
        err.to_string(),
        "message ended early: expected another field after field 2"
    );
}

#[test]
fn every_request_type_encodes_a_valid_frame() {
    use ibkit_tws::message::outgoing as out;
    let c = aapl();
    let o = limit(1.0, 1.0);
    let oc = OrderCancel::default();
    let f = ExecutionFilter::default();
    let s = ScannerSubscription::default();
    let wsh = WshEventData::default();
    let tv = [TagValue::new("a", "b")];
    type Enc<'a> = Box<dyn Fn(&mut FieldWriter) + 'a>;
    let cases: Vec<(i32, Enc<'_>)> = vec![
        (
            out::REQ_MKT_DATA,
            Box::new(|w| encode::req_mkt_data(w, 1, &c, "", false, false, &tv)),
        ),
        (
            out::CANCEL_MKT_DATA,
            Box::new(|w| encode::cancel_mkt_data(w, 1)),
        ),
        (
            out::PLACE_ORDER,
            Box::new(|w| encode::place_order(w, 200, 1, &c, &o).unwrap()),
        ),
        (
            out::CANCEL_ORDER,
            Box::new(|w| encode::cancel_order(w, 200, 1, &oc)),
        ),
        (out::REQ_OPEN_ORDERS, Box::new(encode::req_open_orders)),
        (
            out::REQ_ACCT_DATA,
            Box::new(|w| encode::req_account_updates(w, true, "DU1")),
        ),
        (
            out::REQ_EXECUTIONS,
            Box::new(|w| encode::req_executions(w, 200, 1, &f).unwrap()),
        ),
        (out::REQ_IDS, Box::new(|w| encode::req_ids(w, 1))),
        (
            out::REQ_CONTRACT_DATA,
            Box::new(|w| encode::req_contract_details(w, 200, 1, &c)),
        ),
        (
            out::REQ_MKT_DEPTH,
            Box::new(|w| encode::req_mkt_depth(w, 1, &c, 5, true, &tv)),
        ),
        (
            out::CANCEL_MKT_DEPTH,
            Box::new(|w| encode::cancel_mkt_depth(w, 1, true)),
        ),
        (
            out::REQ_NEWS_BULLETINS,
            Box::new(|w| encode::req_news_bulletins(w, true)),
        ),
        (
            out::CANCEL_NEWS_BULLETINS,
            Box::new(encode::cancel_news_bulletins),
        ),
        (
            out::SET_SERVER_LOGLEVEL,
            Box::new(|w| encode::set_server_log_level(w, 5)),
        ),
        (
            out::REQ_AUTO_OPEN_ORDERS,
            Box::new(|w| encode::req_auto_open_orders(w, true)),
        ),
        (
            out::REQ_ALL_OPEN_ORDERS,
            Box::new(encode::req_all_open_orders),
        ),
        (out::REQ_MANAGED_ACCTS, Box::new(encode::req_managed_accts)),
        (out::REQ_FA, Box::new(|w| encode::request_fa(w, 1))),
        (
            out::REPLACE_FA,
            Box::new(|w| encode::replace_fa(w, 1, 1, "<xml/>")),
        ),
        (
            out::REQ_HISTORICAL_DATA,
            Box::new(|w| {
                encode::req_historical_data(
                    w, 1, &c, "", "1 D", "1 min", "TRADES", true, 1, false, &tv,
                )
            }),
        ),
        (
            out::EXERCISE_OPTIONS,
            Box::new(|w| {
                encode::exercise_options(w, 200, 1, &c, 1, 1, "DU1", 0, "", "", false).unwrap()
            }),
        ),
        (
            out::REQ_SCANNER_SUBSCRIPTION,
            Box::new(|w| encode::req_scanner_subscription(w, 1, &s, &tv, &tv)),
        ),
        (
            out::CANCEL_SCANNER_SUBSCRIPTION,
            Box::new(|w| encode::cancel_scanner_subscription(w, 1)),
        ),
        (
            out::REQ_SCANNER_PARAMETERS,
            Box::new(encode::req_scanner_parameters),
        ),
        (
            out::CANCEL_HISTORICAL_DATA,
            Box::new(|w| encode::cancel_historical_data(w, 1)),
        ),
        (out::REQ_CURRENT_TIME, Box::new(encode::req_current_time)),
        (
            out::REQ_REAL_TIME_BARS,
            Box::new(|w| encode::req_real_time_bars(w, 1, &c, 5, "TRADES", true, &tv)),
        ),
        (
            out::CANCEL_REAL_TIME_BARS,
            Box::new(|w| encode::cancel_real_time_bars(w, 1)),
        ),
        (
            out::REQ_FUNDAMENTAL_DATA,
            Box::new(|w| encode::req_fundamental_data(w, 1, &c, "ReportSnapshot", &tv)),
        ),
        (
            out::CANCEL_FUNDAMENTAL_DATA,
            Box::new(|w| encode::cancel_fundamental_data(w, 1)),
        ),
        (
            out::REQ_CALC_IMPLIED_VOLAT,
            Box::new(|w| encode::calculate_implied_volatility(w, 1, &c, 1.0, 100.0, &tv)),
        ),
        (
            out::REQ_CALC_OPTION_PRICE,
            Box::new(|w| encode::calculate_option_price(w, 1, &c, 0.2, 100.0, &tv)),
        ),
        (
            out::CANCEL_CALC_IMPLIED_VOLAT,
            Box::new(|w| encode::cancel_calculate_implied_volatility(w, 1)),
        ),
        (
            out::CANCEL_CALC_OPTION_PRICE,
            Box::new(|w| encode::cancel_calculate_option_price(w, 1)),
        ),
        (
            out::REQ_GLOBAL_CANCEL,
            Box::new(|w| encode::req_global_cancel(w, 200, &oc)),
        ),
        (
            out::REQ_MARKET_DATA_TYPE,
            Box::new(|w| encode::req_market_data_type(w, 3)),
        ),
        (out::REQ_POSITIONS, Box::new(encode::req_positions)),
        (
            out::REQ_ACCOUNT_SUMMARY,
            Box::new(|w| encode::req_account_summary(w, 1, "All", "NetLiquidation")),
        ),
        (
            out::CANCEL_ACCOUNT_SUMMARY,
            Box::new(|w| encode::cancel_account_summary(w, 1)),
        ),
        (out::CANCEL_POSITIONS, Box::new(encode::cancel_positions)),
        (
            out::VERIFY_REQUEST,
            Box::new(|w| encode::verify_request(w, "app", "1")),
        ),
        (
            out::VERIFY_MESSAGE,
            Box::new(|w| encode::verify_message(w, "data")),
        ),
        (
            out::QUERY_DISPLAY_GROUPS,
            Box::new(|w| encode::query_display_groups(w, 1)),
        ),
        (
            out::SUBSCRIBE_TO_GROUP_EVENTS,
            Box::new(|w| encode::subscribe_to_group_events(w, 1, 1)),
        ),
        (
            out::UPDATE_DISPLAY_GROUP,
            Box::new(|w| encode::update_display_group(w, 1, "8314@SMART")),
        ),
        (
            out::UNSUBSCRIBE_FROM_GROUP_EVENTS,
            Box::new(|w| encode::unsubscribe_from_group_events(w, 1)),
        ),
        (out::START_API, Box::new(|w| encode::start_api(w, 1, ""))),
        (
            out::VERIFY_AND_AUTH_REQUEST,
            Box::new(|w| encode::verify_and_auth_request(w, "app", "1", "key")),
        ),
        (
            out::VERIFY_AND_AUTH_MESSAGE,
            Box::new(|w| encode::verify_and_auth_message(w, "data", "xyz")),
        ),
        (
            out::REQ_POSITIONS_MULTI,
            Box::new(|w| encode::req_positions_multi(w, 1, "DU1", "")),
        ),
        (
            out::CANCEL_POSITIONS_MULTI,
            Box::new(|w| encode::cancel_positions_multi(w, 1)),
        ),
        (
            out::REQ_ACCOUNT_UPDATES_MULTI,
            Box::new(|w| encode::req_account_updates_multi(w, 1, "DU1", "", true)),
        ),
        (
            out::CANCEL_ACCOUNT_UPDATES_MULTI,
            Box::new(|w| encode::cancel_account_updates_multi(w, 1)),
        ),
        (
            out::REQ_SEC_DEF_OPT_PARAMS,
            Box::new(|w| encode::req_sec_def_opt_params(w, 1, "IBM", "", "STK", 8314)),
        ),
        (
            out::REQ_SOFT_DOLLAR_TIERS,
            Box::new(|w| encode::req_soft_dollar_tiers(w, 1)),
        ),
        (out::REQ_FAMILY_CODES, Box::new(encode::req_family_codes)),
        (
            out::REQ_MATCHING_SYMBOLS,
            Box::new(|w| encode::req_matching_symbols(w, 1, "IB")),
        ),
        (
            out::REQ_MKT_DEPTH_EXCHANGES,
            Box::new(encode::req_mkt_depth_exchanges),
        ),
        (
            out::REQ_SMART_COMPONENTS,
            Box::new(|w| encode::req_smart_components(w, 1, "9c0001")),
        ),
        (
            out::REQ_NEWS_ARTICLE,
            Box::new(|w| encode::req_news_article(w, 1, "BZ", "BZ$1", &tv)),
        ),
        (
            out::REQ_NEWS_PROVIDERS,
            Box::new(encode::req_news_providers),
        ),
        (
            out::REQ_HISTORICAL_NEWS,
            Box::new(|w| encode::req_historical_news(w, 1, 8314, "BZ", "", "", 10, &tv)),
        ),
        (
            out::REQ_HEAD_TIMESTAMP,
            Box::new(|w| encode::req_head_time_stamp(w, 1, &c, "TRADES", true, 1)),
        ),
        (
            out::REQ_HISTOGRAM_DATA,
            Box::new(|w| encode::req_histogram_data(w, 1, &c, true, "1 week")),
        ),
        (
            out::CANCEL_HISTOGRAM_DATA,
            Box::new(|w| encode::cancel_histogram_data(w, 1)),
        ),
        (
            out::CANCEL_HEAD_TIMESTAMP,
            Box::new(|w| encode::cancel_head_time_stamp(w, 1)),
        ),
        (
            out::REQ_MARKET_RULE,
            Box::new(|w| encode::req_market_rule(w, 26)),
        ),
        (out::REQ_PNL, Box::new(|w| encode::req_pnl(w, 1, "DU1", ""))),
        (out::CANCEL_PNL, Box::new(|w| encode::cancel_pnl(w, 1))),
        (
            out::REQ_PNL_SINGLE,
            Box::new(|w| encode::req_pnl_single(w, 1, "DU1", "", 8314)),
        ),
        (
            out::CANCEL_PNL_SINGLE,
            Box::new(|w| encode::cancel_pnl_single(w, 1)),
        ),
        (
            out::REQ_HISTORICAL_TICKS,
            Box::new(|w| {
                encode::req_historical_ticks(
                    w,
                    1,
                    &c,
                    "",
                    "20260102 10:00:00",
                    100,
                    "TRADES",
                    true,
                    false,
                    &tv,
                )
            }),
        ),
        (
            out::REQ_TICK_BY_TICK_DATA,
            Box::new(|w| encode::req_tick_by_tick_data(w, 1, &c, "AllLast", 0, false)),
        ),
        (
            out::CANCEL_TICK_BY_TICK_DATA,
            Box::new(|w| encode::cancel_tick_by_tick_data(w, 1)),
        ),
        (
            out::REQ_COMPLETED_ORDERS,
            Box::new(|w| encode::req_completed_orders(w, false)),
        ),
        (
            out::REQ_WSH_META_DATA,
            Box::new(|w| encode::req_wsh_meta_data(w, 1)),
        ),
        (
            out::CANCEL_WSH_META_DATA,
            Box::new(|w| encode::cancel_wsh_meta_data(w, 1)),
        ),
        (
            out::REQ_WSH_EVENT_DATA,
            Box::new(|w| encode::req_wsh_event_data(w, 200, 1, &wsh)),
        ),
        (
            out::CANCEL_WSH_EVENT_DATA,
            Box::new(|w| encode::cancel_wsh_event_data(w, 1)),
        ),
        (
            out::REQ_USER_INFO,
            Box::new(|w| encode::req_user_info(w, 1)),
        ),
        (
            out::REQ_CURRENT_TIME_IN_MILLIS,
            Box::new(|w| encode::req_current_time_in_millis(w, 200).unwrap()),
        ),
    ];
    let mut seen = std::collections::BTreeSet::new();
    for (id, enc) in &cases {
        let mut w = FieldWriter::new();
        enc(&mut w);
        let f = fields(&mut w);
        assert_eq!(
            f[0],
            id.to_string(),
            "request {id} starts with its message id"
        );
        assert!(seen.insert(*id), "request {id} listed twice");
    }
    assert_eq!(
        seen.len(),
        81,
        "every outgoing message id has an encoder case"
    );
}

#[test]
fn requests_that_need_a_newer_server_are_refused() {
    let mut w = FieldWriter::new();
    assert_eq!(
        encode::req_current_time_in_millis(&mut w, 196)
            .unwrap_err()
            .min_version,
        197
    );
    let err = encode::exercise_options(
        &mut w,
        178,
        1,
        &aapl(),
        1,
        1,
        "DU1",
        0,
        "20260101-10:00:00",
        "",
        false,
    )
    .unwrap_err();
    assert_eq!(err.min_version, 180);
}
