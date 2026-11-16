//! TWS API protocol and client for Interactive Brokers Trader Workstation and
//! IB Gateway.
//!
//! The crate has two layers:
//!
//! * A protocol core with no IO: [`wire`] (field and frame codec), [`encode`]
//!   (one function per client request), [`decode`] (one [`Incoming`] variant per
//!   server message) and the data [`types`]. Layouts follow IB's official
//!   client for server versions 157 through 200; from 201 to 226 requests and
//!   messages use the protobuf protocol ([`encode_pb`], [`decode_pb`],
//!   [`pb`], [`proto`]) where IB's client does.
//! * An async [`Client`] over tokio that performs the handshake, throttles
//!   outgoing requests and streams decoded messages.

// TWS requests carry many positional fields; the API mirrors IB's signatures.
#![allow(clippy::too_many_arguments)]

pub mod client;
pub mod consts;
pub mod decode;
pub mod decode_pb;
pub mod encode;
pub mod encode_pb;
pub mod message;
pub mod pb;
#[rustfmt::skip]
pub mod proto;
pub mod server_versions;
pub mod testing;
pub mod tick_type;
pub mod types;
pub mod wire;

pub use client::{Client, ConnectOptions, Error, ErrorKind};
pub use decode::{Incoming, decode};
pub use types::*;
