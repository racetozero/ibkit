//! End-to-end tests of the high-level client against the fake TWS server.
//!
//! The server answers like TWS at server version 178. Request fields are
//! indexed as the encoder writes them (message id first).

use ibkit::tws::testing::{FakeTws, FakeTwsConfig};
use ibkit::tws::tws_msg;
use ibkit::*;
use std::sync::Arc;
use std::time::Duration;

const MAX: &str = "1.7976931348623157e308";
const ACCOUNT: &str = "DU123456";
const WAIT: Duration = Duration::from_secs(5);

/// OPEN_ORDER fields at server version 178 (same order as ib_async's decoder).
fn open_order(
    order_id: &str,
    symbol: &str,
    status: &str,
    what_if: bool,
    init_margin_change: &str,
) -> Vec<String> {
    let mut f = tws_msg![
        5, order_id, 265598, symbol, "STK", "", 0, "", "", "SMART", "USD", symbol, "NMS", "BUY",
        100, "LMT", 150, "", "DAY", "", ACCOUNT, "O", 0, "", 1, 777, 0, 0, 0, "", ""
    ];
    f.extend(tws_msg!["", "", "", "", "", "", "", "", 0, "", -1]);
    f.extend(tws_msg![
        0, "", "", "", "", "", 0, 0, 0, 0, "", 0, 0, 0, "", 0, 0
    ]);
    f.extend(tws_msg![
        "", 0, "", "", 0, 0, "", "", "", "", "", 0, 0, 0, "", "", "", "", 0, "", "", 0, 0, ""
    ]);
    f.extend(tws_msg![
        0,
        u8::from(what_if),
        status,
        "",
        "",
        "",
        init_margin_change,
        "",
        "",
        "",
        "",
        "",
        1.5,
        "",
        "",
        "USD",
        ""
    ]);
    f.extend(tws_msg![
        0, 0, 0, "", MAX, MAX, MAX, MAX, MAX, MAX, 0, "", "", "", MAX, 0, 0, 0, 0, "", "", 0, "",
        "", "", "", ""
    ]);
    f
}

fn order_status(
    order_id: &str,
    status: &str,
    filled: f64,
    remaining: f64,
    avg: f64,
) -> Vec<String> {
    tws_msg![
        3, order_id, status, filled, remaining, avg, 777, 0, avg, 1, "", 0
    ]
}

