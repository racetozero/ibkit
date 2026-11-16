//! Shared client state and the message handlers (ib_async `Wrapper`).

use crate::objects::*;
use crate::ticker::{Defaults, Ticker, greeks_slot};
use crate::trade::{OrderStatus, Trade, status};
use ibkit_tws::consts::{UNSET_DOUBLE, is_warning_code};
use ibkit_tws::decode::{Incoming, implied_size_tick};
use ibkit_tws::types::*;
use parking_lot::{Mutex, RwLock};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::oneshot;

pub type TickerHandle = Arc<RwLock<Ticker>>;
pub type TradeHandle = Arc<Mutex<Trade>>;
pub type BarsHandle = Arc<RwLock<BarDataList>>;
pub type RealTimeBarsHandle = Arc<RwLock<RealTimeBarList>>;
pub type ScanHandle = Arc<RwLock<ScanDataList>>;
pub type PnLHandle = Arc<RwLock<PnL>>;
pub type PnLSingleHandle = Arc<RwLock<PnLSingle>>;

/// Key of a trade: (clientId, orderId), or permId for orders without an id.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum OrderKey {
    Client(i32, i32),
    Perm(i64),
}

pub fn order_key(client_id: i32, order_id: i32, perm_id: i64) -> OrderKey {
    if order_id <= 0 {
        OrderKey::Perm(perm_id)
    } else {
        OrderKey::Client(client_id, order_id)
    }
}

/// An error the server reported for one request.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("request {req_id} failed with error {code}: {message}")]
pub struct RequestError {
    pub req_id: i32,
    pub code: i32,
    pub message: String,
}

/// Key of a pending request.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ReqKey {
    Id(i32),
    Positions,
    OpenOrders,
    CompletedOrders,
    AccountValues,
    CurrentTime,
    CurrentTimeMillis,
    ScannerParams,
    NewsProviders,
    RequestFa,
    MktDepthExchanges,
    FamilyCodes,
    MarketRule(i32),
}

/// Items gathered for a pending request.
#[derive(Clone, Debug)]
pub(crate) enum Collected {
    Msg(Incoming),
    Trade(TradeHandle),
    Fill(Box<Fill>),
    Position(Box<Position>),
}

pub(crate) type Reply = Result<Vec<Collected>, RequestError>;

pub(crate) struct Pending {
    pub items: Vec<Collected>,
    pub tx: oneshot::Sender<Reply>,
}

/// Live subscription that receives streaming updates.
#[derive(Clone)]
pub(crate) enum Subscriber {
    Bars(BarsHandle),
    RealTimeBars(RealTimeBarsHandle),
    Scan(ScanHandle),
}

/// Something that happened, broadcast to [`crate::IB::events`] subscribers.
#[derive(Clone, Debug)]
pub enum Event {
    /// Every decoded message, when `IbOptions::emit_messages` is set.
    Message(Arc<Incoming>),
    Connected,
    Disconnected(String),
    /// Emitted once per network packet after all its messages are processed.
    Update,
    PendingTickers(Vec<TickerHandle>),
    BarUpdate {
        req_id: i32,
        has_new_bar: bool,
    },
    NewOrder(TradeHandle),
    OrderModify(TradeHandle),
    CancelOrder(TradeHandle),
    OpenOrder(TradeHandle),
    OrderStatus(TradeHandle),
    Filled(TradeHandle),
    Cancelled(TradeHandle),
    ExecDetails(TradeHandle, Box<Fill>),
    CommissionReport(TradeHandle, Box<Fill>, CommissionReport),
    UpdatePortfolio(Box<PortfolioItem>),
    Position(Box<Position>),
    AccountValue(AccountValue),
    AccountSummary(AccountValue),
    PnL(PnL),
    PnLSingle(PnLSingle),
    ScannerData(i32),
    TickNews(NewsTick),
    NewsBulletin(NewsBulletin),
    WshMeta(String),
    WshEvent(String),
    Error {
        req_id: i32,
        code: i32,
        message: String,
        contract: Option<Box<Contract>>,
    },
    Timeout(f64),
}

