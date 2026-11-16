//! Protobuf encoders for client requests (server version 201 and later).
//!
//! Each function returns `None` when the connected server version is below
//! the version at which IB's client sends that request as protobuf; the
//! caller then uses the text encoder in [`crate::encode`]. The field choices
//! follow IB's `client_utils.py`: a field is set only when it holds a value.

use crate::consts::{UNSET_DOUBLE, UNSET_INTEGER};
use crate::message::outgoing as out;
use crate::pb::PbWriter;
use crate::proto::*;
use crate::server_versions as sv;
use crate::types::*;

fn frame(
    server_version: i32,
    min_version: i32,
    msg_id: i32,
    build: impl FnOnce(&mut PbWriter),
) -> Option<Vec<u8>> {
    (server_version >= min_version).then(|| {
        let mut w = PbWriter::default();
        build(&mut w);
        w.frame(msg_id)
    })
}

/// Request that carries only a request id.
fn req_id_only(
    server_version: i32,
    min_version: i32,
    msg_id: i32,
    field: u32,
    req_id: i32,
) -> Option<Vec<u8>> {
    frame(server_version, min_version, msg_id, |w| {
        w.opt_i32(field, req_id)
    })
}

pub(crate) fn contract_pb(w: &mut PbWriter, c: &Contract, order: Option<&Order>) {
    use contract as f;
    w.opt_i32(f::CON_ID, c.con_id);
    w.opt_str(f::SYMBOL, &c.symbol);
    w.opt_str(f::SEC_TYPE, &c.sec_type);
    w.opt_str(
        f::LAST_TRADE_DATE_OR_CONTRACT_MONTH,
        &c.last_trade_date_or_contract_month,
    );
    w.opt_f64(f::STRIKE, c.strike);
    w.opt_str(f::RIGHT, &c.right);
    if let Ok(m) = c.multiplier.trim().parse::<f64>() {
        w.f64(f::MULTIPLIER, m);
    }
    w.opt_str(f::EXCHANGE, &c.exchange);
    w.opt_str(f::PRIMARY_EXCH, &c.primary_exchange);
    w.opt_str(f::CURRENCY, &c.currency);
    w.opt_str(f::LOCAL_SYMBOL, &c.local_symbol);
    w.opt_str(f::TRADING_CLASS, &c.trading_class);
    w.opt_str(f::SEC_ID_TYPE, &c.sec_id_type);
    w.opt_str(f::SEC_ID, &c.sec_id);
    w.opt_bool(f::INCLUDE_EXPIRED, c.include_expired);
    w.opt_str(f::COMBO_LEGS_DESCRIP, &c.combo_legs_descrip);
    w.opt_str(f::DESCRIPTION, &c.description);
    w.opt_str(f::ISSUER_ID, &c.issuer_id);
    for (i, leg) in c.combo_legs.iter().enumerate() {
        let per_leg_price = order
            .and_then(|o| o.order_combo_legs.get(i))
            .map_or(UNSET_DOUBLE, |l| l.price);
        w.msg(f::COMBO_LEGS, |w| {
            use combo_leg as l;
            w.opt_i32(l::CON_ID, leg.con_id);
            w.opt_i32(l::RATIO, leg.ratio);
            w.opt_str(l::ACTION, &leg.action);
            w.opt_str(l::EXCHANGE, &leg.exchange);
            w.opt_i32(l::OPEN_CLOSE, leg.open_close);
            w.opt_i32(l::SHORT_SALES_SLOT, leg.short_sale_slot);
            w.opt_str(l::DESIGNATED_LOCATION, &leg.designated_location);
            w.opt_i32(l::EXEMPT_CODE, leg.exempt_code);
            w.opt_f64(l::PER_LEG_PRICE, per_leg_price);
        });
    }
    if let Some(d) = &c.delta_neutral_contract {
        w.msg(f::DELTA_NEUTRAL_CONTRACT, |w| {
            w.opt_i32(delta_neutral_contract::CON_ID, d.con_id);
            w.opt_f64(delta_neutral_contract::DELTA, d.delta);
            w.opt_f64(delta_neutral_contract::PRICE, d.price);
        });
    }
}

fn condition_pb(w: &mut PbWriter, c: &OrderCondition) {
    use order_condition as f;
    w.i32(f::TYPE, c.cond_type());
    w.bool(f::IS_CONJUNCTION_CONNECTION, c.conjunction() == "a");
    match c {
        OrderCondition::Price {
            is_more,
            price,
            con_id,
            exch,
            trigger_method,
            ..
        } => {
            w.bool(f::IS_MORE, *is_more);
            w.opt_i32(f::CON_ID, *con_id);
            w.opt_str(f::EXCHANGE, exch);
            w.opt_f64(f::PRICE, *price);
            w.opt_i32(f::TRIGGER_METHOD, *trigger_method);
        }
        OrderCondition::Time { is_more, time, .. } => {
            w.bool(f::IS_MORE, *is_more);
            w.opt_str(f::TIME, time);
        }
        OrderCondition::Margin {
            is_more, percent, ..
        } => {
            w.bool(f::IS_MORE, *is_more);
            w.opt_i32(f::PERCENT, *percent);
        }
        OrderCondition::Execution {
            sec_type,
            exch,
            symbol,
            ..
        } => {
            w.opt_str(f::SEC_TYPE, sec_type);
            w.opt_str(f::EXCHANGE, exch);
            w.opt_str(f::SYMBOL, symbol);
        }
        OrderCondition::Volume {
            is_more,
            volume,
            con_id,
            exch,
            ..
        } => {
            w.bool(f::IS_MORE, *is_more);
            w.opt_i32(f::CON_ID, *con_id);
            w.opt_str(f::EXCHANGE, exch);
            w.opt_i32(f::VOLUME, *volume);
        }
        OrderCondition::PercentChange {
            is_more,
            change_percent,
            con_id,
            exch,
            ..
        } => {
            w.bool(f::IS_MORE, *is_more);
            w.opt_i32(f::CON_ID, *con_id);
            w.opt_str(f::EXCHANGE, exch);
            w.opt_f64(f::CHANGE_PERCENT, *change_percent);
        }
    }
}

fn opt_flag(w: &mut PbWriter, field: u32, v: Option<bool>) {
    if let Some(b) = v {
        w.i32(field, i32::from(b));
    }
}

