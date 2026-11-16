//! Interactive Brokers order messages over FIX 4.2, as the IBKR FIX
//! Implementation Manual (Revision 2025-001) specifies them.
//!
//! The builders turn a TWS [`Contract`] and [`Order`] into the messages IB
//! accepts: NewOrderSingle (35=D), OrderCancelReplaceRequest (G),
//! OrderCancelRequest (F), OrderStatusRequest (H), NewOrderMultileg (AB),
//! MultilegCancelReplace (AC) and AllocationInstruction (J). IB supports no
//! other application message types; the typed views read what IB sends:
//! [`ExecutionReport`] (8), [`OrderCancelReject`] (9), [`Bulletin`] (B) and
//! [`AllocationAck`] (P).
//!
//! IB conventions applied: HandlInst 21=2, Account (1) on orders, cancels and
//! status requests, derivatives by full exchange symbol (6035) or root plus
//! expiry/right/strike, IB's order type codes (40=J MIT, LT LIT, TSL, MIDPX,
//! PSVR, SMKT, SMID, E2M peg-to-best, P with ExecInst R/P/M/a/s), OCA groups
//! as 583=`IBOCAGROUP <name>`, algos as 18=e + 847 + IB algo tags + 6205=1,
//! times in UTC `yyyymmdd-hh:mm:ss`. Tags without a TWS `Order` field
//! (ParentClientID 6107, SenderLocationID 142, pre-trade allocations 78/79/80,
//! ...) are pushed by the caller with [`Message::push`]; see [`crate::tags`].

use crate::message::{Message, utc_timestamp};
use crate::tags::{self, msg_type};
use chrono::TimeZone;
use ibkit_tws::consts::{UNSET_DOUBLE, UNSET_INTEGER};
use ibkit_tws::types::{Contract, Order, OrderCondition};

/// A contract or order that cannot be expressed in IB's FIX dialect.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct MappingError(pub String);

fn err<T>(text: String) -> Result<T, MappingError> {
    Err(MappingError(text))
}

fn set(v: f64) -> Option<f64> {
    (v != UNSET_DOUBLE && !v.is_nan()).then_some(v)
}

fn set_i(v: i32) -> Option<i32> {
    (v != UNSET_INTEGER).then_some(v)
}

fn side(action: &str) -> Result<&'static str, MappingError> {
    match action {
        "BUY" => Ok("1"),
        "SELL" => Ok("2"),
        "SSHORT" => Ok("5"),
        other => err(format!(
            "order action is {other:?}; use BUY, SELL or SSHORT"
        )),
    }
}

fn tif(tif: &str) -> Result<&'static str, MappingError> {
    match tif {
        "" | "DAY" => Ok("0"),
        "GTC" => Ok("1"),
        "OPG" => Ok("2"),
        "IOC" => Ok("3"),
        "FOK" => Ok("4"),
        "GTD" => Ok("6"),
        "MINUTES" => Ok("p"),
        "AUC" => Ok("8"),
        other => err(format!(
            "time in force is {other:?}; use DAY, GTC, OPG, IOC, FOK, GTD, MINUTES (crypto) or AUC"
        )),
    }
}

/// FIX SecurityType (167) for a TWS secType.
pub fn security_type(sec_type: &str) -> &str {
    match sec_type {
        "STK" => "CS",
        "BAG" => "MULTILEG",
        other => other,
    }
}

/// UTC `yyyymmdd-hh:mm:ss` for a TWS time: `yyyymmdd-hh:mm:ss` (UTC),
/// `yyyymmdd hh:mm:ss <zone>` or `hh:mm:ss <zone>` (today in that zone),
/// where `<zone>` is `UTC`, `GMT` or an IANA name such as `US/Eastern`.
pub fn utc_time(field: &str, value: &str) -> Result<String, MappingError> {
    let bad = || {
        MappingError(format!(
            "{field} is {value:?}; IB's FIX engine needs a time it can convert to UTC. Use \"yyyymmdd-hh:mm:ss\" (UTC), \"yyyymmdd hh:mm:ss <zone>\" or \"hh:mm:ss <zone>\" with zone UTC or an IANA name such as US/Eastern"
        ))
    };
    if value.len() == 17 && value.as_bytes()[8] == b'-' {
        return crate::message::parse_utc_timestamp(value)
            .map(|_| value.to_string())
            .ok_or_else(bad);
    }
    let parts: Vec<&str> = value.split_whitespace().collect();
    let (date, time, zone) = match parts.as_slice() {
        [d, t, z] => (Some(*d), *t, *z),
        [t, z] if t.contains(':') => (None, *t, *z),
        _ => return Err(bad()),
    };
    let tz: chrono_tz::Tz = match zone {
        "UTC" | "GMT" => chrono_tz::UTC,
        z => z.parse().map_err(|_| bad())?,
    };
    let date = match date {
        Some(d) => chrono::NaiveDate::parse_from_str(d, "%Y%m%d").map_err(|_| bad())?,
        None => chrono::Utc::now().with_timezone(&tz).date_naive(),
    };
    let time = chrono::NaiveTime::parse_from_str(time, "%H:%M:%S").map_err(|_| bad())?;
    let local = tz
        .from_local_datetime(&date.and_time(time))
        .single()
        .ok_or_else(bad)?;
    Ok(local
        .with_timezone(&chrono::Utc)
        .format("%Y%m%d-%H:%M:%S")
        .to_string())
}

fn add_instrument(m: &mut Message, c: &Contract) {
    m.push(tags::SYMBOL, &c.symbol);
    if !c.sec_type.is_empty() {
        m.push(tags::SECURITY_TYPE, security_type(&c.sec_type));
    }
    let id_source = match c.sec_id_type.as_str() {
        "CUSIP" => Some("1"),
        "ISIN" => Some("4"),
        "FIGI" => Some("S"),
        _ => None,
    };
    if let Some(src) = id_source
        && !c.sec_id.is_empty()
    {
        m.push(tags::SECURITY_ID, &c.sec_id)
            .push(tags::ID_SOURCE, src);
    }
    if matches!(c.sec_type.as_str(), "OPT" | "FOP" | "FUT" | "WAR") {
        let ltd = &c.last_trade_date_or_contract_month;
        if ltd.len() >= 6 {
            m.push(tags::MATURITY_MONTH_YEAR, &ltd[..6]);
        }
        if ltd.len() >= 8 {
            m.push(tags::MATURITY_DAY, &ltd[6..8]);
        }
        if matches!(c.sec_type.as_str(), "OPT" | "FOP" | "WAR") {
            match c.right.as_str() {
                "P" | "PUT" => {
                    m.push(tags::PUT_OR_CALL, 0);
                }
                "C" | "CALL" => {
                    m.push(tags::PUT_OR_CALL, 1);
                }
                _ => {}
            }
            if let Some(k) = set(c.strike).filter(|k| *k != 0.0) {
                m.push_f64(tags::STRIKE_PRICE, k);
            }
        }
        if !c.multiplier.is_empty() {
            m.push(tags::CONTRACT_MULTIPLIER, &c.multiplier);
        }
    }
    if c.con_id > 0 {
        m.push(tags::CONTRACT_ID, c.con_id);
    }
    if !c.local_symbol.is_empty() {
        m.push(tags::IB_LOCAL_SYMBOL, &c.local_symbol);
    }
    if !c.trading_class.is_empty() {
        m.push(tags::TRADING_CLASS, &c.trading_class);
    }
    if !c.primary_exchange.is_empty() {
        m.push(tags::SECURITY_EXCHANGE, &c.primary_exchange);
    }
    if !c.exchange.is_empty() {
        m.push(tags::EX_DESTINATION, &c.exchange);
    }
    if !c.currency.is_empty() {
        m.push(tags::CURRENCY, &c.currency);
    }
}

