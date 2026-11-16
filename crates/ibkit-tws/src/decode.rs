//! Decoder for every server message of the TWS API text protocol.
//!
//! [`decode`] turns one frame payload into an [`Incoming`] value. Layouts and
//! version gates follow IB's official decoder and `OrderDecoder` for server
//! versions 157 through 200.

use crate::consts::UNSET_DOUBLE;
use crate::encode::is_peg_bench;
use crate::message::incoming as msg;
use crate::server_versions as sv;
use crate::types::*;
use crate::wire::{DecodeError, FieldReader};

/// Order status update (`orderStatus` callback).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderStatusMsg {
    pub order_id: i32,
    pub status: String,
    pub filled: f64,
    pub remaining: f64,
    pub avg_fill_price: f64,
    pub perm_id: i64,
    pub parent_id: i32,
    pub last_fill_price: f64,
    pub client_id: i32,
    pub why_held: String,
    pub mkt_cap_price: f64,
}

/// Error or informational notice (`error` callback).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorMsg {
    pub req_id: i32,
    pub code: i32,
    pub message: String,
    pub advanced_order_reject_json: String,
    /// Milliseconds since the epoch; 0 below server version 194.
    pub error_time: i64,
}

/// Option greeks (`tickOptionComputation` callback). `None` marks values the
/// server reports as "not computed".
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionComputation {
    pub tick_attrib: Option<i32>,
    pub implied_vol: Option<f64>,
    pub delta: Option<f64>,
    pub opt_price: Option<f64>,
    pub pv_dividend: Option<f64>,
    pub gamma: Option<f64>,
    pub vega: Option<f64>,
    pub theta: Option<f64>,
    pub und_price: Option<f64>,
}

/// EFP tick (`tickEFP` callback).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EfpData {
    pub basis_points: f64,
    pub formatted_basis_points: String,
    pub implied_future: f64,
    pub hold_days: i32,
    pub future_last_trade_date: String,
    pub dividend_impact: f64,
    pub dividends_to_last_trade_date: f64,
}

/// Portfolio update (`updatePortfolio` callback).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioMsg {
    pub contract: Contract,
    pub position: f64,
    pub market_price: f64,
    pub market_value: f64,
    pub average_cost: f64,
    pub unrealized_pnl: f64,
    pub realized_pnl: f64,
    pub account: String,
}

/// Option chain parameters (`securityDefinitionOptionParameter` callback).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecDefOptParams {
    pub req_id: i32,
    pub exchange: String,
    pub underlying_con_id: i32,
    pub trading_class: String,
    pub multiplier: String,
    pub expirations: Vec<String>,
    pub strikes: Vec<f64>,
}

/// One message from the server. Variant names follow IB's `EWrapper`
/// callbacks; messages that carry a list deliver the whole list at once.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all_fields = "camelCase")]
pub enum Incoming {
    TickPrice {
        req_id: i32,
        tick_type: i32,
        price: f64,
        size: f64,
        attrib: TickAttrib,
    },
    TickSize {
        req_id: i32,
        tick_type: i32,
        size: f64,
    },
    TickOptionComputation {
        req_id: i32,
        tick_type: i32,
        computation: OptionComputation,
    },
    TickGeneric {
        req_id: i32,
        tick_type: i32,
        value: f64,
    },
    TickString {
        req_id: i32,
        tick_type: i32,
        value: String,
    },
    TickEfp {
        req_id: i32,
        tick_type: i32,
        efp: EfpData,
    },
    OrderStatus(OrderStatusMsg),
    Error(ErrorMsg),
    OpenOrder {
        order_id: i32,
        contract: Box<Contract>,
        order: Box<Order>,
        state: Box<OrderState>,
    },
    UpdateAccountValue {
        key: String,
        value: String,
        currency: String,
        account: String,
    },
    UpdatePortfolio(Box<PortfolioMsg>),
    UpdateAccountTime {
        timestamp: String,
    },
    NextValidId {
        order_id: i32,
    },
    ContractDetails {
        req_id: i32,
        details: Box<ContractDetails>,
    },
    BondContractDetails {
        req_id: i32,
        details: Box<ContractDetails>,
    },
    ExecDetails {
        req_id: i32,
        contract: Box<Contract>,
        execution: Box<Execution>,
    },
    UpdateMktDepth {
        req_id: i32,
        position: i32,
        operation: i32,
        side: i32,
        price: f64,
        size: f64,
    },
    UpdateMktDepthL2 {
        req_id: i32,
        position: i32,
        market_maker: String,
        operation: i32,
        side: i32,
        price: f64,
        size: f64,
        is_smart_depth: bool,
    },
    UpdateNewsBulletin {
        msg_id: i32,
        msg_type: i32,
        message: String,
        orig_exchange: String,
    },
    ManagedAccounts {
        accounts: String,
    },
    ReceiveFa {
        fa_data_type: i32,
        xml: String,
    },
    /// Bars of one `HISTORICAL_DATA` message. Below server version 196 the
    /// start and end strings arrive here and no separate end message follows.
    HistoricalData {
        req_id: i32,
        bars: Vec<BarData>,
        end: Option<(String, String)>,
    },
    HistoricalDataEnd {
        req_id: i32,
        start: String,
        end: String,
    },
    HistoricalDataUpdate {
        req_id: i32,
        bar: BarData,
    },
    ScannerParameters {
        xml: String,
    },
    ScannerData {
        req_id: i32,
        rows: Vec<ScanData>,
    },
    CurrentTime {
        time: i64,
    },
    CurrentTimeInMillis {
        time: i64,
    },
    RealtimeBar {
        req_id: i32,
        bar: RealTimeBar,
    },
    FundamentalData {
        req_id: i32,
        data: String,
    },
    ContractDetailsEnd {
        req_id: i32,
    },
    OpenOrderEnd,
    AccountDownloadEnd {
        account: String,
    },
    ExecDetailsEnd {
        req_id: i32,
    },
    DeltaNeutralValidation {
        req_id: i32,
        contract: DeltaNeutralContract,
    },
    TickSnapshotEnd {
        req_id: i32,
    },
    MarketDataType {
        req_id: i32,
        market_data_type: i32,
    },
    CommissionReport(CommissionReport),
    Position {
        account: String,
        contract: Box<Contract>,
        position: f64,
        avg_cost: f64,
    },
    PositionEnd,
    AccountSummary {
        req_id: i32,
        account: String,
        tag: String,
        value: String,
        currency: String,
    },
    AccountSummaryEnd {
        req_id: i32,
    },
    VerifyMessageApi {
        api_data: String,
    },
    VerifyCompleted {
        is_successful: bool,
        error_text: String,
    },
    DisplayGroupList {
        req_id: i32,
        groups: String,
    },
    DisplayGroupUpdated {
        req_id: i32,
        contract_info: String,
    },
    VerifyAndAuthMessageApi {
        api_data: String,
        xyz_challenge: String,
    },
    VerifyAndAuthCompleted {
        is_successful: bool,
        error_text: String,
    },
    PositionMulti {
        req_id: i32,
        account: String,
        model_code: String,
        contract: Box<Contract>,
        position: f64,
        avg_cost: f64,
    },
    PositionMultiEnd {
        req_id: i32,
    },
    AccountUpdateMulti {
        req_id: i32,
        account: String,
        model_code: String,
        key: String,
        value: String,
        currency: String,
    },
    AccountUpdateMultiEnd {
        req_id: i32,
    },
    SecurityDefinitionOptionParameter(Box<SecDefOptParams>),
    SecurityDefinitionOptionParameterEnd {
        req_id: i32,
    },
    SoftDollarTiers {
        req_id: i32,
        tiers: Vec<SoftDollarTier>,
    },
    FamilyCodes {
        codes: Vec<FamilyCode>,
    },
    SymbolSamples {
        req_id: i32,
        descriptions: Vec<ContractDescription>,
    },
    MktDepthExchanges {
        descriptions: Vec<DepthMktDataDescription>,
    },
    TickReqParams {
        req_id: i32,
        min_tick: f64,
        bbo_exchange: String,
        snapshot_permissions: i32,
    },
    SmartComponents {
        req_id: i32,
        components: Vec<SmartComponent>,
    },
    NewsArticle {
        req_id: i32,
        article_type: i32,
        article_text: String,
    },
    TickNews {
        req_id: i32,
        time_stamp: i64,
        provider_code: String,
        article_id: String,
        headline: String,
        extra_data: String,
    },
    NewsProviders {
        providers: Vec<NewsProvider>,
    },
    HistoricalNews {
        req_id: i32,
        time: String,
        provider_code: String,
        article_id: String,
        headline: String,
    },
    HistoricalNewsEnd {
        req_id: i32,
        has_more: bool,
    },
    HeadTimestamp {
        req_id: i32,
        head_timestamp: String,
    },
    HistogramData {
        req_id: i32,
        items: Vec<HistogramData>,
    },
    RerouteMktDataReq {
        req_id: i32,
        con_id: i32,
        exchange: String,
    },
    RerouteMktDepthReq {
        req_id: i32,
        con_id: i32,
        exchange: String,
    },
    MarketRule {
        market_rule_id: i32,
        increments: Vec<PriceIncrement>,
    },
    Pnl {
        req_id: i32,
        daily_pnl: f64,
        unrealized_pnl: f64,
        realized_pnl: f64,
    },
    PnlSingle {
        req_id: i32,
        position: f64,
        daily_pnl: f64,
        unrealized_pnl: f64,
        realized_pnl: f64,
        value: f64,
    },
    HistoricalTicks {
        req_id: i32,
        ticks: Vec<HistoricalTick>,
        done: bool,
    },
    HistoricalTicksBidAsk {
        req_id: i32,
        ticks: Vec<HistoricalTickBidAsk>,
        done: bool,
    },
    HistoricalTicksLast {
        req_id: i32,
        ticks: Vec<HistoricalTickLast>,
        done: bool,
    },
    TickByTickAllLast {
        req_id: i32,
        tick_type: i32,
        time: i64,
        price: f64,
        size: f64,
        attrib: TickAttribLast,
        exchange: String,
        special_conditions: String,
    },
    TickByTickBidAsk {
        req_id: i32,
        time: i64,
        bid_price: f64,
        ask_price: f64,
        bid_size: f64,
        ask_size: f64,
        attrib: TickAttribBidAsk,
    },
    TickByTickMidPoint {
        req_id: i32,
        time: i64,
        mid_point: f64,
    },
    OrderBound {
        perm_id: i64,
        client_id: i32,
        order_id: i32,
    },
    CompletedOrder {
        contract: Box<Contract>,
        order: Box<Order>,
        state: Box<OrderState>,
    },
    CompletedOrdersEnd,
    ReplaceFaEnd {
        req_id: i32,
        text: String,
    },
    WshMetaData {
        req_id: i32,
        data_json: String,
    },
    WshEventData {
        req_id: i32,
        data_json: String,
    },
    HistoricalSchedule {
        req_id: i32,
        start_date_time: String,
        end_date_time: String,
        time_zone: String,
        sessions: Vec<HistoricalSession>,
    },
    UserInfo {
        req_id: i32,
        white_branding_id: String,
    },
    /// TWS API configuration (`reqConfig`, server 219+), as the JSON form of
    /// IB's `ConfigResponse`.
    ConfigResponse {
        req_id: i32,
        config: serde_json::Value,
    },
    /// Answer to `updateConfig` (server 221+), the JSON form of IB's
    /// `UpdateConfigResponse` (`status`, `message`, `changedFields`, ...).
    UpdateConfigResponse {
        req_id: i32,
        response: serde_json::Value,
    },
    /// A message id this decoder does not know. The raw fields are kept.
    Unknown {
        msg_id: i32,
        fields: Vec<String>,
    },
}