pub(crate) struct State {
    pub client_id: i32,
    pub defaults: Defaults,
    pub raise_request_errors: bool,
    pub account_values: BTreeMap<(String, String, String, String), AccountValue>,
    pub acct_summary: BTreeMap<(String, String, String), AccountValue>,
    pub portfolio: BTreeMap<String, BTreeMap<i32, PortfolioItem>>,
    pub positions: BTreeMap<String, BTreeMap<i32, Position>>,
    pub trades: HashMap<OrderKey, TradeHandle>,
    pub trade_order: Vec<OrderKey>,
    pub perm_id_to_trade: HashMap<i64, TradeHandle>,
    pub fills: HashMap<String, Fill>,
    pub fill_order: Vec<String>,
    pub news_ticks: Vec<NewsTick>,
    pub news_bulletins: BTreeMap<i32, NewsBulletin>,
    pub tickers: HashMap<String, TickerHandle>,
    pub req_id_to_ticker: HashMap<i32, TickerHandle>,
    pub ticker_req_ids: HashMap<(String, String), i32>,
    pub req_id_to_contract: HashMap<i32, Contract>,
    pub subscribers: HashMap<i32, Subscriber>,
    pub pnls: HashMap<i32, PnLHandle>,
    pub pnl_singles: HashMap<i32, PnLSingleHandle>,
    pub pnl_keys: HashMap<(String, String), i32>,
    pub pnl_single_keys: HashMap<(String, String, i32), i32>,
    pub pending: HashMap<ReqKey, Pending>,
    pending_tickers: Vec<TickerHandle>,
    pending_ids: HashSet<usize>,
    pub last_time: Timestamp,
    pub events: Vec<Event>,
    /// Bar subscriptions to re-request after error 10225, done by the dispatcher.
    pub resubscribe: Vec<Subscriber>,
}

impl State {
    pub fn new(client_id: i32, defaults: Defaults, raise_request_errors: bool) -> Self {
        Self {
            client_id,
            defaults,
            raise_request_errors,
            account_values: BTreeMap::new(),
            acct_summary: BTreeMap::new(),
            portfolio: BTreeMap::new(),
            positions: BTreeMap::new(),
            trades: HashMap::new(),
            trade_order: Vec::new(),
            perm_id_to_trade: HashMap::new(),
            fills: HashMap::new(),
            fill_order: Vec::new(),
            news_ticks: Vec::new(),
            news_bulletins: BTreeMap::new(),
            tickers: HashMap::new(),
            req_id_to_ticker: HashMap::new(),
            ticker_req_ids: HashMap::new(),
            req_id_to_contract: HashMap::new(),
            subscribers: HashMap::new(),
            pnls: HashMap::new(),
            pnl_singles: HashMap::new(),
            pnl_keys: HashMap::new(),
            pnl_single_keys: HashMap::new(),
            pending: HashMap::new(),
            pending_tickers: Vec::new(),
            pending_ids: HashSet::new(),
            last_time: now(),
            events: Vec::new(),
            resubscribe: Vec::new(),
        }
    }

    // ---- request bookkeeping -------------------------------------------

    pub fn start_req(
        &mut self,
        key: ReqKey,
        contract: Option<&Contract>,
    ) -> oneshot::Receiver<Reply> {
        let (tx, rx) = oneshot::channel();
        if let (ReqKey::Id(id), Some(c)) = (&key, contract) {
            self.req_id_to_contract.insert(*id, c.clone());
        }
        self.pending.insert(
            key,
            Pending {
                items: Vec::new(),
                tx,
            },
        );
        rx
    }

    /// Remove a request with everything recorded for it: its contract, and
    /// the ticker of a snapshot (`req_tickers`).
    pub(crate) fn take_req(&mut self, key: &ReqKey) -> Option<Pending> {
        if let ReqKey::Id(id) = key {
            self.req_id_to_contract.remove(id);
            self.req_id_to_ticker.remove(id);
        }
        self.pending.remove(key)
    }

    pub fn end_req(&mut self, key: &ReqKey, extra: Option<Collected>) {
        if let Some(mut p) = self.take_req(key) {
            p.items.extend(extra);
            let _ = p.tx.send(Ok(p.items));
        }
    }

    fn fail_req(&mut self, key: &ReqKey, err: RequestError) {
        if let Some(p) = self.take_req(key) {
            let _ = p.tx.send(Err(err));
        }
    }

    fn push(&mut self, key: &ReqKey, item: Collected) -> bool {
        match self.pending.get_mut(key) {
            Some(p) => {
                p.items.push(item);
                true
            }
            None => false,
        }
    }

    /// The ticker of `contract`, which `req_id` updates until the request ends.
    pub fn ticker_for(&mut self, req_id: i32, contract: &Contract) -> TickerHandle {
        let ticker = self
            .tickers
            .entry(contract.key())
            .or_insert_with(|| Arc::new(RwLock::new(Ticker::new(contract.clone()))))
            .clone();
        self.req_id_to_ticker.insert(req_id, ticker.clone());
        self.req_id_to_contract.insert(req_id, contract.clone());
        ticker
    }

    /// A streaming subscription of `kind`, which `end_ticker` cancels.
    pub fn start_ticker(&mut self, req_id: i32, contract: &Contract, kind: &str) -> TickerHandle {
        let ticker = self.ticker_for(req_id, contract);
        self.ticker_req_ids
            .insert((kind.to_string(), contract.key()), req_id);
        ticker
    }

    pub fn end_ticker(&mut self, contract: &Contract, kind: &str) -> Option<i32> {
        let req_id = self
            .ticker_req_ids
            .remove(&(kind.to_string(), contract.key()))?;
        self.req_id_to_contract.remove(&req_id);
        self.req_id_to_ticker.remove(&req_id);
        Some(req_id)
    }