/// IB's rules for identifying the instrument of an order.
fn check_instrument(c: &Contract) -> Result<(), MappingError> {
    if c.exchange == "SMART" && c.currency.is_empty() && c.primary_exchange.is_empty() {
        return err(format!(
            "contract {} routes to SMART without currency or primary exchange; IB needs tag 15 or 207 with 100=SMART, so set contract.currency (and ideally primary_exchange)",
            c.symbol
        ));
    }
    if matches!(c.sec_type.as_str(), "FUT" | "FOP") && c.exchange == "SMART" {
        return err(format!(
            "contract {} is a future on SMART; IB has no SMART routing for futures, so set contract.exchange to the futures exchange",
            c.symbol
        ));
    }
    if c.sec_type == "BOND" && (c.sec_id_type != "ISIN" || c.sec_id.is_empty()) {
        return err(format!(
            "bond {} has no ISIN; IB identifies bonds only by 48=<ISIN> 22=4, so set contract.sec_id_type = \"ISIN\" and contract.sec_id",
            c.symbol
        ));
    }
    Ok(())
}

/// Exec instructions, accumulated and sent space separated in tag 18.
#[derive(Default)]
struct ExecInst(Vec<&'static str>);

fn trailing(m: &mut Message, o: &Order) {
    match (set(o.trailing_percent), set(o.aux_price)) {
        (Some(pct), _) => {
            m.push_f64(tags::PEG_DIFFERENCE, pct)
                .push(tags::TRAILING_AMT_UNIT, 100);
        }
        (None, Some(amt)) => {
            m.push_f64(tags::PEG_DIFFERENCE, amt);
        }
        (None, None) => {}
    }
}

fn add_order_type(m: &mut Message, o: &Order, inst: &mut ExecInst) -> Result<(), MappingError> {
    let lmt = set(o.lmt_price);
    let aux = set(o.aux_price);
    let price = |m: &mut Message| {
        if let Some(p) = lmt {
            m.push_f64(tags::PRICE, p);
        }
    };
    let stop = |m: &mut Message| {
        if let Some(p) = aux {
            m.push_f64(tags::STOP_PX, p);
        }
    };
    let offset = |m: &mut Message| {
        if let Some(a) = aux {
            m.push_f64(tags::PEG_DIFFERENCE, a);
        }
    };
    match o.order_type.as_str() {
        "MKT" => {
            m.push(tags::ORD_TYPE, "1");
        }
        "LMT" => {
            m.push(tags::ORD_TYPE, "2");
            price(m);
        }
        "STP" => {
            m.push(tags::ORD_TYPE, "3");
            stop(m);
        }
        "STP LMT" => {
            m.push(tags::ORD_TYPE, "4");
            price(m);
            stop(m);
        }
        "MOC" => {
            m.push(tags::ORD_TYPE, "5");
        }
        "LOC" => {
            m.push(tags::ORD_TYPE, "B");
            price(m);
        }
        "MIT" => {
            m.push(tags::ORD_TYPE, "J");
            stop(m);
        }
        "LIT" => {
            m.push(tags::ORD_TYPE, "LT");
            price(m);
            stop(m);
        }
        "REL" | "PEG PRIM" => {
            m.push(tags::ORD_TYPE, "P");
            inst.0.push("R");
            offset(m);
            price(m);
        }
        "PEG MKT" => {
            m.push(tags::ORD_TYPE, "P");
            inst.0.push("P");
            offset(m);
            price(m);
        }
        "PEG MID" => {
            // IB: PegDifference must be 0 for midpoint pegs.
            m.push(tags::ORD_TYPE, "P").push(tags::PEG_DIFFERENCE, 0);
            inst.0.push("M");
            price(m);
        }
        "TRAIL" => {
            m.push(tags::ORD_TYPE, "P");
            inst.0.push("a");
            trailing(m, o);
        }
        "TRAIL LIMIT" => {
            m.push(tags::ORD_TYPE, "TSL");
            price(m);
            trailing(m, o);
            if let Some(off) = set(o.lmt_price_offset) {
                m.push_f64(tags::TRAIL_LIMIT_OFFSET, off);
            }
        }
        "MIDPRICE" => {
            m.push(tags::ORD_TYPE, "MIDPX");
            price(m);
        }
        "PASSV REL" => {
            m.push(tags::ORD_TYPE, "PSVR");
            price(m);
            offset(m);
        }
        "SNAP MKT" => {
            m.push(tags::ORD_TYPE, "SMKT");
            offset(m);
        }
        "SNAP MID" => {
            m.push(tags::ORD_TYPE, "SMID");
            offset(m);
        }
        "PEG BEST" => {
            m.push(tags::ORD_TYPE, "E2M");
            price(m);
            if let Some(v) = set_i(o.min_compete_size) {
                m.push(tags::MIN_COMPETE_SIZE, v);
            }
            match set(o.compete_against_best_offset) {
                Some(v) if v.is_infinite() => {
                    m.push(tags::COMPETE_AGAINST_BEST_OFFSET, "UpToMid");
                }
                Some(v) => {
                    m.push_f64(tags::COMPETE_AGAINST_BEST_OFFSET, v);
                }
                None => {}
            }
            if let Some(v) = set(o.mid_offset_at_whole) {
                m.push_f64(tags::EVEN_MID_OFFSET, v);
            }
            if let Some(v) = set(o.mid_offset_at_half) {
                m.push_f64(tags::ODD_MID_OFFSET, v);
            }
        }
        "PEG STK" => {
            m.push(tags::ORD_TYPE, "P");
            inst.0.push("s");
            for (tag, v) in [
                (tags::STOCK_REF_PRICE, o.stock_ref_price),
                (tags::STOCK_RANGE_LOWER, o.stock_range_lower),
                (tags::STOCK_RANGE_UPPER, o.stock_range_upper),
                (tags::DELTA, o.delta),
                (tags::STOP_PX, o.starting_price),
            ] {
                if let Some(v) = set(v) {
                    m.push_f64(tag, v);
                }
            }
            // The client acknowledges that IB does not reroute to exchanges
            // with cancellation fees (required by IB).
            m.push(tags::CHEAP_TO_REROUTE, 1);
        }
        "PEG BENCH" => {
            // Pegged to benchmark: 40=PB, 18=R, starting price in 99.
            m.push(tags::ORD_TYPE, "PB");
            inst.0.push("R");
            if let Some(p) = lmt {
                m.push_f64(tags::STOP_PX, p);
            }
            for (tag, v) in [
                (tags::BENCHMARK_REF_PRICE, o.stock_ref_price),
                (tags::STOCK_RANGE_LOWER, o.stock_range_lower),
                (tags::STOCK_RANGE_UPPER, o.stock_range_upper),
            ] {
                if let Some(v) = set(v) {
                    m.push_f64(tag, v);
                }
            }
            // The peg moves pegged_change_amount per reference_change_amount
            // (IB: DeltaNum / DeltaDenom, both or neither).
            if let (Some(num), Some(denom)) = (
                set(o.pegged_change_amount).filter(|v| *v != 0.0),
                set(o.reference_change_amount).filter(|v| *v != 0.0),
            ) {
                let num = if o.is_pegged_change_amount_decrease {
                    -num.abs()
                } else {
                    num
                };
                m.push_f64(tags::DELTA_NUM, num)
                    .push_f64(tags::DELTA_DENOM, denom);
            }
        }
        "VOL" => {
            let Some(vol) = set(o.volatility) else {
                return err("VOL order has no volatility; set order.volatility (percent)".into());
            };
            m.push(tags::ORD_TYPE, "2")
                .push_f64(tags::IMP_VOLATILITY, vol)
                .push(
                    tags::CONTINUOUS_UPDATE,
                    if o.continuous_update { 2 } else { 1 },
                );
            if let Some(t) = set_i(o.volatility_type) {
                m.push(tags::VOLAT_TYPE, t);
            }
            if let Some(r) = set_i(o.reference_price_type) {
                m.push(tags::UNDERLYING_REF_PRICE, r);
            }
            let hedge = match o.delta_neutral_order_type.as_str() {
                "" | "NONE" => "-1",
                "MKT" => "1",
                "LMT" => "2",
                "REL" => "E",
                other => {
                    return err(format!(
                        "VOL hedge order type {other:?} has no FIX value; use MKT, LMT, REL or none"
                    ));
                }
            };
            m.push(tags::HEDGING_TYPE, hedge);
            for (tag, v) in [
                (tags::STOCK_RANGE_LOWER, o.stock_range_lower),
                (tags::STOCK_RANGE_UPPER, o.stock_range_upper),
            ] {
                if let Some(v) = set(v) {
                    m.push_f64(tag, v);
                }
            }
        }
        other => {
            return err(format!(
                "order type {other:?} has no IB FIX mapping; use MKT, LMT, STP, STP LMT, MOC, LOC, MIT, LIT, REL, PEG MKT, PEG MID, PEG STK, PEG BEST, TRAIL, TRAIL LIMIT, MIDPRICE, PASSV REL, SNAP MKT, SNAP MID, PEG BENCH or VOL, or build the message by hand"
            ));
        }
    }
    Ok(())
}

/// IB FIX tag of a TWS algo parameter.
fn algo_tag(strategy: &str, name: &str) -> Option<u32> {
    Some(match name {
        "startTime" => tags::ALGO_START_TIME,
        "endTime" => tags::ALGO_END_TIME,
        "allowPastEndTime" => tags::ALGO_ALLOW_PAST_END_TIME,
        "noTakeLiq" => tags::ALGO_NO_TAKE_LIQ,
        "maxPctVol" => tags::PARTICIPATION_RATE,
        "pctVol" if strategy == "PctVol" => tags::ALGO_PCT_VOL,
        "pctVol" => tags::PARTICIPATION_RATE,
        "riskAversion" => tags::ALGO_RISK_AVERSION,
        "forceCompletion" => tags::ALGO_FORCE_COMPLETION,
        "displaySize" => tags::ALGO_DISPLAY_SIZE,
        "componentSize" => tags::ALGO_COMPONENT_SIZE,
        "timeBetweenOrders" => tags::ALGO_TIME_BETWEEN_ORDERS,
        "randomizeTime20" => tags::ALGO_RANDOMIZE_TIME,
        "randomizeSize55" => tags::ALGO_RANDOMIZE_SIZE,
        "catchUp" => tags::ALGO_CATCH_UP,
        "waitForFill" => tags::ALGO_WAIT_FOR_FILL,
        "adaptivePriority" => tags::ALGO_ADAPTIVE_PRIORITY,
        "optOutClosingAuction" => tags::ALGO_OPT_OUT_CLOSING_AUCTION,
        "includeBlockTrades" => tags::ALGO_INCLUDE_BLOCK_TRADES,
        _ => return None,
    })
}

fn add_algo(m: &mut Message, o: &Order, inst: &mut ExecInst) -> Result<(), MappingError> {
    let strategy = o.algo_strategy.as_str();
    inst.0.push("e");
    m.push(tags::TARGET_STRATEGY, strategy);
    for p in &o.algo_params {
        let Some(tag) = algo_tag(strategy, &p.tag) else {
            return err(format!(
                "algo parameter {:?} of {strategy} has no IB FIX tag; IB's FIX algos take startTime, endTime, allowPastEndTime, noTakeLiq, maxPctVol, pctVol, riskAversion, forceCompletion, displaySize, componentSize, timeBetweenOrders, randomizeTime20, randomizeSize55, catchUp, waitForFill, adaptivePriority, optOutClosingAuction and includeBlockTrades",
                p.tag
            ));
        };
        let value = match p.value.as_str() {
            "true" | "True" => "1".to_string(),
            "false" | "False" => "0".to_string(),
            v if tag == tags::ALGO_START_TIME || tag == tags::ALGO_END_TIME => {
                utc_time(&format!("algo parameter {}", p.tag), v)?
            }
            v => v.to_string(),
        };
        m.push(tag, value);
    }
    Ok(())
}

fn add_conditions(m: &mut Message, o: &Order) -> Result<(), MappingError> {
    if o.conditions.is_empty() {
        return Ok(());
    }
    m.push(tags::CONDITION_LIST_SIZE, o.conditions.len());
    if o.conditions_ignore_rth {
        m.push(tags::CONDITION_IGNORE_RTH, 1);
    }
    m.push(
        tags::COND_SUBMIT_CANCEL,
        if o.conditions_cancel_order { 1 } else { 0 },
    );
    let operand = |is_more: bool| if is_more { ">=" } else { "<=" };
    let last = o.conditions.len() - 1;
    for (i, c) in o.conditions.iter().enumerate() {
        let conjunction = match c {
            OrderCondition::Price { conjunction, .. }
            | OrderCondition::Time { conjunction, .. }
            | OrderCondition::Margin { conjunction, .. }
            | OrderCondition::Execution { conjunction, .. }
            | OrderCondition::Volume { conjunction, .. }
            | OrderCondition::PercentChange { conjunction, .. } => conjunction,
        };
        let binder = if i == last {
            "n"
        } else if conjunction == "o" {
            "o"
        } else {
            "a"
        };
        match c {
            OrderCondition::Price {
                is_more,
                price,
                con_id,
                exch,
                trigger_method,
                ..
            } => {
                m.push(tags::CONDITION_TYPE, 1)
                    .push(tags::CONDITION_CON_ID, con_id)
                    .push(tags::CONDITION_EXCHANGE, exch)
                    .push_f64(tags::CONDITION_PRICE, *price)
                    .push(tags::CONDITION_OPERAND, operand(*is_more));
                if *trigger_method != 0 {
                    m.push(tags::CONDITION_TRIGGER_METHOD, trigger_method);
                }
            }
            OrderCondition::Time { is_more, time, .. } => {
                m.push(tags::CONDITION_TYPE, 3)
                    .push(tags::CONDITION_OPERAND, operand(*is_more))
                    .push(tags::CONDITION_TIME, utc_time("time condition", time)?);
            }
            OrderCondition::Margin {
                is_more, percent, ..
            } => {
                m.push(tags::CONDITION_TYPE, 4)
                    .push(tags::CONDITION_OPERAND, operand(*is_more))
                    .push(tags::CONDITION_MARGIN, percent);
            }
            OrderCondition::Execution {
                sec_type,
                exch,
                symbol,
                ..
            } => {
                m.push(tags::CONDITION_TYPE, 5).push(
                    tags::CONDITION_EXECUTION_PATTERN,
                    format!(
                        "symbol={symbol}; exchange={exch}; securityType={};",
                        security_type(sec_type)
                    ),
                );
            }
            OrderCondition::Volume {
                is_more,
                volume,
                con_id,
                exch,
                ..
            } => {
                m.push(tags::CONDITION_TYPE, 6)
                    .push(tags::CONDITION_CON_ID, con_id)
                    .push(tags::CONDITION_EXCHANGE, exch)
                    .push(tags::CONDITION_OPERAND, operand(*is_more))
                    .push(tags::CONDITION_VOLUME, volume);
            }
            OrderCondition::PercentChange { .. } => {
                return err(
                    "percent change conditions have no IB FIX tags; IB's FIX conditions are price, time, margin, execution and volume".into(),
                );
            }
        }
        m.push(tags::CONDITION_LOGIC_BINDER, binder);
    }
    Ok(())
}

/// Adjustable stop (one barrier, tags 6257 to 6269).
fn add_barrier(m: &mut Message, o: &Order) -> Result<(), MappingError> {
    if o.adjusted_order_type.is_empty() {
        return Ok(());
    }
    let kind = match o.adjusted_order_type.as_str() {
        "STP" => "3",
        "STP LMT" => "4",
        "TRAIL" => "T",
        "TRAIL LIMIT" => "TSL",
        other => {
            return err(format!(
                "adjusted order type {other:?} has no IB FIX barrier type; use STP, STP LMT, TRAIL or TRAIL LIMIT"
            ));
        }
    };
    let Some(trigger) = set(o.trigger_price) else {
        return err("an adjustable stop needs order.trigger_price (the barrier price)".into());
    };
    m.push(tags::NO_BARRIERS, 1)
        .push_f64(tags::BARRIER_PRICE, trigger);
    let opt = |v: f64| set(v).map(|v| v.to_string()).unwrap_or_default();
    m.push(tags::BARRIER_STOP_PRICE, opt(o.adjusted_stop_price))
        .push(tags::BARRIER_TRAILING_AMT, opt(o.adjusted_trailing_amount))
        .push(tags::BARRIER_ORDER_TYPE, kind)
        .push(tags::BARRIER_LIMIT_PRICE, opt(o.adjusted_stop_limit_price));
    if let Some(unit) = set_i(o.adjustable_trailing_unit) {
        m.push(
            tags::BARRIER_TRAILING_AMT_UNIT,
            if unit == 1 { 100 } else { 0 },
        );
    }
    Ok(())
}

/// Short sale locate and exemption fields (US stocks).
fn add_short_sale(m: &mut Message, o: &Order) {
    if o.action != "SSHORT" {
        return;
    }
    match o.short_sale_slot {
        1 => {
            m.push(tags::SHORT_SALE_RULE, 1);
        }
        2 => {
            m.push(tags::SHORT_SALE_RULE, 2)
                .push(tags::LOCATE_REQD, "N");
            if !o.designated_location.is_empty() {
                m.push(tags::LOCATE_BROKER, &o.designated_location);
            }
        }
        _ => {}
    }
}

fn add_order_common(m: &mut Message, o: &Order) -> Result<(), MappingError> {
    let mut side_code = side(&o.action)?;
    if o.action == "SSHORT" && set_i(o.exempt_code).is_some_and(|e| e >= 0) {
        side_code = "6";
    }
    m.push(tags::SIDE, side_code);
    if side_code == "6" {
        m.push(tags::SHORT_SALE_EXEMPTION_REASON, o.exempt_code);
    }
    let cash = set(o.cash_qty).filter(|q| *q > 0.0);
    if let Some(q) = cash {
        m.push_f64(tags::CASH_ORDER_QTY, q);
    }
    if o.total_quantity > 0.0 || cash.is_none() {
        m.push_f64(tags::ORDER_QTY, o.total_quantity);
    }
    let mut inst = ExecInst::default();
    add_order_type(m, o, &mut inst)?;
    m.push(tags::TIME_IN_FORCE, tif(&o.tif)?);
    if !o.good_till_date.is_empty() {
        if o.good_till_date.len() == 8 && o.good_till_date.bytes().all(|b| b.is_ascii_digit()) {
            m.push(tags::EXPIRE_DATE, &o.good_till_date);
        } else {
            m.push(
                tags::EXPIRE_TIME,
                utc_time("order.good_till_date", &o.good_till_date)?,
            );
        }
    }
    if !o.good_after_time.is_empty() {
        m.push(
            tags::EFFECTIVE_TIME,
            utc_time("order.good_after_time", &o.good_after_time)?,
        );
    }
    if !o.account.is_empty() {
        m.push(tags::ACCOUNT, &o.account);
    }
    if o.hidden {
        m.push(tags::MAX_FLOOR, 0);
    } else if let Some(d) = set_i(o.display_size).filter(|d| *d > 0) {
        m.push(tags::MAX_FLOOR, d);
    }
    if let Some(q) = set_i(o.min_qty) {
        m.push(tags::MIN_QTY, q);
    }
    if let Some(d) = set(o.discretionary_amt).filter(|d| *d != 0.0) {
        m.push(tags::DISCRETION_INST, 0)
            .push_f64(tags::DISCRETION_OFFSET, d);
    }
    if !o.settling_firm.is_empty() {
        m.push(tags::CLEARING_FIRM, &o.settling_firm);
    }
    if !o.clearing_account.is_empty() {
        m.push(tags::CLEARING_ACCOUNT, &o.clearing_account);
    }
    if !o.open_close.is_empty() {
        m.push(tags::OPEN_CLOSE, &o.open_close);
    }
    m.push(tags::CUSTOMER_OR_FIRM, if o.origin == 1 { 1 } else { 0 });
    if !o.rule80_a.is_empty() {
        m.push(tags::RULE80A, &o.rule80_a);
    }
    if !o.oca_group.is_empty() {
        let group = if o.oca_group.starts_with("IBOCAGROUP") {
            o.oca_group.clone()
        } else {
            format!("IBOCAGROUP {}", o.oca_group)
        };
        m.push(tags::CL_ORD_LINK_ID, group);
        let method = match o.oca_type {
            1 => Some("CancelOnFillWBlock"),
            2 => Some("ReduceOnFillWBlock"),
            3 => Some("ReduceOnFillNonBlock"),
            _ => None,
        };
        if let Some(method) = method {
            m.push(tags::OCA_HANDLING_METHOD, method);
        }
    }
    if !o.order_ref.is_empty() {
        m.push(tags::ORDER_REFERENCE, &o.order_ref);
    }
    if o.trigger_method != 0 && o.trigger_method != UNSET_INTEGER {
        m.push(tags::TRIGGER_METHOD, o.trigger_method);
    }
    if !o.algo_strategy.is_empty() {
        // IB runs algos during regular trading hours only and needs 6205=1.
        add_algo(m, o, &mut inst)?;
        m.push(tags::FORCE_ONLY_RTH, 1);
    } else if !o.outside_rth {
        m.push(tags::FORCE_ONLY_RTH, 1);
    }
    if o.allow_pre_open {
        m.push(tags::ALLOW_PRE_OPEN_FOR_RTH, 1);
    }
    if o.all_or_none {
        inst.0.push("G");
    }
    if !inst.0.is_empty() {
        m.push(tags::EXEC_INST, inst.0.join(" "));
    }
    if o.what_if {
        m.push(tags::WHAT_IF, 1);
    }
    if o.not_held {
        m.push(tags::NOT_HELD, 1);
    }
    if let Some(s) = set_i(o.post_to_ats).filter(|s| *s > 0) {
        m.push(tags::POST_TO_ATS, s);
    }
    if o.sweep_to_fill {
        m.push(tags::SWEEP_TO_FILL, 1);
    }
    if o.imbalance_only {
        m.push(tags::IMBALANCE_ONLY, 1);
    }
    if o.include_overnight {
        m.push(tags::INCLUDE_OVERNIGHT, 1);
    }
    if let Some(q) = set_i(o.min_trade_qty) {
        m.push(tags::MIN_TRADE_QTY, q);
    }
    if o.use_price_mgmt_algo == Some(true) {
        m.push(tags::PRICE_MGMT_ALGO, 1);
    }
    match set_i(o.manual_order_indicator) {
        Some(1) => {
            m.push(tags::MANUAL_ORDER_INDICATOR, "Y");
        }
        Some(0) => {
            m.push(tags::MANUAL_ORDER_INDICATOR, "N");
        }
        _ => {}
    }
    if !o.ext_operator.is_empty() {
        m.push(tags::EXT_OPERATOR, &o.ext_operator);
    }
    for (tag, v) in [
        (tags::MIFID2_DECISION_MAKER, &o.mifid2_decision_maker),
        (tags::MIFID2_DECISION_ALGO, &o.mifid2_decision_algo),
        (tags::MIFID2_EXECUTION_TRADER, &o.mifid2_execution_trader),
        (tags::MIFID2_EXECUTION_ALGO, &o.mifid2_execution_algo),
        (tags::CUST_ACCOUNT, &o.customer_account),
    ] {
        if !v.is_empty() {
            m.push(tag, v);
        }
    }
    if o.professional_customer {
        m.push(tags::PROFESSIONAL_CUSTOMER, 1);
    }
    // BOX price improvement auction strategy (match, improve, transparent).
    if (1..=3).contains(&o.auction_strategy) {
        m.push(tags::DISCRETIONARY_TYPE, o.auction_strategy);
    }
    match o.hedge_type.as_str() {
        "" => {}
        // Pair trade child: IB's HedgeType 3 with the ratio in 6666.
        "P" => {
            m.push(tags::HEDGE_TYPE, 3);
            if !o.hedge_param.is_empty() {
                m.push(tags::HEDGE_RATIO, &o.hedge_param);
            }
        }
        other => {
            return err(format!(
                "hedge type {other:?} has no IB FIX mapping; IB's FIX engine supports pair trades (hedge_type \"P\"); send delta hedges of volatility orders as a separate order with OrderExtras::delta_hedge"
            ));
        }
    }
    add_short_sale(m, o);
    add_conditions(m, o)?;
    add_barrier(m, o)
}

fn require_account(o: &Order, what: &str) -> Result<(), MappingError> {
    if o.account.is_empty() {
        return err(format!(
            "{what} has no account; IB requires Account (tag 1) on orders, so set order.account to the IBKR account id"
        ));
    }
    Ok(())
}

fn transact_time(m: &mut Message) {
    m.push(
        tags::TRANSACT_TIME,
        utc_timestamp(std::time::SystemTime::now()),
    );
}

/// IB order tags that have no field in the TWS [`Order`].
#[derive(Clone, Debug, Default)]
pub struct OrderExtras<'a> {
    /// ClOrdID of the parent order (6107): bracket, auto-stop, pair trade
    /// and delta-hedge children wait for the parent to fill.
    pub parent_cl_ord_id: Option<&'a str>,
    /// Where the decision maker or algo is (142); IB requires it on futures.
    pub sender_location_id: Option<&'a str>,
    /// Pre-trade allocation (78/79/80): `(sub-account, quantity)` pairs; the
    /// order's account is the master account.
    pub allocations: &'a [(&'a str, f64)],
    /// Apply the allocations exactly, without IB's close-first logic (6806=1).
    pub disable_fa_close_first: bool,
    /// Reject the whole allocation order when an account fails the credit
    /// check (6240=1).
    pub strict_per_account_allocation: bool,
    /// This order hedges the volatility order `parent_cl_ord_id` (6460=1).
    pub delta_hedge: bool,
    /// Currency conversion, not FX trading (6707=1, with CASH on IDEALPRO).
    pub fx_conversion: bool,
    /// Bond limit and stop prices are net of platform fees (6882=1).
    pub use_net_price: bool,
    /// Avoid dark pools on SMART routing (6595=1).
    pub no_dark_pools: bool,
    /// Stage the order in the TWS blotter instead of transmitting it (6406=1).
    pub staged: bool,
}

