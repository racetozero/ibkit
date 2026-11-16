//! Data types of the TWS API.
//!
//! Field names are the snake_case form of the names used by IB's official
//! client and by ib_async. Serde uses the original camelCase names, so a
//! dictionary produced by ib_async (`util.dataclassAsDict`) deserializes into
//! these types unchanged.

use crate::consts::{UNSET_DOUBLE, UNSET_INTEGER};
use serde::{Deserialize, Serialize};

/// A tag/value pair, used for algo parameters and option lists.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TagValue {
    pub tag: String,
    pub value: String,
}

impl TagValue {
    pub fn new(tag: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            value: value.into(),
        }
    }
}

/// One leg of a combination (BAG) contract.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ComboLeg {
    pub con_id: i32,
    pub ratio: i32,
    pub action: String,
    pub exchange: String,
    pub open_close: i32,
    pub short_sale_slot: i32,
    pub designated_location: String,
    pub exempt_code: i32,
}

impl Default for ComboLeg {
    fn default() -> Self {
        Self {
            con_id: 0,
            ratio: 0,
            action: String::new(),
            exchange: String::new(),
            open_close: 0,
            short_sale_slot: 0,
            designated_location: String::new(),
            exempt_code: -1,
        }
    }
}

/// Delta neutral hedge attached to a contract.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeltaNeutralContract {
    pub con_id: i32,
    pub delta: f64,
    pub price: f64,
}

/// A financial instrument.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Contract {
    pub con_id: i32,
    pub symbol: String,
    pub sec_type: String,
    pub last_trade_date_or_contract_month: String,
    /// Filled by the server at version 182 and later.
    pub last_trade_date: String,
    pub strike: f64,
    pub right: String,
    pub multiplier: String,
    pub exchange: String,
    pub primary_exchange: String,
    pub currency: String,
    pub local_symbol: String,
    pub trading_class: String,
    pub include_expired: bool,
    pub sec_id_type: String,
    pub sec_id: String,
    pub description: String,
    pub issuer_id: String,
    pub combo_legs_descrip: String,
    pub combo_legs: Vec<ComboLeg>,
    pub delta_neutral_contract: Option<DeltaNeutralContract>,
}

/// A security id (for example ISIN) or other tagged attribute of a contract.
pub type SecIdList = Vec<TagValue>;

/// A reason why a contract is not eligible for trading.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IneligibilityReason {
    #[serde(rename = "id")]
    pub id: String,
    pub description: String,
}

/// Full description of a contract returned by `reqContractDetails`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ContractDetails {
    pub contract: Contract,
    pub market_name: String,
    pub min_tick: f64,
    pub order_types: String,
    pub valid_exchanges: String,
    pub price_magnifier: i32,
    pub under_con_id: i32,
    pub long_name: String,
    pub contract_month: String,
    pub industry: String,
    pub category: String,
    pub subcategory: String,
    pub time_zone_id: String,
    pub trading_hours: String,
    pub liquid_hours: String,
    pub ev_rule: String,
    pub ev_multiplier: i32,
    pub agg_group: i32,
    pub under_symbol: String,
    pub under_sec_type: String,
    pub market_rule_ids: String,
    pub sec_id_list: SecIdList,
    pub real_expiration_date: String,
    pub last_trade_time: String,
    pub stock_type: String,
    pub min_size: f64,
    pub size_increment: f64,
    pub suggested_size_increment: f64,
    // Bond fields.
    pub cusip: String,
    pub ratings: String,
    pub desc_append: String,
    pub bond_type: String,
    pub coupon_type: String,
    pub callable: bool,
    pub putable: bool,
    pub coupon: f64,
    pub convertible: bool,
    pub maturity: String,
    pub issue_date: String,
    pub next_option_date: String,
    pub next_option_type: String,
    pub next_option_partial: bool,
    pub notes: String,
    // Fund fields (server version 179 and later, secType FUND).
    pub fund_name: String,
    pub fund_family: String,
    pub fund_type: String,
    pub fund_front_load: String,
    pub fund_back_load: String,
    pub fund_back_load_time_interval: String,
    pub fund_management_fee: String,
    pub fund_closed: bool,
    pub fund_closed_for_new_investors: bool,
    pub fund_closed_for_new_money: bool,
    pub fund_notify_amount: String,
    pub fund_minimum_initial_purchase: String,
    pub fund_subsequent_minimum_purchase: String,
    pub fund_blue_sky_states: String,
    pub fund_blue_sky_territories: String,
    pub fund_distribution_policy_indicator: String,
    pub fund_asset_type: String,
    pub ineligibility_reason_list: Vec<IneligibilityReason>,
    /// Event contracts (server 201+).
    pub event_contract1: String,
    pub event_contract_description1: String,
    pub event_contract_description2: String,
    pub min_algo_size: f64,
    pub last_price_precision: f64,
    pub last_size_precision: f64,
    /// "Physical" or "Cash" settlement on expiry (TWS API 10.52+).
    pub settlement_method: String,
}