    pub fn add_trade(&mut self, key: OrderKey, trade: TradeHandle) {
        if self.trades.insert(key.clone(), trade).is_none() {
            self.trade_order.push(key);
        }
    }

    fn mark_pending(&mut self, t: &TickerHandle) {
        if self.pending_ids.insert(Arc::as_ptr(t) as usize) {
            self.pending_tickers.push(t.clone());
        }
    }

    /// Start of a network packet (ib_async `tcpDataArrived`).
    pub fn begin_batch(&mut self) {
        self.last_time = now();
        for t in self.pending_tickers.drain(..) {
            t.write().clear_ticks();
        }
        self.pending_ids.clear();
    }

    /// End of a network packet (ib_async `tcpDataProcessed`).
    pub fn end_batch(&mut self) -> (Vec<Event>, Vec<Subscriber>) {
        let mut events = std::mem::take(&mut self.events);
        events.push(Event::Update);
        if !self.pending_tickers.is_empty() {
            for t in &self.pending_tickers {
                t.write().time = Some(self.last_time);
            }
            events.push(Event::PendingTickers(self.pending_tickers.clone()));
        }
        (events, std::mem::take(&mut self.resubscribe))
    }

    /// Fail every pending request after the connection closed.
    pub fn connection_closed(&mut self, reason: &str) {
        for (key, p) in self.pending.drain() {
            let req_id = if let ReqKey::Id(id) = key { id } else { -1 };
            let _ = p.tx.send(Err(RequestError {
                req_id,
                code: 504,
                message: format!("connection closed: {reason}"),
            }));
        }
    }

    fn ticker(&self, req_id: i32) -> Option<TickerHandle> {
        self.req_id_to_ticker.get(&req_id).cloned()
    }

    // ---- message handlers ----------------------------------------------