fn add_extras(m: &mut Message, x: &OrderExtras<'_>) {
    if let Some(p) = x.parent_cl_ord_id {
        m.push(tags::PARENT_CLIENT_ID, p);
    }
    if let Some(l) = x.sender_location_id {
        m.push(tags::SENDER_LOCATION_ID, l);
    }
    if !x.allocations.is_empty() {
        m.push(tags::NO_ALLOCS, x.allocations.len());
        for (account, qty) in x.allocations {
            m.push(tags::ALLOC_ACCOUNT, account)
                .push_f64(tags::ALLOC_SHARES, *qty);
        }
    }
    for (flag, tag) in [
        (x.disable_fa_close_first, tags::DISABLE_FA_CLOSE_FIRST),
        (x.strict_per_account_allocation, tags::STRICT_PER_ACCT_ALLOC),
        (x.delta_hedge, tags::IS_DELTA_HEDGE),
        (x.fx_conversion, tags::IS_FX_CONV),
        (x.use_net_price, tags::USE_NET_PRICE),
        (x.no_dark_pools, tags::NO_DARK_POOLS),
        (x.staged, tags::STAGED_ORDER),
    ] {
        if flag {
            m.push(tag, 1);
        }
    }
}

/// NewOrderSingle (35=D). Use [`new_order_single_with`] for the IB tags
/// that have no TWS `Order` field (parent order, futures sender location,
/// pre-trade allocations, ...).
pub fn new_order_single(
    cl_ord_id: &str,
    contract: &Contract,
    order: &Order,
) -> Result<Message, MappingError> {
    new_order_single_with(cl_ord_id, contract, order, &OrderExtras::default())
}

