//! Async socket client for TWS and IB Gateway (the equivalent of IB's `EClient`).
//!
//! [`Client::connect`] performs the handshake, sends START_API, and returns once
//! the server has sent both `nextValidId` and `managedAccounts`, like ib_async.
//! A reader task decodes incoming frames; a writer task sends requests subject
//! to a throttle (by default 45 requests per second, the limit TWS enforces).

use crate::consts::UNSET_INTEGER;
use crate::decode::{Incoming, decode};
use crate::encode::{self, Unsupported};
use crate::encode_pb;
use crate::server_versions as sv;
use crate::server_versions::{MAX_CLIENT_VER, MIN_CLIENT_VER};
use crate::types::*;
use crate::wire::{DecodeError, FieldReader, FieldWriter, FrameBuffer};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Errors of the socket client.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "could not connect to {addr}: {source}. Check that TWS or IB Gateway runs and that the API port is enabled (Configure > API > Settings)"
    )]
    Connect {
        addr: String,
        source: std::io::Error,
    },
    #[error("socket error: {0}")]
    Io(#[from] std::io::Error),
    #[error(
        "the API connection was not ready within {0:?}. Is client id already in use, or does TWS show a confirmation dialog?"
    )]
    Timeout(Duration),
    #[error(
        "server version {server} is below the minimum {min} this client supports; upgrade TWS or IB Gateway"
    )]
    ServerVersion { server: i32, min: i32 },
    #[error("the connection is closed: {0}")]
    Closed(String),
    #[error(transparent)]
    Unsupported(#[from] Unsupported),
    #[error("could not decode a server message: {0}")]
    Decode(#[from] DecodeError),
}

/// The class of an error, shared by the Python and Node bindings: each
/// binding raises the error class that `name` gives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    /// IB answered a request with an error message.
    Request,
    /// The connection is closed or could not be opened.
    Connection,
    /// An answer did not arrive in time.
    Timeout,
    /// An argument has the wrong form or value.
    InvalidArgument,
    /// Any other error.
    Other,
}

impl ErrorKind {
    /// The error class name in the bindings.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Request => "RequestError",
            Self::Connection => "ConnectionError",
            Self::Timeout => "TimeoutError",
            Self::InvalidArgument => "InvalidArgumentError",
            Self::Other => "IbkitError",
        }
    }
}

impl Error {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Connect { .. } | Self::Closed(_) | Self::Io(_) => ErrorKind::Connection,
            Self::Timeout(_) => ErrorKind::Timeout,
            Self::ServerVersion { .. } | Self::Unsupported(_) | Self::Decode(_) => ErrorKind::Other,
        }
    }
}

/// Parameters for [`Client::connect`].
#[derive(Clone, Debug)]
pub struct ConnectOptions {
    pub host: String,
    pub port: u16,
    pub client_id: i32,
    /// Time allowed for the TCP connect plus the API handshake.
    pub timeout: Duration,
    /// Extra handshake options, for example `"+PACEAPI"`.
    pub connect_options: String,
    /// Optional capabilities string sent with START_API.
    pub optional_capabilities: String,
    /// Throttle: at most this many requests per `requests_interval`; 0 disables.
    pub max_requests: u32,
    pub requests_interval: Duration,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 7497,
            client_id: 1,
            timeout: Duration::from_secs(4),
            connect_options: String::new(),
            optional_capabilities: String::new(),
            max_requests: 45,
            requests_interval: Duration::from_secs(1),
        }
    }
}

impl ConnectOptions {
    pub fn new(host: impl Into<String>, port: u16, client_id: i32) -> Self {
        Self {
            host: host.into(),
            port,
            client_id,
            ..Default::default()
        }
    }
}

/// Byte and message counters of a connection.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ConnectionStats {
    pub start_time: f64,
    pub duration: f64,
    pub num_bytes_recv: u64,
    pub num_bytes_sent: u64,
    pub num_msg_recv: u64,
    pub num_msg_sent: u64,
}