    pub fn apply(&mut self, msg: Incoming) {
        let t = self.last_time;
        let d = self.defaults;
        match msg {
            Incoming::TickPrice {
                req_id,
                tick_type,
                price,
                size,
                ..
            } => {
                let Some(ticker) = self.ticker(req_id) else {
                    return;
                };
                if price != 0.0 {
                    let size = if size == UNSET_DOUBLE || implied_size_tick(tick_type).is_none() {
                        0.0
                    } else {
                        size
                    };
                    ticker.write().on_price(t, tick_type, price, size, &d);
                    self.mark_pending(&ticker);
                }
            }
            Incoming::TickSize {
                req_id,
                tick_type,
                size,
            } => {
                let Some(ticker) = self.ticker(req_id) else {
                    return;
                };
                let size = if size == UNSET_DOUBLE { 0.0 } else { size };
                if ticker.write().on_size(t, tick_type, size, &d) {
                    self.mark_pending(&ticker);
                }
            }
            Incoming::TickGeneric {
                req_id,
                tick_type,
                value,
            } => {
                if let Some(ticker) = self.ticker(req_id) {
                    ticker.write().on_generic(t, tick_type, value, &d);
                    self.mark_pending(&ticker);
                }
            }
            Incoming::TickString {
                req_id,
                tick_type,
                value,
            } => {
                if let Some(ticker) = self.ticker(req_id) {
                    ticker.write().on_string(t, tick_type, &value);
                    self.mark_pending(&ticker);
                }
            }
            Incoming::TickEfp {
                req_id,
                tick_type,
                efp,
            } => {
                if let Some(ticker) = self.ticker(req_id) {
                    ticker.write().on_efp(tick_type, efp);
                    self.mark_pending(&ticker);
                }
            }
            Incoming::TickOptionComputation {
                req_id,
                tick_type,
                computation,
            } => {
                if let Some(ticker) = self.ticker(req_id) {
                    if let Some(slot) = greeks_slot(&mut ticker.write(), tick_type) {
                        *slot = Some(computation);
                    }
                    self.mark_pending(&ticker);
                } else {
                    let key = ReqKey::Id(req_id);
                    self.end_req(
                        &key,
                        Some(Collected::Msg(Incoming::TickOptionComputation {
                            req_id,
                            tick_type,
                            computation,
                        })),
                    );
                }
            }
            Incoming::TickReqParams {
                req_id,
                min_tick,
                bbo_exchange,
                snapshot_permissions,
            } => {
                if let Some(ticker) = self.ticker(req_id) {
                    let mut w = ticker.write();
                    w.min_tick = min_tick;
                    w.bbo_exchange = bbo_exchange;
                    w.snapshot_permissions = snapshot_permissions;
                }
            }
            Incoming::TickSnapshotEnd { req_id } => self.end_req(&ReqKey::Id(req_id), None),
            Incoming::MarketDataType {
                req_id,
                market_data_type,
            } => {
                if let Some(ticker) = self.ticker(req_id) {
                    ticker.write().market_data_type = market_data_type;
                }
            }
            Incoming::TickByTickAllLast {
                req_id,
                tick_type,
                time,
                price,
                size,
                attrib,
                exchange,
                special_conditions,
            } => {
                if let Some(ticker) = self.ticker(req_id) {
                    let tick = TickByTick::AllLast {
                        tick_type,
                        time: time as f64,
                        price,
                        size,
                        tick_attrib_last: attrib,
                        exchange,
                        special_conditions,
                    };
                    ticker.write().on_tbt_all_last(tick, price, size, &d);
                    self.mark_pending(&ticker);
                }
            }
            Incoming::TickByTickBidAsk {
                req_id,
                time,
                bid_price,
                ask_price,
                bid_size,
                ask_size,
                attrib,
            } => {
                if let Some(ticker) = self.ticker(req_id) {
                    let tick = TickByTick::BidAsk {
                        time: time as f64,
                        bid_price,
                        ask_price,
                        bid_size,
                        ask_size,
                        tick_attrib_bid_ask: attrib,
                    };
                    ticker
                        .write()
                        .on_tbt_bid_ask(tick, bid_price, ask_price, bid_size, ask_size, &d);
                    self.mark_pending(&ticker);
                }
            }
            Incoming::TickByTickMidPoint {
                req_id,
                time,
                mid_point,
            } => {
                if let Some(ticker) = self.ticker(req_id) {
                    ticker.write().tick_by_ticks.push(TickByTick::MidPoint {
                        time: time as f64,
                        mid_point,
                    });
                    self.mark_pending(&ticker);
                }
            }
            Incoming::UpdateMktDepth {
                req_id,
                position,
                operation,
                side,
                price,
                size,
            } => {
                if let Some(ticker) = self.ticker(req_id) {
                    ticker
                        .write()
                        .on_depth(t, position, "", operation, side, price, size);
                    self.mark_pending(&ticker);
                }
            }
            Incoming::UpdateMktDepthL2 {
                req_id,
                position,
                market_maker,
                operation,
                side,
                price,
                size,
                ..
            } => {
                if let Some(ticker) = self.ticker(req_id) {
                    ticker.write().on_depth(
                        t,
                        position,
                        &market_maker,
                        operation,
                        side,
                        price,
                        size,
                    );
                    self.mark_pending(&ticker);
                }
            }
            Incoming::OrderStatus(s) => self.order_status(s),
            Incoming::OpenOrder {
                order_id,
                contract,
                order,
                state,
            } => self.open_order(order_id, *contract, *order, *state),
            Incoming::OpenOrderEnd => self.end_req(&ReqKey::OpenOrders, None),
            Incoming::CompletedOrder {
                contract,
                order,
                state,
            } => {
                let trade = Arc::new(Mutex::new(Trade {
                    contract: *contract,
                    order_status: OrderStatus {
                        order_id: order.order_id,
                        status: state.status.clone(),
                        ..Default::default()
                    },
                    order: *order,
                    ..Default::default()
                }));
                let perm_id = trade.lock().order.perm_id;
                self.push(&ReqKey::CompletedOrders, Collected::Trade(trade.clone()));
                if !self.perm_id_to_trade.contains_key(&perm_id) {
                    self.add_trade(OrderKey::Perm(perm_id), trade.clone());
                    self.perm_id_to_trade.insert(perm_id, trade);
                }
            }
            Incoming::CompletedOrdersEnd => self.end_req(&ReqKey::CompletedOrders, None),
            Incoming::ExecDetails {
                req_id,
                contract,
                execution,
            } => self.exec_details(req_id, *contract, *execution),
            Incoming::ExecDetailsEnd { req_id } => self.end_req(&ReqKey::Id(req_id), None),
            Incoming::CommissionReport(mut r) => {
                if r.yield_ == UNSET_DOUBLE {
                    r.yield_ = 0.0;
                }
                if r.realized_pnl == UNSET_DOUBLE {
                    r.realized_pnl = 0.0;
                }
                if let Some(fill) = self.fills.get_mut(&r.exec_id) {
                    fill.commission_report = r.clone();
                    let fill = fill.clone();
                    if let Some(trade) = self.perm_id_to_trade.get(&fill.execution.perm_id).cloned()
                    {
                        {
                            let mut tr = trade.lock();
                            if let Some(f) = tr
                                .fills
                                .iter_mut()
                                .find(|f| f.execution.exec_id == r.exec_id)
                            {
                                f.commission_report = r.clone();
                            }
                        }
                        self.events
                            .push(Event::CommissionReport(trade, Box::new(fill), r));
                    }
                }
            }
            Incoming::Position {
                account,
                contract,
                position,
                avg_cost,
            } => {
                let p = Position {
                    account: account.clone(),
                    contract: *contract,
                    position,
                    avg_cost,
                };
                let map = self.positions.entry(account).or_default();
                if position == 0.0 {
                    map.remove(&p.contract.con_id);
                } else {
                    map.insert(p.contract.con_id, p.clone());
                }
                self.push(&ReqKey::Positions, Collected::Position(Box::new(p.clone())));
                self.events.push(Event::Position(Box::new(p)));
            }
            Incoming::PositionEnd => self.end_req(&ReqKey::Positions, None),
            Incoming::UpdatePortfolio(p) => {
                let p = *p;
                let item = PortfolioItem {
                    contract: p.contract,
                    position: p.position,
                    market_price: p.market_price,
                    market_value: p.market_value,
                    average_cost: p.average_cost,
                    unrealized_pnl: p.unrealized_pnl,
                    realized_pnl: p.realized_pnl,
                    account: p.account.clone(),
                };
                let map = self.portfolio.entry(p.account).or_default();
                if item.position == 0.0 {
                    map.remove(&item.contract.con_id);
                } else {
                    map.insert(item.contract.con_id, item.clone());
                }
                self.events.push(Event::UpdatePortfolio(Box::new(item)));
            }
            Incoming::UpdateAccountValue {
                key,
                value,
                currency,
                account,
            } => {
                let v = AccountValue {
                    account: account.clone(),
                    tag: key.clone(),
                    value,
                    currency: currency.clone(),
                    model_code: String::new(),
                };
                self.account_values
                    .insert((account, key, currency, String::new()), v.clone());
                self.events.push(Event::AccountValue(v));
            }
            Incoming::AccountDownloadEnd { .. } => self.end_req(&ReqKey::AccountValues, None),
            Incoming::AccountUpdateMulti {
                account,
                model_code,
                key,
                value,
                currency,
                ..
            } => {
                let v = AccountValue {
                    account: account.clone(),
                    tag: key.clone(),
                    value,
                    currency: currency.clone(),
                    model_code: model_code.clone(),
                };
                self.account_values
                    .insert((account, key, currency, model_code), v.clone());
                self.events.push(Event::AccountValue(v));
            }
            Incoming::AccountUpdateMultiEnd { req_id } => self.end_req(&ReqKey::Id(req_id), None),
            Incoming::AccountSummary {
                account,
                tag,
                value,
                currency,
                ..
            } => {
                let v = AccountValue {
                    account: account.clone(),
                    tag: tag.clone(),
                    value,
                    currency: currency.clone(),
                    model_code: String::new(),
                };
                self.acct_summary
                    .insert((account, tag, currency), v.clone());
                self.events.push(Event::AccountSummary(v));
            }
            Incoming::AccountSummaryEnd { req_id } => self.end_req(&ReqKey::Id(req_id), None),
            Incoming::Pnl {
                req_id,
                daily_pnl,
                unrealized_pnl,
                realized_pnl,
            } => {
                if let Some(h) = self.pnls.get(&req_id) {
                    let mut p = h.write();
                    p.daily_pnl = daily_pnl;
                    p.unrealized_pnl = unrealized_pnl;
                    p.realized_pnl = realized_pnl;
                    self.events.push(Event::PnL(p.clone()));
                }
            }
            Incoming::PnlSingle {
                req_id,
                position,
                daily_pnl,
                unrealized_pnl,
                realized_pnl,
                value,
            } => {
                if let Some(h) = self.pnl_singles.get(&req_id) {
                    let mut p = h.write();
                    p.position = position;
                    p.daily_pnl = daily_pnl;
                    p.unrealized_pnl = unrealized_pnl;
                    p.realized_pnl = realized_pnl;
                    p.value = value;
                    self.events.push(Event::PnLSingle(p.clone()));
                }
            }
            m @ (Incoming::ContractDetails { .. } | Incoming::BondContractDetails { .. }) => {
                let key = ReqKey::Id(m.req_id().unwrap_or_default());
                self.push(&key, Collected::Msg(m));
            }
            Incoming::ContractDetailsEnd { req_id } => self.end_req(&ReqKey::Id(req_id), None),
            Incoming::RealtimeBar { req_id, bar } => {
                if let Some(Subscriber::RealTimeBars(h)) = self.subscribers.get(&req_id) {
                    h.write().bars.push(bar);
                    self.events.push(Event::BarUpdate {
                        req_id,
                        has_new_bar: true,
                    });
                }
            }
            Incoming::HistoricalData { req_id, bars, end } => {
                if let Some(Subscriber::Bars(h)) = self.subscribers.get(&req_id) {
                    h.write().bars.extend(bars);
                }
                if end.is_some() {
                    self.finish_bars(req_id);
                }
            }
            Incoming::HistoricalDataEnd { req_id, .. } => self.finish_bars(req_id),
            Incoming::HistoricalDataUpdate { req_id, bar } => {
                let Some(Subscriber::Bars(h)) = self.subscribers.get(&req_id).cloned() else {
                    return;
                };
                let mut list = h.write();
                let has_new_bar = match list.bars.last() {
                    None => true,
                    Some(last) if bar.date < last.date => return,
                    Some(last) if bar.date > last.date => true,
                    Some(last) if *last == bar => return,
                    Some(_) => false,
                };
                if has_new_bar {
                    list.bars.push(bar);
                } else if let Some(last) = list.bars.last_mut() {
                    *last = bar;
                }
                self.events.push(Event::BarUpdate {
                    req_id,
                    has_new_bar,
                });
            }
            Incoming::ScannerData { req_id, rows } => {
                if let Some(Subscriber::Scan(h)) = self.subscribers.get(&req_id) {
                    h.write().rows = rows;
                    self.events.push(Event::ScannerData(req_id));
                }
                self.end_req(&ReqKey::Id(req_id), None);
            }
            Incoming::SecurityDefinitionOptionParameter(p) => {
                let key = ReqKey::Id(p.req_id);
                self.push(
                    &key,
                    Collected::Msg(Incoming::SecurityDefinitionOptionParameter(p)),
                );
            }
            Incoming::SecurityDefinitionOptionParameterEnd { req_id } => {
                self.end_req(&ReqKey::Id(req_id), None)
            }
            m @ (Incoming::HistoricalNews { .. }
            | Incoming::HistoricalTicks { .. }
            | Incoming::HistoricalTicksBidAsk { .. }
            | Incoming::HistoricalTicksLast { .. }) => {
                let req_id = m.req_id().unwrap_or_default();
                let done = matches!(
                    m,
                    Incoming::HistoricalTicks { done: true, .. }
                        | Incoming::HistoricalTicksBidAsk { done: true, .. }
                        | Incoming::HistoricalTicksLast { done: true, .. }
                );
                self.push(&ReqKey::Id(req_id), Collected::Msg(m));
                if done {
                    self.end_req(&ReqKey::Id(req_id), None);
                }
            }
            Incoming::HistoricalNewsEnd { req_id, .. } => self.end_req(&ReqKey::Id(req_id), None),
            m @ (Incoming::SymbolSamples { .. }
            | Incoming::HeadTimestamp { .. }
            | Incoming::HistogramData { .. }
            | Incoming::SmartComponents { .. }
            | Incoming::FundamentalData { .. }
            | Incoming::NewsArticle { .. }
            | Incoming::HistoricalSchedule { .. }
            | Incoming::UserInfo { .. }
            | Incoming::ConfigResponse { .. }
            | Incoming::UpdateConfigResponse { .. }
            | Incoming::ReplaceFaEnd { .. }
            | Incoming::SoftDollarTiers { .. }) => {
                let key = ReqKey::Id(m.req_id().unwrap_or_default());
                self.end_req(&key, Some(Collected::Msg(m)));
            }
            Incoming::WshMetaData { req_id, data_json } => {
                self.events.push(Event::WshMeta(data_json.clone()));
                self.end_req(
                    &ReqKey::Id(req_id),
                    Some(Collected::Msg(Incoming::WshMetaData { req_id, data_json })),
                );
            }
            Incoming::WshEventData { req_id, data_json } => {
                self.events.push(Event::WshEvent(data_json.clone()));
                self.end_req(
                    &ReqKey::Id(req_id),
                    Some(Collected::Msg(Incoming::WshEventData { req_id, data_json })),
                );
            }
            Incoming::MarketRule {
                market_rule_id,
                increments,
            } => self.end_req(
                &ReqKey::MarketRule(market_rule_id),
                Some(Collected::Msg(Incoming::MarketRule {
                    market_rule_id,
                    increments,
                })),
            ),
            m @ Incoming::MktDepthExchanges { .. } => {
                self.end_req(&ReqKey::MktDepthExchanges, Some(Collected::Msg(m)))
            }
            m @ Incoming::NewsProviders { .. } => {
                self.end_req(&ReqKey::NewsProviders, Some(Collected::Msg(m)))
            }
            m @ Incoming::ScannerParameters { .. } => {
                self.end_req(&ReqKey::ScannerParams, Some(Collected::Msg(m)))
            }
            m @ Incoming::ReceiveFa { .. } => {
                self.end_req(&ReqKey::RequestFa, Some(Collected::Msg(m)))
            }
            m @ Incoming::CurrentTime { .. } => {
                self.end_req(&ReqKey::CurrentTime, Some(Collected::Msg(m)))
            }
            m @ Incoming::CurrentTimeInMillis { .. } => {
                self.end_req(&ReqKey::CurrentTimeMillis, Some(Collected::Msg(m)))
            }
            m @ Incoming::FamilyCodes { .. } => {
                self.end_req(&ReqKey::FamilyCodes, Some(Collected::Msg(m)))
            }
            Incoming::TickNews {
                time_stamp,
                provider_code,
                article_id,
                headline,
                extra_data,
                ..
            } => {
                let n = NewsTick {
                    time_stamp,
                    provider_code,
                    article_id,
                    headline,
                    extra_data,
                };
                self.news_ticks.push(n.clone());
                self.events.push(Event::TickNews(n));
            }
            Incoming::UpdateNewsBulletin {
                msg_id,
                msg_type,
                message,
                orig_exchange,
            } => {
                let b = NewsBulletin {
                    msg_id,
                    msg_type,
                    message,
                    orig_exchange,
                };
                self.news_bulletins.insert(msg_id, b.clone());
                self.events.push(Event::NewsBulletin(b));
            }
            Incoming::Error(e) => {
                self.error(e.req_id, e.code, e.message, e.advanced_order_reject_json)
            }
            Incoming::ManagedAccounts { .. }
            | Incoming::NextValidId { .. }
            | Incoming::UpdateAccountTime { .. }
            | Incoming::OrderBound { .. }
            | Incoming::DeltaNeutralValidation { .. }
            | Incoming::PositionMulti { .. }
            | Incoming::PositionMultiEnd { .. }
            | Incoming::RerouteMktDataReq { .. }
            | Incoming::RerouteMktDepthReq { .. }
            | Incoming::VerifyMessageApi { .. }
            | Incoming::VerifyCompleted { .. }
            | Incoming::VerifyAndAuthMessageApi { .. }
            | Incoming::VerifyAndAuthCompleted { .. }
            | Incoming::DisplayGroupList { .. }
            | Incoming::DisplayGroupUpdated { .. }
            | Incoming::Unknown { .. } => {}
        }
    }