/// NewOrderSingle (35=D) with [`OrderExtras`]. Futures and futures options
/// need ManualOrderIndicator (`order.manual_order_indicator`), ExtOperator
/// (`order.ext_operator`) and SenderLocationID (`extras.sender_location_id`).
pub fn new_order_single_with(
    cl_ord_id: &str,
    contract: &Contract,
    order: &Order,
    extras: &OrderExtras<'_>,
) -> Result<Message, MappingError> {
    require_account(order, "the order")?;
    check_instrument(contract)?;
    if matches!(contract.sec_type.as_str(), "FUT" | "FOP")
        && (set_i(order.manual_order_indicator).is_none()
            || order.ext_operator.is_empty()
            || extras.sender_location_id.is_none_or(str::is_empty))
    {
        return err(format!(
            "futures order for {} lacks a field IB requires on futures: set order.manual_order_indicator (1 manual, 0 automated), order.ext_operator and OrderExtras::sender_location_id",
            contract.symbol
        ));
    }
    let mut m = Message::new(msg_type::NEW_ORDER_SINGLE);
    m.push(tags::CL_ORD_ID, cl_ord_id).push(tags::HANDL_INST, 2);
    add_instrument(&mut m, contract);
    add_order_common(&mut m, order)?;
    if order.order_type == "PEG BEST" && contract.exchange == "IBKRATS" {
        // IB: peg-to-best orders for the IBKR ATS need 8032=1.
        m.push(tags::PEG_BEST_ATS, 1);
    }
    add_extras(&mut m, extras);
    transact_time(&mut m);
    Ok(m)
}