impl Incoming {
    /// The request id the message belongs to, if it has one.
    pub fn req_id(&self) -> Option<i32> {
        use Incoming::*;
        Some(match self {
            TickPrice { req_id, .. }
            | TickSize { req_id, .. }
            | TickOptionComputation { req_id, .. }
            | TickGeneric { req_id, .. }
            | TickString { req_id, .. }
            | TickEfp { req_id, .. }
            | ContractDetails { req_id, .. }
            | BondContractDetails { req_id, .. }
            | ExecDetails { req_id, .. }
            | UpdateMktDepth { req_id, .. }
            | UpdateMktDepthL2 { req_id, .. }
            | HistoricalData { req_id, .. }
            | HistoricalDataEnd { req_id, .. }
            | HistoricalDataUpdate { req_id, .. }
            | ScannerData { req_id, .. }
            | RealtimeBar { req_id, .. }
            | FundamentalData { req_id, .. }
            | ContractDetailsEnd { req_id }
            | ExecDetailsEnd { req_id }
            | DeltaNeutralValidation { req_id, .. }
            | TickSnapshotEnd { req_id }
            | MarketDataType { req_id, .. }
            | AccountSummary { req_id, .. }
            | AccountSummaryEnd { req_id }
            | DisplayGroupList { req_id, .. }
            | DisplayGroupUpdated { req_id, .. }
            | PositionMulti { req_id, .. }
            | PositionMultiEnd { req_id }
            | AccountUpdateMulti { req_id, .. }
            | AccountUpdateMultiEnd { req_id }
            | SecurityDefinitionOptionParameterEnd { req_id }
            | SoftDollarTiers { req_id, .. }
            | SymbolSamples { req_id, .. }
            | TickReqParams { req_id, .. }
            | SmartComponents { req_id, .. }
            | NewsArticle { req_id, .. }
            | TickNews { req_id, .. }
            | HistoricalNews { req_id, .. }
            | HistoricalNewsEnd { req_id, .. }
            | HeadTimestamp { req_id, .. }
            | HistogramData { req_id, .. }
            | RerouteMktDataReq { req_id, .. }
            | RerouteMktDepthReq { req_id, .. }
            | Pnl { req_id, .. }
            | PnlSingle { req_id, .. }
            | HistoricalTicks { req_id, .. }
            | HistoricalTicksBidAsk { req_id, .. }
            | HistoricalTicksLast { req_id, .. }
            | TickByTickAllLast { req_id, .. }
            | TickByTickBidAsk { req_id, .. }
            | TickByTickMidPoint { req_id, .. }
            | ReplaceFaEnd { req_id, .. }
            | WshMetaData { req_id, .. }
            | WshEventData { req_id, .. }
            | HistoricalSchedule { req_id, .. }
            | UserInfo { req_id, .. }
            | ConfigResponse { req_id, .. }
            | UpdateConfigResponse { req_id, .. } => *req_id,
            SecurityDefinitionOptionParameter(p) => p.req_id,
            Error(e) => e.req_id,
            _ => return None,
        })
    }
}

/// Decode one frame payload received from a server of `server_version`.
pub fn decode(server_version: i32, payload: &[u8]) -> Result<Incoming, DecodeError> {
    decode_counting(server_version, payload).map(|(m, _)| m)
}

/// Like [`decode`], and also report whether every field was consumed. Extra
/// fields are not an error in production (newer servers may append fields),
/// but layout tests use this to catch a field count drift.
fn decode_counting(server_version: i32, payload: &[u8]) -> Result<(Incoming, bool), DecodeError> {
    // From server version 201 the id is a raw 4-byte integer; ids above 200
    // carry a protobuf payload.
    let (id, r) = if server_version >= sv::PROTOBUF {
        let Some((head, rest)) = payload.split_first_chunk::<4>() else {
            return Err(DecodeError::MissingField(0));
        };
        let id = i32::from_be_bytes(*head);
        if id > crate::message::PROTOBUF_MSG_ID {
            let m = crate::decode_pb::decode_pb(id - crate::message::PROTOBUF_MSG_ID, rest)?;
            return Ok((m, true));
        }
        (id, FieldReader::new(rest))
    } else {
        let mut r = FieldReader::new(payload);
        (r.int()?, r)
    };
    let mut d = Decoder {
        r,
        v: server_version,
    };
    let m = d.message(id, payload)?;
    Ok((m, d.r.is_empty()))
}

struct Decoder<'a> {
    r: FieldReader<'a>,
    v: i32,
}

/// Size tick type implied by a price tick, if any (bid -> bid size, ...).
pub fn implied_size_tick(tick_type: i32) -> Option<i32> {
    use crate::tick_type::*;
    match tick_type {
        BID => Some(BID_SIZE),
        ASK => Some(ASK_SIZE),
        LAST => Some(LAST_SIZE),
        DELAYED_BID => Some(DELAYED_BID_SIZE),
        DELAYED_ASK => Some(DELAYED_ASK_SIZE),
        DELAYED_LAST => Some(DELAYED_LAST_SIZE),
        _ => None,
    }
}

