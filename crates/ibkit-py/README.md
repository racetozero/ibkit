# ibkit

ibkit is an Interactive Brokers client with a Rust core and Python bindings. It replaces
[ib_async](https://github.com/ib-api-reloaded/ib_async) and
[ib_insync](https://github.com/erdewit/ib_insync) and covers the three IBKR APIs:

| API | Crate | What it does |
| --- | --- | --- |
| TWS API (socket to TWS / IB Gateway) | `ibkit-tws` | Wire protocol, every request and every server message, async client |
| TWS API, high level | `ibkit` | ib_async-style `IB` client: trades, tickers, positions, account values, sync helpers |
| Web API (Client Portal) | `ibkit-web` | REST endpoints, WebSocket streaming, OAuth 1.0a |
| FIX | `ibkit-fix` | FIX 4.2 codec, session layer, async initiator, IB order mapping |
| Python | `ibkit-py` | PyO3 extension plus the `ibkit` Python package with the ib_async API |

## Python quick start

```python
from ibkit import IB, Stock, LimitOrder, util

ib = IB()
ib.connect('127.0.0.1', 7497, clientId=1)

aapl = Stock('AAPL', 'SMART', 'USD')
ib.qualifyContracts(aapl)  # fills conId in place

ticker = ib.reqMktData(aapl)  # live: attributes always current
ib.sleep(2)
print(ticker.bid, ticker.ask, ticker.marketPrice())

bars = ib.reqHistoricalData(aapl, '', '5 D', '1 hour', 'TRADES', useRTH=True)
print(util.df(bars).tail())  # needs pandas

trade = ib.placeOrder(aapl, LimitOrder('BUY', 100, 150.0))
trade.filledEvent += lambda t: print('filled', t.orderStatus.avgFillPrice)
while not trade.isDone():
    ib.waitOnUpdate()
```

The same calls exist as coroutines (`await ib.connectAsync(...)`,
`await ib.qualifyContractsAsync(...)`, ...). Events (`ib.orderStatusEvent`,
`ib.pendingTickersEvent`, `trade.fillEvent`, `ticker.updateEvent`, ...) support
`+=`, `-=`, `await` and `async for`.

The package is typed (`py.typed`; pyright reports 100% type completeness).
Events are generic in their handler arguments: `ib.execDetailsEvent` is
`Event[Trade, Fill]`, so a checker tests each handler, and
`await ib.orderStatusEvent` is a `Trade`. Requests return their classes
(`ib.positions()` is `list[Position]`). Give a new event its argument types,
for example `Event[Trade, Fill]('fills')`: a bare `Event()` has unknown
arguments, so a checker rejects its handlers. Web API bodies are `Json`
(`Any`): IBKR publishes no schema for them.

A blocking call inside a running event loop stops the loop until IB answers,
so it emits a `RuntimeWarning`; await the `...Async` method instead (in a
notebook, call `util.startLoop()` first). Asyncio variants: `IB` (every
request), `AsyncWebClient` (every Web API method), `WsClient.recvAsync`,
`FixSession.connectAsync`, `pollAsync` and `async for`,
`FlexReport.downloadAsync`, and `GatewayClient`.

Errors: `RequestError` (with `reqId`, `code` and `message`), and in
`ibkit.errors` also `ConnectionError`, `TimeoutError`, `InvalidArgumentError`
and `GatewayError` (with `status`). All derive from `IbkitError`; the first
three also derive from the built-in `ConnectionError`, `TimeoutError` and
`ValueError`. They are the same kinds as in `@ibkit/node`.

### Migrating from ib_async

Change `from ib_async import ...` to `from ibkit import ...`. Method names,
argument names, contract and order classes, `Trade`, `Ticker`, `BarDataList`,
order conditions and events keep their ib_async names. Differences:

* Fields of `Trade` and `Ticker` are read from the Rust core on access. They
  are always current; there is no stale copy.
* Sizes are floats (as in ib_async). `Order.usePriceMgmtAlgo` defaults to `None`
  (IB's default) instead of `False`.

## Rust quick start

```rust
use ibkit::{IB, Contract, Order, Event};

#[tokio::main]
async fn main() -> ibkit::Result<()> {
    let ib = IB::connect("127.0.0.1", 7497, 1).await?;
    let aapl = ib.qualify_contracts(&[Contract::stock("AAPL", "SMART", "USD")]).await?.remove(0).unwrap();
    let ticker = ib.req_mkt_data(&aapl, "", false, false, &[])?;
    let trade = ib.place_order(&aapl, Order::limit("BUY", 100.0, 150.0))?;
    let mut events = ib.events();
    while let Ok(event) = events.recv().await {
        if let Event::Filled(t) = event {
            println!("filled at {}", t.lock().order_status.avg_fill_price);
            break;
        }
    }
    println!("bid {}", ticker.read().bid);
    Ok(())
}
```

`ibkit_tws::Client` is the low-level client: one method per TWS request
(`req_mkt_data`, `place_order`, ...) and a stream of `Incoming` messages, one
variant per server message.

## TWS protocol coverage

* Every request type (81) and every server message type (84) of the text
  protocol. The four requests that exist only in the protobuf protocol
  (cancelContractData, cancelHistoricalTicks, reqConfig, updateConfig) are not
  included.
* Every version gate from server version 157 (the ib_async minimum) to 200 is
  implemented as in IB's official client and decoder. The client announces
  `v157..200`, so current TWS builds answer with the text protocol. Server
  versions 201 and later switch to protobuf, which ibkit does not speak; the
  server negotiates down to 200.
* Requests are throttled to 45 per second by default, like ib_async.

## Web API

`ibkit_web::WebClient` (Python: `ibkit.WebClient`, or `ibkit.AsyncWebClient`
for asyncio) has named methods for the
session, account, portfolio, portfolio analyst, contract, market data, order,
scanner, watchlist, FA and notification endpoints, plus `request(method, path,
query, body)` for anything else. It works with the local Client Portal Gateway
(self-signed certificate accepted for localhost) and with OAuth 1.0a
(`oauth_init` derives the live session token with Diffie-Hellman, then signs
each request with HMAC-SHA256). `ibkit_web::WsClient` streams the WebSocket
topics (`smd`, `smh`, `sor`, `spl`, `str`, `ssd`, `sld`, `sbd`) with
heartbeats.

## FIX

`ibkit_fix` validates BodyLength and CheckSum, runs logon, heartbeats, test
requests, resend requests with SequenceReset-GapFill, sequence resets and
logout, and persists sequence numbers in memory or in a file. `ibkit_fix::ib`
builds NewOrderSingle, OrderCancelRequest, OrderCancelReplaceRequest and
OrderStatusRequest from ibkit contracts and orders using the IB conventions
(6035 IBLocalSymbol, 100 ExDestination, pegged orders as 40=P with 18
ExecInst and 211 PegDifference, 204, 77, 440). Other IB custom tags can be
added by number. TLS is not built in; IB FIX sessions normally run over a VPN,
cross-connect or an stunnel.

## Performance

`cargo run --release -p ibkit-bench` on the development machine (Apple Silicon,
macOS); numbers vary by machine:

| Path | Throughput |
| --- | --- |
| decode TICK_PRICE | 21.9 M msg/s (46 ns/msg) |
| decode EXECUTION_DATA | 2.5 M msg/s |
| encode PLACE_ORDER | 4.8 M msg/s |
| FIX parse ExecutionReport | 7.7 M msg/s |
| socket → decode → Ticker update | 10.9 M msg/s |

For comparison, ib_async's decoder alone handles 2.05 M TICK_PRICE msg/s on
the same machine (Python 3.15, no ticker update).

## Development

```
just test        # cargo nextest run --workspace, then doctests
just test-stress # repeat the networked tests 30 times (finds races)
just py-test     # build the extension with maturin, run pytest
just test-all    # Rust and Python
just fmt         # format and lint everything (prek: rustfmt, clippy, ruff, oxfmt)
just bench       # throughput numbers
just gen-python-types   # regenerate python/ibkit/_types.py from the Rust types
```

Tests use [cargo-nextest](https://nexte.st) (`cargo install cargo-nextest`).
`.config/nextest.toml` kills a test that runs longer than 30 s, so a hung
network test fails instead of blocking the run; the `ci` profile writes
`target/nextest/ci/junit.xml`.

What the suite covers:

* Every TWS request type (81) and every server message type (84) has a wire
  vector; decoding must consume exactly the fields of each message, and gated
  layouts are also checked at server version 200.
* `place_order` layouts at server versions 157 to 200, handshake bytes,
  throttling, connection errors.
* End-to-end `IB` flows against an in-process fake TWS server
  (`ibkit_tws::testing`) that speaks the real wire protocol: startup sync,
  qualify, order fill with commission, cancel, reject, what-if, brackets,
  market data, snapshots, depth, tick-by-tick, historical and real time bars,
  P&L, scanner, account summary, timeouts and disconnects.
* FIX session recovery (resend with gap fill), test requests, logon and
  logout timeouts, rejects, sequence resets, TCP round trip, IB order mapping.
* Web API OAuth vectors (computed independently with Python's standard
  library), REST requests, error reporting, WebSocket topics.
* The Python package against the same fake server, sync and asyncio.

Nothing connects to IB.

## License

MIT
