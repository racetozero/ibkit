//! Encoders for every client request of the TWS API text protocol.
//!
//! Each function clears the writer, writes the complete message and leaves the
//! frame ready for [`FieldWriter::finish`]. Layouts and version gates follow
//! IB's official client for server versions 157 through 200. Gates below 157
//! are always satisfied, because [`crate::server_versions::MIN_CLIENT_VER`] is
//! 157, so they are not repeated here.

use crate::consts::UNSET_DOUBLE;
use crate::message::outgoing as out;
use crate::server_versions as sv;
use crate::types::*;
use crate::wire::FieldWriter;

/// A request that the connected server cannot accept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "{feature} needs server version {min_version} or later, but the connected server has version {server_version}; upgrade TWS or IB Gateway, or do not use {feature}"
)]
pub struct Unsupported {
    pub feature: &'static str,
    pub min_version: i32,
    pub server_version: i32,
}

fn require(
    server_version: i32,
    min_version: i32,
    feature: &'static str,
) -> Result<(), Unsupported> {
    if server_version < min_version {
        Err(Unsupported {
            feature,
            min_version,
            server_version,
        })
    } else {
        Ok(())
    }
}

pub(crate) fn is_peg_bench(order_type: &str) -> bool {
    matches!(order_type, "PEG BENCH" | "PEGBENCH")
}
fn is_peg_best(order_type: &str) -> bool {
    matches!(order_type, "PEG BEST" | "PEGBEST")
}
fn is_peg_mid(order_type: &str) -> bool {
    matches!(order_type, "PEG MID" | "PEGMID")
}

/// conId .. tradingClass, the contract block shared by most requests.
fn contract_full(w: &mut FieldWriter, c: &Contract) {
    w.int(c.con_id);
    w.str(&c.symbol);
    w.str(&c.sec_type);
    w.str(&c.last_trade_date_or_contract_month);
    w.f64_max(c.strike);
    w.str(&c.right);
    w.str(&c.multiplier);
    w.str(&c.exchange);
    w.str(&c.primary_exchange);
    w.str(&c.currency);
    w.str(&c.local_symbol);
    w.str(&c.trading_class);
}

fn combo_legs_short(w: &mut FieldWriter, c: &Contract) {
    if c.sec_type == "BAG" {
        w.int(c.combo_legs.len());
        for leg in &c.combo_legs {
            w.int(leg.con_id);
            w.int(leg.ratio);
            w.str(&leg.action);
            w.str(&leg.exchange);
        }
    }
}

fn delta_neutral(w: &mut FieldWriter, c: &Contract) {
    match &c.delta_neutral_contract {
        Some(d) => {
            w.bool(true);
            w.int(d.con_id);
            w.f64(d.delta);
            w.f64(d.price);
        }
        None => w.bool(false),
    }
}

fn header(w: &mut FieldWriter, msg_id: i32) {
    w.begin();
    w.int(msg_id);
}

fn header_v(w: &mut FieldWriter, msg_id: i32, version: i32) {
    header(w, msg_id);
    w.int(version);
}

/// Initial `API\0` greeting followed by the length-prefixed version range,
/// for example `API\0` + `\0\0\0\x09v157..200`.
pub fn handshake(min_version: i32, max_version: i32, connect_options: &str) -> Vec<u8> {
    let mut range = format!("v{min_version}..{max_version}");
    if !connect_options.is_empty() {
        range.push(' ');
        range.push_str(connect_options);
    }
    let mut out = Vec::with_capacity(8 + range.len());
    out.extend_from_slice(b"API\0");
    out.extend_from_slice(&(range.len() as u32).to_be_bytes());
    out.extend_from_slice(range.as_bytes());
    out
}

pub fn start_api(w: &mut FieldWriter, client_id: i32, optional_capabilities: &str) {
    header_v(w, out::START_API, 2);
    w.int(client_id);
    w.str(optional_capabilities);
}

pub fn req_mkt_data(
    w: &mut FieldWriter,
    req_id: i32,
    c: &Contract,
    generic_tick_list: &str,
    snapshot: bool,
    regulatory_snapshot: bool,
    options: &[TagValue],
) {
    header_v(w, out::REQ_MKT_DATA, 11);
    w.int(req_id);
    contract_full(w, c);
    combo_legs_short(w, c);
    delta_neutral(w, c);
    w.str(generic_tick_list);
    w.bool(snapshot);
    w.bool(regulatory_snapshot);
    w.tag_values(options);
}

pub fn cancel_mkt_data(w: &mut FieldWriter, req_id: i32) {
    header_v(w, out::CANCEL_MKT_DATA, 2);
    w.int(req_id);
}