/// Result row of `reqMatchingSymbols`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ContractDescription {
    pub contract: Contract,
    pub derivative_sec_types: Vec<String>,
}

/// Soft dollar tier of an order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SoftDollarTier {
    pub name: String,
    #[serde(rename = "val")]
    pub value: String,
    pub display_name: String,
}

/// Per-leg price of a combination order.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OrderComboLeg {
    pub price: f64,
}

impl Default for OrderComboLeg {
    fn default() -> Self {
        Self {
            price: UNSET_DOUBLE,
        }
    }
}

/// Condition that activates or cancels an order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "condType")]
pub enum OrderCondition {
    #[serde(rename = "1", rename_all = "camelCase")]
    Price {
        conjunction: String,
        is_more: bool,
        price: f64,
        con_id: i32,
        exch: String,
        trigger_method: i32,
    },
    #[serde(rename = "3", rename_all = "camelCase")]
    Time {
        conjunction: String,
        is_more: bool,
        time: String,
    },
    #[serde(rename = "4", rename_all = "camelCase")]
    Margin {
        conjunction: String,
        is_more: bool,
        percent: i32,
    },
    #[serde(rename = "5", rename_all = "camelCase")]
    Execution {
        conjunction: String,
        sec_type: String,
        exch: String,
        symbol: String,
    },
    #[serde(rename = "6", rename_all = "camelCase")]
    Volume {
        conjunction: String,
        is_more: bool,
        volume: i32,
        con_id: i32,
        exch: String,
    },
    #[serde(rename = "7", rename_all = "camelCase")]
    PercentChange {
        conjunction: String,
        is_more: bool,
        change_percent: f64,
        con_id: i32,
        exch: String,
    },
}

impl OrderCondition {
    /// Wire type number of the condition.
    pub fn cond_type(&self) -> i32 {
        match self {
            Self::Price { .. } => 1,
            Self::Time { .. } => 3,
            Self::Margin { .. } => 4,
            Self::Execution { .. } => 5,
            Self::Volume { .. } => 6,
            Self::PercentChange { .. } => 7,
        }
    }

    /// `"a"` (and) or `"o"` (or).
    pub fn conjunction(&self) -> &str {
        match self {
            Self::Price { conjunction, .. }
            | Self::Time { conjunction, .. }
            | Self::Margin { conjunction, .. }
            | Self::Execution { conjunction, .. }
            | Self::Volume { conjunction, .. }
            | Self::PercentChange { conjunction, .. } => conjunction,
        }
    }
}