impl<'a> Decoder<'a> {
    fn message(&mut self, id: i32, payload: &[u8]) -> Result<Incoming, DecodeError> {
        let r = &mut self.r;
        let v = self.v;
        Ok(match id {
            msg::TICK_PRICE => {
                r.skip()?;
                let req_id = r.int()?;
                let tick_type = r.int()?;
                let price = r.f64()?;
                let size = r.decimal()?;
                let mask = r.int()?;
                let attrib = TickAttrib {
                    can_auto_execute: mask & 1 != 0,
                    past_limit: mask & 2 != 0,
                    pre_open: mask & 4 != 0,
                };
                Incoming::TickPrice {
                    req_id,
                    tick_type,
                    price,
                    size,
                    attrib,
                }
            }
            msg::TICK_SIZE => {
                r.skip()?;
                Incoming::TickSize {
                    req_id: r.int()?,
                    tick_type: r.int()?,
                    size: r.decimal()?,
                }
            }
            msg::ORDER_STATUS => Incoming::OrderStatus(OrderStatusMsg {
                order_id: r.int()?,
                status: r.string()?,
                filled: r.decimal()?,
                remaining: r.decimal()?,
                avg_fill_price: r.f64()?,
                perm_id: r.long()?,
                parent_id: r.int()?,
                last_fill_price: r.f64()?,
                client_id: r.int()?,
                why_held: r.string()?,
                mkt_cap_price: r.f64()?,
            }),
            msg::ERR_MSG => {
                if v < sv::ERROR_TIME {
                    r.skip()?;
                }
                let req_id = r.int()?;
                let code = r.int()?;
                let message = r.string_unescaped()?;
                let advanced_order_reject_json = if v >= sv::ADVANCED_ORDER_REJECT {
                    r.string_unescaped()?
                } else {
                    String::new()
                };
                let error_time = if v >= sv::ERROR_TIME { r.long()? } else { 0 };
                Incoming::Error(ErrorMsg {
                    req_id,
                    code,
                    message,
                    advanced_order_reject_json,
                    error_time,
                })
            }
            msg::OPEN_ORDER => self.open_order()?,
            msg::ACCT_VALUE => {
                r.skip()?;
                Incoming::UpdateAccountValue {
                    key: r.string()?,
                    value: r.string()?,
                    currency: r.string()?,
                    account: r.string()?,
                }
            }
            msg::PORTFOLIO_VALUE => {
                let version = r.int()?;
                let mut c = Contract {
                    con_id: r.int()?,
                    symbol: r.string()?,
                    sec_type: r.string()?,
                    ..Default::default()
                };
                c.last_trade_date_or_contract_month = r.string()?;
                c.strike = r.f64()?;
                c.right = r.string()?;
                if version >= 7 {
                    c.multiplier = r.string()?;
                    c.primary_exchange = r.string()?;
                }
                c.currency = r.string()?;
                c.local_symbol = r.string()?;
                if version >= 8 {
                    c.trading_class = r.string()?;
                }
                Incoming::UpdatePortfolio(Box::new(PortfolioMsg {
                    contract: c,
                    position: r.decimal()?,
                    market_price: r.f64()?,
                    market_value: r.f64()?,
                    average_cost: r.f64()?,
                    unrealized_pnl: r.f64()?,
                    realized_pnl: r.f64()?,
                    account: r.string()?,
                }))
            }
            msg::ACCT_UPDATE_TIME => {
                r.skip()?;
                Incoming::UpdateAccountTime {
                    timestamp: r.string()?,
                }
            }
            msg::NEXT_VALID_ID => {
                r.skip()?;
                Incoming::NextValidId { order_id: r.int()? }
            }
            msg::CONTRACT_DATA => self.contract_data()?,
            msg::BOND_CONTRACT_DATA => self.bond_contract_data()?,
            msg::EXECUTION_DATA => self.execution_data()?,
            msg::MARKET_DEPTH => {
                r.skip()?;
                Incoming::UpdateMktDepth {
                    req_id: r.int()?,
                    position: r.int()?,
                    operation: r.int()?,
                    side: r.int()?,
                    price: r.f64()?,
                    size: r.decimal()?,
                }
            }
            msg::MARKET_DEPTH_L2 => {
                r.skip()?;
                Incoming::UpdateMktDepthL2 {
                    req_id: r.int()?,
                    position: r.int()?,
                    market_maker: r.string()?,
                    operation: r.int()?,
                    side: r.int()?,
                    price: r.f64()?,
                    size: r.decimal()?,
                    is_smart_depth: r.bool()?,
                }
            }
            msg::NEWS_BULLETINS => {
                r.skip()?;
                Incoming::UpdateNewsBulletin {
                    msg_id: r.int()?,
                    msg_type: r.int()?,
                    message: r.string()?,
                    orig_exchange: r.string()?,
                }
            }
            msg::MANAGED_ACCTS => {
                r.skip()?;
                Incoming::ManagedAccounts {
                    accounts: r.string()?,
                }
            }
            msg::RECEIVE_FA => {
                r.skip()?;
                Incoming::ReceiveFa {
                    fa_data_type: r.int()?,
                    xml: r.string()?,
                }
            }
            msg::HISTORICAL_DATA => {
                let req_id = r.int()?;
                let end = if v < sv::HISTORICAL_DATA_END {
                    Some((r.string()?, r.string()?))
                } else {
                    None
                };
                let n = r.int()?.max(0) as usize;
                let mut bars = Vec::with_capacity(n);
                for _ in 0..n {
                    bars.push(BarData {
                        date: r.string()?,
                        open: r.f64()?,
                        high: r.f64()?,
                        low: r.f64()?,
                        close: r.f64()?,
                        volume: r.decimal()?,
                        average: r.decimal()?,
                        bar_count: r.int()?,
                    });
                }
                Incoming::HistoricalData { req_id, bars, end }
            }
            msg::HISTORICAL_DATA_END => Incoming::HistoricalDataEnd {
                req_id: r.int()?,
                start: r.string()?,
                end: r.string()?,
            },
            msg::HISTORICAL_DATA_UPDATE => {
                let req_id = r.int()?;
                let bar_count = r.int()?;
                let date = r.string()?;
                let open = r.f64()?;
                let close = r.f64()?;
                let high = r.f64()?;
                let low = r.f64()?;
                let average = r.decimal()?;
                let volume = r.decimal()?;
                Incoming::HistoricalDataUpdate {
                    req_id,
                    bar: BarData {
                        date,
                        open,
                        high,
                        low,
                        close,
                        volume,
                        average,
                        bar_count,
                    },
                }
            }
            msg::SCANNER_PARAMETERS => {
                r.skip()?;
                Incoming::ScannerParameters { xml: r.string()? }
            }
            msg::SCANNER_DATA => {
                r.skip()?;
                let req_id = r.int()?;
                let n = r.int()?.max(0) as usize;
                let mut rows = Vec::with_capacity(n);
                for _ in 0..n {
                    let rank = r.int()?;
                    let mut cd = ContractDetails::default();
                    let c = &mut cd.contract;
                    c.con_id = r.int()?;
                    c.symbol = r.string()?;
                    c.sec_type = r.string()?;
                    c.last_trade_date_or_contract_month = r.string()?;
                    c.strike = r.f64()?;
                    c.right = r.string()?;
                    c.exchange = r.string()?;
                    c.currency = r.string()?;
                    c.local_symbol = r.string()?;
                    cd.market_name = r.string()?;
                    cd.contract.trading_class = r.string()?;
                    rows.push(ScanData {
                        rank,
                        contract_details: cd,
                        distance: r.string()?,
                        benchmark: r.string()?,
                        projection: r.string()?,
                        legs_str: r.string()?,
                    });
                }
                Incoming::ScannerData { req_id, rows }
            }
            msg::TICK_OPTION_COMPUTATION => {
                let req_id = r.int()?;
                let tick_type = r.int()?;
                let tick_attrib = Some(r.int()?);
                let not = |x: f64, sentinel: f64| if x == sentinel { None } else { Some(x) };
                let iv = r.f64()?;
                let delta = r.f64()?;
                let computation = OptionComputation {
                    tick_attrib,
                    implied_vol: if iv < 0.0 { None } else { Some(iv) },
                    delta: not(delta, -2.0),
                    opt_price: not(r.f64()?, -1.0),
                    pv_dividend: not(r.f64()?, -1.0),
                    gamma: not(r.f64()?, -2.0),
                    vega: not(r.f64()?, -2.0),
                    theta: not(r.f64()?, -2.0),
                    und_price: not(r.f64()?, -1.0),
                };
                Incoming::TickOptionComputation {
                    req_id,
                    tick_type,
                    computation,
                }
            }
            msg::TICK_GENERIC => {
                r.skip()?;
                Incoming::TickGeneric {
                    req_id: r.int()?,
                    tick_type: r.int()?,
                    value: r.f64()?,
                }
            }
            msg::TICK_STRING => {
                r.skip()?;
                Incoming::TickString {
                    req_id: r.int()?,
                    tick_type: r.int()?,
                    value: r.string()?,
                }
            }
            msg::TICK_EFP => {
                r.skip()?;
                let req_id = r.int()?;
                let tick_type = r.int()?;
                let efp = EfpData {
                    basis_points: r.f64()?,
                    formatted_basis_points: r.string()?,
                    implied_future: r.f64()?,
                    hold_days: r.int()?,
                    future_last_trade_date: r.string()?,
                    dividend_impact: r.f64()?,
                    dividends_to_last_trade_date: r.f64()?,
                };
                Incoming::TickEfp {
                    req_id,
                    tick_type,
                    efp,
                }
            }
            msg::CURRENT_TIME => {
                r.skip()?;
                Incoming::CurrentTime { time: r.long()? }
            }
            msg::CURRENT_TIME_IN_MILLIS => Incoming::CurrentTimeInMillis { time: r.long()? },
            msg::REAL_TIME_BARS => {
                r.skip()?;
                let req_id = r.int()?;
                let bar = RealTimeBar {
                    time: r.long()?,
                    open: r.f64()?,
                    high: r.f64()?,
                    low: r.f64()?,
                    close: r.f64()?,
                    volume: r.decimal()?,
                    wap: r.decimal()?,
                    count: r.int()?,
                };
                Incoming::RealtimeBar { req_id, bar }
            }
            msg::FUNDAMENTAL_DATA => {
                r.skip()?;
                Incoming::FundamentalData {
                    req_id: r.int()?,
                    data: r.string()?,
                }
            }
            msg::CONTRACT_DATA_END => {
                r.skip()?;
                Incoming::ContractDetailsEnd { req_id: r.int()? }
            }
            msg::OPEN_ORDER_END => Incoming::OpenOrderEnd,
            msg::ACCT_DOWNLOAD_END => {
                r.skip()?;
                Incoming::AccountDownloadEnd {
                    account: r.string()?,
                }
            }
            msg::EXECUTION_DATA_END => {
                r.skip()?;
                Incoming::ExecDetailsEnd { req_id: r.int()? }
            }
            msg::DELTA_NEUTRAL_VALIDATION => {
                r.skip()?;
                let req_id = r.int()?;
                let contract = DeltaNeutralContract {
                    con_id: r.int()?,
                    delta: r.f64()?,
                    price: r.f64()?,
                };
                Incoming::DeltaNeutralValidation { req_id, contract }
            }
            msg::TICK_SNAPSHOT_END => {
                r.skip()?;
                Incoming::TickSnapshotEnd { req_id: r.int()? }
            }
            msg::MARKET_DATA_TYPE => {
                r.skip()?;
                Incoming::MarketDataType {
                    req_id: r.int()?,
                    market_data_type: r.int()?,
                }
            }
            msg::COMMISSION_REPORT => {
                r.skip()?;
                Incoming::CommissionReport(CommissionReport {
                    exec_id: r.string()?,
                    commission: r.f64()?,
                    currency: r.string()?,
                    realized_pnl: r.f64()?,
                    yield_: r.f64()?,
                    yield_redemption_date: r.int()?,
                })
            }
            msg::POSITION_DATA => {
                let version = r.int()?;
                let account = r.string()?;
                let mut c = Contract {
                    con_id: r.int()?,
                    symbol: r.string()?,
                    sec_type: r.string()?,
                    ..Default::default()
                };
                c.last_trade_date_or_contract_month = r.string()?;
                c.strike = r.f64()?;
                c.right = r.string()?;
                c.multiplier = r.string()?;
                c.exchange = r.string()?;
                c.currency = r.string()?;
                c.local_symbol = r.string()?;
                if version >= 2 {
                    c.trading_class = r.string()?;
                }
                let position = r.decimal()?;
                let avg_cost = if version >= 3 { r.f64()? } else { 0.0 };
                Incoming::Position {
                    account,
                    contract: Box::new(c),
                    position,
                    avg_cost,
                }
            }
            msg::POSITION_END => Incoming::PositionEnd,
            msg::ACCOUNT_SUMMARY => {
                r.skip()?;
                Incoming::AccountSummary {
                    req_id: r.int()?,
                    account: r.string()?,
                    tag: r.string()?,
                    value: r.string()?,
                    currency: r.string()?,
                }
            }
            msg::ACCOUNT_SUMMARY_END => {
                r.skip()?;
                Incoming::AccountSummaryEnd { req_id: r.int()? }
            }
            msg::VERIFY_MESSAGE_API => {
                r.skip()?;
                Incoming::VerifyMessageApi {
                    api_data: r.string()?,
                }
            }
            msg::VERIFY_COMPLETED => {
                r.skip()?;
                Incoming::VerifyCompleted {
                    is_successful: r.bool()?,
                    error_text: r.string()?,
                }
            }
            msg::DISPLAY_GROUP_LIST => {
                r.skip()?;
                Incoming::DisplayGroupList {
                    req_id: r.int()?,
                    groups: r.string()?,
                }
            }
            msg::DISPLAY_GROUP_UPDATED => {
                r.skip()?;
                Incoming::DisplayGroupUpdated {
                    req_id: r.int()?,
                    contract_info: r.string()?,
                }
            }
            msg::VERIFY_AND_AUTH_MESSAGE_API => {
                r.skip()?;
                Incoming::VerifyAndAuthMessageApi {
                    api_data: r.string()?,
                    xyz_challenge: r.string()?,
                }
            }
            msg::VERIFY_AND_AUTH_COMPLETED => {
                r.skip()?;
                Incoming::VerifyAndAuthCompleted {
                    is_successful: r.bool()?,
                    error_text: r.string()?,
                }
            }
            msg::POSITION_MULTI => {
                r.skip()?;
                let req_id = r.int()?;
                let account = r.string()?;
                let mut c = Contract {
                    con_id: r.int()?,
                    symbol: r.string()?,
                    sec_type: r.string()?,
                    ..Default::default()
                };
                c.last_trade_date_or_contract_month = r.string()?;
                c.strike = r.f64()?;
                c.right = r.string()?;
                c.multiplier = r.string()?;
                c.exchange = r.string()?;
                c.currency = r.string()?;
                c.local_symbol = r.string()?;
                c.trading_class = r.string()?;
                let position = r.decimal()?;
                let avg_cost = r.f64()?;
                let model_code = r.string()?;
                Incoming::PositionMulti {
                    req_id,
                    account,
                    model_code,
                    contract: Box::new(c),
                    position,
                    avg_cost,
                }
            }
            msg::POSITION_MULTI_END => {
                r.skip()?;
                Incoming::PositionMultiEnd { req_id: r.int()? }
            }
            msg::ACCOUNT_UPDATE_MULTI => {
                r.skip()?;
                Incoming::AccountUpdateMulti {
                    req_id: r.int()?,
                    account: r.string()?,
                    model_code: r.string()?,
                    key: r.string()?,
                    value: r.string()?,
                    currency: r.string()?,
                }
            }
            msg::ACCOUNT_UPDATE_MULTI_END => {
                r.skip()?;
                Incoming::AccountUpdateMultiEnd { req_id: r.int()? }
            }
            msg::SECURITY_DEFINITION_OPTION_PARAMETER => {
                let req_id = r.int()?;
                let exchange = r.string()?;
                let underlying_con_id = r.int()?;
                let trading_class = r.string()?;
                let multiplier = r.string()?;
                let ne = r.int()?.max(0) as usize;
                let mut expirations = Vec::with_capacity(ne);
                for _ in 0..ne {
                    expirations.push(r.string()?);
                }
                let ns = r.int()?.max(0) as usize;
                let mut strikes = Vec::with_capacity(ns);
                for _ in 0..ns {
                    strikes.push(r.f64()?);
                }
                Incoming::SecurityDefinitionOptionParameter(Box::new(SecDefOptParams {
                    req_id,
                    exchange,
                    underlying_con_id,
                    trading_class,
                    multiplier,
                    expirations,
                    strikes,
                }))
            }
            msg::SECURITY_DEFINITION_OPTION_PARAMETER_END => {
                Incoming::SecurityDefinitionOptionParameterEnd { req_id: r.int()? }
            }
            msg::SOFT_DOLLAR_TIERS => {
                let req_id = r.int()?;
                let n = r.int()?.max(0) as usize;
                let mut tiers = Vec::with_capacity(n);
                for _ in 0..n {
                    tiers.push(SoftDollarTier {
                        name: r.string()?,
                        value: r.string()?,
                        display_name: r.string()?,
                    });
                }
                Incoming::SoftDollarTiers { req_id, tiers }
            }
            msg::FAMILY_CODES => {
                let n = r.int()?.max(0) as usize;
                let mut codes = Vec::with_capacity(n);
                for _ in 0..n {
                    codes.push(FamilyCode {
                        account_id: r.string()?,
                        family_code: r.string()?,
                    });
                }
                Incoming::FamilyCodes { codes }
            }
            msg::SYMBOL_SAMPLES => {
                let req_id = r.int()?;
                let n = r.int()?.max(0) as usize;
                let mut descriptions = Vec::with_capacity(n);
                for _ in 0..n {
                    let mut d = ContractDescription::default();
                    d.contract.con_id = r.int()?;
                    d.contract.symbol = r.string()?;
                    d.contract.sec_type = r.string()?;
                    d.contract.primary_exchange = r.string()?;
                    d.contract.currency = r.string()?;
                    let m = r.int()?.max(0) as usize;
                    for _ in 0..m {
                        d.derivative_sec_types.push(r.string()?);
                    }
                    if v >= sv::BOND_ISSUERID {
                        d.contract.description = r.string()?;
                        d.contract.issuer_id = r.string()?;
                    }
                    descriptions.push(d);
                }
                Incoming::SymbolSamples {
                    req_id,
                    descriptions,
                }
            }
            msg::MKT_DEPTH_EXCHANGES => {
                let n = r.int()?.max(0) as usize;
                let mut descriptions = Vec::with_capacity(n);
                for _ in 0..n {
                    descriptions.push(DepthMktDataDescription {
                        exchange: r.string()?,
                        sec_type: r.string()?,
                        listing_exch: r.string()?,
                        service_data_type: r.string()?,
                        agg_group: r.int_max()?,
                    });
                }
                Incoming::MktDepthExchanges { descriptions }
            }
            msg::TICK_REQ_PARAMS => Incoming::TickReqParams {
                req_id: r.int()?,
                min_tick: r.f64()?,
                bbo_exchange: r.string()?,
                snapshot_permissions: r.int()?,
            },
            msg::SMART_COMPONENTS => {
                let req_id = r.int()?;
                let n = r.int()?.max(0) as usize;
                let mut components = Vec::with_capacity(n);
                for _ in 0..n {
                    components.push(SmartComponent {
                        bit_number: r.int()?,
                        exchange: r.string()?,
                        exchange_letter: r.string()?,
                    });
                }
                Incoming::SmartComponents { req_id, components }
            }
            msg::NEWS_ARTICLE => Incoming::NewsArticle {
                req_id: r.int()?,
                article_type: r.int()?,
                article_text: r.string()?,
            },
            msg::TICK_NEWS => Incoming::TickNews {
                req_id: r.int()?,
                time_stamp: r.long()?,
                provider_code: r.string()?,
                article_id: r.string()?,
                headline: r.string()?,
                extra_data: r.string()?,
            },
            msg::NEWS_PROVIDERS => {
                let n = r.int()?.max(0) as usize;
                let mut providers = Vec::with_capacity(n);
                for _ in 0..n {
                    providers.push(NewsProvider {
                        code: r.string()?,
                        name: r.string()?,
                    });
                }
                Incoming::NewsProviders { providers }
            }
            msg::HISTORICAL_NEWS => Incoming::HistoricalNews {
                req_id: r.int()?,
                time: r.string()?,
                provider_code: r.string()?,
                article_id: r.string()?,
                headline: r.string()?,
            },
            msg::HISTORICAL_NEWS_END => Incoming::HistoricalNewsEnd {
                req_id: r.int()?,
                has_more: r.bool()?,
            },
            msg::HEAD_TIMESTAMP => Incoming::HeadTimestamp {
                req_id: r.int()?,
                head_timestamp: r.string()?,
            },
            msg::HISTOGRAM_DATA => {
                let req_id = r.int()?;
                let n = r.int()?.max(0) as usize;
                let mut items = Vec::with_capacity(n);
                for _ in 0..n {
                    items.push(HistogramData {
                        price: r.f64()?,
                        count: r.decimal()?,
                    });
                }
                Incoming::HistogramData { req_id, items }
            }
            msg::REROUTE_MKT_DATA_REQ => Incoming::RerouteMktDataReq {
                req_id: r.int()?,
                con_id: r.int()?,
                exchange: r.string()?,
            },
            msg::REROUTE_MKT_DEPTH_REQ => Incoming::RerouteMktDepthReq {
                req_id: r.int()?,
                con_id: r.int()?,
                exchange: r.string()?,
            },
            msg::MARKET_RULE => {
                let market_rule_id = r.int()?;
                let n = r.int()?.max(0) as usize;
                let mut increments = Vec::with_capacity(n);
                for _ in 0..n {
                    increments.push(PriceIncrement {
                        low_edge: r.f64()?,
                        increment: r.f64()?,
                    });
                }
                Incoming::MarketRule {
                    market_rule_id,
                    increments,
                }
            }
            msg::PNL => Incoming::Pnl {
                req_id: r.int()?,
                daily_pnl: r.f64()?,
                unrealized_pnl: r.f64()?,
                realized_pnl: r.f64()?,
            },
            msg::PNL_SINGLE => Incoming::PnlSingle {
                req_id: r.int()?,
                position: r.decimal()?,
                daily_pnl: r.f64()?,
                unrealized_pnl: r.f64()?,
                realized_pnl: r.f64()?,
                value: r.f64()?,
            },
            msg::HISTORICAL_TICKS => {
                let req_id = r.int()?;
                let n = r.int()?.max(0) as usize;
                let mut ticks = Vec::with_capacity(n);
                for _ in 0..n {
                    let time = r.long()?;
                    r.skip()?;
                    ticks.push(HistoricalTick {
                        time,
                        price: r.f64()?,
                        size: r.decimal()?,
                    });
                }
                Incoming::HistoricalTicks {
                    req_id,
                    ticks,
                    done: r.bool()?,
                }
            }
            msg::HISTORICAL_TICKS_BID_ASK => {
                let req_id = r.int()?;
                let n = r.int()?.max(0) as usize;
                let mut ticks = Vec::with_capacity(n);
                for _ in 0..n {
                    let time = r.long()?;
                    let mask = r.int()?;
                    ticks.push(HistoricalTickBidAsk {
                        time,
                        tick_attrib_bid_ask: TickAttribBidAsk {
                            ask_past_high: mask & 1 != 0,
                            bid_past_low: mask & 2 != 0,
                        },
                        price_bid: r.f64()?,
                        price_ask: r.f64()?,
                        size_bid: r.decimal()?,
                        size_ask: r.decimal()?,
                    });
                }
                Incoming::HistoricalTicksBidAsk {
                    req_id,
                    ticks,
                    done: r.bool()?,
                }
            }
            msg::HISTORICAL_TICKS_LAST => {
                let req_id = r.int()?;
                let n = r.int()?.max(0) as usize;
                let mut ticks = Vec::with_capacity(n);
                for _ in 0..n {
                    let time = r.long()?;
                    let mask = r.int()?;
                    ticks.push(HistoricalTickLast {
                        time,
                        tick_attrib_last: TickAttribLast {
                            past_limit: mask & 1 != 0,
                            unreported: mask & 2 != 0,
                        },
                        price: r.f64()?,
                        size: r.decimal()?,
                        exchange: r.string()?,
                        special_conditions: r.string()?,
                    });
                }
                Incoming::HistoricalTicksLast {
                    req_id,
                    ticks,
                    done: r.bool()?,
                }
            }
            msg::TICK_BY_TICK => {
                let req_id = r.int()?;
                let tick_type = r.int()?;
                let time = r.long()?;
                match tick_type {
                    1 | 2 => {
                        let price = r.f64()?;
                        let size = r.decimal()?;
                        let mask = r.int()?;
                        Incoming::TickByTickAllLast {
                            req_id,
                            tick_type,
                            time,
                            price,
                            size,
                            attrib: TickAttribLast {
                                past_limit: mask & 1 != 0,
                                unreported: mask & 2 != 0,
                            },
                            exchange: r.string()?,
                            special_conditions: r.string()?,
                        }
                    }
                    3 => {
                        let bid_price = r.f64()?;
                        let ask_price = r.f64()?;
                        let bid_size = r.decimal()?;
                        let ask_size = r.decimal()?;
                        let mask = r.int()?;
                        Incoming::TickByTickBidAsk {
                            req_id,
                            time,
                            bid_price,
                            ask_price,
                            bid_size,
                            ask_size,
                            attrib: TickAttribBidAsk {
                                bid_past_low: mask & 1 != 0,
                                ask_past_high: mask & 2 != 0,
                            },
                        }
                    }
                    4 => Incoming::TickByTickMidPoint {
                        req_id,
                        time,
                        mid_point: r.f64()?,
                    },
                    _ => return Ok(unknown(id, payload)),
                }
            }
            msg::ORDER_BOUND => Incoming::OrderBound {
                perm_id: r.long()?,
                client_id: r.int()?,
                order_id: r.int()?,
            },
            msg::COMPLETED_ORDER => self.completed_order()?,
            msg::COMPLETED_ORDERS_END => Incoming::CompletedOrdersEnd,
            msg::REPLACE_FA_END => Incoming::ReplaceFaEnd {
                req_id: r.int()?,
                text: r.string()?,
            },
            msg::WSH_META_DATA => Incoming::WshMetaData {
                req_id: r.int()?,
                data_json: r.string()?,
            },
            msg::WSH_EVENT_DATA => Incoming::WshEventData {
                req_id: r.int()?,
                data_json: r.string()?,
            },
            msg::HISTORICAL_SCHEDULE => {
                let req_id = r.int()?;
                let start_date_time = r.string()?;
                let end_date_time = r.string()?;
                let time_zone = r.string()?;
                let n = r.int()?.max(0) as usize;
                let mut sessions = Vec::with_capacity(n);
                for _ in 0..n {
                    sessions.push(HistoricalSession {
                        start_date_time: r.string()?,
                        end_date_time: r.string()?,
                        ref_date: r.string()?,
                    });
                }
                Incoming::HistoricalSchedule {
                    req_id,
                    start_date_time,
                    end_date_time,
                    time_zone,
                    sessions,
                }
            }
            msg::USER_INFO => Incoming::UserInfo {
                req_id: r.int()?,
                white_branding_id: r.string()?,
            },
            _ => unknown(id, payload),
        })
    }

