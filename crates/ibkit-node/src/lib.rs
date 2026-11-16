//! Node.js and Bun bindings for ibkit.
//!
//! Like the Python extension, this addon exposes "cores": thin handles over
//! the Rust objects. The TypeScript package in `ts/` builds the typed API on
//! top of them. Requests without a handle result go through one `request`
//! (or `call`) entry per object: the method name and a JSON argument array.
//! The typed wrapper is the public surface, so one dispatch table per object
//! replaces a hand-written binding per method.
//!
//! Errors carry their kind as a prefix that the wrapper maps to classes:
//! `RequestError:{json}`, `TimeoutError:`, `ConnectionError:`, `InvalidArgumentError:`
//! or `IbkitError:`.
//!
//! The ibkit control client is plain TypeScript (`ts/gateway.ts`, fetch
//! and WebSocket), so it also runs where the addon cannot load.

// Request methods mirror IB's positional signatures.
#![allow(clippy::too_many_arguments)]

use ibkit::state::{
    BarsHandle, Event, PnLHandle, PnLSingleHandle, RealTimeBarsHandle, ScanHandle, TickerHandle,
    TradeHandle,
};
use ibkit::*;
use napi::{Error, Result, Status};
use napi_derive::napi;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::{HashMap, hash_map::Entry};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

// ---- errors and conversion ---------------------------------------------------

fn kind_error(kind: &str, message: impl std::fmt::Display) -> Error {
    Error::new(Status::GenericFailure, format!("{kind}:{message}"))
}

fn invalid(message: impl std::fmt::Display) -> Error {
    kind_error(ibkit::ErrorKind::InvalidArgument.name(), message)
}

fn err(e: ibkit::Error) -> Error {
    match e {
        ibkit::Error::Request(r) => kind_error(
            ibkit::ErrorKind::Request.name(),
            json!({"reqId": r.req_id, "code": r.code, "message": r.message}),
        ),
        ibkit::Error::Invalid(m) => invalid(m),
        other => kind_error(other.kind().name(), other),
    }
}

fn tws_err(e: ibkit::tws::Error) -> Error {
    err(ibkit::Error::Tws(e))
}

fn to_json<T: Serialize + ?Sized>(value: &T) -> Result<Value> {
    serde_json::to_value(value).map_err(|e| kind_error(ibkit::ErrorKind::Other.name(), e))
}

fn from_json<T: DeserializeOwned>(value: Value, what: &str) -> Result<T> {
    serde_json::from_value(value).map_err(|e| {
        invalid(format!(
            "{what} is not valid: {e}. Pass the object form of the ibkit types (camelCase fields)."
        ))
    })
}

/// The next argument of `method`, converted to `T`. A missing argument is
/// read as `null`, so optional arguments may be left out.
fn arg<T: DeserializeOwned>(
    args: &mut std::vec::IntoIter<Value>,
    index: &mut usize,
    method: &str,
) -> Result<T> {
    *index += 1;
    from_json(
        args.next().unwrap_or(Value::Null),
        &format!("Argument {index} of {method}"),
    )
}

/// Dispatch `method` to one arm; each arm names its typed arguments.
macro_rules! dispatch {
    ($method:expr, $args:expr, $what:literal, { $( $name:literal => |$($a:ident : $t:ty),*| $body:expr ),* $(,)? }) => {{
        let method: &str = &$method;
        let mut args = $args.into_iter();
        let mut index = 0usize;
        let _ = (&mut args, &mut index);
        match method {
            $( $name => {
                $( let $a: $t = arg(&mut args, &mut index, $name)?; )*
                to_json(&$body)
            } )*
            other => Err(invalid(format!(concat!($what, " has no method {:?}. Use a method of the TypeScript wrapper."), other))),
        }
    }};
}

// ---- live handles ------------------------------------------------------------

macro_rules! handle {
    ($name:ident, $handle:ty, $read:ident) => {
        #[napi]
        pub struct $name {
            inner: $handle,
        }

        #[napi]
        impl $name {
            /// Identity of the underlying Rust object.
            #[napi]
            pub fn id(&self) -> i64 {
                Arc::as_ptr(&self.inner) as *const () as usize as i64
            }

            /// The current state as a plain object.
            #[napi]
            pub fn snapshot(&self) -> Result<Value> {
                to_json(&*self.inner.$read())
            }
        }
    };
}

handle!(TickerCore, TickerHandle, read);
handle!(TradeCore, TradeHandle, lock);
handle!(BarsCore, BarsHandle, read);
handle!(RealTimeBarsCore, RealTimeBarsHandle, read);

/// `barsFrom(from)` on a bar list: `{len, bars}` with the bars from index
/// `from` on. Bars before the last one never change, so after an update the
/// wrapper reads only the last bar and the new ones.
macro_rules! bars_from {
    ($($name:ident),*) => {$(
        #[napi]
        impl $name {
            #[napi]
            pub fn bars_from(&self, from: u32) -> Result<Value> {
                let list = self.inner.read();
                let from = (from as usize).min(list.bars.len());
                Ok(json!({"len": list.bars.len(), "bars": to_json(&list.bars[from..])?}))
            }
        }
    )*};
}
bars_from!(BarsCore, RealTimeBarsCore);
handle!(ScanCore, ScanHandle, read);
handle!(PnLCore, PnLHandle, read);
handle!(PnLSingleCore, PnLSingleHandle, read);

#[napi]
impl TickerCore {
    #[napi]
    pub fn midpoint(&self) -> f64 {
        self.inner.read().midpoint()
    }
    #[napi]
    pub fn market_price(&self) -> f64 {
        self.inner.read().market_price()
    }
    #[napi]
    pub fn has_bid_ask(&self) -> bool {
        self.inner.read().has_bid_ask()
    }
}

#[napi]
impl TradeCore {
    #[napi]
    pub fn status(&self) -> String {
        self.inner.lock().order_status.status.clone()
    }
    #[napi]
    pub fn is_done(&self) -> bool {
        self.inner.lock().is_done()
    }
    #[napi]
    pub fn is_active(&self) -> bool {
        self.inner.lock().is_active()
    }
    #[napi]
    pub fn is_working(&self) -> bool {
        self.inner.lock().is_working()
    }
    #[napi]
    pub fn is_waiting(&self) -> bool {
        self.inner.lock().is_waiting()
    }
    #[napi]
    pub fn filled(&self) -> f64 {
        self.inner.lock().filled()
    }
    #[napi]
    pub fn remaining(&self) -> f64 {
        self.inner.lock().remaining()
    }
}

fn ticker(inner: TickerHandle) -> TickerCore {
    TickerCore { inner }
}
fn trade(inner: TradeHandle) -> TradeCore {
    TradeCore { inner }
}
fn id_of<T: ?Sized>(handle: &Arc<T>) -> i64 {
    Arc::as_ptr(handle) as *const () as usize as i64
}

// ---- high-level IB client ----------------------------------------------------

