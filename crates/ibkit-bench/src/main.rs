//! Prints throughput of decoding, encoding and the full market data pipeline.
//!
//! Run with `cargo run --release -p ibkit-bench`.

#![allow(clippy::useless_vec)]

use ibkit::tws::testing::{FakeTws, FakeTwsConfig, frame};
use ibkit::tws::wire::{FieldWriter, FrameBuffer};
use ibkit::tws::{decode, encode, tws_msg};
use ibkit::{Contract, Event, IB, IbOptions, Order};
use std::hint::black_box;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn report(name: &str, n: usize, elapsed: Duration) {
    let rate = n as f64 / elapsed.as_secs_f64();
    let per = elapsed.as_nanos() as f64 / n as f64;
    println!("{name:<34} {:>8.2} M msg/s {per:>9.1} ns/msg", rate / 1e6);
}

fn tick_frames(n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..n {
        let price = 150.0 + (i % 100) as f64 * 0.01;
        out.extend(frame(&tws_msg![
            1,
            6,
            1,
            1 + (i % 2),
            price,
            100 + i % 7,
            0
        ]));
    }
    out
}

fn bench_decode_ticks() {
    let n = 2_000_000;
    let bytes = tick_frames(n);
    let t = Instant::now();
    let mut fb = FrameBuffer::new();
    fb.extend(&bytes);
    let mut count = 0;
    while let Ok(Some(p)) = fb.next_frame() {
        black_box(decode(178, p).unwrap());
        count += 1;
    }
    assert_eq!(count, n);
    report("decode TICK_PRICE", n, t.elapsed());
}

fn bench_encode_place_order() {
    let n = 1_000_000;
    let c = Contract::stock("AAPL", "SMART", "USD");
    let o = Order::limit("BUY", 100.0, 150.25);
    let mut w = FieldWriter::new();
    let t = Instant::now();
    for i in 0..n {
        encode::place_order(&mut w, 200, i as i32, &c, &o).unwrap();
        black_box(w.finish());
    }
    report("encode PLACE_ORDER", n, t.elapsed());
}

fn bench_decode_exec_details() {
    let n = 500_000;
    let f = tws_msg![
        11,
        5,
        12,
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
        "0001f4e8.6574.01.01",
        "20260102 10:00:00 US/Eastern",
        "DU123456",
        "ISLAND",
        "BOT",
        100,
        150.5,
        987654321,
        1,
        0,
        100,
        150.5,
        "ref",
        "",
        "",
        "",
        1,
        0
    ];
    let fr = frame(&f);
    let payload = &fr[4..];
    let t = Instant::now();
    for _ in 0..n {
        black_box(decode(178, payload).unwrap());
    }
    report("decode EXECUTION_DATA", n, t.elapsed());
}

fn bench_fix_parse() {
    use ibkit_fix::{Message, MessageView};
    let mut er = Message::new("8");
    for (t, v) in [
        (37, "99"),
        (11, "C1"),
        (17, "E1"),
        (20, "0"),
        (150, "2"),
        (39, "2"),
        (55, "AAPL"),
        (54, "1"),
        (151, "0"),
        (14, "2"),
        (6, "3.5"),
        (32, "2"),
        (31, "3.5"),
    ] {
        er.push(t, v);
    }
    let mut raw = Vec::new();
    er.encode(
        "FIX.4.2",
        &[
            (49, "IB"),
            (56, "CLIENT"),
            (34, "12"),
            (52, "20260101-00:00:00.000"),
        ],
        &mut raw,
    );
    let n = 1_000_000;
    let t = Instant::now();
    for _ in 0..n {
        black_box(MessageView::parse(black_box(&raw)).unwrap());
    }
    report("FIX parse ExecutionReport", n, t.elapsed());

    // Splitting: a 256 KiB read holds about 1,000 messages.
    let read: Vec<u8> = raw.iter().copied().cycle().take(raw.len() * 1000).collect();
    let rounds = 200;
    let t = Instant::now();
    let mut count = 0;
    for _ in 0..rounds {
        let mut splitter = ibkit_fix::message::FrameSplitter::default();
        splitter.extend(&read);
        while let Some(m) = splitter.next_message() {
            black_box(m);
            count += 1;
        }
    }
    assert_eq!(count, rounds * 1000);
    report("FIX split 1,000-message read", count, t.elapsed());
}

async fn bench_pipeline() {
    let tws = FakeTws::with(FakeTwsConfig::default(), Arc::new(|_| vec![])).await;
    let opts = IbOptions {
        connect: ibkit::ConnectOptions::new("127.0.0.1", tws.port(), 1),
        fetch: ibkit::StartupFetch {
            positions: false,
            orders_open: false,
            orders_complete: false,
            account_updates: false,
            sub_account_updates: false,
            executions: false,
        },
        ..Default::default()
    };
    let ib = IB::connect_with(opts).await.unwrap();
    let c = Contract {
        con_id: 265598,
        ..Contract::stock("AAPL", "SMART", "USD")
    };
    let ticker = ib.req_mkt_data(&c, "", false, false, &[]).unwrap();
    let n = 1_000_000;
    // Request id 1 is the first id after nextValidId=1. The last tick is a
    // trade at a sentinel price; the run ends when the ticker shows it.
    let mut bytes = tick_frames(n - 1);
    bytes.extend(frame(&tws_msg![1, 6, 1, 4, 999.75, 1, 0]));
    let mut events = ib.events();
    let t = Instant::now();
    for chunk in bytes.chunks(1 << 20) {
        tws.send_raw(chunk.to_vec());
    }
    loop {
        if let Ok(Event::PendingTickers(_)) = events.recv().await
            && ticker.read().last == 999.75
        {
            break;
        }
    }
    report("socket -> decode -> Ticker", n, t.elapsed());
}

fn main() {
    println!("ibkit benchmarks (single thread unless noted)");
    bench_decode_ticks();
    bench_decode_exec_details();
    bench_encode_place_order();
    bench_fix_parse();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(bench_pipeline());
}