fn responder() -> ibkit::tws::testing::Responder {
    Arc::new(|req: &[String]| {
        let id: i32 = req[0].parse().unwrap();
        let at = |i: usize| req.get(i).cloned().unwrap_or_default();
        match id {
            61 => vec![
                tws_msg![
                    61, 3, ACCOUNT, 8314, "IBM", "STK", "", 0, "", "", "NYSE", "USD", "IBM", "IBM",
                    10, 120.5
                ],
                tws_msg![62, 1],
            ],
            5 => vec![tws_msg![53, 1]],
            99 => vec![tws_msg![102]],
            6 => vec![
                tws_msg![6, 2, "NetLiquidation", "100000", "USD", ACCOUNT],
                tws_msg![54, 1, ACCOUNT],
            ],
            76 => vec![
                tws_msg![73, 1, at(2), ACCOUNT, "", "CashBalance", "5000", "USD"],
                tws_msg![74, 1, at(2)],
            ],
            7 => vec![tws_msg![55, 1, at(2)]],
            9 => {
                let r = at(2);
                if at(4) == "XYZ" {
                    vec![tws_msg![
                        4,
                        2,
                        r,
                        200,
                        "No security definition has been found for the request",
                        ""
                    ]]
                } else {
                    vec![
                        tws_msg![
                            10,
                            r,
                            "AAPL",
                            "STK",
                            "",
                            0,
                            "",
                            "SMART",
                            "USD",
                            "AAPL",
                            "NMS",
                            "NMS",
                            265598,
                            0.01,
                            1,
                            "LMT",
                            "SMART,NASDAQ",
                            1,
                            0,
                            "APPLE INC",
                            "NASDAQ",
                            "",
                            "Technology",
                            "",
                            "",
                            "US/Eastern",
                            "",
                            "",
                            "",
                            "",
                            0,
                            1,
                            "AAPL",
                            "",
                            "26",
                            "",
                            "COMMON",
                            0.0001,
                            0.0001,
                            100
                        ],
                        tws_msg![52, 1, r],
                    ]
                }
            }
            // PLACE_ORDER: [3, orderId, conId, symbol, ...]. The symbol picks the scenario.
            3 => {
                let oid = at(1);
                match at(3).as_str() {
                    "MSFT" => vec![
                        open_order(&oid, "MSFT", "Submitted", false, ""),
                        order_status(&oid, "Submitted", 0.0, 100.0, 0.0),
                    ],
                    "REJ" => vec![tws_msg![
                        4,
                        2,
                        oid,
                        201,
                        "Order rejected - reason: no trading permissions",
                        ""
                    ]],
                    "WHATIF" => vec![open_order(&oid, "WHATIF", "PreSubmitted", true, "2500.5")],
                    _ => vec![
                        open_order(&oid, "AAPL", "Submitted", false, ""),
                        order_status(&oid, "Submitted", 0.0, 100.0, 0.0),
                        tws_msg![
                            11,
                            -1,
                            oid,
                            265598,
                            "AAPL",
                            "STK",
                            "",
                            0,
                            "",
                            "",
                            "ISLAND",
                            "USD",
                            "AAPL",
                            "NMS",
                            "e1",
                            "20260102 10:00:00",
                            ACCOUNT,
                            "ISLAND",
                            "BOT",
                            100,
                            150,
                            777,
                            1,
                            0,
                            100,
                            150,
                            "",
                            "",
                            "",
                            "",
                            1,
                            0
                        ],
                        order_status(&oid, "Filled", 100.0, 0.0, 150.0),
                        tws_msg![59, 1, "e1", 1.0, "USD", MAX, MAX, ""],
                    ],
                }
            }
            // CANCEL_ORDER at 178: [4, 1, orderId, manualCancelTime].
            4 => vec![order_status(&at(2), "Cancelled", 0.0, 100.0, 0.0)],
            20 => {
                let r = at(1);
                let mut out = vec![tws_msg![
                    17, r, "a", "b", 2, "20260102", 1, 2, 0.5, 1.5, 100, 1.2, 10, "20260105", 1.5,
                    2.5, 1, 2, 200, 1.8, 20
                ]];
                if req[req.len() - 2] == "1" {
                    out.push(tws_msg![90, r, 21, "20260105", 1.5, 2.2, 2.6, 1, 1.9, 210]);
                    out.push(tws_msg![90, r, 1, "20260106", 2.2, 2.3, 2.4, 2.1, 2.25, 10]);
                }
                out
            }
            // REQ_MKT_DATA: [1, 11, reqId, contract x12, deltaNeutral, genericTicks, snapshot, ...].
            1 => {
                let r = at(2);
                let mut out = vec![
                    tws_msg![1, 6, r, 1, 150.25, 300, 1],
                    tws_msg![1, 6, r, 2, 150.5, 200, 1],
                    tws_msg![1, 6, r, 4, 150.4, 5, 0],
                    tws_msg![2, 6, r, 8, 12345],
                ];
                if at(17) == "1" {
                    out.push(tws_msg![57, 1, r]);
                }
                out
            }
            10 => {
                let r = at(2);
                vec![
                    tws_msg![13, 1, r, 0, "NSDQ", 0, 1, 150.25, 300, 0],
                    tws_msg![13, 1, r, 1, "ARCA", 0, 1, 150.2, 100, 0],
                    tws_msg![13, 1, r, 0, "NSDQ", 0, 0, 150.5, 200, 0],
                    tws_msg![13, 1, r, 1, "ARCA", 2, 1, 150.2, 0, 0],
                ]
            }
            97 => {
                let r = at(1);
                vec![
                    tws_msg![99, r, 2, 1767225600, 150.5, 100, 2, "ISLAND", ""],
                    tws_msg![99, r, 3, 1767225601, 150.4, 150.6, 10, 20, 0],
                    tws_msg![99, r, 4, 1767225602, 150.5],
                ]
            }
            50 => vec![tws_msg![
                50,
                3,
                at(2),
                1767225600,
                1,
                2,
                0.5,
                1.5,
                100,
                1.2,
                10
            ]],
            92 => vec![tws_msg![94, at(1), 10.5, 20.5, 30.5]],
            22 => vec![tws_msg![
                20,
                3,
                at(1),
                1,
                0,
                265598,
                "AAPL",
                "STK",
                "",
                0,
                "",
                "SMART",
                "USD",
                "AAPL",
                "NMS",
                "NMS",
                "",
                "",
                "",
                ""
            ]],
            62 => vec![
                tws_msg![63, 1, at(2), ACCOUNT, "NetLiquidation", "100000", "USD"],
                tws_msg![64, 1, at(2)],
            ],
            49 => vec![tws_msg![49, 1, 1767225600]],
            _ => vec![],
        }
    })
}

