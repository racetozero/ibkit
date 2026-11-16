//! ibkit: a Rust rewrite of ib_async / ib_insync for the Interactive Brokers
//! TWS API.
//!
//! [`IB`] connects to TWS or IB Gateway, synchronizes positions, orders,
//! account values and executions, and keeps them current. Market data streams
//! into live [`Ticker`]s; orders are tracked as [`Trade`]s. The low-level
//! protocol client is re-exported as [`tws`].
//!
//! ```no_run
//! use ibkit::{IB, Contract, Order};
//! # async fn demo() -> ibkit::Result<()> {
//! let ib = IB::connect("127.0.0.1", 7497, 1).await?;
//! let aapl = ib.qualify_contracts(&[Contract::stock("AAPL", "SMART", "USD")]).await?.remove(0).unwrap();
//! let ticker = ib.req_mkt_data(&aapl, "", false, false, &[])?;
//! let trade = ib.place_order(&aapl, Order::limit("BUY", 100.0, 150.0))?;
//! println!("{:?} {:?}", ticker.read().bid, trade.lock().order_status.status);
//! # Ok(()) }
//! ```

// The TWS API has many positional parameters; the methods mirror ib_async.
#![allow(clippy::too_many_arguments)]

pub mod ib;
pub mod objects;
pub mod order;
pub mod state;
pub mod ticker;
pub mod trade;

pub use ib::{ACCOUNT_SUMMARY_TAGS, Error, IB, IbOptions, Result, StartupFetch};
pub use ibkit_tws as tws;
pub use ibkit_tws::types::*;
pub use ibkit_tws::{ConnectOptions, ErrorKind, Incoming};
pub use objects::*;
pub use order::{BracketOrder, one_cancels_all};
pub use state::{
    BarsHandle, Event, OrderKey, PnLHandle, PnLSingleHandle, RealTimeBarsHandle, RequestError,
    ScanHandle, TickerHandle, TradeHandle,
};
pub use ticker::{Defaults, Ticker};
pub use trade::{OrderStatus, Trade, status};
