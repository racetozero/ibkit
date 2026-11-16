//! Python bindings for ibkit.
//!
//! The extension exposes "cores": thin handles over the Rust objects. The pure
//! Python package `ibkit` builds the ib_async-style API on top of them. Every
//! request method takes a `blocking` flag: `True` blocks the calling thread
//! with the GIL released, `False` returns an awaitable for asyncio.

// Request methods mirror IB's positional signatures.
#![allow(clippy::too_many_arguments)]

use ibkit::state::{
    BarsHandle, Event, PnLHandle, PnLSingleHandle, RealTimeBarsHandle, ScanHandle, TickerHandle,
    TradeHandle,
};
use ibkit::*;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};
use pyo3_async_runtimes::tokio::{future_into_py, get_runtime};
use pythonize::{depythonize, pythonize};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

// The error classes live in `ibkit/errors.py`: they also derive from the
// built-in ConnectionError, TimeoutError and ValueError.
mod errors {
    pyo3::import_exception!(ibkit.errors, IbkitError);
    pyo3::import_exception!(ibkit.errors, RequestError);
    pyo3::import_exception!(ibkit.errors, ConnectionError);
    pyo3::import_exception!(ibkit.errors, TimeoutError);
    pyo3::import_exception!(ibkit.errors, InvalidArgumentError);
    pyo3::import_exception!(ibkit.errors, GatewayError);
}
use errors::{
    ConnectionError, GatewayError, IbkitError, InvalidArgumentError, RequestError, TimeoutError,
};

/// The error class of `kind`, shared with the Node binding.
fn kind_err(kind: ibkit::ErrorKind, message: String) -> PyErr {
    match kind {
        ibkit::ErrorKind::Request | ibkit::ErrorKind::Other => IbkitError::new_err(message),
        ibkit::ErrorKind::Connection => ConnectionError::new_err(message),
        ibkit::ErrorKind::Timeout => TimeoutError::new_err(message),
        ibkit::ErrorKind::InvalidArgument => InvalidArgumentError::new_err(message),
    }
}

fn err(e: ibkit::Error) -> PyErr {
    match e {
        ibkit::Error::Request(r) => RequestError::new_err((r.req_id, r.code, r.message)),
        ibkit::Error::Invalid(m) => InvalidArgumentError::new_err(m),
        other => kind_err(other.kind(), other.to_string()),
    }
}

fn to_py<T: Serialize + ?Sized>(py: Python<'_>, v: &T) -> PyResult<Py<PyAny>> {
    Ok(pythonize(py, v)?.unbind())
}

fn attach_py<T: Serialize>(v: &T) -> PyResult<Py<PyAny>> {
    Python::attach(|py| to_py(py, v))
}

fn from_py<T: DeserializeOwned>(obj: &Bound<'_, PyAny>, what: &str) -> PyResult<T> {
    depythonize(obj).map_err(|e| {
        InvalidArgumentError::new_err(format!(
            "{what} is not valid: {e}. Pass the dictionary form produced by ibkit.util.to_dict"
        ))
    })
}

/// Block (GIL released) or return an awaitable.
/// Warn when a blocking call runs inside a running asyncio event loop: the
/// loop stops until the call returns. A loop that nest_asyncio patched
/// (`util.startLoop()`, for notebooks) expects blocking calls.
fn warn_in_event_loop(py: Python<'_>) -> PyResult<()> {
    let running = py.import("asyncio")?.call_method0("_get_running_loop")?;
    if running.is_none() || running.hasattr("_nest_patched")? {
        return Ok(());
    }
    PyErr::warn(
        py,
        &py.get_type::<pyo3::exceptions::PyRuntimeWarning>(),
        c"A blocking ibkit call ran inside a running asyncio event loop, which stops the loop until the call returns. Await the ...Async method (or use AsyncWebClient) instead; in a notebook, call ibkit.util.startLoop() first.",
        2,
    )
}

