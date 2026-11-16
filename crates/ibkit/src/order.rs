//! Order helpers that combine several orders.

use ibkit_tws::types::Order;

/// Parent, take-profit and stop-loss orders of a bracket.
#[derive(Clone, Debug, PartialEq)]
pub struct BracketOrder {
    pub parent: Order,
    pub take_profit: Order,
    pub stop_loss: Order,
}

/// Put `orders` into one OCA group (ib_async `oneCancelsAll`).
pub fn one_cancels_all(orders: &mut [Order], oca_group: &str, oca_type: i32) {
    for o in orders {
        o.oca_group = oca_group.into();
        o.oca_type = oca_type;
    }
}