    fn finish_bars(&mut self, req_id: i32) {
        let keep = matches!(self.subscribers.get(&req_id), Some(Subscriber::Bars(h)) if h.read().keep_up_to_date);
        if !keep {
            self.subscribers.remove(&req_id);
        }
        self.end_req(&ReqKey::Id(req_id), None);
    }

    fn open_order(&mut self, order_id: i32, contract: Contract, order: Order, state: OrderState) {
        if order.what_if {
            let margin: f64 = state.init_margin_change.parse().unwrap_or(UNSET_DOUBLE);
            if margin != UNSET_DOUBLE {
                self.end_req(
                    &ReqKey::Id(order.order_id),
                    Some(Collected::Msg(Incoming::OpenOrder {
                        order_id,
                        contract: Box::new(contract),
                        order: Box::new(order),
                        state: Box::new(state),
                    })),
                );
            }
            return;
        }
        let key = order_key(order.client_id, order.order_id, order.perm_id);
        let trade = match self.trades.get(&key).cloned() {
            Some(trade) => {
                {
                    let mut t = trade.lock();
                    t.order.perm_id = order.perm_id;
                    t.order.total_quantity = order.total_quantity;
                    t.order.lmt_price = order.lmt_price;
                    t.order.aux_price = order.aux_price;
                    t.order.order_type = order.order_type.clone();
                    t.order.order_ref = order.order_ref.clone();
                }
                trade
            }
            None => {
                let mut contract = contract;
                if contract.right == "?" {
                    contract.right.clear();
                }
                let trade = Arc::new(Mutex::new(Trade {
                    contract,
                    order_status: OrderStatus {
                        order_id,
                        status: state.status.clone(),
                        ..Default::default()
                    },
                    order,
                    ..Default::default()
                }));
                self.add_trade(key, trade.clone());
                trade
            }
        };
        let perm_id = trade.lock().order.perm_id;
        self.perm_id_to_trade
            .entry(perm_id)
            .or_insert_with(|| trade.clone());
        if !self.push(&ReqKey::OpenOrders, Collected::Trade(trade.clone())) {
            self.events.push(Event::OpenOrder(trade));
        }
    }

