//! FIX 4.2 engine for Interactive Brokers.
//!
//! * [`message`]: zero-copy parsing with BodyLength and CheckSum validation,
//!   encoding, stream splitting, FIXCOMP (zlib) blocks.
//! * [`session`]: a sans-IO session state machine (logon, heartbeats, test
//!   requests, resend requests with gap fill, sequence resets, logout).
//! * [`transport`]: an async TCP or TLS initiator or acceptor around the session.
//! * [`ib`]: the order, cancel, replace, status, multileg and allocation
//!   messages IB's FIX engine accepts, built from `ibkit_tws` contracts and
//!   orders as the IBKR FIX Implementation Manual specifies, and typed views
//!   of the messages IB sends (ExecutionReport, OrderCancelReject, Bulletin,
//!   AllocationAck).

pub mod ib;
pub mod message;
pub mod session;
pub mod tags;
pub mod transport;

pub use message::{Message, MessageView, ParseError};
pub use session::{
    FileStore, MemoryStore, Output, SeqStore, Session, SessionConfig, SessionEvent, State,
};
pub use transport::{FixEvent, FixHandle, connect, connect_tls, run, rustls};
