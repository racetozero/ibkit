//! Protobuf decoders for server messages (server version 201 and later).
//!
//! The server sends a message as protobuf when its id is above 200 (the id
//! minus 200 is the usual message id). Each message is decoded into the same
//! [`Incoming`] value as its text form; absent fields take the defaults IB's
//! `decoder.py` uses.

use crate::consts::{NO_VALID_ID, UNSET_DOUBLE, UNSET_INTEGER};
use crate::decode::*;
use crate::message::incoming as msg;
use crate::pb::{PbMsg, to_json};
use crate::proto::*;
use crate::types::*;
use crate::wire::DecodeError;

impl PbMsg<'_> {
    fn set_i32(&self, f: u32, dst: &mut i32) {
        if let Some(v) = self.i32(f) {
            *dst = v;
        }
    }
    fn set_i64(&self, f: u32, dst: &mut i64) {
        if let Some(v) = self.i64(f) {
            *dst = v;
        }
    }
    fn set_f64(&self, f: u32, dst: &mut f64) {
        if let Some(v) = self.f64(f) {
            *dst = v;
        }
    }
    fn set_bool(&self, f: u32, dst: &mut bool) {
        if let Some(v) = self.bool(f) {
            *dst = v;
        }
    }
    fn set_str(&self, f: u32, dst: &mut String) {
        if let Some(v) = self.str(f) {
            *dst = v.to_string();
        }
    }
    /// Decimal sent as a string.
    fn set_decimal(&self, f: u32, dst: &mut f64) {
        if self.has(f) {
            *dst = self.decimal(f);
        }
    }
    /// int32 0/1 into an optional bool.
    fn set_opt_flag(&self, f: u32, dst: &mut Option<bool>) {
        if let Some(v) = self.i32(f) {
            *dst = Some(v != 0);
        }
    }
}

fn missing(what: &str) -> DecodeError {
    DecodeError::Protobuf(format!("{what} is missing"))
}

/// `floatMaxString`: shortest text of a double, empty for `UNSET_DOUBLE`.
fn float_text(v: f64) -> String {
    if v == UNSET_DOUBLE {
        return String::new();
    }
    let mut b = Vec::new();
    crate::wire::write_f64(&mut b, v);
    String::from_utf8(b).unwrap_or_default()
}

pub(crate) fn contract(m: &PbMsg<'_>) -> Contract {
    use contract as f;
    let mut c = Contract::default();
    m.set_i32(f::CON_ID, &mut c.con_id);
    m.set_str(f::SYMBOL, &mut c.symbol);
    m.set_str(f::SEC_TYPE, &mut c.sec_type);
    m.set_str(
        f::LAST_TRADE_DATE_OR_CONTRACT_MONTH,
        &mut c.last_trade_date_or_contract_month,
    );
    m.set_f64(f::STRIKE, &mut c.strike);
    m.set_str(f::RIGHT, &mut c.right);
    if let Some(x) = m.f64(f::MULTIPLIER) {
        c.multiplier = float_text(x);
    }
    m.set_str(f::EXCHANGE, &mut c.exchange);
    m.set_str(f::CURRENCY, &mut c.currency);
    m.set_str(f::LOCAL_SYMBOL, &mut c.local_symbol);
    m.set_str(f::TRADING_CLASS, &mut c.trading_class);
    m.set_str(f::COMBO_LEGS_DESCRIP, &mut c.combo_legs_descrip);
    c.combo_legs = m
        .msgs(f::COMBO_LEGS)
        .map(|l| {
            use combo_leg as g;
            let mut leg = ComboLeg::default();
            l.set_i32(g::CON_ID, &mut leg.con_id);
            l.set_i32(g::RATIO, &mut leg.ratio);
            l.set_str(g::ACTION, &mut leg.action);
            l.set_str(g::EXCHANGE, &mut leg.exchange);
            l.set_i32(g::OPEN_CLOSE, &mut leg.open_close);
            l.set_i32(g::SHORT_SALES_SLOT, &mut leg.short_sale_slot);
            l.set_str(g::DESIGNATED_LOCATION, &mut leg.designated_location);
            l.set_i32(g::EXEMPT_CODE, &mut leg.exempt_code);
            leg
        })
        .collect();
    if let Some(d) = m.msg(f::DELTA_NEUTRAL_CONTRACT) {
        let mut dn = DeltaNeutralContract::default();
        d.set_i32(delta_neutral_contract::CON_ID, &mut dn.con_id);
        d.set_f64(delta_neutral_contract::DELTA, &mut dn.delta);
        d.set_f64(delta_neutral_contract::PRICE, &mut dn.price);
        c.delta_neutral_contract = Some(dn);
    }
    m.set_str(f::LAST_TRADE_DATE, &mut c.last_trade_date);
    m.set_str(f::PRIMARY_EXCH, &mut c.primary_exchange);
    m.set_str(f::ISSUER_ID, &mut c.issuer_id);
    m.set_str(f::DESCRIPTION, &mut c.description);
    c
}

fn execution(m: &PbMsg<'_>) -> Execution {
    use execution as f;
    let mut e = Execution {
        opt_exercise_or_lapse_type: -1,
        ..Default::default()
    };
    m.set_i32(f::ORDER_ID, &mut e.order_id);
    m.set_i32(f::CLIENT_ID, &mut e.client_id);
    m.set_str(f::EXEC_ID, &mut e.exec_id);
    m.set_str(f::TIME, &mut e.time);
    m.set_str(f::ACCT_NUMBER, &mut e.acct_number);
    m.set_str(f::EXCHANGE, &mut e.exchange);
    m.set_str(f::SIDE, &mut e.side);
    m.set_decimal(f::SHARES, &mut e.shares);
    m.set_f64(f::PRICE, &mut e.price);
    m.set_i64(f::PERM_ID, &mut e.perm_id);
    if let Some(l) = m.bool(f::IS_LIQUIDATION) {
        e.liquidation = i32::from(l);
    }
    m.set_decimal(f::CUM_QTY, &mut e.cum_qty);
    m.set_f64(f::AVG_PRICE, &mut e.avg_price);
    m.set_str(f::ORDER_REF, &mut e.order_ref);
    m.set_str(f::EV_RULE, &mut e.ev_rule);
    m.set_f64(f::EV_MULTIPLIER, &mut e.ev_multiplier);
    m.set_str(f::MODEL_CODE, &mut e.model_code);
    m.set_i32(f::LAST_LIQUIDITY, &mut e.last_liquidity);
    m.set_bool(f::IS_PRICE_REVISION_PENDING, &mut e.pending_price_revision);
    m.set_str(f::SUBMITTER, &mut e.submitter);
    m.set_i32(
        f::OPT_EXERCISE_OR_LAPSE_TYPE,
        &mut e.opt_exercise_or_lapse_type,
    );
    e
}

fn condition(m: &PbMsg<'_>) -> Option<OrderCondition> {
    use order_condition as f;
    let conjunction = if m.flag(f::IS_CONJUNCTION_CONNECTION) {
        "a"
    } else {
        "o"
    }
    .to_string();
    let is_more = m.flag(f::IS_MORE);
    let con_id = m.int_or(f::CON_ID, 0);
    let exch = m.string(f::EXCHANGE);
    Some(match m.int_or(f::TYPE, 0) {
        1 => OrderCondition::Price {
            conjunction,
            is_more,
            price: m.f64(f::PRICE).unwrap_or(0.0),
            con_id,
            exch,
            trigger_method: m.int_or(f::TRIGGER_METHOD, 0),
        },
        3 => OrderCondition::Time {
            conjunction,
            is_more,
            time: m.string(f::TIME),
        },
        4 => OrderCondition::Margin {
            conjunction,
            is_more,
            percent: m.int_or(f::PERCENT, 0),
        },
        5 => OrderCondition::Execution {
            conjunction,
            sec_type: m.string(f::SEC_TYPE),
            exch,
            symbol: m.string(f::SYMBOL),
        },
        6 => OrderCondition::Volume {
            conjunction,
            is_more,
            volume: m.int_or(f::VOLUME, 0),
            con_id,
            exch,
        },
        7 => OrderCondition::PercentChange {
            conjunction,
            is_more,
            change_percent: m.f64(f::CHANGE_PERCENT).unwrap_or(0.0),
            con_id,
            exch,
        },
        _ => return None,
    })
}

