# ibkit

[![CI](https://github.com/racetozero/ibkit/actions/workflows/ci.yml/badge.svg)](https://github.com/racetozero/ibkit/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/racetozero/ibkit?sort=semver)](https://github.com/racetozero/ibkit/releases)
[![Rust](https://img.shields.io/badge/Rust-stable-orange?logo=rust)](https://github.com/racetozero/ibkit/blob/main/rust-toolchain.toml)
[![Downloads](https://img.shields.io/github/downloads/racetozero/ibkit/total)](https://github.com/racetozero/ibkit/releases)
[![crates.io](https://img.shields.io/crates/v/ibkit?label=crates.io)](https://crates.io/crates/ibkit)
[![npm](https://img.shields.io/npm/v/%40ibkit%2Fnode?label=npm)](https://www.npmjs.com/package/@ibkit/node)
[![PyPI](https://img.shields.io/pypi/v/ibkit?label=PyPI)](https://pypi.org/project/ibkit/)
[![Python](https://img.shields.io/pypi/pyversions/ibkit)](https://pypi.org/project/ibkit/)
[![License: MIT and Apache-2.0](https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue)](#license)

**Trade with Interactive Brokers from Python, TypeScript or Rust, and keep IB
Gateway logged in without touching it.**

ibkit is a fast Interactive Brokers client with a Rust core. It speaks all three
IBKR APIs (the TWS socket API, the Web API and FIX) and gives Python the
[ib_async](https://github.com/ib-api-reloaded/ib_async) API you already know, so
most programs move over by changing one import.

It also ships `ibkit`, a command that starts IB Gateway, logs in (including the
second factor), answers its dialogs, and brings it back after restarts,
disconnects and crashes. It replaces IBC and IBAutomater, and runs on your
desktop, on a server, in Docker or in Kubernetes.

```sh
ibkit gateway tui
```

That command opens a terminal workspace where you enter your login, press `r`,
and approve the login on your phone. Gateway is then ready for your code on port 4002.

## Install

### The `ibkit` command

On macOS or Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/racetozero/ibkit/main/install.sh | sh
```

On Windows (x64, experimental):

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://raw.githubusercontent.com/racetozero/ibkit/main/install.ps1 | iex"
```

Or use Homebrew: `brew install racetozero/tap/ibkit`.

The installer picks the release for your machine, checks its SHA-256 checksum,
and puts `ibkit` in `~/.local/bin` with its Gateway agent beside it. Update
later with `ibkit update` (or `brew upgrade racetozero/tap/ibkit` for Homebrew).
You also need [IB Gateway](https://www.interactivebrokers.com/en/trading/ibgateway-latest.php)
from IBKR; `ibkit` finds the version that IBKR's installer put in its default
place.

### The SDKs

```sh
uv add ibkit            # Python 3.11+ (or pip install ibkit)
bun add @ibkit/node     # Node.js 22+ and Bun (or npm install @ibkit/node)
cargo add ibkit         # Rust
```

The Python and Node packages include the compiled Rust core. There is nothing
else to build.

## Why ibkit

- **Your ib_async code keeps working.** The Python package has ib_async's
  classes, method names, arguments and events, sync and asyncio.
- **Fast.** The Rust core decodes 21.9 million tick messages per second, about
  ten times ib_async's decoder ([numbers](#performance)).
- **Always current.** `Trade` and `Ticker` fields are read from the core when
  you access them, so there is never a stale copy.
- **All of IBKR.** The TWS API up to server version 226 (text and protobuf), every
  Web API endpoint with OAuth 1.0a and 2.0, and FIX 4.2 with IB's order tags.
- **One API in three languages.** Python, TypeScript and Rust share one core, the
  same objects and the same error kinds.
- **Gateway that stays up.** `ibkit gateway` logs in, enters TOTP codes or waits
  for IB Key, handles IBKR's daily restart, and recovers from faults without a
  limit. It stops and tells you when only a person can fix the problem.
- **Built for servers.** A Docker image, a Helm chart, health checks,
  Prometheus metrics, webhook alerts and a REST and WebSocket control API.

## Use the SDK

Start TWS or IB Gateway with the API turned on (or let
[`ibkit gateway`](#automate-ib-gateway) do it), then connect. The examples use
TWS's paper port 7497; IB Gateway's paper port is 4002.

### Python

```python
from ibkit import IB, Stock, LimitOrder, util

ib = IB()
ib.connect('127.0.0.1', 7497, clientId=1)

aapl = Stock('AAPL', 'SMART', 'USD')
ib.qualifyContracts(aapl)                 # fills conId in place

ticker = ib.reqMktData(aapl)              # live: attributes always current
ib.sleep(2)
print(ticker.bid, ticker.ask, ticker.marketPrice())

bars = ib.reqHistoricalData(aapl, '', '5 D', '1 hour', 'TRADES', useRTH=True)
print(util.df(bars).tail())               # needs pandas

trade = ib.placeOrder(aapl, LimitOrder('BUY', 100, 150.0))
trade.filledEvent += lambda t: print('filled', t.orderStatus.avgFillPrice)
while not trade.isDone():
    ib.waitOnUpdate()
```

Every call that waits for IB also has a coroutine form
(`await ib.connectAsync(...)`, `await ib.qualifyContractsAsync(...)`, ...).
Events such as `ib.orderStatusEvent`, `trade.fillEvent` and `ticker.updateEvent`
support `+=`, `-=` and `await`. [Python guide](crates/ibkit-py/README.md).

**Coming from ib_async?** Change `from ib_async import ...` to
`from ibkit import ...`. The differences are small:

- Fields of `Trade` and `Ticker` are always current; there is no stale copy.
- Sizes are floats (as in ib_async). `Order.usePriceMgmtAlgo` defaults to `None`
  (IB's default) instead of `False`.
- `ib.run()`, `util.startLoop()`, `IBC`/`Watchdog` and the tick bar operators
  (`TimeBars`, `TickBars`, ...) are not included. `ibkit gateway` takes the
  place of `IBC` and `Watchdog`.

### TypeScript

```ts
import { IB, LimitOrder, Stock } from "@ibkit/node"

const ib = await new IB().connect({ port: 7497, clientId: 1 })
const [aapl] = await ib.qualifyContracts(Stock("AAPL", "SMART", "USD"))
const ticker = ib.reqMktData(aapl!)
ticker.on("update", (t) => console.log(t.bid, t.ask))
const trade = ib.placeOrder(aapl!, LimitOrder("BUY", 100, 150))
trade.on("status", (t) => console.log(t.orderStatus.status))
ib.on("apiError", (reqId, code, message) => console.warn(reqId, code, message))
```

The package also has the low-level `Client`, the Web API (`WebClient`,
`WsClient`, Flex) and FIX. [TypeScript guide](crates/ibkit-node/README.md).

### Rust

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

### Web API and FIX

`WebClient` has a named method for every endpoint of IBKR's Web API reference.
It signs in through the local Client Portal Gateway, OAuth 1.0a or OAuth 2.0,
and it waits as needed to stay within IBKR's rate limits, so your IP address is
not blocked. `WsClient` streams market data, orders and P&L, and
`flex::download` fetches Flex statements.

`ibkit_fix` is a complete FIX 4.2 session (TCP or TLS, with resend and recovery)
with builders for IB's orders, multileg orders, allocations and exercises.

### Crates

| Crate        | What it does                                                                   |
| ------------ | ------------------------------------------------------------------------------ |
| `ibkit`      | The ib_async-style `IB` client: trades, tickers, positions, account values     |
| `ibkit-tws`  | The TWS wire protocol, every request and server message, and an async client   |
| `ibkit-web`  | Web API REST endpoints, WebSocket streaming, OAuth 1.0a and 2.0, Flex          |
| `ibkit-fix`  | FIX 4.2 codec, session layer, TCP/TLS initiator and acceptor, IB order mapping |
| `ibkit-py`   | The `ibkit` Python package (PyO3)                                              |
| `ibkit-node` | The `@ibkit/node` package for Node.js and Bun (napi-rs)                        |

## Automate IB Gateway

IB Gateway logs out every day, asks for a second factor, and opens dialogs that
wait for a click. `ibkit gateway` does those tasks for you. A small native agent
inside Gateway's Java process fills in the login, enters TOTP codes, answers the
dialogs, sets the API options and reports Gateway's state. The `ibkit` command
around it restarts Gateway when something goes wrong and keeps the API port
ready for your code.

### On your desktop

```sh
ibkit gateway tui
```

1. In **Settings**, enter your username and password under **Login**, and pick
   paper or live trading. Add your Mobile Authenticator secret to have TOTP
   codes entered for you.
2. Press `r` to start Gateway, then approve the IB Key request on your phone
   (unless you added the secret).
3. The **Overview** shows the session state, the API address and a live event
   log. Press `?` to see every key.

Your login is used only for that run unless you turn on **Save credentials**.

### On a server

```sh
export IB_USERNAME=... IB_PASSWORD=...
export IB_TOTP_SECRET=...        # optional: the Mobile Authenticator secret
ibkit gateway run
```

`run` stays in the foreground and prints JSON events. The default is paper
trading on port 4002; add `--trading-mode live --api-port 4001` for a live
account. Without `IB_TOTP_SECRET`, the login waits for IB Key approval on your
phone. Every setting is a flag, an `IBKIT_*` variable or a key in a
`config.toml` ([example](crates/ibkit-gateway/config.example.toml));
`ibkit gateway config` shows what is in effect.

From another terminal:

```sh
ibkit gateway status                 # state, session and recovery
ibkit gateway health --kind ready    # logged in and the API answers
ibkit gateway reconnect-account      # reconnect without a new login
ibkit gateway restart                # new login
ibkit gateway stop
```

When a fault needs a person (a wrong password or an account task in IBKR's
portal), the controller stops Gateway and waits in the `halted` state instead of
retrying until IBKR locks the account. Correct the fault, then run
`ibkit gateway restart`.

### From your code

Set `control_listen = "127.0.0.1:5800"` and a token in `IBKIT_CONTROL_TOKEN` to
serve the control API. The SDKs have clients for it:

```python
from ibkit.gateway import Control, GatewayClient

gw = GatewayClient()                      # http://127.0.0.1:5800, token from IBKIT_CONTROL_TOKEN
print(gw.status()['state'])
gw.control(Control.RECONNECT_ACCOUNT)
for event in gw.events():
    print(event['event'])
```

```ts
import { Control, GatewayClient } from "@ibkit/node/gateway"

const gw = new GatewayClient("http://127.0.0.1:5800")
console.log((await gw.status()).state)
await gw.control(Control.ReconnectAccount)
```

### In Docker

The [Dockerfile](Dockerfile) builds an image with IB Gateway, a private Xvfb
display and the controller, for amd64 and arm64. It runs as a non-root user and
keeps Gateway's settings and restart tokens on a volume:

```sh
docker build --target runtime -t ibkit-gateway .
docker run -d --name ibkit --restart unless-stopped \
  --env-file /secure/ibkit.env \
  -v ibkit-state:/var/lib/ibkit \
  -p 127.0.0.1:4002:4004 --stop-timeout 45 ibkit-gateway
```

`/secure/ibkit.env` holds `IB_USERNAME`, `IB_PASSWORD` and, if you use it,
`IB_TOTP_SECRET`. Any other setting can come from the environment too, such as
`IBKIT_TRADING_MODE=live`. `--restart unless-stopped` is safe: after a fault
that needs a person, a restarted container waits instead of logging in again.
Keep `--stop-timeout` above 30 s, so that Gateway can log out of IBKR before
Docker kills it.

Keep the container private:

- **Publish ports on loopback** (`-p 127.0.0.1:...`) or on a private network,
  never on a public address. The API port accepts orders with no
  authentication or encryption of its own, and Gateway sees every client of
  the container as 127.0.0.1, so its trusted-IP setting does not protect it.
  Use `api_allowed_sources` to limit which addresses can connect.
- **Keep credentials out of the image and the command line.** Use an env file
  that only you can read, or mount files and set `IBKIT_USERNAME_FILE` and
  `IBKIT_PASSWORD_FILE`.
- **The control API has no TLS.** Turn it on only with a random token of 16 or
  more characters, publish it on loopback, and put a TLS proxy in front of it
  for remote access.
- **VNC is off by default.** If you turn it on for an account task, use a
  password file, publish it on loopback, and connect through an SSH tunnel.

### In Kubernetes

The [Helm chart](charts/ibkit-gateway) runs one replica with a persistent
volume, non-root permissions, separate liveness and readiness probes, and an
optional Prometheus ServiceMonitor. The liveness probe passes while the
controller waits for a person, so Kubernetes does not restart that pod into
another login. Credentials come from a Secret that you create; the chart never
creates or embeds them:

```sh
kubectl create secret generic ibkit-credentials \
  --from-file=username=/secure/username --from-file=password=/secure/password
helm upgrade --install trading charts/ibkit-gateway
```

The chart's NetworkPolicy is on by default and denies all inbound traffic. Allow
only the pods that trade through Gateway, on the ports they use:

```yaml
# values.yaml
networkPolicy:
  enabled: true
  ingress:
    - from:
        - podSelector:
            matchLabels:
              app.kubernetes.io/name: my-trading-bot
      ports:
        - port: api
    - from: # only if controlApi.serviceMonitor is on
        - namespaceSelector:
            matchLabels:
              kubernetes.io/metadata.name: monitoring
          podSelector:
            matchLabels:
              app.kubernetes.io/name: prometheus
      ports:
        - port: control
```

A rule without `ports` opens every port of the pod, including the control API
and VNC, so always name the ports.

> [!IMPORTANT]
> On a cluster that trades a live account, lock Gateway down to only the pods
> that need it. Anything that reaches the API port can place orders in your
> account, and anything that reaches the control API with its token can change
> the login. Use zero-trust, deny-by-default network policies: Kubernetes
> NetworkPolicy, or your CNI's own policies (for example Cilium's
> `CiliumNetworkPolicy` or Calico's `GlobalNetworkPolicy`) for a cluster-wide
> default deny. Allow only trusted pods by label, and deny everything else.
> NetworkPolicy has no effect unless the cluster's CNI enforces it (Cilium,
> Calico and Antrea do; plain Flannel does not), so check that a pod outside
> the allow list cannot connect before you trade.

Also keep the Service as `ClusterIP` (never `LoadBalancer` or `NodePort`), limit
who can read the credentials and control token Secrets with RBAC, and open the
VNC port only with `kubectl port-forward` when you need it.

Read the [Gateway guide](crates/ibkit-gateway/README.md) for every setting, the
TUI keys, the control API, metrics, alerts and the limits.

## Performance

`cargo run --release -p ibkit-bench` on the development machine (Apple Silicon,
macOS); numbers vary by machine:

| Path                            | Throughput               |
| ------------------------------- | ------------------------ |
| decode TICK_PRICE               | 21.9 M msg/s (46 ns/msg) |
| decode EXECUTION_DATA           | 2.5 M msg/s              |
| encode PLACE_ORDER              | 4.8 M msg/s              |
| FIX parse ExecutionReport       | 7.7 M msg/s              |
| socket → decode → Ticker update | 10.9 M msg/s             |

For comparison, ib_async's decoder alone handles 2.05 M TICK_PRICE msg/s on the
same machine (Python 3.15, no ticker update).

## Development

```
just test        # cargo nextest run --workspace, then doctests
just test-stress # repeat the networked tests 30 times (finds races)
just py-test     # build the extension with maturin, run pytest
just node-test   # build the Node addon, type-check, bun test
just test-all    # Rust, Python and TypeScript
just fmt         # format and lint everything (prek: rustfmt, clippy, ruff, oxfmt)
just bench       # throughput numbers
just gen-python-types   # regenerate python/ibkit/_types.py and ibkit-node/ts/types.ts from the Rust types
just jvm         # JVM agent tests (needs JAVA_HOME and a display)
just tui-check   # drive the `ibkit gateway tui` against a fixture
just chart-check # helm lint plus config validation of the rendered chart
just image       # build the Gateway container image
```

The tests run against in-process fakes of TWS, FIX and Gateway; nothing
connects to IB.

## License

The client libraries (`ibkit`, `ibkit-tws`, `ibkit-web`, `ibkit-fix`, the Python
and Node packages) are MIT. The Gateway crates (`ibkit-cli`,
`ibkit-gateway`, `ibkit-gateway-core`, `ibkit-gateway-client`) are
Apache-2.0.

ibkit is not affiliated with or endorsed by Interactive Brokers. IB, IBKR and IB
Gateway are names of Interactive Brokers LLC. IB Gateway is IBKR's proprietary
software; get it from IBKR and use it under their terms.