pub(crate) fn order_pb(w: &mut PbWriter, o: &Order) {
    use order as f;
    w.opt_i32(f::CLIENT_ID, o.client_id);
    w.opt_i64(f::PERM_ID, o.perm_id);
    w.opt_i32(f::PARENT_ID, o.parent_id);
    w.opt_str(f::ACTION, &o.action);
    w.opt_decimal(f::TOTAL_QUANTITY, o.total_quantity);
    w.opt_i32(f::DISPLAY_SIZE, o.display_size);
    w.opt_str(f::ORDER_TYPE, &o.order_type);
    w.opt_f64(f::LMT_PRICE, o.lmt_price);
    w.opt_f64(f::AUX_PRICE, o.aux_price);
    w.opt_str(f::TIF, &o.tif);
    w.opt_str(f::ACCOUNT, &o.account);
    w.opt_str(f::SETTLING_FIRM, &o.settling_firm);
    w.opt_str(f::CLEARING_ACCOUNT, &o.clearing_account);
    w.opt_str(f::CLEARING_INTENT, &o.clearing_intent);
    w.opt_bool(f::ALL_OR_NONE, o.all_or_none);
    w.opt_bool(f::BLOCK_ORDER, o.block_order);
    w.opt_bool(f::HIDDEN, o.hidden);
    w.opt_bool(f::OUTSIDE_RTH, o.outside_rth);
    w.opt_bool(f::SWEEP_TO_FILL, o.sweep_to_fill);
    w.opt_f64(f::PERCENT_OFFSET, o.percent_offset);
    w.opt_f64(f::TRAILING_PERCENT, o.trailing_percent);
    w.opt_f64(f::TRAIL_STOP_PRICE, o.trail_stop_price);
    w.opt_i32(f::MIN_QTY, o.min_qty);
    w.opt_str(f::GOOD_AFTER_TIME, &o.good_after_time);
    w.opt_str(f::GOOD_TILL_DATE, &o.good_till_date);
    w.opt_str(f::OCA_GROUP, &o.oca_group);
    w.opt_str(f::ORDER_REF, &o.order_ref);
    w.opt_str(f::RULE80_A, &o.rule80_a);
    w.opt_i32(f::OCA_TYPE, o.oca_type);
    w.opt_i32(f::TRIGGER_METHOD, o.trigger_method);
    w.opt_str(f::ACTIVE_START_TIME, &o.active_start_time);
    w.opt_str(f::ACTIVE_STOP_TIME, &o.active_stop_time);
    w.opt_str(f::FA_GROUP, &o.fa_group);
    w.opt_str(f::FA_METHOD, &o.fa_method);
    w.opt_str(f::FA_PERCENTAGE, &o.fa_percentage);
    w.opt_f64(f::VOLATILITY, o.volatility);
    w.opt_i32(f::VOLATILITY_TYPE, o.volatility_type);
    w.opt_bool(f::CONTINUOUS_UPDATE, o.continuous_update);
    w.opt_i32(f::REFERENCE_PRICE_TYPE, o.reference_price_type);
    w.opt_str(f::DELTA_NEUTRAL_ORDER_TYPE, &o.delta_neutral_order_type);
    w.opt_f64(f::DELTA_NEUTRAL_AUX_PRICE, o.delta_neutral_aux_price);
    w.opt_i32(f::DELTA_NEUTRAL_CON_ID, o.delta_neutral_con_id);
    w.opt_str(f::DELTA_NEUTRAL_OPEN_CLOSE, &o.delta_neutral_open_close);
    w.opt_bool(f::DELTA_NEUTRAL_SHORT_SALE, o.delta_neutral_short_sale);
    w.opt_i32(
        f::DELTA_NEUTRAL_SHORT_SALE_SLOT,
        o.delta_neutral_short_sale_slot,
    );
    w.opt_str(
        f::DELTA_NEUTRAL_DESIGNATED_LOCATION,
        &o.delta_neutral_designated_location,
    );
    w.opt_i32(f::SCALE_INIT_LEVEL_SIZE, o.scale_init_level_size);
    w.opt_i32(f::SCALE_SUBS_LEVEL_SIZE, o.scale_subs_level_size);
    w.opt_f64(f::SCALE_PRICE_INCREMENT, o.scale_price_increment);
    w.opt_f64(f::SCALE_PRICE_ADJUST_VALUE, o.scale_price_adjust_value);
    w.opt_i32(
        f::SCALE_PRICE_ADJUST_INTERVAL,
        o.scale_price_adjust_interval,
    );
    w.opt_f64(f::SCALE_PROFIT_OFFSET, o.scale_profit_offset);
    w.opt_bool(f::SCALE_AUTO_RESET, o.scale_auto_reset);
    w.opt_i32(f::SCALE_INIT_POSITION, o.scale_init_position);
    w.opt_i32(f::SCALE_INIT_FILL_QTY, o.scale_init_fill_qty);
    w.opt_bool(f::SCALE_RANDOM_PERCENT, o.scale_random_percent);
    w.opt_str(f::SCALE_TABLE, &o.scale_table);
    w.opt_str(f::HEDGE_TYPE, &o.hedge_type);
    w.opt_str(f::HEDGE_PARAM, &o.hedge_param);
    w.opt_i32(f::HEDGE_MAX_SIZE, o.hedge_max_size);
    w.opt_str(f::ALGO_STRATEGY, &o.algo_strategy);
    w.map(f::ALGO_PARAMS, &o.algo_params);
    w.opt_str(f::ALGO_ID, &o.algo_id);
    w.map(f::SMART_COMBO_ROUTING_PARAMS, &o.smart_combo_routing_params);
    w.opt_bool(f::WHAT_IF, o.what_if);
    w.opt_bool(f::TRANSMIT, o.transmit);
    w.opt_bool(
        f::OVERRIDE_PERCENTAGE_CONSTRAINTS,
        o.override_percentage_constraints,
    );
    w.opt_str(f::OPEN_CLOSE, &o.open_close);
    w.opt_i32(f::ORIGIN, o.origin);
    w.opt_i32(f::SHORT_SALE_SLOT, o.short_sale_slot);
    w.opt_str(f::DESIGNATED_LOCATION, &o.designated_location);
    w.opt_i32(f::EXEMPT_CODE, o.exempt_code);
    w.opt_str(
        f::DELTA_NEUTRAL_SETTLING_FIRM,
        &o.delta_neutral_settling_firm,
    );
    w.opt_str(
        f::DELTA_NEUTRAL_CLEARING_ACCOUNT,
        &o.delta_neutral_clearing_account,
    );
    w.opt_str(
        f::DELTA_NEUTRAL_CLEARING_INTENT,
        &o.delta_neutral_clearing_intent,
    );
    w.opt_f64(f::DISCRETIONARY_AMT, o.discretionary_amt);
    w.opt_bool(f::OPT_OUT_SMART_ROUTING, o.opt_out_smart_routing);
    w.opt_f64(f::STARTING_PRICE, o.starting_price);
    w.opt_f64(f::STOCK_REF_PRICE, o.stock_ref_price);
    w.opt_f64(f::DELTA, o.delta);
    w.opt_f64(f::STOCK_RANGE_LOWER, o.stock_range_lower);
    w.opt_f64(f::STOCK_RANGE_UPPER, o.stock_range_upper);
    w.opt_bool(f::NOT_HELD, o.not_held);
    w.map(f::ORDER_MISC_OPTIONS, &o.order_misc_options);
    w.opt_bool(f::SOLICITED, o.solicited);
    w.opt_bool(f::RANDOMIZE_SIZE, o.randomize_size);
    w.opt_bool(f::RANDOMIZE_PRICE, o.randomize_price);
    w.opt_i32(f::REFERENCE_CONTRACT_ID, o.reference_contract_id);
    w.opt_f64(f::PEGGED_CHANGE_AMOUNT, o.pegged_change_amount);
    w.opt_bool(
        f::IS_PEGGED_CHANGE_AMOUNT_DECREASE,
        o.is_pegged_change_amount_decrease,
    );
    w.opt_f64(f::REFERENCE_CHANGE_AMOUNT, o.reference_change_amount);
    w.opt_str(f::REFERENCE_EXCHANGE_ID, &o.reference_exchange_id);
    w.opt_str(f::ADJUSTED_ORDER_TYPE, &o.adjusted_order_type);
    w.opt_f64(f::TRIGGER_PRICE, o.trigger_price);
    w.opt_f64(f::ADJUSTED_STOP_PRICE, o.adjusted_stop_price);
    w.opt_f64(f::ADJUSTED_STOP_LIMIT_PRICE, o.adjusted_stop_limit_price);
    w.opt_f64(f::ADJUSTED_TRAILING_AMOUNT, o.adjusted_trailing_amount);
    w.opt_i32(f::ADJUSTABLE_TRAILING_UNIT, o.adjustable_trailing_unit);
    w.opt_f64(f::LMT_PRICE_OFFSET, o.lmt_price_offset);
    for c in &o.conditions {
        w.msg(f::CONDITIONS, |w| condition_pb(w, c));
    }
    w.opt_bool(f::CONDITIONS_CANCEL_ORDER, o.conditions_cancel_order);
    w.opt_bool(f::CONDITIONS_IGNORE_RTH, o.conditions_ignore_rth);
    w.opt_bool(
        f::CONDITIONS_INCLUDE_OVERNIGHT,
        o.conditions_include_overnight,
    );
    w.opt_str(f::MODEL_CODE, &o.model_code);
    w.opt_str(f::EXT_OPERATOR, &o.ext_operator);
    let tier = &o.soft_dollar_tier;
    if !tier.name.is_empty() || !tier.value.is_empty() || !tier.display_name.is_empty() {
        w.msg(f::SOFT_DOLLAR_TIER, |w| {
            w.opt_str(soft_dollar_tier::NAME, &tier.name);
            w.opt_str(soft_dollar_tier::VALUE, &tier.value);
            w.opt_str(soft_dollar_tier::DISPLAY_NAME, &tier.display_name);
        });
    }
    w.opt_f64(f::CASH_QTY, o.cash_qty);
    w.opt_str(f::MIFID2_DECISION_MAKER, &o.mifid2_decision_maker);
    w.opt_str(f::MIFID2_DECISION_ALGO, &o.mifid2_decision_algo);
    w.opt_str(f::MIFID2_EXECUTION_TRADER, &o.mifid2_execution_trader);
    w.opt_str(f::MIFID2_EXECUTION_ALGO, &o.mifid2_execution_algo);
    w.opt_bool(
        f::DONT_USE_AUTO_PRICE_FOR_HEDGE,
        o.dont_use_auto_price_for_hedge,
    );
    w.opt_bool(f::IS_OMS_CONTAINER, o.is_oms_container);
    w.opt_bool(
        f::DISCRETIONARY_UP_TO_LIMIT_PRICE,
        o.discretionary_up_to_limit_price,
    );
    opt_flag(w, f::USE_PRICE_MGMT_ALGO, o.use_price_mgmt_algo);
    w.opt_i32(f::DURATION, o.duration);
    w.opt_i32(f::POST_TO_ATS, o.post_to_ats);
    w.opt_str(f::ADVANCED_ERROR_OVERRIDE, &o.advanced_error_override);
    w.opt_str(f::MANUAL_ORDER_TIME, &o.manual_order_time);
    w.opt_i32(f::MIN_TRADE_QTY, o.min_trade_qty);
    w.opt_i32(f::MIN_COMPETE_SIZE, o.min_compete_size);
    w.opt_f64(
        f::COMPETE_AGAINST_BEST_OFFSET,
        o.compete_against_best_offset,
    );
    w.opt_f64(f::MID_OFFSET_AT_WHOLE, o.mid_offset_at_whole);
    w.opt_f64(f::MID_OFFSET_AT_HALF, o.mid_offset_at_half);
    w.opt_str(f::CUSTOMER_ACCOUNT, &o.customer_account);
    w.opt_bool(f::PROFESSIONAL_CUSTOMER, o.professional_customer);
    w.opt_str(f::BOND_ACCRUED_INTEREST, &o.bond_accrued_interest);
    w.opt_bool(f::INCLUDE_OVERNIGHT, o.include_overnight);
    w.opt_i32(f::MANUAL_ORDER_INDICATOR, o.manual_order_indicator);
    w.opt_str(f::SUBMITTER, &o.submitter);
    w.opt_bool(f::AUTO_CANCEL_PARENT, o.auto_cancel_parent);
    w.opt_bool(f::IMBALANCE_ONLY, o.imbalance_only);
    w.opt_bool(f::POST_ONLY, o.post_only);
    w.opt_bool(f::ALLOW_PRE_OPEN, o.allow_pre_open);
    w.opt_bool(f::IGNORE_OPEN_AUCTION, o.ignore_open_auction);
    w.opt_bool(f::DEACTIVATE, o.deactivate);
    opt_flag(w, f::SEEK_PRICE_IMPROVEMENT, o.seek_price_improvement);
    w.opt_i32(f::WHAT_IF_TYPE, o.what_if_type);
    opt_flag(w, f::ROUTE_MARKETABLE_TO_BBO, o.route_marketable_to_bbo);
}

