# @ibkit/node

Interactive Brokers for Node.js (22+) and Bun, on the ibkit Rust core: the TWS API (`IB` with ib_async's methods and events, and the low-level `Client`), the IBKR Web API (`WebClient`, `WsClient`, Flex), FIX 4.2 (`FixSession` and IB's message builders) and the `ibkit gateway` control API (`GatewayClient`).

```sh
bun add @ibkit/node   # or npm install; the native addon comes in a per-platform package
```

```ts
import { IB, LimitOrder, Stock } from '@ibkit/node';

const ib = await new IB().connect({ port: 7497, clientId: 1 });
const [aapl] = await ib.qualifyContracts(Stock('AAPL', 'SMART', 'USD'));
const ticker = ib.reqMktData(aapl!);
ticker.on('update', (t) => console.log(t.bid, t.ask));
const trade = ib.placeOrder(aapl!, LimitOrder('BUY', 100, 150));
trade.on('status', (t) => console.log(t.orderStatus.status));
ib.on('apiError', (reqId, code, message) => console.warn(reqId, code, message));
```

IB's error messages arrive as `apiError` (ib_async `errorEvent`), not `error`, because Node throws an `error` event that has no listener. Unset ticker numbers read as `NaN`. Every request that waits for IB returns a `Promise`. Raw `message` events need `connect({ emitMessages: true })`.

Events do not get lost when your code is busy: the addon reads the client's lossless event batches. Ticker and trade events carry only the fields that changed and merge into `data` in order (`seq`); `refresh()` reads the current state at once. A trade's `log`, and its `fills` until a commission report changes one, receive only their new entries. A bar update reads only the last bar and the new ones from the core, so its cost does not grow with the length of the list. A batch emits `pendingTickers` before `update`, so tickers are current when `update` (and `waitOnUpdate`) fires; ib_async emits them in the other order but has already changed its tickers.

Errors are classes: `RequestError` (with `reqId` and `code`), `ConnectionError`, `TimeoutError`, `InvalidArgumentError`, all subclasses of `IbkitError`. The Rust core assigns each error its kind (`ErrorKind`), so the Python package raises the same kinds.

## Gateway control

`@ibkit/node/gateway` is plain `fetch` and WebSocket and does not load the native addon, so it also runs in Deno and other runtimes (a browser WebSocket cannot send the bearer token, so `events()` needs Node, Bun or Deno). Errors are `GatewayError` with the HTTP `status`. The types (`GatewayState`, `ErrorCode`, `GatewayConfig`, and `GatewayEvent` as a union tagged by `event`) are generated from the OpenAPI document.

```ts
import { Control, GatewayClient } from '@ibkit/node/gateway';

const gw = new GatewayClient('http://127.0.0.1:5800'); // token from IBKIT_CONTROL_TOKEN
console.log((await gw.status()).state);
await gw.control(Control.ReconnectAccount);
console.log(await gw.metrics()); // Prometheus text
for await (const event of await gw.events()) console.log(event.event);
```

## Build and test

```sh
bun install
bun run build      # native addon (cargo, release) and dist/
bun test           # against the in-process fake TWS server
```

`ts/types.ts` is generated from the Rust defaults (`just gen-python-types`) and `ts/client_requests.ts` from the request table in `src/lib.rs` (`uv run --no-project python scripts/gen_node_client.py`).

Releases: `.github/workflows/publish-packages.yml` builds the addon per platform and `scripts/node-packages.ts` assembles `@ibkit/node-<platform>` packages and the main package that lists them as optional dependencies.