fn write_condition(w: &mut FieldWriter, cond: &OrderCondition) {
    w.int(cond.cond_type());
    w.str(cond.conjunction());
    match cond {
        OrderCondition::Price {
            is_more,
            price,
            con_id,
            exch,
            trigger_method,
            ..
        } => {
            w.bool(*is_more);
            w.f64(*price);
            w.int(*con_id);
            w.str(exch);
            w.int(*trigger_method);
        }
        OrderCondition::Time { is_more, time, .. } => {
            w.bool(*is_more);
            w.str(time);
        }
        OrderCondition::Margin {
            is_more, percent, ..
        } => {
            w.bool(*is_more);
            w.int(*percent);
        }
        OrderCondition::Execution {
            sec_type,
            exch,
            symbol,
            ..
        } => {
            w.str(sec_type);
            w.str(exch);
            w.str(symbol);
        }
        OrderCondition::Volume {
            is_more,
            volume,
            con_id,
            exch,
            ..
        } => {
            w.bool(*is_more);
            w.int(*volume);
            w.int(*con_id);
            w.str(exch);
        }
        OrderCondition::PercentChange {
            is_more,
            change_percent,
            con_id,
            exch,
            ..
        } => {
            w.bool(*is_more);
            w.f64(*change_percent);
            w.int(*con_id);
            w.str(exch);
        }
    }
}