/// OptionExerciseType (6809) of an [`option_exercise`] request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExerciseType {
    Exercise,
    /// Lapse: IB accepts only irrevocable lapses, on the last trading day.
    Lapse,
}

/// Option exercise or lapse request: a NewOrderSingle (35=D) with
/// OptionExerciseType (6809) and RevocableExercise (8012). IB enables this
/// per session; `order` carries the quantity, side, order type and price
/// that IB's FIX Engineering team specifies for the session.
pub fn option_exercise(
    cl_ord_id: &str,
    contract: &Contract,
    order: &Order,
    exercise: ExerciseType,
    revocable: bool,
) -> Result<Message, MappingError> {
    if !matches!(contract.sec_type.as_str(), "OPT" | "FOP") {
        return err(format!(
            "option_exercise got secType {:?}; only options (OPT, FOP) can be exercised",
            contract.sec_type
        ));
    }
    if exercise == ExerciseType::Lapse && revocable {
        return err("IB accepts only irrevocable lapses; pass revocable = false".into());
    }
    let mut m = new_order_single(cl_ord_id, contract, order)?;
    m.push(
        tags::OPTION_EXERCISE_TYPE,
        match exercise {
            ExerciseType::Exercise => 1,
            ExerciseType::Lapse => 2,
        },
    )
    .push(tags::REVOCABLE_EXERCISE, if revocable { 1 } else { 0 });
    Ok(m)
}