async fn connect_with(request_timeout: Duration) -> (IB, FakeTws) {
    let tws = FakeTws::with(
        FakeTwsConfig {
            server_version: 178,
            next_valid_id: 1000,
            accounts: ACCOUNT.into(),
        },
        responder(),
    )
    .await;
    let opts = IbOptions {
        connect: ConnectOptions::new("127.0.0.1", tws.port(), 1),
        request_timeout: Some(request_timeout),
        ..Default::default()
    };
    (IB::connect_with(opts).await.expect("connect"), tws)
}

async fn connect() -> (IB, FakeTws) {
    connect_with(WAIT).await
}

fn aapl() -> Contract {
    Contract {
        con_id: 265598,
        ..Contract::stock("AAPL", "SMART", "USD")
    }
}

/// Wait for the first event `f` maps to `Some`.
async fn wait_for<T>(
    events: &mut tokio::sync::broadcast::Receiver<Event>,
    mut f: impl FnMut(&Event) -> Option<T>,
) -> T {
    let wait = async {
        loop {
            if let Ok(e) = events.recv().await
                && let Some(v) = f(&e)
            {
                return v;
            }
        }
    };
    tokio::time::timeout(WAIT, wait)
        .await
        .expect("expected event within 5 s")
}

#[tokio::test]
async fn connect_synchronizes_positions_and_account() {
    let (ib, _tws) = connect().await;
    let positions = ib.positions("");
    assert_eq!(positions.len(), 1);
    assert_eq!(
        (
            positions[0].contract.symbol.as_str(),
            positions[0].position,
            positions[0].avg_cost
        ),
        ("IBM", 10.0, 120.5)
    );
    let values = ib.account_values(ACCOUNT);
    assert!(
        values
            .iter()
            .any(|v| v.tag == "NetLiquidation" && v.value == "100000")
    );
    assert!(
        values
            .iter()
            .any(|v| v.tag == "CashBalance" && v.value == "5000")
    );
    assert_eq!(ib.managed_accounts(), vec![ACCOUNT]);
}

#[tokio::test]
async fn qualify_contracts_fills_con_id_and_reports_unknown() {
    let (ib, _tws) = connect().await;
    let q = ib
        .qualify_contracts(&[
            Contract::stock("AAPL", "SMART", "USD"),
            Contract::stock("XYZ", "SMART", "USD"),
        ])
        .await
        .unwrap();
    let aapl = q[0].as_ref().expect("AAPL qualified");
    assert_eq!(
        (
            aapl.con_id,
            aapl.exchange.as_str(),
            aapl.primary_exchange.as_str()
        ),
        (265598, "SMART", "NASDAQ")
    );
    assert!(q[1].is_none());
}

#[tokio::test]
async fn place_order_tracks_trade_to_filled_with_commission() {
    let (ib, _tws) = connect().await;
    let mut events = ib.events();
    let trade = ib
        .place_order(&aapl(), Order::limit("BUY", 100.0, 150.0))
        .unwrap();
    assert_eq!(trade.lock().order_status.status, "PendingSubmit");
    assert!(
        trade.lock().order.order_id >= 1000,
        "order ids continue the nextValidId sequence"
    );
    let report = wait_for(&mut events, |e| {
        if let Event::CommissionReport(_, _, r) = e {
            Some(r.clone())
        } else {
            None
        }
    })
    .await;
    let t = trade.lock();
    assert_eq!(t.order_status.status, "Filled");
    assert!(t.is_done());
    assert_eq!(t.fills.len(), 1);
    assert_eq!(t.filled(), 100.0);
    assert_eq!(t.fills[0].commission_report.commission, 1.0);
    assert_eq!(
        report.realized_pnl, 0.0,
        "unset realized P&L becomes 0 like ib_async"
    );
    let statuses: Vec<&str> = t.log.iter().map(|l| l.status.as_str()).collect();
    assert_eq!(
        statuses,
        ["PendingSubmit", "Submitted", "Submitted", "Filled"]
    );
    drop(t);
    assert_eq!(ib.fills().len(), 1);
    assert_eq!(ib.open_trades().len(), 0);
}