/// PLACE_ORDER. `server_version` selects the version-gated fields.
pub fn place_order(
    w: &mut FieldWriter,
    server_version: i32,
    order_id: i32,
    c: &Contract,
    o: &Order,
) -> Result<(), Unsupported> {
    let v = server_version;
    if !o.manual_order_time.is_empty() {
        require(v, sv::MANUAL_ORDER_TIME, "Order.manualOrderTime")?;
    }
    if !o.customer_account.is_empty() {
        require(v, sv::CUSTOMER_ACCOUNT, "Order.customerAccount")?;
    }
    if o.professional_customer {
        require(v, sv::PROFESSIONAL_CUSTOMER, "Order.professionalCustomer")?;
    }
    if o.include_overnight {
        require(v, sv::INCLUDE_OVERNIGHT, "Order.includeOvernight")?;
    }
    if o.manual_order_indicator != crate::consts::UNSET_INTEGER {
        require(v, sv::CME_TAGGING_FIELDS, "Order.manualOrderIndicator")?;
    }
    if o.imbalance_only {
        require(v, sv::IMBALANCE_ONLY, "Order.imbalanceOnly")?;
    }

    header(w, out::PLACE_ORDER);
    w.int(order_id);
    contract_full(w, c);
    w.str(&c.sec_id_type);
    w.str(&c.sec_id);

    w.str(&o.action);
    w.f64(o.total_quantity);
    w.str(&o.order_type);
    w.f64_max(o.lmt_price);
    w.f64_max(o.aux_price);

    w.str(&o.tif);
    w.str(&o.oca_group);
    w.str(&o.account);
    w.str(&o.open_close);
    w.int(o.origin);
    w.str(&o.order_ref);
    w.bool(o.transmit);
    w.int(o.parent_id);
    w.bool(o.block_order);
    w.bool(o.sweep_to_fill);
    w.int(o.display_size);
    w.int(o.trigger_method);
    w.bool(o.outside_rth);
    w.bool(o.hidden);

    if c.sec_type == "BAG" {
        w.int(c.combo_legs.len());
        for leg in &c.combo_legs {
            w.int(leg.con_id);
            w.int(leg.ratio);
            w.str(&leg.action);
            w.str(&leg.exchange);
            w.int(leg.open_close);
            w.int(leg.short_sale_slot);
            w.str(&leg.designated_location);
            w.int(leg.exempt_code);
        }
        w.int(o.order_combo_legs.len());
        for leg in &o.order_combo_legs {
            w.f64_max(leg.price);
        }
        w.int(o.smart_combo_routing_params.len());
        for tv in &o.smart_combo_routing_params {
            w.str(&tv.tag);
            w.str(&tv.value);
        }
    }

    w.str(""); // deprecated sharesAllocation
    w.f64(o.discretionary_amt);
    w.str(&o.good_after_time);
    w.str(&o.good_till_date);
    w.str(&o.fa_group);
    w.str(&o.fa_method);
    w.str(&o.fa_percentage);
    if v < sv::FA_PROFILE_DESUPPORT {
        w.str(""); // deprecated faProfile
    }
    w.str(&o.model_code);
    w.int(o.short_sale_slot);
    w.str(&o.designated_location);
    w.int(o.exempt_code);
    w.int(o.oca_type);
    w.str(&o.rule80_a);
    w.str(&o.settling_firm);
    w.bool(o.all_or_none);
    w.int_max(o.min_qty);
    w.f64_max(o.percent_offset);
    w.bool(false); // eTradeOnly, deprecated
    w.bool(false); // firmQuoteOnly, deprecated
    w.f64_max(UNSET_DOUBLE); // nbboPriceCap, deprecated
    w.int(o.auction_strategy);
    w.f64_max(o.starting_price);
    w.f64_max(o.stock_ref_price);
    w.f64_max(o.delta);
    w.f64_max(o.stock_range_lower);
    w.f64_max(o.stock_range_upper);
    w.bool(o.override_percentage_constraints);
    // ib_async clears volatility on non-VOL orders, because IB echoes the field
    // into live orders and then rejects modifications that keep it.
    let volatility = if o.order_type.starts_with("VOL") {
        o.volatility
    } else {
        UNSET_DOUBLE
    };
    w.f64_max(volatility);
    w.int_max(o.volatility_type);
    w.str(&o.delta_neutral_order_type);
    w.f64_max(o.delta_neutral_aux_price);
    if !o.delta_neutral_order_type.is_empty() {
        w.int(o.delta_neutral_con_id);
        w.str(&o.delta_neutral_settling_firm);
        w.str(&o.delta_neutral_clearing_account);
        w.str(&o.delta_neutral_clearing_intent);
        w.str(&o.delta_neutral_open_close);
        w.bool(o.delta_neutral_short_sale);
        w.int(o.delta_neutral_short_sale_slot);
        w.str(&o.delta_neutral_designated_location);
    }
    w.bool(o.continuous_update);
    w.int_max(o.reference_price_type);
    w.f64_max(o.trail_stop_price);
    w.f64_max(o.trailing_percent);
    w.int_max(o.scale_init_level_size);
    w.int_max(o.scale_subs_level_size);
    w.f64_max(o.scale_price_increment);
    if o.scale_price_increment != UNSET_DOUBLE && o.scale_price_increment > 0.0 {
        w.f64_max(o.scale_price_adjust_value);
        w.int_max(o.scale_price_adjust_interval);
        w.f64_max(o.scale_profit_offset);
        w.bool(o.scale_auto_reset);
        w.int_max(o.scale_init_position);
        w.int_max(o.scale_init_fill_qty);
        w.bool(o.scale_random_percent);
    }
    w.str(&o.scale_table);
    w.str(&o.active_start_time);
    w.str(&o.active_stop_time);
    w.str(&o.hedge_type);
    if !o.hedge_type.is_empty() {
        w.str(&o.hedge_param);
    }
    w.bool(o.opt_out_smart_routing);
    w.str(&o.clearing_account);
    w.str(&o.clearing_intent);
    w.bool(o.not_held);
    delta_neutral(w, c);
    w.str(&o.algo_strategy);
    if !o.algo_strategy.is_empty() {
        w.int(o.algo_params.len());
        for tv in &o.algo_params {
            w.str(&tv.tag);
            w.str(&tv.value);
        }
    }
    w.str(&o.algo_id);
    w.bool(o.what_if);
    w.tag_values(&o.order_misc_options);
    w.bool(o.solicited);
    w.bool(o.randomize_size);
    w.bool(o.randomize_price);
    if is_peg_bench(&o.order_type) {
        w.int(o.reference_contract_id);
        w.bool(o.is_pegged_change_amount_decrease);
        w.f64(o.pegged_change_amount);
        w.f64(o.reference_change_amount);
        w.str(&o.reference_exchange_id);
    }
    w.int(o.conditions.len());
    if !o.conditions.is_empty() {
        for cond in &o.conditions {
            write_condition(w, cond);
        }
        w.bool(o.conditions_ignore_rth);
        w.bool(o.conditions_cancel_order);
    }
    w.str(&o.adjusted_order_type);
    w.f64(o.trigger_price);
    w.f64(o.lmt_price_offset);
    w.f64(o.adjusted_stop_price);
    w.f64(o.adjusted_stop_limit_price);
    w.f64(o.adjusted_trailing_amount);
    w.int(o.adjustable_trailing_unit);
    w.str(&o.ext_operator);
    w.str(&o.soft_dollar_tier.name);
    w.str(&o.soft_dollar_tier.value);
    w.f64(o.cash_qty);
    w.str(&o.mifid2_decision_maker);
    w.str(&o.mifid2_decision_algo);
    w.str(&o.mifid2_execution_trader);
    w.str(&o.mifid2_execution_algo);
    w.bool(o.dont_use_auto_price_for_hedge);
    w.bool(o.is_oms_container);
    w.bool(o.discretionary_up_to_limit_price);
    match o.use_price_mgmt_algo {
        None => w.str(""),
        Some(b) => w.bool(b),
    }
    if v >= sv::DURATION {
        w.int(o.duration);
    }
    if v >= sv::POST_TO_ATS {
        w.int(o.post_to_ats);
    }
    if v >= sv::AUTO_CANCEL_PARENT {
        w.bool(o.auto_cancel_parent);
    }
    if v >= sv::ADVANCED_ORDER_REJECT {
        w.str(&o.advanced_error_override);
    }
    if v >= sv::MANUAL_ORDER_TIME {
        w.str(&o.manual_order_time);
    }
    if v >= sv::PEGBEST_PEGMID_OFFSETS {
        let mut send_mid_offsets = false;
        if c.exchange == "IBKRATS" {
            w.int_max(o.min_trade_qty);
        }
        if is_peg_best(&o.order_type) {
            w.int_max(o.min_compete_size);
            w.f64_max(o.compete_against_best_offset);
            if o.compete_against_best_offset == crate::consts::COMPETE_AGAINST_BEST_OFFSET_UP_TO_MID
            {
                send_mid_offsets = true;
            }
        } else if is_peg_mid(&o.order_type) {
            send_mid_offsets = true;
        }
        if send_mid_offsets {
            w.f64_max(o.mid_offset_at_whole);
            w.f64_max(o.mid_offset_at_half);
        }
    }
    if v >= sv::CUSTOMER_ACCOUNT {
        w.str(&o.customer_account);
    }
    if v >= sv::PROFESSIONAL_CUSTOMER {
        w.bool(o.professional_customer);
    }
    if (sv::RFQ_FIELDS..sv::UNDO_RFQ_FIELDS).contains(&v) {
        w.str("");
        w.int(crate::consts::UNSET_INTEGER);
    }
    if v >= sv::INCLUDE_OVERNIGHT {
        w.bool(o.include_overnight);
    }
    if v >= sv::CME_TAGGING_FIELDS {
        w.int(o.manual_order_indicator);
    }
    if v >= sv::IMBALANCE_ONLY {
        w.bool(o.imbalance_only);
    }
    Ok(())
}