fn order(order_id: i32, contract_msg: &PbMsg<'_>, m: &PbMsg<'_>) -> Order {
    use order as f;
    let mut o = Order::default();
    if order_id != UNSET_INTEGER {
        o.order_id = order_id;
    }
    m.set_i32(f::ORDER_ID, &mut o.order_id);
    m.set_str(f::ACTION, &mut o.action);
    m.set_decimal(f::TOTAL_QUANTITY, &mut o.total_quantity);
    m.set_str(f::ORDER_TYPE, &mut o.order_type);
    m.set_f64(f::LMT_PRICE, &mut o.lmt_price);
    m.set_f64(f::AUX_PRICE, &mut o.aux_price);
    m.set_str(f::TIF, &mut o.tif);
    m.set_str(f::OCA_GROUP, &mut o.oca_group);
    m.set_str(f::ACCOUNT, &mut o.account);
    m.set_str(f::OPEN_CLOSE, &mut o.open_close);
    m.set_i32(f::ORIGIN, &mut o.origin);
    m.set_str(f::ORDER_REF, &mut o.order_ref);
    m.set_i32(f::CLIENT_ID, &mut o.client_id);
    m.set_i64(f::PERM_ID, &mut o.perm_id);
    m.set_bool(f::OUTSIDE_RTH, &mut o.outside_rth);
    m.set_bool(f::HIDDEN, &mut o.hidden);
    m.set_f64(f::DISCRETIONARY_AMT, &mut o.discretionary_amt);
    m.set_str(f::GOOD_AFTER_TIME, &mut o.good_after_time);
    m.set_str(f::FA_GROUP, &mut o.fa_group);
    m.set_str(f::FA_METHOD, &mut o.fa_method);
    m.set_str(f::FA_PERCENTAGE, &mut o.fa_percentage);
    m.set_str(f::MODEL_CODE, &mut o.model_code);
    m.set_str(f::GOOD_TILL_DATE, &mut o.good_till_date);
    m.set_str(f::RULE80_A, &mut o.rule80_a);
    m.set_f64(f::PERCENT_OFFSET, &mut o.percent_offset);
    m.set_str(f::SETTLING_FIRM, &mut o.settling_firm);
    m.set_i32(f::SHORT_SALE_SLOT, &mut o.short_sale_slot);
    m.set_str(f::DESIGNATED_LOCATION, &mut o.designated_location);
    m.set_i32(f::EXEMPT_CODE, &mut o.exempt_code);
    m.set_f64(f::STARTING_PRICE, &mut o.starting_price);
    m.set_f64(f::STOCK_REF_PRICE, &mut o.stock_ref_price);
    m.set_f64(f::DELTA, &mut o.delta);
    m.set_f64(f::STOCK_RANGE_LOWER, &mut o.stock_range_lower);
    m.set_f64(f::STOCK_RANGE_UPPER, &mut o.stock_range_upper);
    m.set_i32(f::DISPLAY_SIZE, &mut o.display_size);
    m.set_bool(f::BLOCK_ORDER, &mut o.block_order);
    m.set_bool(f::SWEEP_TO_FILL, &mut o.sweep_to_fill);
    m.set_bool(f::ALL_OR_NONE, &mut o.all_or_none);
    m.set_i32(f::MIN_QTY, &mut o.min_qty);
    m.set_i32(f::OCA_TYPE, &mut o.oca_type);
    m.set_i32(f::PARENT_ID, &mut o.parent_id);
    m.set_i32(f::TRIGGER_METHOD, &mut o.trigger_method);
    m.set_f64(f::VOLATILITY, &mut o.volatility);
    m.set_i32(f::VOLATILITY_TYPE, &mut o.volatility_type);
    m.set_str(f::DELTA_NEUTRAL_ORDER_TYPE, &mut o.delta_neutral_order_type);
    m.set_f64(f::DELTA_NEUTRAL_AUX_PRICE, &mut o.delta_neutral_aux_price);
    m.set_i32(f::DELTA_NEUTRAL_CON_ID, &mut o.delta_neutral_con_id);
    m.set_str(
        f::DELTA_NEUTRAL_SETTLING_FIRM,
        &mut o.delta_neutral_settling_firm,
    );
    m.set_str(
        f::DELTA_NEUTRAL_CLEARING_ACCOUNT,
        &mut o.delta_neutral_clearing_account,
    );
    m.set_str(
        f::DELTA_NEUTRAL_CLEARING_INTENT,
        &mut o.delta_neutral_clearing_intent,
    );
    m.set_str(f::DELTA_NEUTRAL_OPEN_CLOSE, &mut o.delta_neutral_open_close);
    m.set_bool(f::DELTA_NEUTRAL_SHORT_SALE, &mut o.delta_neutral_short_sale);
    m.set_i32(
        f::DELTA_NEUTRAL_SHORT_SALE_SLOT,
        &mut o.delta_neutral_short_sale_slot,
    );
    m.set_str(
        f::DELTA_NEUTRAL_DESIGNATED_LOCATION,
        &mut o.delta_neutral_designated_location,
    );
    m.set_bool(f::CONTINUOUS_UPDATE, &mut o.continuous_update);
    m.set_i32(f::REFERENCE_PRICE_TYPE, &mut o.reference_price_type);
    m.set_f64(f::TRAIL_STOP_PRICE, &mut o.trail_stop_price);
    m.set_f64(f::TRAILING_PERCENT, &mut o.trailing_percent);
    o.order_combo_legs = contract_msg
        .msgs(contract::COMBO_LEGS)
        .map(|l| OrderComboLeg {
            price: l.double(combo_leg::PER_LEG_PRICE),
        })
        .collect();
    o.smart_combo_routing_params = m.map(f::SMART_COMBO_ROUTING_PARAMS);
    m.set_i32(f::SCALE_INIT_LEVEL_SIZE, &mut o.scale_init_level_size);
    m.set_i32(f::SCALE_SUBS_LEVEL_SIZE, &mut o.scale_subs_level_size);
    m.set_f64(f::SCALE_PRICE_INCREMENT, &mut o.scale_price_increment);
    m.set_f64(f::SCALE_PRICE_ADJUST_VALUE, &mut o.scale_price_adjust_value);
    m.set_i32(
        f::SCALE_PRICE_ADJUST_INTERVAL,
        &mut o.scale_price_adjust_interval,
    );
    m.set_f64(f::SCALE_PROFIT_OFFSET, &mut o.scale_profit_offset);
    m.set_bool(f::SCALE_AUTO_RESET, &mut o.scale_auto_reset);
    m.set_i32(f::SCALE_INIT_POSITION, &mut o.scale_init_position);
    m.set_i32(f::SCALE_INIT_FILL_QTY, &mut o.scale_init_fill_qty);
    m.set_bool(f::SCALE_RANDOM_PERCENT, &mut o.scale_random_percent);
    m.set_str(f::HEDGE_TYPE, &mut o.hedge_type);
    if !o.hedge_type.is_empty() {
        m.set_str(f::HEDGE_PARAM, &mut o.hedge_param);
    }
    if m.str(f::HEDGE_TYPE).is_some() {
        m.set_i32(f::HEDGE_MAX_SIZE, &mut o.hedge_max_size);
    }
    m.set_bool(f::OPT_OUT_SMART_ROUTING, &mut o.opt_out_smart_routing);
    m.set_str(f::CLEARING_ACCOUNT, &mut o.clearing_account);
    m.set_str(f::CLEARING_INTENT, &mut o.clearing_intent);
    m.set_bool(f::NOT_HELD, &mut o.not_held);
    if let Some(s) = m.str(f::ALGO_STRATEGY) {
        o.algo_strategy = s.to_string();
        o.algo_params = m.map(f::ALGO_PARAMS);
    }
    m.set_bool(f::SOLICITED, &mut o.solicited);
    m.set_bool(f::WHAT_IF, &mut o.what_if);
    m.set_bool(f::RANDOMIZE_SIZE, &mut o.randomize_size);
    m.set_bool(f::RANDOMIZE_PRICE, &mut o.randomize_price);
    m.set_i32(f::REFERENCE_CONTRACT_ID, &mut o.reference_contract_id);
    m.set_bool(
        f::IS_PEGGED_CHANGE_AMOUNT_DECREASE,
        &mut o.is_pegged_change_amount_decrease,
    );
    m.set_f64(f::PEGGED_CHANGE_AMOUNT, &mut o.pegged_change_amount);
    m.set_f64(f::REFERENCE_CHANGE_AMOUNT, &mut o.reference_change_amount);
    m.set_str(f::REFERENCE_EXCHANGE_ID, &mut o.reference_exchange_id);
    o.conditions = m
        .msgs(f::CONDITIONS)
        .filter_map(|c| condition(&c))
        .collect();
    m.set_bool(f::CONDITIONS_IGNORE_RTH, &mut o.conditions_ignore_rth);
    m.set_bool(f::CONDITIONS_CANCEL_ORDER, &mut o.conditions_cancel_order);
    m.set_bool(
        f::CONDITIONS_INCLUDE_OVERNIGHT,
        &mut o.conditions_include_overnight,
    );
    m.set_str(f::ADJUSTED_ORDER_TYPE, &mut o.adjusted_order_type);
    m.set_f64(f::TRIGGER_PRICE, &mut o.trigger_price);
    m.set_f64(f::LMT_PRICE_OFFSET, &mut o.lmt_price_offset);
    m.set_f64(f::ADJUSTED_STOP_PRICE, &mut o.adjusted_stop_price);
    m.set_f64(
        f::ADJUSTED_STOP_LIMIT_PRICE,
        &mut o.adjusted_stop_limit_price,
    );
    m.set_f64(f::ADJUSTED_TRAILING_AMOUNT, &mut o.adjusted_trailing_amount);
    m.set_i32(f::ADJUSTABLE_TRAILING_UNIT, &mut o.adjustable_trailing_unit);
    if let Some(t) = m.msg(f::SOFT_DOLLAR_TIER) {
        o.soft_dollar_tier = soft_dollar_tier(&t);
    }
    m.set_f64(f::CASH_QTY, &mut o.cash_qty);
    m.set_bool(
        f::DONT_USE_AUTO_PRICE_FOR_HEDGE,
        &mut o.dont_use_auto_price_for_hedge,
    );
    m.set_bool(f::IS_OMS_CONTAINER, &mut o.is_oms_container);
    m.set_bool(
        f::DISCRETIONARY_UP_TO_LIMIT_PRICE,
        &mut o.discretionary_up_to_limit_price,
    );
    m.set_opt_flag(f::USE_PRICE_MGMT_ALGO, &mut o.use_price_mgmt_algo);
    m.set_i32(f::DURATION, &mut o.duration);
    m.set_i32(f::POST_TO_ATS, &mut o.post_to_ats);
    m.set_bool(f::AUTO_CANCEL_PARENT, &mut o.auto_cancel_parent);
    m.set_i32(f::MIN_TRADE_QTY, &mut o.min_trade_qty);
    m.set_i32(f::MIN_COMPETE_SIZE, &mut o.min_compete_size);
    m.set_f64(
        f::COMPETE_AGAINST_BEST_OFFSET,
        &mut o.compete_against_best_offset,
    );
    m.set_f64(f::MID_OFFSET_AT_WHOLE, &mut o.mid_offset_at_whole);
    m.set_f64(f::MID_OFFSET_AT_HALF, &mut o.mid_offset_at_half);
    m.set_str(f::CUSTOMER_ACCOUNT, &mut o.customer_account);
    m.set_bool(f::PROFESSIONAL_CUSTOMER, &mut o.professional_customer);
    m.set_str(f::BOND_ACCRUED_INTEREST, &mut o.bond_accrued_interest);
    m.set_bool(f::INCLUDE_OVERNIGHT, &mut o.include_overnight);
    m.set_str(f::EXT_OPERATOR, &mut o.ext_operator);
    m.set_i32(f::MANUAL_ORDER_INDICATOR, &mut o.manual_order_indicator);
    m.set_str(f::SUBMITTER, &mut o.submitter);
    m.set_bool(f::IMBALANCE_ONLY, &mut o.imbalance_only);
    m.set_str(f::AUTO_CANCEL_DATE, &mut o.auto_cancel_date);
    m.set_decimal(f::FILLED_QUANTITY, &mut o.filled_quantity);
    m.set_i32(f::REF_FUTURES_CON_ID, &mut o.ref_futures_con_id);
    m.set_str(f::SHAREHOLDER, &mut o.shareholder);
    m.set_opt_flag(f::ROUTE_MARKETABLE_TO_BBO, &mut o.route_marketable_to_bbo);
    m.set_i64(f::PARENT_PERM_ID, &mut o.parent_perm_id);
    m.set_bool(f::POST_ONLY, &mut o.post_only);
    m.set_bool(f::ALLOW_PRE_OPEN, &mut o.allow_pre_open);
    m.set_bool(f::IGNORE_OPEN_AUCTION, &mut o.ignore_open_auction);
    m.set_bool(f::DEACTIVATE, &mut o.deactivate);
    m.set_str(f::ACTIVE_START_TIME, &mut o.active_start_time);
    m.set_str(f::ACTIVE_STOP_TIME, &mut o.active_stop_time);
    m.set_opt_flag(f::SEEK_PRICE_IMPROVEMENT, &mut o.seek_price_improvement);
    m.set_i32(f::WHAT_IF_TYPE, &mut o.what_if_type);
    o
}