fn order_cancel_pb(w: &mut PbWriter, c: &OrderCancel) {
    use order_cancel as f;
    w.opt_str(f::MANUAL_ORDER_CANCEL_TIME, &c.manual_order_cancel_time);
    w.opt_str(f::EXT_OPERATOR, &c.ext_operator);
    w.opt_i32(f::MANUAL_ORDER_INDICATOR, c.manual_order_indicator);
}

// ---- orders -------------------------------------------------------------------

pub fn place_order(v: i32, order_id: i32, contract: &Contract, order: &Order) -> Option<Vec<u8>> {
    use place_order_request as f;
    frame(v, sv::PROTOBUF_PLACE_ORDER, out::PLACE_ORDER, |w| {
        w.opt_i32(f::ORDER_ID, order_id);
        w.msg(f::CONTRACT, |w| contract_pb(w, contract, Some(order)));
        w.msg(f::ORDER, |w| order_pb(w, order));
        w.msg(f::ATTACHED_ORDERS, |w| {
            use attached_orders as a;
            w.opt_i32(a::SL_ORDER_ID, order.sl_order_id);
            w.opt_str(a::SL_ORDER_TYPE, &order.sl_order_type);
            w.opt_i32(a::PT_ORDER_ID, order.pt_order_id);
            w.opt_str(a::PT_ORDER_TYPE, &order.pt_order_type);
        });
    })
}

pub fn cancel_order(v: i32, order_id: i32, order_cancel: &OrderCancel) -> Option<Vec<u8>> {
    use cancel_order_request as f;
    frame(v, sv::PROTOBUF_PLACE_ORDER, out::CANCEL_ORDER, |w| {
        w.opt_i32(f::ORDER_ID, order_id);
        w.msg(f::ORDER_CANCEL, |w| order_cancel_pb(w, order_cancel));
    })
}