/// Messages from the reader task. Each item is one batch: all messages decoded
/// from one socket read, so that consumers can coalesce updates per packet.
pub struct EventReceiver {
    rx: mpsc::UnboundedReceiver<Vec<Incoming>>,
    pending: VecDeque<Incoming>,
}

impl std::fmt::Debug for EventReceiver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventReceiver")
            .field("pending", &self.pending.len())
            .finish()
    }
}

impl EventReceiver {
    /// Next batch of messages; `None` after the connection closed.
    pub async fn recv_batch(&mut self) -> Option<Vec<Incoming>> {
        if !self.pending.is_empty() {
            return Some(self.pending.drain(..).collect());
        }
        self.rx.recv().await
    }

    /// Next single message; `None` after the connection closed.
    pub async fn recv(&mut self) -> Option<Incoming> {
        loop {
            if let Some(m) = self.pending.pop_front() {
                return Some(m);
            }
            self.pending.extend(self.rx.recv().await?);
        }
    }

    /// Next message if one is already available.
    pub fn try_recv(&mut self) -> Option<Incoming> {
        loop {
            if let Some(m) = self.pending.pop_front() {
                return Some(m);
            }
            self.pending.extend(self.rx.try_recv().ok()?);
        }
    }
}

struct Inner {
    client_id: i32,
    server_version: i32,
    connection_time: String,
    next_id: AtomicI32,
    accounts: Mutex<Vec<String>>,
    tx: mpsc::UnboundedSender<Vec<u8>>,
    /// `None` while connected; the reason once closed.
    close_reason: Mutex<Option<String>>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    started: Instant,
    start_unix: f64,
    bytes_recv: AtomicU64,
    bytes_sent: AtomicU64,
    msgs_recv: AtomicU64,
    msgs_sent: AtomicU64,
}

/// Connected TWS API client. Cheap to clone; all clones share the connection.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("client_id", &self.inner.client_id)
            .field("server_version", &self.inner.server_version)
            .field("connected", &self.is_connected())
            .finish()
    }
}

async fn read_frame(rd: &mut OwnedReadHalf, fb: &mut FrameBuffer) -> Result<Vec<u8>, Error> {
    loop {
        if let Some(f) = fb.next_frame()? {
            return Ok(f.to_vec());
        }
        let buf = fb.read_buf(4096);
        if rd.read_buf(buf).await? == 0 {
            return Err(Error::Closed(
                "peer closed the connection during the handshake".into(),
            ));
        }
    }
}

impl Client {
    /// Connect, perform the API handshake and wait until the API is ready.
    pub async fn connect(opts: ConnectOptions) -> Result<(Client, EventReceiver), Error> {
        let timeout = opts.timeout;
        match tokio::time::timeout(timeout, Self::connect_inner(opts)).await {
            Ok(r) => r,
            Err(_) => Err(Error::Timeout(timeout)),
        }
    }