fn order_state(m: &PbMsg<'_>) -> OrderState {
    use order_state as f;
    let mut s = OrderState::default();
    let margin = |field: u32, dst: &mut String| {
        if let Some(x) = m.f64(field) {
            *dst = float_text(x);
        }
    };
    m.set_str(f::STATUS, &mut s.status);
    margin(f::INIT_MARGIN_BEFORE, &mut s.init_margin_before);
    margin(f::MAINT_MARGIN_BEFORE, &mut s.maint_margin_before);
    margin(f::EQUITY_WITH_LOAN_BEFORE, &mut s.equity_with_loan_before);
    margin(f::INIT_MARGIN_CHANGE, &mut s.init_margin_change);
    margin(f::MAINT_MARGIN_CHANGE, &mut s.maint_margin_change);
    margin(f::EQUITY_WITH_LOAN_CHANGE, &mut s.equity_with_loan_change);
    margin(f::INIT_MARGIN_AFTER, &mut s.init_margin_after);
    margin(f::MAINT_MARGIN_AFTER, &mut s.maint_margin_after);
    margin(f::EQUITY_WITH_LOAN_AFTER, &mut s.equity_with_loan_after);
    m.set_f64(f::COMMISSION_AND_FEES, &mut s.commission);
    m.set_f64(f::MIN_COMMISSION_AND_FEES, &mut s.min_commission);
    m.set_f64(f::MAX_COMMISSION_AND_FEES, &mut s.max_commission);
    m.set_str(f::COMMISSION_AND_FEES_CURRENCY, &mut s.commission_currency);
    m.set_str(f::WARNING_TEXT, &mut s.warning_text);
    m.set_str(f::MARGIN_CURRENCY, &mut s.margin_currency);
    m.set_f64(
        f::INIT_MARGIN_BEFORE_OUTSIDE_RTH,
        &mut s.init_margin_before_outside_rth,
    );
    m.set_f64(
        f::MAINT_MARGIN_BEFORE_OUTSIDE_RTH,
        &mut s.maint_margin_before_outside_rth,
    );
    m.set_f64(
        f::EQUITY_WITH_LOAN_BEFORE_OUTSIDE_RTH,
        &mut s.equity_with_loan_before_outside_rth,
    );
    m.set_f64(
        f::INIT_MARGIN_CHANGE_OUTSIDE_RTH,
        &mut s.init_margin_change_outside_rth,
    );
    m.set_f64(
        f::MAINT_MARGIN_CHANGE_OUTSIDE_RTH,
        &mut s.maint_margin_change_outside_rth,
    );
    m.set_f64(
        f::EQUITY_WITH_LOAN_CHANGE_OUTSIDE_RTH,
        &mut s.equity_with_loan_change_outside_rth,
    );
    m.set_f64(
        f::INIT_MARGIN_AFTER_OUTSIDE_RTH,
        &mut s.init_margin_after_outside_rth,
    );
    m.set_f64(
        f::MAINT_MARGIN_AFTER_OUTSIDE_RTH,
        &mut s.maint_margin_after_outside_rth,
    );
    m.set_f64(
        f::EQUITY_WITH_LOAN_AFTER_OUTSIDE_RTH,
        &mut s.equity_with_loan_after_outside_rth,
    );
    m.set_decimal(f::SUGGESTED_SIZE, &mut s.suggested_size);
    m.set_str(f::REJECT_REASON, &mut s.reject_reason);
    s.order_allocations = m
        .msgs(f::ORDER_ALLOCATIONS)
        .map(|a| {
            use order_allocation as g;
            let mut x = OrderAllocation::default();
            a.set_str(g::ACCOUNT, &mut x.account);
            a.set_decimal(g::POSITION, &mut x.position);
            a.set_decimal(g::POSITION_DESIRED, &mut x.position_desired);
            a.set_decimal(g::POSITION_AFTER, &mut x.position_after);
            a.set_decimal(g::DESIRED_ALLOC_QTY, &mut x.desired_alloc_qty);
            a.set_decimal(g::ALLOWED_ALLOC_QTY, &mut x.allowed_alloc_qty);
            a.set_bool(g::IS_MONETARY, &mut x.is_monetary);
            x
        })
        .collect();
    m.set_str(f::COMPLETED_TIME, &mut s.completed_time);
    m.set_str(f::COMPLETED_STATUS, &mut s.completed_status);
    s
}