    fn last_trade_date(
        &mut self,
        d: &mut ContractDetails,
        is_bond: bool,
    ) -> Result<(), DecodeError> {
        let s = self.r.string()?;
        let parts: Vec<&str> = if s.contains('-') {
            s.split('-').collect()
        } else {
            s.split_whitespace().collect()
        };
        if let Some(p) = parts.first() {
            if is_bond {
                d.maturity = p.to_string();
            } else {
                d.contract.last_trade_date_or_contract_month = p.to_string();
            }
        }
        if let Some(p) = parts.get(1) {
            d.last_trade_time = p.to_string();
        }
        if is_bond && let Some(p) = parts.get(2) {
            d.time_zone_id = p.to_string();
        }
        Ok(())
    }

    fn sec_id_list(&mut self) -> Result<Vec<TagValue>, DecodeError> {
        let n = self.r.int()?.max(0) as usize;
        let mut list = Vec::with_capacity(n);
        for _ in 0..n {
            list.push(TagValue {
                tag: self.r.string()?,
                value: self.r.string()?,
            });
        }
        Ok(list)
    }

    fn contract_data(&mut self) -> Result<Incoming, DecodeError> {
        let v = self.v;
        if v < sv::SIZE_RULES {
            self.r.skip()?;
        }
        let req_id = self.r.int()?;
        let mut d = ContractDetails::default();
        d.contract.symbol = self.r.string()?;
        d.contract.sec_type = self.r.string()?;
        self.last_trade_date(&mut d, false)?;
        let r = &mut self.r;
        if v >= sv::LAST_TRADE_DATE {
            d.contract.last_trade_date = r.string()?;
        }
        d.contract.strike = r.f64()?;
        d.contract.right = r.string()?;
        d.contract.exchange = r.string()?;
        d.contract.currency = r.string()?;
        d.contract.local_symbol = r.string()?;
        d.market_name = r.string()?;
        d.contract.trading_class = r.string()?;
        d.contract.con_id = r.int()?;
        d.min_tick = r.f64()?;
        if v < sv::SIZE_RULES {
            r.skip()?; // mdSizeMultiplier
        }
        d.contract.multiplier = r.string()?;
        d.order_types = r.string()?;
        d.valid_exchanges = r.string()?;
        d.price_magnifier = r.int()?;
        d.under_con_id = r.int()?;
        d.long_name = r.string_unescaped()?;
        d.contract.primary_exchange = r.string()?;
        d.contract_month = r.string()?;
        d.industry = r.string()?;
        d.category = r.string()?;
        d.subcategory = r.string()?;
        d.time_zone_id = r.string()?;
        d.trading_hours = r.string()?;
        d.liquid_hours = r.string()?;
        d.ev_rule = r.string()?;
        d.ev_multiplier = r.int()?;
        d.sec_id_list = self.sec_id_list()?;
        let r = &mut self.r;
        d.agg_group = r.int()?;
        d.under_symbol = r.string()?;
        d.under_sec_type = r.string()?;
        d.market_rule_ids = r.string()?;
        d.real_expiration_date = r.string()?;
        d.stock_type = r.string()?;
        if (sv::FRACTIONAL_SIZE_SUPPORT..sv::SIZE_RULES).contains(&v) {
            r.skip()?; // sizeMinTick
        }
        if v >= sv::SIZE_RULES {
            d.min_size = r.decimal()?;
            d.size_increment = r.decimal()?;
            d.suggested_size_increment = r.decimal()?;
        }
        if v >= sv::FUND_DATA_FIELDS && d.contract.sec_type == "FUND" {
            d.fund_name = r.string()?;
            d.fund_family = r.string()?;
            d.fund_type = r.string()?;
            d.fund_front_load = r.string()?;
            d.fund_back_load = r.string()?;
            d.fund_back_load_time_interval = r.string()?;
            d.fund_management_fee = r.string()?;
            d.fund_closed = r.bool()?;
            d.fund_closed_for_new_investors = r.bool()?;
            d.fund_closed_for_new_money = r.bool()?;
            d.fund_notify_amount = r.string()?;
            d.fund_minimum_initial_purchase = r.string()?;
            d.fund_subsequent_minimum_purchase = r.string()?;
            d.fund_blue_sky_states = r.string()?;
            d.fund_blue_sky_territories = r.string()?;
            d.fund_distribution_policy_indicator = r.string()?;
            d.fund_asset_type = r.string()?;
        }
        if v >= sv::INELIGIBILITY_REASONS {
            let n = r.int()?.max(0) as usize;
            for _ in 0..n {
                d.ineligibility_reason_list.push(IneligibilityReason {
                    id: r.string()?,
                    description: r.string()?,
                });
            }
        }
        Ok(Incoming::ContractDetails {
            req_id,
            details: Box::new(d),
        })
    }

