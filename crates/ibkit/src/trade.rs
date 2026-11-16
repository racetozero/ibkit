//! Order tracking (ib_async `Trade` and `OrderStatus`).

use crate::objects::{Fill, TradeLogEntry};
use ibkit_tws::types::{Contract, Order};
use serde::{Deserialize, Serialize};

/// Order status strings sent by TWS.
pub mod status {
    pub const PENDING_SUBMIT: &str = "PendingSubmit";
    pub const PENDING_CANCEL: &str = "PendingCancel";
    pub const PRE_SUBMITTED: &str = "PreSubmitted";
    pub const SUBMITTED: &str = "Submitted";
    pub const API_PENDING: &str = "ApiPending";
    pub const API_CANCELLED: &str = "ApiCancelled";
    pub const API_UPDATE: &str = "ApiUpdate";
    pub const CANCELLED: &str = "Cancelled";
    pub const FILLED: &str = "Filled";
    pub const INACTIVE: &str = "Inactive";
    pub const VALIDATION_ERROR: &str = "ValidationError";

    /// Completed, cancelled, or destroyed by risk management.
    pub const DONE_STATES: [&str; 4] = [FILLED, CANCELLED, API_CANCELLED, INACTIVE];
    /// Able to execute at some time in the future.
    pub const ACTIVE_STATES: [&str; 6] = [
        PENDING_SUBMIT,
        API_PENDING,
        PRE_SUBMITTED,
        SUBMITTED,
        VALIDATION_ERROR,
        API_UPDATE,
    ];
    /// Not live yet.
    pub const WAITING_STATES: [&str; 3] = [PENDING_SUBMIT, API_PENDING, PRE_SUBMITTED];
    /// Live and working at the broker.
    pub const WORKING_STATES: [&str; 3] = [SUBMITTED, VALIDATION_ERROR, API_UPDATE];
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderStatus {
    pub order_id: i32,
    pub status: String,
    pub filled: f64,
    pub remaining: f64,
    pub avg_fill_price: f64,
    pub perm_id: i64,
    pub parent_id: i32,
    pub last_fill_price: f64,
    pub client_id: i32,
    pub why_held: String,
    pub mkt_cap_price: f64,
}

/// An order with its contract, status, fills and log.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trade {
    pub contract: Contract,
    pub order: Order,
    pub order_status: OrderStatus,
    pub fills: Vec<Fill>,
    pub log: Vec<TradeLogEntry>,
    pub advanced_error: String,
}

impl Trade {
    pub fn is_waiting(&self) -> bool {
        status::WAITING_STATES.contains(&self.order_status.status.as_str())
    }
    pub fn is_working(&self) -> bool {
        status::WORKING_STATES.contains(&self.order_status.status.as_str())
    }
    pub fn is_active(&self) -> bool {
        status::ACTIVE_STATES.contains(&self.order_status.status.as_str())
    }
    pub fn is_done(&self) -> bool {
        status::DONE_STATES.contains(&self.order_status.status.as_str())
    }

    /// Shares filled so far; for combos only the BAG fills count.
    pub fn filled(&self) -> f64 {
        let bag = self.contract.sec_type == "BAG";
        self.fills
            .iter()
            .filter(|f| !bag || f.contract.sec_type == "BAG")
            .map(|f| f.execution.shares)
            .sum()
    }

    pub fn remaining(&self) -> f64 {
        self.order.total_quantity - self.filled()
    }
}