/// An order. Defaults follow ib_async (`openClose` "O", `exemptCode` -1,
/// unset numeric fields at the IB sentinels).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Order {
    pub order_id: i32,
    pub client_id: i32,
    pub perm_id: i64,
    pub action: String,
    pub total_quantity: f64,
    pub order_type: String,
    pub lmt_price: f64,
    pub aux_price: f64,
    pub tif: String,
    pub active_start_time: String,
    pub active_stop_time: String,
    pub oca_group: String,
    pub oca_type: i32,
    pub order_ref: String,
    pub transmit: bool,
    pub parent_id: i32,
    pub block_order: bool,
    pub sweep_to_fill: bool,
    pub display_size: i32,
    pub trigger_method: i32,
    pub outside_rth: bool,
    pub hidden: bool,
    pub good_after_time: String,
    pub good_till_date: String,
    pub rule80_a: String,
    pub all_or_none: bool,
    pub min_qty: i32,
    pub percent_offset: f64,
    pub override_percentage_constraints: bool,
    pub trail_stop_price: f64,
    pub trailing_percent: f64,
    pub fa_group: String,
    pub fa_profile: String,
    pub fa_method: String,
    pub fa_percentage: String,
    pub designated_location: String,
    pub open_close: String,
    pub origin: i32,
    pub short_sale_slot: i32,
    pub exempt_code: i32,
    pub discretionary_amt: f64,
    pub e_trade_only: bool,
    pub firm_quote_only: bool,
    pub nbbo_price_cap: f64,
    pub opt_out_smart_routing: bool,
    pub auction_strategy: i32,
    pub starting_price: f64,
    pub stock_ref_price: f64,
    pub delta: f64,
    pub stock_range_lower: f64,
    pub stock_range_upper: f64,
    pub randomize_price: bool,
    pub randomize_size: bool,
    pub volatility: f64,
    pub volatility_type: i32,
    pub delta_neutral_order_type: String,
    pub delta_neutral_aux_price: f64,
    pub delta_neutral_con_id: i32,
    pub delta_neutral_settling_firm: String,
    pub delta_neutral_clearing_account: String,
    pub delta_neutral_clearing_intent: String,
    pub delta_neutral_open_close: String,
    pub delta_neutral_short_sale: bool,
    pub delta_neutral_short_sale_slot: i32,
    pub delta_neutral_designated_location: String,
    pub continuous_update: bool,
    pub reference_price_type: i32,
    pub basis_points: f64,
    pub basis_points_type: i32,
    pub scale_init_level_size: i32,
    pub scale_subs_level_size: i32,
    pub scale_price_increment: f64,
    pub scale_price_adjust_value: f64,
    pub scale_price_adjust_interval: i32,
    pub scale_profit_offset: f64,
    pub scale_auto_reset: bool,
    pub scale_init_position: i32,
    pub scale_init_fill_qty: i32,
    pub scale_random_percent: bool,
    pub scale_table: String,
    pub hedge_type: String,
    pub hedge_param: String,
    /// Largest number of shares a beta hedge child order may short (server 223+).
    pub hedge_max_size: i32,
    pub account: String,
    pub settling_firm: String,
    pub clearing_account: String,
    pub clearing_intent: String,
    pub algo_strategy: String,
    pub algo_params: Vec<TagValue>,
    pub smart_combo_routing_params: Vec<TagValue>,
    pub algo_id: String,
    pub what_if: bool,
    pub not_held: bool,
    pub solicited: bool,
    pub model_code: String,
    pub order_combo_legs: Vec<OrderComboLeg>,
    pub order_misc_options: Vec<TagValue>,
    pub reference_contract_id: i32,
    pub pegged_change_amount: f64,
    pub is_pegged_change_amount_decrease: bool,
    pub reference_change_amount: f64,
    pub reference_exchange_id: String,
    pub adjusted_order_type: String,
    pub trigger_price: f64,
    pub adjusted_stop_price: f64,
    pub adjusted_stop_limit_price: f64,
    pub adjusted_trailing_amount: f64,
    pub adjustable_trailing_unit: i32,
    pub lmt_price_offset: f64,
    pub conditions: Vec<OrderCondition>,
    pub conditions_cancel_order: bool,
    pub conditions_ignore_rth: bool,
    /// Conditions may also trigger in the overnight session (server 226+).
    pub conditions_include_overnight: bool,
    pub ext_operator: String,
    pub soft_dollar_tier: SoftDollarTier,
    pub cash_qty: f64,
    pub mifid2_decision_maker: String,
    pub mifid2_decision_algo: String,
    pub mifid2_execution_trader: String,
    pub mifid2_execution_algo: String,
    pub dont_use_auto_price_for_hedge: bool,
    pub is_oms_container: bool,
    pub discretionary_up_to_limit_price: bool,
    pub auto_cancel_date: String,
    pub filled_quantity: f64,
    pub ref_futures_con_id: i32,
    pub auto_cancel_parent: bool,
    pub shareholder: String,
    pub imbalance_only: bool,
    pub route_marketable_to_bbo: Option<bool>,
    pub parent_perm_id: i64,
    pub use_price_mgmt_algo: Option<bool>,
    pub duration: i32,
    pub post_to_ats: i32,
    pub advanced_error_override: String,
    pub manual_order_time: String,
    pub min_trade_qty: i32,
    pub min_compete_size: i32,
    pub compete_against_best_offset: f64,
    pub mid_offset_at_whole: f64,
    pub mid_offset_at_half: f64,
    pub customer_account: String,
    pub professional_customer: bool,
    pub bond_accrued_interest: String,
    pub include_overnight: bool,
    pub manual_order_indicator: i32,
    pub submitter: String,
    /// Server 216+.
    pub post_only: bool,
    pub allow_pre_open: bool,
    pub ignore_open_auction: bool,
    pub deactivate: bool,
    /// Server 217+.
    pub seek_price_improvement: Option<bool>,
    pub what_if_type: i32,
    /// Attached stop-loss and profit-taker orders (server 218+).
    pub sl_order_id: i32,
    pub sl_order_type: String,
    pub pt_order_id: i32,
    pub pt_order_type: String,
}