    fn order_status(&mut self, s: ibkit_tws::decode::OrderStatusMsg) {
        let key = order_key(s.client_id, s.order_id, s.perm_id);
        let Some(trade) = self.trades.get(&key).cloned() else {
            tracing::error!(
                order_id = s.order_id,
                client_id = s.client_id,
                "orderStatus: no order found"
            );
            return;
        };
        let new = OrderStatus {
            order_id: trade.lock().order_status.order_id,
            status: s.status.clone(),
            filled: s.filled,
            remaining: s.remaining,
            avg_fill_price: s.avg_fill_price,
            perm_id: s.perm_id,
            parent_id: s.parent_id,
            last_fill_price: s.last_fill_price,
            client_id: s.client_id,
            why_held: s.why_held,
            mkt_cap_price: s.mkt_cap_price,
        };
        let (msg, old_status) = {
            let mut t = trade.lock();
            let old = t.order_status.status.clone();
            let msg = if t.order_status != new {
                t.order_status = new;
                Some(String::new())
            } else if s.status == status::SUBMITTED
                && t.log.last().is_some_and(|l| l.message == "Modify")
            {
                Some("Modified".to_string())
            } else {
                None
            };
            if let Some(m) = &msg {
                t.log.push(TradeLogEntry {
                    time: self.last_time,
                    status: s.status.clone(),
                    message: m.clone(),
                    error_code: 0,
                });
            }
            (msg, old)
        };
        if msg.is_some() {
            self.events.push(Event::OrderStatus(trade.clone()));
            if s.status != old_status {
                if s.status == status::FILLED {
                    self.events.push(Event::Filled(trade));
                } else if s.status == status::CANCELLED {
                    self.events.push(Event::Cancelled(trade));
                }
            }
        }
    }