    async fn connect_inner(opts: ConnectOptions) -> Result<(Client, EventReceiver), Error> {
        let addr = format!("{}:{}", opts.host, opts.port);
        let stream = TcpStream::connect(&addr)
            .await
            .map_err(|source| Error::Connect {
                addr: addr.clone(),
                source,
            })?;
        stream.set_nodelay(true)?;
        let (mut rd, mut wr) = stream.into_split();
        wr.write_all(&encode::handshake(
            MIN_CLIENT_VER,
            MAX_CLIENT_VER,
            &opts.connect_options,
        ))
        .await?;

        let mut fb = FrameBuffer::new();
        let hello = read_frame(&mut rd, &mut fb).await?;
        let mut r = FieldReader::new(&hello);
        let server_version = r.int()?;
        let connection_time = r.string().unwrap_or_default();
        if server_version < MIN_CLIENT_VER {
            return Err(Error::ServerVersion {
                server: server_version,
                min: MIN_CLIENT_VER,
            });
        }

        let start =
            match encode_pb::start_api(server_version, opts.client_id, &opts.optional_capabilities)
            {
                Some(frame) => frame,
                None => {
                    let mut w = FieldWriter::new();
                    encode::start_api(&mut w, opts.client_id, &opts.optional_capabilities);
                    text_frame(server_version, w.take())
                }
            };
        wr.write_all(&start).await?;

        // Read until both nextValidId and managedAccounts arrived; keep every
        // message for the consumer.
        let mut early: Vec<Incoming> = Vec::new();
        let mut next_id = None;
        let mut accounts = None;
        let mut msgs = 0u64;
        while next_id.is_none() || accounts.is_none() {
            let payload = read_frame(&mut rd, &mut fb).await?;
            msgs += 1;
            let m = decode(server_version, &payload)?;
            match &m {
                Incoming::NextValidId { order_id } => next_id = Some(*order_id),
                Incoming::ManagedAccounts { accounts: a } => {
                    accounts = Some(
                        a.split(',')
                            .filter(|s| !s.is_empty())
                            .map(String::from)
                            .collect::<Vec<_>>(),
                    )
                }
                _ => {}
            }
            early.push(m);
        }

        let (tx, wrx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (etx, erx) = mpsc::unbounded_channel::<Vec<Incoming>>();
        let start_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or_default();
        let inner = Arc::new(Inner {
            client_id: opts.client_id,
            server_version,
            connection_time,
            next_id: AtomicI32::new(next_id.unwrap_or(1)),
            accounts: Mutex::new(accounts.unwrap_or_default()),
            tx,
            close_reason: Mutex::new(None),
            tasks: Mutex::new(Vec::new()),
            started: Instant::now(),
            start_unix,
            bytes_recv: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            msgs_recv: AtomicU64::new(msgs),
            msgs_sent: AtomicU64::new(1),
        });
        let _ = etx.send(early);

        let reader = tokio::spawn(reader_task(Arc::downgrade(&inner), rd, fb, etx));
        let writer = tokio::spawn(writer_task(
            Arc::downgrade(&inner),
            wr,
            wrx,
            opts.max_requests,
            opts.requests_interval,
        ));
        inner.tasks.lock().extend([reader, writer]);
        Ok((
            Client { inner },
            EventReceiver {
                rx: erx,
                pending: VecDeque::new(),
            },
        ))
    }

    pub fn client_id(&self) -> i32 {
        self.inner.client_id
    }

    pub fn server_version(&self) -> i32 {
        self.inner.server_version
    }

    /// Connection time string sent by the server during the handshake.
    pub fn connection_time(&self) -> &str {
        &self.inner.connection_time
    }

    pub fn is_connected(&self) -> bool {
        self.inner.close_reason.lock().is_none()
    }

    /// Why the connection closed, once it has.
    pub fn close_reason(&self) -> Option<String> {
        self.inner.close_reason.lock().clone()
    }

    /// Next request or order id. Request ids and order ids share one sequence
    /// that starts at the server's `nextValidId`.
    pub fn next_req_id(&self) -> i32 {
        self.inner.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Raise the id sequence to at least `min_id`.
    pub fn update_req_id(&self, min_id: i32) {
        self.inner.next_id.fetch_max(min_id, Ordering::Relaxed);
    }

    /// Accounts reported by the server at connect time.
    pub fn managed_accounts(&self) -> Vec<String> {
        self.inner.accounts.lock().clone()
    }

    pub fn connection_stats(&self) -> ConnectionStats {
        let i = &self.inner;
        ConnectionStats {
            start_time: i.start_unix,
            duration: i.started.elapsed().as_secs_f64(),
            num_bytes_recv: i.bytes_recv.load(Ordering::Relaxed),
            num_bytes_sent: i.bytes_sent.load(Ordering::Relaxed),
            num_msg_recv: i.msgs_recv.load(Ordering::Relaxed),
            num_msg_sent: i.msgs_sent.load(Ordering::Relaxed),
        }
    }

    /// Close the connection. Pending messages are dropped.
    pub fn disconnect(&self) {
        close(&self.inner, "disconnected by the client".into());
        for t in self.inner.tasks.lock().drain(..) {
            t.abort();
        }
    }

    /// Queue an encoded frame for the writer task.
    pub fn send_frame(&self, frame: Vec<u8>) -> Result<(), Error> {
        if let Some(reason) = self.close_reason() {
            return Err(Error::Closed(reason));
        }
        self.inner
            .tx
            .send(frame)
            .map_err(|_| Error::Closed("writer stopped".into()))
    }

    fn send(&self, mut w: FieldWriter) -> Result<(), Error> {
        self.send_frame(text_frame(self.server_version(), w.take()))
    }

    /// Send a request that exists only in the protobuf protocol.
    fn send_pb(
        &self,
        frame: Option<Vec<u8>>,
        feature: &'static str,
        min_version: i32,
    ) -> Result<(), Error> {
        let frame = frame.ok_or(Unsupported {
            feature,
            min_version,
            server_version: self.server_version(),
        })?;
        self.send_frame(frame)
    }

    /// Cancel a `reqContractDetails` request (server 215+).
    pub fn cancel_contract_data(&self, req_id: i32) -> Result<(), Error> {
        let f = encode_pb::cancel_contract_data(self.server_version(), req_id);
        self.send_pb(f, "cancelContractData", sv::CANCEL_CONTRACT_DATA)
    }

    /// Cancel a `reqHistoricalTicks` request (server 215+).
    pub fn cancel_historical_ticks(&self, req_id: i32) -> Result<(), Error> {
        let f = encode_pb::cancel_historical_ticks(self.server_version(), req_id);
        self.send_pb(f, "cancelHistoricalTicks", sv::CANCEL_CONTRACT_DATA)
    }

    /// Request the TWS API configuration (server 219+); the answer is
    /// [`Incoming::ConfigResponse`].
    pub fn req_config(&self, req_id: i32) -> Result<(), Error> {
        let f = encode_pb::req_config(self.server_version(), req_id);
        self.send_pb(f, "reqConfig", sv::CONFIG)
    }

    /// Change the TWS API configuration (server 221+). `request` is the
    /// JSON form of IB's `UpdateConfigRequest`; the answer is
    /// [`Incoming::UpdateConfigResponse`].
    pub fn update_config(&self, request: &serde_json::Value) -> Result<(), Error> {
        let f = encode_pb::update_config(self.server_version(), request);
        self.send_pb(f, "updateConfig", sv::UPDATE_CONFIG)
    }
}

/// From server version 201 the message id of a text frame is a raw 4-byte
/// integer instead of a text field.
fn text_frame(server_version: i32, frame: Vec<u8>) -> Vec<u8> {
    if server_version < sv::PROTOBUF {
        return frame;
    }
    let Some(end) = frame[4..].iter().position(|b| *b == 0).map(|p| p + 4) else {
        return frame;
    };
    let id: i32 = std::str::from_utf8(&frame[4..end])
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or(0);
    let rest = &frame[end + 1..];
    let mut out = Vec::with_capacity(8 + rest.len());
    out.extend_from_slice(&((rest.len() + 4) as u32).to_be_bytes());
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(rest);
    out
}

fn close(inner: &Inner, reason: String) {
    inner.close_reason.lock().get_or_insert(reason);
}

async fn reader_task(
    inner: std::sync::Weak<Inner>,
    mut rd: OwnedReadHalf,
    mut fb: FrameBuffer,
    etx: mpsc::UnboundedSender<Vec<Incoming>>,
) {
    let Some(server_version) = inner.upgrade().map(|i| i.server_version) else {
        return;
    };
    let reason = loop {
        // Deliver frames already buffered before reading more.
        let mut batch = Vec::new();
        let mut decode_error = None;
        loop {
            match fb.next_frame() {
                Ok(Some(payload)) => match decode(server_version, payload) {
                    Ok(m) => batch.push(m),
                    Err(e) => {
                        tracing::error!(error = %e, "could not decode a TWS message; it is skipped");
                    }
                },
                Ok(None) => break,
                Err(e) => {
                    decode_error = Some(e.to_string());
                    break;
                }
            }
        }
        if let Some(i) = inner.upgrade() {
            i.msgs_recv.fetch_add(batch.len() as u64, Ordering::Relaxed);
            for m in &batch {
                match m {
                    Incoming::NextValidId { order_id } => {
                        i.next_id.fetch_max(*order_id, Ordering::Relaxed);
                    }
                    Incoming::ManagedAccounts { accounts } => {
                        *i.accounts.lock() = accounts
                            .split(',')
                            .filter(|s| !s.is_empty())
                            .map(String::from)
                            .collect();
                    }
                    Incoming::OpenOrder { order_id, .. } => {
                        i.next_id
                            .fetch_max(order_id.saturating_add(1), Ordering::Relaxed);
                    }
                    _ => {}
                }
            }
        }
        if !batch.is_empty() && etx.send(batch).is_err() {
            break "the event receiver was dropped".to_string();
        }
        if let Some(e) = decode_error {
            break e;
        }
        let buf = fb.read_buf(64 * 1024);
        match rd.read_buf(buf).await {
            Ok(0) => break "the server closed the connection".to_string(),
            Ok(n) => {
                if let Some(i) = inner.upgrade() {
                    i.bytes_recv.fetch_add(n as u64, Ordering::Relaxed);
                }
            }
            Err(e) => break format!("socket read failed: {e}"),
        }
    };
    if let Some(i) = inner.upgrade() {
        close(&i, reason);
    }
}

async fn writer_task(
    inner: std::sync::Weak<Inner>,
    mut wr: OwnedWriteHalf,
    mut rx: mpsc::UnboundedReceiver<Vec<u8>>,
    max_requests: u32,
    interval: Duration,
) {
    let mut times: VecDeque<Instant> = VecDeque::new();
    let mut out: Vec<u8> = Vec::with_capacity(4096);
    while let Some(first) = rx.recv().await {
        let mut queue = VecDeque::from([first]);
        while let Ok(more) = rx.try_recv() {
            queue.push_back(more);
        }
        while !queue.is_empty() {
            let now = Instant::now();
            while times
                .front()
                .is_some_and(|t| now.duration_since(*t) >= interval)
            {
                times.pop_front();
            }
            // 0 disables the throttle.
            let room = if max_requests == 0 {
                queue.len()
            } else {
                (max_requests as usize).saturating_sub(times.len())
            };
            if room == 0 {
                let wait = interval.saturating_sub(now.duration_since(times[0]));
                tracing::debug!(?wait, "throttling requests");
                tokio::time::sleep(wait).await;
                while let Ok(more) = rx.try_recv() {
                    queue.push_back(more);
                }
                continue;
            }
            let n = room.min(queue.len());
            out.clear();
            for frame in queue.drain(..n) {
                out.extend_from_slice(&frame);
            }
            if max_requests != 0 {
                times.extend(std::iter::repeat_n(now, n));
            }
            if let Err(e) = wr.write_all(&out).await {
                if let Some(i) = inner.upgrade() {
                    close(&i, format!("socket write failed: {e}"));
                }
                return;
            }
            if let Some(i) = inner.upgrade() {
                i.bytes_sent.fetch_add(out.len() as u64, Ordering::Relaxed);
                i.msgs_sent.fetch_add(n as u64, Ordering::Relaxed);
            }
        }
    }
    let _ = wr.shutdown().await;
}

macro_rules! requests {
    ($( $(#[$m:meta])* fn $name:ident($($arg:ident : $ty:ty),*) => $enc:ident; )*) => {
        impl Client {
            $(
                $(#[$m])*
                pub fn $name(&self, $($arg: $ty),*) -> Result<(), Error> {
                    if let Some(f) = encode_pb::$enc(self.server_version(), $($arg),*) {
                        return self.send_frame(f);
                    }
                    let mut w = FieldWriter::new();
                    encode::$enc(&mut w, $($arg),*);
                    self.send(w)
                }
            )*
        }
    };
}

/// Requests that stay in the text protocol at every server version.
macro_rules! requests_text {
    ($( $(#[$m:meta])* fn $name:ident($($arg:ident : $ty:ty),*) => $enc:ident; )*) => {
        impl Client {
            $(
                $(#[$m])*
                pub fn $name(&self, $($arg: $ty),*) -> Result<(), Error> {
                    let mut w = FieldWriter::new();
                    encode::$enc(&mut w, $($arg),*);
                    self.send(w)
                }
            )*
        }
    };
}

macro_rules! requests_sv {
    ($( $(#[$m:meta])* fn $name:ident($($arg:ident : $ty:ty),*) => $enc:ident; )*) => {
        impl Client {
            $(
                $(#[$m])*
                pub fn $name(&self, $($arg: $ty),*) -> Result<(), Error> {
                    if let Some(f) = encode_pb::$enc(self.server_version(), $($arg),*) {
                        return self.send_frame(f);
                    }
                    let mut w = FieldWriter::new();
                    encode::$enc(&mut w, self.server_version(), $($arg),*);
                    self.send(w)
                }
            )*
        }
    };
}

macro_rules! requests_sv_try {
    ($( $(#[$m:meta])* fn $name:ident($($arg:ident : $ty:ty),*) => $enc:ident; )*) => {
        impl Client {
            $(
                $(#[$m])*
                pub fn $name(&self, $($arg: $ty),*) -> Result<(), Error> {
                    if let Some(f) = encode_pb::$enc(self.server_version(), $($arg),*) {
                        return self.send_frame(f);
                    }
                    let mut w = FieldWriter::new();
                    encode::$enc(&mut w, self.server_version(), $($arg),*)?;
                    self.send(w)
                }
            )*
        }
    };
}

requests! {
    /// Request streaming or snapshot market data.
    fn req_mkt_data(req_id: i32, contract: &Contract, generic_tick_list: &str, snapshot: bool, regulatory_snapshot: bool, options: &[TagValue]) => req_mkt_data;
    fn cancel_mkt_data(req_id: i32) => cancel_mkt_data;
    fn req_open_orders() => req_open_orders;
    fn req_account_updates(subscribe: bool, acct_code: &str) => req_account_updates;
    fn req_ids(num_ids: i32) => req_ids;
    fn req_mkt_depth(req_id: i32, contract: &Contract, num_rows: i32, is_smart_depth: bool, options: &[TagValue]) => req_mkt_depth;
    fn cancel_mkt_depth(req_id: i32, is_smart_depth: bool) => cancel_mkt_depth;
    fn req_news_bulletins(all_msgs: bool) => req_news_bulletins;
    fn cancel_news_bulletins() => cancel_news_bulletins;
    fn set_server_log_level(log_level: i32) => set_server_log_level;
    fn req_auto_open_orders(auto_bind: bool) => req_auto_open_orders;
    fn req_all_open_orders() => req_all_open_orders;
    fn req_managed_accts() => req_managed_accts;
    fn req_historical_data(req_id: i32, contract: &Contract, end_date_time: &str, duration_str: &str, bar_size_setting: &str, what_to_show: &str, use_rth: bool, format_date: i32, keep_up_to_date: bool, chart_options: &[TagValue]) => req_historical_data;
    fn req_scanner_subscription(req_id: i32, subscription: &ScannerSubscription, options: &[TagValue], filter_options: &[TagValue]) => req_scanner_subscription;
    fn cancel_scanner_subscription(req_id: i32) => cancel_scanner_subscription;
    fn req_scanner_parameters() => req_scanner_parameters;
    fn cancel_historical_data(req_id: i32) => cancel_historical_data;
    fn req_current_time() => req_current_time;
    fn req_real_time_bars(req_id: i32, contract: &Contract, bar_size: i32, what_to_show: &str, use_rth: bool, options: &[TagValue]) => req_real_time_bars;
    fn cancel_real_time_bars(req_id: i32) => cancel_real_time_bars;
    fn req_fundamental_data(req_id: i32, contract: &Contract, report_type: &str, options: &[TagValue]) => req_fundamental_data;
    fn cancel_fundamental_data(req_id: i32) => cancel_fundamental_data;
    fn calculate_implied_volatility(req_id: i32, contract: &Contract, option_price: f64, under_price: f64, options: &[TagValue]) => calculate_implied_volatility;
    fn calculate_option_price(req_id: i32, contract: &Contract, volatility: f64, under_price: f64, options: &[TagValue]) => calculate_option_price;
    fn cancel_calculate_implied_volatility(req_id: i32) => cancel_calculate_implied_volatility;
    fn cancel_calculate_option_price(req_id: i32) => cancel_calculate_option_price;
    /// 1 live, 2 frozen, 3 delayed, 4 delayed frozen.
    fn req_market_data_type(market_data_type: i32) => req_market_data_type;
    fn req_positions() => req_positions;
    fn cancel_positions() => cancel_positions;
    fn req_account_summary(req_id: i32, group_name: &str, tags: &str) => req_account_summary;
    fn cancel_account_summary(req_id: i32) => cancel_account_summary;
    fn verify_request(api_name: &str, api_version: &str) => verify_request;
    fn verify_message(api_data: &str) => verify_message;
    fn query_display_groups(req_id: i32) => query_display_groups;
    fn subscribe_to_group_events(req_id: i32, group_id: i32) => subscribe_to_group_events;
    fn update_display_group(req_id: i32, contract_info: &str) => update_display_group;
    fn unsubscribe_from_group_events(req_id: i32) => unsubscribe_from_group_events;
    fn req_positions_multi(req_id: i32, account: &str, model_code: &str) => req_positions_multi;
    fn cancel_positions_multi(req_id: i32) => cancel_positions_multi;
    fn req_account_updates_multi(req_id: i32, account: &str, model_code: &str, ledger_and_nlv: bool) => req_account_updates_multi;
    fn cancel_account_updates_multi(req_id: i32) => cancel_account_updates_multi;
    fn req_sec_def_opt_params(req_id: i32, underlying_symbol: &str, fut_fop_exchange: &str, underlying_sec_type: &str, underlying_con_id: i32) => req_sec_def_opt_params;
    fn req_soft_dollar_tiers(req_id: i32) => req_soft_dollar_tiers;
    fn req_family_codes() => req_family_codes;
    fn req_matching_symbols(req_id: i32, pattern: &str) => req_matching_symbols;
    fn req_mkt_depth_exchanges() => req_mkt_depth_exchanges;
    fn req_smart_components(req_id: i32, bbo_exchange: &str) => req_smart_components;
    fn req_news_article(req_id: i32, provider_code: &str, article_id: &str, options: &[TagValue]) => req_news_article;
    fn req_news_providers() => req_news_providers;
    fn req_historical_news(req_id: i32, con_id: i32, provider_codes: &str, start_date_time: &str, end_date_time: &str, total_results: i32, options: &[TagValue]) => req_historical_news;
    fn req_head_time_stamp(req_id: i32, contract: &Contract, what_to_show: &str, use_rth: bool, format_date: i32) => req_head_time_stamp;
    fn cancel_head_time_stamp(req_id: i32) => cancel_head_time_stamp;
    fn req_histogram_data(req_id: i32, contract: &Contract, use_rth: bool, time_period: &str) => req_histogram_data;
    fn cancel_histogram_data(req_id: i32) => cancel_histogram_data;
    fn req_market_rule(market_rule_id: i32) => req_market_rule;
    fn req_pnl(req_id: i32, account: &str, model_code: &str) => req_pnl;
    fn cancel_pnl(req_id: i32) => cancel_pnl;
    fn req_pnl_single(req_id: i32, account: &str, model_code: &str, con_id: i32) => req_pnl_single;
    fn cancel_pnl_single(req_id: i32) => cancel_pnl_single;
    fn req_historical_ticks(req_id: i32, contract: &Contract, start_date_time: &str, end_date_time: &str, number_of_ticks: i32, what_to_show: &str, use_rth: bool, ignore_size: bool, misc_options: &[TagValue]) => req_historical_ticks;
    /// `tick_type`: "Last", "AllLast", "BidAsk" or "MidPoint".
    fn req_tick_by_tick_data(req_id: i32, contract: &Contract, tick_type: &str, number_of_ticks: i32, ignore_size: bool) => req_tick_by_tick_data;
    fn cancel_tick_by_tick_data(req_id: i32) => cancel_tick_by_tick_data;
    fn req_completed_orders(api_only: bool) => req_completed_orders;
    fn req_wsh_meta_data(req_id: i32) => req_wsh_meta_data;
    fn cancel_wsh_meta_data(req_id: i32) => cancel_wsh_meta_data;
    fn cancel_wsh_event_data(req_id: i32) => cancel_wsh_event_data;
    fn req_user_info(req_id: i32) => req_user_info;
    /// `fa_data_type`: 1 groups, 3 aliases.
    fn request_fa(fa_data_type: i32) => request_fa;
    fn replace_fa(req_id: i32, fa_data_type: i32, xml: &str) => replace_fa;
}

requests_text! {
    fn verify_and_auth_request(api_name: &str, api_version: &str, opaque_isv_key: &str) => verify_and_auth_request;
    fn verify_and_auth_message(api_data: &str, xyz_response: &str) => verify_and_auth_message;
}

requests_sv! {
    fn cancel_order(order_id: i32, order_cancel: &OrderCancel) => cancel_order;
    fn req_contract_details(req_id: i32, contract: &Contract) => req_contract_details;
    fn req_global_cancel(order_cancel: &OrderCancel) => req_global_cancel;
    fn req_wsh_event_data(req_id: i32, data: &WshEventData) => req_wsh_event_data;
}

requests_sv_try! {
    /// Place a new order or modify an existing one (same order id).
    fn place_order(order_id: i32, contract: &Contract, order: &Order) => place_order;
    fn req_executions(req_id: i32, filter: &ExecutionFilter) => req_executions;
    fn exercise_options(req_id: i32, contract: &Contract, exercise_action: i32, exercise_quantity: i32, account: &str, override_: i32, manual_order_time: &str, customer_account: &str, professional_customer: bool) => exercise_options;
}

impl Client {
    pub fn req_current_time_in_millis(&self) -> Result<(), Error> {
        if let Some(f) = encode_pb::req_current_time_in_millis(self.server_version()) {
            return self.send_frame(f);
        }
        let mut w = FieldWriter::new();
        encode::req_current_time_in_millis(&mut w, self.server_version())?;
        self.send(w)
    }

    /// Cancel an order with default cancel parameters (ib_async's
    /// `cancelOrder(orderId, manualCancelOrderTime)`).
    pub fn cancel_order_simple(
        &self,
        order_id: i32,
        manual_cancel_order_time: &str,
    ) -> Result<(), Error> {
        let oc = OrderCancel {
            manual_order_cancel_time: manual_cancel_order_time.into(),
            ext_operator: String::new(),
            manual_order_indicator: UNSET_INTEGER,
        };
        self.cancel_order(order_id, &oc)
    }
}