fn soft_dollar_tier(m: &PbMsg<'_>) -> SoftDollarTier {
    SoftDollarTier {
        name: m.string(soft_dollar_tier::NAME),
        value: m.string(soft_dollar_tier::VALUE),
        display_name: m.string(soft_dollar_tier::DISPLAY_NAME),
    }
}

fn contract_details(c: &PbMsg<'_>, m: &PbMsg<'_>, is_bond: bool) -> ContractDetails {
    use contract_details as f;
    let mut d = ContractDetails {
        contract: contract(c),
        ..Default::default()
    };
    m.set_str(f::MARKET_NAME, &mut d.market_name);
    if let Some(t) = m.str(f::MIN_TICK) {
        d.min_tick = t.trim().parse().unwrap_or(0.0);
    }
    m.set_i32(f::PRICE_MAGNIFIER, &mut d.price_magnifier);
    m.set_str(f::ORDER_TYPES, &mut d.order_types);
    m.set_str(f::VALID_EXCHANGES, &mut d.valid_exchanges);
    m.set_i32(f::UNDER_CON_ID, &mut d.under_con_id);
    m.set_str(f::LONG_NAME, &mut d.long_name);
    m.set_str(f::CONTRACT_MONTH, &mut d.contract_month);
    m.set_str(f::INDUSTRY, &mut d.industry);
    m.set_str(f::CATEGORY, &mut d.category);
    m.set_str(f::SUBCATEGORY, &mut d.subcategory);
    m.set_str(f::TIME_ZONE_ID, &mut d.time_zone_id);
    m.set_str(f::TRADING_HOURS, &mut d.trading_hours);
    m.set_str(f::LIQUID_HOURS, &mut d.liquid_hours);
    m.set_str(f::EV_RULE, &mut d.ev_rule);
    if let Some(x) = m.f64(f::EV_MULTIPLIER) {
        d.ev_multiplier = x as i32;
    }
    d.sec_id_list = m.map(f::SEC_ID_LIST);
    m.set_i32(f::AGG_GROUP, &mut d.agg_group);
    m.set_str(f::UNDER_SYMBOL, &mut d.under_symbol);
    m.set_str(f::UNDER_SEC_TYPE, &mut d.under_sec_type);
    m.set_str(f::MARKET_RULE_IDS, &mut d.market_rule_ids);
    m.set_str(f::REAL_EXPIRATION_DATE, &mut d.real_expiration_date);
    m.set_str(f::STOCK_TYPE, &mut d.stock_type);
    m.set_decimal(f::MIN_SIZE, &mut d.min_size);
    m.set_decimal(f::SIZE_INCREMENT, &mut d.size_increment);
    m.set_decimal(f::SUGGESTED_SIZE_INCREMENT, &mut d.suggested_size_increment);
    m.set_decimal(f::MIN_ALGO_SIZE, &mut d.min_algo_size);
    m.set_decimal(f::LAST_PRICE_PRECISION, &mut d.last_price_precision);
    m.set_decimal(f::LAST_SIZE_PRECISION, &mut d.last_size_precision);
    // IB's setLastTradeDate: "date[-| ]time[ zone]".
    let ltd = d.contract.last_trade_date_or_contract_month.clone();
    let parts: Vec<&str> = if ltd.contains('-') {
        ltd.split('-').collect()
    } else {
        ltd.split_whitespace().collect()
    };
    if let Some(first) = parts.first() {
        if is_bond {
            d.maturity = first.to_string();
        } else {
            d.contract.last_trade_date_or_contract_month = first.to_string();
        }
    }
    if let Some(time) = parts.get(1) {
        d.last_trade_time = time.to_string();
    }
    if is_bond && let Some(zone) = parts.get(2) {
        d.time_zone_id = zone.to_string();
    }
    m.set_str(f::CUSIP, &mut d.cusip);
    m.set_str(f::RATINGS, &mut d.ratings);
    m.set_str(f::DESC_APPEND, &mut d.desc_append);
    m.set_str(f::BOND_TYPE, &mut d.bond_type);
    m.set_f64(f::COUPON, &mut d.coupon);
    m.set_str(f::COUPON_TYPE, &mut d.coupon_type);
    m.set_bool(f::CALLABLE, &mut d.callable);
    m.set_bool(f::PUTTABLE, &mut d.putable);
    m.set_bool(f::CONVERTIBLE, &mut d.convertible);
    m.set_str(f::ISSUE_DATE, &mut d.issue_date);
    m.set_str(f::NEXT_OPTION_DATE, &mut d.next_option_date);
    m.set_str(f::NEXT_OPTION_TYPE, &mut d.next_option_type);
    m.set_bool(f::NEXT_OPTION_PARTIAL, &mut d.next_option_partial);
    m.set_str(f::BOND_NOTES, &mut d.notes);
    m.set_str(f::FUND_NAME, &mut d.fund_name);
    m.set_str(f::FUND_FAMILY, &mut d.fund_family);
    m.set_str(f::FUND_TYPE, &mut d.fund_type);
    m.set_str(f::FUND_FRONT_LOAD, &mut d.fund_front_load);
    m.set_str(f::FUND_BACK_LOAD, &mut d.fund_back_load);
    m.set_str(
        f::FUND_BACK_LOAD_TIME_INTERVAL,
        &mut d.fund_back_load_time_interval,
    );
    m.set_str(f::FUND_MANAGEMENT_FEE, &mut d.fund_management_fee);
    m.set_bool(f::FUND_CLOSED, &mut d.fund_closed);
    m.set_bool(
        f::FUND_CLOSED_FOR_NEW_INVESTORS,
        &mut d.fund_closed_for_new_investors,
    );
    m.set_bool(
        f::FUND_CLOSED_FOR_NEW_MONEY,
        &mut d.fund_closed_for_new_money,
    );
    m.set_str(f::FUND_NOTIFY_AMOUNT, &mut d.fund_notify_amount);
    m.set_str(
        f::FUND_MINIMUM_INITIAL_PURCHASE,
        &mut d.fund_minimum_initial_purchase,
    );
    m.set_str(
        f::FUND_MINIMUM_SUBSEQUENT_PURCHASE,
        &mut d.fund_subsequent_minimum_purchase,
    );
    m.set_str(f::FUND_BLUE_SKY_STATES, &mut d.fund_blue_sky_states);
    m.set_str(
        f::FUND_BLUE_SKY_TERRITORIES,
        &mut d.fund_blue_sky_territories,
    );
    m.set_str(
        f::FUND_DISTRIBUTION_POLICY_INDICATOR,
        &mut d.fund_distribution_policy_indicator,
    );
    m.set_str(f::FUND_ASSET_TYPE, &mut d.fund_asset_type);
    d.ineligibility_reason_list = m
        .msgs(f::INELIGIBILITY_REASON_LIST)
        .map(|r| IneligibilityReason {
            id: r.string(ineligibility_reason::ID),
            description: r.string(ineligibility_reason::DESCRIPTION),
        })
        .collect();
    m.set_str(f::EVENT_CONTRACT1, &mut d.event_contract1);
    m.set_str(
        f::EVENT_CONTRACT_DESCRIPTION1,
        &mut d.event_contract_description1,
    );
    m.set_str(
        f::EVENT_CONTRACT_DESCRIPTION2,
        &mut d.event_contract_description2,
    );
    m.set_str(f::SETTLEMENT_METHOD, &mut d.settlement_method);
    d
}

fn historical_tick(m: &PbMsg<'_>) -> HistoricalTick {
    use historical_tick as f;
    let mut t = HistoricalTick::default();
    m.set_i64(f::TIME, &mut t.time);
    m.set_f64(f::PRICE, &mut t.price);
    m.set_decimal(f::SIZE, &mut t.size);
    t
}