impl Default for Order {
    fn default() -> Self {
        Self {
            order_id: 0,
            client_id: 0,
            perm_id: 0,
            action: String::new(),
            total_quantity: 0.0,
            order_type: String::new(),
            lmt_price: UNSET_DOUBLE,
            aux_price: UNSET_DOUBLE,
            tif: String::new(),
            active_start_time: String::new(),
            active_stop_time: String::new(),
            oca_group: String::new(),
            oca_type: 0,
            order_ref: String::new(),
            transmit: true,
            parent_id: 0,
            block_order: false,
            sweep_to_fill: false,
            display_size: 0,
            trigger_method: 0,
            outside_rth: false,
            hidden: false,
            good_after_time: String::new(),
            good_till_date: String::new(),
            rule80_a: String::new(),
            all_or_none: false,
            min_qty: UNSET_INTEGER,
            percent_offset: UNSET_DOUBLE,
            override_percentage_constraints: false,
            trail_stop_price: UNSET_DOUBLE,
            trailing_percent: UNSET_DOUBLE,
            fa_group: String::new(),
            fa_profile: String::new(),
            fa_method: String::new(),
            fa_percentage: String::new(),
            designated_location: String::new(),
            open_close: "O".into(),
            origin: 0,
            short_sale_slot: 0,
            exempt_code: -1,
            discretionary_amt: 0.0,
            e_trade_only: false,
            firm_quote_only: false,
            nbbo_price_cap: UNSET_DOUBLE,
            opt_out_smart_routing: false,
            auction_strategy: 0,
            starting_price: UNSET_DOUBLE,
            stock_ref_price: UNSET_DOUBLE,
            delta: UNSET_DOUBLE,
            stock_range_lower: UNSET_DOUBLE,
            stock_range_upper: UNSET_DOUBLE,
            randomize_price: false,
            randomize_size: false,
            volatility: UNSET_DOUBLE,
            volatility_type: UNSET_INTEGER,
            delta_neutral_order_type: String::new(),
            delta_neutral_aux_price: UNSET_DOUBLE,
            delta_neutral_con_id: 0,
            delta_neutral_settling_firm: String::new(),
            delta_neutral_clearing_account: String::new(),
            delta_neutral_clearing_intent: String::new(),
            delta_neutral_open_close: String::new(),
            delta_neutral_short_sale: false,
            delta_neutral_short_sale_slot: 0,
            delta_neutral_designated_location: String::new(),
            continuous_update: false,
            reference_price_type: UNSET_INTEGER,
            basis_points: UNSET_DOUBLE,
            basis_points_type: UNSET_INTEGER,
            scale_init_level_size: UNSET_INTEGER,
            scale_subs_level_size: UNSET_INTEGER,
            scale_price_increment: UNSET_DOUBLE,
            scale_price_adjust_value: UNSET_DOUBLE,
            scale_price_adjust_interval: UNSET_INTEGER,
            scale_profit_offset: UNSET_DOUBLE,
            scale_auto_reset: false,
            scale_init_position: UNSET_INTEGER,
            scale_init_fill_qty: UNSET_INTEGER,
            scale_random_percent: false,
            scale_table: String::new(),
            hedge_type: String::new(),
            hedge_param: String::new(),
            hedge_max_size: UNSET_INTEGER,
            account: String::new(),
            settling_firm: String::new(),
            clearing_account: String::new(),
            clearing_intent: String::new(),
            algo_strategy: String::new(),
            algo_params: Vec::new(),
            smart_combo_routing_params: Vec::new(),
            algo_id: String::new(),
            what_if: false,
            not_held: false,
            solicited: false,
            model_code: String::new(),
            order_combo_legs: Vec::new(),
            order_misc_options: Vec::new(),
            reference_contract_id: 0,
            pegged_change_amount: 0.0,
            is_pegged_change_amount_decrease: false,
            reference_change_amount: 0.0,
            reference_exchange_id: String::new(),
            adjusted_order_type: String::new(),
            trigger_price: UNSET_DOUBLE,
            adjusted_stop_price: UNSET_DOUBLE,
            adjusted_stop_limit_price: UNSET_DOUBLE,
            adjusted_trailing_amount: UNSET_DOUBLE,
            adjustable_trailing_unit: 0,
            lmt_price_offset: UNSET_DOUBLE,
            conditions: Vec::new(),
            conditions_cancel_order: false,
            conditions_ignore_rth: false,
            conditions_include_overnight: false,
            ext_operator: String::new(),
            soft_dollar_tier: SoftDollarTier::default(),
            cash_qty: UNSET_DOUBLE,
            mifid2_decision_maker: String::new(),
            mifid2_decision_algo: String::new(),
            mifid2_execution_trader: String::new(),
            mifid2_execution_algo: String::new(),
            dont_use_auto_price_for_hedge: false,
            is_oms_container: false,
            discretionary_up_to_limit_price: false,
            auto_cancel_date: String::new(),
            filled_quantity: UNSET_DOUBLE,
            ref_futures_con_id: 0,
            auto_cancel_parent: false,
            shareholder: String::new(),
            imbalance_only: false,
            route_marketable_to_bbo: None,
            parent_perm_id: 0,
            use_price_mgmt_algo: None,
            duration: UNSET_INTEGER,
            post_to_ats: UNSET_INTEGER,
            advanced_error_override: String::new(),
            manual_order_time: String::new(),
            min_trade_qty: UNSET_INTEGER,
            min_compete_size: UNSET_INTEGER,
            compete_against_best_offset: UNSET_DOUBLE,
            mid_offset_at_whole: UNSET_DOUBLE,
            mid_offset_at_half: UNSET_DOUBLE,
            customer_account: String::new(),
            professional_customer: false,
            bond_accrued_interest: String::new(),
            include_overnight: false,
            manual_order_indicator: UNSET_INTEGER,
            submitter: String::new(),
            post_only: false,
            allow_pre_open: false,
            ignore_open_auction: false,
            deactivate: false,
            seek_price_improvement: None,
            what_if_type: UNSET_INTEGER,
            sl_order_id: UNSET_INTEGER,
            sl_order_type: String::new(),
            pt_order_id: UNSET_INTEGER,
            pt_order_type: String::new(),
        }
    }
}