    fn bond_contract_data(&mut self) -> Result<Incoming, DecodeError> {
        let v = self.v;
        if v < sv::SIZE_RULES {
            self.r.skip()?;
        }
        let req_id = self.r.int()?;
        let mut d = ContractDetails::default();
        d.contract.symbol = self.r.string()?;
        d.contract.sec_type = self.r.string()?;
        d.cusip = self.r.string()?;
        d.coupon = self.r.f64()?;
        self.last_trade_date(&mut d, true)?;
        let r = &mut self.r;
        d.issue_date = r.string()?;
        d.ratings = r.string()?;
        d.bond_type = r.string()?;
        d.coupon_type = r.string()?;
        d.convertible = r.bool()?;
        d.callable = r.bool()?;
        d.putable = r.bool()?;
        d.desc_append = r.string()?;
        d.contract.exchange = r.string()?;
        d.contract.currency = r.string()?;
        d.market_name = r.string()?;
        d.contract.trading_class = r.string()?;
        d.contract.con_id = r.int()?;
        d.min_tick = r.f64()?;
        if v < sv::SIZE_RULES {
            r.skip()?;
        }
        d.order_types = r.string()?;
        d.valid_exchanges = r.string()?;
        d.next_option_date = r.string()?;
        d.next_option_type = r.string()?;
        d.next_option_partial = r.bool()?;
        d.notes = r.string()?;
        d.long_name = r.string()?;
        if v >= sv::BOND_TRADING_HOURS {
            d.time_zone_id = r.string()?;
            d.trading_hours = r.string()?;
            d.liquid_hours = r.string()?;
        }
        d.ev_rule = r.string()?;
        d.ev_multiplier = r.int()?;
        d.sec_id_list = self.sec_id_list()?;
        let r = &mut self.r;
        d.agg_group = r.int()?;
        d.market_rule_ids = r.string()?;
        if v >= sv::SIZE_RULES {
            d.min_size = r.decimal()?;
            d.size_increment = r.decimal()?;
            d.suggested_size_increment = r.decimal()?;
        }
        Ok(Incoming::BondContractDetails {
            req_id,
            details: Box::new(d),
        })
    }