    fn exec_details(&mut self, req_id: i32, contract: Contract, mut execution: Execution) {
        if execution.order_id == ibkit_tws::consts::UNSET_INTEGER {
            execution.order_id = 0;
        }
        let trade = self
            .perm_id_to_trade
            .get(&execution.perm_id)
            .cloned()
            .or_else(|| {
                self.trades
                    .get(&order_key(
                        execution.client_id,
                        execution.order_id,
                        execution.perm_id,
                    ))
                    .cloned()
            });
        let is_live = !self.pending.contains_key(&ReqKey::Id(req_id));
        let fill = Fill {
            contract,
            execution,
            commission_report: CommissionReport::default(),
            time: self.last_time,
        };
        let exec_id = fill.execution.exec_id.clone();
        if !self.fills.contains_key(&exec_id) {
            self.fills.insert(exec_id.clone(), fill.clone());
            self.fill_order.push(exec_id);
            if let Some(trade) = &trade {
                {
                    let mut t = trade.lock();
                    t.fills.push(fill.clone());
                    let status = t.order_status.status.clone();
                    t.log.push(TradeLogEntry {
                        time: self.last_time,
                        status,
                        message: format!("Fill {}@{}", fill.execution.shares, fill.execution.price),
                        error_code: 0,
                    });
                }
                if is_live {
                    self.events
                        .push(Event::ExecDetails(trade.clone(), Box::new(fill.clone())));
                }
            }
        }
        if !is_live {
            self.push(&ReqKey::Id(req_id), Collected::Fill(Box::new(fill)));
        }
    }