pub fn cancel_order(w: &mut FieldWriter, server_version: i32, order_id: i32, oc: &OrderCancel) {
    let v = server_version;
    header(w, out::CANCEL_ORDER);
    if v < sv::CME_TAGGING_FIELDS {
        w.int(1);
    }
    w.int(order_id);
    if v >= sv::MANUAL_ORDER_TIME {
        w.str(&oc.manual_order_cancel_time);
    }
    if (sv::RFQ_FIELDS..sv::UNDO_RFQ_FIELDS).contains(&v) {
        w.str("");
        w.str("");
        w.int(crate::consts::UNSET_INTEGER);
    }
    if v >= sv::CME_TAGGING_FIELDS {
        w.str(&oc.ext_operator);
        w.int(oc.manual_order_indicator);
    }
}

pub fn req_open_orders(w: &mut FieldWriter) {
    header_v(w, out::REQ_OPEN_ORDERS, 1);
}

pub fn req_account_updates(w: &mut FieldWriter, subscribe: bool, acct_code: &str) {
    header_v(w, out::REQ_ACCT_DATA, 2);
    w.bool(subscribe);
    w.str(acct_code);
}

pub fn req_executions(
    w: &mut FieldWriter,
    server_version: i32,
    req_id: i32,
    f: &ExecutionFilter,
) -> Result<(), Unsupported> {
    if f.last_n_days != crate::consts::UNSET_INTEGER || !f.specific_dates.is_empty() {
        require(
            server_version,
            sv::PARAMETRIZED_DAYS_OF_EXECUTIONS,
            "ExecutionFilter.lastNDays/specificDates",
        )?;
    }
    header_v(w, out::REQ_EXECUTIONS, 3);
    w.int(req_id);
    w.int(f.client_id);
    w.str(&f.acct_code);
    w.str(&f.time);
    w.str(&f.symbol);
    w.str(&f.sec_type);
    w.str(&f.exchange);
    w.str(&f.side);
    if server_version >= sv::PARAMETRIZED_DAYS_OF_EXECUTIONS {
        w.int(f.last_n_days);
        w.int(f.specific_dates.len());
        for d in &f.specific_dates {
            w.int(*d);
        }
    }
    Ok(())
}

pub fn req_ids(w: &mut FieldWriter, num_ids: i32) {
    header_v(w, out::REQ_IDS, 1);
    w.int(num_ids);
}

pub fn req_contract_details(w: &mut FieldWriter, server_version: i32, req_id: i32, c: &Contract) {
    header_v(w, out::REQ_CONTRACT_DATA, 8);
    w.int(req_id);
    contract_full(w, c);
    w.bool(c.include_expired);
    w.str(&c.sec_id_type);
    w.str(&c.sec_id);
    if server_version >= sv::BOND_ISSUERID {
        w.str(&c.issuer_id);
    }
}

pub fn req_mkt_depth(
    w: &mut FieldWriter,
    req_id: i32,
    c: &Contract,
    num_rows: i32,
    is_smart_depth: bool,
    options: &[TagValue],
) {
    header_v(w, out::REQ_MKT_DEPTH, 5);
    w.int(req_id);
    contract_full(w, c);
    w.int(num_rows);
    w.bool(is_smart_depth);
    w.tag_values(options);
}

pub fn cancel_mkt_depth(w: &mut FieldWriter, req_id: i32, is_smart_depth: bool) {
    header_v(w, out::CANCEL_MKT_DEPTH, 1);
    w.int(req_id);
    w.bool(is_smart_depth);
}

pub fn req_news_bulletins(w: &mut FieldWriter, all_msgs: bool) {
    header_v(w, out::REQ_NEWS_BULLETINS, 1);
    w.bool(all_msgs);
}

pub fn cancel_news_bulletins(w: &mut FieldWriter) {
    header_v(w, out::CANCEL_NEWS_BULLETINS, 1);
}

pub fn set_server_log_level(w: &mut FieldWriter, log_level: i32) {
    header_v(w, out::SET_SERVER_LOGLEVEL, 1);
    w.int(log_level);
}

pub fn req_auto_open_orders(w: &mut FieldWriter, auto_bind: bool) {
    header_v(w, out::REQ_AUTO_OPEN_ORDERS, 1);
    w.bool(auto_bind);
}