#[tokio::test]
async fn cancel_order_moves_through_pending_cancel_to_cancelled() {
    let (ib, _tws) = connect().await;
    let mut events = ib.events();
    let msft = Contract {
        con_id: 272093,
        ..Contract::stock("MSFT", "SMART", "USD")
    };
    let trade = ib
        .place_order(&msft, Order::limit("BUY", 100.0, 150.0))
        .unwrap();
    wait_for(&mut events, |e| {
        matches!(e, Event::OrderStatus(t) if t.lock().order_status.status == "Submitted")
            .then_some(())
    })
    .await;
    let order = trade.lock().order.clone();
    ib.cancel_order(&order, "").unwrap().expect("known trade");
    assert_eq!(trade.lock().order_status.status, "PendingCancel");
    wait_for(&mut events, |e| {
        matches!(e, Event::Cancelled(_)).then_some(())
    })
    .await;
    assert_eq!(trade.lock().order_status.status, "Cancelled");
    assert!(ib.open_trades().is_empty());
}

#[tokio::test]
async fn order_error_cancels_the_trade_with_the_error_code() {
    let (ib, _tws) = connect().await;
    let mut events = ib.events();
    let rej = Contract::stock("REJ", "SMART", "USD");
    let trade = ib.place_order(&rej, Order::limit("BUY", 1.0, 1.0)).unwrap();
    let code = wait_for(&mut events, |e| {
        if let Event::Error { code, .. } = e {
            Some(*code)
        } else {
            None
        }
    })
    .await;
    assert_eq!(code, 201);
    let t = trade.lock();
    assert_eq!(t.order_status.status, "Cancelled");
    assert_eq!(t.log.last().unwrap().error_code, 201);
}

#[tokio::test]
async fn modifying_a_done_order_is_refused() {
    let (ib, _tws) = connect().await;
    let mut events = ib.events();
    let trade = ib
        .place_order(&aapl(), Order::limit("BUY", 100.0, 150.0))
        .unwrap();
    wait_for(&mut events, |e| matches!(e, Event::Filled(_)).then_some(())).await;
    let order = trade.lock().order.clone();
    let err = ib.place_order(&aapl(), order).unwrap_err();
    assert!(err.to_string().contains("already Filled"), "{err}");
}

#[tokio::test]
async fn what_if_order_returns_margin_impact() {
    let (ib, _tws) = connect().await;
    let state = ib
        .what_if_order(
            &Contract::stock("WHATIF", "SMART", "USD"),
            &Order::limit("BUY", 1.0, 1.0),
        )
        .await
        .unwrap();
    assert_eq!(state.init_margin_change, "2500.5");
    assert_eq!(state.commission, 1.5);
    assert!(ib.trades().is_empty(), "a what-if order creates no trade");
}

#[tokio::test]
async fn bracket_order_links_children_to_the_parent() {
    let (ib, _tws) = connect().await;
    let b = ib.bracket_order("BUY", 10.0, 100.0, 110.0, 95.0).unwrap();
    assert_eq!(
        (b.take_profit.parent_id, b.stop_loss.parent_id),
        (b.parent.order_id, b.parent.order_id)
    );
    assert_eq!(
        (
            b.parent.transmit,
            b.take_profit.transmit,
            b.stop_loss.transmit
        ),
        (false, false, true)
    );
    assert_eq!(
        (
            b.take_profit.action.as_str(),
            b.stop_loss.order_type.as_str(),
            b.stop_loss.aux_price
        ),
        ("SELL", "STP", 95.0)
    );
    assert!(ib.bracket_order("HOLD", 1.0, 1.0, 1.0, 1.0).is_err());
}

#[tokio::test]
async fn market_data_streams_into_ticker() {
    let (ib, _tws) = connect().await;
    let mut events = ib.events();
    let ticker = ib.req_mkt_data(&aapl(), "", false, false, &[]).unwrap();
    wait_for(&mut events, |e| {
        (matches!(e, Event::PendingTickers(_)) && ticker.read().volume == 12345.0).then_some(())
    })
    .await;
    let t = ticker.read();
    assert_eq!(
        (t.bid, t.bid_size, t.ask, t.ask_size, t.last, t.last_size),
        (150.25, 300.0, 150.5, 200.0, 150.4, 5.0)
    );
    assert_eq!(t.midpoint(), 150.375);
    assert_eq!(t.market_price(), 150.4);
    drop(t);
    assert!(ib.ticker(&aapl()).is_some());
    assert!(ib.cancel_mkt_data(&aapl()).unwrap());
    assert!(
        !ib.cancel_mkt_data(&aapl()).unwrap(),
        "a second cancel finds no subscription"
    );
}