fn run<F>(py: Python<'_>, blocking: bool, fut: F) -> PyResult<Py<PyAny>>
where
    F: Future<Output = PyResult<Py<PyAny>>> + Send + 'static,
{
    if blocking {
        warn_in_event_loop(py)?;
        py.detach(|| get_runtime().block_on(fut))
    } else {
        Ok(future_into_py(py, fut)?.unbind())
    }
}

macro_rules! core_handle {
    ($name:ident, $handle:ty) => {
        #[pyclass(frozen, skip_from_py_object, module = "ibkit._ibkit")]
        #[derive(Clone)]
        pub struct $name(pub $handle);

        impl $name {
            fn ptr_id(&self) -> usize {
                Arc::as_ptr(&self.0) as *const () as usize
            }
        }
    };
}

core_handle!(TickerCore, TickerHandle);
core_handle!(TradeCore, TradeHandle);
core_handle!(BarsCore, BarsHandle);
core_handle!(RealTimeBarsCore, RealTimeBarsHandle);
core_handle!(ScanCore, ScanHandle);
core_handle!(PnLCore, PnLHandle);
core_handle!(PnLSingleCore, PnLSingleHandle);

macro_rules! ticker_fields {
    ($t:expr, $name:expr; $($camel:literal => $field:ident),* $(,)?) => {
        match $name { $($camel => Some($t.$field),)* _ => None }
    };
}

#[pymethods]
impl TickerCore {
    /// Identity of the underlying Rust object.
    fn id(&self) -> usize {
        self.ptr_id()
    }

    /// Value of one camelCase field (fast path for numeric fields).
    fn field(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        let t = self.0.read();
        let num = ticker_fields!(t, name;
            "bid" => bid, "bidSize" => bid_size, "ask" => ask, "askSize" => ask_size, "last" => last, "lastSize" => last_size,
            "prevBid" => prev_bid, "prevBidSize" => prev_bid_size, "prevAsk" => prev_ask, "prevAskSize" => prev_ask_size,
            "prevLast" => prev_last, "prevLastSize" => prev_last_size, "volume" => volume, "open" => open, "high" => high,
            "low" => low, "close" => close, "vwap" => vwap, "markPrice" => mark_price, "halted" => halted,
            "rtVolume" => rt_volume, "avVolume" => av_volume, "shortableShares" => shortable_shares,
            "impliedVolatility" => implied_volatility, "histVolatility" => hist_volatility, "minTick" => min_tick,
        );
        if let Some(v) = num {
            return Ok(v.into_pyobject(py)?.into_any().unbind());
        }
        let all = pythonize(py, &*t)?;
        let d = all.cast::<PyDict>()?;
        match d.get_item(name)? {
            Some(v) => Ok(v.unbind()),
            None => Err(pyo3::exceptions::PyAttributeError::new_err(format!(
                "Ticker has no field {name:?}"
            ))),
        }
    }

    fn snapshot(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        to_py(py, &*self.0.read())
    }

    fn midpoint(&self) -> f64 {
        self.0.read().midpoint()
    }

    fn market_price(&self) -> f64 {
        self.0.read().market_price()
    }

    fn has_bid_ask(&self) -> bool {
        self.0.read().has_bid_ask()
    }
}

#[pymethods]
impl TradeCore {
    /// Identity of the underlying Rust object.
    fn id(&self) -> usize {
        self.ptr_id()
    }

    fn snapshot(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        to_py(py, &*self.0.lock())
    }
    fn status(&self) -> String {
        self.0.lock().order_status.status.clone()
    }
    fn is_done(&self) -> bool {
        self.0.lock().is_done()
    }
    fn is_active(&self) -> bool {
        self.0.lock().is_active()
    }
    fn is_working(&self) -> bool {
        self.0.lock().is_working()
    }
    fn is_waiting(&self) -> bool {
        self.0.lock().is_waiting()
    }
    fn filled(&self) -> f64 {
        self.0.lock().filled()
    }
    fn remaining(&self) -> f64 {
        self.0.lock().remaining()
    }
}

#[pymethods]
impl BarsCore {
    /// Identity of the underlying Rust object.
    fn id(&self) -> usize {
        self.ptr_id()
    }

    fn snapshot(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        to_py(py, &*self.0.read())
    }
    fn bars(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        to_py(py, &self.0.read().bars)
    }
    fn req_id(&self) -> i32 {
        self.0.read().req_id
    }
}

#[pymethods]
impl RealTimeBarsCore {
    /// Identity of the underlying Rust object.
    fn id(&self) -> usize {
        self.ptr_id()
    }

    fn snapshot(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        to_py(py, &*self.0.read())
    }
    fn bars(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        to_py(py, &self.0.read().bars)
    }
    fn req_id(&self) -> i32 {
        self.0.read().req_id
    }
}

#[pymethods]
impl ScanCore {
    /// Identity of the underlying Rust object.
    fn id(&self) -> usize {
        self.ptr_id()
    }

    fn snapshot(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        to_py(py, &*self.0.read())
    }
    fn req_id(&self) -> i32 {
        self.0.read().req_id
    }
}

#[pymethods]
impl PnLCore {
    /// Identity of the underlying Rust object.
    fn id(&self) -> usize {
        self.ptr_id()
    }

    fn snapshot(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        to_py(py, &*self.0.read())
    }
}

#[pymethods]
impl PnLSingleCore {
    /// Identity of the underlying Rust object.
    fn id(&self) -> usize {
        self.ptr_id()
    }

    fn snapshot(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        to_py(py, &*self.0.read())
    }
}

fn event_to_py(py: Python<'_>, e: &Event) -> PyResult<Py<PyAny>> {
    let trade = |t: &TradeHandle| -> PyResult<Py<PyAny>> {
        Ok(Py::new(py, TradeCore(t.clone()))?.into_any())
    };
    let items: Vec<Py<PyAny>> = match e {
        Event::Message(m) => vec![to_py(py, "message")?, incoming_to_py(py, m)?],
        Event::Connected => vec![to_py(py, "connected")?],
        Event::Disconnected(r) => vec![to_py(py, "disconnected")?, to_py(py, r)?],
        Event::Update => vec![to_py(py, "update")?],
        Event::PendingTickers(ts) => {
            let list = PyList::empty(py);
            for t in ts {
                list.append(Py::new(py, TickerCore(t.clone()))?)?;
            }
            vec![to_py(py, "pendingTickers")?, list.into_any().unbind()]
        }
        Event::BarUpdate {
            req_id,
            has_new_bar,
        } => vec![
            to_py(py, "barUpdate")?,
            to_py(py, req_id)?,
            to_py(py, has_new_bar)?,
        ],
        Event::NewOrder(t) => vec![to_py(py, "newOrder")?, trade(t)?],
        Event::OrderModify(t) => vec![to_py(py, "orderModify")?, trade(t)?],
        Event::CancelOrder(t) => vec![to_py(py, "cancelOrder")?, trade(t)?],
        Event::OpenOrder(t) => vec![to_py(py, "openOrder")?, trade(t)?],
        Event::OrderStatus(t) => vec![to_py(py, "orderStatus")?, trade(t)?],
        Event::Filled(t) => vec![to_py(py, "filled")?, trade(t)?],
        Event::Cancelled(t) => vec![to_py(py, "cancelled")?, trade(t)?],
        Event::ExecDetails(t, f) => vec![to_py(py, "execDetails")?, trade(t)?, to_py(py, &**f)?],
        Event::CommissionReport(t, f, r) => vec![
            to_py(py, "commissionReport")?,
            trade(t)?,
            to_py(py, &**f)?,
            to_py(py, r)?,
        ],
        Event::UpdatePortfolio(p) => vec![to_py(py, "updatePortfolio")?, to_py(py, &**p)?],
        Event::Position(p) => vec![to_py(py, "position")?, to_py(py, &**p)?],
        Event::AccountValue(v) => vec![to_py(py, "accountValue")?, to_py(py, v)?],
        Event::AccountSummary(v) => vec![to_py(py, "accountSummary")?, to_py(py, v)?],
        Event::PnL(p) => vec![to_py(py, "pnl")?, to_py(py, p)?],
        Event::PnLSingle(p) => vec![to_py(py, "pnlSingle")?, to_py(py, p)?],
        Event::ScannerData(id) => vec![to_py(py, "scannerData")?, to_py(py, id)?],
        Event::TickNews(n) => vec![to_py(py, "tickNews")?, to_py(py, n)?],
        Event::NewsBulletin(b) => vec![to_py(py, "newsBulletin")?, to_py(py, b)?],
        Event::WshMeta(s) => vec![to_py(py, "wshMeta")?, to_py(py, s)?],
        Event::WshEvent(s) => vec![to_py(py, "wshEvent")?, to_py(py, s)?],
        Event::Error {
            req_id,
            code,
            message,
            contract,
        } => {
            vec![
                to_py(py, "error")?,
                to_py(py, req_id)?,
                to_py(py, code)?,
                to_py(py, message)?,
                to_py(py, contract)?,
            ]
        }
        Event::Timeout(s) => vec![to_py(py, "timeout")?, to_py(py, s)?],
    };
    Ok(PyTuple::new(py, items)?.into_any().unbind())
}

type EventRx = Arc<tokio::sync::Mutex<broadcast::Receiver<Event>>>;

async fn next_events(rx: EventRx, timeout: Option<Duration>) -> Vec<Event> {
    // Taking the lock is part of the wait: a pump that holds it must not
    // extend `timeout` until its next message.
    let batch = async {
        let mut rx = rx.lock().await;
        let first = loop {
            match rx.recv().await {
                Ok(e) => break e,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!(
                        "ibkit: the Python event consumer lagged and dropped {n} events; poll events more often"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => return Vec::new(),
            }
        };
        let mut out = vec![first];
        while let Ok(e) = rx.try_recv() {
            out.push(e);
        }
        out
    };
    match timeout {
        Some(t) => tokio::time::timeout(t, batch).await.unwrap_or_default(),
        None => batch.await,
    }
}

fn events_to_py(events: Vec<Event>) -> PyResult<Py<PyAny>> {
    Python::attach(|py| {
        let list = PyList::empty(py);
        for e in &events {
            list.append(event_to_py(py, e)?)?;
        }
        Ok(list.into_any().unbind())
    })
}

/// Connected high-level client.
#[pyclass(frozen, module = "ibkit._ibkit")]
pub struct IBCore {
    ib: IB,
    events: EventRx,
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ConnectArgs {
    host: String,
    port: u16,
    client_id: i32,
    timeout: Option<f64>,
    readonly: bool,
    account: String,
    raise_sync_errors: bool,
    raise_request_errors: bool,
    request_timeout: Option<f64>,
    emit_messages: bool,
    max_requests: Option<u32>,
    requests_interval: Option<f64>,
    connect_options: String,
    /// `StartupFetch` bits: 1 positions, 2 open orders, 4 completed orders,
    /// 8 account updates, 16 sub-account updates, 32 executions.
    fetch_fields: Option<u32>,
    empty_price: Option<f64>,
    empty_size: Option<f64>,
}

macro_rules! req {
    ($self:ident, $py:ident, $blocking:ident, |$ib:ident| $body:expr) => {{
        let $ib = $self.ib.clone();
        run($py, $blocking, async move {
            let v = $body.await.map_err(err)?;
            attach_py(&v)
        })
    }};
}

#[pymethods]
impl IBCore {
    /// Connect and synchronize (ib_async `connect`). `args` keys are camelCase.
    #[staticmethod]
    fn connect(py: Python<'_>, args: &Bound<'_, PyAny>, blocking: bool) -> PyResult<Py<PyAny>> {
        let a: ConnectArgs = from_py(args, "connect arguments")?;
        let mut opts = IbOptions {
            connect: ConnectOptions::new(&a.host, a.port, a.client_id),
            ..Default::default()
        };
        if let Some(t) = a.timeout.filter(|t| *t > 0.0) {
            opts.connect.timeout = Duration::from_secs_f64(t);
        }
        if let Some(m) = a.max_requests {
            opts.connect.max_requests = m;
        }
        if let Some(i) = a.requests_interval {
            opts.connect.requests_interval = Duration::from_secs_f64(i);
        }
        opts.connect.connect_options = a.connect_options;
        opts.readonly = a.readonly;
        opts.account = a.account;
        opts.raise_sync_errors = a.raise_sync_errors;
        opts.raise_request_errors = a.raise_request_errors;
        opts.emit_messages = a.emit_messages;
        opts.request_timeout = a
            .request_timeout
            .filter(|t| *t > 0.0)
            .map(Duration::from_secs_f64);
        let bits = a.fetch_fields.unwrap_or(63);
        opts.fetch = StartupFetch {
            positions: bits & 1 != 0,
            orders_open: bits & 2 != 0,
            orders_complete: bits & 4 != 0,
            account_updates: bits & 8 != 0,
            sub_account_updates: bits & 16 != 0,
            executions: bits & 32 != 0,
        };
        if let Some(p) = a.empty_price {
            opts.defaults.empty_price = p;
        }
        if let Some(s) = a.empty_size {
            opts.defaults.empty_size = s;
        }
        run(py, blocking, async move {
            let ib = IB::connect_with(opts).await.map_err(err)?;
            let events = Arc::new(tokio::sync::Mutex::new(ib.events()));
            Python::attach(|py| Ok(Py::new(py, IBCore { ib, events })?.into_any()))
        })
    }

    fn is_connected(&self) -> bool {
        self.ib.is_connected()
    }

    fn disconnect(&self) {
        self.ib.disconnect();
    }

    fn client_id(&self) -> i32 {
        self.ib.client().client_id()
    }

    fn server_version(&self) -> i32 {
        self.ib.client().server_version()
    }

    fn managed_accounts(&self) -> Vec<String> {
        self.ib.managed_accounts()
    }

    fn next_req_id(&self) -> i32 {
        self.ib.client().next_req_id()
    }

    /// Low-level client sharing this connection.
    fn client(&self) -> ClientCore {
        ClientCore {
            client: self.ib.client().clone(),
            messages: None,
        }
    }

    /// Emit a `timeout` event once no message arrived for `timeout` seconds; 0 stops.
    fn set_timeout(&self, timeout: f64) {
        let _rt = get_runtime().enter();
        self.ib
            .set_timeout(Some(Duration::from_secs_f64(timeout.max(0.0))));
    }

    fn connection_stats(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let s = self.ib.connection_stats();
        let d = PyDict::new(py);
        d.set_item("startTime", s.start_time)?;
        d.set_item("duration", s.duration)?;
        d.set_item("numBytesRecv", s.num_bytes_recv)?;
        d.set_item("numBytesSent", s.num_bytes_sent)?;
        d.set_item("numMsgRecv", s.num_msg_recv)?;
        d.set_item("numMsgSent", s.num_msg_sent)?;
        Ok(d.into_any().unbind())
    }

    /// Wait up to `timeout` seconds for events; returns all queued events.
    fn poll_events(&self, py: Python<'_>, timeout: Option<f64>) -> PyResult<Py<PyAny>> {
        let rx = self.events.clone();
        let t = timeout.map(|t| Duration::from_secs_f64(t.max(0.0)));
        if t != Some(Duration::ZERO) {
            warn_in_event_loop(py)?;
        }
        let events = py.detach(|| get_runtime().block_on(next_events(rx, t)));
        events_to_py(events)
    }

    /// Awaitable variant of `poll_events` without a timeout.
    fn next_events(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let rx = self.events.clone();
        Ok(future_into_py(py, async move { events_to_py(next_events(rx, None).await) })?.unbind())
    }

    // ---- state ------------------------------------------------------------

    fn positions(&self, py: Python<'_>, account: &str) -> PyResult<Py<PyAny>> {
        to_py(py, &self.ib.positions(account))
    }
    fn portfolio(&self, py: Python<'_>, account: &str) -> PyResult<Py<PyAny>> {
        to_py(py, &self.ib.portfolio(account))
    }
    fn account_values(&self, py: Python<'_>, account: &str) -> PyResult<Py<PyAny>> {
        to_py(py, &self.ib.account_values(account))
    }
    fn account_summary(&self, py: Python<'_>, account: &str) -> PyResult<Py<PyAny>> {
        to_py(py, &self.ib.account_summary(account))
    }
    fn pnl(&self, py: Python<'_>, account: &str, model_code: &str) -> PyResult<Py<PyAny>> {
        to_py(py, &self.ib.pnl(account, model_code))
    }
    fn pnl_single(
        &self,
        py: Python<'_>,
        account: &str,
        model_code: &str,
        con_id: i32,
    ) -> PyResult<Py<PyAny>> {
        to_py(py, &self.ib.pnl_single(account, model_code, con_id))
    }
    fn trades(&self) -> Vec<TradeCore> {
        self.ib.trades().into_iter().map(TradeCore).collect()
    }
    fn open_trades(&self) -> Vec<TradeCore> {
        self.ib.open_trades().into_iter().map(TradeCore).collect()
    }
    fn fills(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        to_py(py, &self.ib.fills())
    }
    fn tickers(&self) -> Vec<TickerCore> {
        self.ib.tickers().into_iter().map(TickerCore).collect()
    }
    fn ticker(&self, contract: &Bound<'_, PyAny>) -> PyResult<Option<TickerCore>> {
        let c: Contract = from_py(contract, "contract")?;
        Ok(self.ib.ticker(&c).map(TickerCore))
    }
    fn news_ticks(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        to_py(py, &self.ib.news_ticks())
    }
    fn news_bulletins(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        to_py(py, &self.ib.news_bulletins())
    }
    fn realtime_bars(&self) -> Vec<RealTimeBarsCore> {
        self.ib
            .realtime_bars()
            .into_iter()
            .map(RealTimeBarsCore)
            .collect()
    }

    // ---- immediate actions ------------------------------------------------

    fn place_order(
        &self,
        contract: &Bound<'_, PyAny>,
        order: &Bound<'_, PyAny>,
    ) -> PyResult<TradeCore> {
        let c: Contract = from_py(contract, "contract")?;
        let o: Order = from_py(order, "order")?;
        self.ib.place_order(&c, o).map(TradeCore).map_err(err)
    }
    fn cancel_order(
        &self,
        order: &Bound<'_, PyAny>,
        manual_cancel_order_time: &str,
    ) -> PyResult<Option<TradeCore>> {
        let o: Order = from_py(order, "order")?;
        Ok(self
            .ib
            .cancel_order(&o, manual_cancel_order_time)
            .map_err(err)?
            .map(TradeCore))
    }
    fn req_global_cancel(&self) -> PyResult<()> {
        self.ib.req_global_cancel().map_err(err)
    }
    fn bracket_order(
        &self,
        py: Python<'_>,
        action: &str,
        quantity: f64,
        limit_price: f64,
        take_profit_price: f64,
        stop_loss_price: f64,
    ) -> PyResult<Py<PyAny>> {
        let b = self
            .ib
            .bracket_order(
                action,
                quantity,
                limit_price,
                take_profit_price,
                stop_loss_price,
            )
            .map_err(err)?;
        Ok(PyTuple::new(
            py,
            [
                to_py(py, &b.parent)?,
                to_py(py, &b.take_profit)?,
                to_py(py, &b.stop_loss)?,
            ],
        )?
        .into_any()
        .unbind())
    }
    fn req_auto_open_orders(&self, auto_bind: bool) -> PyResult<()> {
        self.ib.req_auto_open_orders(auto_bind).map_err(err)
    }
    fn req_market_data_type(&self, market_data_type: i32) -> PyResult<()> {
        self.ib.req_market_data_type(market_data_type).map_err(err)
    }
    fn req_mkt_data(
        &self,
        contract: &Bound<'_, PyAny>,
        generic_tick_list: &str,
        snapshot: bool,
        regulatory_snapshot: bool,
        options: &Bound<'_, PyAny>,
    ) -> PyResult<TickerCore> {
        let c: Contract = from_py(contract, "contract")?;
        let opts: Vec<TagValue> = from_py(options, "mktDataOptions")?;
        self.ib
            .req_mkt_data(&c, generic_tick_list, snapshot, regulatory_snapshot, &opts)
            .map(TickerCore)
            .map_err(err)
    }
    fn cancel_mkt_data(&self, contract: &Bound<'_, PyAny>) -> PyResult<bool> {
        let c: Contract = from_py(contract, "contract")?;
        self.ib.cancel_mkt_data(&c).map_err(err)
    }
    fn req_tick_by_tick_data(
        &self,
        contract: &Bound<'_, PyAny>,
        tick_type: &str,
        number_of_ticks: i32,
        ignore_size: bool,
    ) -> PyResult<TickerCore> {
        let c: Contract = from_py(contract, "contract")?;
        self.ib
            .req_tick_by_tick_data(&c, tick_type, number_of_ticks, ignore_size)
            .map(TickerCore)
            .map_err(err)
    }
    fn cancel_tick_by_tick_data(
        &self,
        contract: &Bound<'_, PyAny>,
        tick_type: &str,
    ) -> PyResult<bool> {
        let c: Contract = from_py(contract, "contract")?;
        self.ib.cancel_tick_by_tick_data(&c, tick_type).map_err(err)
    }
    fn req_mkt_depth(
        &self,
        contract: &Bound<'_, PyAny>,
        num_rows: i32,
        is_smart_depth: bool,
        options: &Bound<'_, PyAny>,
    ) -> PyResult<TickerCore> {
        let c: Contract = from_py(contract, "contract")?;
        let opts: Vec<TagValue> = from_py(options, "mktDepthOptions")?;
        self.ib
            .req_mkt_depth(&c, num_rows, is_smart_depth, &opts)
            .map(TickerCore)
            .map_err(err)
    }
    fn cancel_mkt_depth(
        &self,
        contract: &Bound<'_, PyAny>,
        is_smart_depth: bool,
    ) -> PyResult<bool> {
        let c: Contract = from_py(contract, "contract")?;
        self.ib.cancel_mkt_depth(&c, is_smart_depth).map_err(err)
    }
    fn req_real_time_bars(
        &self,
        contract: &Bound<'_, PyAny>,
        bar_size: i32,
        what_to_show: &str,
        use_rth: bool,
        options: &Bound<'_, PyAny>,
    ) -> PyResult<RealTimeBarsCore> {
        let c: Contract = from_py(contract, "contract")?;
        let opts: Vec<TagValue> = from_py(options, "realTimeBarsOptions")?;
        self.ib
            .req_real_time_bars(&c, bar_size, what_to_show, use_rth, &opts)
            .map(RealTimeBarsCore)
            .map_err(err)
    }
    fn cancel_real_time_bars(&self, bars: &RealTimeBarsCore) -> PyResult<()> {
        self.ib.cancel_real_time_bars(&bars.0).map_err(err)
    }
    fn cancel_historical_data(&self, bars: &BarsCore) -> PyResult<()> {
        self.ib.cancel_historical_data(&bars.0).map_err(err)
    }
    fn req_pnl(&self, account: &str, model_code: &str) -> PyResult<PnLCore> {
        self.ib
            .req_pnl(account, model_code)
            .map(PnLCore)
            .map_err(err)
    }
    fn cancel_pnl(&self, account: &str, model_code: &str) -> PyResult<()> {
        self.ib.cancel_pnl(account, model_code).map_err(err)
    }
    fn req_pnl_single(
        &self,
        account: &str,
        model_code: &str,
        con_id: i32,
    ) -> PyResult<PnLSingleCore> {
        self.ib
            .req_pnl_single(account, model_code, con_id)
            .map(PnLSingleCore)
            .map_err(err)
    }
    fn cancel_pnl_single(&self, account: &str, model_code: &str, con_id: i32) -> PyResult<()> {
        self.ib
            .cancel_pnl_single(account, model_code, con_id)
            .map_err(err)
    }
    fn req_scanner_subscription(
        &self,
        subscription: &Bound<'_, PyAny>,
        options: &Bound<'_, PyAny>,
        filter_options: &Bound<'_, PyAny>,
    ) -> PyResult<ScanCore> {
        let s: ScannerSubscription = from_py(subscription, "subscription")?;
        let o: Vec<TagValue> = from_py(options, "scannerSubscriptionOptions")?;
        let f: Vec<TagValue> = from_py(filter_options, "scannerSubscriptionFilterOptions")?;
        self.ib
            .req_scanner_subscription(&s, &o, &f)
            .map(ScanCore)
            .map_err(err)
    }
    fn cancel_scanner_subscription(&self, scan: &ScanCore) -> PyResult<()> {
        self.ib.cancel_scanner_subscription(&scan.0).map_err(err)
    }
    fn exercise_options(
        &self,
        contract: &Bound<'_, PyAny>,
        exercise_action: i32,
        exercise_quantity: i32,
        account: &str,
        override_: i32,
    ) -> PyResult<()> {
        let c: Contract = from_py(contract, "contract")?;
        self.ib
            .exercise_options(&c, exercise_action, exercise_quantity, account, override_)
            .map_err(err)
    }
    fn req_news_bulletins(&self, all_messages: bool) -> PyResult<()> {
        self.ib.req_news_bulletins(all_messages).map_err(err)
    }
    fn cancel_news_bulletins(&self) -> PyResult<()> {
        self.ib.cancel_news_bulletins().map_err(err)
    }

    // ---- requests (blocking or awaitable) ------------------------------------

    fn qualify_contracts(
        &self,
        py: Python<'_>,
        contracts: &Bound<'_, PyAny>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let cs: Vec<Contract> = from_py(contracts, "contracts")?;
        req!(self, py, blocking, |ib| async move {
            ib.qualify_contracts(&cs).await
        })
    }
    fn req_contract_details(
        &self,
        py: Python<'_>,
        contract: &Bound<'_, PyAny>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let c: Contract = from_py(contract, "contract")?;
        req!(self, py, blocking, |ib| async move {
            ib.req_contract_details(&c).await
        })
    }
    fn req_tickers(
        &self,
        py: Python<'_>,
        contracts: &Bound<'_, PyAny>,
        regulatory_snapshot: bool,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let cs: Vec<Contract> = from_py(contracts, "contracts")?;
        let ib = self.ib.clone();
        run(py, blocking, async move {
            let ts = ib
                .req_tickers(&cs, regulatory_snapshot)
                .await
                .map_err(err)?;
            Python::attach(|py| {
                let list = PyList::empty(py);
                for t in ts {
                    list.append(Py::new(py, TickerCore(t))?)?;
                }
                Ok(list.into_any().unbind())
            })
        })
    }
    #[pyo3(signature = (contract, end_date_time, duration_str, bar_size_setting, what_to_show, use_rth, format_date, keep_up_to_date, chart_options, blocking))]
    fn req_historical_data(
        &self,
        py: Python<'_>,
        contract: &Bound<'_, PyAny>,
        end_date_time: String,
        duration_str: String,
        bar_size_setting: String,
        what_to_show: String,
        use_rth: bool,
        format_date: i32,
        keep_up_to_date: bool,
        chart_options: &Bound<'_, PyAny>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let c: Contract = from_py(contract, "contract")?;
        let opts: Vec<TagValue> = from_py(chart_options, "chartOptions")?;
        let ib = self.ib.clone();
        run(py, blocking, async move {
            let bars = ib
                .req_historical_data(
                    &c,
                    &end_date_time,
                    &duration_str,
                    &bar_size_setting,
                    &what_to_show,
                    use_rth,
                    format_date,
                    keep_up_to_date,
                    &opts,
                )
                .await
                .map_err(err)?;
            Python::attach(|py| Ok(Py::new(py, BarsCore(bars))?.into_any()))
        })
    }
    fn req_historical_schedule(
        &self,
        py: Python<'_>,
        contract: &Bound<'_, PyAny>,
        num_days: i32,
        end_date_time: String,
        use_rth: bool,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let c: Contract = from_py(contract, "contract")?;
        req!(self, py, blocking, |ib| async move {
            ib.req_historical_schedule(&c, num_days, &end_date_time, use_rth)
                .await
        })
    }
    #[pyo3(signature = (contract, start_date_time, end_date_time, number_of_ticks, what_to_show, use_rth, ignore_size, misc_options, blocking))]
    fn req_historical_ticks(
        &self,
        py: Python<'_>,
        contract: &Bound<'_, PyAny>,
        start_date_time: String,
        end_date_time: String,
        number_of_ticks: i32,
        what_to_show: String,
        use_rth: bool,
        ignore_size: bool,
        misc_options: &Bound<'_, PyAny>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let c: Contract = from_py(contract, "contract")?;
        let opts: Vec<TagValue> = from_py(misc_options, "miscOptions")?;
        let ib = self.ib.clone();
        run(py, blocking, async move {
            let msgs = ib
                .req_historical_ticks(
                    &c,
                    &start_date_time,
                    &end_date_time,
                    number_of_ticks,
                    &what_to_show,
                    use_rth,
                    ignore_size,
                    &opts,
                )
                .await
                .map_err(err)?;
            let mut ticks: Vec<serde_json::Value> = Vec::new();
            for m in msgs {
                let v = match m {
                    Incoming::HistoricalTicks { ticks, .. } => serde_json::to_value(ticks),
                    Incoming::HistoricalTicksBidAsk { ticks, .. } => serde_json::to_value(ticks),
                    Incoming::HistoricalTicksLast { ticks, .. } => serde_json::to_value(ticks),
                    _ => continue,
                };
                if let Ok(serde_json::Value::Array(a)) = v {
                    ticks.extend(a);
                }
            }
            attach_py(&ticks)
        })
    }
    fn req_head_time_stamp(
        &self,
        py: Python<'_>,
        contract: &Bound<'_, PyAny>,
        what_to_show: String,
        use_rth: bool,
        format_date: i32,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let c: Contract = from_py(contract, "contract")?;
        req!(self, py, blocking, |ib| async move {
            ib.req_head_time_stamp(&c, &what_to_show, use_rth, format_date)
                .await
        })
    }
    fn req_histogram_data(
        &self,
        py: Python<'_>,
        contract: &Bound<'_, PyAny>,
        use_rth: bool,
        period: String,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let c: Contract = from_py(contract, "contract")?;
        req!(self, py, blocking, |ib| async move {
            ib.req_histogram_data(&c, use_rth, &period).await
        })
    }
    fn what_if_order(
        &self,
        py: Python<'_>,
        contract: &Bound<'_, PyAny>,
        order: &Bound<'_, PyAny>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let c: Contract = from_py(contract, "contract")?;
        let o: Order = from_py(order, "order")?;
        req!(self, py, blocking, |ib| async move {
            ib.what_if_order(&c, &o).await
        })
    }
    fn req_open_orders(
        &self,
        py: Python<'_>,
        all_clients: bool,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let ib = self.ib.clone();
        run(py, blocking, async move {
            let ts = if all_clients {
                ib.req_all_open_orders().await
            } else {
                ib.req_open_orders().await
            }
            .map_err(err)?;
            Python::attach(|py| {
                Ok(PyList::new(py, ts.into_iter().map(TradeCore))?
                    .into_any()
                    .unbind())
            })
        })
    }
    fn req_completed_orders(
        &self,
        py: Python<'_>,
        api_only: bool,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let ib = self.ib.clone();
        run(py, blocking, async move {
            let ts = ib.req_completed_orders(api_only).await.map_err(err)?;
            Python::attach(|py| {
                Ok(PyList::new(py, ts.into_iter().map(TradeCore))?
                    .into_any()
                    .unbind())
            })
        })
    }
    fn req_executions(
        &self,
        py: Python<'_>,
        filter: &Bound<'_, PyAny>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let f: ExecutionFilter = from_py(filter, "execFilter")?;
        req!(self, py, blocking, |ib| async move {
            ib.req_executions(&f).await
        })
    }
    fn req_positions(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        req!(
            self,
            py,
            blocking,
            |ib| async move { ib.req_positions().await }
        )
    }
    fn req_account_updates(
        &self,
        py: Python<'_>,
        account: String,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        req!(self, py, blocking, |ib| async move {
            ib.req_account_updates(&account).await
        })
    }
    fn req_account_updates_multi(
        &self,
        py: Python<'_>,
        account: String,
        model_code: String,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        req!(self, py, blocking, |ib| async move {
            ib.req_account_updates_multi(&account, &model_code).await
        })
    }
    fn req_account_summary(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        req!(self, py, blocking, |ib| async move {
            ib.req_account_summary().await
        })
    }
    fn req_matching_symbols(
        &self,
        py: Python<'_>,
        pattern: String,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        req!(self, py, blocking, |ib| async move {
            ib.req_matching_symbols(&pattern).await
        })
    }
    fn req_market_rule(
        &self,
        py: Python<'_>,
        market_rule_id: i32,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        req!(self, py, blocking, |ib| async move {
            ib.req_market_rule(market_rule_id).await
        })
    }
    fn req_sec_def_opt_params(
        &self,
        py: Python<'_>,
        underlying_symbol: String,
        fut_fop_exchange: String,
        underlying_sec_type: String,
        underlying_con_id: i32,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        req!(self, py, blocking, |ib| async move {
            ib.req_sec_def_opt_params(
                &underlying_symbol,
                &fut_fop_exchange,
                &underlying_sec_type,
                underlying_con_id,
            )
            .await
        })
    }
    fn req_mkt_depth_exchanges(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        req!(self, py, blocking, |ib| async move {
            ib.req_mkt_depth_exchanges().await
        })
    }
    fn req_smart_components(
        &self,
        py: Python<'_>,
        bbo_exchange: String,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        req!(self, py, blocking, |ib| async move {
            ib.req_smart_components(&bbo_exchange).await
        })
    }
    fn req_scanner_data(
        &self,
        py: Python<'_>,
        subscription: &Bound<'_, PyAny>,
        options: &Bound<'_, PyAny>,
        filter_options: &Bound<'_, PyAny>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let s: ScannerSubscription = from_py(subscription, "subscription")?;
        let o: Vec<TagValue> = from_py(options, "scannerSubscriptionOptions")?;
        let f: Vec<TagValue> = from_py(filter_options, "scannerSubscriptionFilterOptions")?;
        req!(self, py, blocking, |ib| async move {
            ib.req_scanner_data(&s, &o, &f).await
        })
    }
    fn req_scanner_parameters(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        req!(self, py, blocking, |ib| async move {
            ib.req_scanner_parameters().await
        })
    }
    fn calculate_implied_volatility(
        &self,
        py: Python<'_>,
        contract: &Bound<'_, PyAny>,
        option_price: f64,
        under_price: f64,
        options: &Bound<'_, PyAny>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let c: Contract = from_py(contract, "contract")?;
        let o: Vec<TagValue> = from_py(options, "implVolOptions")?;
        req!(self, py, blocking, |ib| async move {
            ib.calculate_implied_volatility(&c, option_price, under_price, &o)
                .await
        })
    }
    fn calculate_option_price(
        &self,
        py: Python<'_>,
        contract: &Bound<'_, PyAny>,
        volatility: f64,
        under_price: f64,
        options: &Bound<'_, PyAny>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let c: Contract = from_py(contract, "contract")?;
        let o: Vec<TagValue> = from_py(options, "optPrcOptions")?;
        req!(self, py, blocking, |ib| async move {
            ib.calculate_option_price(&c, volatility, under_price, &o)
                .await
        })
    }
    fn req_news_providers(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        req!(self, py, blocking, |ib| async move {
            ib.req_news_providers().await
        })
    }
    fn req_news_article(
        &self,
        py: Python<'_>,
        provider_code: String,
        article_id: String,
        options: &Bound<'_, PyAny>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let o: Vec<TagValue> = from_py(options, "newsArticleOptions")?;
        req!(self, py, blocking, |ib| async move {
            ib.req_news_article(&provider_code, &article_id, &o).await
        })
    }
    #[pyo3(signature = (con_id, provider_codes, start_date_time, end_date_time, total_results, options, blocking))]
    fn req_historical_news(
        &self,
        py: Python<'_>,
        con_id: i32,
        provider_codes: String,
        start_date_time: String,
        end_date_time: String,
        total_results: i32,
        options: &Bound<'_, PyAny>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let o: Vec<TagValue> = from_py(options, "historicalNewsOptions")?;
        req!(self, py, blocking, |ib| async move {
            ib.req_historical_news(
                con_id,
                &provider_codes,
                &start_date_time,
                &end_date_time,
                total_results,
                &o,
            )
            .await
        })
    }
    fn req_fundamental_data(
        &self,
        py: Python<'_>,
        contract: &Bound<'_, PyAny>,
        report_type: String,
        options: &Bound<'_, PyAny>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let c: Contract = from_py(contract, "contract")?;
        let o: Vec<TagValue> = from_py(options, "fundamentalDataOptions")?;
        req!(self, py, blocking, |ib| async move {
            ib.req_fundamental_data(&c, &report_type, &o).await
        })
    }
    fn request_fa(&self, py: Python<'_>, fa_data_type: i32, blocking: bool) -> PyResult<Py<PyAny>> {
        req!(self, py, blocking, |ib| async move {
            ib.request_fa(fa_data_type).await
        })
    }
    fn replace_fa(
        &self,
        py: Python<'_>,
        fa_data_type: i32,
        xml: String,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        req!(self, py, blocking, |ib| async move {
            ib.replace_fa(fa_data_type, &xml).await
        })
    }
    fn get_wsh_meta_data(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        req!(self, py, blocking, |ib| async move {
            ib.get_wsh_meta_data().await
        })
    }
    fn get_wsh_event_data(
        &self,
        py: Python<'_>,
        data: &Bound<'_, PyAny>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let d: WshEventData = from_py(data, "WshEventData")?;
        req!(self, py, blocking, |ib| async move {
            ib.get_wsh_event_data(&d).await
        })
    }
    fn req_config(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        req!(
            self,
            py,
            blocking,
            |ib| async move { ib.req_config().await }
        )
    }
    fn update_config(
        &self,
        py: Python<'_>,
        request: &Bound<'_, PyAny>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let request: serde_json::Value = from_py(request, "updateConfig request")?;
        req!(self, py, blocking, |ib| async move {
            ib.update_config(&request).await
        })
    }
    fn req_user_info(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        req!(
            self,
            py,
            blocking,
            |ib| async move { ib.req_user_info().await }
        )
    }
    fn req_current_time(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        req!(self, py, blocking, |ib| async move {
            ib.req_current_time().await
        })
    }
    fn req_current_time_in_millis(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        req!(self, py, blocking, |ib| async move {
            ib.req_current_time_in_millis().await
        })
    }
    fn req_family_codes(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        req!(self, py, blocking, |ib| async move {
            ib.req_family_codes().await
        })
    }
    fn req_soft_dollar_tiers(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        req!(self, py, blocking, |ib| async move {
            ib.req_soft_dollar_tiers().await
        })
    }
}

// ---- low-level client ------------------------------------------------------------

/// Convert one decoded message to `(variant, payload)`; unit variants get `{}`.
fn incoming_to_py(py: Python<'_>, m: &Incoming) -> PyResult<Py<PyAny>> {
    let (name, payload) = match serde_json::to_value(m)
        .map_err(|e| InvalidArgumentError::new_err(e.to_string()))?
    {
        serde_json::Value::Object(map) => map.into_iter().next().unwrap_or_default(),
        serde_json::Value::String(name) => (name, serde_json::Value::Object(Default::default())),
        other => ("Unknown".to_string(), other),
    };
    Ok(PyTuple::new(
        py,
        [
            name.into_pyobject(py)?.into_any().unbind(),
            to_py(py, &payload)?,
        ],
    )?
    .into_any()
    .unbind())
}

type MessageRx = Arc<tokio::sync::Mutex<ibkit::tws::client::EventReceiver>>;

/// Low-level TWS client (IB's `EClient`): one method per request, messages
/// as `(variant, dict)` tuples.
#[pyclass(frozen, module = "ibkit._ibkit")]
pub struct ClientCore {
    client: ibkit::tws::Client,
    /// `None` when the connection belongs to an `IB` (messages go to the `IB`).
    messages: Option<MessageRx>,
}

fn tws_err(e: ibkit::tws::Error) -> PyErr {
    err(ibkit::Error::Tws(e))
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct ClientArgs {
    host: String,
    port: u16,
    client_id: i32,
    timeout: Option<f64>,
    connect_options: String,
    optional_capabilities: String,
    max_requests: Option<u32>,
    requests_interval: Option<f64>,
}

macro_rules! client_core {
    ($( $py:literal fn $name:ident ( $( $kind:ident $arg:ident : $ty:ty ),* ); )*) => {
        #[pymethods]
        impl ClientCore {
            #[staticmethod]
            fn connect(py: Python<'_>, args: &Bound<'_, PyAny>, blocking: bool) -> PyResult<Py<PyAny>> {
                let a: ClientArgs = from_py(args, "connect arguments")?;
                let mut opts = ConnectOptions::new(&a.host, a.port, a.client_id);
                if let Some(t) = a.timeout.filter(|t| *t > 0.0) {
                    opts.timeout = Duration::from_secs_f64(t);
                }
                if let Some(m) = a.max_requests {
                    opts.max_requests = m;
                }
                if let Some(i) = a.requests_interval {
                    opts.requests_interval = Duration::from_secs_f64(i);
                }
                opts.connect_options = a.connect_options;
                opts.optional_capabilities = a.optional_capabilities;
                run(py, blocking, async move {
                    let (client, rx) = ibkit::tws::Client::connect(opts).await.map_err(tws_err)?;
                    let messages = Some(Arc::new(tokio::sync::Mutex::new(rx)));
                    Python::attach(|py| Ok(Py::new(py, ClientCore { client, messages })?.into_any()))
                })
            }

            fn is_connected(&self) -> bool {
                self.client.is_connected()
            }
            fn disconnect(&self) {
                self.client.disconnect();
            }
            fn server_version(&self) -> i32 {
                self.client.server_version()
            }
            fn client_id(&self) -> i32 {
                self.client.client_id()
            }
            fn connection_time(&self) -> String {
                self.client.connection_time().to_string()
            }
            fn next_req_id(&self) -> i32 {
                self.client.next_req_id()
            }
            fn update_req_id(&self, min_id: i32) {
                self.client.update_req_id(min_id)
            }
            fn managed_accounts(&self) -> Vec<String> {
                self.client.managed_accounts()
            }
            #[pyo3(name = "reqCurrentTimeInMillis")]
            fn req_current_time_in_millis(&self) -> PyResult<()> {
                self.client.req_current_time_in_millis().map_err(tws_err)
            }

            /// Wait up to `timeout` seconds; return every message available.
            fn poll(&self, py: Python<'_>, timeout: f64) -> PyResult<Py<PyAny>> {
                let rx = self.messages.clone().ok_or_else(|| {
                    InvalidArgumentError::new_err("this client shares an IB connection; its messages arrive through the IB events")
                })?;
                let wait = Duration::from_secs_f64(timeout.max(0.0));
                let batch = py.detach(|| {
                    get_runtime().block_on(async move {
                        let batch = async {
                            let mut rx = rx.lock().await;
                            let mut out = Vec::new();
                            if let Some(first) = rx.recv().await {
                                out.push(first);
                                while let Some(m) = rx.try_recv() {
                                    out.push(m);
                                }
                            }
                            out
                        };
                        tokio::time::timeout(wait, batch).await.unwrap_or_default()
                    })
                });
                let list = PyList::empty(py);
                for m in &batch {
                    list.append(incoming_to_py(py, m)?)?;
                }
                Ok(list.into_any().unbind())
            }

            $(
                #[pyo3(name = $py)]
                fn $name(&self, $( $arg: &Bound<'_, PyAny> ),*) -> PyResult<()> {
                    $( let $arg: $ty = from_py($arg, stringify!($arg))?; )*
                    self.client.$name($( client_core!(@pass $kind $arg) ),*).map_err(tws_err)
                }
            )*
        }
    };
    (@pass val $a:ident) => { $a };
    (@pass ref $a:ident) => { &$a };
}

client_core! {
    "reqMktData" fn req_mkt_data(val req_id: i32, ref contract: Contract, ref generic_tick_list: String, val snapshot: bool, val regulatory_snapshot: bool, ref options: Vec<TagValue>);
    "cancelMktData" fn cancel_mkt_data(val req_id: i32);
    "reqOpenOrders" fn req_open_orders();
    "reqAccountUpdates" fn req_account_updates(val subscribe: bool, ref acct_code: String);
    "reqIds" fn req_ids(val num_ids: i32);
    "reqMktDepth" fn req_mkt_depth(val req_id: i32, ref contract: Contract, val num_rows: i32, val is_smart_depth: bool, ref options: Vec<TagValue>);
    "cancelMktDepth" fn cancel_mkt_depth(val req_id: i32, val is_smart_depth: bool);
    "reqNewsBulletins" fn req_news_bulletins(val all_msgs: bool);
    "cancelNewsBulletins" fn cancel_news_bulletins();
    "setServerLogLevel" fn set_server_log_level(val log_level: i32);
    "reqAutoOpenOrders" fn req_auto_open_orders(val auto_bind: bool);
    "reqAllOpenOrders" fn req_all_open_orders();
    "reqManagedAccts" fn req_managed_accts();
    "reqHistoricalData" fn req_historical_data(val req_id: i32, ref contract: Contract, ref end_date_time: String, ref duration_str: String, ref bar_size_setting: String, ref what_to_show: String, val use_rth: bool, val format_date: i32, val keep_up_to_date: bool, ref chart_options: Vec<TagValue>);
    "reqScannerSubscription" fn req_scanner_subscription(val req_id: i32, ref subscription: ScannerSubscription, ref options: Vec<TagValue>, ref filter_options: Vec<TagValue>);
    "cancelScannerSubscription" fn cancel_scanner_subscription(val req_id: i32);
    "reqScannerParameters" fn req_scanner_parameters();
    "cancelHistoricalData" fn cancel_historical_data(val req_id: i32);
    "reqCurrentTime" fn req_current_time();
    "reqRealTimeBars" fn req_real_time_bars(val req_id: i32, ref contract: Contract, val bar_size: i32, ref what_to_show: String, val use_rth: bool, ref options: Vec<TagValue>);
    "cancelRealTimeBars" fn cancel_real_time_bars(val req_id: i32);
    "reqFundamentalData" fn req_fundamental_data(val req_id: i32, ref contract: Contract, ref report_type: String, ref options: Vec<TagValue>);
    "cancelFundamentalData" fn cancel_fundamental_data(val req_id: i32);
    "calculateImpliedVolatility" fn calculate_implied_volatility(val req_id: i32, ref contract: Contract, val option_price: f64, val under_price: f64, ref options: Vec<TagValue>);
    "calculateOptionPrice" fn calculate_option_price(val req_id: i32, ref contract: Contract, val volatility: f64, val under_price: f64, ref options: Vec<TagValue>);
    "cancelCalculateImpliedVolatility" fn cancel_calculate_implied_volatility(val req_id: i32);
    "cancelCalculateOptionPrice" fn cancel_calculate_option_price(val req_id: i32);
    "reqMarketDataType" fn req_market_data_type(val market_data_type: i32);
    "reqPositions" fn req_positions();
    "cancelPositions" fn cancel_positions();
    "reqAccountSummary" fn req_account_summary(val req_id: i32, ref group_name: String, ref tags: String);
    "cancelAccountSummary" fn cancel_account_summary(val req_id: i32);
    "verifyRequest" fn verify_request(ref api_name: String, ref api_version: String);
    "verifyMessage" fn verify_message(ref api_data: String);
    "verifyAndAuthRequest" fn verify_and_auth_request(ref api_name: String, ref api_version: String, ref opaque_isv_key: String);
    "verifyAndAuthMessage" fn verify_and_auth_message(ref api_data: String, ref xyz_response: String);
    "queryDisplayGroups" fn query_display_groups(val req_id: i32);
    "subscribeToGroupEvents" fn subscribe_to_group_events(val req_id: i32, val group_id: i32);
    "updateDisplayGroup" fn update_display_group(val req_id: i32, ref contract_info: String);
    "unsubscribeFromGroupEvents" fn unsubscribe_from_group_events(val req_id: i32);
    "reqPositionsMulti" fn req_positions_multi(val req_id: i32, ref account: String, ref model_code: String);
    "cancelPositionsMulti" fn cancel_positions_multi(val req_id: i32);
    "reqAccountUpdatesMulti" fn req_account_updates_multi(val req_id: i32, ref account: String, ref model_code: String, val ledger_and_nlv: bool);
    "cancelAccountUpdatesMulti" fn cancel_account_updates_multi(val req_id: i32);
    "reqSecDefOptParams" fn req_sec_def_opt_params(val req_id: i32, ref underlying_symbol: String, ref fut_fop_exchange: String, ref underlying_sec_type: String, val underlying_con_id: i32);
    "reqSoftDollarTiers" fn req_soft_dollar_tiers(val req_id: i32);
    "reqFamilyCodes" fn req_family_codes();
    "reqMatchingSymbols" fn req_matching_symbols(val req_id: i32, ref pattern: String);
    "reqMktDepthExchanges" fn req_mkt_depth_exchanges();
    "reqSmartComponents" fn req_smart_components(val req_id: i32, ref bbo_exchange: String);
    "reqNewsArticle" fn req_news_article(val req_id: i32, ref provider_code: String, ref article_id: String, ref options: Vec<TagValue>);
    "reqNewsProviders" fn req_news_providers();
    "reqHistoricalNews" fn req_historical_news(val req_id: i32, val con_id: i32, ref provider_codes: String, ref start_date_time: String, ref end_date_time: String, val total_results: i32, ref options: Vec<TagValue>);
    "reqHeadTimeStamp" fn req_head_time_stamp(val req_id: i32, ref contract: Contract, ref what_to_show: String, val use_rth: bool, val format_date: i32);
    "cancelHeadTimeStamp" fn cancel_head_time_stamp(val req_id: i32);
    "reqHistogramData" fn req_histogram_data(val req_id: i32, ref contract: Contract, val use_rth: bool, ref time_period: String);
    "cancelHistogramData" fn cancel_histogram_data(val req_id: i32);
    "reqMarketRule" fn req_market_rule(val market_rule_id: i32);
    "reqPnL" fn req_pnl(val req_id: i32, ref account: String, ref model_code: String);
    "cancelPnL" fn cancel_pnl(val req_id: i32);
    "reqPnLSingle" fn req_pnl_single(val req_id: i32, ref account: String, ref model_code: String, val con_id: i32);
    "cancelPnLSingle" fn cancel_pnl_single(val req_id: i32);
    "reqHistoricalTicks" fn req_historical_ticks(val req_id: i32, ref contract: Contract, ref start_date_time: String, ref end_date_time: String, val number_of_ticks: i32, ref what_to_show: String, val use_rth: bool, val ignore_size: bool, ref misc_options: Vec<TagValue>);
    "reqTickByTickData" fn req_tick_by_tick_data(val req_id: i32, ref contract: Contract, ref tick_type: String, val number_of_ticks: i32, val ignore_size: bool);
    "cancelTickByTickData" fn cancel_tick_by_tick_data(val req_id: i32);
    "reqCompletedOrders" fn req_completed_orders(val api_only: bool);
    "reqWshMetaData" fn req_wsh_meta_data(val req_id: i32);
    "cancelWshMetaData" fn cancel_wsh_meta_data(val req_id: i32);
    "cancelWshEventData" fn cancel_wsh_event_data(val req_id: i32);
    "reqUserInfo" fn req_user_info(val req_id: i32);
    "cancelContractData" fn cancel_contract_data(val req_id: i32);
    "cancelHistoricalTicks" fn cancel_historical_ticks(val req_id: i32);
    "reqConfig" fn req_config(val req_id: i32);
    "updateConfig" fn update_config(ref request: serde_json::Value);
    "requestFA" fn request_fa(val fa_data_type: i32);
    "replaceFA" fn replace_fa(val req_id: i32, val fa_data_type: i32, ref xml: String);
    "cancelOrder" fn cancel_order(val order_id: i32, ref order_cancel: OrderCancel);
    "reqContractDetails" fn req_contract_details(val req_id: i32, ref contract: Contract);
    "reqGlobalCancel" fn req_global_cancel(ref order_cancel: OrderCancel);
    "reqWshEventData" fn req_wsh_event_data(val req_id: i32, ref data: WshEventData);
    "placeOrder" fn place_order(val order_id: i32, ref contract: Contract, ref order: Order);
    "reqExecutions" fn req_executions(val req_id: i32, ref filter: ExecutionFilter);
    "exerciseOptions" fn exercise_options(val req_id: i32, ref contract: Contract, val exercise_action: i32, val exercise_quantity: i32, ref account: String, val override_: i32, ref manual_order_time: String, ref customer_account: String, val professional_customer: bool);

}

// ---- Web API -----------------------------------------------------------------

/// Web API client (`ibkit.web.WebClient` wraps it).
#[pyclass(frozen, module = "ibkit._ibkit")]
pub struct WebCore(Arc<ibkit_web::WebClient>);

fn web_err(e: ibkit_web::WebError) -> PyErr {
    kind_err(e.kind(), e.to_string())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct OAuthArgs {
    consumer_key: String,
    access_token: String,
    access_token_secret: String,
    encryption_key: String,
    signature_key: String,
    dh_prime: String,
    #[serde(default)]
    dh_generator: Option<u32>,
    #[serde(default)]
    realm: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct OAuth2Args {
    client_id: String,
    key_id: String,
    credential: String,
    private_key: String,
    ip: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    auth_url: Option<String>,
}

#[pymethods]
impl WebCore {
    #[new]
    #[pyo3(signature = (base_url, accept_invalid_certs, timeout, oauth=None, oauth2=None, pacing=true))]
    fn new(
        base_url: String,
        accept_invalid_certs: bool,
        timeout: f64,
        oauth: Option<&Bound<'_, PyAny>>,
        oauth2: Option<&Bound<'_, PyAny>>,
        pacing: bool,
    ) -> PyResult<Self> {
        let oauth = match oauth {
            Some(o) if !o.is_none() => {
                let a: OAuthArgs = from_py(o, "oauth settings")?;
                let mut cfg = ibkit_web::OAuthConfig::new(
                    &a.consumer_key,
                    &a.access_token,
                    &a.access_token_secret,
                    &a.encryption_key,
                    &a.signature_key,
                    &a.dh_prime,
                );
                if let Some(g) = a.dh_generator {
                    cfg.dh_generator = g;
                }
                if let Some(r) = a.realm {
                    cfg.realm = r;
                }
                Some(cfg)
            }
            _ => None,
        };
        let oauth2 = match oauth2 {
            Some(o) if !o.is_none() => {
                let a: OAuth2Args = from_py(o, "oauth2 settings")?;
                let mut cfg = ibkit_web::OAuth2Config::new(
                    &a.client_id,
                    &a.key_id,
                    &a.credential,
                    &a.private_key,
                    &a.ip,
                );
                if let Some(scope) = a.scope {
                    cfg.scope = scope;
                }
                if let Some(url) = a.auth_url {
                    cfg.auth_url = url;
                }
                Some(cfg)
            }
            _ => None,
        };
        let cfg = ibkit_web::WebConfig {
            base_url,
            accept_invalid_certs,
            timeout: Duration::from_secs_f64(timeout),
            oauth,
            oauth2,
            pacing,
        };
        Ok(Self(Arc::new(
            ibkit_web::WebClient::new(cfg).map_err(web_err)?,
        )))
    }

    /// Send `method` to `path` with optional query pairs and JSON body.
    #[pyo3(signature = (method, path, query=None, body=None, blocking=true))]
    fn request(
        &self,
        py: Python<'_>,
        method: &str,
        path: String,
        query: Option<&Bound<'_, PyAny>>,
        body: Option<&Bound<'_, PyAny>>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let method = ibkit_web::client::parse_method(method).map_err(web_err)?;
        let query: Vec<(String, String)> = match query {
            Some(q) if !q.is_none() => {
                let map: serde_json::Map<String, serde_json::Value> = from_py(q, "query")?;
                map.into_iter()
                    .filter(|(_, v)| !v.is_null())
                    .map(|(k, v)| {
                        (
                            k,
                            if let serde_json::Value::String(s) = v {
                                s
                            } else {
                                v.to_string()
                            },
                        )
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        let body: Option<serde_json::Value> = match body {
            Some(b) if !b.is_none() => Some(from_py(b, "body")?),
            _ => None,
        };
        let web = self.0.clone();
        run(py, blocking, async move {
            let v = web
                .request(method, &path, &query, body.as_ref())
                .await
                .map_err(web_err)?;
            attach_py(&v)
        })
    }

    /// Send `method` to a `/gw/api` path (Account Management and other
    /// OAuth 2.0 endpoints): access token, and a signed JWT body.
    #[pyo3(signature = (method, path, query=None, body=None, blocking=true))]
    fn gw_request(
        &self,
        py: Python<'_>,
        method: &str,
        path: String,
        query: Option<&Bound<'_, PyAny>>,
        body: Option<&Bound<'_, PyAny>>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let method = ibkit_web::client::parse_method(method).map_err(web_err)?;
        let query: Vec<(String, String)> = match query {
            Some(q) if !q.is_none() => {
                let map: serde_json::Map<String, serde_json::Value> = from_py(q, "query")?;
                map.into_iter()
                    .filter(|(_, v)| !v.is_null())
                    .map(|(k, v)| match v {
                        serde_json::Value::String(s) => (k, s),
                        other => (k, other.to_string()),
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        let body: Option<serde_json::Value> = match body {
            Some(b) if !b.is_none() => Some(from_py(b, "body")?),
            _ => None,
        };
        let web = self.0.clone();
        run(py, blocking, async move {
            let v = web
                .gw_request(method, &path, &query, body.as_ref())
                .await
                .map_err(web_err)?;
            attach_py(&v)
        })
    }

    #[pyo3(signature = (blocking=true))]
    fn oauth_request_token(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        let web = self.0.clone();
        run(py, blocking, async move {
            let token = web.oauth_request_token().await.map_err(web_err)?;
            Python::attach(|py| to_py(py, &token))
        })
    }

    #[pyo3(signature = (request_token, verifier, blocking=true))]
    fn oauth_access_token(
        &self,
        py: Python<'_>,
        request_token: String,
        verifier: String,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let web = self.0.clone();
        run(py, blocking, async move {
            let v = web
                .oauth_access_token(&request_token, &verifier)
                .await
                .map_err(web_err)?;
            attach_py(&v)
        })
    }

    #[pyo3(signature = (compete=true, blocking=true))]
    fn oauth_init(&self, py: Python<'_>, compete: bool, blocking: bool) -> PyResult<Py<PyAny>> {
        let web = self.0.clone();
        run(py, blocking, async move {
            let v = web.oauth_init(compete).await.map_err(web_err)?;
            attach_py(&v)
        })
    }

    #[pyo3(signature = (compete=true, blocking=true))]
    fn oauth2_init(&self, py: Python<'_>, compete: bool, blocking: bool) -> PyResult<Py<PyAny>> {
        let web = self.0.clone();
        run(py, blocking, async move {
            let v = web.oauth2_init(compete).await.map_err(web_err)?;
            attach_py(&v)
        })
    }

    fn live_session_token(&self) -> Option<String> {
        self.0.live_session_token()
    }
}

// ---- Web API WebSocket ---------------------------------------------------------

/// Web API WebSocket (`ibkit.web.WsClient` wraps it).
#[pyclass(frozen, module = "ibkit._ibkit")]
pub struct WsCore {
    sender: ibkit_web::WsSender,
    client: Arc<tokio::sync::Mutex<ibkit_web::WsClient>>,
}

fn ws_err(e: ibkit_web::ws::WsError) -> PyErr {
    kind_err(e.kind(), e.to_string())
}

fn secs(v: Option<f64>) -> Option<Duration> {
    v.filter(|s| *s > 0.0).map(Duration::from_secs_f64)
}

#[pymethods]
impl WsCore {
    #[staticmethod]
    #[pyo3(signature = (url, session=None, oauth_token=None, bearer_token=None, accept_invalid_certs=false, heartbeat=Some(30.0), confirm_timeout=Some(10.0), renew_market_data=Some(600.0), blocking=true))]
    fn connect(
        py: Python<'_>,
        url: String,
        session: Option<String>,
        oauth_token: Option<String>,
        bearer_token: Option<String>,
        accept_invalid_certs: bool,
        heartbeat: Option<f64>,
        confirm_timeout: Option<f64>,
        renew_market_data: Option<f64>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let auth = ibkit_web::WsAuth {
            session,
            oauth_token,
            bearer_token,
            accept_invalid_certs,
            heartbeat: secs(heartbeat),
            confirm_timeout: secs(confirm_timeout),
            renew_market_data: secs(renew_market_data),
        };
        run(py, blocking, async move {
            let ws = ibkit_web::WsClient::connect(&url, auth)
                .await
                .map_err(ws_err)?;
            let core = WsCore {
                sender: ws.sender(),
                client: Arc::new(tokio::sync::Mutex::new(ws)),
            };
            Python::attach(|py| Ok(Py::new(py, core)?.into_any()))
        })
    }

    fn send(&self, text: String) -> PyResult<()> {
        self.sender.send(text).map_err(ws_err)
    }
    fn subscribe_market_data(&self, conid: i64, fields: Vec<String>) -> PyResult<()> {
        self.sender
            .subscribe_market_data(
                conid,
                &fields.iter().map(String::as_str).collect::<Vec<_>>(),
            )
            .map_err(ws_err)
    }
    fn unsubscribe_market_data(&self, conid: i64) -> PyResult<()> {
        self.sender.unsubscribe_market_data(conid).map_err(ws_err)
    }
    #[pyo3(signature = (filters=Vec::new()))]
    fn subscribe_orders(&self, filters: Vec<String>) -> PyResult<()> {
        let filters: Vec<&str> = filters.iter().map(String::as_str).collect();
        self.sender.subscribe_orders(&filters).map_err(ws_err)
    }
    fn exercise_options(&self, conid: i64) -> PyResult<()> {
        self.sender.exercise_options(conid).map_err(ws_err)
    }
    fn subscribe_pnl(&self) -> PyResult<()> {
        self.sender.subscribe_pnl().map_err(ws_err)
    }
    #[pyo3(signature = (realtime_updates_only=false, days=None))]
    fn subscribe_trades(&self, realtime_updates_only: bool, days: Option<u8>) -> PyResult<()> {
        self.sender
            .subscribe_trades(realtime_updates_only, days)
            .map_err(ws_err)
    }
    fn subscribe_account_summary(&self, account_id: &str) -> PyResult<()> {
        self.sender
            .subscribe_account_summary(account_id)
            .map_err(ws_err)
    }
    fn subscribe_account_ledger(&self, account_id: &str) -> PyResult<()> {
        self.sender
            .subscribe_account_ledger(account_id)
            .map_err(ws_err)
    }
    #[pyo3(signature = (account_id, conid, exchange=None))]
    fn subscribe_book_trader(
        &self,
        account_id: &str,
        conid: i64,
        exchange: Option<String>,
    ) -> PyResult<()> {
        self.sender
            .subscribe_book_trader(account_id, conid, exchange.as_deref())
            .map_err(ws_err)
    }

    /// Next message as `(topic, args, data)`; `None` on timeout or close.
    #[pyo3(signature = (timeout=None, blocking=true))]
    fn recv(&self, py: Python<'_>, timeout: Option<f64>, blocking: bool) -> PyResult<Py<PyAny>> {
        let client = self.client.clone();
        run(py, blocking, async move {
            let mut ws = client.lock().await;
            let m = match timeout {
                Some(t) => tokio::time::timeout(Duration::from_secs_f64(t.max(0.0)), ws.recv())
                    .await
                    .ok()
                    .flatten(),
                None => ws.recv().await,
            };
            Python::attach(|py| match m {
                Some(m) => Ok(PyTuple::new(
                    py,
                    [
                        to_py(py, &m.topic)?,
                        to_py(py, &m.args)?,
                        to_py(py, &m.data)?,
                    ],
                )?
                .into_any()
                .unbind()),
                None => Ok(py.None()),
            })
        })
    }
}

// ---- FIX ---------------------------------------------------------------------

/// FIX initiator session (`ibkit.fix.FixSession` wraps it).
#[pyclass(frozen, module = "ibkit._ibkit")]
pub struct FixCore {
    handle: ibkit_fix::FixHandle,
    events: Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<ibkit_fix::FixEvent>>>,
}

fn fix_message(msg_type: &str, fields: Vec<(u32, String)>) -> ibkit_fix::Message {
    let mut m = ibkit_fix::Message::new(msg_type);
    m.fields = fields;
    m
}

fn fix_event_to_py(py: Python<'_>, e: ibkit_fix::FixEvent) -> PyResult<Py<PyAny>> {
    let items: Vec<Py<PyAny>> = match e {
        ibkit_fix::FixEvent::App(m) => vec![
            to_py(py, "app")?,
            to_py(py, &m.msg_type)?,
            to_py(py, &m.fields)?,
        ],
        ibkit_fix::FixEvent::Session(s) => {
            vec![to_py(py, "session")?, to_py(py, &format!("{s:?}"))?]
        }
        ibkit_fix::FixEvent::Disconnected(r) => vec![to_py(py, "disconnected")?, to_py(py, &r)?],
    };
    Ok(PyTuple::new(py, items)?.into_any().unbind())
}

#[pymethods]
impl FixCore {
    #[staticmethod]
    #[pyo3(signature = (host, port, sender_comp_id, target_comp_id, heartbeat_interval=30, reset_on_logon=false, seq_file=None, logon_fields=Vec::new(), tls_ca_file=None, tls_server_name=None, max_latency=Some(120.0), compression=false, blocking=true))]
    fn connect(
        py: Python<'_>,
        host: String,
        port: u16,
        sender_comp_id: String,
        target_comp_id: String,
        heartbeat_interval: u64,
        reset_on_logon: bool,
        seq_file: Option<String>,
        logon_fields: Vec<(u32, String)>,
        tls_ca_file: Option<String>,
        tls_server_name: Option<String>,
        max_latency: Option<f64>,
        compression: bool,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        use ibkit_fix::rustls;
        use rustls::pki_types::CertificateDer;
        use rustls::pki_types::pem::PemObject;
        let mut cfg = ibkit_fix::SessionConfig::initiator(&sender_comp_id, &target_comp_id);
        cfg.heartbeat_interval = Duration::from_secs(heartbeat_interval.max(1));
        cfg.reset_on_logon = reset_on_logon;
        cfg.logon_fields = logon_fields;
        cfg.max_latency = max_latency.map(|s| Duration::from_secs_f64(s.max(0.0)));
        cfg.compression = compression;
        let tls = match &tls_ca_file {
            Some(path) => {
                let mut roots = rustls::RootCertStore::empty();
                let certs = CertificateDer::pem_file_iter(path)
                    .and_then(|it| it.collect::<std::result::Result<Vec<_>, _>>())
                    .map_err(|e| {
                        InvalidArgumentError::new_err(format!(
                            "could not read PEM certificates from tlsCaFile={path:?}: {e}; pass a file with the CA or server certificate in PEM form"
                        ))
                    })?;
                for cert in certs {
                    roots.add(cert).map_err(|e| {
                        InvalidArgumentError::new_err(format!(
                            "tlsCaFile={path:?} holds a certificate rustls rejects: {e}"
                        ))
                    })?;
                }
                let provider = Arc::new(rustls::crypto::ring::default_provider());
                let config = rustls::ClientConfig::builder_with_provider(provider)
                    .with_safe_default_protocol_versions()
                    .map_err(|e| IbkitError::new_err(e.to_string()))?
                    .with_root_certificates(roots)
                    .with_no_client_auth();
                Some(Arc::new(config))
            }
            None => None,
        };
        let store: Box<dyn ibkit_fix::SeqStore> = match seq_file {
            Some(p) => Box::new(ibkit_fix::FileStore(p.into())),
            None => Box::new(ibkit_fix::MemoryStore::default()),
        };
        run(py, blocking, async move {
            let addr = format!("{host}:{port}");
            let connected = match tls {
                Some(tls) => {
                    let name = tls_server_name.unwrap_or_else(|| host.clone());
                    ibkit_fix::connect_tls(&addr, &name, tls, cfg, store).await
                }
                None => ibkit_fix::connect(&addr, cfg, store).await,
            };
            let (handle, rx) = connected.map_err(|e| {
                ConnectionError::new_err(format!(
                    "could not connect to the FIX gateway at {host}:{port}: {e}"
                ))
            })?;
            Python::attach(|py| {
                Ok(Py::new(
                    py,
                    FixCore {
                        handle,
                        events: Arc::new(tokio::sync::Mutex::new(rx)),
                    },
                )?
                .into_any())
            })
        })
    }

    fn send(&self, msg_type: &str, fields: Vec<(u32, String)>) -> PyResult<()> {
        self.handle
            .send(fix_message(msg_type, fields))
            .map_err(|e| ConnectionError::new_err(e.to_string()))
    }

    fn logout(&self, text: &str) -> PyResult<()> {
        self.handle
            .logout(text)
            .map_err(|e| ConnectionError::new_err(e.to_string()))
    }

    /// Wait up to `timeout` seconds for the next event; `None` on timeout.
    #[pyo3(signature = (timeout, blocking=true))]
    fn poll(&self, py: Python<'_>, timeout: f64, blocking: bool) -> PyResult<Py<PyAny>> {
        let rx = self.events.clone();
        run(py, blocking, async move {
            let got = tokio::time::timeout(Duration::from_secs_f64(timeout.max(0.0)), async {
                rx.lock().await.recv().await
            })
            .await
            .ok()
            .flatten();
            Python::attach(|py| match got {
                Some(e) => fix_event_to_py(py, e),
                None => Ok(py.None()),
            })
        })
    }
}

/// IB order tags without a TWS `Order` field (`ibkit_fix::ib::OrderExtras`).
#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct FixExtras {
    parent_cl_ord_id: Option<String>,
    sender_location_id: Option<String>,
    allocations: Vec<(String, f64)>,
    disable_fa_close_first: bool,
    strict_per_account_allocation: bool,
    delta_hedge: bool,
    fx_conversion: bool,
    use_net_price: bool,
    no_dark_pools: bool,
    staged: bool,
}

/// NewOrderSingle fields built from a contract, an order and optional extras.
#[pyfunction]
#[pyo3(signature = (cl_ord_id, contract, order, extras=None))]
fn fix_new_order_single(
    cl_ord_id: &str,
    contract: &Bound<'_, PyAny>,
    order: &Bound<'_, PyAny>,
    extras: Option<&Bound<'_, PyAny>>,
) -> PyResult<Vec<(u32, String)>> {
    let c: Contract = from_py(contract, "contract")?;
    let o: Order = from_py(order, "order")?;
    let x: FixExtras = match extras {
        Some(e) if !e.is_none() => from_py(e, "extras")?,
        _ => FixExtras::default(),
    };
    let allocations: Vec<(&str, f64)> = x
        .allocations
        .iter()
        .map(|(a, q)| (a.as_str(), *q))
        .collect();
    let extras = ibkit_fix::ib::OrderExtras {
        parent_cl_ord_id: x.parent_cl_ord_id.as_deref(),
        sender_location_id: x.sender_location_id.as_deref(),
        allocations: &allocations,
        disable_fa_close_first: x.disable_fa_close_first,
        strict_per_account_allocation: x.strict_per_account_allocation,
        delta_hedge: x.delta_hedge,
        fx_conversion: x.fx_conversion,
        use_net_price: x.use_net_price,
        no_dark_pools: x.no_dark_pools,
        staged: x.staged,
    };
    ibkit_fix::ib::new_order_single_with(cl_ord_id, &c, &o, &extras)
        .map(|m| m.fields)
        .map_err(|e| InvalidArgumentError::new_err(e.0))
}

/// Option exercise or lapse request fields (`exercise` "exercise" or "lapse").
#[pyfunction]
fn fix_option_exercise(
    cl_ord_id: &str,
    contract: &Bound<'_, PyAny>,
    order: &Bound<'_, PyAny>,
    exercise: &str,
    revocable: bool,
) -> PyResult<Vec<(u32, String)>> {
    use ibkit_fix::ib::ExerciseType;
    let kind = match exercise {
        "exercise" => ExerciseType::Exercise,
        "lapse" => ExerciseType::Lapse,
        other => {
            return Err(InvalidArgumentError::new_err(format!(
                "exercise is {other:?}; use 'exercise' or 'lapse'"
            )));
        }
    };
    let c: Contract = from_py(contract, "contract")?;
    let o: Order = from_py(order, "order")?;
    ibkit_fix::ib::option_exercise(cl_ord_id, &c, &o, kind, revocable)
        .map(|m| m.fields)
        .map_err(|e| InvalidArgumentError::new_err(e.0))
}

/// OrderCancelReplaceRequest (35=G) fields.
#[pyfunction]
fn fix_cancel_replace(
    orig_cl_ord_id: &str,
    cl_ord_id: &str,
    contract: &Bound<'_, PyAny>,
    order: &Bound<'_, PyAny>,
) -> PyResult<Vec<(u32, String)>> {
    let c: Contract = from_py(contract, "contract")?;
    let o: Order = from_py(order, "order")?;
    ibkit_fix::ib::cancel_replace(orig_cl_ord_id, cl_ord_id, &c, &o)
        .map(|m| m.fields)
        .map_err(|e| InvalidArgumentError::new_err(e.0))
}

/// OrderCancelRequest (35=F) fields.
#[pyfunction]
fn fix_cancel_request(
    orig_cl_ord_id: &str,
    cl_ord_id: &str,
    account: &str,
    contract: &Bound<'_, PyAny>,
    action: &str,
    quantity: f64,
) -> PyResult<Vec<(u32, String)>> {
    let c: Contract = from_py(contract, "contract")?;
    ibkit_fix::ib::cancel_request(orig_cl_ord_id, cl_ord_id, account, &c, action, quantity)
        .map(|m| m.fields)
        .map_err(|e| InvalidArgumentError::new_err(e.0))
}

/// OrderStatusRequest (35=H) fields; `cl_ord_id` "*" asks for all open orders.
#[pyfunction]
fn fix_order_status_request(account: &str, cl_ord_id: &str) -> Vec<(u32, String)> {
    ibkit_fix::ib::order_status_request(account, cl_ord_id).fields
}

/// NewOrderMultileg (35=AB) fields; `legs` are `(contract, ratio, action, openClose)`.
#[pyfunction]
fn fix_new_order_multileg(
    cl_ord_id: &str,
    combo: &Bound<'_, PyAny>,
    legs: Vec<(Bound<'_, PyAny>, f64, String, String)>,
    order: &Bound<'_, PyAny>,
) -> PyResult<Vec<(u32, String)>> {
    let combo: Contract = from_py(combo, "combo contract")?;
    let o: Order = from_py(order, "order")?;
    let contracts = legs
        .iter()
        .map(|(c, _, _, _)| from_py::<Contract>(c, "leg contract"))
        .collect::<PyResult<Vec<_>>>()?;
    let legs: Vec<ibkit_fix::ib::Leg<'_>> = contracts
        .iter()
        .zip(&legs)
        .map(
            |(contract, (_, ratio, action, open_close))| ibkit_fix::ib::Leg {
                contract,
                ratio: *ratio,
                action,
                open_close,
            },
        )
        .collect();
    ibkit_fix::ib::new_order_multileg(cl_ord_id, &combo, &legs, &o)
        .map(|m| m.fields)
        .map_err(|e| InvalidArgumentError::new_err(e.0))
}

/// MultilegCancelReplace (35=AC) fields (quantity and price only).
#[pyfunction]
fn fix_multileg_cancel_replace(
    orig_cl_ord_id: &str,
    cl_ord_id: &str,
    combo: &Bound<'_, PyAny>,
    order: &Bound<'_, PyAny>,
) -> PyResult<Vec<(u32, String)>> {
    let c: Contract = from_py(combo, "combo contract")?;
    let o: Order = from_py(order, "order")?;
    ibkit_fix::ib::multileg_cancel_replace(orig_cl_ord_id, cl_ord_id, &c, &o)
        .map(|m| m.fields)
        .map_err(|e| InvalidArgumentError::new_err(e.0))
}

/// AllocationInstruction (35=J) fields; `trans_type` is "new", "replace" or
/// "cancel", `allocations` are `(clearing broker, quantity)`.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
#[pyo3(signature = (alloc_id, trans_type, ref_alloc_id, account, contract, action, quantity, avg_px, trade_date, allocations))]
fn fix_allocation_instruction(
    alloc_id: &str,
    trans_type: &str,
    ref_alloc_id: Option<&str>,
    account: &str,
    contract: &Bound<'_, PyAny>,
    action: &str,
    quantity: f64,
    avg_px: f64,
    trade_date: &str,
    allocations: Vec<(String, f64)>,
) -> PyResult<Vec<(u32, String)>> {
    use ibkit_fix::ib::AllocTransType;
    let kind = match trans_type {
        "new" => AllocTransType::New,
        "replace" => AllocTransType::Replace,
        "cancel" => AllocTransType::Cancel,
        other => {
            return Err(InvalidArgumentError::new_err(format!(
                "transType is {other:?}; use 'new', 'replace' or 'cancel'"
            )));
        }
    };
    let c: Contract = from_py(contract, "contract")?;
    let allocs: Vec<(&str, f64)> = allocations.iter().map(|(a, q)| (a.as_str(), *q)).collect();
    ibkit_fix::ib::allocation_instruction(
        alloc_id,
        kind,
        ref_alloc_id,
        account,
        &c,
        action,
        quantity,
        avg_px,
        trade_date,
        &allocs,
    )
    .map(|m| m.fields)
    .map_err(|e| InvalidArgumentError::new_err(e.0))
}

// ---- testing -------------------------------------------------------------------

/// In-process fake TWS server for tests (wraps `ibkit_tws::testing::FakeTws`).
#[pyclass(module = "ibkit._ibkit")]
pub struct FakeTws {
    inner: Option<ibkit::tws::testing::FakeTws>,
    port: u16,
}

#[pymethods]
impl FakeTws {
    /// `responder(fields: list[str]) -> list[list[str]]` answers each request.
    #[new]
    #[pyo3(signature = (server_version=178, next_valid_id=1, accounts="DU123456".to_string(), responder=None))]
    fn new(
        py: Python<'_>,
        server_version: i32,
        next_valid_id: i32,
        accounts: String,
        responder: Option<Py<PyAny>>,
    ) -> PyResult<Self> {
        let resp: ibkit::tws::testing::Responder = match responder {
            Some(f) => {
                let f = Arc::new(f);
                Arc::new(move |req: &[String]| {
                    Python::attach(|py| {
                        match f
                            .call1(py, (req.to_vec(),))
                            .and_then(|r| r.extract::<Vec<Vec<String>>>(py))
                        {
                            Ok(v) => v,
                            Err(e) => {
                                e.print(py);
                                Vec::new()
                            }
                        }
                    })
                })
            }
            None => Arc::new(|_: &[String]| Vec::new()),
        };
        let cfg = ibkit::tws::testing::FakeTwsConfig {
            server_version,
            next_valid_id,
            accounts,
        };
        let inner =
            py.detach(|| get_runtime().block_on(ibkit::tws::testing::FakeTws::with(cfg, resp)));
        let port = inner.port();
        Ok(Self {
            inner: Some(inner),
            port,
        })
    }

    #[getter]
    fn port(&self) -> u16 {
        self.port
    }

    /// Fields of a protobuf answer `msg_id` built from the JSON form of IB
    /// message `name`, for example `FakeTws.protobuf(110, 'ConfigResponse', {...})`.
    #[staticmethod]
    fn protobuf(msg_id: i32, name: &str, value: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
        let value: serde_json::Value = from_py(value, "protobuf message")?;
        Ok(ibkit::tws::testing::protobuf(msg_id, |w| {
            ibkit::tws::pb::from_json(w, name, &value)
        }))
    }

    /// JSON form of a protobuf request recorded as `[id, 'protobuf', hex]`.
    #[staticmethod]
    fn parse(py: Python<'_>, name: &str, hex: &str) -> PyResult<Py<PyAny>> {
        let bytes = ibkit::tws::testing::unhex(hex);
        let m = ibkit::tws::pb::PbMsg::parse(&bytes).map_err(|e| {
            InvalidArgumentError::new_err(format!("{hex:?} is not a protobuf message: {e}"))
        })?;
        to_py(py, &ibkit::tws::pb::to_json(name, &m))
    }

    /// Push one message (list of fields) to connected clients.
    fn send(&self, fields: Vec<String>) {
        if let Some(t) = &self.inner {
            t.send(fields);
        }
    }

    fn close(&mut self) {
        self.inner = None;
    }
}

/// Version of the Rust extension.
#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

// ---------------------------------------------------------------------------
// ibkit control API client
// ---------------------------------------------------------------------------

#[pyclass(frozen, module = "ibkit._ibkit")]
pub struct GatewayCore(ibkit_gateway_client::GatewayClient);

fn gateway_err(e: anyhow::Error) -> PyErr {
    if let Some(status) = e.downcast_ref::<ibkit_gateway_client::StatusError>() {
        return GatewayError::new_err((status.status, status.message.clone()));
    }
    let text = format!("{e:#}");
    if e.is::<ibkit_gateway_client::ConnectError>() {
        ConnectionError::new_err(text)
    } else {
        IbkitError::new_err(text)
    }
}

#[pymethods]
impl GatewayCore {
    #[new]
    fn new(url: String, token: String) -> PyResult<Self> {
        ibkit_gateway_client::GatewayClient::new(url, token)
            .map(Self)
            .map_err(|e| InvalidArgumentError::new_err(format!("{e:#}")))
    }

    #[pyo3(signature = (blocking=true))]
    fn health(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        let client = self.0.clone();
        run(py, blocking, async move {
            attach_py(&client.health().await.map_err(gateway_err)?)
        })
    }

    #[pyo3(signature = (blocking=true))]
    fn ready(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        let client = self.0.clone();
        run(py, blocking, async move {
            attach_py(&client.ready().await.map_err(gateway_err)?)
        })
    }

    #[pyo3(signature = (blocking=true))]
    fn status(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        let client = self.0.clone();
        run(py, blocking, async move {
            attach_py(&client.status().await.map_err(gateway_err)?)
        })
    }

    #[pyo3(signature = (blocking=true))]
    fn metrics(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        let client = self.0.clone();
        run(py, blocking, async move {
            attach_py(&client.metrics().await.map_err(gateway_err)?)
        })
    }

    #[pyo3(signature = (blocking=true))]
    fn openapi(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        let client = self.0.clone();
        run(py, blocking, async move {
            attach_py(&client.openapi().await.map_err(gateway_err)?)
        })
    }

    #[pyo3(signature = (control, blocking=true))]
    fn control(&self, py: Python<'_>, control: String, blocking: bool) -> PyResult<Py<PyAny>> {
        let control: ibkit_gateway_client::Control =
            serde_json::from_value(serde_json::Value::String(control.clone())).map_err(|_| {
                InvalidArgumentError::new_err(format!(
                    "You sent the command {control:?}. Use stop, restart, soft_restart, export_logs, reconnect_account, reconnect_data or reload."
                ))
            })?;
        let client = self.0.clone();
        run(py, blocking, async move {
            client.control(control).await.map_err(gateway_err)?;
            Python::attach(|py| Ok(py.None()))
        })
    }

    #[pyo3(signature = (blocking=true))]
    fn config(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        let client = self.0.clone();
        run(py, blocking, async move {
            attach_py(&client.config().await.map_err(gateway_err)?)
        })
    }

    #[pyo3(signature = (values, blocking=true))]
    fn update_config(
        &self,
        py: Python<'_>,
        values: &Bound<'_, PyAny>,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let update: ibkit_gateway_client::ConfigUpdate = depythonize(values).map_err(|e| {
            InvalidArgumentError::new_err(format!(
                "The settings are not valid: {e}. Pass a dict of setting names to int, bool or str values, or None to remove a value."
            ))
        })?;
        let client = self.0.clone();
        run(py, blocking, async move {
            attach_py(&client.update_config(&update).await.map_err(gateway_err)?)
        })
    }

    #[pyo3(signature = (username, password, blocking=true))]
    fn set_credentials(
        &self,
        py: Python<'_>,
        username: String,
        password: String,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let client = self.0.clone();
        run(py, blocking, async move {
            client
                .set_credentials(&username, &password)
                .await
                .map_err(gateway_err)?;
            Python::attach(|py| Ok(py.None()))
        })
    }

    #[pyo3(signature = (secret, blocking=true))]
    fn set_totp_secret(
        &self,
        py: Python<'_>,
        secret: String,
        blocking: bool,
    ) -> PyResult<Py<PyAny>> {
        let client = self.0.clone();
        run(py, blocking, async move {
            client.set_totp_secret(&secret).await.map_err(gateway_err)?;
            Python::attach(|py| Ok(py.None()))
        })
    }

    #[pyo3(signature = (blocking=true))]
    fn clear_config(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        let client = self.0.clone();
        run(py, blocking, async move {
            attach_py(&client.clear_config().await.map_err(gateway_err)?)
        })
    }

    #[pyo3(signature = (blocking=true))]
    fn clear_totp_secret(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        let client = self.0.clone();
        run(py, blocking, async move {
            client.clear_totp_secret().await.map_err(gateway_err)?;
            Python::attach(|py| Ok(py.None()))
        })
    }

    #[pyo3(signature = (blocking=true))]
    fn events(&self, py: Python<'_>, blocking: bool) -> PyResult<Py<PyAny>> {
        let client = self.0.clone();
        run(py, blocking, async move {
            let events = client.events().await.map_err(gateway_err)?;
            Python::attach(|py| {
                Ok(Py::new(
                    py,
                    GatewayEventsCore(Arc::new(tokio::sync::Mutex::new(events))),
                )?
                .into_any())
            })
        })
    }
}

#[pyclass(frozen, module = "ibkit._ibkit")]
pub struct GatewayEventsCore(Arc<tokio::sync::Mutex<ibkit_gateway_client::Events>>);

#[pymethods]
impl GatewayEventsCore {
    /// Next event as a dict; `None` on timeout or when the stream closed.
    #[pyo3(signature = (timeout=None, blocking=true))]
    fn recv(&self, py: Python<'_>, timeout: Option<f64>, blocking: bool) -> PyResult<Py<PyAny>> {
        let events = self.0.clone();
        run(py, blocking, async move {
            let mut events = events.lock().await;
            let event = match timeout {
                Some(t) => {
                    match tokio::time::timeout(Duration::from_secs_f64(t.max(0.0)), events.next())
                        .await
                    {
                        Ok(event) => event,
                        Err(_) => Ok(None),
                    }
                }
                None => events.next().await,
            }
            .map_err(gateway_err)?;
            Python::attach(|py| match event {
                Some(event) => to_py(py, &event),
                None => Ok(py.None()),
            })
        })
    }
}

#[pymodule]
fn _ibkit(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<IBCore>()?;
    m.add_class::<ClientCore>()?;
    m.add_class::<TickerCore>()?;
    m.add_class::<TradeCore>()?;
    m.add_class::<BarsCore>()?;
    m.add_class::<RealTimeBarsCore>()?;
    m.add_class::<ScanCore>()?;
    m.add_class::<PnLCore>()?;
    m.add_class::<PnLSingleCore>()?;
    m.add_class::<WebCore>()?;
    m.add_class::<WsCore>()?;
    m.add_class::<FixCore>()?;
    m.add_class::<FakeTws>()?;
    m.add_class::<GatewayCore>()?;
    m.add_class::<GatewayEventsCore>()?;
    m.add_function(wrap_pyfunction!(fix_new_order_single, m)?)?;
    m.add_function(wrap_pyfunction!(fix_option_exercise, m)?)?;
    m.add_function(wrap_pyfunction!(fix_cancel_replace, m)?)?;
    m.add_function(wrap_pyfunction!(fix_cancel_request, m)?)?;
    m.add_function(wrap_pyfunction!(fix_order_status_request, m)?)?;
    m.add_function(wrap_pyfunction!(fix_new_order_multileg, m)?)?;
    m.add_function(wrap_pyfunction!(fix_multileg_cancel_replace, m)?)?;
    m.add_function(wrap_pyfunction!(fix_allocation_instruction, m)?)?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    Ok(())
}