pub fn req_all_open_orders(w: &mut FieldWriter) {
    header_v(w, out::REQ_ALL_OPEN_ORDERS, 1);
}

pub fn req_managed_accts(w: &mut FieldWriter) {
    header_v(w, out::REQ_MANAGED_ACCTS, 1);
}

/// `fa_data_type`: 1 groups, 3 aliases (2, profiles, was removed at server version 177).
pub fn request_fa(w: &mut FieldWriter, fa_data_type: i32) {
    header_v(w, out::REQ_FA, 1);
    w.int(fa_data_type);
}

pub fn replace_fa(w: &mut FieldWriter, req_id: i32, fa_data_type: i32, xml: &str) {
    header_v(w, out::REPLACE_FA, 1);
    w.int(fa_data_type);
    w.str(xml);
    w.int(req_id);
}

pub fn req_historical_data(
    w: &mut FieldWriter,
    req_id: i32,
    c: &Contract,
    end_date_time: &str,
    duration_str: &str,
    bar_size_setting: &str,
    what_to_show: &str,
    use_rth: bool,
    format_date: i32,
    keep_up_to_date: bool,
    chart_options: &[TagValue],
) {
    header(w, out::REQ_HISTORICAL_DATA);
    w.int(req_id);
    contract_full(w, c);
    w.bool(c.include_expired);
    w.str(end_date_time);
    w.str(bar_size_setting);
    w.str(duration_str);
    w.bool(use_rth);
    w.str(what_to_show);
    w.int(format_date);
    combo_legs_short(w, c);
    w.bool(keep_up_to_date);
    w.tag_values(chart_options);
}

pub fn exercise_options(
    w: &mut FieldWriter,
    server_version: i32,
    req_id: i32,
    c: &Contract,
    exercise_action: i32,
    exercise_quantity: i32,
    account: &str,
    override_: i32,
    manual_order_time: &str,
    customer_account: &str,
    professional_customer: bool,
) -> Result<(), Unsupported> {
    let v = server_version;
    if !manual_order_time.is_empty() {
        require(
            v,
            sv::MANUAL_ORDER_TIME_EXERCISE_OPTIONS,
            "exerciseOptions manualOrderTime",
        )?;
    }
    if !customer_account.is_empty() {
        require(v, sv::CUSTOMER_ACCOUNT, "exerciseOptions customerAccount")?;
    }
    if professional_customer {
        require(
            v,
            sv::PROFESSIONAL_CUSTOMER,
            "exerciseOptions professionalCustomer",
        )?;
    }
    header_v(w, out::EXERCISE_OPTIONS, 2);
    w.int(req_id);
    w.int(c.con_id);
    w.str(&c.symbol);
    w.str(&c.sec_type);
    w.str(&c.last_trade_date_or_contract_month);
    w.f64_max(c.strike);
    w.str(&c.right);
    w.str(&c.multiplier);
    w.str(&c.exchange);
    w.str(&c.currency);
    w.str(&c.local_symbol);
    w.str(&c.trading_class);
    w.int(exercise_action);
    w.int(exercise_quantity);
    w.str(account);
    w.int(override_);
    if v >= sv::MANUAL_ORDER_TIME_EXERCISE_OPTIONS {
        w.str(manual_order_time);
    }
    if v >= sv::CUSTOMER_ACCOUNT {
        w.str(customer_account);
    }
    if v >= sv::PROFESSIONAL_CUSTOMER {
        w.bool(professional_customer);
    }
    Ok(())
}

pub fn req_scanner_subscription(
    w: &mut FieldWriter,
    req_id: i32,
    s: &ScannerSubscription,
    options: &[TagValue],
    filter_options: &[TagValue],
) {
    header(w, out::REQ_SCANNER_SUBSCRIPTION);
    w.int(req_id);
    w.int_max(s.number_of_rows);
    w.str(&s.instrument);
    w.str(&s.location_code);
    w.str(&s.scan_code);
    w.f64_max(s.above_price);
    w.f64_max(s.below_price);
    w.int_max(s.above_volume);
    w.f64_max(s.market_cap_above);
    w.f64_max(s.market_cap_below);
    w.str(&s.moody_rating_above);
    w.str(&s.moody_rating_below);
    w.str(&s.sp_rating_above);
    w.str(&s.sp_rating_below);
    w.str(&s.maturity_date_above);
    w.str(&s.maturity_date_below);
    w.f64_max(s.coupon_rate_above);
    w.f64_max(s.coupon_rate_below);
    w.bool(s.exclude_convertible);
    w.int_max(s.average_option_volume_above);
    w.str(&s.scanner_setting_pairs);
    w.str(&s.stock_type_filter);
    w.tag_values(filter_options);
    w.tag_values(options);
}

pub fn cancel_scanner_subscription(w: &mut FieldWriter, req_id: i32) {
    header_v(w, out::CANCEL_SCANNER_SUBSCRIPTION, 1);
    w.int(req_id);
}

pub fn req_scanner_parameters(w: &mut FieldWriter) {
    header_v(w, out::REQ_SCANNER_PARAMETERS, 1);
}

pub fn cancel_historical_data(w: &mut FieldWriter, req_id: i32) {
    header_v(w, out::CANCEL_HISTORICAL_DATA, 1);
    w.int(req_id);
}