pub fn req_global_cancel(v: i32, order_cancel: &OrderCancel) -> Option<Vec<u8>> {
    frame(v, sv::PROTOBUF_PLACE_ORDER, out::REQ_GLOBAL_CANCEL, |w| {
        w.msg(global_cancel_request::ORDER_CANCEL, |w| {
            order_cancel_pb(w, order_cancel)
        });
    })
}

pub fn req_executions(v: i32, req_id: i32, filter: &ExecutionFilter) -> Option<Vec<u8>> {
    frame(v, sv::PROTOBUF, out::REQ_EXECUTIONS, |w| {
        w.opt_i32(execution_request::REQ_ID, req_id);
        w.msg(execution_request::EXECUTION_FILTER, |w| {
            use execution_filter as f;
            w.opt_i32(f::CLIENT_ID, filter.client_id);
            w.opt_str(f::ACCT_CODE, &filter.acct_code);
            w.opt_str(f::TIME, &filter.time);
            w.opt_str(f::SYMBOL, &filter.symbol);
            w.opt_str(f::SEC_TYPE, &filter.sec_type);
            w.opt_str(f::EXCHANGE, &filter.exchange);
            w.opt_str(f::SIDE, &filter.side);
            w.opt_i32(f::LAST_N_DAYS, filter.last_n_days);
            w.packed_i32(f::SPECIFIC_DATES, &filter.specific_dates);
        });
    })
}

pub fn req_all_open_orders(v: i32) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_COMPLETED_ORDER,
        out::REQ_ALL_OPEN_ORDERS,
        |_| {},
    )
}

pub fn req_auto_open_orders(v: i32, auto_bind: bool) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_COMPLETED_ORDER,
        out::REQ_AUTO_OPEN_ORDERS,
        |w| w.opt_bool(auto_open_orders_request::AUTO_BIND, auto_bind),
    )
}

pub fn req_open_orders(v: i32) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_COMPLETED_ORDER,
        out::REQ_OPEN_ORDERS,
        |_| {},
    )
}

pub fn req_completed_orders(v: i32, api_only: bool) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_COMPLETED_ORDER,
        out::REQ_COMPLETED_ORDERS,
        |w| w.opt_bool(completed_orders_request::API_ONLY, api_only),
    )
}

pub fn exercise_options(
    v: i32,
    req_id: i32,
    contract: &Contract,
    exercise_action: i32,
    exercise_quantity: i32,
    account: &str,
    override_: i32,
    manual_order_time: &str,
    customer_account: &str,
    professional_customer: bool,
) -> Option<Vec<u8>> {
    use exercise_options_request as f;
    frame(
        v,
        sv::PROTOBUF_REST_MESSAGES_1,
        out::EXERCISE_OPTIONS,
        |w| {
            w.opt_i32(f::ORDER_ID, req_id);
            w.msg(f::CONTRACT, |w| contract_pb(w, contract, None));
            w.opt_i32(f::EXERCISE_ACTION, exercise_action);
            w.opt_i32(f::EXERCISE_QUANTITY, exercise_quantity);
            w.opt_str(f::ACCOUNT, account);
            w.opt_bool(f::OVERRIDE, override_ != 0);
            w.opt_str(f::MANUAL_ORDER_TIME, manual_order_time);
            w.opt_str(f::CUSTOMER_ACCOUNT, customer_account);
            w.opt_bool(f::PROFESSIONAL_CUSTOMER, professional_customer);
        },
    )
}

// ---- contracts and market data ---------------------------------------------------

pub fn req_contract_details(v: i32, req_id: i32, contract: &Contract) -> Option<Vec<u8>> {
    frame(v, sv::PROTOBUF_CONTRACT_DATA, out::REQ_CONTRACT_DATA, |w| {
        w.opt_i32(contract_data_request::REQ_ID, req_id);
        w.msg(contract_data_request::CONTRACT, |w| {
            contract_pb(w, contract, None)
        });
    })
}

pub fn cancel_contract_data(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::CANCEL_CONTRACT_DATA,
        out::CANCEL_CONTRACT_DATA,
        cancel_contract_data::REQ_ID,
        req_id,
    )
}

pub fn req_mkt_data(
    v: i32,
    req_id: i32,
    contract: &Contract,
    generic_tick_list: &str,
    snapshot: bool,
    regulatory_snapshot: bool,
    options: &[TagValue],
) -> Option<Vec<u8>> {
    use market_data_request as f;
    frame(v, sv::PROTOBUF_MARKET_DATA, out::REQ_MKT_DATA, |w| {
        w.opt_i32(f::REQ_ID, req_id);
        w.msg(f::CONTRACT, |w| contract_pb(w, contract, None));
        w.opt_str(f::GENERIC_TICK_LIST, generic_tick_list);
        w.opt_bool(f::SNAPSHOT, snapshot);
        w.opt_bool(f::REGULATORY_SNAPSHOT, regulatory_snapshot);
        w.map(f::MARKET_DATA_OPTIONS, options);
    })
}

pub fn cancel_mkt_data(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_MARKET_DATA,
        out::CANCEL_MKT_DATA,
        cancel_market_data::REQ_ID,
        req_id,
    )
}

pub fn req_mkt_depth(
    v: i32,
    req_id: i32,
    contract: &Contract,
    num_rows: i32,
    is_smart_depth: bool,
    options: &[TagValue],
) -> Option<Vec<u8>> {
    use market_depth_request as f;
    frame(v, sv::PROTOBUF_MARKET_DATA, out::REQ_MKT_DEPTH, |w| {
        w.opt_i32(f::REQ_ID, req_id);
        w.msg(f::CONTRACT, |w| contract_pb(w, contract, None));
        w.opt_i32(f::NUM_ROWS, num_rows);
        w.opt_bool(f::IS_SMART_DEPTH, is_smart_depth);
        w.map(f::MARKET_DEPTH_OPTIONS, options);
    })
}

pub fn cancel_mkt_depth(v: i32, req_id: i32, is_smart_depth: bool) -> Option<Vec<u8>> {
    frame(v, sv::PROTOBUF_MARKET_DATA, out::CANCEL_MKT_DEPTH, |w| {
        w.opt_i32(cancel_market_depth::REQ_ID, req_id);
        w.opt_bool(cancel_market_depth::IS_SMART_DEPTH, is_smart_depth);
    })
}

pub fn req_market_data_type(v: i32, market_data_type: i32) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_MARKET_DATA,
        out::REQ_MARKET_DATA_TYPE,
        |w| w.opt_i32(market_data_type_request::MARKET_DATA_TYPE, market_data_type),
    )
}

// ---- accounts and positions ----------------------------------------------------------

pub fn req_account_updates(v: i32, subscribe: bool, acct_code: &str) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_ACCOUNTS_POSITIONS,
        out::REQ_ACCT_DATA,
        |w| {
            w.opt_bool(account_data_request::SUBSCRIBE, subscribe);
            w.opt_str(account_data_request::ACCT_CODE, acct_code);
        },
    )
}