    fn execution_data(&mut self) -> Result<Incoming, DecodeError> {
        let v = self.v;
        let r = &mut self.r;
        let req_id = r.int()?;
        let order_id = r.int()?;
        let mut c = Contract {
            con_id: r.int()?,
            symbol: r.string()?,
            sec_type: r.string()?,
            ..Default::default()
        };
        c.last_trade_date_or_contract_month = r.string()?;
        c.strike = r.f64()?;
        c.right = r.string()?;
        c.multiplier = r.string()?;
        c.exchange = r.string()?;
        c.currency = r.string()?;
        c.local_symbol = r.string()?;
        c.trading_class = r.string()?;
        let mut e = Execution {
            order_id,
            exec_id: r.string()?,
            time: r.string()?,
            acct_number: r.string()?,
            exchange: r.string()?,
            side: r.string()?,
            shares: r.decimal()?,
            price: r.f64()?,
            perm_id: r.long()?,
            client_id: r.int()?,
            liquidation: r.int()?,
            cum_qty: r.decimal()?,
            avg_price: r.f64()?,
            order_ref: r.string()?,
            ev_rule: r.string()?,
            ev_multiplier: r.f64()?,
            model_code: r.string()?,
            last_liquidity: r.int()?,
            ..Default::default()
        };
        if v >= sv::PENDING_PRICE_REVISION {
            e.pending_price_revision = r.bool()?;
        }
        if v >= sv::SUBMITTER {
            e.submitter = r.string()?;
        }
        Ok(Incoming::ExecDetails {
            req_id,
            contract: Box::new(c),
            execution: Box::new(e),
        })
    }