pub fn req_current_time(w: &mut FieldWriter) {
    header_v(w, out::REQ_CURRENT_TIME, 1);
}

pub fn req_current_time_in_millis(
    w: &mut FieldWriter,
    server_version: i32,
) -> Result<(), Unsupported> {
    require(
        server_version,
        sv::CURRENT_TIME_IN_MILLIS,
        "reqCurrentTimeInMillis",
    )?;
    header(w, out::REQ_CURRENT_TIME_IN_MILLIS);
    Ok(())
}

pub fn req_real_time_bars(
    w: &mut FieldWriter,
    req_id: i32,
    c: &Contract,
    bar_size: i32,
    what_to_show: &str,
    use_rth: bool,
    options: &[TagValue],
) {
    header_v(w, out::REQ_REAL_TIME_BARS, 3);
    w.int(req_id);
    contract_full(w, c);
    w.int(bar_size);
    w.str(what_to_show);
    w.bool(use_rth);
    w.tag_values(options);
}

pub fn cancel_real_time_bars(w: &mut FieldWriter, req_id: i32) {
    header_v(w, out::CANCEL_REAL_TIME_BARS, 1);
    w.int(req_id);
}

pub fn req_fundamental_data(
    w: &mut FieldWriter,
    req_id: i32,
    c: &Contract,
    report_type: &str,
    options: &[TagValue],
) {
    header_v(w, out::REQ_FUNDAMENTAL_DATA, 2);
    w.int(req_id);
    w.int(c.con_id);
    w.str(&c.symbol);
    w.str(&c.sec_type);
    w.str(&c.exchange);
    w.str(&c.primary_exchange);
    w.str(&c.currency);
    w.str(&c.local_symbol);
    w.str(report_type);
    w.int(options.len());
    w.tag_values(options);
}

pub fn cancel_fundamental_data(w: &mut FieldWriter, req_id: i32) {
    header_v(w, out::CANCEL_FUNDAMENTAL_DATA, 1);
    w.int(req_id);
}

pub fn calculate_implied_volatility(
    w: &mut FieldWriter,
    req_id: i32,
    c: &Contract,
    option_price: f64,
    under_price: f64,
    options: &[TagValue],
) {
    header_v(w, out::REQ_CALC_IMPLIED_VOLAT, 3);
    w.int(req_id);
    contract_full(w, c);
    w.f64(option_price);
    w.f64(under_price);
    w.tag_values(options);
}

pub fn calculate_option_price(
    w: &mut FieldWriter,
    req_id: i32,
    c: &Contract,
    volatility: f64,
    under_price: f64,
    options: &[TagValue],
) {
    header_v(w, out::REQ_CALC_OPTION_PRICE, 3);
    w.int(req_id);
    contract_full(w, c);
    w.f64(volatility);
    w.f64(under_price);
    w.tag_values(options);
}

pub fn cancel_calculate_implied_volatility(w: &mut FieldWriter, req_id: i32) {
    header_v(w, out::CANCEL_CALC_IMPLIED_VOLAT, 1);
    w.int(req_id);
}

pub fn cancel_calculate_option_price(w: &mut FieldWriter, req_id: i32) {
    header_v(w, out::CANCEL_CALC_OPTION_PRICE, 1);
    w.int(req_id);
}

pub fn req_global_cancel(w: &mut FieldWriter, server_version: i32, oc: &OrderCancel) {
    header(w, out::REQ_GLOBAL_CANCEL);
    if server_version < sv::CME_TAGGING_FIELDS {
        w.int(1);
    } else {
        w.str(&oc.ext_operator);
        w.int(oc.manual_order_indicator);
    }
}

/// 1 live, 2 frozen, 3 delayed, 4 delayed frozen.
pub fn req_market_data_type(w: &mut FieldWriter, market_data_type: i32) {
    header_v(w, out::REQ_MARKET_DATA_TYPE, 1);
    w.int(market_data_type);
}

pub fn req_positions(w: &mut FieldWriter) {
    header_v(w, out::REQ_POSITIONS, 1);
}

pub fn cancel_positions(w: &mut FieldWriter) {
    header_v(w, out::CANCEL_POSITIONS, 1);
}

pub fn req_account_summary(w: &mut FieldWriter, req_id: i32, group_name: &str, tags: &str) {
    header_v(w, out::REQ_ACCOUNT_SUMMARY, 1);
    w.int(req_id);
    w.str(group_name);
    w.str(tags);
}

pub fn cancel_account_summary(w: &mut FieldWriter, req_id: i32) {
    header_v(w, out::CANCEL_ACCOUNT_SUMMARY, 1);
    w.int(req_id);
}

pub fn verify_request(w: &mut FieldWriter, api_name: &str, api_version: &str) {
    header_v(w, out::VERIFY_REQUEST, 1);
    w.str(api_name);
    w.str(api_version);
}

pub fn verify_message(w: &mut FieldWriter, api_data: &str) {
    header_v(w, out::VERIFY_MESSAGE, 1);
    w.str(api_data);
}

