use ibkit_gateway_core::client::{
    ClientAction, ClientPhase, ClientRecovery, Snapshot, reconcile_order_ids,
};

#[test]
fn upstream_recovery_requires_current_snapshots_and_restores_only_lost_subscriptions() {
    for code in [1101, 1102] {
        let mut client = ClientRecovery::default();
        client.require_subscription("quotes").unwrap();
        client.require_subscription("account_updates").unwrap();
        let old = client.connected();
        for snapshot in [
            Snapshot::OpenOrders,
            Snapshot::Executions,
            Snapshot::Positions,
        ] {
            assert!(client.snapshot_complete(old, snapshot));
        }
        let report = reconcile_order_ids(&[12], &[12], &[], 0).unwrap();
        client.orders_reviewed(old, &report).unwrap();
        for id in ["quotes", "account_updates"] {
            client.subscription_received(old, id).unwrap();
        }
        assert!(client.can_submit_orders());
        client.require_subscription("quotes").unwrap();
        assert!(
            client.can_submit_orders(),
            "Registering the same subscription must be idempotent"
        );
        assert_eq!(client.notice(1100), ClientAction::WaitForServer);
        assert!(!client.can_submit_orders());
        assert!(!client.snapshot_complete(old, Snapshot::OpenOrders));
        assert_eq!(client.notice(code), ClientAction::Reconcile);
        let current = client.generation();
        assert!(!client.subscription_received(old, "quotes").unwrap());
        assert!(!client.orders_reviewed(old, &report).unwrap());
        assert!(client.orders_reviewed(current, &report).is_err());
        for snapshot in [Snapshot::OpenOrders, Snapshot::Executions] {
            client.snapshot_complete(current, snapshot);
        }
        client.orders_reviewed(current, &report).unwrap();
        assert!(
            !client.can_submit_orders(),
            "Positions are still incomplete"
        );
        client.snapshot_complete(current, Snapshot::Positions);
        if code == 1101 {
            assert!(!client.can_submit_orders());
            assert_eq!(
                client.pending_subscriptions().collect::<Vec<_>>(),
                ["account_updates", "quotes"]
            );
            client.subscription_received(current, "quotes").unwrap();
            assert!(!client.can_submit_orders());
            client
                .subscription_received(current, "account_updates")
                .unwrap();
        } else {
            assert_eq!(client.pending_subscriptions().count(), 0);
        }
        assert!(client.can_submit_orders());
    }
}

#[test]
fn socket_loss_and_port_change_invalidate_old_receipts_and_unresolved_orders_block_trading() {
    let mut client = ClientRecovery::default();
    client.require_subscription("quotes").unwrap();
    let old = client.connected();
    client.disconnected();
    assert_eq!(client.notice(1102), ClientAction::Reconnect);
    assert_eq!(client.phase(), ClientPhase::Disconnected);
    let reconnected = client.connected();
    assert_eq!(client.notice(1102), ClientAction::Reconcile);
    let current = client.generation();
    assert!(!client.snapshot_complete(reconnected, Snapshot::Positions));
    assert!(!client.subscription_received(old, "quotes").unwrap());
    assert_eq!(
        client.pending_subscriptions().collect::<Vec<_>>(),
        ["quotes"]
    );
    for snapshot in [
        Snapshot::OpenOrders,
        Snapshot::Executions,
        Snapshot::Positions,
    ] {
        client.snapshot_complete(current, snapshot);
    }
    client.subscription_received(current, "quotes").unwrap();
    client
        .orders_reviewed(current, &reconcile_order_ids(&[12], &[], &[], 0).unwrap())
        .unwrap();
    assert!(!client.can_submit_orders());
    client
        .orders_reviewed(current, &reconcile_order_ids(&[12], &[], &[12], 1).unwrap())
        .unwrap();
    assert!(
        !client.can_submit_orders(),
        "An unacknowledged intent requires operator review"
    );
    client
        .orders_reviewed(current, &reconcile_order_ids(&[12], &[], &[12], 0).unwrap())
        .unwrap();
    assert!(client.can_submit_orders());
    assert_eq!(client.notice(1300), ClientAction::Reconnect);
    assert!(!client.can_submit_orders());
    assert_eq!(client.notice(2104), ClientAction::None);
    assert_eq!(client.phase(), ClientPhase::Disconnected);
}

#[test]
fn order_presence_checks_keep_missing_intents_and_partial_fill_ids_visible() {
    let report = reconcile_order_ids(&[12, 13, 14], &[12, 99], &[12, 13, 13], 1).unwrap();
    assert_eq!(report.open.into_iter().collect::<Vec<_>>(), [12]);
    assert_eq!(report.executed.into_iter().collect::<Vec<_>>(), [12, 13]);
    assert_eq!(report.unresolved.into_iter().collect::<Vec<_>>(), [14]);
    assert_eq!(report.unacknowledged_intents, 1);
    for ids in [&[0][..], &[-1][..], &[12, 12][..]] {
        assert!(reconcile_order_ids(ids, &[], &[], 0).is_err());
    }
    let mut client = ClientRecovery::default();
    assert!(client.require_subscription("  ").is_err());
    let generation = client.connected();
    assert!(client.subscription_received(generation, "unknown").is_err());
}
