//! The high-level client (ib_async `IB`).

use crate::objects::*;
use crate::order::BracketOrder;
use crate::state::*;
use crate::ticker::Defaults;
use crate::trade::{OrderStatus, Trade, status};
use ibkit_tws::client::{Client, ConnectOptions, ConnectionStats, EventReceiver};
use ibkit_tws::decode::{Incoming, OptionComputation};
use ibkit_tws::types::*;
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};

/// Errors of the high-level client.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Tws(#[from] ibkit_tws::Error),
    #[error(transparent)]
    Request(#[from] RequestError),
    #[error(
        "{what} got no answer within {timeout:?}; raise IbOptions::request_timeout or check the TWS log"
    )]
    Timeout {
        what: &'static str,
        timeout: Duration,
    },
    #[error("the connection closed before {0} completed")]
    Disconnected(&'static str),
    #[error("{0}")]
    Invalid(String),
    #[error("startup synchronization failed: {0:?}")]
    Sync(Vec<String>),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn kind(&self) -> ibkit_tws::ErrorKind {
        use ibkit_tws::ErrorKind;
        match self {
            Self::Tws(error) => error.kind(),
            Self::Request(_) => ErrorKind::Request,
            Self::Timeout { .. } => ErrorKind::Timeout,
            Self::Disconnected(_) | Self::Sync(_) => ErrorKind::Connection,
            Self::Invalid(_) => ErrorKind::InvalidArgument,
        }
    }
}

/// Which data to load while connecting (ib_async `StartupFetch`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartupFetch {
    pub positions: bool,
    pub orders_open: bool,
    pub orders_complete: bool,
    pub account_updates: bool,
    pub sub_account_updates: bool,
    pub executions: bool,
}

impl Default for StartupFetch {
    fn default() -> Self {
        Self {
            positions: true,
            orders_open: true,
            orders_complete: true,
            account_updates: true,
            sub_account_updates: true,
            executions: true,
        }
    }
}

/// Options for [`IB::connect_with`].
#[derive(Clone, Debug)]
pub struct IbOptions {
    pub connect: ConnectOptions,
    /// Do not request open and completed orders at startup.
    pub readonly: bool,
    /// Account to subscribe to account updates for; empty selects the only
    /// managed account when there is exactly one.
    pub account: String,
    /// Fail `connect` when a startup request times out.
    pub raise_sync_errors: bool,
    pub fetch: StartupFetch,
    /// Subscribe each sub-account when there are at most this many.
    pub max_synced_sub_accounts: usize,
    /// Fail requests whose server answer is an error (ib_async `RaiseRequestErrors`).
    pub raise_request_errors: bool,
    /// Time limit for each request; `None` waits forever (ib_async `RequestTimeout`).
    pub request_timeout: Option<Duration>,
    /// Broadcast every decoded message as [`Event::Message`].
    pub emit_messages: bool,
    pub defaults: Defaults,
    /// Capacity of the event broadcast channel.
    pub event_capacity: usize,
}

impl Default for IbOptions {
    fn default() -> Self {
        Self {
            connect: ConnectOptions::default(),
            readonly: false,
            account: String::new(),
            raise_sync_errors: false,
            fetch: StartupFetch::default(),
            max_synced_sub_accounts: 50,
            raise_request_errors: false,
            request_timeout: None,
            emit_messages: false,
            defaults: Defaults::default(),
            event_capacity: 65536,
        }
    }
}

struct Inner {
    client: Client,
    state: Mutex<State>,
    events: broadcast::Sender<Event>,
    /// Subscribers of [`IB::event_batches`].
    batches: Mutex<Vec<mpsc::UnboundedSender<Arc<[Event]>>>>,
    opts: IbOptions,
    /// Time the last message batch arrived, for [`IB::set_timeout`].
    last_recv: Mutex<tokio::time::Instant>,
    /// Bumped by each `set_timeout` call; an idle watch with an older value stops.
    timeout_gen: AtomicU64,
}

impl Inner {
    /// Broadcast `events` one by one, and send them as one batch to every
    /// [`IB::event_batches`] subscriber.
    fn emit(&self, events: Vec<Event>) {
        {
            let mut batches = self.batches.lock();
            if !batches.is_empty() {
                let batch: Arc<[Event]> = events.iter().cloned().collect();
                batches.retain(|tx| tx.send(batch.clone()).is_ok());
            }
        }
        for e in events {
            let _ = self.events.send(e);
        }
    }
}

/// Connected high-level client. Cheap to clone.
#[derive(Clone)]
pub struct IB {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for IB {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IB")
            .field("client", &self.inner.client)
            .finish()
    }
}

/// Account summary tags requested by [`IB::req_account_summary`].
pub const ACCOUNT_SUMMARY_TAGS: &str = "AccountType,NetLiquidation,TotalCashValue,SettledCash,AccruedCash,BuyingPower,\
EquityWithLoanValue,PreviousDayEquityWithLoanValue,GrossPositionValue,RegTEquity,RegTMargin,SMA,InitMarginReq,\
MaintMarginReq,AvailableFunds,ExcessLiquidity,Cushion,FullInitMarginReq,FullMaintMarginReq,FullAvailableFunds,\
FullExcessLiquidity,LookAheadNextChange,LookAheadInitMarginReq,LookAheadMaintMarginReq,LookAheadAvailableFunds,\
LookAheadExcessLiquidity,HighestSeverity,DayTradesRemaining,DayTradesRemainingT+1,DayTradesRemainingT+2,\
DayTradesRemainingT+3,DayTradesRemainingT+4,Leverage,$LEDGER:ALL";

/// The first reply message that matches `pattern`, mapped to `value`.
macro_rules! reply {
    ($items:expr, $pattern:pat => $value:expr) => {
        msgs($items).find_map(|m| match m {
            $pattern => Some($value),
            _ => None,
        })
    };
}

fn msgs(items: Vec<Collected>) -> impl Iterator<Item = Incoming> {
    items.into_iter().filter_map(|c| {
        if let Collected::Msg(m) = c {
            Some(m)
        } else {
            None
        }
    })
}

impl IB {
    /// Connect with default options and synchronize positions, orders,
    /// account values and executions (ib_async `IB.connect`).
    pub async fn connect(host: &str, port: u16, client_id: i32) -> Result<IB> {
        Self::connect_with(IbOptions {
            connect: ConnectOptions::new(host, port, client_id),
            ..Default::default()
        })
        .await
    }

    pub async fn connect_with(opts: IbOptions) -> Result<IB> {
        let (client, rx) = Client::connect(opts.connect.clone()).await?;
        let (events, _) = broadcast::channel(opts.event_capacity.max(16));
        let state = State::new(
            opts.connect.client_id,
            opts.defaults,
            opts.raise_request_errors,
        );
        let ib = IB {
            inner: Arc::new(Inner {
                client,
                state: Mutex::new(state),
                events,
                batches: Mutex::default(),
                opts,
                last_recv: Mutex::new(tokio::time::Instant::now()),
                timeout_gen: AtomicU64::new(0),
            }),
        };
        tokio::spawn(dispatcher(Arc::downgrade(&ib.inner), rx));
        if let Err(e) = ib.synchronize().await {
            ib.disconnect();
            return Err(e);
        }
        ib.inner.emit(vec![Event::Connected]);
        Ok(ib)
    }