pub fn req_managed_accts(v: i32) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_ACCOUNTS_POSITIONS,
        out::REQ_MANAGED_ACCTS,
        |_| {},
    )
}

pub fn req_positions(v: i32) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_ACCOUNTS_POSITIONS,
        out::REQ_POSITIONS,
        |_| {},
    )
}

pub fn cancel_positions(v: i32) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_ACCOUNTS_POSITIONS,
        out::CANCEL_POSITIONS,
        |_| {},
    )
}

pub fn req_account_summary(v: i32, req_id: i32, group_name: &str, tags: &str) -> Option<Vec<u8>> {
    use account_summary_request as f;
    frame(
        v,
        sv::PROTOBUF_ACCOUNTS_POSITIONS,
        out::REQ_ACCOUNT_SUMMARY,
        |w| {
            w.opt_i32(f::REQ_ID, req_id);
            w.opt_str(f::GROUP, group_name);
            w.opt_str(f::TAGS, tags);
        },
    )
}

pub fn cancel_account_summary(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_ACCOUNTS_POSITIONS,
        out::CANCEL_ACCOUNT_SUMMARY,
        cancel_account_summary::REQ_ID,
        req_id,
    )
}

pub fn req_positions_multi(
    v: i32,
    req_id: i32,
    account: &str,
    model_code: &str,
) -> Option<Vec<u8>> {
    use positions_multi_request as f;
    frame(
        v,
        sv::PROTOBUF_ACCOUNTS_POSITIONS,
        out::REQ_POSITIONS_MULTI,
        |w| {
            w.opt_i32(f::REQ_ID, req_id);
            w.opt_str(f::ACCOUNT, account);
            w.opt_str(f::MODEL_CODE, model_code);
        },
    )
}

pub fn cancel_positions_multi(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_ACCOUNTS_POSITIONS,
        out::CANCEL_POSITIONS_MULTI,
        cancel_positions_multi::REQ_ID,
        req_id,
    )
}

pub fn req_account_updates_multi(
    v: i32,
    req_id: i32,
    account: &str,
    model_code: &str,
    ledger_and_nlv: bool,
) -> Option<Vec<u8>> {
    use account_updates_multi_request as f;
    frame(
        v,
        sv::PROTOBUF_ACCOUNTS_POSITIONS,
        out::REQ_ACCOUNT_UPDATES_MULTI,
        |w| {
            w.opt_i32(f::REQ_ID, req_id);
            w.opt_str(f::ACCOUNT, account);
            w.opt_str(f::MODEL_CODE, model_code);
            w.opt_bool(f::LEDGER_AND_NLV, ledger_and_nlv);
        },
    )
}

pub fn cancel_account_updates_multi(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_ACCOUNTS_POSITIONS,
        out::CANCEL_ACCOUNT_UPDATES_MULTI,
        cancel_account_updates_multi::REQ_ID,
        req_id,
    )
}

// ---- historical data -------------------------------------------------------------------

pub fn req_historical_data(
    v: i32,
    req_id: i32,
    contract: &Contract,
    end_date_time: &str,
    duration_str: &str,
    bar_size_setting: &str,
    what_to_show: &str,
    use_rth: bool,
    format_date: i32,
    keep_up_to_date: bool,
    chart_options: &[TagValue],
) -> Option<Vec<u8>> {
    use historical_data_request as f;
    frame(
        v,
        sv::PROTOBUF_HISTORICAL_DATA,
        out::REQ_HISTORICAL_DATA,
        |w| {
            w.opt_i32(f::REQ_ID, req_id);
            w.msg(f::CONTRACT, |w| contract_pb(w, contract, None));
            w.opt_str(f::END_DATE_TIME, end_date_time);
            w.opt_str(f::DURATION, duration_str);
            w.opt_str(f::BAR_SIZE_SETTING, bar_size_setting);
            w.opt_str(f::WHAT_TO_SHOW, what_to_show);
            w.opt_bool(f::USE_RTH, use_rth);
            w.opt_i32(f::FORMAT_DATE, format_date);
            w.opt_bool(f::KEEP_UP_TO_DATE, keep_up_to_date);
            w.map(f::CHART_OPTIONS, chart_options);
        },
    )
}

pub fn cancel_historical_data(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_HISTORICAL_DATA,
        out::CANCEL_HISTORICAL_DATA,
        cancel_historical_data::REQ_ID,
        req_id,
    )
}

pub fn req_real_time_bars(
    v: i32,
    req_id: i32,
    contract: &Contract,
    bar_size: i32,
    what_to_show: &str,
    use_rth: bool,
    options: &[TagValue],
) -> Option<Vec<u8>> {
    use real_time_bars_request as f;
    frame(
        v,
        sv::PROTOBUF_HISTORICAL_DATA,
        out::REQ_REAL_TIME_BARS,
        |w| {
            w.opt_i32(f::REQ_ID, req_id);
            w.msg(f::CONTRACT, |w| contract_pb(w, contract, None));
            w.opt_i32(f::BAR_SIZE, bar_size);
            w.opt_str(f::WHAT_TO_SHOW, what_to_show);
            w.opt_bool(f::USE_RTH, use_rth);
            w.map(f::REAL_TIME_BARS_OPTIONS, options);
        },
    )
}

pub fn cancel_real_time_bars(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_HISTORICAL_DATA,
        out::CANCEL_REAL_TIME_BARS,
        cancel_real_time_bars::REQ_ID,
        req_id,
    )
}

pub fn req_head_time_stamp(
    v: i32,
    req_id: i32,
    contract: &Contract,
    what_to_show: &str,
    use_rth: bool,
    format_date: i32,
) -> Option<Vec<u8>> {
    use head_timestamp_request as f;
    frame(
        v,
        sv::PROTOBUF_HISTORICAL_DATA,
        out::REQ_HEAD_TIMESTAMP,
        |w| {
            w.opt_i32(f::REQ_ID, req_id);
            w.msg(f::CONTRACT, |w| contract_pb(w, contract, None));
            w.opt_str(f::WHAT_TO_SHOW, what_to_show);
            w.opt_bool(f::USE_RTH, use_rth);
            w.opt_i32(f::FORMAT_DATE, format_date);
        },
    )
}

pub fn cancel_head_time_stamp(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_HISTORICAL_DATA,
        out::CANCEL_HEAD_TIMESTAMP,
        cancel_head_timestamp::REQ_ID,
        req_id,
    )
}

pub fn req_histogram_data(
    v: i32,
    req_id: i32,
    contract: &Contract,
    use_rth: bool,
    time_period: &str,
) -> Option<Vec<u8>> {
    use histogram_data_request as f;
    frame(
        v,
        sv::PROTOBUF_HISTORICAL_DATA,
        out::REQ_HISTOGRAM_DATA,
        |w| {
            w.opt_i32(f::REQ_ID, req_id);
            w.msg(f::CONTRACT, |w| contract_pb(w, contract, None));
            w.opt_bool(f::USE_RTH, use_rth);
            w.opt_str(f::TIME_PERIOD, time_period);
        },
    )
}

