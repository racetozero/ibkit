//! One wire vector per server message type. Each must decode to a known
//! variant and consume exactly its fields. Field order follows IB's official
//! decoder; the server version 178 open order vector also matches ib_async.

use super::*;
use crate::message::incoming as msg;

const MAX: &str = "1.7976931348623157e308";

/// (server version, fields including the message id, every field consumed)
type Case = (i32, Vec<&'static str>, bool);

fn open_order_178() -> Vec<&'static str> {
    let mut f = vec![
        "5", "42", "265598", "AAPL", "STK", "", "0", "", "", "SMART", "USD", "AAPL", "NMS",
    ];
    f.extend([
        "BUY",
        "100",
        "LMT",
        "150.5",
        "",
        "DAY",
        "",
        "DU1",
        "O",
        "0",
        "ref",
        "1",
        "1234567890",
        "0",
        "0",
        "0",
        "",
        "",
    ]);
    f.extend(["", "", "", "", "", "", "", "", "0", "", "-1"]);
    f.extend([
        "0", "", "", "", "", "", "0", "0", "0", "0", "", "3", "0", "0", "", "0", "0",
    ]);
    f.extend([
        "", "0", "", "", "0", "0", "", "", "", "", "", "0", "0", "0", "", "", "", "", "0", "", "",
        "0", "0", "",
    ]);
    f.extend([
        "0",
        "0",
        "Submitted",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
    ]);
    f.extend([
        "0", "0", "0", "", MAX, MAX, MAX, MAX, MAX, MAX, "0", "", "", "", MAX, "0", "0", "0", "0",
        "", "", "0",
    ]);
    f.extend(["", "", "", "", ""]);
    f
}

fn open_order_200() -> Vec<&'static str> {
    let mut f = open_order_178();
    // Full preview fields (195) follow the commission currency.
    // status, 9 margin strings, 3 commissions, currency, then warningText.
    let pos = f.iter().position(|x| *x == "Submitted").unwrap() + 14;
    let preview = [
        "USD", MAX, MAX, MAX, MAX, MAX, MAX, MAX, MAX, MAX, "", "", "0",
    ];
    for (i, v) in preview.iter().enumerate() {
        f.insert(pos + i, v);
    }
    // customerAccount, professionalCustomer, bondAccruedInterest,
    // includeOvernight, extOperator, manualOrderIndicator, submitter,
    // imbalanceOnly.
    f.extend(["", "0", "", "0", "", "", "me", "0"]);
    f
}

fn completed_order(v200: bool) -> Vec<&'static str> {
    let mut f = vec![
        "101", "265598", "AAPL", "STK", "", "0", "", "", "SMART", "USD", "AAPL", "NMS",
    ];
    f.extend([
        "BUY",
        "100",
        "LMT",
        "150.5",
        "",
        "DAY",
        "",
        "DU1",
        "O",
        "0",
        "ref",
        "1234567890",
        "0",
        "0",
        "0",
        "",
    ]);
    f.extend(["", "", "", "", "", "", "", "", "0", "", "-1"]);
    f.extend(["", "", "", "", "", "", "0", "0", "", "0", "0"]);
    f.extend(["", "0", "", "", "0", "0", "", ""]);
    f.extend(["", "0", "0", "0", "", "", "", ""]);
    f.extend(["", "", "0", "0", "", "0", "Filled", "0", "0", "0"]);
    f.extend([
        "",
        "",
        "",
        "0",
        "0",
        "",
        "100",
        "0",
        "0",
        "",
        "0",
        "0",
        "0",
        "20260102 10:00:00",
        "Filled",
    ]);
    f.extend(["", "", "", "", ""]);
    if v200 {
        f.extend(["", "0", "me"]);
    }
    f
}

fn contract_data(v200: bool) -> Vec<&'static str> {
    let mut f = vec!["10", "3", "AAPL", "STK", "20260102 16:00 US/Eastern"];
    if v200 {
        f.push("20260102");
    }
    f.extend([
        "0",
        "",
        "SMART",
        "USD",
        "AAPL",
        "NMS",
        "NMS",
        "265598",
        "0.01",
        "1",
        "LMT",
        "SMART",
        "1",
        "0",
        "APPLE INC",
    ]);
    f.extend([
        "NASDAQ",
        "",
        "Tech",
        "",
        "",
        "US/Eastern",
        "",
        "",
        "",
        "0",
        "1",
        "ISIN",
        "US0378331005",
        "1",
        "AAPL",
        "",
        "26",
        "",
        "COMMON",
    ]);
    f.extend(["0.0001", "0.0001", "100"]);
    if v200 {
        f.extend(["1", "id1", "reason"]);
    }
    f
}