/// Allocation of an order to one account (full preview fields, server 195).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OrderAllocation {
    pub account: String,
    pub position: f64,
    pub position_desired: f64,
    pub position_after: f64,
    pub desired_alloc_qty: f64,
    pub allowed_alloc_qty: f64,
    pub is_monetary: bool,
}

/// State of an order, including what-if margin and commission figures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OrderState {
    pub status: String,
    pub init_margin_before: String,
    pub maint_margin_before: String,
    pub equity_with_loan_before: String,
    pub init_margin_change: String,
    pub maint_margin_change: String,
    pub equity_with_loan_change: String,
    pub init_margin_after: String,
    pub maint_margin_after: String,
    pub equity_with_loan_after: String,
    pub commission: f64,
    pub min_commission: f64,
    pub max_commission: f64,
    pub commission_currency: String,
    pub margin_currency: String,
    pub init_margin_before_outside_rth: f64,
    pub maint_margin_before_outside_rth: f64,
    pub equity_with_loan_before_outside_rth: f64,
    pub init_margin_change_outside_rth: f64,
    pub maint_margin_change_outside_rth: f64,
    pub equity_with_loan_change_outside_rth: f64,
    pub init_margin_after_outside_rth: f64,
    pub maint_margin_after_outside_rth: f64,
    pub equity_with_loan_after_outside_rth: f64,
    pub suggested_size: f64,
    pub reject_reason: String,
    pub order_allocations: Vec<OrderAllocation>,
    pub warning_text: String,
    pub completed_time: String,
    pub completed_status: String,
}

impl Default for OrderState {
    fn default() -> Self {
        Self {
            status: String::new(),
            init_margin_before: String::new(),
            maint_margin_before: String::new(),
            equity_with_loan_before: String::new(),
            init_margin_change: String::new(),
            maint_margin_change: String::new(),
            equity_with_loan_change: String::new(),
            init_margin_after: String::new(),
            maint_margin_after: String::new(),
            equity_with_loan_after: String::new(),
            commission: UNSET_DOUBLE,
            min_commission: UNSET_DOUBLE,
            max_commission: UNSET_DOUBLE,
            commission_currency: String::new(),
            margin_currency: String::new(),
            init_margin_before_outside_rth: UNSET_DOUBLE,
            maint_margin_before_outside_rth: UNSET_DOUBLE,
            equity_with_loan_before_outside_rth: UNSET_DOUBLE,
            init_margin_change_outside_rth: UNSET_DOUBLE,
            maint_margin_change_outside_rth: UNSET_DOUBLE,
            equity_with_loan_change_outside_rth: UNSET_DOUBLE,
            init_margin_after_outside_rth: UNSET_DOUBLE,
            maint_margin_after_outside_rth: UNSET_DOUBLE,
            equity_with_loan_after_outside_rth: UNSET_DOUBLE,
            suggested_size: UNSET_DOUBLE,
            reject_reason: String::new(),
            order_allocations: Vec::new(),
            warning_text: String::new(),
            completed_time: String::new(),
            completed_status: String::new(),
        }
    }
}