pub fn cancel_histogram_data(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_HISTORICAL_DATA,
        out::CANCEL_HISTOGRAM_DATA,
        cancel_histogram_data::REQ_ID,
        req_id,
    )
}

pub fn req_historical_ticks(
    v: i32,
    req_id: i32,
    contract: &Contract,
    start_date_time: &str,
    end_date_time: &str,
    number_of_ticks: i32,
    what_to_show: &str,
    use_rth: bool,
    ignore_size: bool,
    misc_options: &[TagValue],
) -> Option<Vec<u8>> {
    use historical_ticks_request as f;
    frame(
        v,
        sv::PROTOBUF_HISTORICAL_DATA,
        out::REQ_HISTORICAL_TICKS,
        |w| {
            w.opt_i32(f::REQ_ID, req_id);
            w.msg(f::CONTRACT, |w| contract_pb(w, contract, None));
            w.opt_str(f::START_DATE_TIME, start_date_time);
            w.opt_str(f::END_DATE_TIME, end_date_time);
            w.opt_i32(f::NUMBER_OF_TICKS, number_of_ticks);
            w.opt_str(f::WHAT_TO_SHOW, what_to_show);
            w.opt_bool(f::USE_RTH, use_rth);
            w.opt_bool(f::IGNORE_SIZE, ignore_size);
            w.map(f::MISC_OPTIONS, misc_options);
        },
    )
}

pub fn cancel_historical_ticks(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::CANCEL_CONTRACT_DATA,
        out::CANCEL_HISTORICAL_TICKS,
        cancel_historical_ticks::REQ_ID,
        req_id,
    )
}

pub fn req_tick_by_tick_data(
    v: i32,
    req_id: i32,
    contract: &Contract,
    tick_type: &str,
    number_of_ticks: i32,
    ignore_size: bool,
) -> Option<Vec<u8>> {
    use tick_by_tick_request as f;
    frame(
        v,
        sv::PROTOBUF_HISTORICAL_DATA,
        out::REQ_TICK_BY_TICK_DATA,
        |w| {
            w.opt_i32(f::REQ_ID, req_id);
            w.msg(f::CONTRACT, |w| contract_pb(w, contract, None));
            w.opt_str(f::TICK_TYPE, tick_type);
            w.opt_i32(f::NUMBER_OF_TICKS, number_of_ticks);
            w.opt_bool(f::IGNORE_SIZE, ignore_size);
        },
    )
}

pub fn cancel_tick_by_tick_data(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_HISTORICAL_DATA,
        out::CANCEL_TICK_BY_TICK_DATA,
        cancel_tick_by_tick::REQ_ID,
        req_id,
    )
}

// ---- news and WSH ----------------------------------------------------------------------

pub fn req_news_bulletins(v: i32, all_msgs: bool) -> Option<Vec<u8>> {
    frame(v, sv::PROTOBUF_NEWS_DATA, out::REQ_NEWS_BULLETINS, |w| {
        w.opt_bool(news_bulletins_request::ALL_MESSAGES, all_msgs)
    })
}

pub fn cancel_news_bulletins(v: i32) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_NEWS_DATA,
        out::CANCEL_NEWS_BULLETINS,
        |_| {},
    )
}

pub fn req_news_article(
    v: i32,
    req_id: i32,
    provider_code: &str,
    article_id: &str,
    options: &[TagValue],
) -> Option<Vec<u8>> {
    use news_article_request as f;
    frame(v, sv::PROTOBUF_NEWS_DATA, out::REQ_NEWS_ARTICLE, |w| {
        w.opt_i32(f::REQ_ID, req_id);
        w.opt_str(f::PROVIDER_CODE, provider_code);
        w.opt_str(f::ARTICLE_ID, article_id);
        w.map(f::NEWS_ARTICLE_OPTIONS, options);
    })
}

pub fn req_news_providers(v: i32) -> Option<Vec<u8>> {
    frame(v, sv::PROTOBUF_NEWS_DATA, out::REQ_NEWS_PROVIDERS, |_| {})
}

pub fn req_historical_news(
    v: i32,
    req_id: i32,
    con_id: i32,
    provider_codes: &str,
    start_date_time: &str,
    end_date_time: &str,
    total_results: i32,
    options: &[TagValue],
) -> Option<Vec<u8>> {
    use historical_news_request as f;
    frame(v, sv::PROTOBUF_NEWS_DATA, out::REQ_HISTORICAL_NEWS, |w| {
        w.opt_i32(f::REQ_ID, req_id);
        w.opt_i32(f::CON_ID, con_id);
        w.opt_str(f::PROVIDER_CODES, provider_codes);
        w.opt_str(f::START_DATE_TIME, start_date_time);
        w.opt_str(f::END_DATE_TIME, end_date_time);
        w.opt_i32(f::TOTAL_RESULTS, total_results);
        w.map(f::HISTORICAL_NEWS_OPTIONS, options);
    })
}

pub fn req_wsh_meta_data(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_NEWS_DATA,
        out::REQ_WSH_META_DATA,
        wsh_meta_data_request::REQ_ID,
        req_id,
    )
}

pub fn cancel_wsh_meta_data(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_NEWS_DATA,
        out::CANCEL_WSH_META_DATA,
        cancel_wsh_meta_data::REQ_ID,
        req_id,
    )
}

pub fn req_wsh_event_data(v: i32, req_id: i32, d: &WshEventData) -> Option<Vec<u8>> {
    use wsh_event_data_request as f;
    frame(v, sv::PROTOBUF_NEWS_DATA, out::REQ_WSH_EVENT_DATA, |w| {
        w.opt_i32(f::REQ_ID, req_id);
        w.opt_i32(f::CON_ID, d.con_id);
        w.opt_str(f::FILTER, &d.filter);
        w.opt_bool(f::FILL_WATCHLIST, d.fill_watchlist);
        w.opt_bool(f::FILL_PORTFOLIO, d.fill_portfolio);
        w.opt_bool(f::FILL_COMPETITORS, d.fill_competitors);
        w.opt_str(f::START_DATE, &d.start_date);
        w.opt_str(f::END_DATE, &d.end_date);
        w.opt_i32(f::TOTAL_LIMIT, d.total_limit);
    })
}

pub fn cancel_wsh_event_data(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_NEWS_DATA,
        out::CANCEL_WSH_EVENT_DATA,
        cancel_wsh_event_data::REQ_ID,
        req_id,
    )
}

// ---- scanner, fundamentals, PnL ------------------------------------------------------

pub fn req_scanner_parameters(v: i32) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_SCAN_DATA,
        out::REQ_SCANNER_PARAMETERS,
        |_| {},
    )
}