pub fn verify_and_auth_request(
    w: &mut FieldWriter,
    api_name: &str,
    api_version: &str,
    opaque_isv_key: &str,
) {
    header_v(w, out::VERIFY_AND_AUTH_REQUEST, 1);
    w.str(api_name);
    w.str(api_version);
    w.str(opaque_isv_key);
}

pub fn verify_and_auth_message(w: &mut FieldWriter, api_data: &str, xyz_response: &str) {
    header_v(w, out::VERIFY_AND_AUTH_MESSAGE, 1);
    w.str(api_data);
    w.str(xyz_response);
}

pub fn query_display_groups(w: &mut FieldWriter, req_id: i32) {
    header_v(w, out::QUERY_DISPLAY_GROUPS, 1);
    w.int(req_id);
}

pub fn subscribe_to_group_events(w: &mut FieldWriter, req_id: i32, group_id: i32) {
    header_v(w, out::SUBSCRIBE_TO_GROUP_EVENTS, 1);
    w.int(req_id);
    w.int(group_id);
}

pub fn update_display_group(w: &mut FieldWriter, req_id: i32, contract_info: &str) {
    header_v(w, out::UPDATE_DISPLAY_GROUP, 1);
    w.int(req_id);
    w.str(contract_info);
}

pub fn unsubscribe_from_group_events(w: &mut FieldWriter, req_id: i32) {
    header_v(w, out::UNSUBSCRIBE_FROM_GROUP_EVENTS, 1);
    w.int(req_id);
}

pub fn req_positions_multi(w: &mut FieldWriter, req_id: i32, account: &str, model_code: &str) {
    header_v(w, out::REQ_POSITIONS_MULTI, 1);
    w.int(req_id);
    w.str(account);
    w.str(model_code);
}

pub fn cancel_positions_multi(w: &mut FieldWriter, req_id: i32) {
    header_v(w, out::CANCEL_POSITIONS_MULTI, 1);
    w.int(req_id);
}

pub fn req_account_updates_multi(
    w: &mut FieldWriter,
    req_id: i32,
    account: &str,
    model_code: &str,
    ledger_and_nlv: bool,
) {
    header_v(w, out::REQ_ACCOUNT_UPDATES_MULTI, 1);
    w.int(req_id);
    w.str(account);
    w.str(model_code);
    w.bool(ledger_and_nlv);
}

pub fn cancel_account_updates_multi(w: &mut FieldWriter, req_id: i32) {
    header_v(w, out::CANCEL_ACCOUNT_UPDATES_MULTI, 1);
    w.int(req_id);
}

pub fn req_sec_def_opt_params(
    w: &mut FieldWriter,
    req_id: i32,
    underlying_symbol: &str,
    fut_fop_exchange: &str,
    underlying_sec_type: &str,
    underlying_con_id: i32,
) {
    header(w, out::REQ_SEC_DEF_OPT_PARAMS);
    w.int(req_id);
    w.str(underlying_symbol);
    w.str(fut_fop_exchange);
    w.str(underlying_sec_type);
    w.int(underlying_con_id);
}

pub fn req_soft_dollar_tiers(w: &mut FieldWriter, req_id: i32) {
    header(w, out::REQ_SOFT_DOLLAR_TIERS);
    w.int(req_id);
}

pub fn req_family_codes(w: &mut FieldWriter) {
    header(w, out::REQ_FAMILY_CODES);
}

pub fn req_matching_symbols(w: &mut FieldWriter, req_id: i32, pattern: &str) {
    header(w, out::REQ_MATCHING_SYMBOLS);
    w.int(req_id);
    w.str(pattern);
}

pub fn req_mkt_depth_exchanges(w: &mut FieldWriter) {
    header(w, out::REQ_MKT_DEPTH_EXCHANGES);
}

pub fn req_smart_components(w: &mut FieldWriter, req_id: i32, bbo_exchange: &str) {
    header(w, out::REQ_SMART_COMPONENTS);
    w.int(req_id);
    w.str(bbo_exchange);
}

pub fn req_news_article(
    w: &mut FieldWriter,
    req_id: i32,
    provider_code: &str,
    article_id: &str,
    options: &[TagValue],
) {
    header(w, out::REQ_NEWS_ARTICLE);
    w.int(req_id);
    w.str(provider_code);
    w.str(article_id);
    w.tag_values(options);
}

pub fn req_news_providers(w: &mut FieldWriter) {
    header(w, out::REQ_NEWS_PROVIDERS);
}

pub fn req_historical_news(
    w: &mut FieldWriter,
    req_id: i32,
    con_id: i32,
    provider_codes: &str,
    start_date_time: &str,
    end_date_time: &str,
    total_results: i32,
    options: &[TagValue],
) {
    header(w, out::REQ_HISTORICAL_NEWS);
    w.int(req_id);
    w.int(con_id);
    w.str(provider_codes);
    w.str(start_date_time);
    w.str(end_date_time);
    w.int(total_results);
    w.tag_values(options);
}