/// OrderCancelReplaceRequest (35=G). IB modifies quantity, price, stop price,
/// time in force (DAY to GTC and back), expiry, order reference and barriers;
/// `order.deactivate` pauses (6521=1) or resumes (6521=0) the order. Any
/// optional tag sent on the first modify must be sent on later ones.
pub fn cancel_replace(
    orig_cl_ord_id: &str,
    cl_ord_id: &str,
    contract: &Contract,
    order: &Order,
) -> Result<Message, MappingError> {
    require_account(order, "the modify")?;
    let mut m = Message::new(msg_type::ORDER_CANCEL_REPLACE_REQUEST);
    m.push(tags::ORIG_CL_ORD_ID, orig_cl_ord_id)
        .push(tags::CL_ORD_ID, cl_ord_id)
        .push(tags::HANDL_INST, 2);
    add_instrument(&mut m, contract);
    add_order_common(&mut m, order)?;
    m.push(tags::DEACTIVATE, if order.deactivate { 1 } else { 0 });
    transact_time(&mut m);
    Ok(m)
}

/// OrderCancelRequest (35=F): `action` and `quantity` must match the order.
pub fn cancel_request(
    orig_cl_ord_id: &str,
    cl_ord_id: &str,
    account: &str,
    contract: &Contract,
    action: &str,
    quantity: f64,
) -> Result<Message, MappingError> {
    if account.is_empty() {
        return err(
            "cancel_request got an empty account; IB requires Account (tag 1) on cancels".into(),
        );
    }
    let mut m = Message::new(msg_type::ORDER_CANCEL_REQUEST);
    m.push(tags::ORIG_CL_ORD_ID, orig_cl_ord_id)
        .push(tags::CL_ORD_ID, cl_ord_id)
        .push(tags::ACCOUNT, account);
    add_instrument(&mut m, contract);
    m.push(tags::SIDE, side(action)?);
    m.push_f64(tags::ORDER_QTY, quantity);
    transact_time(&mut m);
    Ok(m)
}

/// OrderStatusRequest (35=H): IB identifies the order by Account (1) and
/// ClOrdID (11). `cl_ord_id` `"*"` asks for every open order; IB then sends
/// one ExecutionReport per order and ends with one whose ClOrdID is `"*"`.
pub fn order_status_request(account: &str, cl_ord_id: &str) -> Message {
    let mut m = Message::new(msg_type::ORDER_STATUS_REQUEST);
    m.push(tags::ACCOUNT, account)
        .push(tags::CL_ORD_ID, cl_ord_id);
    m
}

/// One leg of a [`new_order_multileg`] order.
#[derive(Clone, Debug)]
pub struct Leg<'a> {
    pub contract: &'a Contract,
    pub ratio: f64,
    /// BUY, SELL or SSHORT (stock legs).
    pub action: &'a str,
    /// LegPositionEffect: "O" (open, the default when empty) or "C".
    pub open_close: &'a str,
}

/// LegCFICode (608) of a leg contract.
fn cfi_code(c: &Contract) -> Result<&'static str, MappingError> {
    let call = matches!(c.right.as_str(), "C" | "CALL");
    Ok(match (c.sec_type.as_str(), call) {
        ("STK", _) => "ESXXXX",
        ("OPT", true) => "OCXXXX",
        ("OPT", false) => "OPXXXX",
        ("FUT", _) => "FXXXXX",
        ("FOP", true) => "FCXXXX",
        ("FOP", false) => "FPXXXX",
        ("WAR", _) => "WXXXXX",
        ("CASH", _) => "MXXXXX",
        (other, _) => {
            return err(format!(
                "leg {} has secType {other:?}; IB's multileg orders take STK, OPT, FUT, FOP, WAR and CASH legs",
                c.symbol
            ));
        }
    })
}

/// NewOrderMultileg (35=AB) with IB's FIX 4.3 leg block. `contract` is the
/// combo (symbol, exchange, currency, primary exchange); a stock leg must be
/// the last leg. IB reads the price as debit (positive) or credit
/// (negative) and recommends sending combos as BUY with each leg's own side.
/// "NonGuaranteed"=1 in `order.smart_combo_routing_params` sends 6248=1.
pub fn new_order_multileg(
    cl_ord_id: &str,
    contract: &Contract,
    legs: &[Leg<'_>],
    order: &Order,
) -> Result<Message, MappingError> {
    if legs.len() < 2 {
        return err(format!(
            "new_order_multileg got {} legs; pass at least two Leg values",
            legs.len()
        ));
    }
    if let Some(i) = legs.iter().position(|l| l.contract.sec_type == "STK")
        && i != legs.len() - 1
    {
        return err(format!(
            "leg {} is a stock leg but not the last one; IB needs the stock leg last",
            i + 1
        ));
    }
    if !matches!(order.tif.as_str(), "" | "DAY" | "GTC" | "GTD") {
        return err(format!(
            "multileg time in force is {:?}; IB accepts DAY, GTC and GTD on combos",
            order.tif
        ));
    }
    let exchange = if contract.primary_exchange.is_empty() {
        &contract.exchange
    } else {
        &contract.primary_exchange
    };
    if exchange.is_empty() || contract.currency.is_empty() {
        return err(
            "the combo contract needs exchange (or primary_exchange) and currency; IB requires SecurityExchange (207) and Currency (15) on multileg orders".into(),
        );
    }
    let mut m = Message::new(msg_type::NEW_ORDER_MULTILEG);
    m.push(tags::CL_ORD_ID, cl_ord_id)
        .push(tags::HANDL_INST, 2)
        .push(tags::SYMBOL, &contract.symbol)
        .push(tags::SECURITY_TYPE, "MULTILEG")
        .push(tags::SECURITY_EXCHANGE, exchange)
        .push(tags::CURRENCY, &contract.currency);
    if !contract.exchange.is_empty() {
        m.push(tags::EX_DESTINATION, &contract.exchange);
    }
    if order
        .smart_combo_routing_params
        .iter()
        .any(|p| p.tag == "NonGuaranteed" && p.value == "1")
    {
        m.push(tags::SMART_COMBO_GUARANTEE, 1);
    }
    m.push(tags::NO_LEGS, legs.len());
    for (i, leg) in legs.iter().enumerate() {
        let c = leg.contract;
        m.push(tags::LEG_SYMBOL, &c.symbol)
            .push(tags::LEG_SECURITY_TYPE, &c.sec_type)
            .push(tags::LEG_CFI_CODE, cfi_code(c)?);
        let ltd = &c.last_trade_date_or_contract_month;
        if ltd.len() >= 8 {
            m.push(tags::LEG_MATURITY_DATE, &ltd[..8]);
        } else if ltd.len() == 6 {
            m.push(tags::LEG_MATURITY_MONTH_YEAR, ltd);
        }
        if let Some(k) = set(c.strike).filter(|k| *k != 0.0) {
            m.push_f64(tags::LEG_STRIKE_PRICE, k);
        }
        if !c.multiplier.is_empty() {
            m.push(tags::LEG_CONTRACT_MULTIPLIER, &c.multiplier);
        }
        if !c.exchange.is_empty() {
            m.push(tags::LEG_SECURITY_EXCHANGE, &c.exchange);
        }
        let effect = match leg.open_close {
            "" | "O" => "O",
            "C" => "C",
            other => {
                return err(format!(
                    "leg {} open_close is {other:?}; use \"O\" or \"C\"",
                    i + 1
                ));
            }
        };
        m.push(tags::LEG_POSITION_EFFECT, effect)
            .push_f64(tags::LEG_RATIO_QTY, leg.ratio)
            .push(tags::LEG_SIDE, side(leg.action)?)
            .push(tags::LEG_REF_ID, i + 1);
    }
    add_order_common(&mut m, order)?;
    transact_time(&mut m);
    Ok(m)
}

/// MultilegCancelReplace (35=AC): IB changes only quantity and price of a
/// combo order; the leg block is not sent.
pub fn multileg_cancel_replace(
    orig_cl_ord_id: &str,
    cl_ord_id: &str,
    contract: &Contract,
    order: &Order,
) -> Result<Message, MappingError> {
    require_account(order, "the multileg modify")?;
    let mut m = Message::new(msg_type::MULTILEG_CANCEL_REPLACE);
    m.push(tags::ACCOUNT, &order.account)
        .push(tags::HANDL_INST, 2)
        .push(tags::ORIG_CL_ORD_ID, orig_cl_ord_id)
        .push(tags::CL_ORD_ID, cl_ord_id)
        .push(tags::SYMBOL, &contract.symbol)
        .push(tags::SIDE, side(&order.action)?)
        .push_f64(tags::ORDER_QTY, order.total_quantity);
    let mut inst = ExecInst::default();
    add_order_type(&mut m, order, &mut inst)?;
    m.push(tags::TIME_IN_FORCE, tif(&order.tif)?);
    if !contract.exchange.is_empty() {
        m.push(tags::EX_DESTINATION, &contract.exchange);
    }
    if !contract.currency.is_empty() {
        m.push(tags::CURRENCY, &contract.currency);
    }
    if !contract.primary_exchange.is_empty() {
        m.push(tags::SECURITY_EXCHANGE, &contract.primary_exchange);
    }
    m.push(
        tags::CUSTOMER_OR_FIRM,
        if order.origin == 1 { 1 } else { 0 },
    );
    if !order.order_ref.is_empty() {
        m.push(tags::ORDER_REFERENCE, &order.order_ref);
    }
    transact_time(&mut m);
    Ok(m)
}

/// AllocTransType (71) of an [`allocation_instruction`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocTransType {
    New,
    /// Replace the allocation `ref_alloc_id`.
    Replace,
    /// Cancel the allocation `ref_alloc_id`.
    Cancel,
}