pub fn req_scanner_subscription(
    v: i32,
    req_id: i32,
    s: &ScannerSubscription,
    options: &[TagValue],
    filter_options: &[TagValue],
) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_SCAN_DATA,
        out::REQ_SCANNER_SUBSCRIPTION,
        |w| {
            w.opt_i32(scanner_subscription_request::REQ_ID, req_id);
            w.msg(scanner_subscription_request::SCANNER_SUBSCRIPTION, |w| {
                use scanner_subscription as f;
                w.opt_i32(f::NUMBER_OF_ROWS, s.number_of_rows);
                w.opt_str(f::INSTRUMENT, &s.instrument);
                w.opt_str(f::LOCATION_CODE, &s.location_code);
                w.opt_str(f::SCAN_CODE, &s.scan_code);
                w.opt_f64(f::ABOVE_PRICE, s.above_price);
                w.opt_f64(f::BELOW_PRICE, s.below_price);
                if s.above_volume != UNSET_INTEGER {
                    w.i64(f::ABOVE_VOLUME, i64::from(s.above_volume));
                }
                if s.average_option_volume_above != UNSET_INTEGER {
                    w.i64(
                        f::AVERAGE_OPTION_VOLUME_ABOVE,
                        i64::from(s.average_option_volume_above),
                    );
                }
                w.opt_f64(f::MARKET_CAP_ABOVE, s.market_cap_above);
                w.opt_f64(f::MARKET_CAP_BELOW, s.market_cap_below);
                w.opt_str(f::MOODY_RATING_ABOVE, &s.moody_rating_above);
                w.opt_str(f::MOODY_RATING_BELOW, &s.moody_rating_below);
                w.opt_str(f::SP_RATING_ABOVE, &s.sp_rating_above);
                w.opt_str(f::SP_RATING_BELOW, &s.sp_rating_below);
                w.opt_str(f::MATURITY_DATE_ABOVE, &s.maturity_date_above);
                w.opt_str(f::MATURITY_DATE_BELOW, &s.maturity_date_below);
                w.opt_f64(f::COUPON_RATE_ABOVE, s.coupon_rate_above);
                w.opt_f64(f::COUPON_RATE_BELOW, s.coupon_rate_below);
                w.opt_bool(f::EXCLUDE_CONVERTIBLE, s.exclude_convertible);
                w.opt_str(f::SCANNER_SETTING_PAIRS, &s.scanner_setting_pairs);
                w.opt_str(f::STOCK_TYPE_FILTER, &s.stock_type_filter);
                w.map(f::SCANNER_SUBSCRIPTION_OPTIONS, options);
                w.map(f::SCANNER_SUBSCRIPTION_FILTER_OPTIONS, filter_options);
            });
        },
    )
}

pub fn cancel_scanner_subscription(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_SCAN_DATA,
        out::CANCEL_SCANNER_SUBSCRIPTION,
        cancel_scanner_subscription::REQ_ID,
        req_id,
    )
}

pub fn req_fundamental_data(
    v: i32,
    req_id: i32,
    contract: &Contract,
    report_type: &str,
    options: &[TagValue],
) -> Option<Vec<u8>> {
    use fundamentals_data_request as f;
    frame(v, sv::PROTOBUF_SCAN_DATA, out::REQ_FUNDAMENTAL_DATA, |w| {
        w.opt_i32(f::REQ_ID, req_id);
        w.msg(f::CONTRACT, |w| contract_pb(w, contract, None));
        w.opt_str(f::REPORT_TYPE, report_type);
        w.map(f::FUNDAMENTALS_DATA_OPTIONS, options);
    })
}

pub fn cancel_fundamental_data(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_SCAN_DATA,
        out::CANCEL_FUNDAMENTAL_DATA,
        cancel_fundamentals_data::REQ_ID,
        req_id,
    )
}

pub fn req_pnl(v: i32, req_id: i32, account: &str, model_code: &str) -> Option<Vec<u8>> {
    use pn_l_request as f;
    frame(v, sv::PROTOBUF_SCAN_DATA, out::REQ_PNL, |w| {
        w.opt_i32(f::REQ_ID, req_id);
        w.opt_str(f::ACCOUNT, account);
        w.opt_str(f::MODEL_CODE, model_code);
    })
}

pub fn cancel_pnl(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_SCAN_DATA,
        out::CANCEL_PNL,
        cancel_pn_l::REQ_ID,
        req_id,
    )
}

pub fn req_pnl_single(
    v: i32,
    req_id: i32,
    account: &str,
    model_code: &str,
    con_id: i32,
) -> Option<Vec<u8>> {
    use pn_l_single_request as f;
    frame(v, sv::PROTOBUF_SCAN_DATA, out::REQ_PNL_SINGLE, |w| {
        w.opt_i32(f::REQ_ID, req_id);
        w.opt_str(f::ACCOUNT, account);
        w.opt_str(f::MODEL_CODE, model_code);
        w.opt_i32(f::CON_ID, con_id);
    })
}

pub fn cancel_pnl_single(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_SCAN_DATA,
        out::CANCEL_PNL_SINGLE,
        cancel_pn_l_single::REQ_ID,
        req_id,
    )
}

// ---- FA and option calculations ------------------------------------------------------

pub fn request_fa(v: i32, fa_data_type: i32) -> Option<Vec<u8>> {
    frame(v, sv::PROTOBUF_REST_MESSAGES_1, out::REQ_FA, |w| {
        w.opt_i32(fa_request::FA_DATA_TYPE, fa_data_type)
    })
}

pub fn replace_fa(v: i32, req_id: i32, fa_data_type: i32, xml: &str) -> Option<Vec<u8>> {
    frame(v, sv::PROTOBUF_REST_MESSAGES_1, out::REPLACE_FA, |w| {
        w.opt_i32(fa_replace::REQ_ID, req_id);
        w.opt_i32(fa_replace::FA_DATA_TYPE, fa_data_type);
        w.opt_str(fa_replace::XML, xml);
    })
}

pub fn calculate_implied_volatility(
    v: i32,
    req_id: i32,
    contract: &Contract,
    option_price: f64,
    under_price: f64,
    options: &[TagValue],
) -> Option<Vec<u8>> {
    use calculate_implied_volatility_request as f;
    frame(
        v,
        sv::PROTOBUF_REST_MESSAGES_1,
        out::REQ_CALC_IMPLIED_VOLAT,
        |w| {
            w.opt_i32(f::REQ_ID, req_id);
            w.msg(f::CONTRACT, |w| contract_pb(w, contract, None));
            w.opt_f64(f::OPTION_PRICE, option_price);
            w.opt_f64(f::UNDER_PRICE, under_price);
            w.map(f::IMPLIED_VOLATILITY_OPTIONS, options);
        },
    )
}

pub fn cancel_calculate_implied_volatility(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_REST_MESSAGES_1,
        out::CANCEL_CALC_IMPLIED_VOLAT,
        cancel_calculate_implied_volatility::REQ_ID,
        req_id,
    )
}

pub fn calculate_option_price(
    v: i32,
    req_id: i32,
    contract: &Contract,
    volatility: f64,
    under_price: f64,
    options: &[TagValue],
) -> Option<Vec<u8>> {
    use calculate_option_price_request as f;
    frame(
        v,
        sv::PROTOBUF_REST_MESSAGES_1,
        out::REQ_CALC_OPTION_PRICE,
        |w| {
            w.opt_i32(f::REQ_ID, req_id);
            w.msg(f::CONTRACT, |w| contract_pb(w, contract, None));
            w.opt_f64(f::VOLATILITY, volatility);
            w.opt_f64(f::UNDER_PRICE, under_price);
            w.map(f::OPTION_PRICE_OPTIONS, options);
        },
    )
}