    fn error(&mut self, req_id: i32, code: i32, message: String, advanced: String) {
        let is_request = self.pending.contains_key(&ReqKey::Id(req_id));
        let trade = if req_id != -1 {
            self.trades
                .get(&OrderKey::Client(self.client_id, req_id))
                .cloned()
        } else {
            None
        };
        let mut is_warning = is_warning_code(code);
        if code == 110 && is_request {
            is_warning = false;
        }
        if code == 110
            && trade
                .as_ref()
                .is_some_and(|t| t.lock().order_status.status == status::PENDING_SUBMIT)
        {
            is_warning = false;
        }
        let contract = self.req_id_to_contract.get(&req_id).cloned();
        let mut text = format!(
            "{} {code}, reqId {req_id}: {message}",
            if is_warning { "Warning" } else { "Error" }
        );
        if let Some(c) = &contract {
            text.push_str(&format!(
                ", contract: {} {} {}",
                c.sec_type, c.symbol, c.local_symbol
            ));
        }
        if is_warning {
            if let Some(trade) = &trade {
                {
                    let mut t = trade.lock();
                    t.order_status.status = status::VALIDATION_ERROR.into();
                    t.log.push(TradeLogEntry {
                        time: self.last_time,
                        status: status::VALIDATION_ERROR.into(),
                        message: text.clone(),
                        error_code: code,
                    });
                }
                tracing::warn!("{text}");
                self.events.push(Event::OrderStatus(trade.clone()));
            } else {
                tracing::info!("{text}");
            }
        } else {
            tracing::error!("{text}");
            if is_request {
                let key = ReqKey::Id(req_id);
                if self.raise_request_errors {
                    self.fail_req(
                        &key,
                        RequestError {
                            req_id,
                            code,
                            message: message.clone(),
                        },
                    );
                } else {
                    self.end_req(&key, None);
                }
            } else if let Some(trade) = &trade {
                let cancelled = {
                    let mut t = trade.lock();
                    if !advanced.is_empty() {
                        t.advanced_error = advanced;
                    }
                    if !t.is_done() {
                        t.order_status.status = status::CANCELLED.into();
                        t.log.push(TradeLogEntry {
                            time: self.last_time,
                            status: status::CANCELLED.into(),
                            message: text.clone(),
                            error_code: code,
                        });
                        true
                    } else {
                        false
                    }
                };
                if cancelled {
                    self.events.push(Event::OrderStatus(trade.clone()));
                    self.events.push(Event::Cancelled(trade.clone()));
                }
            }
        }
        match code {
            165 => {
                if let Some(Subscriber::Scan(h)) = self.subscribers.get(&req_id) {
                    h.write().rows.clear();
                    self.events.push(Event::ScannerData(req_id));
                }
            }
            317 => {
                if let Some(ticker) = self.ticker(req_id) {
                    ticker.write().reset_depth(self.last_time);
                    self.mark_pending(&ticker);
                }
            }
            10225 => {
                if let Some(s) = self.subscribers.get(&req_id).cloned() {
                    self.resubscribe.push(s);
                }
            }
            _ => {}
        }
        self.events.push(Event::Error {
            req_id,
            code,
            message,
            contract: contract.map(Box::new),
        });
    }
}
