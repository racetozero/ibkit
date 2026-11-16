//! Recovery checks for a trading client's own API callbacks.
//! This module never submits, cancels, or replays an order.
use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientPhase {
    #[default]
    Disconnected,
    WaitingForServer,
    Reconciling,
    Ready,
}

/// Attach this token to requests; retain it when their callbacks arrive.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Generation(u64);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Snapshot {
    OpenOrders,
    Executions,
    Positions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientAction {
    None,
    WaitForServer,
    Reconcile,
    Reconnect,
}

/// Presence of acknowledged permanent IDs, not a quantity or P&L comparison.
#[derive(Debug, Serialize)]
pub struct OrderReport {
    pub open: BTreeSet<i64>,
    pub executed: BTreeSet<i64>,
    pub unresolved: BTreeSet<i64>,
    pub unacknowledged_intents: usize,
}

pub fn reconcile_order_ids(
    expected: &[i64],
    open_orders: &[i64],
    executions: &[i64],
    unacknowledged_intents: usize,
) -> Result<OrderReport> {
    let expected_ids: BTreeSet<_> = expected.iter().copied().collect();
    if expected.iter().any(|id| *id <= 0) || expected_ids.len() != expected.len() {
        bail!(
            "You supplied invalid expected order IDs. Use unique positive broker permanent IDs; track intents without an acknowledgement in unacknowledged_intents."
        );
    }
    let open: BTreeSet<_> = open_orders
        .iter()
        .copied()
        .filter(|id| expected_ids.contains(id))
        .collect();
    let executed: BTreeSet<_> = executions
        .iter()
        .copied()
        .filter(|id| expected_ids.contains(id))
        .collect();
    let unresolved = expected_ids
        .into_iter()
        .filter(|id| !open.contains(id) && !executed.contains(id))
        .collect();
    Ok(OrderReport {
        open,
        executed,
        unresolved,
        unacknowledged_intents,
    })
}

#[derive(Debug, Default)]
pub struct ClientRecovery {
    phase: ClientPhase,
    generation: Generation,
    subscriptions: BTreeSet<String>,
    pending: BTreeSet<String>,
    snapshots: BTreeSet<Snapshot>,
    orders_resolved: bool,
}

impl ClientRecovery {
    pub fn phase(&self) -> ClientPhase {
        self.phase
    }
    pub fn generation(&self) -> Generation {
        self.generation
    }
    pub fn can_submit_orders(&self) -> bool {
        self.phase == ClientPhase::Ready
    }
    pub fn pending_subscriptions(&self) -> impl Iterator<Item = &str> {
        self.pending.iter().map(String::as_str)
    }

    pub fn require_subscription(&mut self, id: impl Into<String>) -> Result<()> {
        let id = id.into();
        if id.trim().is_empty() {
            bail!(
                "You registered an empty subscription ID. Use a nonempty stable client subscription name and register it again."
            );
        }
        if self.subscriptions.insert(id.clone()) {
            self.pending.insert(id);
        }
        self.refresh();
        Ok(())
    }

    /// Call after an authenticated socket connection. Feed subsequent broker
    /// notices as well: a socket connection alone does not prove upstream health.
    pub fn connected(&mut self) -> Generation {
        self.invalidate(true);
        self.phase = ClientPhase::Reconciling;
        self.generation
    }

    pub fn disconnected(&mut self) {
        self.invalidate(true);
        self.phase = ClientPhase::Disconnected;
    }

    pub fn notice(&mut self, code: i32) -> ClientAction {
        match code {
            1100 => {
                if self.phase == ClientPhase::Disconnected {
                    return ClientAction::Reconnect;
                }
                self.invalidate(false);
                self.phase = ClientPhase::WaitingForServer;
                ClientAction::WaitForServer
            }
            1101 | 1102 => {
                if self.phase == ClientPhase::Disconnected {
                    return ClientAction::Reconnect;
                }
                self.invalidate(code == 1101);
                self.phase = ClientPhase::Reconciling;
                ClientAction::Reconcile
            }
            1300 => {
                self.disconnected();
                ClientAction::Reconnect
            }
            _ => ClientAction::None,
        }
    }

    pub fn snapshot_complete(&mut self, generation: Generation, kind: Snapshot) -> bool {
        if !self.accepts(generation) {
            return false;
        }
        self.snapshots.insert(kind);
        self.refresh();
        true
    }

    /// Submit this report only after both current order and execution end markers.
    /// Unknown intents stay blocked; their absence must never trigger replay.
    pub fn orders_reviewed(
        &mut self,
        generation: Generation,
        report: &OrderReport,
    ) -> Result<bool> {
        if !self.accepts(generation) {
            return Ok(false);
        }
        if !self.snapshots.contains(&Snapshot::OpenOrders)
            || !self.snapshots.contains(&Snapshot::Executions)
        {
            bail!(
                "You reviewed orders before their snapshots were complete. Wait for the current open-order and execution end markers, then compare your durable intent ledger with those snapshots."
            );
        }
        self.orders_resolved = report.unresolved.is_empty() && report.unacknowledged_intents == 0;
        self.refresh();
        Ok(true)
    }

    /// A request being sent is insufficient: confirm an actual callback or
    /// application-level acknowledgement for the current subscription.
    pub fn subscription_received(&mut self, generation: Generation, id: &str) -> Result<bool> {
        if !self.accepts(generation) {
            return Ok(false);
        }
        if !self.subscriptions.contains(id) {
            bail!(
                "You acknowledged an unregistered subscription. Register its stable ID before you request and confirm it."
            );
        }
        self.pending.remove(id);
        self.refresh();
        Ok(true)
    }

    fn accepts(&self, generation: Generation) -> bool {
        self.generation == generation
            && matches!(self.phase, ClientPhase::Reconciling | ClientPhase::Ready)
    }

    fn invalidate(&mut self, resubscribe: bool) {
        self.generation.0 = self
            .generation
            .0
            .checked_add(1)
            .expect("Client recovery generation exhausted");
        self.snapshots.clear();
        self.orders_resolved = false;
        if resubscribe {
            self.pending.clone_from(&self.subscriptions);
        }
    }

    fn refresh(&mut self) {
        if matches!(self.phase, ClientPhase::Reconciling | ClientPhase::Ready) {
            self.phase =
                if self.snapshots.len() == 3 && self.orders_resolved && self.pending.is_empty() {
                    ClientPhase::Ready
                } else {
                    ClientPhase::Reconciling
                };
        }
    }
}