pub fn cancel_calculate_option_price(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_REST_MESSAGES_1,
        out::CANCEL_CALC_OPTION_PRICE,
        cancel_calculate_option_price::REQ_ID,
        req_id,
    )
}

// ---- reference data --------------------------------------------------------------------

pub fn req_sec_def_opt_params(
    v: i32,
    req_id: i32,
    underlying_symbol: &str,
    fut_fop_exchange: &str,
    underlying_sec_type: &str,
    underlying_con_id: i32,
) -> Option<Vec<u8>> {
    use sec_def_opt_params_request as f;
    frame(
        v,
        sv::PROTOBUF_REST_MESSAGES_2,
        out::REQ_SEC_DEF_OPT_PARAMS,
        |w| {
            w.opt_i32(f::REQ_ID, req_id);
            w.opt_str(f::UNDERLYING_SYMBOL, underlying_symbol);
            w.opt_str(f::FUT_FOP_EXCHANGE, fut_fop_exchange);
            w.opt_str(f::UNDERLYING_SEC_TYPE, underlying_sec_type);
            w.opt_i32(f::UNDERLYING_CON_ID, underlying_con_id);
        },
    )
}

pub fn req_soft_dollar_tiers(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_REST_MESSAGES_2,
        out::REQ_SOFT_DOLLAR_TIERS,
        soft_dollar_tiers_request::REQ_ID,
        req_id,
    )
}

pub fn req_family_codes(v: i32) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_REST_MESSAGES_2,
        out::REQ_FAMILY_CODES,
        |_| {},
    )
}

pub fn req_matching_symbols(v: i32, req_id: i32, pattern: &str) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_REST_MESSAGES_2,
        out::REQ_MATCHING_SYMBOLS,
        |w| {
            w.opt_i32(matching_symbols_request::REQ_ID, req_id);
            w.opt_str(matching_symbols_request::PATTERN, pattern);
        },
    )
}

pub fn req_smart_components(v: i32, req_id: i32, bbo_exchange: &str) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_REST_MESSAGES_2,
        out::REQ_SMART_COMPONENTS,
        |w| {
            w.opt_i32(smart_components_request::REQ_ID, req_id);
            w.opt_str(smart_components_request::BBO_EXCHANGE, bbo_exchange);
        },
    )
}

pub fn req_market_rule(v: i32, market_rule_id: i32) -> Option<Vec<u8>> {
    frame(v, sv::PROTOBUF_REST_MESSAGES_2, out::REQ_MARKET_RULE, |w| {
        w.opt_i32(market_rule_request::MARKET_RULE_ID, market_rule_id)
    })
}

pub fn req_user_info(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_REST_MESSAGES_2,
        out::REQ_USER_INFO,
        user_info_request::REQ_ID,
        req_id,
    )
}

// ---- session and display groups -----------------------------------------------------

pub fn req_ids(v: i32, num_ids: i32) -> Option<Vec<u8>> {
    frame(v, sv::PROTOBUF_REST_MESSAGES_3, out::REQ_IDS, |w| {
        w.opt_i32(ids_request::NUM_IDS, num_ids)
    })
}

pub fn req_current_time(v: i32) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_REST_MESSAGES_3,
        out::REQ_CURRENT_TIME,
        |_| {},
    )
}

pub fn req_current_time_in_millis(v: i32) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_REST_MESSAGES_3,
        out::REQ_CURRENT_TIME_IN_MILLIS,
        |_| {},
    )
}

pub fn start_api(v: i32, client_id: i32, optional_capabilities: &str) -> Option<Vec<u8>> {
    frame(v, sv::PROTOBUF_REST_MESSAGES_3, out::START_API, |w| {
        w.opt_i32(start_api_request::CLIENT_ID, client_id);
        w.opt_str(
            start_api_request::OPTIONAL_CAPABILITIES,
            optional_capabilities,
        );
    })
}

pub fn set_server_log_level(v: i32, log_level: i32) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_REST_MESSAGES_3,
        out::SET_SERVER_LOGLEVEL,
        |w| w.opt_i32(set_server_log_level_request::LOG_LEVEL, log_level),
    )
}

pub fn verify_request(v: i32, api_name: &str, api_version: &str) -> Option<Vec<u8>> {
    frame(v, sv::PROTOBUF_REST_MESSAGES_3, out::VERIFY_REQUEST, |w| {
        w.opt_str(verify_request::API_NAME, api_name);
        w.opt_str(verify_request::API_VERSION, api_version);
    })
}

pub fn verify_message(v: i32, api_data: &str) -> Option<Vec<u8>> {
    frame(v, sv::PROTOBUF_REST_MESSAGES_3, out::VERIFY_MESSAGE, |w| {
        w.opt_str(verify_message_request::API_DATA, api_data)
    })
}

pub fn query_display_groups(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_REST_MESSAGES_3,
        out::QUERY_DISPLAY_GROUPS,
        query_display_groups_request::REQ_ID,
        req_id,
    )
}

pub fn subscribe_to_group_events(v: i32, req_id: i32, group_id: i32) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_REST_MESSAGES_3,
        out::SUBSCRIBE_TO_GROUP_EVENTS,
        |w| {
            w.opt_i32(subscribe_to_group_events_request::REQ_ID, req_id);
            w.opt_i32(subscribe_to_group_events_request::GROUP_ID, group_id);
        },
    )
}

pub fn update_display_group(v: i32, req_id: i32, contract_info: &str) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_REST_MESSAGES_3,
        out::UPDATE_DISPLAY_GROUP,
        |w| {
            w.opt_i32(update_display_group_request::REQ_ID, req_id);
            w.opt_str(update_display_group_request::CONTRACT_INFO, contract_info);
        },
    )
}

pub fn unsubscribe_from_group_events(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::PROTOBUF_REST_MESSAGES_3,
        out::UNSUBSCRIBE_FROM_GROUP_EVENTS,
        unsubscribe_from_group_events_request::REQ_ID,
        req_id,
    )
}

pub fn req_mkt_depth_exchanges(v: i32) -> Option<Vec<u8>> {
    frame(
        v,
        sv::PROTOBUF_REST_MESSAGES_3,
        out::REQ_MKT_DEPTH_EXCHANGES,
        |_| {},
    )
}

// ---- configuration (protobuf only) ------------------------------------------------------

pub fn req_config(v: i32, req_id: i32) -> Option<Vec<u8>> {
    req_id_only(
        v,
        sv::CONFIG,
        out::REQ_CONFIG,
        config_request::REQ_ID,
        req_id,
    )
}

/// `UpdateConfigRequest` from its JSON form (field names as in IB's schema,
/// for example `{"reqId": 1, "api": {"settings": {...}}}`).
pub fn update_config(v: i32, request: &serde_json::Value) -> Option<Vec<u8>> {
    frame(v, sv::UPDATE_CONFIG, out::UPDATE_CONFIG, |w| {
        crate::pb::from_json(w, "UpdateConfigRequest", request)
    })
}