/// Order fields used when cancelling (server 169 and 192).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OrderCancel {
    pub manual_order_cancel_time: String,
    pub ext_operator: String,
    pub manual_order_indicator: i32,
}

impl Default for OrderCancel {
    fn default() -> Self {
        Self {
            manual_order_cancel_time: String::new(),
            ext_operator: String::new(),
            manual_order_indicator: UNSET_INTEGER,
        }
    }
}

/// One execution (fill) of an order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Execution {
    pub exec_id: String,
    /// Raw server time string, for example `"20231101 10:00:00 US/Eastern"`.
    pub time: String,
    pub acct_number: String,
    pub exchange: String,
    pub side: String,
    pub shares: f64,
    pub price: f64,
    pub perm_id: i64,
    pub client_id: i32,
    pub order_id: i32,
    pub liquidation: i32,
    pub cum_qty: f64,
    pub avg_price: f64,
    pub order_ref: String,
    pub ev_rule: String,
    pub ev_multiplier: f64,
    pub model_code: String,
    pub last_liquidity: i32,
    pub pending_price_revision: bool,
    pub submitter: String,
    /// Option exercise or lapse type (server 201+; -1 none, 1 exercise, 2 lapse, 3 do nothing, 100 assigned, ...).
    pub opt_exercise_or_lapse_type: i32,
}

/// Filter for `reqExecutions`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExecutionFilter {
    pub client_id: i32,
    pub acct_code: String,
    pub time: String,
    pub symbol: String,
    pub sec_type: String,
    pub exchange: String,
    pub side: String,
    pub last_n_days: i32,
    pub specific_dates: Vec<i32>,
}

impl Default for ExecutionFilter {
    fn default() -> Self {
        Self {
            client_id: 0,
            acct_code: String::new(),
            time: String::new(),
            symbol: String::new(),
            sec_type: String::new(),
            exchange: String::new(),
            side: String::new(),
            last_n_days: UNSET_INTEGER,
            specific_dates: Vec::new(),
        }
    }
}

/// Commission and fees of one execution.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CommissionReport {
    pub exec_id: String,
    pub commission: f64,
    pub currency: String,
    #[serde(rename = "realizedPNL")]
    pub realized_pnl: f64,
    #[serde(rename = "yield_")]
    pub yield_: f64,
    pub yield_redemption_date: i32,
}

/// A historical bar.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BarData {
    pub date: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    /// Volume weighted average price (`wap` in IB's client, `average` in ib_async).
    pub average: f64,
    pub bar_count: i32,
}

/// A five-second real time bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RealTimeBar {
    pub time: i64,
    #[serde(rename = "open_")]
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub wap: f64,
    pub count: i32,
}

/// Attributes of a price tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TickAttrib {
    pub can_auto_execute: bool,
    pub past_limit: bool,
    pub pre_open: bool,
}

/// Attributes of a bid/ask tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TickAttribBidAsk {
    pub bid_past_low: bool,
    pub ask_past_high: bool,
}

/// Attributes of a trade tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TickAttribLast {
    pub past_limit: bool,
    pub unreported: bool,
}

/// Historical midpoint tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HistoricalTick {
    pub time: i64,
    pub price: f64,
    pub size: f64,
}

/// Historical bid/ask tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HistoricalTickBidAsk {
    pub time: i64,
    pub tick_attrib_bid_ask: TickAttribBidAsk,
    pub price_bid: f64,
    pub price_ask: f64,
    pub size_bid: f64,
    pub size_ask: f64,
}

/// Historical trade tick.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HistoricalTickLast {
    pub time: i64,
    pub tick_attrib_last: TickAttribLast,
    pub price: f64,
    pub size: f64,
    pub exchange: String,
    pub special_conditions: String,
}

/// One trading session in a historical schedule.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HistoricalSession {
    pub start_date_time: String,
    pub end_date_time: String,
    pub ref_date: String,
}

/// Market scanner subscription parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScannerSubscription {
    pub number_of_rows: i32,
    pub instrument: String,
    pub location_code: String,
    pub scan_code: String,
    pub above_price: f64,
    pub below_price: f64,
    pub above_volume: i32,
    pub market_cap_above: f64,
    pub market_cap_below: f64,
    pub moody_rating_above: String,
    pub moody_rating_below: String,
    pub sp_rating_above: String,
    pub sp_rating_below: String,
    pub maturity_date_above: String,
    pub maturity_date_below: String,
    pub coupon_rate_above: f64,
    pub coupon_rate_below: f64,
    pub exclude_convertible: bool,
    pub average_option_volume_above: i32,
    pub scanner_setting_pairs: String,
    pub stock_type_filter: String,
}