#[tokio::test]
async fn snapshot_tickers_complete_on_snapshot_end() {
    let (ib, _tws) = connect().await;
    let tickers = ib.req_tickers(&[aapl()], false).await.unwrap();
    assert_eq!(tickers[0].read().last, 150.4);
}

#[tokio::test]
async fn market_depth_builds_the_book() {
    let (ib, _tws) = connect().await;
    let mut events = ib.events();
    let ticker = ib.req_mkt_depth(&aapl(), 5, false, &[]).unwrap();
    wait_for(&mut events, |e| {
        (matches!(e, Event::PendingTickers(_)) && ticker.read().dom_asks.len() == 1).then_some(())
    })
    .await;
    let t = ticker.read();
    assert_eq!(t.dom_bids.len(), 1, "the second bid level was deleted");
    assert_eq!(
        (t.dom_bids[0].price, t.dom_bids[0].market_maker.as_str()),
        (150.25, "NSDQ")
    );
    assert_eq!(t.dom_asks[0].price, 150.5);
}

#[tokio::test]
async fn tick_by_tick_updates_last_and_quotes() {
    let (ib, _tws) = connect().await;
    let mut events = ib.events();
    let ticker = ib
        .req_tick_by_tick_data(&aapl(), "AllLast", 0, false)
        .unwrap();
    wait_for(&mut events, |e| {
        (matches!(e, Event::PendingTickers(_)) && ticker.read().ask == 150.6).then_some(())
    })
    .await;
    let t = ticker.read();
    assert_eq!((t.last, t.bid, t.ask_size), (150.5, 150.4, 20.0));
    assert!(
        t.tick_by_ticks
            .iter()
            .any(|k| matches!(k, TickByTick::MidPoint { mid_point, .. } if *mid_point == 150.5))
    );
    drop(t);
    assert!(ib.cancel_tick_by_tick_data(&aapl(), "AllLast").unwrap());
}

#[tokio::test]
async fn historical_data_returns_bars() {
    let (ib, _tws) = connect().await;
    let bars = ib
        .req_historical_data(&aapl(), "", "2 D", "1 day", "TRADES", true, 1, false, &[])
        .await
        .unwrap();
    let b = bars.read();
    assert_eq!(b.bars.len(), 2);
    assert_eq!(
        (
            b.bars[1].date.as_str(),
            b.bars[1].close,
            b.bars[1].bar_count
        ),
        ("20260105", 2.0, 20)
    );
}

#[tokio::test]
async fn keep_up_to_date_bars_replace_and_append() {
    let (ib, _tws) = connect().await;
    let mut events = ib.events();
    let bars = ib
        .req_historical_data(&aapl(), "", "2 D", "1 day", "TRADES", true, 1, true, &[])
        .await
        .unwrap();
    wait_for(&mut events, |e| {
        matches!(
            e,
            Event::BarUpdate {
                has_new_bar: true,
                ..
            }
        )
        .then_some(())
    })
    .await;
    let b = bars.read();
    assert_eq!(b.bars.len(), 3);
    assert_eq!(
        (b.bars[1].close, b.bars[1].bar_count),
        (2.2, 21),
        "the update replaced the last bar"
    );
    assert_eq!(b.bars[2].date, "20260106");
    drop(b);
    ib.cancel_historical_data(&bars).unwrap();
}

#[tokio::test]
async fn real_time_bars_stream() {
    let (ib, _tws) = connect().await;
    let mut events = ib.events();
    let bars = ib
        .req_real_time_bars(&aapl(), 5, "TRADES", true, &[])
        .unwrap();
    wait_for(&mut events, |e| {
        matches!(e, Event::BarUpdate { .. }).then_some(())
    })
    .await;
    assert_eq!(bars.read().bars[0].close, 1.5);
    assert_eq!(ib.realtime_bars().len(), 1);
    ib.cancel_real_time_bars(&bars).unwrap();
    assert!(ib.realtime_bars().is_empty());
}

