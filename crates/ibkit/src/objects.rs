//! Value objects produced by the high-level client (ib_async `objects.py`).

use ibkit_tws::types::*;
use serde::{Deserialize, Serialize};

/// Seconds since the Unix epoch.
pub type Timestamp = f64;

/// Current time as seconds since the Unix epoch.
pub fn now() -> Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default()
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountValue {
    pub account: String,
    pub tag: String,
    pub value: String,
    pub currency: String,
    pub model_code: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub account: String,
    pub contract: Contract,
    pub position: f64,
    pub avg_cost: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioItem {
    pub contract: Contract,
    pub position: f64,
    pub market_price: f64,
    pub market_value: f64,
    pub average_cost: f64,
    #[serde(rename = "unrealizedPNL")]
    pub unrealized_pnl: f64,
    #[serde(rename = "realizedPNL")]
    pub realized_pnl: f64,
    pub account: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PnL {
    pub account: String,
    pub model_code: String,
    #[serde(rename = "dailyPnL")]
    pub daily_pnl: f64,
    #[serde(rename = "unrealizedPnL")]
    pub unrealized_pnl: f64,
    #[serde(rename = "realizedPnL")]
    pub realized_pnl: f64,
}

impl PnL {
    pub fn new(account: &str, model_code: &str) -> Self {
        Self {
            account: account.into(),
            model_code: model_code.into(),
            daily_pnl: f64::NAN,
            unrealized_pnl: f64::NAN,
            realized_pnl: f64::NAN,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PnLSingle {
    pub account: String,
    pub model_code: String,
    pub con_id: i32,
    #[serde(rename = "dailyPnL")]
    pub daily_pnl: f64,
    #[serde(rename = "unrealizedPnL")]
    pub unrealized_pnl: f64,
    #[serde(rename = "realizedPnL")]
    pub realized_pnl: f64,
    pub position: f64,
    pub value: f64,
}

impl PnLSingle {
    pub fn new(account: &str, model_code: &str, con_id: i32) -> Self {
        Self {
            account: account.into(),
            model_code: model_code.into(),
            con_id,
            daily_pnl: f64::NAN,
            unrealized_pnl: f64::NAN,
            realized_pnl: f64::NAN,
            position: 0.0,
            value: f64::NAN,
        }
    }
}

/// An execution with its commission report.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fill {
    pub contract: Contract,
    pub execution: Execution,
    pub commission_report: CommissionReport,
    pub time: Timestamp,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeLogEntry {
    pub time: Timestamp,
    pub status: String,
    pub message: String,
    pub error_code: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TickData {
    pub time: Timestamp,
    pub tick_type: i32,
    pub price: f64,
    pub size: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TickByTick {
    #[serde(rename_all = "camelCase")]
    AllLast {
        tick_type: i32,
        time: Timestamp,
        price: f64,
        size: f64,
        tick_attrib_last: TickAttribLast,
        exchange: String,
        special_conditions: String,
    },
    #[serde(rename_all = "camelCase")]
    BidAsk {
        time: Timestamp,
        bid_price: f64,
        ask_price: f64,
        bid_size: f64,
        ask_size: f64,
        tick_attrib_bid_ask: TickAttribBidAsk,
    },
    #[serde(rename_all = "camelCase")]
    MidPoint { time: Timestamp, mid_point: f64 },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DOMLevel {
    pub price: f64,
    pub size: f64,
    pub market_maker: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MktDepthData {
    pub time: Timestamp,
    pub position: i32,
    pub market_maker: String,
    pub operation: i32,
    pub side: i32,
    pub price: f64,
    pub size: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dividends {
    pub past12_months: Option<f64>,
    pub next12_months: Option<f64>,
    pub next_date: Option<String>,
    pub next_amount: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionChain {
    pub exchange: String,
    pub underlying_con_id: i32,
    pub trading_class: String,
    pub multiplier: String,
    pub expirations: Vec<String>,
    pub strikes: Vec<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsTick {
    pub time_stamp: i64,
    pub provider_code: String,
    pub article_id: String,
    pub headline: String,
    pub extra_data: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsBulletin {
    pub msg_id: i32,
    pub msg_type: i32,
    pub message: String,
    pub orig_exchange: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsArticle {
    pub article_type: i32,
    pub article_text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalNews {
    pub time: String,
    pub provider_code: String,
    pub article_id: String,
    pub headline: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalSchedule {
    pub start_date_time: String,
    pub end_date_time: String,
    pub time_zone: String,
    pub sessions: Vec<HistoricalSession>,
}

/// Bars of a historical data request; live when `keep_up_to_date` is set.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BarDataList {
    pub req_id: i32,
    pub contract: Contract,
    pub end_date_time: String,
    pub duration_str: String,
    pub bar_size_setting: String,
    pub what_to_show: String,
    pub use_rth: bool,
    pub format_date: i32,
    pub keep_up_to_date: bool,
    pub chart_options: Vec<TagValue>,
    pub bars: Vec<BarData>,
}

/// Five-second bars of a real time bar subscription.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RealTimeBarList {
    pub req_id: i32,
    pub contract: Contract,
    pub bar_size: i32,
    pub what_to_show: String,
    pub use_rth: bool,
    pub real_time_bars_options: Vec<TagValue>,
    pub bars: Vec<RealTimeBar>,
}

/// Results of a scanner subscription.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanDataList {
    pub req_id: i32,
    pub subscription: ScannerSubscription,
    pub rows: Vec<ScanData>,
}