/// One event as `{type, ...}`. A trade is referenced by id and carries its
/// change since the last send (`SentTrade::event`); the wrapper maps ids to
/// its live objects.
fn event_to_json(e: &Event, trades: &mut HashMap<i64, SentTrade>) -> Result<Value> {
    let mut trade = |t: &TradeHandle| SentTrade::event(trades, t);
    Ok(match e {
        Event::Message(m) => json!({"type": "message", "message": incoming_to_json(m)?}),
        Event::Connected => json!({"type": "connected"}),
        Event::Disconnected(r) => json!({"type": "disconnected", "reason": r}),
        Event::Update => json!({"type": "update"}),
        Event::PendingTickers(_) => unreachable!("forward sends ticker deltas"),
        Event::BarUpdate {
            req_id,
            has_new_bar,
        } => {
            json!({"type": "barUpdate", "reqId": req_id, "hasNewBar": has_new_bar})
        }
        Event::NewOrder(t) => json!({"type": "newOrder", "trade": trade(t)?}),
        Event::OrderModify(t) => json!({"type": "orderModify", "trade": trade(t)?}),
        Event::CancelOrder(t) => json!({"type": "cancelOrder", "trade": trade(t)?}),
        Event::OpenOrder(t) => json!({"type": "openOrder", "trade": trade(t)?}),
        Event::OrderStatus(t) => json!({"type": "orderStatus", "trade": trade(t)?}),
        Event::Filled(t) => json!({"type": "filled", "trade": trade(t)?}),
        Event::Cancelled(t) => json!({"type": "cancelled", "trade": trade(t)?}),
        Event::ExecDetails(t, f) => {
            json!({"type": "execDetails", "trade": trade(t)?, "fill": to_json(&**f)?})
        }
        Event::CommissionReport(t, f, r) => json!({
            "type": "commissionReport", "trade": trade(t)?, "fill": to_json(&**f)?, "report": to_json(r)?,
        }),
        Event::UpdatePortfolio(p) => json!({"type": "updatePortfolio", "item": to_json(&**p)?}),
        Event::Position(p) => json!({"type": "position", "position": to_json(&**p)?}),
        Event::AccountValue(v) => json!({"type": "accountValue", "value": to_json(v)?}),
        Event::AccountSummary(v) => json!({"type": "accountSummary", "value": to_json(v)?}),
        Event::PnL(p) => json!({"type": "pnl", "pnl": to_json(p)?}),
        Event::PnLSingle(p) => json!({"type": "pnlSingle", "pnl": to_json(p)?}),
        Event::ScannerData(id) => json!({"type": "scannerData", "reqId": id}),
        Event::TickNews(n) => json!({"type": "tickNews", "news": to_json(n)?}),
        Event::NewsBulletin(b) => json!({"type": "newsBulletin", "bulletin": to_json(b)?}),
        Event::WshMeta(s) => json!({"type": "wshMeta", "data": s}),
        Event::WshEvent(s) => json!({"type": "wshEvent", "data": s}),
        Event::Error {
            req_id,
            code,
            message,
            contract,
        } => json!({
            "type": "error", "reqId": req_id, "code": code, "message": message, "contract": to_json(contract)?,
        }),
        Event::Timeout(s) => json!({"type": "timeout", "idlePeriod": s}),
    })
}

/// One decoded TWS message as `[variant, payload]`; unit variants get `{}`.
fn incoming_to_json(m: &Incoming) -> Result<Value> {
    Ok(match to_json(m)? {
        Value::Object(map) => {
            let (name, payload) = map.into_iter().next().unwrap_or_default();
            json!([name, payload])
        }
        Value::String(name) => json!([name, {}]),
        other => json!(["Unknown", other]),
    })
}

type EventRx = Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<Value>>>;

/// The top-level fields of a struct as `(name, value)` in declaration
/// order, without `skip`. Unlike `serde_json::to_value` it builds no map, so
/// no key is allocated or hashed; the ticker diff compares by position.
fn fields_of<T: Serialize>(value: &T, skip: &'static str) -> Result<Vec<(&'static str, Value)>> {
    value
        .serialize(FieldList(skip))
        .map_err(|e| kind_error(ibkit::ErrorKind::Other.name(), e))
}

struct FieldList(&'static str);

struct CollectFields(&'static str, Vec<(&'static str, Value)>);

impl serde::ser::SerializeStruct for CollectFields {
    type Ok = Vec<(&'static str, Value)>;
    type Error = serde_json::Error;
    fn serialize_field<V: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &V,
    ) -> std::result::Result<(), Self::Error> {
        if key != self.0 {
            self.1.push((key, serde_json::to_value(value)?));
        }
        Ok(())
    }
    fn end(self) -> std::result::Result<Self::Ok, Self::Error> {
        Ok(self.1)
    }
}

/// Every serializer method except `serialize_struct` refuses its input.
macro_rules! not_a_struct {
    ($($method:ident($($arg:ty),*) -> $ret:ty;)*) => {
        $(fn $method(self, $(_: $arg),*) -> std::result::Result<$ret, Self::Error> {
            Err(serde::ser::Error::custom("fields_of takes a struct"))
        })*
    };
}