    async fn synchronize(&self) -> Result<()> {
        let opts = self.inner.opts.clone();
        let client = &self.inner.client;
        if client.client_id() == 0 {
            self.req_auto_open_orders(true)?;
        }
        let accounts = client.managed_accounts();
        let account = if opts.account.is_empty() && accounts.len() == 1 {
            accounts[0].clone()
        } else {
            opts.account.clone()
        };
        let timeout = opts.connect.timeout;
        fn spawn(
            timeout: Duration,
            fut: impl std::future::Future<Output = Result<()>> + Send + 'static,
        ) -> tokio::task::JoinHandle<Result<()>> {
            tokio::spawn(async move {
                tokio::time::timeout(timeout, fut)
                    .await
                    .unwrap_or(Err(Error::Timeout {
                        what: "startup request",
                        timeout,
                    }))
            })
        }
        let mut tasks: Vec<(String, tokio::task::JoinHandle<Result<()>>)> = Vec::new();
        if opts.fetch.positions {
            let ib = self.clone();
            tasks.push((
                "positions".into(),
                spawn(timeout, async move { ib.req_positions().await.map(drop) }),
            ));
        }
        if !opts.readonly && opts.fetch.orders_open {
            let ib = self.clone();
            tasks.push((
                "open orders".into(),
                spawn(timeout, async move { ib.req_open_orders().await.map(drop) }),
            ));
        }
        if !opts.readonly && opts.fetch.orders_complete {
            let ib = self.clone();
            tasks.push((
                "completed orders".into(),
                spawn(timeout, async move {
                    ib.req_completed_orders(false).await.map(drop)
                }),
            ));
        }
        if !account.is_empty() && opts.fetch.account_updates {
            let (ib, a) = (self.clone(), account.clone());
            tasks.push((
                "account updates".into(),
                spawn(timeout, async move { ib.req_account_updates(&a).await }),
            ));
        }
        if accounts.len() <= opts.max_synced_sub_accounts && opts.fetch.sub_account_updates {
            for acc in &accounts {
                let (ib, a) = (self.clone(), acc.clone());
                tasks.push((
                    format!("account updates for {acc}"),
                    spawn(timeout, async move {
                        ib.req_account_updates_multi(&a, "").await
                    }),
                ));
            }
        }
        let mut errors = Vec::new();
        for (name, t) in tasks {
            match t.await {
                Ok(Ok(())) => {}
                Ok(Err(Error::Timeout { .. })) => errors.push(format!("{name} request timed out")),
                Ok(Err(e)) => errors.push(format!("{name} request failed: {e}")),
                Err(e) => errors.push(format!("{name} request panicked: {e}")),
            }
        }
        if opts.fetch.executions {
            match tokio::time::timeout(timeout, self.req_executions(&ExecutionFilter::default()))
                .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => errors.push(format!("executions request failed: {e}")),
                Err(_) => errors.push("executions request timed out".into()),
            }
        }
        for e in &errors {
            tracing::error!("{e}");
        }
        if opts.raise_sync_errors && !errors.is_empty() {
            return Err(Error::Sync(errors));
        }
        if !self.is_connected() {
            return Err(Error::Disconnected("startup synchronization"));
        }
        Ok(())
    }

    /// The low-level client, for requests without a high-level wrapper.
    pub fn client(&self) -> &Client {
        &self.inner.client
    }

    pub fn is_connected(&self) -> bool {
        self.inner.client.is_connected()
    }

    pub fn disconnect(&self) {
        self.inner.client.disconnect();
    }

    /// Subscribe to events. A receiver that falls more than
    /// `IbOptions::event_capacity` events behind loses the oldest ones.
    pub fn events(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    /// Every event without loss, one batch per network packet (or per local
    /// action such as `place_order`). Within a packet's batch, `Update`
    /// comes after the other events and before `PendingTickers`, as in
    /// ib_async. The queue has no limit: read it continuously (language
    /// bindings do), or drop the receiver.
    pub fn event_batches(&self) -> mpsc::UnboundedReceiver<Arc<[Event]>> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.inner.batches.lock().push(tx);
        rx
    }

    /// Wait for the next network packet to be processed (ib_async
    /// `waitOnUpdate`). Returns false on timeout.
    pub async fn wait_on_update(&self, timeout: Option<Duration>) -> bool {
        let mut rx = self.events();
        let wait = async {
            loop {
                match rx.recv().await {
                    Ok(Event::Update) => return true,
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => return false,
                }
            }
        };
        match timeout {
            Some(t) => tokio::time::timeout(t, wait).await.unwrap_or(false),
            None => wait.await,
        }
    }

    /// Emit [`Event::Timeout`] with the idle seconds once no message has
    /// arrived for `timeout` (ib_async `setTimeout`). The watch then stops;
    /// call again to re-arm. `None` or zero stops the current watch.
    pub fn set_timeout(&self, timeout: Option<Duration>) {
        let generation = self.inner.timeout_gen.fetch_add(1, Ordering::SeqCst) + 1;
        *self.inner.last_recv.lock() = tokio::time::Instant::now();
        let Some(limit) = timeout.filter(|t| !t.is_zero()) else {
            return;
        };
        let weak = Arc::downgrade(&self.inner);
        tokio::spawn(async move {
            let mut delay = limit;
            loop {
                tokio::time::sleep(delay).await;
                let Some(inner) = weak.upgrade() else { return };
                if inner.timeout_gen.load(Ordering::SeqCst) != generation
                    || !inner.client.is_connected()
                {
                    return;
                }
                let idle = inner.last_recv.lock().elapsed();
                if idle >= limit {
                    inner.timeout_gen.fetch_add(1, Ordering::SeqCst);
                    inner.emit(vec![Event::Timeout(idle.as_secs_f64())]);
                    return;
                }
                delay = limit - idle;
            }
        });
    }

    pub fn connection_stats(&self) -> ConnectionStats {
        self.inner.client.connection_stats()
    }

    fn state(&self) -> parking_lot::MutexGuard<'_, State> {
        self.inner.state.lock()
    }

    async fn request(
        &self,
        what: &'static str,
        key: ReqKey,
        contract: Option<&Contract>,
        send: impl FnOnce(&Client) -> std::result::Result<(), ibkit_tws::Error>,
    ) -> Result<Vec<Collected>> {
        let rx = {
            let mut state = self.state();
            // A request without a reqId has one reply slot: a second waiter
            // would replace the first one, which would then fail.
            if !matches!(key, ReqKey::Id(_)) && state.pending.contains_key(&key) {
                return Err(Error::Invalid(format!(
                    "{what} is already running; await that call, or call it once and share its result"
                )));
            }
            state.start_req(key.clone(), contract)
        };
        // Every early exit (send error, timeout, dropped future) removes the
        // pending entry through the guard; only a received reply disarms it.
        let mut guard = PendingGuard {
            ib: self,
            keys: vec![key],
        };
        send(&self.inner.client)?;
        let reply = match self.inner.opts.request_timeout {
            Some(t) => tokio::time::timeout(t, rx)
                .await
                .map_err(|_| Error::Timeout { what, timeout: t })?,
            None => rx.await,
        };
        guard.keys.clear();
        match reply {
            Ok(Ok(items)) => Ok(items),
            Ok(Err(e)) => Err(e.into()),
            Err(_) => Err(Error::Disconnected(what)),
        }
    }

    // ---- state accessors ----------------------------------------------

    pub fn managed_accounts(&self) -> Vec<String> {
        self.inner.client.managed_accounts()
    }

    pub fn account_values(&self, account: &str) -> Vec<AccountValue> {
        self.state()
            .account_values
            .values()
            .filter(|v| account.is_empty() || v.account == account)
            .cloned()
            .collect()
    }

    pub fn account_summary(&self, account: &str) -> Vec<AccountValue> {
        self.state()
            .acct_summary
            .values()
            .filter(|v| account.is_empty() || v.account == account)
            .cloned()
            .collect()
    }

    pub fn portfolio(&self, account: &str) -> Vec<PortfolioItem> {
        let s = self.state();
        s.portfolio
            .iter()
            .filter(|(a, _)| account.is_empty() || *a == account)
            .flat_map(|(_, m)| m.values().cloned())
            .collect()
    }

    pub fn positions(&self, account: &str) -> Vec<Position> {
        let s = self.state();
        s.positions
            .iter()
            .filter(|(a, _)| account.is_empty() || *a == account)
            .flat_map(|(_, m)| m.values().cloned())
            .collect()
    }

    pub fn pnl(&self, account: &str, model_code: &str) -> Vec<PnL> {
        self.state()
            .pnls
            .values()
            .map(|h| h.read().clone())
            .filter(|p| {
                (account.is_empty() || p.account == account)
                    && (model_code.is_empty() || p.model_code == model_code)
            })
            .collect()
    }

    pub fn pnl_single(&self, account: &str, model_code: &str, con_id: i32) -> Vec<PnLSingle> {
        self.state()
            .pnl_singles
            .values()
            .map(|h| h.read().clone())
            .filter(|p| {
                (account.is_empty() || p.account == account)
                    && (model_code.is_empty() || p.model_code == model_code)
                    && (con_id == 0 || p.con_id == con_id)
            })
            .collect()
    }

    pub fn trades(&self) -> Vec<TradeHandle> {
        let s = self.state();
        s.trade_order
            .iter()
            .filter_map(|k| s.trades.get(k).cloned())
            .collect()
    }

    pub fn open_trades(&self) -> Vec<TradeHandle> {
        self.trades()
            .into_iter()
            .filter(|t| !t.lock().is_done())
            .collect()
    }

    pub fn orders(&self) -> Vec<Order> {
        self.trades()
            .iter()
            .map(|t| t.lock().order.clone())
            .collect()
    }

    pub fn open_orders(&self) -> Vec<Order> {
        self.open_trades()
            .iter()
            .map(|t| t.lock().order.clone())
            .collect()
    }

    pub fn fills(&self) -> Vec<Fill> {
        let s = self.state();
        s.fill_order
            .iter()
            .filter_map(|k| s.fills.get(k).cloned())
            .collect()
    }

    pub fn executions(&self) -> Vec<Execution> {
        self.fills().into_iter().map(|f| f.execution).collect()
    }

    pub fn ticker(&self, contract: &Contract) -> Option<TickerHandle> {
        self.state().tickers.get(&contract.key()).cloned()
    }

    pub fn tickers(&self) -> Vec<TickerHandle> {
        self.state().tickers.values().cloned().collect()
    }

    pub fn news_ticks(&self) -> Vec<NewsTick> {
        self.state().news_ticks.clone()
    }

    pub fn news_bulletins(&self) -> Vec<NewsBulletin> {
        self.state().news_bulletins.values().cloned().collect()
    }

    pub fn realtime_bars(&self) -> Vec<RealTimeBarsHandle> {
        self.state()
            .subscribers
            .values()
            .filter_map(|s| {
                if let Subscriber::RealTimeBars(h) = s {
                    Some(h.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    // ---- orders --------------------------------------------------------

    /// Place a new order, or modify it when a trade with its order id exists.
    pub fn place_order(&self, contract: &Contract, mut order: Order) -> Result<TradeHandle> {
        let client = &self.inner.client;
        let order_id = if order.order_id != 0 {
            order.order_id
        } else {
            client.next_req_id()
        };
        // Hold the state lock while sending, so the dispatcher cannot apply the
        // server's answer before the trade is recorded here.
        let mut s = self.state();
        let key = order_key(s.client_id, order_id, order.perm_id);
        let existing = s.trades.get(&key).cloned();
        if let Some(tr) = &existing {
            let tr = tr.lock();
            if tr.is_done() {
                return Err(Error::Invalid(format!(
                    "order {order_id} is already {}; a done order cannot be modified, place a new order instead",
                    tr.order_status.status
                )));
            }
        }
        client.place_order(order_id, contract, &order)?;
        let t = now();
        if let Some(trade) = existing {
            {
                let mut tr = trade.lock();
                let st = tr.order_status.status.clone();
                tr.log.push(TradeLogEntry {
                    time: t,
                    status: st,
                    message: "Modify".into(),
                    error_code: 0,
                });
                tr.order = Order {
                    order_id,
                    client_id: tr.order.client_id,
                    perm_id: tr.order.perm_id,
                    ..order
                };
            }
            s.events.push(Event::OrderModify(trade.clone()));
            let events = std::mem::take(&mut s.events);
            drop(s);
            self.inner.emit(events);
            Ok(trade)
        } else {
            order.client_id = s.client_id;
            order.order_id = order_id;
            let trade = Arc::new(Mutex::new(Trade {
                contract: contract.clone(),
                order,
                order_status: OrderStatus {
                    order_id,
                    status: status::PENDING_SUBMIT.into(),
                    ..Default::default()
                },
                fills: Vec::new(),
                log: vec![TradeLogEntry {
                    time: t,
                    status: status::PENDING_SUBMIT.into(),
                    ..Default::default()
                }],
                advanced_error: String::new(),
            }));
            s.add_trade(key, trade.clone());
            drop(s);
            self.inner.emit(vec![Event::NewOrder(trade.clone())]);
            Ok(trade)
        }
    }

    /// Cancel an order (ib_async `cancelOrder`).
    pub fn cancel_order(
        &self,
        order: &Order,
        manual_cancel_order_time: &str,
    ) -> Result<Option<TradeHandle>> {
        // Hold the state lock while sending (see `place_order`).
        let s = self.state();
        self.inner
            .client
            .cancel_order_simple(order.order_id, manual_cancel_order_time)?;
        let key = order_key(order.client_id, order.order_id, order.perm_id);
        let Some(trade) = s.trades.get(&key).cloned() else {
            tracing::error!(order_id = order.order_id, "cancelOrder: unknown orderId");
            return Ok(None);
        };
        let mut events = Vec::new();
        {
            let mut t = trade.lock();
            if !t.is_done() {
                let st = t.order_status.status.as_str();
                let new_status = if (st == status::PENDING_SUBMIT && !order.transmit)
                    || st == status::INACTIVE
                {
                    status::CANCELLED
                } else {
                    status::PENDING_CANCEL
                };
                t.log.push(TradeLogEntry {
                    time: now(),
                    status: new_status.into(),
                    ..Default::default()
                });
                t.order_status.status = new_status.into();
                events.push(Event::CancelOrder(trade.clone()));
                events.push(Event::OrderStatus(trade.clone()));
                if new_status == status::CANCELLED {
                    events.push(Event::Cancelled(trade.clone()));
                }
            }
        }
        drop(s);
        self.inner.emit(events);
        Ok(Some(trade))
    }

    /// Cancel all open orders of all clients.
    pub fn req_global_cancel(&self) -> Result<()> {
        Ok(self
            .inner
            .client
            .req_global_cancel(&OrderCancel::default())?)
    }

    /// Create a bracket order with consecutive order ids (ib_async `bracketOrder`).
    pub fn bracket_order(
        &self,
        action: &str,
        quantity: f64,
        limit_price: f64,
        take_profit_price: f64,
        stop_loss_price: f64,
    ) -> Result<BracketOrder> {
        let reverse = match action {
            "BUY" => "SELL",
            "SELL" => "BUY",
            other => {
                return Err(Error::Invalid(format!(
                    "bracket order action is {other:?}; use \"BUY\" or \"SELL\""
                )));
            }
        };
        let c = &self.inner.client;
        let parent = Order {
            order_id: c.next_req_id(),
            transmit: false,
            ..Order::limit(action, quantity, limit_price)
        };
        let take_profit = Order {
            order_id: c.next_req_id(),
            transmit: false,
            parent_id: parent.order_id,
            ..Order::limit(reverse, quantity, take_profit_price)
        };
        let stop_loss = Order {
            order_id: c.next_req_id(),
            transmit: true,
            parent_id: parent.order_id,
            ..Order::stop(reverse, quantity, stop_loss_price)
        };
        Ok(BracketOrder {
            parent,
            take_profit,
            stop_loss,
        })
    }

    /// Margin and commission impact of an order without placing it.
    pub async fn what_if_order(&self, contract: &Contract, order: &Order) -> Result<OrderState> {
        let mut o = order.clone();
        o.what_if = true;
        let req_id = self.inner.client.next_req_id();
        o.order_id = req_id;
        let items = self
            .request("whatIfOrder", ReqKey::Id(req_id), Some(contract), |c| {
                c.place_order(req_id, contract, &o)
            })
            .await?;
        Ok(reply!(items, Incoming::OpenOrder { state, .. } => *state).unwrap_or_default())
    }

    pub async fn req_open_orders(&self) -> Result<Vec<TradeHandle>> {
        let items = self
            .request("reqOpenOrders", ReqKey::OpenOrders, None, |c| {
                c.req_open_orders()
            })
            .await?;
        Ok(trades_of(items))
    }

    pub async fn req_all_open_orders(&self) -> Result<Vec<TradeHandle>> {
        let items = self
            .request("reqAllOpenOrders", ReqKey::OpenOrders, None, |c| {
                c.req_all_open_orders()
            })
            .await?;
        Ok(trades_of(items))
    }

    pub async fn req_completed_orders(&self, api_only: bool) -> Result<Vec<TradeHandle>> {
        let items = self
            .request("reqCompletedOrders", ReqKey::CompletedOrders, None, |c| {
                c.req_completed_orders(api_only)
            })
            .await?;
        Ok(trades_of(items))
    }

    pub fn req_auto_open_orders(&self, auto_bind: bool) -> Result<()> {
        Ok(self.inner.client.req_auto_open_orders(auto_bind)?)
    }

    pub async fn req_executions(&self, filter: &ExecutionFilter) -> Result<Vec<Fill>> {
        let req_id = self.inner.client.next_req_id();
        let items = self
            .request("reqExecutions", ReqKey::Id(req_id), None, |c| {
                c.req_executions(req_id, filter)
            })
            .await?;
        Ok(items
            .into_iter()
            .filter_map(|c| {
                if let Collected::Fill(f) = c {
                    Some(*f)
                } else {
                    None
                }
            })
            .collect())
    }

    // ---- account -------------------------------------------------------

    pub async fn req_positions(&self) -> Result<Vec<Position>> {
        let items = self
            .request("reqPositions", ReqKey::Positions, None, |c| {
                c.req_positions()
            })
            .await?;
        Ok(items
            .into_iter()
            .filter_map(|c| {
                if let Collected::Position(p) = c {
                    Some(*p)
                } else {
                    None
                }
            })
            .collect())
    }

    /// Subscribe to account values and portfolio of `account` and wait for
    /// the first full download.
    pub async fn req_account_updates(&self, account: &str) -> Result<()> {
        self.request("reqAccountUpdates", ReqKey::AccountValues, None, |c| {
            c.req_account_updates(true, account)
        })
        .await?;
        Ok(())
    }

    pub async fn req_account_updates_multi(&self, account: &str, model_code: &str) -> Result<()> {
        let req_id = self.inner.client.next_req_id();
        self.request("reqAccountUpdatesMulti", ReqKey::Id(req_id), None, |c| {
            c.req_account_updates_multi(req_id, account, model_code, false)
        })
        .await?;
        Ok(())
    }

    /// Request the account summary of all accounts and wait for it.
    pub async fn req_account_summary(&self) -> Result<Vec<AccountValue>> {
        let req_id = self.inner.client.next_req_id();
        self.request("reqAccountSummary", ReqKey::Id(req_id), None, |c| {
            c.req_account_summary(req_id, "All", ACCOUNT_SUMMARY_TAGS)
        })
        .await?;
        Ok(self.account_summary(""))
    }

    /// Start live P&L updates for an account; returns the live object.
    pub fn req_pnl(&self, account: &str, model_code: &str) -> Result<PnLHandle> {
        let key = (account.to_string(), model_code.to_string());
        let mut s = self.state();
        if s.pnl_keys.contains_key(&key) {
            return Err(Error::Invalid(format!(
                "P&L for account {account:?} and model {model_code:?} is already subscribed; call cancel_pnl first"
            )));
        }
        let req_id = self.inner.client.next_req_id();
        let h = Arc::new(RwLock::new(PnL::new(account, model_code)));
        s.pnl_keys.insert(key, req_id);
        s.pnls.insert(req_id, h.clone());
        drop(s);
        self.inner.client.req_pnl(req_id, account, model_code)?;
        Ok(h)
    }

    pub fn cancel_pnl(&self, account: &str, model_code: &str) -> Result<()> {
        let mut s = self.state();
        match s
            .pnl_keys
            .remove(&(account.to_string(), model_code.to_string()))
        {
            Some(req_id) => {
                s.pnls.remove(&req_id);
                drop(s);
                Ok(self.inner.client.cancel_pnl(req_id)?)
            }
            None => Err(Error::Invalid(format!(
                "no P&L subscription for account {account:?} and model {model_code:?}"
            ))),
        }
    }

    pub fn req_pnl_single(
        &self,
        account: &str,
        model_code: &str,
        con_id: i32,
    ) -> Result<PnLSingleHandle> {
        let key = (account.to_string(), model_code.to_string(), con_id);
        let mut s = self.state();
        if s.pnl_single_keys.contains_key(&key) {
            return Err(Error::Invalid(format!(
                "P&L for conId {con_id} is already subscribed; call cancel_pnl_single first"
            )));
        }
        let req_id = self.inner.client.next_req_id();
        let h = Arc::new(RwLock::new(PnLSingle::new(account, model_code, con_id)));
        s.pnl_single_keys.insert(key, req_id);
        s.pnl_singles.insert(req_id, h.clone());
        drop(s);
        self.inner
            .client
            .req_pnl_single(req_id, account, model_code, con_id)?;
        Ok(h)
    }

    pub fn cancel_pnl_single(&self, account: &str, model_code: &str, con_id: i32) -> Result<()> {
        let mut s = self.state();
        match s
            .pnl_single_keys
            .remove(&(account.to_string(), model_code.to_string(), con_id))
        {
            Some(req_id) => {
                s.pnl_singles.remove(&req_id);
                drop(s);
                Ok(self.inner.client.cancel_pnl_single(req_id)?)
            }
            None => Err(Error::Invalid(format!(
                "no P&L subscription for conId {con_id}"
            ))),
        }
    }

    // ---- contracts -----------------------------------------------------

    pub async fn req_contract_details(&self, contract: &Contract) -> Result<Vec<ContractDetails>> {
        let req_id = self.inner.client.next_req_id();
        let items = self
            .request(
                "reqContractDetails",
                ReqKey::Id(req_id),
                Some(contract),
                |c| c.req_contract_details(req_id, contract),
            )
            .await?;
        Ok(msgs(items)
            .filter_map(|m| match m {
                Incoming::ContractDetails { details, .. }
                | Incoming::BondContractDetails { details, .. } => Some(*details),
                _ => None,
            })
            .collect())
    }

    /// Fill in missing contract fields from contract details. Each result is
    /// `None` when the contract is unknown or ambiguous (ib_async
    /// `qualifyContracts`).
    pub async fn qualify_contracts(&self, contracts: &[Contract]) -> Result<Vec<Option<Contract>>> {
        let mut handles = Vec::new();
        for c in contracts {
            let ib = self.clone();
            let c = c.clone();
            handles.push(tokio::spawn(
                async move { ib.req_contract_details(&c).await },
            ));
        }
        let mut out = Vec::with_capacity(contracts.len());
        for (contract, h) in contracts.iter().zip(handles) {
            let details = h.await.map_err(|e| Error::Invalid(e.to_string()))??;
            let candidates: Vec<&ContractDetails> =
                if details.len() > 1 && !contract.sec_type.is_empty() {
                    details
                        .iter()
                        .filter(|d| d.contract.sec_type == contract.sec_type)
                        .collect()
                } else {
                    details.iter().collect()
                };
            if candidates.len() == 1 {
                let mut c = candidates[0].contract.clone();
                if contract.exchange == "SMART" {
                    c.exchange = contract.exchange.clone();
                }
                out.push(Some(c));
            } else {
                if candidates.is_empty() {
                    tracing::warn!(?contract, "unknown contract");
                } else {
                    tracing::warn!(?contract, count = candidates.len(), "ambiguous contract");
                }
                out.push(None);
            }
        }
        Ok(out)
    }

    pub async fn req_matching_symbols(&self, pattern: &str) -> Result<Vec<ContractDescription>> {
        let req_id = self.inner.client.next_req_id();
        let items = self
            .request("reqMatchingSymbols", ReqKey::Id(req_id), None, |c| {
                c.req_matching_symbols(req_id, pattern)
            })
            .await?;
        Ok(
            reply!(items, Incoming::SymbolSamples { descriptions, .. } => descriptions)
                .unwrap_or_default(),
        )
    }

    pub async fn req_market_rule(&self, market_rule_id: i32) -> Result<Vec<PriceIncrement>> {
        let items = self
            .request(
                "reqMarketRule",
                ReqKey::MarketRule(market_rule_id),
                None,
                |c| c.req_market_rule(market_rule_id),
            )
            .await?;
        Ok(
            reply!(items, Incoming::MarketRule { increments, .. } => increments)
                .unwrap_or_default(),
        )
    }

    pub async fn req_sec_def_opt_params(
        &self,
        underlying_symbol: &str,
        fut_fop_exchange: &str,
        underlying_sec_type: &str,
        underlying_con_id: i32,
    ) -> Result<Vec<OptionChain>> {
        let req_id = self.inner.client.next_req_id();
        let items = self
            .request("reqSecDefOptParams", ReqKey::Id(req_id), None, |c| {
                c.req_sec_def_opt_params(
                    req_id,
                    underlying_symbol,
                    fut_fop_exchange,
                    underlying_sec_type,
                    underlying_con_id,
                )
            })
            .await?;
        Ok(msgs(items)
            .filter_map(|m| match m {
                Incoming::SecurityDefinitionOptionParameter(p) => Some(OptionChain {
                    exchange: p.exchange,
                    underlying_con_id: p.underlying_con_id,
                    trading_class: p.trading_class,
                    multiplier: p.multiplier,
                    expirations: p.expirations,
                    strikes: p.strikes,
                }),
                _ => None,
            })
            .collect())
    }

    // ---- market data ---------------------------------------------------

    /// 1 live, 2 frozen, 3 delayed, 4 delayed frozen.
    pub fn req_market_data_type(&self, market_data_type: i32) -> Result<()> {
        Ok(self.inner.client.req_market_data_type(market_data_type)?)
    }

    /// Subscribe to streaming market data; returns the live ticker.
    pub fn req_mkt_data(
        &self,
        contract: &Contract,
        generic_tick_list: &str,
        snapshot: bool,
        regulatory_snapshot: bool,
        options: &[TagValue],
    ) -> Result<TickerHandle> {
        let req_id = self.inner.client.next_req_id();
        let ticker = self.state().start_ticker(req_id, contract, "mktData");
        self.inner.client.req_mkt_data(
            req_id,
            contract,
            generic_tick_list,
            snapshot,
            regulatory_snapshot,
            options,
        )?;
        Ok(ticker)
    }

    pub fn cancel_mkt_data(&self, contract: &Contract) -> Result<bool> {
        let req_id = self.state().end_ticker(contract, "mktData");
        match req_id {
            Some(id) => {
                self.inner.client.cancel_mkt_data(id)?;
                Ok(true)
            }
            None => {
                tracing::error!(?contract, "cancelMktData: no reqId found");
                Ok(false)
            }
        }
    }

    /// Snapshot market data for several contracts (ib_async `reqTickers`).
    pub async fn req_tickers(
        &self,
        contracts: &[Contract],
        regulatory_snapshot: bool,
    ) -> Result<Vec<TickerHandle>> {
        let mut waits = Vec::new();
        let mut tickers = Vec::new();
        // Removes every snapshot that has not ended, also on an early exit.
        let mut guard = PendingGuard {
            ib: self,
            keys: Vec::new(),
        };
        for c in contracts {
            let req_id = self.inner.client.next_req_id();
            let (rx, ticker) = {
                let mut s = self.state();
                let rx = s.start_req(ReqKey::Id(req_id), Some(c));
                (rx, s.ticker_for(req_id, c))
            };
            guard.keys.push(ReqKey::Id(req_id));
            tickers.push(ticker);
            self.inner
                .client
                .req_mkt_data(req_id, c, "", true, regulatory_snapshot, &[])?;
            waits.push(rx);
        }
        for rx in waits {
            match self.inner.opts.request_timeout {
                Some(t) => {
                    let _ = tokio::time::timeout(t, rx).await;
                }
                None => {
                    let _ = rx.await;
                }
            }
        }
        Ok(tickers)
    }

    /// `tick_type`: "Last", "AllLast", "BidAsk" or "MidPoint".
    pub fn req_tick_by_tick_data(
        &self,
        contract: &Contract,
        tick_type: &str,
        number_of_ticks: i32,
        ignore_size: bool,
    ) -> Result<TickerHandle> {
        let req_id = self.inner.client.next_req_id();
        let ticker = self.state().start_ticker(req_id, contract, tick_type);
        self.inner.client.req_tick_by_tick_data(
            req_id,
            contract,
            tick_type,
            number_of_ticks,
            ignore_size,
        )?;
        Ok(ticker)
    }

    pub fn cancel_tick_by_tick_data(&self, contract: &Contract, tick_type: &str) -> Result<bool> {
        match self.state().end_ticker(contract, tick_type) {
            Some(id) => {
                self.inner.client.cancel_tick_by_tick_data(id)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn req_mkt_depth(
        &self,
        contract: &Contract,
        num_rows: i32,
        is_smart_depth: bool,
        options: &[TagValue],
    ) -> Result<TickerHandle> {
        let req_id = self.inner.client.next_req_id();
        let ticker = self.state().start_ticker(req_id, contract, "mktDepth");
        {
            let mut t = ticker.write();
            t.dom_bids.clear();
            t.dom_asks.clear();
            t.dom_bids_dict.clear();
            t.dom_asks_dict.clear();
        }
        self.inner
            .client
            .req_mkt_depth(req_id, contract, num_rows, is_smart_depth, options)?;
        Ok(ticker)
    }

    pub fn cancel_mkt_depth(&self, contract: &Contract, is_smart_depth: bool) -> Result<bool> {
        match self.state().end_ticker(contract, "mktDepth") {
            Some(id) => {
                self.inner.client.cancel_mkt_depth(id, is_smart_depth)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub async fn req_mkt_depth_exchanges(&self) -> Result<Vec<DepthMktDataDescription>> {
        let items = self
            .request(
                "reqMktDepthExchanges",
                ReqKey::MktDepthExchanges,
                None,
                |c| c.req_mkt_depth_exchanges(),
            )
            .await?;
        Ok(
            reply!(items, Incoming::MktDepthExchanges { descriptions } => descriptions)
                .unwrap_or_default(),
        )
    }

    pub async fn req_smart_components(&self, bbo_exchange: &str) -> Result<Vec<SmartComponent>> {
        let req_id = self.inner.client.next_req_id();
        let items = self
            .request("reqSmartComponents", ReqKey::Id(req_id), None, |c| {
                c.req_smart_components(req_id, bbo_exchange)
            })
            .await?;
        Ok(
            reply!(items, Incoming::SmartComponents { components, .. } => components)
                .unwrap_or_default(),
        )
    }

    // ---- historical data -----------------------------------------------

    /// Historical bars. With `keep_up_to_date` the returned list keeps
    /// receiving updates until [`IB::cancel_historical_data`].
    #[allow(clippy::too_many_arguments)]
    pub async fn req_historical_data(
        &self,
        contract: &Contract,
        end_date_time: &str,
        duration_str: &str,
        bar_size_setting: &str,
        what_to_show: &str,
        use_rth: bool,
        format_date: i32,
        keep_up_to_date: bool,
        chart_options: &[TagValue],
    ) -> Result<BarsHandle> {
        let req_id = self.inner.client.next_req_id();
        let bars = Arc::new(RwLock::new(BarDataList {
            req_id,
            contract: contract.clone(),
            end_date_time: end_date_time.into(),
            duration_str: duration_str.into(),
            bar_size_setting: bar_size_setting.into(),
            what_to_show: what_to_show.into(),
            use_rth,
            format_date,
            keep_up_to_date,
            chart_options: chart_options.to_vec(),
            bars: Vec::new(),
        }));
        self.state()
            .subscribers
            .insert(req_id, Subscriber::Bars(bars.clone()));
        let r = self
            .request(
                "reqHistoricalData",
                ReqKey::Id(req_id),
                Some(contract),
                |c| {
                    c.req_historical_data(
                        req_id,
                        contract,
                        end_date_time,
                        duration_str,
                        bar_size_setting,
                        what_to_show,
                        use_rth,
                        format_date,
                        keep_up_to_date,
                        chart_options,
                    )
                },
            )
            .await;
        if let Err(e) = r {
            self.state().subscribers.remove(&req_id);
            if matches!(e, Error::Timeout { .. }) {
                let _ = self.inner.client.cancel_historical_data(req_id);
            }
            return Err(e);
        }
        Ok(bars)
    }

    pub fn cancel_historical_data(&self, bars: &BarsHandle) -> Result<()> {
        let req_id = bars.read().req_id;
        self.state().subscribers.remove(&req_id);
        Ok(self.inner.client.cancel_historical_data(req_id)?)
    }

    pub async fn req_historical_schedule(
        &self,
        contract: &Contract,
        num_days: i32,
        end_date_time: &str,
        use_rth: bool,
    ) -> Result<HistoricalSchedule> {
        let req_id = self.inner.client.next_req_id();
        let duration = format!("{num_days} D");
        let items = self
            .request(
                "reqHistoricalSchedule",
                ReqKey::Id(req_id),
                Some(contract),
                |c| {
                    c.req_historical_data(
                        req_id,
                        contract,
                        end_date_time,
                        &duration,
                        "1 day",
                        "SCHEDULE",
                        use_rth,
                        1,
                        false,
                        &[],
                    )
                },
            )
            .await?;
        Ok(msgs(items)
            .find_map(|m| match m {
                Incoming::HistoricalSchedule {
                    start_date_time,
                    end_date_time,
                    time_zone,
                    sessions,
                    ..
                } => Some(HistoricalSchedule {
                    start_date_time,
                    end_date_time,
                    time_zone,
                    sessions,
                }),
                _ => None,
            })
            .unwrap_or_default())
    }

    /// Historical ticks. `what_to_show`: "TRADES", "MIDPOINT" or "BID_ASK".
    #[allow(clippy::too_many_arguments)]
    pub async fn req_historical_ticks(
        &self,
        contract: &Contract,
        start_date_time: &str,
        end_date_time: &str,
        number_of_ticks: i32,
        what_to_show: &str,
        use_rth: bool,
        ignore_size: bool,
        misc_options: &[TagValue],
    ) -> Result<Vec<Incoming>> {
        let req_id = self.inner.client.next_req_id();
        let items = self
            .request(
                "reqHistoricalTicks",
                ReqKey::Id(req_id),
                Some(contract),
                |c| {
                    c.req_historical_ticks(
                        req_id,
                        contract,
                        start_date_time,
                        end_date_time,
                        number_of_ticks,
                        what_to_show,
                        use_rth,
                        ignore_size,
                        misc_options,
                    )
                },
            )
            .await?;
        Ok(msgs(items).collect())
    }

    pub async fn req_head_time_stamp(
        &self,
        contract: &Contract,
        what_to_show: &str,
        use_rth: bool,
        format_date: i32,
    ) -> Result<String> {
        let req_id = self.inner.client.next_req_id();
        let items = self
            .request(
                "reqHeadTimeStamp",
                ReqKey::Id(req_id),
                Some(contract),
                |c| c.req_head_time_stamp(req_id, contract, what_to_show, use_rth, format_date),
            )
            .await?;
        Ok(
            reply!(items, Incoming::HeadTimestamp { head_timestamp, .. } => head_timestamp)
                .unwrap_or_default(),
        )
    }

    pub async fn req_histogram_data(
        &self,
        contract: &Contract,
        use_rth: bool,
        period: &str,
    ) -> Result<Vec<HistogramData>> {
        let req_id = self.inner.client.next_req_id();
        let items = self
            .request(
                "reqHistogramData",
                ReqKey::Id(req_id),
                Some(contract),
                |c| c.req_histogram_data(req_id, contract, use_rth, period),
            )
            .await?;
        Ok(reply!(items, Incoming::HistogramData { items, .. } => items).unwrap_or_default())
    }

    /// Five-second real time bars; returns the live list.
    pub fn req_real_time_bars(
        &self,
        contract: &Contract,
        bar_size: i32,
        what_to_show: &str,
        use_rth: bool,
        options: &[TagValue],
    ) -> Result<RealTimeBarsHandle> {
        let req_id = self.inner.client.next_req_id();
        let bars = Arc::new(RwLock::new(RealTimeBarList {
            req_id,
            contract: contract.clone(),
            bar_size,
            what_to_show: what_to_show.into(),
            use_rth,
            real_time_bars_options: options.to_vec(),
            bars: Vec::new(),
        }));
        {
            let mut s = self.state();
            s.subscribers
                .insert(req_id, Subscriber::RealTimeBars(bars.clone()));
            s.req_id_to_contract.insert(req_id, contract.clone());
        }
        self.inner.client.req_real_time_bars(
            req_id,
            contract,
            bar_size,
            what_to_show,
            use_rth,
            options,
        )?;
        Ok(bars)
    }

    pub fn cancel_real_time_bars(&self, bars: &RealTimeBarsHandle) -> Result<()> {
        let req_id = bars.read().req_id;
        let mut s = self.state();
        s.subscribers.remove(&req_id);
        s.req_id_to_contract.remove(&req_id);
        drop(s);
        Ok(self.inner.client.cancel_real_time_bars(req_id)?)
    }

    // ---- scanners ------------------------------------------------------

    pub async fn req_scanner_data(
        &self,
        subscription: &ScannerSubscription,
        options: &[TagValue],
        filter_options: &[TagValue],
    ) -> Result<Vec<ScanData>> {
        let req_id = self.inner.client.next_req_id();
        let list = Arc::new(RwLock::new(ScanDataList {
            req_id,
            subscription: subscription.clone(),
            rows: Vec::new(),
        }));
        self.state()
            .subscribers
            .insert(req_id, Subscriber::Scan(list.clone()));
        let r = self
            .request("reqScannerData", ReqKey::Id(req_id), None, |c| {
                c.req_scanner_subscription(req_id, subscription, options, filter_options)
            })
            .await;
        self.cancel_scanner_subscription(&list)?;
        r?;
        let rows = list.read().rows.clone();
        Ok(rows)
    }

    pub fn req_scanner_subscription(
        &self,
        subscription: &ScannerSubscription,
        options: &[TagValue],
        filter_options: &[TagValue],
    ) -> Result<ScanHandle> {
        let req_id = self.inner.client.next_req_id();
        let list = Arc::new(RwLock::new(ScanDataList {
            req_id,
            subscription: subscription.clone(),
            rows: Vec::new(),
        }));
        self.state()
            .subscribers
            .insert(req_id, Subscriber::Scan(list.clone()));
        self.inner.client.req_scanner_subscription(
            req_id,
            subscription,
            options,
            filter_options,
        )?;
        Ok(list)
    }

    pub fn cancel_scanner_subscription(&self, list: &ScanHandle) -> Result<()> {
        let req_id = list.read().req_id;
        self.state().subscribers.remove(&req_id);
        Ok(self.inner.client.cancel_scanner_subscription(req_id)?)
    }

    pub async fn req_scanner_parameters(&self) -> Result<String> {
        let items = self
            .request("reqScannerParameters", ReqKey::ScannerParams, None, |c| {
                c.req_scanner_parameters()
            })
            .await?;
        Ok(reply!(items, Incoming::ScannerParameters { xml } => xml).unwrap_or_default())
    }

    // ---- options -------------------------------------------------------

    pub async fn calculate_implied_volatility(
        &self,
        contract: &Contract,
        option_price: f64,
        under_price: f64,
        options: &[TagValue],
    ) -> Result<Option<OptionComputation>> {
        let req_id = self.inner.client.next_req_id();
        let r = self
            .request(
                "calculateImpliedVolatility",
                ReqKey::Id(req_id),
                Some(contract),
                |c| {
                    c.calculate_implied_volatility(
                        req_id,
                        contract,
                        option_price,
                        under_price,
                        options,
                    )
                },
            )
            .await;
        let _ = self
            .inner
            .client
            .cancel_calculate_implied_volatility(req_id);
        Ok(reply!(r?, Incoming::TickOptionComputation { computation, .. } => computation))
    }

    pub async fn calculate_option_price(
        &self,
        contract: &Contract,
        volatility: f64,
        under_price: f64,
        options: &[TagValue],
    ) -> Result<Option<OptionComputation>> {
        let req_id = self.inner.client.next_req_id();
        let r = self
            .request(
                "calculateOptionPrice",
                ReqKey::Id(req_id),
                Some(contract),
                |c| c.calculate_option_price(req_id, contract, volatility, under_price, options),
            )
            .await;
        let _ = self.inner.client.cancel_calculate_option_price(req_id);
        Ok(reply!(r?, Incoming::TickOptionComputation { computation, .. } => computation))
    }

    /// `exercise_action`: 1 exercise, 2 lapse. `override_`: 0 no, 1 yes.
    pub fn exercise_options(
        &self,
        contract: &Contract,
        exercise_action: i32,
        exercise_quantity: i32,
        account: &str,
        override_: i32,
    ) -> Result<()> {
        let req_id = self.inner.client.next_req_id();
        Ok(self.inner.client.exercise_options(
            req_id,
            contract,
            exercise_action,
            exercise_quantity,
            account,
            override_,
            "",
            "",
            false,
        )?)
    }

    // ---- news, fundamentals, FA, WSH -----------------------------------

    pub async fn req_news_providers(&self) -> Result<Vec<NewsProvider>> {
        let items = self
            .request("reqNewsProviders", ReqKey::NewsProviders, None, |c| {
                c.req_news_providers()
            })
            .await?;
        Ok(reply!(items, Incoming::NewsProviders { providers } => providers).unwrap_or_default())
    }

    pub async fn req_news_article(
        &self,
        provider_code: &str,
        article_id: &str,
        options: &[TagValue],
    ) -> Result<NewsArticle> {
        let req_id = self.inner.client.next_req_id();
        let items = self
            .request("reqNewsArticle", ReqKey::Id(req_id), None, |c| {
                c.req_news_article(req_id, provider_code, article_id, options)
            })
            .await?;
        Ok(msgs(items)
            .find_map(|m| {
                if let Incoming::NewsArticle {
                    article_type,
                    article_text,
                    ..
                } = m
                {
                    Some(NewsArticle {
                        article_type,
                        article_text,
                    })
                } else {
                    None
                }
            })
            .unwrap_or_default())
    }

    pub async fn req_historical_news(
        &self,
        con_id: i32,
        provider_codes: &str,
        start_date_time: &str,
        end_date_time: &str,
        total_results: i32,
        options: &[TagValue],
    ) -> Result<Vec<HistoricalNews>> {
        let req_id = self.inner.client.next_req_id();
        let items = self
            .request("reqHistoricalNews", ReqKey::Id(req_id), None, |c| {
                c.req_historical_news(
                    req_id,
                    con_id,
                    provider_codes,
                    start_date_time,
                    end_date_time,
                    total_results,
                    options,
                )
            })
            .await?;
        Ok(msgs(items)
            .filter_map(|m| match m {
                Incoming::HistoricalNews {
                    time,
                    provider_code,
                    article_id,
                    headline,
                    ..
                } => Some(HistoricalNews {
                    time,
                    provider_code,
                    article_id,
                    headline,
                }),
                _ => None,
            })
            .collect())
    }

    pub fn req_news_bulletins(&self, all_messages: bool) -> Result<()> {
        Ok(self.inner.client.req_news_bulletins(all_messages)?)
    }

    pub fn cancel_news_bulletins(&self) -> Result<()> {
        Ok(self.inner.client.cancel_news_bulletins()?)
    }

    pub async fn req_fundamental_data(
        &self,
        contract: &Contract,
        report_type: &str,
        options: &[TagValue],
    ) -> Result<String> {
        let req_id = self.inner.client.next_req_id();
        let items = self
            .request(
                "reqFundamentalData",
                ReqKey::Id(req_id),
                Some(contract),
                |c| c.req_fundamental_data(req_id, contract, report_type, options),
            )
            .await?;
        Ok(reply!(items, Incoming::FundamentalData { data, .. } => data).unwrap_or_default())
    }

    /// `fa_data_type`: 1 groups, 3 aliases.
    pub async fn request_fa(&self, fa_data_type: i32) -> Result<String> {
        let items = self
            .request("requestFA", ReqKey::RequestFa, None, |c| {
                c.request_fa(fa_data_type)
            })
            .await?;
        Ok(reply!(items, Incoming::ReceiveFa { xml, .. } => xml).unwrap_or_default())
    }

    pub async fn replace_fa(&self, fa_data_type: i32, xml: &str) -> Result<String> {
        let req_id = self.inner.client.next_req_id();
        let items = self
            .request("replaceFA", ReqKey::Id(req_id), None, |c| {
                c.replace_fa(req_id, fa_data_type, xml)
            })
            .await?;
        Ok(reply!(items, Incoming::ReplaceFaEnd { text, .. } => text).unwrap_or_default())
    }

    pub async fn get_wsh_meta_data(&self) -> Result<String> {
        let req_id = self.inner.client.next_req_id();
        let items = self
            .request("reqWshMetaData", ReqKey::Id(req_id), None, |c| {
                c.req_wsh_meta_data(req_id)
            })
            .await?;
        Ok(reply!(items, Incoming::WshMetaData { data_json, .. } => data_json).unwrap_or_default())
    }

    pub async fn get_wsh_event_data(&self, data: &WshEventData) -> Result<String> {
        let req_id = self.inner.client.next_req_id();
        let items = self
            .request("reqWshEventData", ReqKey::Id(req_id), None, |c| {
                c.req_wsh_event_data(req_id, data)
            })
            .await?;
        Ok(
            reply!(items, Incoming::WshEventData { data_json, .. } => data_json)
                .unwrap_or_default(),
        )
    }

    /// TWS API configuration (server 219+) as the JSON form of IB's `ConfigResponse`.
    pub async fn req_config(&self) -> Result<serde_json::Value> {
        let req_id = self.inner.client.next_req_id();
        let items = self
            .request("reqConfig", ReqKey::Id(req_id), None, |c| {
                c.req_config(req_id)
            })
            .await?;
        Ok(msgs(items)
            .find_map(|m| match m {
                Incoming::ConfigResponse { config, .. } => Some(config),
                _ => None,
            })
            .unwrap_or_default())
    }

    /// Change the TWS API configuration (server 221+). `request` is the JSON
    /// form of IB's `UpdateConfigRequest` without `reqId`; returns the
    /// `UpdateConfigResponse` (`status`, `changedFields`, `errors`, ...).
    pub async fn update_config(&self, request: &serde_json::Value) -> Result<serde_json::Value> {
        let req_id = self.inner.client.next_req_id();
        let mut request = request.clone();
        if let Some(obj) = request.as_object_mut() {
            obj.insert("reqId".into(), req_id.into());
        }
        let items = self
            .request("updateConfig", ReqKey::Id(req_id), None, |c| {
                c.update_config(&request)
            })
            .await?;
        Ok(msgs(items)
            .find_map(|m| match m {
                Incoming::UpdateConfigResponse { response, .. } => Some(response),
                _ => None,
            })
            .unwrap_or_default())
    }

    pub async fn req_user_info(&self) -> Result<String> {
        let req_id = self.inner.client.next_req_id();
        let items = self
            .request("reqUserInfo", ReqKey::Id(req_id), None, |c| {
                c.req_user_info(req_id)
            })
            .await?;
        Ok(msgs(items)
            .find_map(|m| {
                if let Incoming::UserInfo {
                    white_branding_id, ..
                } = m
                {
                    Some(white_branding_id)
                } else {
                    None
                }
            })
            .unwrap_or_default())
    }

    /// Server time in seconds since the epoch.
    pub async fn req_current_time(&self) -> Result<i64> {
        let items = self
            .request("reqCurrentTime", ReqKey::CurrentTime, None, |c| {
                c.req_current_time()
            })
            .await?;
        Ok(reply!(items, Incoming::CurrentTime { time } => time).unwrap_or_default())
    }

    /// Server time in milliseconds since the epoch (server version 197+).
    pub async fn req_current_time_in_millis(&self) -> Result<i64> {
        let items = self
            .request(
                "reqCurrentTimeInMillis",
                ReqKey::CurrentTimeMillis,
                None,
                |c| c.req_current_time_in_millis(),
            )
            .await?;
        Ok(reply!(items, Incoming::CurrentTimeInMillis { time } => time).unwrap_or_default())
    }

    pub async fn req_family_codes(&self) -> Result<Vec<FamilyCode>> {
        let items = self
            .request("reqFamilyCodes", ReqKey::FamilyCodes, None, |c| {
                c.req_family_codes()
            })
            .await?;
        Ok(reply!(items, Incoming::FamilyCodes { codes } => codes).unwrap_or_default())
    }

    pub async fn req_soft_dollar_tiers(&self) -> Result<Vec<SoftDollarTier>> {
        let req_id = self.inner.client.next_req_id();
        let items = self
            .request("reqSoftDollarTiers", ReqKey::Id(req_id), None, |c| {
                c.req_soft_dollar_tiers(req_id)
            })
            .await?;
        Ok(reply!(items, Incoming::SoftDollarTiers { tiers, .. } => tiers).unwrap_or_default())
    }
}

struct PendingGuard<'a> {
    ib: &'a IB,
    keys: Vec<ReqKey>,
}

impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.ib.state();
        for key in &self.keys {
            state.take_req(key);
        }
    }
}

fn trades_of(items: Vec<Collected>) -> Vec<TradeHandle> {
    items
        .into_iter()
        .filter_map(|c| {
            if let Collected::Trade(t) = c {
                Some(t)
            } else {
                None
            }
        })
        .collect()
}

async fn dispatcher(inner: std::sync::Weak<Inner>, mut rx: EventReceiver) {
    while let Some(batch) = rx.recv_batch().await {
        let Some(inner) = inner.upgrade() else { return };
        *inner.last_recv.lock() = tokio::time::Instant::now();
        let emit_messages = inner.opts.emit_messages;
        let mut events = Vec::new();
        let resubscribe = {
            let mut s = inner.state.lock();
            s.begin_batch();
            for m in batch {
                if emit_messages {
                    events.push(Event::Message(Arc::new(m.clone())));
                }
                s.apply(m);
            }
            let (batch_events, resubscribe) = s.end_batch();
            events.extend(batch_events);
            resubscribe
        };
        inner.emit(events);
        for sub in resubscribe {
            let c = &inner.client;
            let r = match sub {
                Subscriber::Bars(h) => {
                    let b = h.read();
                    c.cancel_historical_data(b.req_id).and_then(|_| {
                        c.req_historical_data(
                            b.req_id,
                            &b.contract,
                            &b.end_date_time,
                            &b.duration_str,
                            &b.bar_size_setting,
                            &b.what_to_show,
                            b.use_rth,
                            b.format_date,
                            b.keep_up_to_date,
                            &b.chart_options,
                        )
                    })
                }
                Subscriber::RealTimeBars(h) => {
                    let b = h.read();
                    c.cancel_real_time_bars(b.req_id).and_then(|_| {
                        c.req_real_time_bars(
                            b.req_id,
                            &b.contract,
                            b.bar_size,
                            &b.what_to_show,
                            b.use_rth,
                            &b.real_time_bars_options,
                        )
                    })
                }
                Subscriber::Scan(_) => Ok(()),
            };
            if let Err(e) = r {
                tracing::error!(error = %e, "resubscription after error 10225 failed");
            }
        }
    }
    if let Some(inner) = inner.upgrade() {
        let reason = inner
            .client
            .close_reason()
            .unwrap_or_else(|| "connection closed".into());
        inner.state.lock().connection_closed(&reason);
        inner.emit(vec![Event::Disconnected(reason)]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ibkit_tws::testing::{FakeTws, FakeTwsConfig};

    #[tokio::test]
    async fn requests_that_end_early_leave_no_pending_entry() {
        let tws = FakeTws::with(
            FakeTwsConfig::default(),
            Arc::new(|_: &[String]| Vec::new()),
        )
        .await;
        let opts = IbOptions {
            connect: ConnectOptions::new("127.0.0.1", tws.port(), 1),
            request_timeout: Some(Duration::from_millis(100)),
            fetch: StartupFetch {
                positions: false,
                orders_open: false,
                orders_complete: false,
                account_updates: false,
                sub_account_updates: false,
                executions: false,
            },
            ..Default::default()
        };
        let ib = IB::connect_with(opts).await.unwrap();
        let c = Contract {
            con_id: 1,
            ..Default::default()
        };
        assert!(matches!(
            ib.req_head_time_stamp(&c, "TRADES", true, 1).await,
            Err(Error::Timeout { .. })
        ));
        let dropped =
            tokio::time::timeout(Duration::from_millis(10), ib.req_contract_details(&c)).await;
        assert!(
            dropped.is_err(),
            "the future was dropped by the outer timeout"
        );
        assert!(
            ib.req_tickers(&[c], false).await.unwrap()[0]
                .read()
                .bid
                .is_nan()
        );
        assert!(
            ib.state().pending.is_empty(),
            "pending: {:?}",
            ib.state().pending.keys().collect::<Vec<_>>()
        );
        assert!(ib.state().req_id_to_ticker.is_empty());
    }
}