/// Post-trade AllocationInstruction (35=J) between clearing brokers:
/// `quantity` at `avg_px` (the average of all fills in the symbol and side)
/// on `trade_date` (YYYYMMDD) split over `(clearing broker, quantity)`
/// pairs. Replace and cancel need `ref_alloc_id` and a new `alloc_id`.
#[allow(clippy::too_many_arguments)]
pub fn allocation_instruction(
    alloc_id: &str,
    trans_type: AllocTransType,
    ref_alloc_id: Option<&str>,
    account: &str,
    contract: &Contract,
    action: &str,
    quantity: f64,
    avg_px: f64,
    trade_date: &str,
    allocations: &[(&str, f64)],
) -> Result<Message, MappingError> {
    let code = match trans_type {
        AllocTransType::New => 0,
        AllocTransType::Replace => 1,
        AllocTransType::Cancel => 2,
    };
    if trans_type != AllocTransType::New && ref_alloc_id.is_none_or(str::is_empty) {
        return err(format!(
            "{trans_type:?} allocation {alloc_id} has no ref_alloc_id; pass the AllocID of the allocation it changes"
        ));
    }
    if trans_type != AllocTransType::Cancel && allocations.is_empty() {
        return err(format!(
            "allocation {alloc_id} has no (clearing broker, quantity) pairs"
        ));
    }
    if contract.currency.is_empty() {
        return err(
            "allocation contract has no currency; IB requires Currency (15) on allocations".into(),
        );
    }
    let mut m = Message::new(msg_type::ALLOCATION_INSTRUCTION);
    m.push(tags::ALLOC_ID, alloc_id)
        .push(tags::ALLOC_TRANS_TYPE, code);
    if let Some(r) = ref_alloc_id {
        m.push(tags::REF_ALLOC_ID, r);
    }
    if !account.is_empty() {
        m.push(tags::ACCOUNT, account);
    }
    m.push(tags::SIDE, side(action)?);
    add_instrument(&mut m, contract);
    m.push_f64(tags::SHARES, quantity)
        .push_f64(tags::AVG_PX, avg_px)
        .push(tags::TRADE_DATE, trade_date);
    if trans_type != AllocTransType::Cancel {
        m.push(tags::NO_ALLOCS, allocations.len());
        for (broker, qty) in allocations {
            m.push(tags::ALLOC_ACCOUNT, broker)
                .push_f64(tags::ALLOC_SHARES, *qty)
                .push_f64(tags::ALLOC_AVG_PX, avg_px);
        }
    }
    transact_time(&mut m);
    Ok(m)
}

fn text(m: &Message, tag: u32) -> String {
    m.get(tag).unwrap_or("").to_string()
}

fn num(m: &Message, tag: u32) -> f64 {
    m.get(tag).and_then(|v| v.parse().ok()).unwrap_or(0.0)
}

fn opt_num(m: &Message, tag: u32) -> Option<f64> {
    m.get(tag).and_then(|v| v.parse().ok())
}

/// Typed view of an ExecutionReport (35=8).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExecutionReport {
    pub order_id: String,
    pub cl_ord_id: String,
    pub orig_cl_ord_id: String,
    pub exec_id: String,
    /// 0 new, 1 bust (cancel), 2 restatement (correct), 3 status.
    pub exec_trans_type: String,
    /// ExecID of the busted or restated execution.
    pub exec_ref_id: String,
    pub exec_type: String,
    pub ord_status: String,
    pub ord_rej_reason: String,
    pub exec_restatement_reason: String,
    pub symbol: String,
    pub local_symbol: String,
    pub security_type: String,
    pub side: String,
    pub order_qty: f64,
    pub leaves_qty: f64,
    pub cum_qty: f64,
    pub avg_px: f64,
    pub last_shares: f64,
    pub last_px: f64,
    pub last_mkt: String,
    pub price: f64,
    pub account: String,
    pub transact_time: String,
    pub text: String,
    /// 1 single security, 2 leg of a multileg order, 3 the multileg order.
    pub multi_leg_reporting_type: String,
    pub leg_ref_id: String,
    /// IB's `leg:legs` (6013), `-1` for the combo itself.
    pub combo_leg_info: String,
    /// "SLAL" while no shares are located for a short sale (6236).
    pub short_sell_availability: String,
    /// IB liquidation trade (6959=1).
    pub liquidation: bool,
    /// OP, DRIP or FRAC (8093).
    pub orphan_trade_type: String,
    pub clearing_firm: String,
    pub clearing_account: String,
    /// Settlement date of FX trades (64), YYYYMMDD.
    pub settle_date: String,
    /// "Y" once a contingent order is working (636).
    pub working_indicator: String,
    /// The message may repeat one sent under another sequence number (97).
    pub poss_resend: bool,
    /// 1 added, 2 removed, 3 routed out (851, when enabled for the session).
    pub last_liquidity_ind: String,
    /// Execution id of the venue (6568).
    pub exchange_exec_id: String,
    /// When a GTC order expires at IB (6596).
    pub gtc_expire_time: String,
    /// Exchanges where the order takes part in a BOX PIP auction (6273).
    pub pip_exchanges: String,
    /// 1 exercise, 2 lapse, 100 assigned, 101 auto-exercised, 102 expired (6809).
    pub option_exercise_type: String,
    pub what_if: WhatIf,
}

/// Margin and commission preview of a what-if (6091=1) order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WhatIf {
    pub init_margin: Option<f64>,
    pub maint_margin: Option<f64>,
    pub equity_with_loan: Option<f64>,
    pub init_margin_before: Option<f64>,
    pub maint_margin_before: Option<f64>,
    pub equity_with_loan_before: Option<f64>,
    pub margin_currency: String,
    pub commission: Option<f64>,
    pub min_commission: Option<f64>,
    pub max_commission: Option<f64>,
    pub commission_currency: String,
    pub max_order_size: Option<f64>,
    pub market_price: Option<f64>,
    pub warning_text: String,
}