#[tokio::test]
async fn pnl_is_live_and_subscriptions_are_unique() {
    let (ib, _tws) = connect().await;
    let mut events = ib.events();
    let pnl = ib.req_pnl(ACCOUNT, "").unwrap();
    wait_for(&mut events, |e| matches!(e, Event::PnL(_)).then_some(())).await;
    assert_eq!(
        (pnl.read().daily_pnl, pnl.read().realized_pnl),
        (10.5, 30.5)
    );
    assert!(
        ib.req_pnl(ACCOUNT, "")
            .unwrap_err()
            .to_string()
            .contains("already subscribed")
    );
    ib.cancel_pnl(ACCOUNT, "").unwrap();
    assert!(ib.cancel_pnl(ACCOUNT, "").is_err());
}

#[tokio::test]
async fn scanner_data_returns_rows() {
    let (ib, _tws) = connect().await;
    let sub = ScannerSubscription {
        instrument: "STK".into(),
        location_code: "STK.US".into(),
        scan_code: "TOP_PERC_GAIN".into(),
        ..Default::default()
    };
    let rows = ib.req_scanner_data(&sub, &[], &[]).await.unwrap();
    assert_eq!(
        (
            rows.len(),
            rows[0].contract_details.contract.symbol.as_str()
        ),
        (1, "AAPL")
    );
}

#[tokio::test]
async fn account_summary_is_collected() {
    let (ib, _tws) = connect().await;
    let summary = ib.req_account_summary().await.unwrap();
    assert!(
        summary
            .iter()
            .any(|v| v.tag == "NetLiquidation" && v.value == "100000")
    );
}

#[tokio::test]
async fn unanswered_request_times_out() {
    let (ib, _tws) = connect_with(Duration::from_millis(200)).await;
    let err = ib
        .req_head_time_stamp(&aapl(), "TRADES", true, 1)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::Timeout {
                what: "reqHeadTimeStamp",
                ..
            }
        ),
        "{err}"
    );
}