    /// conId .. tradingClass without primaryExchange (OrderDecoder.decodeContractFields).
    fn order_contract(&mut self, c: &mut Contract) -> Result<(), DecodeError> {
        let r = &mut self.r;
        c.con_id = r.int()?;
        c.symbol = r.string()?;
        c.sec_type = r.string()?;
        c.last_trade_date_or_contract_month = r.string()?;
        c.strike = r.f64()?;
        c.right = r.string()?;
        c.multiplier = r.string()?;
        c.exchange = r.string()?;
        c.currency = r.string()?;
        c.local_symbol = r.string()?;
        c.trading_class = r.string()?;
        Ok(())
    }

    fn order_head(&mut self, o: &mut Order, open: bool) -> Result<(), DecodeError> {
        let r = &mut self.r;
        o.action = r.string()?;
        o.total_quantity = r.decimal()?;
        o.order_type = r.string()?;
        o.lmt_price = r.f64_max()?;
        o.aux_price = r.f64_max()?;
        o.tif = r.string()?;
        o.oca_group = r.string()?;
        o.account = r.string()?;
        o.open_close = r.string()?;
        o.origin = r.int()?;
        o.order_ref = r.string()?;
        if open {
            o.client_id = r.int()?;
        }
        o.perm_id = r.long()?;
        o.outside_rth = r.bool()?;
        o.hidden = r.bool()?;
        o.discretionary_amt = r.f64()?;
        o.good_after_time = r.string()?;
        if open {
            r.skip()?; // sharesAllocation
        }
        o.fa_group = r.string()?;
        o.fa_method = r.string()?;
        o.fa_percentage = r.string()?;
        if self.v < sv::FA_PROFILE_DESUPPORT {
            o.fa_profile = r.string()?;
        }
        o.model_code = r.string()?;
        o.good_till_date = r.string()?;
        o.rule80_a = r.string()?;
        o.percent_offset = r.f64_max()?;
        o.settling_firm = r.string()?;
        o.short_sale_slot = r.int()?;
        o.designated_location = r.string()?;
        o.exempt_code = r.int()?;
        Ok(())
    }

    fn vol_params(&mut self, o: &mut Order, open: bool) -> Result<(), DecodeError> {
        let r = &mut self.r;
        o.volatility = r.f64_max()?;
        o.volatility_type = r.int()?;
        o.delta_neutral_order_type = r.string()?;
        o.delta_neutral_aux_price = r.f64_max()?;
        if !o.delta_neutral_order_type.is_empty() {
            o.delta_neutral_con_id = r.int()?;
            if open {
                o.delta_neutral_settling_firm = r.string()?;
                o.delta_neutral_clearing_account = r.string()?;
                o.delta_neutral_clearing_intent = r.string()?;
                o.delta_neutral_open_close = r.string()?;
            }
            o.delta_neutral_short_sale = r.bool()?;
            o.delta_neutral_short_sale_slot = r.int()?;
            o.delta_neutral_designated_location = r.string()?;
        }
        o.continuous_update = r.bool()?;
        o.reference_price_type = r.int()?;
        Ok(())
    }

    fn combo_and_scale(&mut self, c: &mut Contract, o: &mut Order) -> Result<(), DecodeError> {
        let r = &mut self.r;
        c.combo_legs_descrip = r.string()?;
        let n = r.int()?.max(0) as usize;
        for _ in 0..n {
            c.combo_legs.push(ComboLeg {
                con_id: r.int()?,
                ratio: r.int()?,
                action: r.string()?,
                exchange: r.string()?,
                open_close: r.int()?,
                short_sale_slot: r.int()?,
                designated_location: r.string()?,
                exempt_code: r.int()?,
            });
        }
        let n = r.int()?.max(0) as usize;
        for _ in 0..n {
            o.order_combo_legs.push(OrderComboLeg {
                price: r.f64_max()?,
            });
        }
        let n = r.int()?.max(0) as usize;
        for _ in 0..n {
            o.smart_combo_routing_params.push(TagValue {
                tag: r.string()?,
                value: r.string()?,
            });
        }
        o.scale_init_level_size = r.int_max()?;
        o.scale_subs_level_size = r.int_max()?;
        o.scale_price_increment = r.f64_max()?;
        if o.scale_price_increment != UNSET_DOUBLE && o.scale_price_increment > 0.0 {
            o.scale_price_adjust_value = r.f64_max()?;
            o.scale_price_adjust_interval = r.int_max()?;
            o.scale_profit_offset = r.f64_max()?;
            o.scale_auto_reset = r.bool()?;
            o.scale_init_position = r.int_max()?;
            o.scale_init_fill_qty = r.int_max()?;
            o.scale_random_percent = r.bool()?;
        }
        o.hedge_type = r.string()?;
        if !o.hedge_type.is_empty() {
            o.hedge_param = r.string()?;
        }
        Ok(())
    }

    fn delta_neutral_and_algo(
        &mut self,
        c: &mut Contract,
        o: &mut Order,
    ) -> Result<(), DecodeError> {
        let r = &mut self.r;
        if r.bool()? {
            c.delta_neutral_contract = Some(DeltaNeutralContract {
                con_id: r.int()?,
                delta: r.f64()?,
                price: r.f64()?,
            });
        }
        o.algo_strategy = r.string()?;
        if !o.algo_strategy.is_empty() {
            let n = r.int()?.max(0) as usize;
            for _ in 0..n {
                o.algo_params.push(TagValue {
                    tag: r.string()?,
                    value: r.string()?,
                });
            }
        }
        o.solicited = r.bool()?;
        Ok(())
    }

    fn peg_bench_and_conditions(&mut self, o: &mut Order) -> Result<(), DecodeError> {
        let r = &mut self.r;
        if is_peg_bench(&o.order_type) {
            o.reference_contract_id = r.int()?;
            o.is_pegged_change_amount_decrease = r.bool()?;
            o.pegged_change_amount = r.f64()?;
            o.reference_change_amount = r.f64()?;
            o.reference_exchange_id = r.string()?;
        }
        let n = r.int()?.max(0) as usize;
        if n > 0 {
            for _ in 0..n {
                let t = r.int()?;
                let conjunction = r.string()?;
                let cond = match t {
                    1 => OrderCondition::Price {
                        conjunction,
                        is_more: r.bool()?,
                        price: r.f64()?,
                        con_id: r.int()?,
                        exch: r.string()?,
                        trigger_method: r.int()?,
                    },
                    3 => OrderCondition::Time {
                        conjunction,
                        is_more: r.bool()?,
                        time: r.string()?,
                    },
                    4 => OrderCondition::Margin {
                        conjunction,
                        is_more: r.bool()?,
                        percent: r.int()?,
                    },
                    5 => OrderCondition::Execution {
                        conjunction,
                        sec_type: r.string()?,
                        exch: r.string()?,
                        symbol: r.string()?,
                    },
                    6 => OrderCondition::Volume {
                        conjunction,
                        is_more: r.bool()?,
                        volume: r.int()?,
                        con_id: r.int()?,
                        exch: r.string()?,
                    },
                    7 => OrderCondition::PercentChange {
                        conjunction,
                        is_more: r.bool()?,
                        change_percent: r.f64()?,
                        con_id: r.int()?,
                        exch: r.string()?,
                    },
                    other => {
                        return Err(DecodeError::BadNumber {
                            index: r.index(),
                            value: format!("order condition type {other}"),
                        });
                    }
                };
                o.conditions.push(cond);
            }
            o.conditions_ignore_rth = r.bool()?;
            o.conditions_cancel_order = r.bool()?;
        }
        Ok(())
    }

    fn peg_best_mid(&mut self, o: &mut Order) -> Result<(), DecodeError> {
        if self.v >= sv::PEGBEST_PEGMID_OFFSETS {
            let r = &mut self.r;
            o.min_trade_qty = r.int_max()?;
            o.min_compete_size = r.int_max()?;
            o.compete_against_best_offset = r.f64_max()?;
            o.mid_offset_at_whole = r.f64_max()?;
            o.mid_offset_at_half = r.f64_max()?;
        }
        Ok(())
    }

