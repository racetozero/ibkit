//! Client for the Interactive Brokers Web API (formerly Client Portal API):
//! REST endpoints ([`WebClient`]), WebSocket streaming ([`ws::WsClient`]) and
//! OAuth 1.0a request signing ([`oauth`]), OAuth 2.0 ([`oauth2`]) and the
//! Flex Web Service ([`flex`]).

// Endpoint methods mirror the documented query parameters one to one.
#![allow(clippy::too_many_arguments)]

pub mod account;
pub mod client;
pub mod flex;
pub mod oauth;
pub mod oauth2;
pub mod ws;

pub use client::{GATEWAY_URL, OAUTH_URL, WebClient, WebConfig, WebError, WebOrder};
pub use oauth::OAuthConfig;
pub use oauth2::OAuth2Config;
pub use ws::{WsAuth, WsClient, WsMessage, WsSender};