#[tokio::test]
async fn disconnect_fails_requests_in_flight() {
    let (ib, _tws) = connect().await;
    let mut events = ib.events();
    let ib2 = ib.clone();
    let pending =
        tokio::spawn(async move { ib2.req_head_time_stamp(&aapl(), "TRADES", true, 1).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    ib.disconnect();
    wait_for(&mut events, |e| {
        matches!(e, Event::Disconnected(_)).then_some(())
    })
    .await;
    let err = tokio::time::timeout(WAIT, pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(err.to_string().contains("connection closed"), "{err}");
    assert!(ib.req_current_time().await.is_err());
}

#[tokio::test]
async fn current_time() {
    let (ib, _tws) = connect().await;
    assert_eq!(ib.req_current_time().await.unwrap(), 1767225600);
}

#[tokio::test]
async fn a_second_waiter_for_a_shared_reply_is_refused() {
    let (ib, _tws) = connect().await;
    let (first, second) = tokio::join!(ib.req_current_time(), ib.req_current_time());
    assert_eq!(first.unwrap(), 1767225600);
    assert!(second.unwrap_err().to_string().contains("already running"));
}

#[tokio::test]
async fn set_timeout_reports_idle_connection_once() {
    let (ib, _tws) = connect().await;
    let mut events = ib.events();
    ib.set_timeout(Some(Duration::from_millis(100)));
    let idle = wait_for(&mut events, |e| match e {
        Event::Timeout(s) => Some(*s),
        _ => None,
    })
    .await;
    assert!(idle >= 0.1, "idle {idle}");
    // The watch ends after it fires: no second timeout.
    let again = tokio::time::timeout(Duration::from_millis(300), async {
        loop {
            if let Ok(Event::Timeout(_)) = events.recv().await {
                return;
            }
        }
    })
    .await;
    assert!(again.is_err());
    ib.set_timeout(None);
}

/// Server at version 226: every request and answer is protobuf.
fn protobuf_responder() -> ibkit::tws::testing::Responder {
    use ibkit::tws::pb::PbMsg;
    use ibkit::tws::proto::*;
    use ibkit::tws::testing::{protobuf, unhex};
    Arc::new(|req: &[String]| {
        if req.get(1).map(String::as_str) != Some("protobuf") {
            return vec![];
        }
        let body = unhex(&req[2]);
        let m = PbMsg::parse(&body).unwrap();
        match req[0].as_str() {
            "61" => vec![
                protobuf(61, |w| {
                    w.str(position::ACCOUNT, ACCOUNT);
                    w.msg(position::CONTRACT, |w| {
                        w.i32(contract::CON_ID, 8314);
                        w.str(contract::SYMBOL, "IBM");
                        w.str(contract::SEC_TYPE, "STK");
                    });
                    w.str(position::POSITION, "10");
                    w.f64(position::AVG_COST, 120.5);
                }),
                protobuf(62, |_| {}),
            ],
            "5" => vec![protobuf(53, |_| {})],
            "99" => vec![protobuf(102, |_| {})],
            "6" => vec![protobuf(54, |w| {
                w.str(account_data_end::ACCOUNT_NAME, ACCOUNT)
            })],
            "76" => {
                let r = m.int(account_updates_multi_request::REQ_ID);
                vec![protobuf(74, |w| w.i32(account_update_multi_end::REQ_ID, r))]
            }
            "7" => {
                let r = m.int(execution_request::REQ_ID);
                vec![protobuf(55, |w| w.i32(execution_details_end::REQ_ID, r))]
            }
            "3" => {
                let id = m.int(place_order_request::ORDER_ID);
                let symbol = m
                    .msg(place_order_request::CONTRACT)
                    .unwrap()
                    .string(contract::SYMBOL);
                vec![
                    protobuf(5, |w| {
                        w.i32(open_order::ORDER_ID, id);
                        w.msg(open_order::CONTRACT, |w| {
                            w.i32(contract::CON_ID, 265598);
                            w.str(contract::SYMBOL, &symbol);
                            w.str(contract::SEC_TYPE, "STK");
                        });
                        w.msg(open_order::ORDER, |w| {
                            w.i32(order::CLIENT_ID, 1);
                            w.i64(order::PERM_ID, 777);
                            w.str(order::ACTION, "BUY");
                            w.str(order::TOTAL_QUANTITY, "100");
                            w.str(order::ORDER_TYPE, "LMT");
                            w.f64(order::LMT_PRICE, 150.0);
                        });
                        w.msg(open_order::ORDER_STATE, |w| {
                            w.str(order_state::STATUS, "Submitted")
                        });
                    }),
                    protobuf(3, |w| {
                        w.i32(order_status::ORDER_ID, id);
                        w.str(order_status::STATUS, "Submitted");
                        w.str(order_status::FILLED, "0");
                        w.str(order_status::REMAINING, "100");
                        w.i64(order_status::PERM_ID, 777);
                        w.i32(order_status::CLIENT_ID, 1);
                    }),
                ]
            }
            "108" => {
                let r = m.int(config_request::REQ_ID);
                vec![protobuf(110, |w| {
                    w.i32(config_response::REQ_ID, r);
                    w.msg(config_response::API, |w| {
                        w.msg(api_config::SETTINGS, |w| {
                            w.bool(api_settings_config::READ_ONLY_API, true)
                        });
                    });
                })]
            }
            _ => vec![],
        }
    })
}

#[tokio::test]
async fn ib_works_against_a_protobuf_server() {
    let tws = FakeTws::with(
        FakeTwsConfig {
            server_version: 226,
            next_valid_id: 1000,
            accounts: ACCOUNT.into(),
        },
        protobuf_responder(),
    )
    .await;
    let started = std::time::Instant::now();
    let ib = IB::connect_with(IbOptions {
        connect: ConnectOptions::new("127.0.0.1", tws.port(), 1),
        request_timeout: Some(WAIT),
        raise_sync_errors: true,
        ..Default::default()
    })
    .await
    .expect("connect at version 226 answers every startup request");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(ib.client().server_version(), 226);
    let positions = ib.positions("");
    assert_eq!(
        (positions[0].contract.symbol.as_str(), positions[0].position),
        ("IBM", 10.0)
    );

    let mut events = ib.events();
    let trade = ib
        .place_order(&aapl(), Order::limit("BUY", 100.0, 150.0))
        .unwrap();
    wait_for(&mut events, |e| {
        matches!(e, Event::OrderStatus(_)).then_some(())
    })
    .await;
    assert_eq!(trade.lock().order_status.status, "Submitted");

    let config = ib.req_config().await.unwrap();
    assert_eq!(config["api"]["settings"]["readOnlyApi"], true);
}
