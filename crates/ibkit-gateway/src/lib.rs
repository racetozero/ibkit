// Win32 calls (platform::windows) are the only unsafe code, on Windows only.
#![cfg_attr(not(windows), forbid(unsafe_code))]
#![cfg_attr(windows, deny(unsafe_code))]
pub mod control;
pub mod forwarder;
pub mod gateway;
pub mod monitor;
pub mod openapi;
pub mod platform;
pub mod settings;
pub use gateway::{Gateway, api_handshake, atomic_json, read_status, send_control};
pub use ibkit_gateway_core::*;