impl ExecutionReport {
    pub fn from_message(m: &Message) -> Option<Self> {
        if m.msg_type != msg_type::EXECUTION_REPORT {
            return None;
        }
        Some(Self {
            order_id: text(m, tags::ORDER_ID),
            cl_ord_id: text(m, tags::CL_ORD_ID),
            orig_cl_ord_id: text(m, tags::ORIG_CL_ORD_ID),
            exec_id: text(m, tags::EXEC_ID),
            exec_trans_type: text(m, tags::EXEC_TRANS_TYPE),
            exec_ref_id: text(m, tags::EXEC_REF_ID),
            exec_type: text(m, tags::EXEC_TYPE),
            ord_status: text(m, tags::ORD_STATUS),
            ord_rej_reason: text(m, tags::ORD_REJ_REASON),
            exec_restatement_reason: text(m, tags::EXEC_RESTATEMENT_REASON),
            symbol: text(m, tags::SYMBOL),
            local_symbol: text(m, tags::IB_LOCAL_SYMBOL),
            security_type: text(m, tags::SECURITY_TYPE),
            side: text(m, tags::SIDE),
            order_qty: num(m, tags::ORDER_QTY),
            leaves_qty: num(m, tags::LEAVES_QTY),
            cum_qty: num(m, tags::CUM_QTY),
            avg_px: num(m, tags::AVG_PX),
            last_shares: num(m, tags::LAST_SHARES),
            last_px: num(m, tags::LAST_PX),
            last_mkt: text(m, tags::LAST_MKT),
            price: num(m, tags::PRICE),
            account: text(m, tags::ACCOUNT),
            transact_time: text(m, tags::TRANSACT_TIME),
            text: text(m, tags::TEXT),
            multi_leg_reporting_type: text(m, tags::MULTI_LEG_REPORTING_TYPE),
            leg_ref_id: text(m, tags::LEG_REF_ID),
            combo_leg_info: text(m, tags::COMBO_LEG_INFORMATION),
            short_sell_availability: text(m, tags::SHORT_SELL_AVAILABILITY),
            liquidation: m.get(tags::LIQUIDATION) == Some("1"),
            orphan_trade_type: text(m, tags::ORPHAN_TRADE_TYPE),
            clearing_firm: text(m, tags::CLEARING_FIRM),
            clearing_account: text(m, tags::CLEARING_ACCOUNT),
            settle_date: text(m, tags::SETTLE_DATE),
            working_indicator: text(m, tags::WORKING_INDICATOR),
            poss_resend: m.get(tags::POSS_RESEND) == Some("Y"),
            last_liquidity_ind: text(m, tags::LAST_LIQUIDITY_IND),
            exchange_exec_id: text(m, tags::EXCHANGE_EXEC_ID),
            gtc_expire_time: text(m, tags::GTC_EXPIRE_TIME),
            pip_exchanges: text(m, tags::PIP_EXCHANGES),
            option_exercise_type: text(m, tags::OPTION_EXERCISE_TYPE),
            what_if: WhatIf {
                init_margin: opt_num(m, tags::INIT_MARGIN),
                maint_margin: opt_num(m, tags::MAINT_MARGIN),
                equity_with_loan: opt_num(m, tags::EQUITY_WITH_LOAN),
                init_margin_before: opt_num(m, tags::INIT_MARGIN_BEFORE),
                maint_margin_before: opt_num(m, tags::MAINT_MARGIN_BEFORE),
                equity_with_loan_before: opt_num(m, tags::EQUITY_WITH_LOAN_BEFORE),
                margin_currency: text(m, tags::MARGIN_CURRENCY),
                commission: opt_num(m, tags::COMMISSIONS),
                min_commission: opt_num(m, tags::MIN_COMMISSIONS),
                max_commission: opt_num(m, tags::MAX_COMMISSIONS),
                commission_currency: text(m, tags::COMMISSION_CURRENCY),
                max_order_size: opt_num(m, tags::MAX_ORDER_SIZE),
                market_price: opt_num(m, tags::MARKET_PRICE),
                warning_text: text(m, tags::WARNING_TEXT),
            },
        })
    }

    /// TWS-style status text for OrdStatus (39).
    pub fn status_name(&self) -> &'static str {
        match self.ord_status.as_str() {
            "0" => "Submitted",
            "1" => "PartiallyFilled",
            "2" => "Filled",
            "3" => "DoneForDay",
            "4" => "Cancelled",
            "5" => "Replaced",
            "6" => "PendingCancel",
            "8" => "Rejected",
            "A" => "PendingNew",
            "C" => "Expired",
            "E" => "PendingReplace",
            _ => "Unknown",
        }
    }

    /// True for the terminator of an OrderStatusRequest with ClOrdID `"*"`.
    pub fn is_status_end(&self) -> bool {
        self.cl_ord_id == "*"
    }
}

/// Typed view of an OrderCancelReject (35=9).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OrderCancelReject {
    pub order_id: String,
    pub cl_ord_id: String,
    pub orig_cl_ord_id: String,
    pub ord_status: String,
    /// 0 too late, 1 unknown order, 2 broker option, 3 already pending.
    pub cxl_rej_reason: String,
    /// 1 cancel request, 2 cancel/replace request.
    pub cxl_rej_response_to: String,
    pub text: String,
}

impl OrderCancelReject {
    pub fn from_message(m: &Message) -> Option<Self> {
        (m.msg_type == msg_type::ORDER_CANCEL_REJECT).then(|| Self {
            order_id: text(m, tags::ORDER_ID),
            cl_ord_id: text(m, tags::CL_ORD_ID),
            orig_cl_ord_id: text(m, tags::ORIG_CL_ORD_ID),
            ord_status: text(m, tags::ORD_STATUS),
            cxl_rej_reason: text(m, tags::CXL_REJ_REASON),
            cxl_rej_response_to: text(m, tags::CXL_REJ_RESPONSE_TO),
            text: text(m, tags::TEXT),
        })
    }
}

/// Typed view of a News/Bulletin message (35=B).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bulletin {
    pub headline: String,
    pub daily_new_id: String,
    /// 0 normal, 1 flash, 2 background.
    pub urgency: String,
    pub exchange: String,
}

impl Bulletin {
    pub fn from_message(m: &Message) -> Option<Self> {
        (m.msg_type == msg_type::NEWS).then(|| Self {
            headline: text(m, tags::HEADLINE),
            daily_new_id: text(m, tags::DAILY_NEW_ID),
            urgency: text(m, tags::URGENCY),
            exchange: text(m, tags::SECURITY_EXCHANGE),
        })
    }
}

/// Typed view of an AllocationAck (35=P).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AllocationAck {
    pub alloc_id: String,
    pub trade_date: String,
    /// 0 accepted, 1 rejected, 3 received.
    pub alloc_status: String,
    pub reject_code: String,
    pub text: String,
}

impl AllocationAck {
    pub fn from_message(m: &Message) -> Option<Self> {
        (m.msg_type == msg_type::ALLOCATION_ACK).then(|| Self {
            alloc_id: text(m, tags::ALLOC_ID),
            trade_date: text(m, tags::TRADE_DATE),
            alloc_status: text(m, tags::ALLOC_STATUS),
            reject_code: text(m, tags::ALLOC_REJ_CODE),
            text: text(m, tags::TEXT),
        })
    }

    pub fn status_name(&self) -> &'static str {
        match self.alloc_status.as_str() {
            "0" => "Accepted",
            "1" => "Rejected",
            "3" => "Received",
            _ => "Unknown",
        }
    }
}