fn bond_contract_data(v200: bool) -> Vec<&'static str> {
    let mut f = vec![
        "18", "4", "T", "BOND", "912828", "4.5", "20300515", "20200515", "AAA", "TREASURY",
        "FIXED", "0", "0", "0", "",
    ];
    f.extend([
        "SMART",
        "USD",
        "T",
        "T",
        "1234",
        "0.01",
        "LMT",
        "SMART",
        "",
        "",
        "0",
        "",
        "US T 4.5 05/15/30",
    ]);
    if v200 {
        f.extend([
            "US/Eastern",
            "20260102:0800-20260102:1700",
            "20260102:0800-20260102:1700",
        ]);
    }
    f.extend(["", "0", "0", "1", "1", "1000", "1000", "1000"]);
    f
}

fn execution(v200: bool) -> Vec<&'static str> {
    let mut f = vec![
        "11", "5", "12", "265598", "AAPL", "STK", "", "0", "", "", "ISLAND", "USD", "AAPL", "NMS",
        "e1",
    ];
    f.extend([
        "20260102 10:00:00",
        "DU1",
        "ISLAND",
        "BOT",
        "100",
        "150.5",
        "987654321",
        "1",
        "0",
        "100",
        "150.5",
        "",
        "",
        "",
        "",
        "1",
        "0",
    ]);
    if v200 {
        f.push("me");
    }
    f
}