    fn open_order(&mut self) -> Result<Incoming, DecodeError> {
        let v = self.v;
        let mut c = Contract::default();
        let mut o = Order::default();
        let mut s = OrderState::default();
        o.order_id = self.r.int()?;
        self.order_contract(&mut c)?;
        self.order_head(&mut o, true)?;
        let r = &mut self.r;
        o.auction_strategy = r.int()?;
        o.starting_price = r.f64_max()?;
        o.stock_ref_price = r.f64_max()?;
        o.delta = r.f64_max()?;
        o.stock_range_lower = r.f64_max()?;
        o.stock_range_upper = r.f64_max()?;
        o.display_size = r.int_max()?;
        o.block_order = r.bool()?;
        o.sweep_to_fill = r.bool()?;
        o.all_or_none = r.bool()?;
        o.min_qty = r.int_max()?;
        o.oca_type = r.int()?;
        r.skip()?; // eTradeOnly
        r.skip()?; // firmQuoteOnly
        r.skip()?; // nbboPriceCap
        o.parent_id = r.int()?;
        o.trigger_method = r.int()?;
        self.vol_params(&mut o, true)?;
        let r = &mut self.r;
        o.trail_stop_price = r.f64_max()?;
        o.trailing_percent = r.f64_max()?;
        o.basis_points = r.f64_max()?;
        o.basis_points_type = r.int_max()?;
        self.combo_and_scale(&mut c, &mut o)?;
        let r = &mut self.r;
        o.opt_out_smart_routing = r.bool()?;
        o.clearing_account = r.string()?;
        o.clearing_intent = r.string()?;
        o.not_held = r.bool()?;
        self.delta_neutral_and_algo(&mut c, &mut o)?;
        let r = &mut self.r;
        o.what_if = r.bool()?;
        s.status = r.string()?;
        s.init_margin_before = r.string()?;
        s.maint_margin_before = r.string()?;
        s.equity_with_loan_before = r.string()?;
        s.init_margin_change = r.string()?;
        s.maint_margin_change = r.string()?;
        s.equity_with_loan_change = r.string()?;
        s.init_margin_after = r.string()?;
        s.maint_margin_after = r.string()?;
        s.equity_with_loan_after = r.string()?;
        s.commission = r.f64_max()?;
        s.min_commission = r.f64_max()?;
        s.max_commission = r.f64_max()?;
        s.commission_currency = r.string()?;
        if v >= sv::FULL_ORDER_PREVIEW_FIELDS {
            s.margin_currency = r.string()?;
            s.init_margin_before_outside_rth = r.f64_max()?;
            s.maint_margin_before_outside_rth = r.f64_max()?;
            s.equity_with_loan_before_outside_rth = r.f64_max()?;
            s.init_margin_change_outside_rth = r.f64_max()?;
            s.maint_margin_change_outside_rth = r.f64_max()?;
            s.equity_with_loan_change_outside_rth = r.f64_max()?;
            s.init_margin_after_outside_rth = r.f64_max()?;
            s.maint_margin_after_outside_rth = r.f64_max()?;
            s.equity_with_loan_after_outside_rth = r.f64_max()?;
            s.suggested_size = r.decimal()?;
            s.reject_reason = r.string()?;
            let n = r.int()?.max(0) as usize;
            for _ in 0..n {
                s.order_allocations.push(OrderAllocation {
                    account: r.string()?,
                    position: r.decimal()?,
                    position_desired: r.decimal()?,
                    position_after: r.decimal()?,
                    desired_alloc_qty: r.decimal()?,
                    allowed_alloc_qty: r.decimal()?,
                    is_monetary: r.bool()?,
                });
            }
        }
        s.warning_text = r.string()?;
        o.randomize_size = r.bool()?;
        o.randomize_price = r.bool()?;
        self.peg_bench_and_conditions(&mut o)?;
        let r = &mut self.r;
        o.adjusted_order_type = r.string()?;
        o.trigger_price = r.f64()?;
        o.trail_stop_price = r.f64()?;
        o.lmt_price_offset = r.f64()?;
        o.adjusted_stop_price = r.f64()?;
        o.adjusted_stop_limit_price = r.f64()?;
        o.adjusted_trailing_amount = r.f64()?;
        o.adjustable_trailing_unit = r.int()?;
        o.soft_dollar_tier = SoftDollarTier {
            name: r.string()?,
            value: r.string()?,
            display_name: r.string()?,
        };
        o.cash_qty = r.f64()?;
        o.dont_use_auto_price_for_hedge = r.bool()?;
        o.is_oms_container = r.bool()?;
        o.discretionary_up_to_limit_price = r.bool()?;
        o.use_price_mgmt_algo = Some(r.bool()?);
        if v >= sv::DURATION {
            o.duration = r.int_max()?;
        }
        if v >= sv::POST_TO_ATS {
            o.post_to_ats = r.int_max()?;
        }
        if v >= sv::AUTO_CANCEL_PARENT {
            o.auto_cancel_parent = r.bool()?;
        }
        self.peg_best_mid(&mut o)?;
        let r = &mut self.r;
        if v >= sv::CUSTOMER_ACCOUNT {
            o.customer_account = r.string()?;
        }
        if v >= sv::PROFESSIONAL_CUSTOMER {
            o.professional_customer = r.bool()?;
        }
        if v >= sv::BOND_ACCRUED_INTEREST {
            o.bond_accrued_interest = r.string()?;
        }
        if v >= sv::INCLUDE_OVERNIGHT {
            o.include_overnight = r.bool()?;
        }
        if v >= sv::CME_TAGGING_FIELDS_IN_OPEN_ORDER {
            o.ext_operator = r.string()?;
            o.manual_order_indicator = r.int_max()?;
        }
        if v >= sv::SUBMITTER {
            o.submitter = r.string()?;
        }
        if v >= sv::IMBALANCE_ONLY {
            o.imbalance_only = r.bool()?;
        }
        Ok(Incoming::OpenOrder {
            order_id: o.order_id,
            contract: Box::new(c),
            order: Box::new(o),
            state: Box::new(s),
        })
    }

    fn completed_order(&mut self) -> Result<Incoming, DecodeError> {
        let v = self.v;
        let mut c = Contract::default();
        let mut o = Order::default();
        let mut s = OrderState::default();
        self.order_contract(&mut c)?;
        self.order_head(&mut o, false)?;
        let r = &mut self.r;
        o.starting_price = r.f64_max()?;
        o.stock_ref_price = r.f64_max()?;
        o.delta = r.f64_max()?;
        o.stock_range_lower = r.f64_max()?;
        o.stock_range_upper = r.f64_max()?;
        o.display_size = r.int_max()?;
        o.sweep_to_fill = r.bool()?;
        o.all_or_none = r.bool()?;
        o.min_qty = r.int_max()?;
        o.oca_type = r.int()?;
        o.trigger_method = r.int()?;
        self.vol_params(&mut o, false)?;
        let r = &mut self.r;
        o.trail_stop_price = r.f64_max()?;
        o.trailing_percent = r.f64_max()?;
        self.combo_and_scale(&mut c, &mut o)?;
        let r = &mut self.r;
        o.clearing_account = r.string()?;
        o.clearing_intent = r.string()?;
        o.not_held = r.bool()?;
        self.delta_neutral_and_algo(&mut c, &mut o)?;
        s.status = self.r.string()?;
        let r = &mut self.r;
        o.randomize_size = r.bool()?;
        o.randomize_price = r.bool()?;
        self.peg_bench_and_conditions(&mut o)?;
        let r = &mut self.r;
        o.trail_stop_price = r.f64()?;
        o.lmt_price_offset = r.f64()?;
        o.cash_qty = r.f64()?;
        o.dont_use_auto_price_for_hedge = r.bool()?;
        o.is_oms_container = r.bool()?;
        o.auto_cancel_date = r.string()?;
        o.filled_quantity = r.decimal()?;
        o.ref_futures_con_id = r.int()?;
        o.auto_cancel_parent = r.bool()?;
        o.shareholder = r.string()?;
        o.imbalance_only = r.bool()?;
        o.route_marketable_to_bbo = Some(r.bool()?);
        o.parent_perm_id = r.long()?;
        s.completed_time = r.string()?;
        s.completed_status = r.string()?;
        self.peg_best_mid(&mut o)?;
        let r = &mut self.r;
        if v >= sv::CUSTOMER_ACCOUNT {
            o.customer_account = r.string()?;
        }
        if v >= sv::PROFESSIONAL_CUSTOMER {
            o.professional_customer = r.bool()?;
        }
        if v >= sv::SUBMITTER {
            o.submitter = r.string()?;
        }
        Ok(Incoming::CompletedOrder {
            contract: Box::new(c),
            order: Box::new(o),
            state: Box::new(s),
        })
    }
}

fn unknown(msg_id: i32, payload: &[u8]) -> Incoming {
    let mut fields: Vec<String> = payload
        .split(|b| *b == 0)
        .map(|f| String::from_utf8_lossy(f).into_owned())
        .collect();
    if fields.last().is_some_and(|f| f.is_empty()) {
        fields.pop();
    }
    Incoming::Unknown { msg_id, fields }
}

#[cfg(test)]
#[path = "decode_tests.rs"]
mod tests;