impl Default for ScannerSubscription {
    fn default() -> Self {
        Self {
            number_of_rows: -1,
            instrument: String::new(),
            location_code: String::new(),
            scan_code: String::new(),
            above_price: UNSET_DOUBLE,
            below_price: UNSET_DOUBLE,
            above_volume: UNSET_INTEGER,
            market_cap_above: UNSET_DOUBLE,
            market_cap_below: UNSET_DOUBLE,
            moody_rating_above: String::new(),
            moody_rating_below: String::new(),
            sp_rating_above: String::new(),
            sp_rating_below: String::new(),
            maturity_date_above: String::new(),
            maturity_date_below: String::new(),
            coupon_rate_above: UNSET_DOUBLE,
            coupon_rate_below: UNSET_DOUBLE,
            exclude_convertible: false,
            average_option_volume_above: UNSET_INTEGER,
            scanner_setting_pairs: String::new(),
            stock_type_filter: String::new(),
        }
    }
}

/// One row of scanner results.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScanData {
    pub rank: i32,
    pub contract_details: ContractDetails,
    pub distance: String,
    pub benchmark: String,
    pub projection: String,
    pub legs_str: String,
}

/// Wall Street Horizon event data request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WshEventData {
    pub con_id: i32,
    pub filter: String,
    pub fill_watchlist: bool,
    pub fill_portfolio: bool,
    pub fill_competitors: bool,
    pub start_date: String,
    pub end_date: String,
    pub total_limit: i32,
}

impl Default for WshEventData {
    fn default() -> Self {
        Self {
            con_id: UNSET_INTEGER,
            filter: String::new(),
            fill_watchlist: false,
            fill_portfolio: false,
            fill_competitors: false,
            start_date: String::new(),
            end_date: String::new(),
            total_limit: UNSET_INTEGER,
        }
    }
}

/// Family code of an account.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FamilyCode {
    #[serde(rename = "accountID")]
    pub account_id: String,
    #[serde(rename = "familyCodeStr")]
    pub family_code: String,
}

/// Exchange that offers market depth.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DepthMktDataDescription {
    pub exchange: String,
    pub sec_type: String,
    pub listing_exch: String,
    pub service_data_type: String,
    pub agg_group: i32,
}

impl Default for DepthMktDataDescription {
    fn default() -> Self {
        Self {
            exchange: String::new(),
            sec_type: String::new(),
            listing_exch: String::new(),
            service_data_type: String::new(),
            agg_group: UNSET_INTEGER,
        }
    }
}

/// Component exchange of a SMART route.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SmartComponent {
    pub bit_number: i32,
    pub exchange: String,
    pub exchange_letter: String,
}

/// News provider.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NewsProvider {
    pub code: String,
    pub name: String,
}

/// One histogram bucket.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HistogramData {
    pub price: f64,
    pub count: f64,
}

/// One price increment rule of a market rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PriceIncrement {
    pub low_edge: f64,
    pub increment: f64,
}

fn base(sec_type: &str, symbol: &str, exchange: &str, currency: &str) -> Contract {
    Contract {
        sec_type: sec_type.into(),
        symbol: symbol.into(),
        exchange: exchange.into(),
        currency: currency.into(),
        ..Default::default()
    }
}