fn cases() -> Vec<Case> {
    let c = |v: i32, f: &[&'static str]| (v, f.to_vec(), true);
    vec![
        c(178, &["1", "6", "1", "1", "1.5", "100", "3"]),
        c(178, &["2", "6", "1", "8", "100"]),
        c(
            178,
            &[
                "3",
                "7",
                "Submitted",
                "0",
                "100",
                "0",
                "777",
                "0",
                "0",
                "1",
                "",
                "0",
            ],
        ),
        c(178, &["4", "2", "7", "200", "No security definition", ""]),
        c(
            200,
            &[
                "4",
                "7",
                "200",
                "No security definition",
                "",
                "1767225600000",
            ],
        ),
        (178, open_order_178(), true),
        (200, open_order_200(), true),
        c(178, &["6", "2", "NetLiquidation", "100", "USD", "DU1"]),
        c(
            178,
            &[
                "7", "8", "8314", "IBM", "STK", "", "0", "", "", "NYSE", "USD", "IBM", "IBM", "10",
                "120", "1200", "110", "100", "0", "DU1",
            ],
        ),
        c(178, &["8", "1", "10:00"]),
        c(178, &["9", "1", "1000"]),
        (178, contract_data(false), true),
        (200, contract_data(true), true),
        (178, execution(false), true),
        (200, execution(true), true),
        c(178, &["12", "1", "3", "0", "0", "1", "150.5", "100"]),
        c(
            178,
            &["13", "1", "3", "0", "MM", "0", "1", "150.5", "100", "1"],
        ),
        c(178, &["14", "1", "9", "1", "halted", "NYSE"]),
        c(178, &["15", "1", "DU1,DU2"]),
        c(178, &["16", "1", "1", "<xml/>"]),
        c(
            178,
            &[
                "17", "9", "a", "b", "1", "20260102", "1", "2", "0.5", "1.5", "100", "1.2", "10",
            ],
        ),
        c(
            200,
            &[
                "17", "9", "1", "20260102", "1", "2", "0.5", "1.5", "100", "1.2", "10",
            ],
        ),
        (178, bond_contract_data(false), true),
        (200, bond_contract_data(true), true),
        c(178, &["19", "1", "<xml/>"]),
        c(
            178,
            &[
                "20", "3", "5", "1", "0", "265598", "AAPL", "STK", "", "0", "", "SMART", "USD",
                "AAPL", "NMS", "NMS", "", "", "", "",
            ],
        ),
        c(
            178,
            &[
                "21", "4", "13", "1", "0.2", "0.5", "2.5", "0", "0.01", "0.1", "-0.05", "150",
            ],
        ),
        c(178, &["45", "6", "4", "49", "1"]),
        c(178, &["46", "6", "4", "45", "1767225600"]),
        c(
            178,
            &[
                "47", "6", "4", "38", "1.5", "1.5%", "101", "30", "20260320", "0.1", "0.2",
            ],
        ),
        c(178, &["49", "1", "1767225600"]),
        c(
            178,
            &[
                "50",
                "3",
                "9",
                "1767225600",
                "1",
                "2",
                "0.5",
                "1.5",
                "100",
                "1.2",
                "10",
            ],
        ),
        c(178, &["51", "1", "9", "<xml/>"]),
        c(178, &["52", "1", "9"]),
        (178, vec!["53", "1"], false),
        c(178, &["54", "1", "DU1"]),
        c(178, &["55", "1", "9"]),
        c(178, &["56", "1", "9", "8314", "0.5", "120"]),
        c(178, &["57", "1", "9"]),
        c(178, &["58", "1", "9", "3"]),
        c(178, &["59", "1", "e1", "1", "USD", MAX, MAX, ""]),
        c(
            178,
            &[
                "61", "3", "DU1", "8314", "IBM", "STK", "", "0", "", "", "NYSE", "USD", "IBM",
                "IBM", "10", "120.5",
            ],
        ),
        (178, vec!["62", "1"], false),
        c(
            178,
            &["63", "1", "9", "DU1", "NetLiquidation", "100", "USD"],
        ),
        c(178, &["64", "1", "9"]),
        c(178, &["65", "1", "data"]),
        c(178, &["66", "1", "1", ""]),
        c(178, &["67", "1", "9", "1|2|3"]),
        c(178, &["68", "1", "9", "8314@SMART"]),
        c(178, &["69", "1", "data", "challenge"]),
        c(178, &["70", "1", "0", "failed"]),
        c(
            178,
            &[
                "71", "1", "9", "DU1", "8314", "IBM", "STK", "", "0", "", "", "NYSE", "USD", "IBM",
                "IBM", "10", "120.5", "model",
            ],
        ),
        c(178, &["72", "1", "9"]),
        c(
            178,
            &["73", "1", "9", "DU1", "model", "CashBalance", "5", "USD"],
        ),
        c(178, &["74", "1", "9"]),
        c(
            178,
            &[
                "75", "9", "SMART", "8314", "IBM", "100", "2", "20260320", "20260417", "1", "100",
            ],
        ),
        c(178, &["76", "9"]),
        c(178, &["77", "9", "1", "name", "val", "display"]),
        c(178, &["78", "1", "DU1", "F1"]),
        c(
            178,
            &[
                "79",
                "9",
                "1",
                "8314",
                "IBM",
                "STK",
                "NYSE",
                "USD",
                "1",
                "OPT",
                "INTL BUSINESS MACHINES",
                "",
            ],
        ),
        c(178, &["80", "1", "ISLAND", "STK", "NASDAQ", "Deep2", "1"]),
        c(178, &["81", "9", "0.01", "9c0001", "3"]),
        c(178, &["82", "9", "1", "0", "NYSE", "N"]),
        c(178, &["83", "9", "0", "text"]),
        c(
            178,
            &["84", "9", "1767225600", "BZ", "BZ$1", "headline", "extra"],
        ),
        c(178, &["85", "1", "BZ", "Benzinga"]),
        c(
            178,
            &["86", "9", "2026-01-02 10:00:00.0", "BZ", "BZ$1", "headline"],
        ),
        c(178, &["87", "9", "1"]),
        c(178, &["88", "9", "19800317-14:30:00"]),
        c(178, &["89", "9", "1", "150.5", "1000"]),
        c(
            178,
            &[
                "90",
                "9",
                "10",
                "20260102 10:00:00",
                "1",
                "2",
                "0.5",
                "1.5",
                "1.2",
                "100",
            ],
        ),
        c(178, &["91", "9", "8314", "NYSE"]),
        c(178, &["92", "9", "8314", "NYSE"]),
        c(178, &["93", "26", "1", "0", "0.01"]),
        c(178, &["94", "9", "10", "20", "30"]),
        c(178, &["95", "9", "100", "10", "20", "30", "15000"]),
        c(
            178,
            &["96", "9", "1", "1767225600", "0", "150.5", "100", "1"],
        ),
        c(
            178,
            &[
                "97",
                "9",
                "1",
                "1767225600",
                "3",
                "150.4",
                "150.6",
                "100",
                "200",
                "1",
            ],
        ),
        c(
            178,
            &[
                "98",
                "9",
                "1",
                "1767225600",
                "1",
                "150.5",
                "100",
                "ISLAND",
                "",
                "1",
            ],
        ),
        c(
            178,
            &[
                "99",
                "9",
                "1",
                "1767225600",
                "150.5",
                "100",
                "2",
                "ISLAND",
                "",
            ],
        ),
        c(
            178,
            &[
                "99",
                "9",
                "3",
                "1767225600",
                "150.4",
                "150.6",
                "100",
                "200",
                "3",
            ],
        ),
        c(178, &["99", "9", "4", "1767225600", "150.5"]),
        c(178, &["100", "987654321", "1", "42"]),
        (178, completed_order(false), true),
        (200, completed_order(true), true),
        c(178, &["102"]),
        c(178, &["103", "9", "done"]),
        c(178, &["104", "9", "{}"]),
        c(178, &["105", "9", "{}"]),
        c(
            178,
            &[
                "106",
                "9",
                "20260102-09:30:00",
                "20260102-16:00:00",
                "US/Eastern",
                "1",
                "20260102-09:30:00",
                "20260102-16:00:00",
                "20260102",
            ],
        ),
        c(178, &["107", "9", "brand"]),
        c(200, &["108", "9", "a", "b"]),
        c(200, &["109", "1767225600123"]),
    ]
}

fn payload(fields: &[&str]) -> Vec<u8> {
    let mut p = Vec::new();
    for f in fields {
        p.extend_from_slice(f.as_bytes());
        p.push(0);
    }
    p
}

#[test]
fn every_server_message_decodes_with_exact_field_count() {
    for (v, fields, exact) in cases() {
        let (m, consumed) = decode_counting(v, &payload(&fields))
            .unwrap_or_else(|e| panic!("message {} at version {v}: {e}", fields[0]));
        assert!(
            !matches!(m, Incoming::Unknown { .. }),
            "message {} at version {v} decoded as Unknown",
            fields[0]
        );
        if exact {
            assert!(
                consumed,
                "message {} at version {v} left fields unread: {m:?}",
                fields[0]
            );
        }
    }
}

#[test]
fn every_server_message_type_has_a_vector() {
    let covered: std::collections::BTreeSet<i32> = cases()
        .iter()
        .map(|(_, f, _)| f[0].parse().unwrap())
        .collect();
    let all = [
        msg::TICK_PRICE,
        msg::TICK_SIZE,
        msg::ORDER_STATUS,
        msg::ERR_MSG,
        msg::OPEN_ORDER,
        msg::ACCT_VALUE,
        msg::PORTFOLIO_VALUE,
        msg::ACCT_UPDATE_TIME,
        msg::NEXT_VALID_ID,
        msg::CONTRACT_DATA,
        msg::EXECUTION_DATA,
        msg::MARKET_DEPTH,
        msg::MARKET_DEPTH_L2,
        msg::NEWS_BULLETINS,
        msg::MANAGED_ACCTS,
        msg::RECEIVE_FA,
        msg::HISTORICAL_DATA,
        msg::BOND_CONTRACT_DATA,
        msg::SCANNER_PARAMETERS,
        msg::SCANNER_DATA,
        msg::TICK_OPTION_COMPUTATION,
        msg::TICK_GENERIC,
        msg::TICK_STRING,
        msg::TICK_EFP,
        msg::CURRENT_TIME,
        msg::REAL_TIME_BARS,
        msg::FUNDAMENTAL_DATA,
        msg::CONTRACT_DATA_END,
        msg::OPEN_ORDER_END,
        msg::ACCT_DOWNLOAD_END,
        msg::EXECUTION_DATA_END,
        msg::DELTA_NEUTRAL_VALIDATION,
        msg::TICK_SNAPSHOT_END,
        msg::MARKET_DATA_TYPE,
        msg::COMMISSION_REPORT,
        msg::POSITION_DATA,
        msg::POSITION_END,
        msg::ACCOUNT_SUMMARY,
        msg::ACCOUNT_SUMMARY_END,
        msg::VERIFY_MESSAGE_API,
        msg::VERIFY_COMPLETED,
        msg::DISPLAY_GROUP_LIST,
        msg::DISPLAY_GROUP_UPDATED,
        msg::VERIFY_AND_AUTH_MESSAGE_API,
        msg::VERIFY_AND_AUTH_COMPLETED,
        msg::POSITION_MULTI,
        msg::POSITION_MULTI_END,
        msg::ACCOUNT_UPDATE_MULTI,
        msg::ACCOUNT_UPDATE_MULTI_END,
        msg::SECURITY_DEFINITION_OPTION_PARAMETER,
        msg::SECURITY_DEFINITION_OPTION_PARAMETER_END,
        msg::SOFT_DOLLAR_TIERS,
        msg::FAMILY_CODES,
        msg::SYMBOL_SAMPLES,
        msg::MKT_DEPTH_EXCHANGES,
        msg::TICK_REQ_PARAMS,
        msg::SMART_COMPONENTS,
        msg::NEWS_ARTICLE,
        msg::TICK_NEWS,
        msg::NEWS_PROVIDERS,
        msg::HISTORICAL_NEWS,
        msg::HISTORICAL_NEWS_END,
        msg::HEAD_TIMESTAMP,
        msg::HISTOGRAM_DATA,
        msg::HISTORICAL_DATA_UPDATE,
        msg::REROUTE_MKT_DATA_REQ,
        msg::REROUTE_MKT_DEPTH_REQ,
        msg::MARKET_RULE,
        msg::PNL,
        msg::PNL_SINGLE,
        msg::HISTORICAL_TICKS,
        msg::HISTORICAL_TICKS_BID_ASK,
        msg::HISTORICAL_TICKS_LAST,
        msg::TICK_BY_TICK,
        msg::ORDER_BOUND,
        msg::COMPLETED_ORDER,
        msg::COMPLETED_ORDERS_END,
        msg::REPLACE_FA_END,
        msg::WSH_META_DATA,
        msg::WSH_EVENT_DATA,
        msg::HISTORICAL_SCHEDULE,
        msg::USER_INFO,
        msg::HISTORICAL_DATA_END,
        msg::CURRENT_TIME_IN_MILLIS,
    ];
    assert_eq!(all.len(), 84);
    let missing: Vec<i32> = all
        .iter()
        .copied()
        .filter(|id| !covered.contains(id))
        .collect();
    assert!(
        missing.is_empty(),
        "message ids without a vector: {missing:?}"
    );
}

#[test]
fn gated_fields_are_read_at_version_200() {
    let p = |f: Vec<&str>| payload(&f);
    let Incoming::OpenOrder { order, state, .. } = decode(200, &p(open_order_200())).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        (order.submitter.as_str(), state.margin_currency.as_str()),
        ("me", "USD")
    );
    let Incoming::ContractDetails { details, .. } = decode(200, &p(contract_data(true))).unwrap()
    else {
        panic!()
    };
    assert_eq!(details.contract.last_trade_date, "20260102");
    assert_eq!(details.last_trade_time, "16:00");
    assert_eq!(details.ineligibility_reason_list[0].description, "reason");
    let Incoming::BondContractDetails { details, .. } =
        decode(200, &p(bond_contract_data(true))).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        (details.maturity.as_str(), details.time_zone_id.as_str()),
        ("20300515", "US/Eastern")
    );
    let Incoming::CompletedOrder { order, state, .. } =
        decode(200, &p(completed_order(true))).unwrap()
    else {
        panic!()
    };
    assert_eq!(
        (
            order.submitter.as_str(),
            state.completed_status.as_str(),
            order.filled_quantity
        ),
        ("me", "Filled", 100.0)
    );
}

#[test]
fn truncated_messages_fail_instead_of_panicking() {
    for (v, fields, _) in cases() {
        for cut in 1..fields.len() {
            let _ = decode(v, &payload(&fields[..cut]));
        }
    }
}

#[test]
fn an_extra_field_is_detected() {
    let (_, consumed) = decode_counting(178, &payload(&["9", "1", "1000", "extra"])).unwrap();
    assert!(!consumed);
}