pub fn req_head_time_stamp(
    w: &mut FieldWriter,
    req_id: i32,
    c: &Contract,
    what_to_show: &str,
    use_rth: bool,
    format_date: i32,
) {
    header(w, out::REQ_HEAD_TIMESTAMP);
    w.int(req_id);
    contract_full(w, c);
    w.bool(c.include_expired);
    w.bool(use_rth);
    w.str(what_to_show);
    w.int(format_date);
}

pub fn cancel_head_time_stamp(w: &mut FieldWriter, req_id: i32) {
    header(w, out::CANCEL_HEAD_TIMESTAMP);
    w.int(req_id);
}

pub fn req_histogram_data(
    w: &mut FieldWriter,
    req_id: i32,
    c: &Contract,
    use_rth: bool,
    time_period: &str,
) {
    header(w, out::REQ_HISTOGRAM_DATA);
    w.int(req_id);
    contract_full(w, c);
    w.bool(c.include_expired);
    w.bool(use_rth);
    w.str(time_period);
}

pub fn cancel_histogram_data(w: &mut FieldWriter, req_id: i32) {
    header(w, out::CANCEL_HISTOGRAM_DATA);
    w.int(req_id);
}

pub fn req_market_rule(w: &mut FieldWriter, market_rule_id: i32) {
    header(w, out::REQ_MARKET_RULE);
    w.int(market_rule_id);
}

pub fn req_pnl(w: &mut FieldWriter, req_id: i32, account: &str, model_code: &str) {
    header(w, out::REQ_PNL);
    w.int(req_id);
    w.str(account);
    w.str(model_code);
}

pub fn cancel_pnl(w: &mut FieldWriter, req_id: i32) {
    header(w, out::CANCEL_PNL);
    w.int(req_id);
}

pub fn req_pnl_single(
    w: &mut FieldWriter,
    req_id: i32,
    account: &str,
    model_code: &str,
    con_id: i32,
) {
    header(w, out::REQ_PNL_SINGLE);
    w.int(req_id);
    w.str(account);
    w.str(model_code);
    w.int(con_id);
}

pub fn cancel_pnl_single(w: &mut FieldWriter, req_id: i32) {
    header(w, out::CANCEL_PNL_SINGLE);
    w.int(req_id);
}

pub fn req_historical_ticks(
    w: &mut FieldWriter,
    req_id: i32,
    c: &Contract,
    start_date_time: &str,
    end_date_time: &str,
    number_of_ticks: i32,
    what_to_show: &str,
    use_rth: bool,
    ignore_size: bool,
    misc_options: &[TagValue],
) {
    header(w, out::REQ_HISTORICAL_TICKS);
    w.int(req_id);
    contract_full(w, c);
    w.bool(c.include_expired);
    w.str(start_date_time);
    w.str(end_date_time);
    w.int(number_of_ticks);
    w.str(what_to_show);
    w.bool(use_rth);
    w.bool(ignore_size);
    w.tag_values(misc_options);
}

/// `tick_type`: "Last", "AllLast", "BidAsk" or "MidPoint".
pub fn req_tick_by_tick_data(
    w: &mut FieldWriter,
    req_id: i32,
    c: &Contract,
    tick_type: &str,
    number_of_ticks: i32,
    ignore_size: bool,
) {
    header(w, out::REQ_TICK_BY_TICK_DATA);
    w.int(req_id);
    contract_full(w, c);
    w.str(tick_type);
    w.int(number_of_ticks);
    w.bool(ignore_size);
}

pub fn cancel_tick_by_tick_data(w: &mut FieldWriter, req_id: i32) {
    header(w, out::CANCEL_TICK_BY_TICK_DATA);
    w.int(req_id);
}

pub fn req_completed_orders(w: &mut FieldWriter, api_only: bool) {
    header(w, out::REQ_COMPLETED_ORDERS);
    w.bool(api_only);
}

pub fn req_wsh_meta_data(w: &mut FieldWriter, req_id: i32) {
    header(w, out::REQ_WSH_META_DATA);
    w.int(req_id);
}

pub fn cancel_wsh_meta_data(w: &mut FieldWriter, req_id: i32) {
    header(w, out::CANCEL_WSH_META_DATA);
    w.int(req_id);
}

pub fn req_wsh_event_data(w: &mut FieldWriter, server_version: i32, req_id: i32, d: &WshEventData) {
    header(w, out::REQ_WSH_EVENT_DATA);
    w.int(req_id);
    w.int(d.con_id);
    if server_version >= sv::WSH_EVENT_DATA_FILTERS {
        w.str(&d.filter);
        w.bool(d.fill_watchlist);
        w.bool(d.fill_portfolio);
        w.bool(d.fill_competitors);
    }
    if server_version >= sv::WSH_EVENT_DATA_FILTERS_DATE {
        w.str(&d.start_date);
        w.str(&d.end_date);
        w.int(d.total_limit);
    }
}

pub fn cancel_wsh_event_data(w: &mut FieldWriter, req_id: i32) {
    header(w, out::CANCEL_WSH_EVENT_DATA);
    w.int(req_id);
}

pub fn req_user_info(w: &mut FieldWriter, req_id: i32) {
    header(w, out::REQ_USER_INFO);
    w.int(req_id);
}