/// Constructors for each security type (ib_async `Stock`, `Option`, ...).
impl Contract {
    pub fn stock(symbol: &str, exchange: &str, currency: &str) -> Contract {
        base("STK", symbol, exchange, currency)
    }
    pub fn option(
        symbol: &str,
        last_trade_date: &str,
        strike: f64,
        right: &str,
        exchange: &str,
        multiplier: &str,
        currency: &str,
    ) -> Contract {
        Contract {
            last_trade_date_or_contract_month: last_trade_date.into(),
            strike,
            right: right.into(),
            multiplier: multiplier.into(),
            ..base("OPT", symbol, exchange, currency)
        }
    }
    pub fn future(
        symbol: &str,
        last_trade_date: &str,
        exchange: &str,
        local_symbol: &str,
        multiplier: &str,
        currency: &str,
    ) -> Contract {
        Contract {
            last_trade_date_or_contract_month: last_trade_date.into(),
            local_symbol: local_symbol.into(),
            multiplier: multiplier.into(),
            ..base("FUT", symbol, exchange, currency)
        }
    }
    pub fn cont_future(
        symbol: &str,
        exchange: &str,
        local_symbol: &str,
        multiplier: &str,
        currency: &str,
    ) -> Contract {
        Contract {
            local_symbol: local_symbol.into(),
            multiplier: multiplier.into(),
            ..base("CONTFUT", symbol, exchange, currency)
        }
    }
    /// `pair` such as `"EURUSD"`; exchange defaults to IDEALPRO when empty.
    pub fn forex(pair: &str, exchange: &str) -> Contract {
        let (symbol, currency) = if pair.len() == 6 {
            pair.split_at(3)
        } else {
            (pair, "")
        };
        base(
            "CASH",
            symbol,
            if exchange.is_empty() {
                "IDEALPRO"
            } else {
                exchange
            },
            currency,
        )
    }
    pub fn index(symbol: &str, exchange: &str, currency: &str) -> Contract {
        base("IND", symbol, exchange, currency)
    }
    pub fn cfd(symbol: &str, exchange: &str, currency: &str) -> Contract {
        base("CFD", symbol, exchange, currency)
    }
    pub fn commodity(symbol: &str, exchange: &str, currency: &str) -> Contract {
        base("CMDTY", symbol, exchange, currency)
    }
    pub fn bond(symbol: &str, exchange: &str, currency: &str) -> Contract {
        base("BOND", symbol, exchange, currency)
    }
    pub fn futures_option(
        symbol: &str,
        last_trade_date: &str,
        strike: f64,
        right: &str,
        exchange: &str,
        multiplier: &str,
        currency: &str,
    ) -> Contract {
        Contract {
            sec_type: "FOP".into(),
            ..Self::option(
                symbol,
                last_trade_date,
                strike,
                right,
                exchange,
                multiplier,
                currency,
            )
        }
    }
    pub fn mutual_fund(symbol: &str, exchange: &str, currency: &str) -> Contract {
        base("FUND", symbol, exchange, currency)
    }
    pub fn warrant(symbol: &str, exchange: &str, currency: &str) -> Contract {
        base("WAR", symbol, exchange, currency)
    }
    pub fn bag(symbol: &str, exchange: &str, currency: &str) -> Contract {
        base("BAG", symbol, exchange, currency)
    }
    pub fn crypto(symbol: &str, exchange: &str, currency: &str) -> Contract {
        base("CRYPTO", symbol, exchange, currency)
    }
    /// Currency pair of a CASH contract, for example `"EURUSD"`.
    pub fn pair(&self) -> String {
        if self.sec_type == "CASH" {
            format!("{}{}", self.symbol, self.currency)
        } else {
            String::new()
        }
    }
    /// Key used to share one ticker per contract (ib_async `hash(contract)`).
    pub fn key(&self) -> String {
        if self.con_id != 0 && self.sec_type != "BAG" {
            if self.sec_type == "CONTFUT" {
                format!("CONTFUT:{}", self.con_id)
            } else {
                self.con_id.to_string()
            }
        } else {
            let legs: Vec<String> = self
                .combo_legs
                .iter()
                .map(|l| format!("{}x{}{}", l.con_id, l.ratio, l.action))
                .collect();
            format!(
                "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
                self.sec_type,
                self.symbol,
                self.last_trade_date_or_contract_month,
                self.strike,
                self.right,
                self.multiplier,
                self.exchange,
                self.primary_exchange,
                self.currency,
                self.local_symbol,
                legs.join(",")
            )
        }
    }
}

/// Constructors for common order types (ib_async `LimitOrder`, ...).
impl Order {
    pub fn limit(action: &str, quantity: f64, limit_price: f64) -> Order {
        Order {
            order_type: "LMT".into(),
            action: action.into(),
            total_quantity: quantity,
            lmt_price: limit_price,
            ..Default::default()
        }
    }
    pub fn market(action: &str, quantity: f64) -> Order {
        Order {
            order_type: "MKT".into(),
            action: action.into(),
            total_quantity: quantity,
            ..Default::default()
        }
    }
    pub fn stop(action: &str, quantity: f64, stop_price: f64) -> Order {
        Order {
            order_type: "STP".into(),
            action: action.into(),
            total_quantity: quantity,
            aux_price: stop_price,
            ..Default::default()
        }
    }
    pub fn stop_limit(action: &str, quantity: f64, limit_price: f64, stop_price: f64) -> Order {
        Order {
            order_type: "STP LMT".into(),
            action: action.into(),
            total_quantity: quantity,
            lmt_price: limit_price,
            aux_price: stop_price,
            ..Default::default()
        }
    }
}