fn historical_tick_bid_ask(m: &PbMsg<'_>) -> HistoricalTickBidAsk {
    use historical_tick_bid_ask as f;
    let mut t = HistoricalTickBidAsk::default();
    m.set_i64(f::TIME, &mut t.time);
    if let Some(a) = m.msg(f::TICK_ATTRIB_BID_ASK) {
        a.set_bool(
            tick_attrib_bid_ask::BID_PAST_LOW,
            &mut t.tick_attrib_bid_ask.bid_past_low,
        );
        a.set_bool(
            tick_attrib_bid_ask::ASK_PAST_HIGH,
            &mut t.tick_attrib_bid_ask.ask_past_high,
        );
    }
    m.set_f64(f::PRICE_BID, &mut t.price_bid);
    m.set_f64(f::PRICE_ASK, &mut t.price_ask);
    m.set_decimal(f::SIZE_BID, &mut t.size_bid);
    m.set_decimal(f::SIZE_ASK, &mut t.size_ask);
    t
}

fn historical_tick_last(m: &PbMsg<'_>) -> HistoricalTickLast {
    use historical_tick_last as f;
    let mut t = HistoricalTickLast::default();
    m.set_i64(f::TIME, &mut t.time);
    if let Some(a) = m.msg(f::TICK_ATTRIB_LAST) {
        a.set_bool(
            tick_attrib_last::PAST_LIMIT,
            &mut t.tick_attrib_last.past_limit,
        );
        a.set_bool(
            tick_attrib_last::UNREPORTED,
            &mut t.tick_attrib_last.unreported,
        );
    }
    m.set_f64(f::PRICE, &mut t.price);
    m.set_decimal(f::SIZE, &mut t.size);
    m.set_str(f::EXCHANGE, &mut t.exchange);
    m.set_str(f::SPECIAL_CONDITIONS, &mut t.special_conditions);
    t
}

fn bar(m: &PbMsg<'_>) -> BarData {
    use historical_data_bar as f;
    let mut b = BarData::default();
    m.set_str(f::DATE, &mut b.date);
    m.set_f64(f::OPEN, &mut b.open);
    m.set_f64(f::HIGH, &mut b.high);
    m.set_f64(f::LOW, &mut b.low);
    m.set_f64(f::CLOSE, &mut b.close);
    m.set_decimal(f::VOLUME, &mut b.volume);
    m.set_decimal(f::WAP, &mut b.average);
    m.set_i32(f::BAR_COUNT, &mut b.bar_count);
    b
}

/// Greek value, `None` when the server sends its "not computed" marker.
fn greek(m: &PbMsg<'_>, field: u32, not_computed: f64) -> Option<f64> {
    m.f64(field).filter(|v| *v != not_computed)
}