impl serde::Serializer for FieldList {
    type Ok = Vec<(&'static str, Value)>;
    type Error = serde_json::Error;
    type SerializeStruct = CollectFields;
    type SerializeSeq = serde::ser::Impossible<Self::Ok, Self::Error>;
    type SerializeTuple = serde::ser::Impossible<Self::Ok, Self::Error>;
    type SerializeTupleStruct = serde::ser::Impossible<Self::Ok, Self::Error>;
    type SerializeTupleVariant = serde::ser::Impossible<Self::Ok, Self::Error>;
    type SerializeMap = serde::ser::Impossible<Self::Ok, Self::Error>;
    type SerializeStructVariant = serde::ser::Impossible<Self::Ok, Self::Error>;
    fn serialize_struct(
        self,
        _: &'static str,
        len: usize,
    ) -> std::result::Result<CollectFields, Self::Error> {
        Ok(CollectFields(self.0, Vec::with_capacity(len)))
    }
    fn serialize_some<V: ?Sized + Serialize>(
        self,
        _: &V,
    ) -> std::result::Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("fields_of takes a struct"))
    }
    fn serialize_newtype_struct<V: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: &V,
    ) -> std::result::Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("fields_of takes a struct"))
    }
    fn serialize_newtype_variant<V: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &V,
    ) -> std::result::Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("fields_of takes a struct"))
    }
    not_a_struct! {
        serialize_bool(bool) -> Self::Ok;
        serialize_i8(i8) -> Self::Ok;
        serialize_i16(i16) -> Self::Ok;
        serialize_i32(i32) -> Self::Ok;
        serialize_i64(i64) -> Self::Ok;
        serialize_u8(u8) -> Self::Ok;
        serialize_u16(u16) -> Self::Ok;
        serialize_u32(u32) -> Self::Ok;
        serialize_u64(u64) -> Self::Ok;
        serialize_f32(f32) -> Self::Ok;
        serialize_f64(f64) -> Self::Ok;
        serialize_char(char) -> Self::Ok;
        serialize_str(&str) -> Self::Ok;
        serialize_bytes(&[u8]) -> Self::Ok;
        serialize_none() -> Self::Ok;
        serialize_unit() -> Self::Ok;
        serialize_unit_struct(&'static str) -> Self::Ok;
        serialize_unit_variant(&'static str, u32, &'static str) -> Self::Ok;
        serialize_seq(Option<usize>) -> Self::SerializeSeq;
        serialize_tuple(usize) -> Self::SerializeTuple;
        serialize_tuple_struct(&'static str, usize) -> Self::SerializeTupleStruct;
        serialize_tuple_variant(&'static str, u32, &'static str, usize) -> Self::SerializeTupleVariant;
        serialize_map(Option<usize>) -> Self::SerializeMap;
        serialize_struct_variant(&'static str, u32, &'static str, usize) -> Self::SerializeStructVariant;
    }
}

/// The ticker state that JavaScript holds: the fields sent so far (by
/// position, the contract apart) and the sequence number of the last
/// change. The client keeps every ticker for the connection's life, so a
/// ticker id (its Arc address) is never reused.
struct SentTicker {
    seq: u64,
    contract: Contract,
    fields: Vec<(&'static str, Value)>,
}

impl SentTicker {
    /// Record the current state of `handle` in `sent` as sent to
    /// JavaScript. Returns that state and the fields that changed since the
    /// last send, or `None` when the ticker is new. Each change raises the
    /// sequence number.
    fn observe<'a>(
        sent: &'a mut HashMap<i64, SentTicker>,
        handle: &TickerHandle,
    ) -> Result<(&'a SentTicker, Option<serde_json::Map<String, Value>>)> {
        let ticker = handle.read();
        let state = match sent.entry(id_of(handle)) {
            Entry::Vacant(entry) => {
                let state = entry.insert(SentTicker {
                    seq: 1,
                    contract: ticker.contract.clone(),
                    fields: fields_of(&*ticker, "contract")?,
                });
                return Ok((state, None));
            }
            Entry::Occupied(entry) => entry.into_mut(),
        };
        let mut delta = serde_json::Map::new();
        if ticker.contract != state.contract {
            state.contract = ticker.contract.clone();
            delta.insert("contract".into(), to_json(&state.contract)?);
        }
        for ((key, value), (_, old)) in fields_of(&*ticker, "contract")?
            .into_iter()
            .zip(&mut state.fields)
        {
            if *old != value {
                delta.insert(key.into(), value.clone());
                *old = value;
            }
        }
        if !delta.is_empty() {
            state.seq += 1;
        }
        Ok((state, Some(delta)))
    }

    fn full(&self) -> Result<Value> {
        let mut data = serde_json::Map::with_capacity(self.fields.len() + 1);
        data.insert("contract".into(), to_json(&self.contract)?);
        for (key, value) in &self.fields {
            data.insert((*key).into(), value.clone());
        }
        Ok(Value::Object(data))
    }
}

/// The trade state that JavaScript holds: the fields sent so far, by
/// position, and the sequence number of the last change.
struct SentTrade {
    seq: u64,
    fields: Vec<(&'static str, Value)>,
}

/// Changed trade fields: `data` holds the new values, and `append` the new
/// items of a list that only grew (the log, and fills without a later
/// commission report).
struct TradeDelta {
    data: serde_json::Map<String, Value>,
    append: serde_json::Map<String, Value>,
}

impl SentTrade {
    /// Record the current state of `handle` in `sent` as sent to
    /// JavaScript. Returns that state and the change since the last send,
    /// or `None` when the trade is new. Each change raises the sequence
    /// number.
    fn observe<'a>(
        sent: &'a mut HashMap<i64, SentTrade>,
        handle: &TradeHandle,
    ) -> Result<(&'a SentTrade, Option<TradeDelta>)> {
        let now = fields_of(&*handle.lock(), "")?;
        let state = match sent.entry(id_of(handle)) {
            Entry::Vacant(entry) => {
                return Ok((
                    entry.insert(SentTrade {
                        seq: 1,
                        fields: now,
                    }),
                    None,
                ));
            }
            Entry::Occupied(entry) => entry.into_mut(),
        };
        let mut delta = TradeDelta {
            data: serde_json::Map::new(),
            append: serde_json::Map::new(),
        };
        for ((key, value), (_, old)) in now.into_iter().zip(&mut state.fields) {
            if *old == value {
                continue;
            }
            match (&*old, &value) {
                (Value::Array(was), Value::Array(is))
                    if is.len() > was.len() && is[..was.len()] == was[..] =>
                {
                    delta
                        .append
                        .insert(key.into(), Value::Array(is[was.len()..].to_vec()));
                }
                _ => {
                    delta.data.insert(key.into(), value.clone());
                }
            }
            *old = value;
        }
        if !(delta.data.is_empty() && delta.append.is_empty()) {
            state.seq += 1;
        }
        Ok((state, Some(delta)))
    }

    fn full(&self) -> Value {
        Value::Object(
            self.fields
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
        )
    }

    /// A trade reference for an event: the full state on the first send,
    /// then only the change.
    fn event(sent: &mut HashMap<i64, SentTrade>, handle: &TradeHandle) -> Result<Value> {
        let id = id_of(handle);
        let (state, delta) = Self::observe(sent, handle)?;
        Ok(match delta {
            None => json!({"id": id, "seq": state.seq, "full": true, "data": state.full()}),
            Some(delta) => {
                json!({"id": id, "seq": state.seq, "data": delta.data, "append": delta.append})
            }
        })
    }
}

/// What JavaScript holds of each ticker and trade, so that events carry
/// only changes. The client keeps both for the connection's life, so an id
/// (an Arc address) is never reused.
#[derive(Default)]
struct SentState {
    tickers: HashMap<i64, SentTicker>,
    trades: HashMap<i64, SentTrade>,
}

type Sent = Arc<parking_lot::Mutex<SentState>>;

/// A `pendingTickers` event with, per ticker, only the fields that changed
/// since the last send (`full` on the first send). A delta has a sequence
/// number that grows with each change, so the wrapper skips deltas that are
/// not newer than a state it read directly.
fn pending_tickers(sent: &mut HashMap<i64, SentTicker>, tickers: &[TickerHandle]) -> Result<Value> {
    let mut out = Vec::with_capacity(tickers.len());
    for handle in tickers {
        let (state, delta) = SentTicker::observe(sent, handle)?;
        let id = id_of(handle);
        out.push(match delta {
            None => json!({"id": id, "seq": state.seq, "full": true, "data": state.full()?}),
            Some(delta) => json!({"id": id, "seq": state.seq, "data": delta}),
        });
    }
    Ok(json!({"type": "pendingTickers", "tickers": out}))
}

/// Move every event batch from the client to the JavaScript queue as JSON,
/// as it happens. A packet's `PendingTickers` goes before its `Update`:
/// ticker data travels with that event, and `update` listeners
/// (waitOnUpdate) must see it, as in ib_async, whose tickers change before
/// its updateEvent.
async fn forward(
    mut batches: mpsc::UnboundedReceiver<Arc<[Event]>>,
    queue: mpsc::UnboundedSender<Value>,
    sent: Sent,
) {
    loop {
        let batch = tokio::select! {
            batch = batches.recv() => match batch {
                Some(batch) => batch,
                None => return,
            },
            () = queue.closed() => return,
        };
        let mut update = false;
        let mut sent = sent.lock();
        for event in batch.iter() {
            let value = match event {
                Event::Update => {
                    update = true;
                    continue;
                }
                Event::PendingTickers(tickers) => pending_tickers(&mut sent.tickers, tickers),
                event => event_to_json(event, &mut sent.trades),
            };
            match value {
                Ok(value) => {
                    if queue.send(value).is_err() {
                        return;
                    }
                }
                Err(error) => {
                    eprintln!("ibkit: an event could not be converted for JavaScript: {error}")
                }
            }
        }
        if update && queue.send(json!({"type": "update"})).is_err() {
            return;
        }
    }
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

/// Connected high-level client.
#[napi]
pub struct IbCore {
    ib: IB,
    events: EventRx,
    sent: Sent,
}

/// Connect and synchronize (ib_async `connect`). Keys are camelCase.
#[napi]
pub async fn ib_connect(args: Value) -> Result<IbCore> {
    let a: ConnectArgs = from_json(args, "The connect options")?;
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
    let ib = IB::connect_with(opts).await.map_err(err)?;
    let (queue, events) = mpsc::unbounded_channel();
    let sent = Sent::default();
    tokio::spawn(forward(ib.event_batches(), queue, sent.clone()));
    Ok(IbCore {
        ib,
        events: Arc::new(tokio::sync::Mutex::new(events)),
        sent,
    })
}

#[napi]
impl IbCore {
    #[napi]
    pub fn is_connected(&self) -> bool {
        self.ib.is_connected()
    }
    #[napi]
    pub fn disconnect(&self) {
        self.ib.disconnect();
    }
    #[napi]
    pub fn client_id(&self) -> i32 {
        self.ib.client().client_id()
    }
    #[napi]
    pub fn server_version(&self) -> i32 {
        self.ib.client().server_version()
    }
    #[napi]
    pub fn managed_accounts(&self) -> Vec<String> {
        self.ib.managed_accounts()
    }
    #[napi]
    pub fn next_req_id(&self) -> i32 {
        self.ib.client().next_req_id()
    }
    /// Low-level client sharing this connection.
    #[napi]
    pub fn client(&self) -> ClientCore {
        ClientCore {
            client: self.ib.client().clone(),
            messages: None,
        }
    }
    #[napi]
    pub fn connection_stats(&self) -> Value {
        let s = self.ib.connection_stats();
        json!({
            "startTime": s.start_time, "duration": s.duration, "numBytesRecv": s.num_bytes_recv,
            "numBytesSent": s.num_bytes_sent, "numMsgRecv": s.num_msg_recv, "numMsgSent": s.num_msg_sent,
        })
    }
    /// Emit a `timeout` event once no message arrived for `seconds`; 0 stops.
    #[napi]
    pub async fn set_timeout(&self, seconds: f64) {
        self.ib
            .set_timeout(Some(Duration::from_secs_f64(seconds.max(0.0))));
    }

    /// Wait up to `timeout_ms` (forever when absent) for events; returns
    /// every queued event, or none on timeout or when the client closed.
    #[napi]
    pub async fn next_events(&self, timeout_ms: Option<u32>) -> Result<Vec<Value>> {
        let mut rx = self.events.lock().await;
        let first = match timeout_ms.map(|ms| Duration::from_millis(ms.into())) {
            Some(t) => tokio::time::timeout(t, rx.recv()).await.ok().flatten(),
            None => rx.recv().await,
        };
        let mut out = Vec::new();
        if let Some(value) = first {
            out.push(value);
            while let Ok(value) = rx.try_recv() {
                out.push(value);
            }
        }
        Ok(out)
    }

    /// The current state of `ticker` as `{seq, data}`. It counts as a send:
    /// later deltas build on it, and queued deltas with a lower `seq` are
    /// older than it.
    #[napi]
    pub fn ticker_state(&self, ticker: &TickerCore) -> Result<Value> {
        let mut sent = self.sent.lock();
        let (state, _) = SentTicker::observe(&mut sent.tickers, &ticker.inner)?;
        Ok(json!({"seq": state.seq, "data": state.full()?}))
    }

    /// The current trade state and its sequence number, as `tickerState`.
    #[napi]
    pub fn trade_state(&self, trade: &TradeCore) -> Result<Value> {
        let mut sent = self.sent.lock();
        let (state, _) = SentTrade::observe(&mut sent.trades, &trade.inner)?;
        Ok(json!({"seq": state.seq, "data": state.full()}))
    }

    // ---- live objects --------------------------------------------------------

    #[napi]
    pub fn trades(&self) -> Vec<TradeCore> {
        self.ib.trades().into_iter().map(trade).collect()
    }
    #[napi]
    pub fn open_trades(&self) -> Vec<TradeCore> {
        self.ib.open_trades().into_iter().map(trade).collect()
    }
    #[napi]
    pub fn tickers(&self) -> Vec<TickerCore> {
        self.ib.tickers().into_iter().map(ticker).collect()
    }
    #[napi]
    pub fn ticker(&self, contract: Value) -> Result<Option<TickerCore>> {
        let c: Contract = from_json(contract, "The contract")?;
        Ok(self.ib.ticker(&c).map(ticker))
    }
    #[napi]
    pub fn trade_by_id(&self, id: i64) -> Option<TradeCore> {
        self.ib
            .trades()
            .into_iter()
            .find(|t| id_of(t) == id)
            .map(trade)
    }
    #[napi]
    pub fn ticker_by_id(&self, id: i64) -> Option<TickerCore> {
        self.ib
            .tickers()
            .into_iter()
            .find(|t| id_of(t) == id)
            .map(ticker)
    }
    #[napi]
    pub fn realtime_bars(&self) -> Vec<RealTimeBarsCore> {
        self.ib
            .realtime_bars()
            .into_iter()
            .map(|inner| RealTimeBarsCore { inner })
            .collect()
    }
    #[napi]
    pub fn place_order(&self, contract: Value, order: Value) -> Result<TradeCore> {
        let c: Contract = from_json(contract, "The contract")?;
        let o: Order = from_json(order, "The order")?;
        self.ib.place_order(&c, o).map(trade).map_err(err)
    }
    #[napi]
    pub fn cancel_order(
        &self,
        order: Value,
        manual_cancel_order_time: String,
    ) -> Result<Option<TradeCore>> {
        let o: Order = from_json(order, "The order")?;
        Ok(self
            .ib
            .cancel_order(&o, &manual_cancel_order_time)
            .map_err(err)?
            .map(trade))
    }
    #[napi]
    pub fn req_mkt_data(
        &self,
        contract: Value,
        generic_tick_list: String,
        snapshot: bool,
        regulatory_snapshot: bool,
        options: Value,
    ) -> Result<TickerCore> {
        let c: Contract = from_json(contract, "The contract")?;
        let opts: Vec<TagValue> = from_json(options, "mktDataOptions")?;
        self.ib
            .req_mkt_data(&c, &generic_tick_list, snapshot, regulatory_snapshot, &opts)
            .map(ticker)
            .map_err(err)
    }
    #[napi]
    pub fn req_tick_by_tick_data(
        &self,
        contract: Value,
        tick_type: String,
        number_of_ticks: i32,
        ignore_size: bool,
    ) -> Result<TickerCore> {
        let c: Contract = from_json(contract, "The contract")?;
        self.ib
            .req_tick_by_tick_data(&c, &tick_type, number_of_ticks, ignore_size)
            .map(ticker)
            .map_err(err)
    }
    #[napi]
    pub fn req_mkt_depth(
        &self,
        contract: Value,
        num_rows: i32,
        is_smart_depth: bool,
        options: Value,
    ) -> Result<TickerCore> {
        let c: Contract = from_json(contract, "The contract")?;
        let opts: Vec<TagValue> = from_json(options, "mktDepthOptions")?;
        self.ib
            .req_mkt_depth(&c, num_rows, is_smart_depth, &opts)
            .map(ticker)
            .map_err(err)
    }
    #[napi]
    pub fn req_real_time_bars(
        &self,
        contract: Value,
        bar_size: i32,
        what_to_show: String,
        use_rth: bool,
        options: Value,
    ) -> Result<RealTimeBarsCore> {
        let c: Contract = from_json(contract, "The contract")?;
        let opts: Vec<TagValue> = from_json(options, "realTimeBarsOptions")?;
        self.ib
            .req_real_time_bars(&c, bar_size, &what_to_show, use_rth, &opts)
            .map(|inner| RealTimeBarsCore { inner })
            .map_err(err)
    }
    #[napi]
    pub fn cancel_real_time_bars(&self, bars: &RealTimeBarsCore) -> Result<()> {
        self.ib.cancel_real_time_bars(&bars.inner).map_err(err)
    }
    #[napi]
    pub fn cancel_historical_data(&self, bars: &BarsCore) -> Result<()> {
        self.ib.cancel_historical_data(&bars.inner).map_err(err)
    }
    #[napi]
    pub fn req_pnl(&self, account: String, model_code: String) -> Result<PnLCore> {
        self.ib
            .req_pnl(&account, &model_code)
            .map(|inner| PnLCore { inner })
            .map_err(err)
    }
    #[napi]
    pub fn req_pnl_single(
        &self,
        account: String,
        model_code: String,
        con_id: i32,
    ) -> Result<PnLSingleCore> {
        self.ib
            .req_pnl_single(&account, &model_code, con_id)
            .map(|inner| PnLSingleCore { inner })
            .map_err(err)
    }
    #[napi]
    pub fn req_scanner_subscription(
        &self,
        subscription: Value,
        options: Value,
        filter_options: Value,
    ) -> Result<ScanCore> {
        let s: ScannerSubscription = from_json(subscription, "The scanner subscription")?;
        let o: Vec<TagValue> = from_json(options, "scannerSubscriptionOptions")?;
        let f: Vec<TagValue> = from_json(filter_options, "scannerSubscriptionFilterOptions")?;
        self.ib
            .req_scanner_subscription(&s, &o, &f)
            .map(|inner| ScanCore { inner })
            .map_err(err)
    }
    #[napi]
    pub fn cancel_scanner_subscription(&self, scan: &ScanCore) -> Result<()> {
        self.ib
            .cancel_scanner_subscription(&scan.inner)
            .map_err(err)
    }
    #[napi]
    pub async fn req_tickers(
        &self,
        contracts: Value,
        regulatory_snapshot: bool,
    ) -> Result<Vec<TickerCore>> {
        let cs: Vec<Contract> = from_json(contracts, "The contracts")?;
        let ts = self
            .ib
            .req_tickers(&cs, regulatory_snapshot)
            .await
            .map_err(err)?;
        Ok(ts.into_iter().map(ticker).collect())
    }
    #[napi]
    pub async fn req_historical_data(
        &self,
        contract: Value,
        end_date_time: String,
        duration_str: String,
        bar_size_setting: String,
        what_to_show: String,
        use_rth: bool,
        format_date: i32,
        keep_up_to_date: bool,
        chart_options: Value,
    ) -> Result<BarsCore> {
        let c: Contract = from_json(contract, "The contract")?;
        let opts: Vec<TagValue> = from_json(chart_options, "chartOptions")?;
        let inner = self
            .ib
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
        Ok(BarsCore { inner })
    }
    #[napi]
    pub async fn req_open_orders(&self, all_clients: bool) -> Result<Vec<TradeCore>> {
        let ts = if all_clients {
            self.ib.req_all_open_orders().await
        } else {
            self.ib.req_open_orders().await
        }
        .map_err(err)?;
        Ok(ts.into_iter().map(trade).collect())
    }
    #[napi]
    pub async fn req_completed_orders(&self, api_only: bool) -> Result<Vec<TradeCore>> {
        let ts = self.ib.req_completed_orders(api_only).await.map_err(err)?;
        Ok(ts.into_iter().map(trade).collect())
    }

    /// State reads and immediate actions that return plain data.
    #[napi]
    pub fn call(&self, method: String, args: Vec<Value>) -> Result<Value> {
        let ib = &self.ib;
        dispatch!(method, args, "IB", {
            "positions" => |account: String| ib.positions(&account),
            "portfolio" => |account: String| ib.portfolio(&account),
            "accountValues" => |account: String| ib.account_values(&account),
            "accountSummary" => |account: String| ib.account_summary(&account),
            "pnl" => |account: String, model_code: String| ib.pnl(&account, &model_code),
            "pnlSingle" => |account: String, model_code: String, con_id: i32| ib.pnl_single(&account, &model_code, con_id),
            "fills" => | | ib.fills(),
            "newsTicks" => | | ib.news_ticks(),
            "newsBulletins" => | | ib.news_bulletins(),
            "reqGlobalCancel" => | | ib.req_global_cancel().map_err(err)?,
            "bracketOrder" => |action: String, quantity: f64, limit_price: f64, take_profit_price: f64, stop_loss_price: f64| {
                let b = ib.bracket_order(&action, quantity, limit_price, take_profit_price, stop_loss_price).map_err(err)?;
                [to_json(&b.parent)?, to_json(&b.take_profit)?, to_json(&b.stop_loss)?]
            },
            "reqAutoOpenOrders" => |auto_bind: bool| ib.req_auto_open_orders(auto_bind).map_err(err)?,
            "reqMarketDataType" => |market_data_type: i32| ib.req_market_data_type(market_data_type).map_err(err)?,
            "cancelMktData" => |contract: Contract| ib.cancel_mkt_data(&contract).map_err(err)?,
            "cancelTickByTickData" => |contract: Contract, tick_type: String| ib.cancel_tick_by_tick_data(&contract, &tick_type).map_err(err)?,
            "cancelMktDepth" => |contract: Contract, is_smart_depth: bool| ib.cancel_mkt_depth(&contract, is_smart_depth).map_err(err)?,
            "cancelPnL" => |account: String, model_code: String| ib.cancel_pnl(&account, &model_code).map_err(err)?,
            "cancelPnLSingle" => |account: String, model_code: String, con_id: i32| ib.cancel_pnl_single(&account, &model_code, con_id).map_err(err)?,
            "exerciseOptions" => |contract: Contract, exercise_action: i32, exercise_quantity: i32, account: String, override_: i32|
                ib.exercise_options(&contract, exercise_action, exercise_quantity, &account, override_).map_err(err)?,
            "reqNewsBulletins" => |all_messages: bool| ib.req_news_bulletins(all_messages).map_err(err)?,
            "cancelNewsBulletins" => | | ib.cancel_news_bulletins().map_err(err)?,
        })
    }

    /// Requests that wait for IB's answer and return plain data.
    #[napi]
    pub async fn request(&self, method: String, args: Vec<Value>) -> Result<Value> {
        let ib = self.ib.clone();
        dispatch!(method, args, "IB", {
            "qualifyContracts" => |contracts: Vec<Contract>| ib.qualify_contracts(&contracts).await.map_err(err)?,
            "reqContractDetails" => |contract: Contract| ib.req_contract_details(&contract).await.map_err(err)?,
            "reqHistoricalSchedule" => |contract: Contract, num_days: i32, end_date_time: String, use_rth: bool|
                ib.req_historical_schedule(&contract, num_days, &end_date_time, use_rth).await.map_err(err)?,
            "reqHistoricalTicks" => |contract: Contract, start_date_time: String, end_date_time: String, number_of_ticks: i32, what_to_show: String, use_rth: bool, ignore_size: bool, misc_options: Vec<TagValue>| {
                let msgs = ib.req_historical_ticks(&contract, &start_date_time, &end_date_time, number_of_ticks, &what_to_show, use_rth, ignore_size, &misc_options).await.map_err(err)?;
                let mut ticks: Vec<Value> = Vec::new();
                for m in msgs {
                    let v = match m {
                        Incoming::HistoricalTicks { ticks, .. } => to_json(&ticks)?,
                        Incoming::HistoricalTicksBidAsk { ticks, .. } => to_json(&ticks)?,
                        Incoming::HistoricalTicksLast { ticks, .. } => to_json(&ticks)?,
                        _ => continue,
                    };
                    if let Value::Array(a) = v {
                        ticks.extend(a);
                    }
                }
                ticks
            },
            "reqHeadTimeStamp" => |contract: Contract, what_to_show: String, use_rth: bool, format_date: i32|
                ib.req_head_time_stamp(&contract, &what_to_show, use_rth, format_date).await.map_err(err)?,
            "reqHistogramData" => |contract: Contract, use_rth: bool, period: String| ib.req_histogram_data(&contract, use_rth, &period).await.map_err(err)?,
            "whatIfOrder" => |contract: Contract, order: Order| ib.what_if_order(&contract, &order).await.map_err(err)?,
            "reqExecutions" => |filter: ExecutionFilter| ib.req_executions(&filter).await.map_err(err)?,
            "reqPositions" => | | ib.req_positions().await.map_err(err)?,
            "reqAccountUpdates" => |account: String| ib.req_account_updates(&account).await.map_err(err)?,
            "reqAccountUpdatesMulti" => |account: String, model_code: String| ib.req_account_updates_multi(&account, &model_code).await.map_err(err)?,
            "reqAccountSummary" => | | ib.req_account_summary().await.map_err(err)?,
            "reqMatchingSymbols" => |pattern: String| ib.req_matching_symbols(&pattern).await.map_err(err)?,
            "reqMarketRule" => |market_rule_id: i32| ib.req_market_rule(market_rule_id).await.map_err(err)?,
            "reqSecDefOptParams" => |underlying_symbol: String, fut_fop_exchange: String, underlying_sec_type: String, underlying_con_id: i32|
                ib.req_sec_def_opt_params(&underlying_symbol, &fut_fop_exchange, &underlying_sec_type, underlying_con_id).await.map_err(err)?,
            "reqMktDepthExchanges" => | | ib.req_mkt_depth_exchanges().await.map_err(err)?,
            "reqSmartComponents" => |bbo_exchange: String| ib.req_smart_components(&bbo_exchange).await.map_err(err)?,
            "reqScannerData" => |subscription: ScannerSubscription, options: Vec<TagValue>, filter_options: Vec<TagValue>|
                ib.req_scanner_data(&subscription, &options, &filter_options).await.map_err(err)?,
            "reqScannerParameters" => | | ib.req_scanner_parameters().await.map_err(err)?,
            "calculateImpliedVolatility" => |contract: Contract, option_price: f64, under_price: f64, options: Vec<TagValue>|
                ib.calculate_implied_volatility(&contract, option_price, under_price, &options).await.map_err(err)?,
            "calculateOptionPrice" => |contract: Contract, volatility: f64, under_price: f64, options: Vec<TagValue>|
                ib.calculate_option_price(&contract, volatility, under_price, &options).await.map_err(err)?,
            "reqNewsProviders" => | | ib.req_news_providers().await.map_err(err)?,
            "reqNewsArticle" => |provider_code: String, article_id: String, options: Vec<TagValue>|
                ib.req_news_article(&provider_code, &article_id, &options).await.map_err(err)?,
            "reqHistoricalNews" => |con_id: i32, provider_codes: String, start_date_time: String, end_date_time: String, total_results: i32, options: Vec<TagValue>|
                ib.req_historical_news(con_id, &provider_codes, &start_date_time, &end_date_time, total_results, &options).await.map_err(err)?,
            "reqFundamentalData" => |contract: Contract, report_type: String, options: Vec<TagValue>|
                ib.req_fundamental_data(&contract, &report_type, &options).await.map_err(err)?,
            "requestFA" => |fa_data_type: i32| ib.request_fa(fa_data_type).await.map_err(err)?,
            "replaceFA" => |fa_data_type: i32, xml: String| ib.replace_fa(fa_data_type, &xml).await.map_err(err)?,
            "getWshMetaData" => | | ib.get_wsh_meta_data().await.map_err(err)?,
            "getWshEventData" => |data: WshEventData| ib.get_wsh_event_data(&data).await.map_err(err)?,
            "reqConfig" => | | ib.req_config().await.map_err(err)?,
            "updateConfig" => |request: Value| ib.update_config(&request).await.map_err(err)?,
            "reqUserInfo" => | | ib.req_user_info().await.map_err(err)?,
            "reqCurrentTime" => | | ib.req_current_time().await.map_err(err)?,
            "reqCurrentTimeInMillis" => | | ib.req_current_time_in_millis().await.map_err(err)?,
            "reqFamilyCodes" => | | ib.req_family_codes().await.map_err(err)?,
            "reqSoftDollarTiers" => | | ib.req_soft_dollar_tiers().await.map_err(err)?,
        })
    }
}

// ---- low-level client ------------------------------------------------------------

type MessageRx = Arc<tokio::sync::Mutex<ibkit::tws::client::EventReceiver>>;

/// Low-level TWS client (IB's `EClient`): one method per request, messages
/// as `[variant, payload]` pairs.
#[napi]
pub struct ClientCore {
    client: ibkit::tws::Client,
    /// `None` when the connection belongs to an `IB` (messages go to the `IB`).
    messages: Option<MessageRx>,
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

#[napi]
pub async fn client_connect(args: Value) -> Result<ClientCore> {
    let a: ClientArgs = from_json(args, "The connect options")?;
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
    let (client, rx) = ibkit::tws::Client::connect(opts).await.map_err(tws_err)?;
    Ok(ClientCore {
        client,
        messages: Some(Arc::new(tokio::sync::Mutex::new(rx))),
    })
}

macro_rules! client_requests {
    ($( $js:literal fn $name:ident ( $( $kind:ident $a:ident : $t:ty ),* ); )*) => {
        /// Every request method of IB's `EClient`, by its camelCase name.
        pub const CLIENT_REQUESTS: &[&str] = &[$($js),*];

        #[napi]
        impl ClientCore {
            #[napi]
            pub fn request(&self, method: String, args: Vec<Value>) -> Result<Value> {
                let client = &self.client;
                dispatch!(method, args, "Client", {
                    $( $js => |$($a: $t),*| client.$name($( client_requests!(@pass $kind $a) ),*).map_err(tws_err)? ),*
                })
            }
        }
    };
    (@pass val $a:ident) => { $a };
    (@pass ref $a:ident) => { &$a };
}

client_requests! {
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
    "reqCurrentTimeInMillis" fn req_current_time_in_millis();
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
    "updateConfig" fn update_config(ref request: Value);
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

#[napi]
impl ClientCore {
    #[napi]
    pub fn is_connected(&self) -> bool {
        self.client.is_connected()
    }
    #[napi]
    pub fn disconnect(&self) {
        self.client.disconnect();
    }
    #[napi]
    pub fn server_version(&self) -> i32 {
        self.client.server_version()
    }
    #[napi]
    pub fn client_id(&self) -> i32 {
        self.client.client_id()
    }
    #[napi]
    pub fn connection_time(&self) -> String {
        self.client.connection_time().to_string()
    }
    #[napi]
    pub fn next_req_id(&self) -> i32 {
        self.client.next_req_id()
    }
    #[napi]
    pub fn update_req_id(&self, min_id: i32) {
        self.client.update_req_id(min_id)
    }
    #[napi]
    pub fn managed_accounts(&self) -> Vec<String> {
        self.client.managed_accounts()
    }
    /// Wait up to `timeout_ms` for messages; returns every message available.
    #[napi]
    pub async fn poll(&self, timeout_ms: u32) -> Result<Vec<Value>> {
        let rx = self.messages.clone().ok_or_else(|| {
            invalid(
                "This client shares an IB connection; its messages arrive through the IB events.",
            )
        })?;
        let mut rx = rx.lock().await;
        let mut out = Vec::new();
        if let Ok(Some(first)) =
            tokio::time::timeout(Duration::from_millis(u64::from(timeout_ms)), rx.recv()).await
        {
            out.push(incoming_to_json(&first)?);
            while let Some(m) = rx.try_recv() {
                out.push(incoming_to_json(&m)?);
            }
        }
        Ok(out)
    }
}

/// The request method names of the low-level client.
#[napi]
pub fn client_request_names() -> Vec<&'static str> {
    CLIENT_REQUESTS.to_vec()
}

// ---- Web API -----------------------------------------------------------------

fn web_err(e: ibkit_web::WebError) -> Error {
    match e {
        ibkit_web::WebError::Invalid(m) => invalid(m),
        other => kind_error(other.kind().name(), other),
    }
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

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebArgs {
    base_url: String,
    accept_invalid_certs: bool,
    timeout: f64,
    #[serde(default)]
    oauth: Option<OAuthArgs>,
    #[serde(default)]
    oauth2: Option<OAuth2Args>,
    pacing: bool,
}

/// Web API (Client Portal API) client.
#[napi]
pub struct WebCore(Arc<ibkit_web::WebClient>);

fn query_pairs(query: Option<Value>) -> Result<Vec<(String, String)>> {
    let Some(query) = query.filter(|q| !q.is_null()) else {
        return Ok(Vec::new());
    };
    let map: serde_json::Map<String, Value> = from_json(query, "The query")?;
    Ok(map
        .into_iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| match v {
            Value::String(s) => (k, s),
            other => (k, other.to_string()),
        })
        .collect())
}

#[napi]
impl WebCore {
    #[napi(constructor)]
    pub fn new(args: Value) -> Result<Self> {
        let a: WebArgs = from_json(args, "The Web API settings")?;
        let oauth = a.oauth.map(|a| {
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
            cfg
        });
        let oauth2 = a.oauth2.map(|a| {
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
            cfg
        });
        let cfg = ibkit_web::WebConfig {
            base_url: a.base_url,
            accept_invalid_certs: a.accept_invalid_certs,
            timeout: Duration::from_secs_f64(a.timeout),
            oauth,
            oauth2,
            pacing: a.pacing,
        };
        Ok(Self(Arc::new(
            ibkit_web::WebClient::new(cfg).map_err(web_err)?,
        )))
    }

    /// Send `method` to `path` with optional query pairs and JSON body.
    #[napi]
    pub async fn request(
        &self,
        method: String,
        path: String,
        query: Option<Value>,
        body: Option<Value>,
    ) -> Result<Value> {
        let method = ibkit_web::client::parse_method(&method).map_err(web_err)?;
        let query = query_pairs(query)?;
        let body = body.filter(|b| !b.is_null());
        self.0
            .request(method, &path, &query, body.as_ref())
            .await
            .map_err(web_err)
    }

    /// Send `method` to a `/gw/api` path (OAuth 2.0; signed JWT bodies).
    #[napi]
    pub async fn gw_request(
        &self,
        method: String,
        path: String,
        query: Option<Value>,
        body: Option<Value>,
    ) -> Result<Value> {
        let method = ibkit_web::client::parse_method(&method).map_err(web_err)?;
        let query = query_pairs(query)?;
        let body = body.filter(|b| !b.is_null());
        self.0
            .gw_request(method, &path, &query, body.as_ref())
            .await
            .map_err(web_err)
    }

    #[napi]
    pub async fn oauth_request_token(&self) -> Result<String> {
        self.0.oauth_request_token().await.map_err(web_err)
    }

    #[napi]
    pub async fn oauth_access_token(
        &self,
        request_token: String,
        verifier: String,
    ) -> Result<Value> {
        to_json(
            &self
                .0
                .oauth_access_token(&request_token, &verifier)
                .await
                .map_err(web_err)?,
        )
    }

    #[napi]
    pub async fn oauth_init(&self, compete: bool) -> Result<Value> {
        to_json(&self.0.oauth_init(compete).await.map_err(web_err)?)
    }

    #[napi]
    pub async fn oauth2_init(&self, compete: bool) -> Result<Value> {
        to_json(&self.0.oauth2_init(compete).await.map_err(web_err)?)
    }

    #[napi]
    pub fn live_session_token(&self) -> Option<String> {
        self.0.live_session_token()
    }
}

/// Download a Flex query report (XML) from the Flex Web Service.
#[napi]
pub async fn flex_download(token: String, query_id: String) -> Result<String> {
    ibkit_web::flex::download(&token, &query_id)
        .await
        .map_err(web_err)
}

// ---- Web API WebSocket ---------------------------------------------------------

#[napi]
pub struct WsCore {
    sender: ibkit_web::WsSender,
    client: Arc<tokio::sync::Mutex<ibkit_web::WsClient>>,
}

fn ws_err(e: ibkit_web::ws::WsError) -> Error {
    kind_error(e.kind().name(), e)
}

#[derive(serde::Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct WsArgs {
    session: Option<String>,
    oauth_token: Option<String>,
    bearer_token: Option<String>,
    accept_invalid_certs: bool,
    heartbeat: Option<f64>,
    confirm_timeout: Option<f64>,
    renew_market_data: Option<f64>,
}

#[napi]
pub async fn ws_connect(url: String, args: Value) -> Result<WsCore> {
    let a: WsArgs = from_json(args, "The WebSocket settings")?;
    let seconds = |v: Option<f64>| v.filter(|s| *s > 0.0).map(Duration::from_secs_f64);
    let auth = ibkit_web::WsAuth {
        session: a.session,
        oauth_token: a.oauth_token,
        bearer_token: a.bearer_token,
        accept_invalid_certs: a.accept_invalid_certs,
        heartbeat: seconds(a.heartbeat),
        confirm_timeout: seconds(a.confirm_timeout),
        renew_market_data: seconds(a.renew_market_data),
    };
    let ws = ibkit_web::WsClient::connect(&url, auth)
        .await
        .map_err(ws_err)?;
    Ok(WsCore {
        sender: ws.sender(),
        client: Arc::new(tokio::sync::Mutex::new(ws)),
    })
}

#[napi]
impl WsCore {
    /// Send a raw frame or a subscription by name.
    #[napi]
    pub fn call(&self, method: String, args: Vec<Value>) -> Result<Value> {
        let s = &self.sender;
        dispatch!(method, args, "WsClient", {
            "send" => |text: String| s.send(text).map_err(ws_err)?,
            "subscribeMarketData" => |conid: i64, fields: Vec<String>|
                s.subscribe_market_data(conid, &fields.iter().map(String::as_str).collect::<Vec<_>>()).map_err(ws_err)?,
            "unsubscribeMarketData" => |conid: i64| s.unsubscribe_market_data(conid).map_err(ws_err)?,
            "subscribeOrders" => |filters: Option<Vec<String>>|
                s.subscribe_orders(&filters.unwrap_or_default().iter().map(String::as_str).collect::<Vec<_>>()).map_err(ws_err)?,
            "exerciseOptions" => |conid: i64| s.exercise_options(conid).map_err(ws_err)?,
            "subscribePnl" => | | s.subscribe_pnl().map_err(ws_err)?,
            "subscribeTrades" => |realtime_updates_only: Option<bool>, days: Option<u8>|
                s.subscribe_trades(realtime_updates_only.unwrap_or(false), days).map_err(ws_err)?,
            "subscribeAccountSummary" => |account_id: String| s.subscribe_account_summary(&account_id).map_err(ws_err)?,
            "subscribeAccountLedger" => |account_id: String| s.subscribe_account_ledger(&account_id).map_err(ws_err)?,
            "subscribeBookTrader" => |account_id: String, conid: i64, exchange: Option<String>|
                s.subscribe_book_trader(&account_id, conid, exchange.as_deref()).map_err(ws_err)?,
        })
    }

    /// Next message as `[topic, args, data]`; `null` on timeout or close.
    #[napi]
    pub async fn recv(&self, timeout_ms: Option<u32>) -> Result<Value> {
        let mut ws = self.client.lock().await;
        let m = match timeout_ms.map(|ms| Duration::from_millis(ms.into())) {
            Some(t) => tokio::time::timeout(t, ws.recv()).await.ok().flatten(),
            None => ws.recv().await,
        };
        Ok(match m {
            Some(m) => json!([m.topic, m.args, m.data]),
            None => Value::Null,
        })
    }
}

// ---- FIX ---------------------------------------------------------------------

type FixFields = Vec<(u32, String)>;

#[napi]
pub struct FixCore {
    handle: ibkit_fix::FixHandle,
    events: Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<ibkit_fix::FixEvent>>>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixArgs {
    host: String,
    port: u16,
    sender_comp_id: String,
    target_comp_id: String,
    #[serde(default)]
    heartbeat_interval: Option<u64>,
    #[serde(default)]
    reset_on_logon: bool,
    #[serde(default)]
    seq_file: Option<String>,
    #[serde(default)]
    logon_fields: FixFields,
    #[serde(default)]
    tls_ca_file: Option<String>,
    #[serde(default)]
    tls_server_name: Option<String>,
    #[serde(default = "default_latency")]
    max_latency: Option<f64>,
    #[serde(default)]
    compression: bool,
}

fn default_latency() -> Option<f64> {
    Some(120.0)
}

fn fix_message(msg_type: &str, fields: FixFields) -> ibkit_fix::Message {
    let mut m = ibkit_fix::Message::new(msg_type);
    m.fields = fields;
    m
}

#[napi]
pub async fn fix_connect(args: Value) -> Result<FixCore> {
    use ibkit_fix::rustls;
    use rustls::pki_types::CertificateDer;
    use rustls::pki_types::pem::PemObject;
    let a: FixArgs = from_json(args, "The FIX session settings")?;
    let mut cfg = ibkit_fix::SessionConfig::initiator(&a.sender_comp_id, &a.target_comp_id);
    cfg.heartbeat_interval = Duration::from_secs(a.heartbeat_interval.unwrap_or(30).max(1));
    cfg.reset_on_logon = a.reset_on_logon;
    cfg.logon_fields = a.logon_fields;
    cfg.max_latency = a.max_latency.map(|s| Duration::from_secs_f64(s.max(0.0)));
    cfg.compression = a.compression;
    let tls = match &a.tls_ca_file {
        Some(path) => {
            let mut roots = rustls::RootCertStore::empty();
            let certs = CertificateDer::pem_file_iter(path)
                .and_then(|it| it.collect::<std::result::Result<Vec<_>, _>>())
                .map_err(|e| {
                    invalid(format!(
                        "Cannot read PEM certificates from tlsCaFile={path:?}: {e}. Pass a file with the CA or server certificate in PEM form."
                    ))
                })?;
            for cert in certs {
                roots.add(cert).map_err(|e| {
                    invalid(format!(
                        "tlsCaFile={path:?} holds a certificate rustls rejects: {e}"
                    ))
                })?;
            }
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(|e| kind_error(ibkit::ErrorKind::Other.name(), e))?
                .with_root_certificates(roots)
                .with_no_client_auth();
            Some(Arc::new(config))
        }
        None => None,
    };
    let store: Box<dyn ibkit_fix::SeqStore> = match a.seq_file {
        Some(p) => Box::new(ibkit_fix::FileStore(p.into())),
        None => Box::new(ibkit_fix::MemoryStore::default()),
    };
    let addr = format!("{}:{}", a.host, a.port);
    let connected = match tls {
        Some(tls) => {
            let name = a.tls_server_name.unwrap_or_else(|| a.host.clone());
            ibkit_fix::connect_tls(&addr, &name, tls, cfg, store).await
        }
        None => ibkit_fix::connect(&addr, cfg, store).await,
    };
    let (handle, rx) = connected.map_err(|e| {
        kind_error(
            ibkit::ErrorKind::Connection.name(),
            format!("Cannot connect to the FIX gateway at {addr}: {e}"),
        )
    })?;
    Ok(FixCore {
        handle,
        events: Arc::new(tokio::sync::Mutex::new(rx)),
    })
}

#[napi]
impl FixCore {
    #[napi]
    pub fn send(&self, msg_type: String, fields: Value) -> Result<()> {
        let fields: FixFields = from_json(fields, "The FIX fields ([tag, value] pairs)")?;
        self.handle
            .send(fix_message(&msg_type, fields))
            .map_err(|e| kind_error(ibkit::ErrorKind::Connection.name(), e))
    }

    #[napi]
    pub fn logout(&self, text: String) -> Result<()> {
        self.handle
            .logout(&text)
            .map_err(|e| kind_error(ibkit::ErrorKind::Connection.name(), e))
    }

    /// The next event, or `null` after `timeout_ms`.
    #[napi]
    pub async fn poll(&self, timeout_ms: u32) -> Result<Value> {
        let got = tokio::time::timeout(Duration::from_millis(u64::from(timeout_ms)), async {
            self.events.lock().await.recv().await
        })
        .await
        .ok()
        .flatten();
        Ok(match got {
            Some(ibkit_fix::FixEvent::App(m)) => {
                json!({"type": "app", "msgType": m.msg_type, "fields": m.fields})
            }
            Some(ibkit_fix::FixEvent::Session(s)) => {
                json!({"type": "session", "event": format!("{s:?}")})
            }
            Some(ibkit_fix::FixEvent::Disconnected(r)) => {
                json!({"type": "disconnected", "reason": r})
            }
            None => Value::Null,
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

#[derive(serde::Deserialize)]
struct FixLeg(Contract, f64, String, String);

fn fix_fields(
    m: std::result::Result<ibkit_fix::Message, ibkit_fix::ib::MappingError>,
) -> Result<FixFields> {
    m.map(|m| m.fields).map_err(|e| invalid(e.0))
}

/// Build the fields of an IB FIX message by kind (see `ts/fix.ts`).
#[napi]
pub fn fix_build(kind: String, args: Vec<Value>) -> Result<Value> {
    use ibkit_fix::ib;
    dispatch!(kind, args, "The FIX builder", {
        "newOrderSingle" => |cl_ord_id: String, contract: Contract, order: Order, extras: Option<FixExtras>| {
            let x = extras.unwrap_or_default();
            let allocations: Vec<(&str, f64)> = x.allocations.iter().map(|(a, q)| (a.as_str(), *q)).collect();
            let extras = ib::OrderExtras {
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
            fix_fields(ib::new_order_single_with(&cl_ord_id, &contract, &order, &extras))?
        },
        "optionExercise" => |cl_ord_id: String, contract: Contract, order: Order, exercise: String, revocable: bool| {
            let kind = match exercise.as_str() {
                "exercise" => ib::ExerciseType::Exercise,
                "lapse" => ib::ExerciseType::Lapse,
                other => return Err(invalid(format!("exercise is {other:?}; use 'exercise' or 'lapse'"))),
            };
            fix_fields(ib::option_exercise(&cl_ord_id, &contract, &order, kind, revocable))?
        },
        "cancelReplace" => |orig_cl_ord_id: String, cl_ord_id: String, contract: Contract, order: Order|
            fix_fields(ib::cancel_replace(&orig_cl_ord_id, &cl_ord_id, &contract, &order))?,
        "cancelRequest" => |orig_cl_ord_id: String, cl_ord_id: String, account: String, contract: Contract, action: String, quantity: f64|
            fix_fields(ib::cancel_request(&orig_cl_ord_id, &cl_ord_id, &account, &contract, &action, quantity))?,
        "orderStatusRequest" => |account: String, cl_ord_id: String| ib::order_status_request(&account, &cl_ord_id).fields,
        "newOrderMultileg" => |cl_ord_id: String, combo: Contract, legs: Vec<FixLeg>, order: Order| {
            let legs: Vec<ib::Leg<'_>> = legs.iter().map(|FixLeg(contract, ratio, action, open_close)| ib::Leg { contract, ratio: *ratio, action, open_close }).collect();
            fix_fields(ib::new_order_multileg(&cl_ord_id, &combo, &legs, &order))?
        },
        "multilegCancelReplace" => |orig_cl_ord_id: String, cl_ord_id: String, combo: Contract, order: Order|
            fix_fields(ib::multileg_cancel_replace(&orig_cl_ord_id, &cl_ord_id, &combo, &order))?,
        "allocationInstruction" => |alloc_id: String, trans_type: String, ref_alloc_id: Option<String>, account: String, contract: Contract, action: String, quantity: f64, avg_px: f64, trade_date: String, allocations: Vec<(String, f64)>| {
            let kind = match trans_type.as_str() {
                "new" => ib::AllocTransType::New,
                "replace" => ib::AllocTransType::Replace,
                "cancel" => ib::AllocTransType::Cancel,
                other => return Err(invalid(format!("transType is {other:?}; use 'new', 'replace' or 'cancel'"))),
            };
            let allocs: Vec<(&str, f64)> = allocations.iter().map(|(a, q)| (a.as_str(), *q)).collect();
            fix_fields(ib::allocation_instruction(&alloc_id, kind, ref_alloc_id.as_deref(), &account, &contract, &action, quantity, avg_px, &trade_date, &allocs))?
        },
    })
}

// ---- testing -------------------------------------------------------------------

/// One scripted answer of the fake TWS server: when a request's first field
/// is `msgId` (and, when set, its field `field` equals `equals`), the server
/// sends `reply`. In reply fields `{n}` is replaced by request field `n`.
#[derive(serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FakeRule {
    msg_id: String,
    #[serde(default)]
    field: Option<usize>,
    #[serde(default)]
    equals: Option<String>,
    reply: Vec<Vec<String>>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct FakeArgs {
    server_version: i32,
    next_valid_id: i32,
    accounts: String,
    rules: Vec<FakeRule>,
}

impl Default for FakeArgs {
    fn default() -> Self {
        Self {
            server_version: 178,
            next_valid_id: 1,
            accounts: "DU123456".into(),
            rules: Vec::new(),
        }
    }
}

/// In-process fake TWS server for tests.
#[napi]
pub struct FakeTws {
    inner: Option<ibkit::tws::testing::FakeTws>,
    port: u16,
    requests: Arc<parking_lot::Mutex<Vec<Vec<String>>>>,
}

#[napi]
pub async fn fake_tws(args: Option<Value>) -> Result<FakeTws> {
    let a: FakeArgs = from_json(args.unwrap_or(json!({})), "The fake TWS settings")?;
    let requests = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let seen = requests.clone();
    let rules = a.rules;
    let responder: ibkit::tws::testing::Responder = Arc::new(move |req: &[String]| {
        seen.lock().push(req.to_vec());
        let Some(rule) = rules.iter().find(|rule| {
            req.first() == Some(&rule.msg_id)
                && rule
                    .field
                    .zip(rule.equals.as_ref())
                    .is_none_or(|(i, v)| req.get(i) == Some(v))
        }) else {
            return Vec::new();
        };
        rule.reply
            .iter()
            .map(|fields| {
                fields
                    .iter()
                    .map(|f| {
                        f.strip_prefix('{')
                            .and_then(|f| f.strip_suffix('}'))
                            .and_then(|n| n.parse::<usize>().ok())
                            .and_then(|n| req.get(n).cloned())
                            .unwrap_or_else(|| f.clone())
                    })
                    .collect()
            })
            .collect()
    });
    let cfg = ibkit::tws::testing::FakeTwsConfig {
        server_version: a.server_version,
        next_valid_id: a.next_valid_id,
        accounts: a.accounts,
    };
    let inner = ibkit::tws::testing::FakeTws::with(cfg, responder).await;
    let port = inner.port();
    Ok(FakeTws {
        inner: Some(inner),
        port,
        requests,
    })
}

#[napi]
impl FakeTws {
    #[napi(getter)]
    pub fn port(&self) -> u16 {
        self.port
    }
    /// Every request received so far, as field lists.
    #[napi]
    pub fn requests(&self) -> Vec<Vec<String>> {
        self.requests.lock().clone()
    }
    /// Push one message (a list of fields) to connected clients.
    #[napi]
    pub fn send(&self, fields: Vec<String>) {
        if let Some(t) = &self.inner {
            t.send(fields);
        }
    }
    #[napi]
    pub fn close(&mut self) {
        self.inner = None;
    }
}

/// Version of the native addon.
#[napi]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