/// Decode protobuf payload `b` of message `id` (already reduced by 200).
pub fn decode_pb(id: i32, b: &[u8]) -> Result<Incoming, DecodeError> {
    let m = PbMsg::parse(b)?;
    let req_id = |f: u32| m.int_or(f, NO_VALID_ID);
    Ok(match id {
        msg::TICK_PRICE => {
            use tick_price as f;
            let mask = m.int(f::ATTR_MASK);
            Incoming::TickPrice {
                req_id: req_id(f::REQ_ID),
                tick_type: m.int(f::TICK_TYPE),
                price: m.double(f::PRICE),
                size: m.decimal(f::SIZE),
                attrib: TickAttrib {
                    can_auto_execute: mask & 1 != 0,
                    past_limit: mask & 2 != 0,
                    pre_open: mask & 4 != 0,
                },
            }
        }
        msg::TICK_SIZE => Incoming::TickSize {
            req_id: req_id(tick_size::REQ_ID),
            tick_type: m.int(tick_size::TICK_TYPE),
            size: m.decimal(tick_size::SIZE),
        },
        msg::TICK_OPTION_COMPUTATION => {
            use tick_option_computation as f;
            Incoming::TickOptionComputation {
                req_id: req_id(f::REQ_ID),
                tick_type: m.int(f::TICK_TYPE),
                computation: OptionComputation {
                    tick_attrib: m.i32(f::TICK_ATTRIB),
                    implied_vol: m.f64(f::IMPLIED_VOL).filter(|v| *v >= 0.0),
                    delta: greek(&m, f::DELTA, -2.0),
                    opt_price: greek(&m, f::OPT_PRICE, -1.0),
                    pv_dividend: greek(&m, f::PV_DIVIDEND, -1.0),
                    gamma: greek(&m, f::GAMMA, -2.0),
                    vega: greek(&m, f::VEGA, -2.0),
                    theta: greek(&m, f::THETA, -2.0),
                    und_price: greek(&m, f::UND_PRICE, -1.0),
                },
            }
        }
        msg::TICK_GENERIC => Incoming::TickGeneric {
            req_id: req_id(tick_generic::REQ_ID),
            tick_type: m.int(tick_generic::TICK_TYPE),
            value: m.double(tick_generic::VALUE),
        },
        msg::TICK_STRING => Incoming::TickString {
            req_id: req_id(tick_string::REQ_ID),
            tick_type: m.int(tick_string::TICK_TYPE),
            value: m.string(tick_string::VALUE),
        },
        msg::TICK_SNAPSHOT_END => Incoming::TickSnapshotEnd {
            req_id: req_id(tick_snapshot_end::REQ_ID),
        },
        msg::TICK_REQ_PARAMS => {
            use tick_req_params as f;
            Incoming::TickReqParams {
                req_id: req_id(f::REQ_ID),
                min_tick: m
                    .str(f::MIN_TICK)
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(UNSET_DOUBLE),
                bbo_exchange: m.string(f::BBO_EXCHANGE),
                snapshot_permissions: m.int(f::SNAPSHOT_PERMISSIONS),
            }
        }
        msg::MARKET_DATA_TYPE => Incoming::MarketDataType {
            req_id: req_id(market_data_type::REQ_ID),
            market_data_type: m.int(market_data_type::MARKET_DATA_TYPE),
        },
        msg::MARKET_DEPTH | msg::MARKET_DEPTH_L2 => {
            let rid = req_id(market_depth::REQ_ID);
            let d = m
                .msg(market_depth::MARKET_DEPTH_DATA)
                .ok_or_else(|| missing("MarketDepth.marketDepthData"))?;
            use market_depth_data as f;
            if id == msg::MARKET_DEPTH {
                Incoming::UpdateMktDepth {
                    req_id: rid,
                    position: d.int(f::POSITION),
                    operation: d.int(f::OPERATION),
                    side: d.int(f::SIDE),
                    price: d.double(f::PRICE),
                    size: d.decimal(f::SIZE),
                }
            } else {
                Incoming::UpdateMktDepthL2 {
                    req_id: rid,
                    position: d.int_or(f::POSITION, 0),
                    market_maker: d.string(f::MARKET_MAKER),
                    operation: d.int(f::OPERATION),
                    side: d.int(f::SIDE),
                    price: d.double(f::PRICE),
                    size: d.decimal(f::SIZE),
                    is_smart_depth: d.flag(f::IS_SMART_DEPTH),
                }
            }
        }
        msg::ORDER_STATUS => {
            use order_status as f;
            Incoming::OrderStatus(OrderStatusMsg {
                order_id: m.int(f::ORDER_ID),
                status: m.string(f::STATUS),
                filled: m.decimal(f::FILLED),
                remaining: m.decimal(f::REMAINING),
                avg_fill_price: m.double(f::AVG_FILL_PRICE),
                perm_id: m.long(f::PERM_ID),
                parent_id: m.int(f::PARENT_ID),
                last_fill_price: m.double(f::LAST_FILL_PRICE),
                client_id: m.int(f::CLIENT_ID),
                why_held: m.string(f::WHY_HELD),
                mkt_cap_price: m.double(f::MKT_CAP_PRICE),
            })
        }
        msg::ERR_MSG => {
            use error_message as f;
            Incoming::Error(ErrorMsg {
                req_id: m.int_or(f::ID, 0),
                code: m.int_or(f::ERROR_CODE, 0),
                message: m.string(f::ERROR_MSG),
                advanced_order_reject_json: m.string(f::ADVANCED_ORDER_REJECT_JSON),
                error_time: m.i64(f::ERROR_TIME).unwrap_or(0),
            })
        }
        msg::OPEN_ORDER => {
            use open_order as f;
            let order_id = m.int_or(f::ORDER_ID, 0);
            let c = m
                .msg(f::CONTRACT)
                .ok_or_else(|| missing("OpenOrder.contract"))?;
            let o = m.msg(f::ORDER).ok_or_else(|| missing("OpenOrder.order"))?;
            let s = m
                .msg(f::ORDER_STATE)
                .ok_or_else(|| missing("OpenOrder.orderState"))?;
            Incoming::OpenOrder {
                order_id,
                contract: Box::new(contract(&c)),
                order: Box::new(order(order_id, &c, &o)),
                state: Box::new(order_state(&s)),
            }
        }
        msg::OPEN_ORDER_END => Incoming::OpenOrderEnd,
        msg::COMPLETED_ORDER => {
            use completed_order as f;
            let c = m
                .msg(f::CONTRACT)
                .ok_or_else(|| missing("CompletedOrder.contract"))?;
            let o = m
                .msg(f::ORDER)
                .ok_or_else(|| missing("CompletedOrder.order"))?;
            let s = m
                .msg(f::ORDER_STATE)
                .ok_or_else(|| missing("CompletedOrder.orderState"))?;
            Incoming::CompletedOrder {
                contract: Box::new(contract(&c)),
                order: Box::new(order(UNSET_INTEGER, &c, &o)),
                state: Box::new(order_state(&s)),
            }
        }
        msg::COMPLETED_ORDERS_END => Incoming::CompletedOrdersEnd,
        msg::ORDER_BOUND => Incoming::OrderBound {
            perm_id: m.long(order_bound::PERM_ID),
            client_id: m.int(order_bound::CLIENT_ID),
            order_id: m.int(order_bound::ORDER_ID),
        },
        msg::EXECUTION_DATA => {
            use execution_details as f;
            let c = m
                .msg(f::CONTRACT)
                .ok_or_else(|| missing("ExecutionDetails.contract"))?;
            let e = m
                .msg(f::EXECUTION)
                .ok_or_else(|| missing("ExecutionDetails.execution"))?;
            Incoming::ExecDetails {
                req_id: req_id(f::REQ_ID),
                contract: Box::new(contract(&c)),
                execution: Box::new(execution(&e)),
            }
        }
        msg::EXECUTION_DATA_END => Incoming::ExecDetailsEnd {
            req_id: req_id(execution_details_end::REQ_ID),
        },
        msg::COMMISSION_REPORT => {
            use commission_and_fees_report as f;
            Incoming::CommissionReport(CommissionReport {
                exec_id: m.string(f::EXEC_ID),
                commission: m.f64(f::COMMISSION_AND_FEES).unwrap_or(0.0),
                currency: m.string(f::CURRENCY),
                realized_pnl: m.f64(f::REALIZED_PNL).unwrap_or(0.0),
                yield_: m.f64(f::BOND_YIELD).unwrap_or(0.0),
                yield_redemption_date: m
                    .str(f::YIELD_REDEMPTION_DATE)
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0),
            })
        }
        msg::CONTRACT_DATA | msg::BOND_CONTRACT_DATA => {
            use contract_data as f;
            let c = m
                .msg(f::CONTRACT)
                .ok_or_else(|| missing("ContractData.contract"))?;
            let d = m
                .msg(f::CONTRACT_DETAILS)
                .ok_or_else(|| missing("ContractData.contractDetails"))?;
            let bond = id == msg::BOND_CONTRACT_DATA;
            let details = Box::new(contract_details(&c, &d, bond));
            let req_id = req_id(f::REQ_ID);
            if bond {
                Incoming::BondContractDetails { req_id, details }
            } else {
                Incoming::ContractDetails { req_id, details }
            }
        }
        msg::CONTRACT_DATA_END => Incoming::ContractDetailsEnd {
            req_id: req_id(contract_data_end::REQ_ID),
        },
        msg::ACCT_VALUE => {
            use account_value as f;
            Incoming::UpdateAccountValue {
                key: m.string(f::KEY),
                value: m.string(f::VALUE),
                currency: m.string(f::CURRENCY),
                account: m.string(f::ACCOUNT_NAME),
            }
        }
        msg::PORTFOLIO_VALUE => {
            use portfolio_value as f;
            let c = m
                .msg(f::CONTRACT)
                .ok_or_else(|| missing("PortfolioValue.contract"))?;
            Incoming::UpdatePortfolio(Box::new(PortfolioMsg {
                contract: contract(&c),
                position: m.decimal(f::POSITION),
                market_price: m.f64(f::MARKET_PRICE).unwrap_or(0.0),
                market_value: m.f64(f::MARKET_VALUE).unwrap_or(0.0),
                average_cost: m.f64(f::AVERAGE_COST).unwrap_or(0.0),
                unrealized_pnl: m.f64(f::UNREALIZED_PNL).unwrap_or(0.0),
                realized_pnl: m.f64(f::REALIZED_PNL).unwrap_or(0.0),
                account: m.string(f::ACCOUNT_NAME),
            }))
        }
        msg::ACCT_UPDATE_TIME => Incoming::UpdateAccountTime {
            timestamp: m.string(account_update_time::TIME_STAMP),
        },
        msg::ACCT_DOWNLOAD_END => Incoming::AccountDownloadEnd {
            account: m.string(account_data_end::ACCOUNT_NAME),
        },
        msg::MANAGED_ACCTS => Incoming::ManagedAccounts {
            accounts: m.string(managed_accounts::ACCOUNTS_LIST),
        },
        msg::POSITION_DATA => {
            use position as f;
            let c = m
                .msg(f::CONTRACT)
                .ok_or_else(|| missing("Position.contract"))?;
            Incoming::Position {
                account: m.string(f::ACCOUNT),
                contract: Box::new(contract(&c)),
                position: m.decimal(f::POSITION),
                avg_cost: m.f64(f::AVG_COST).unwrap_or(0.0),
            }
        }
        msg::POSITION_END => Incoming::PositionEnd,
        msg::ACCOUNT_SUMMARY => {
            use account_summary as f;
            Incoming::AccountSummary {
                req_id: req_id(f::REQ_ID),
                account: m.string(f::ACCOUNT),
                tag: m.string(f::TAG),
                value: m.string(f::VALUE),
                currency: m.string(f::CURRENCY),
            }
        }
        msg::ACCOUNT_SUMMARY_END => Incoming::AccountSummaryEnd {
            req_id: req_id(account_summary_end::REQ_ID),
        },
        msg::POSITION_MULTI => {
            use position_multi as f;
            let c = m
                .msg(f::CONTRACT)
                .ok_or_else(|| missing("PositionMulti.contract"))?;
            Incoming::PositionMulti {
                req_id: req_id(f::REQ_ID),
                account: m.string(f::ACCOUNT),
                model_code: m.string(f::MODEL_CODE),
                contract: Box::new(contract(&c)),
                position: m.decimal(f::POSITION),
                avg_cost: m.f64(f::AVG_COST).unwrap_or(0.0),
            }
        }
        msg::POSITION_MULTI_END => Incoming::PositionMultiEnd {
            req_id: req_id(position_multi_end::REQ_ID),
        },
        msg::ACCOUNT_UPDATE_MULTI => {
            use account_update_multi as f;
            Incoming::AccountUpdateMulti {
                req_id: req_id(f::REQ_ID),
                account: m.string(f::ACCOUNT),
                model_code: m.string(f::MODEL_CODE),
                key: m.string(f::KEY),
                value: m.string(f::VALUE),
                currency: m.string(f::CURRENCY),
            }
        }
        msg::ACCOUNT_UPDATE_MULTI_END => Incoming::AccountUpdateMultiEnd {
            req_id: req_id(account_update_multi_end::REQ_ID),
        },
        msg::HISTORICAL_DATA => Incoming::HistoricalData {
            req_id: req_id(historical_data::REQ_ID),
            bars: m
                .msgs(historical_data::HISTORICAL_DATA_BARS)
                .map(|b| bar(&b))
                .collect(),
            end: None,
        },
        msg::HISTORICAL_DATA_UPDATE => {
            let b = m
                .msg(historical_data_update::HISTORICAL_DATA_BAR)
                .ok_or_else(|| missing("HistoricalDataUpdate.historicalDataBar"))?;
            Incoming::HistoricalDataUpdate {
                req_id: req_id(historical_data_update::REQ_ID),
                bar: bar(&b),
            }
        }
        msg::HISTORICAL_DATA_END => Incoming::HistoricalDataEnd {
            req_id: req_id(historical_data_end::REQ_ID),
            start: m.string(historical_data_end::START_DATE_STR),
            end: m.string(historical_data_end::END_DATE_STR),
        },
        msg::REAL_TIME_BARS => {
            use real_time_bar_tick as f;
            Incoming::RealtimeBar {
                req_id: req_id(f::REQ_ID),
                bar: RealTimeBar {
                    time: m.i64(f::TIME).unwrap_or(0),
                    open: m.f64(f::OPEN).unwrap_or(0.0),
                    high: m.f64(f::HIGH).unwrap_or(0.0),
                    low: m.f64(f::LOW).unwrap_or(0.0),
                    close: m.f64(f::CLOSE).unwrap_or(0.0),
                    volume: m.decimal(f::VOLUME),
                    wap: m.decimal(f::WAP),
                    count: m.int_or(f::COUNT, 0),
                },
            }
        }
        msg::HEAD_TIMESTAMP => Incoming::HeadTimestamp {
            req_id: req_id(head_timestamp::REQ_ID),
            head_timestamp: m.string(head_timestamp::HEAD_TIMESTAMP),
        },
        msg::HISTOGRAM_DATA => Incoming::HistogramData {
            req_id: req_id(histogram_data::REQ_ID),
            items: m
                .msgs(histogram_data::HISTOGRAM_DATA_ENTRIES)
                .map(|e| HistogramData {
                    price: e.f64(histogram_data_entry::PRICE).unwrap_or(0.0),
                    count: e.decimal(histogram_data_entry::SIZE),
                })
                .collect(),
        },
        msg::HISTORICAL_TICKS => Incoming::HistoricalTicks {
            req_id: req_id(historical_ticks::REQ_ID),
            ticks: m
                .msgs(historical_ticks::HISTORICAL_TICKS)
                .map(|t| historical_tick(&t))
                .collect(),
            done: m.flag(historical_ticks::IS_DONE),
        },
        msg::HISTORICAL_TICKS_BID_ASK => Incoming::HistoricalTicksBidAsk {
            req_id: req_id(historical_ticks_bid_ask::REQ_ID),
            ticks: m
                .msgs(historical_ticks_bid_ask::HISTORICAL_TICKS_BID_ASK)
                .map(|t| historical_tick_bid_ask(&t))
                .collect(),
            done: m.flag(historical_ticks_bid_ask::IS_DONE),
        },
        msg::HISTORICAL_TICKS_LAST => Incoming::HistoricalTicksLast {
            req_id: req_id(historical_ticks_last::REQ_ID),
            ticks: m
                .msgs(historical_ticks_last::HISTORICAL_TICKS_LAST)
                .map(|t| historical_tick_last(&t))
                .collect(),
            done: m.flag(historical_ticks_last::IS_DONE),
        },
        msg::TICK_BY_TICK => {
            use tick_by_tick_data as f;
            let rid = req_id(f::REQ_ID);
            match m.int_or(f::TICK_TYPE, 0) {
                tick_type @ (1 | 2) => {
                    let t = historical_tick_last(
                        &m.msg(f::HISTORICAL_TICK_LAST)
                            .ok_or_else(|| missing("TickByTickData.historicalTickLast"))?,
                    );
                    Incoming::TickByTickAllLast {
                        req_id: rid,
                        tick_type,
                        time: t.time,
                        price: t.price,
                        size: t.size,
                        attrib: t.tick_attrib_last,
                        exchange: t.exchange,
                        special_conditions: t.special_conditions,
                    }
                }
                3 => {
                    let t = historical_tick_bid_ask(
                        &m.msg(f::HISTORICAL_TICK_BID_ASK)
                            .ok_or_else(|| missing("TickByTickData.historicalTickBidAsk"))?,
                    );
                    Incoming::TickByTickBidAsk {
                        req_id: rid,
                        time: t.time,
                        bid_price: t.price_bid,
                        ask_price: t.price_ask,
                        bid_size: t.size_bid,
                        ask_size: t.size_ask,
                        attrib: t.tick_attrib_bid_ask,
                    }
                }
                4 => {
                    let t = historical_tick(
                        &m.msg(f::HISTORICAL_TICK_MID_POINT)
                            .ok_or_else(|| missing("TickByTickData.historicalTickMidPoint"))?,
                    );
                    Incoming::TickByTickMidPoint {
                        req_id: rid,
                        time: t.time,
                        mid_point: t.price,
                    }
                }
                other => {
                    return Err(DecodeError::Protobuf(format!(
                        "TickByTickData has tick type {other}; expected 1 to 4"
                    )));
                }
            }
        }
        msg::NEWS_BULLETINS => {
            use news_bulletin as f;
            Incoming::UpdateNewsBulletin {
                msg_id: m.int_or(f::NEWS_MSG_ID, 0),
                msg_type: m.int_or(f::NEWS_MSG_TYPE, 0),
                message: m.string(f::NEWS_MESSAGE),
                orig_exchange: m.string(f::ORIGINATING_EXCH),
            }
        }
        msg::NEWS_ARTICLE => Incoming::NewsArticle {
            req_id: req_id(news_article::REQ_ID),
            article_type: m.int_or(news_article::ARTICLE_TYPE, 0),
            article_text: m.string(news_article::ARTICLE_TEXT),
        },
        msg::NEWS_PROVIDERS => Incoming::NewsProviders {
            providers: m
                .msgs(news_providers::NEWS_PROVIDERS)
                .map(|p| NewsProvider {
                    code: p.string(news_provider::PROVIDER_CODE),
                    name: p.string(news_provider::PROVIDER_NAME),
                })
                .collect(),
        },
        msg::HISTORICAL_NEWS => {
            use historical_news as f;
            Incoming::HistoricalNews {
                req_id: req_id(f::REQ_ID),
                time: m.string(f::TIME),
                provider_code: m.string(f::PROVIDER_CODE),
                article_id: m.string(f::ARTICLE_ID),
                headline: m.string(f::HEADLINE),
            }
        }
        msg::HISTORICAL_NEWS_END => Incoming::HistoricalNewsEnd {
            req_id: req_id(historical_news_end::REQ_ID),
            has_more: m.flag(historical_news_end::HAS_MORE),
        },
        msg::TICK_NEWS => {
            use tick_news as f;
            Incoming::TickNews {
                req_id: req_id(f::REQ_ID),
                time_stamp: m.i64(f::TIMESTAMP).unwrap_or(0),
                provider_code: m.string(f::PROVIDER_CODE),
                article_id: m.string(f::ARTICLE_ID),
                headline: m.string(f::HEADLINE),
                extra_data: m.string(f::EXTRA_DATA),
            }
        }
        msg::WSH_META_DATA => Incoming::WshMetaData {
            req_id: req_id(wsh_meta_data::REQ_ID),
            data_json: m.string(wsh_meta_data::DATA_JSON),
        },
        msg::WSH_EVENT_DATA => Incoming::WshEventData {
            req_id: req_id(wsh_event_data::REQ_ID),
            data_json: m.string(wsh_event_data::DATA_JSON),
        },
        msg::SCANNER_PARAMETERS => Incoming::ScannerParameters {
            xml: m.string(scanner_parameters::XML),
        },
        msg::SCANNER_DATA => {
            use scanner_data_element as f;
            Incoming::ScannerData {
                req_id: req_id(scanner_data::REQ_ID),
                rows: m
                    .msgs(scanner_data::SCANNER_DATA_ELEMENT)
                    .map(|e| {
                        let mut details = ContractDetails::default();
                        if let Some(c) = e.msg(f::CONTRACT) {
                            details.contract = contract(&c);
                        }
                        details.market_name = e.string(f::MARKET_NAME);
                        ScanData {
                            rank: e.int_or(f::RANK, 0),
                            contract_details: details,
                            distance: e.string(f::DISTANCE),
                            benchmark: e.string(f::BENCHMARK),
                            projection: e.string(f::PROJECTION),
                            legs_str: e.string(f::COMBO_KEY),
                        }
                    })
                    .collect(),
            }
        }
        msg::FUNDAMENTAL_DATA => Incoming::FundamentalData {
            req_id: req_id(fundamentals_data::REQ_ID),
            data: m.string(fundamentals_data::DATA),
        },
        msg::PNL => {
            use pn_l as f;
            Incoming::Pnl {
                req_id: req_id(f::REQ_ID),
                daily_pnl: m.double(f::DAILY_PN_L),
                unrealized_pnl: m.double(f::UNREALIZED_PN_L),
                realized_pnl: m.double(f::REALIZED_PN_L),
            }
        }
        msg::PNL_SINGLE => {
            use pn_l_single as f;
            Incoming::PnlSingle {
                req_id: req_id(f::REQ_ID),
                position: m.decimal(f::POSITION),
                daily_pnl: m.double(f::DAILY_PN_L),
                unrealized_pnl: m.double(f::UNREALIZED_PN_L),
                realized_pnl: m.double(f::REALIZED_PN_L),
                value: m.double(f::VALUE),
            }
        }
        msg::RECEIVE_FA => Incoming::ReceiveFa {
            fa_data_type: m.int_or(receive_fa::FA_DATA_TYPE, 0),
            xml: m.string(receive_fa::XML),
        },
        msg::REPLACE_FA_END => Incoming::ReplaceFaEnd {
            req_id: req_id(replace_fa_end::REQ_ID),
            text: m.string(replace_fa_end::TEXT),
        },
        msg::HISTORICAL_SCHEDULE => {
            use historical_schedule as f;
            Incoming::HistoricalSchedule {
                req_id: req_id(f::REQ_ID),
                start_date_time: m.string(f::START_DATE_TIME),
                end_date_time: m.string(f::END_DATE_TIME),
                time_zone: m.string(f::TIME_ZONE),
                sessions: m
                    .msgs(f::HISTORICAL_SESSIONS)
                    .map(|s| HistoricalSession {
                        start_date_time: s.string(historical_session::START_DATE_TIME),
                        end_date_time: s.string(historical_session::END_DATE_TIME),
                        ref_date: s.string(historical_session::REF_DATE),
                    })
                    .collect(),
            }
        }
        msg::REROUTE_MKT_DATA_REQ => Incoming::RerouteMktDataReq {
            req_id: req_id(reroute_market_data_request::REQ_ID),
            con_id: m.int_or(reroute_market_data_request::CON_ID, 0),
            exchange: m.string(reroute_market_data_request::EXCHANGE),
        },
        msg::REROUTE_MKT_DEPTH_REQ => Incoming::RerouteMktDepthReq {
            req_id: req_id(reroute_market_depth_request::REQ_ID),
            con_id: m.int_or(reroute_market_depth_request::CON_ID, 0),
            exchange: m.string(reroute_market_depth_request::EXCHANGE),
        },
        msg::SECURITY_DEFINITION_OPTION_PARAMETER => {
            use sec_def_opt_parameter as f;
            Incoming::SecurityDefinitionOptionParameter(Box::new(SecDefOptParams {
                req_id: req_id(f::REQ_ID),
                exchange: m.string(f::EXCHANGE),
                underlying_con_id: m.int_or(f::UNDERLYING_CON_ID, 0),
                trading_class: m.string(f::TRADING_CLASS),
                multiplier: m.string(f::MULTIPLIER),
                expirations: m.strings(f::EXPIRATIONS),
                strikes: m.doubles(f::STRIKES),
            }))
        }
        msg::SECURITY_DEFINITION_OPTION_PARAMETER_END => {
            Incoming::SecurityDefinitionOptionParameterEnd {
                req_id: req_id(sec_def_opt_parameter_end::REQ_ID),
            }
        }
        msg::SOFT_DOLLAR_TIERS => Incoming::SoftDollarTiers {
            req_id: req_id(soft_dollar_tiers::REQ_ID),
            tiers: m
                .msgs(soft_dollar_tiers::SOFT_DOLLAR_TIERS)
                .map(|t| soft_dollar_tier(&t))
                .collect(),
        },
        msg::FAMILY_CODES => Incoming::FamilyCodes {
            codes: m
                .msgs(family_codes::FAMILY_CODES)
                .map(|c| FamilyCode {
                    account_id: c.string(family_code::ACCOUNT_ID),
                    family_code: c.string(family_code::FAMILY_CODE),
                })
                .collect(),
        },
        msg::SYMBOL_SAMPLES => Incoming::SymbolSamples {
            req_id: req_id(symbol_samples::REQ_ID),
            descriptions: m
                .msgs(symbol_samples::CONTRACT_DESCRIPTIONS)
                .map(|d| ContractDescription {
                    contract: d
                        .msg(contract_description::CONTRACT)
                        .map(|c| contract(&c))
                        .unwrap_or_default(),
                    derivative_sec_types: d.strings(contract_description::DERIVATIVE_SEC_TYPES),
                })
                .collect(),
        },
        msg::SMART_COMPONENTS => Incoming::SmartComponents {
            req_id: req_id(smart_components::REQ_ID),
            components: m
                .msgs(smart_components::SMART_COMPONENTS)
                .map(|c| SmartComponent {
                    bit_number: c.int_or(smart_component::BIT_NUMBER, 0),
                    exchange: c.string(smart_component::EXCHANGE),
                    exchange_letter: c.string(smart_component::EXCHANGE_LETTER),
                })
                .collect(),
        },
        msg::MARKET_RULE => Incoming::MarketRule {
            market_rule_id: m.int_or(market_rule::MARKET_RULE_ID, 0),
            increments: m
                .msgs(market_rule::PRICE_INCREMENTS)
                .map(|p| PriceIncrement {
                    low_edge: p.f64(price_increment::LOW_EDGE).unwrap_or(0.0),
                    increment: p.f64(price_increment::INCREMENT).unwrap_or(0.0),
                })
                .collect(),
        },
        msg::MKT_DEPTH_EXCHANGES => Incoming::MktDepthExchanges {
            descriptions: m
                .msgs(market_depth_exchanges::DEPTH_MARKET_DATA_DESCRIPTIONS)
                .map(|d| {
                    use depth_market_data_description as f;
                    DepthMktDataDescription {
                        exchange: d.string(f::EXCHANGE),
                        sec_type: d.string(f::SEC_TYPE),
                        listing_exch: d.string(f::LISTING_EXCH),
                        service_data_type: d.string(f::SERVICE_DATA_TYPE),
                        agg_group: d.int(f::AGG_GROUP),
                    }
                })
                .collect(),
        },
        msg::USER_INFO => Incoming::UserInfo {
            req_id: req_id(user_info::REQ_ID),
            white_branding_id: m.string(user_info::WHITE_BRANDING_ID),
        },
        msg::NEXT_VALID_ID => Incoming::NextValidId {
            order_id: m.int_or(next_valid_id::ORDER_ID, 0),
        },
        msg::CURRENT_TIME => Incoming::CurrentTime {
            time: m.i64(current_time::CURRENT_TIME).unwrap_or(0),
        },
        msg::CURRENT_TIME_IN_MILLIS => Incoming::CurrentTimeInMillis {
            time: m
                .i64(current_time_in_millis::CURRENT_TIME_IN_MILLIS)
                .unwrap_or(0),
        },
        msg::VERIFY_MESSAGE_API => Incoming::VerifyMessageApi {
            api_data: m.string(verify_message_api::API_DATA),
        },
        msg::VERIFY_COMPLETED => Incoming::VerifyCompleted {
            is_successful: m.flag(verify_completed::IS_SUCCESSFUL),
            error_text: m.string(verify_completed::ERROR_TEXT),
        },
        msg::DISPLAY_GROUP_LIST => Incoming::DisplayGroupList {
            req_id: req_id(display_group_list::REQ_ID),
            groups: m.string(display_group_list::GROUPS),
        },
        msg::DISPLAY_GROUP_UPDATED => Incoming::DisplayGroupUpdated {
            req_id: req_id(display_group_updated::REQ_ID),
            contract_info: m.string(display_group_updated::CONTRACT_INFO),
        },
        msg::CONFIG_RESPONSE => Incoming::ConfigResponse {
            req_id: req_id(config_response::REQ_ID),
            config: to_json("ConfigResponse", &m),
        },
        msg::UPDATE_CONFIG_RESPONSE => Incoming::UpdateConfigResponse {
            req_id: req_id(update_config_response::REQ_ID),
            response: to_json("UpdateConfigResponse", &m),
        },
        other => Incoming::Unknown {
            msg_id: other + crate::message::PROTOBUF_MSG_ID,
            fields: vec![to_json_unknown(b)],
        },
    })
}

/// Hex dump of a protobuf payload with no decoder.
fn to_json_unknown(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
