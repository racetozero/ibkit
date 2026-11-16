# Changelog

## 0.0.1

Long-running Gateway service, control API and SDKs:

* Breaking: the `ibgateway` command is now `ibkit`, with the Gateway commands
  under `ibkit gateway` (`ibkit gateway run`, `ibkit gateway tui`, ...).
  The crates are `ibkit-cli` (the binary and, as its library, the JVM agent),
  `ibkit-gateway`, `ibkit-gateway-core` and `ibkit-gateway-client`; the
  variables are `IBKIT_*` (were `IBG_*`); the agent is
  `libibkit_gateway_agent`; the image is `ghcr.io/racetozero/ibkit-gateway`
  and the chart `ibkit-gateway`; default paths use `ibkit` (`/etc/ibkit`,
  `/var/lib/ibkit`); metrics are `ibkit_gateway_*`.
* `ibkit update` replaces the binary and its agent with the latest release.
  Releases are built with cargo-dist: archives with the binary and agent,
  `install.sh` (macOS, glibc Linux), `install.ps1` (Windows x64) and the
  Homebrew formula `racetozero/tap/ibkit`.
* `ibkit gateway tui`: new layout with an Overview (session card and a
  scrollable, filterable event log with a detail view) and a Settings view
  (section list, in-place editing, per-field validation and help). The
  header shows the state and PAPER or LIVE; `?` lists every key. Tab now
  changes the view; Left and Right change the settings section.
* `ibkit gateway`: automatic recovery is on by default and has no attempt limit
  (`recovery_attempts = 0`; was 3, maximum now 100000); the delay doubles up
  to `recovery_max_delay_secs` (1800). More faults are retried (unanswered
  login, existing session, initialization timeout, unknown dialog, Java
  exception, launcher start failure), with longer minimum delays where a
  quick retry would hurt. Faults that need an operator halt the controller
  instead of exiting it; `restart` logs in again. A replacement controller
  adopts a running Gateway. A pending wait survives SIGTERM. The API
  forwarder restarts after a listener failure. A warning reports a JVM heap
  below `java_heap_mb` (the macOS restart child can ignore the launcher
  option). `health --kind live` passes while halted. Breaking: the example
  config and Helm chart now enable recovery.
* `ibkit gateway`: every setting is a flag and an `IBKIT_*` variable; the config
  file is optional. New commands `config` and `reload` (`Control::Reload`).
* `ibkit gateway`: control API on `control_listen` with a bearer token (REST:
  health, status, control, config, credentials, TOTP secret; WebSocket
  events). Settings changed through it are saved in
  `state_dir/api-settings.toml`.
* `ibkit gateway tui`: every setting, a masked TOTP secret field, a Control
  section, and the `n` (reconnect) key.
* `ibkit-gateway-core`: `api` module (shared API types; `Status` moved here),
  `Config::load_with`, `Config::control_token`, `AgentStatus::max_heap_mb`.
* New crate `ibkit-gateway-client`: Rust client of the control API.
* `ibkit-py`: `ibkit.gateway.GatewayClient`; regenerated `_types.py`
  (`ContractDetails.settlementMethod`, `Order.hedgeMaxSize`).
* New package `@ibkit/node` (`crates/ibkit-node`, napi-rs): `IB` with the
  ib_async methods and typed events, the low-level `Client` (every EClient
  request), `WebClient`, `WsClient`, Flex download, FIX builders and
  `FixSession`, `GatewayClient`, and an in-process fake TWS server for tests.
  Types are generated from the Rust defaults.
* Docker image exposes port 5800; Helm chart `controlApi` values.
* `ibkit gateway`: alerts (`alert_webhook_url`): a JSON POST on `down`,
  `recovered`, `halted`, `exhausted` and `controller_failed`, with a `text`
  field for chat webhooks. New event `halted` (was an `output` line).
* `ibkit gateway`: `GET /v1/metrics` (Prometheus text: state, recovery phase
  and retry time, agent heartbeat and heap, launch, error, recovery, halt
  and alert counters) and `GET /v1/openapi.json` (OpenAPI 3.1 from the Rust
  types; also `ibkit gateway openapi` and `docs/gateway/openapi.json`).
  `ibkit-gateway-client`, Python and TypeScript clients have `metrics()` and
  `openapi()`.
* `ibkit`: `IB::event_batches()`: every event without loss, one batch per
  network packet, on an unbounded queue (`IB::events()` stays a bounded
  broadcast that drops the oldest events of a slow reader).
* `@ibkit/node`: events no longer get lost when JavaScript is busy: the
  addon reads `event_batches`. `pendingTickers` carries only the
  changed fields of each ticker (`Ticker.seq`), not full snapshots. The
  diff compares fields by position and the contract as a struct, without a
  hashed JSON map (100 tickers x 300 rounds: median 330 ms to 120 ms wall,
  15 to 10 us CPU per ticker update). Each batch's `pendingTickers` now
  comes before its `update`, so `waitOnUpdate` sees the new ticker data (it
  could wake first and then wait for its timeout).
* `@ibkit/node`: breaking: Node 22 or later. The control client is plain
  `fetch` and WebSocket in `@ibkit/node/gateway` (no native addon; errors
  are `GatewayError` with `status`). The addon loads from a per-platform
  npm package when `ibkit.node` is absent.
* Helm chart: `alerts.existingSecret` (the webhook URL) and
  `controlApi.serviceMonitor`; the image defaults to
  `ghcr.io/racetozero/ibkit-gateway`.
* `ibkit gateway`: fix: a reload that changed `api_port` but not `api_listen`
  kept forwarding to the old port. The forwarder is now one supervised
  task per `(api_listen, api_port)` that binds again after a failure.
* `ibkit gateway`: library: `Gateway::subscribe()` is removed; use
  `gateway.events().subscribe()`. `settings::config_fields()`,
  `settings::merge()`, `Status::is_live()` and `gateway::write_private()`
  are the single owners of those rules. Flag and variable types come from
  the `Config` schema, so a new optional number key needs no special case.
* `ibkit-fix`: `FrameSplitter` skips consumed bytes with an offset instead
  of moving the buffer per message (a 1,000-message read: about 880 to
  37 ns per message).
* `ibkit-web`: `client::parse_method` accepts GET, POST, PUT, PATCH and
  DELETE in any case; the bindings used to accept any token and named
  different methods in their errors.
* Workspace: serde_json `preserve_order` is on for every crate, so JSON key
  order no longer depends on which crates a build includes.
* `ibkit gateway`, its agent and the TUI build for Windows x64 (experimental:
  cross-compiled and covered by Windows platform tests in CI; not yet run
  against a real Gateway on Windows). OS operations moved to
  `ibkit_gateway::platform`: on Windows each session runs in a Job Object named
  after the session (the launcher starts suspended, detached from the
  console, and enters the job before it runs), Ctrl-C/Ctrl-Break stop, and
  a console close leaves Gateway for adoption. Gateway is found in
  `C:\Jts\ibgateway\<version>\ibgateway.exe`. Windows refuses
  `manage_display` and `vnc_listen`. The release adds a `windows-x64`
  archive; CI's Windows job now includes the Gateway crates. The OpenAPI
  document no longer holds the per-OS `executable` and `agent` defaults (it
  differed between macOS and Linux).
* `ibkit gateway` and the TUI find the Gateway launcher (the newest in
  `~/Applications/IB Gateway <version>` on macOS or `~/Jts/ibgateway/<version>`
  on Linux) and the native agent (next to the binary, `../lib`,
  `/usr/local/lib`) when `executable` and `agent` keep their defaults. The
  TUI explains the native agent, and its start errors name the missing path.
* `ibkit gateway` supervision (from a review of the Gateway automation):
  - fix: one heartbeat gap longer than 3 s (a GC pause, a host suspend)
    armed the soft-restart deadline for good, so a healthy Gateway was
    cold-restarted 10 minutes later. Heartbeat loss is now tracked apart
    from soft restarts and clears when the JVM reports again.
  - fix: a command that failed (soft restart before Ready, a fourth soft
    restart in an hour, a restart whose launch failed) stopped the
    controller and Gateway. It is now an `output` event; `soft-restart`
    before Ready is refused when it is sent.
  - fix: `stop` ignored a hung JVM outside the launcher (a restart child, an
    adopted Gateway), and recovery then started a second Gateway. The
    controller now tracks the session's process groups (the launcher group
    it starts and the groups of fresh agents, never its own) and kills them
    after `shutdown_timeout_secs`. `Status.process_group` holds the group.
  - fix: Xvfb shared the controller's process group, so Ctrl-C or
    `tini -g` ended the display before Gateway logged out. The `exited`
    event now has the real exit code; a forced kill is reported.
  - fix: a crashed JVM was found only after `soft_restart_timeout_secs`
    (600 s), and a launcher that exited with code 0 before Ready after
    `initialization_timeout_secs` (900 s); both are reported 120 s after
    nothing of the session runs. A kill before the first agent report no
    longer waits 15 minutes and halts when the launcher group is gone.
  - fix: the controller saved no status while `stop` waited, so after 5 s
    it was reported inactive and refused commands.
  - fix: a reload logged in again for every change and did not apply a new
    `alert_webhook_url`; a reload during a recovery wait skipped the wait.
    Controller settings now apply without a login; SIGHUP reloads.
  - fix: 20 consecutive failed steps (for example a full disk), not one,
    end the controller, and the `controller_failed` alert is sent before
    the final stop.
  - fix: `poll_recovery` could drop a command; a command left by a previous
    controller (same PID after a container restart) is dropped at start.
  - new: `pause` (CLI, API) and `closedown_at` keep the controller up in
    state `paused`, also across its own restarts, until `restart`;
    `cold_restart_at` logs in again weekly at a chosen time.
  - new: lock-out guards: `max_login_failures` (10) consecutive logins
    without Ready after an unknown login dialog or an unanswered second
    factor stop retries (`LoginRetriesExhausted`); two rejected TOTP codes
    fail with `TotpRejected` (never retried). Transient faults still retry
    without a limit.
  - new: retries held by IBKR's reset window emit one event with the end of
    the window. The soak workflow runs from Monday and takes an optional
    TOTP secret.
* `ibkit gateway` control API: breaking: `PATCH /v1/config` refuses
  `executable`, `agent`, `settings_dir`, the credential sources, VNC and the
  control API keys (403); live trading, remote or writable API access,
  accepted incoming connections, bypassed precautions and `api_listen` need
  `control_api_full_access = true` from the file, a flag or a variable.
  New `DELETE /v1/config`; the controller lists settings saved through the
  API at start. Gateway no longer inherits the control token.
* `ibkit gateway` API forwarder: `api_allowed_sources`, `TCP_NODELAY`, and an
  accept error no longer drops the live connections. An empty flag or
  `IBKIT_*` variable clears an optional key (`IBKIT_API_LISTEN=`).
* `ibkit gateway` agent:
  - fix: "Your password will expire in N day(s)" failed the login every day;
    it is dismissed (`dismiss_password_expiry_warning`, on by default).
  - fix: the weekly token-expiry text ("security tokens ... have expired")
    and a restart child that stays at the login screen for 120 s now start
    a new login (it waited 900 s).
  - fix: Re-login took the session back from another user under the
    `refuse` policy; it is a takeover now. Takeover works more than once per
    JVM (at most three times an hour).
  - fix: a modal dialog that a Gateway listener opened while the agent set a
    control deadlocked the event thread; the nested pass now answers known
    confirmations and never blocks.
  - fix: one failed UI pass (a malformed command file, a busy file system,
    a button that is briefly absent) failed the session; failures that last
    10 s do.
  - fix: an agent that cannot start (an unreadable TOTP secret) now writes a
    failed report with the reason; the controller also checks the TOTP
    secret before it starts Gateway.
  - new: the reconnect hotkey confirmation is answered after the agent's own
    shortcut; "connection lost, re-login?" starts a new login; Gateway's
    exit confirmation is answered after a cold restart. Second-factor
    titles match by substring. Agent report files are 0600.
* `ibkit gateway`: `jts.ini` gets `Locale=en` (the agent matches English texts)
  and the current `TZ` at every start. `validate --files` checks the
  launcher and agent; errors name the credential variables, files and the
  state directory owner; `--help` hides variable values.
* Image and chart: x11vnc and `vnc_listen` (chart `vnc.enabled` with
  `kubectl port-forward`); the release build requires the installer
  SHA-256 per architecture; the chart's image tag defaults to its
  appVersion; schema maxima for the time limits; corrected NOTES about
  trusted IPs. New chart values for the new keys.
* `ibkit gateway`: breaking: new state `halted` for a controller that waits for
  an operator (it reported `failed`, which now means one failed session).
  A halted controller whose hourly budget frees up recovers on its own again
  (it stayed halted).
* `ibkit gateway`: commands are a queue (`state_dir/control/`, was the single
  file `control.json`): they run in send order, and a second command within
  500 ms no longer replaces the first. A command applies to the controller
  that was running, so a stop or reload sent while a reload starts a new
  session is no longer dropped (two settings changes in a row could leave
  the second one saved but not applied). Reloads in a row run once. A new
  controller clears the queue, so a command left by the previous one
  (a container restart keeps the PID) does not run.
* `ibkit gateway`: a new controller waits up to 2 seconds for the state
  directory lock, so a replacement started as the old one exits does not
  fail (the lifecycle tests failed about 2 runs in 3 on this race).
* `ibkit-gateway-client`: errors carry `StatusError` (HTTP status and message)
  and `ConnectError`.
* `ibkit`, `ibkit-web`: `Error::kind()`, `WebError::kind()` and
  `WsError::kind()` give the shared `ErrorKind`; the Python and Node
  bindings map it to their classes.
* `ibkit-py`: breaking: errors are classes in `ibkit.errors`:
  `ConnectionError`, `TimeoutError` and `InvalidArgumentError` (subclasses
  of `IbkitError` and of the built-in classes, so `except TimeoutError`
  still works), `RequestError` with `reqId`, `code` and `message`, and
  `GatewayError` with `status` (was `IbkitError`). `FlexError` derives from
  `IbkitError`. A WebSocket argument error is now `InvalidArgumentError`
  (was a connection error).
* `ibkit-py`: typed for checkers: `py.typed`, a generated `_ibkit.pyi`,
  `__all__`, generic `Event[...]` (handler arguments and `await` result),
  every event declared with its arguments, return types on every request,
  and every `Ticker` field (pyright: 0 errors, 100% type completeness).
  Fix: `_types.py` declared time fields that the package converts to
  `datetime` (`Fill`, `Execution`, `TradeLogEntry`, `TickData`,
  `MktDepthData`, `HistoricalTick*`, `RealTimeBar`) as numbers or strings;
  they are `datetime` with the default `EPOCH`, as in ib_async (breaking:
  `Execution().time` was `''`). `Bar.volume` is a float
  (tick sizes are floats). `_types.py` has an `__all__`, so
  `from ibkit import *` no longer exports `dataclass`, `field` and `Any`. The control API
  types (`State` with `HALTED`, `ErrorCode`, `Config`, `Event` per kind)
  are generated from `openapi.json`, and `GatewayClient` returns enum
  members, not strings.
* `ibkit-py`: asyncio: `AsyncWebClient`, `FixSession.connectAsync`,
  `pollAsync` and `async for`, `FlexReport.downloadAsync`,
  `IB.reqCurrentTimeInMillisAsync`, `reqFamilyCodesAsync` and
  `reqSoftDollarTiersAsync`. A blocking call inside a running event loop
  emits a `RuntimeWarning` (it stopped the loop silently).
* `@ibkit/node`: the control API types are generated from `openapi.json`
  (`GatewayConfig`; `GatewayEvent` is a union tagged by `event`). Trade
  events carry only the changed fields (`Trade.seq`; new log and fill
  entries only), and a bar update reads only the last bar and the new
  ones. CPU per update, median (range): trade status 127 (121-258) to
  63 (53-75) us, 50 trades x 200 statuses, 7 runs each; bar update
  4.4 (4.39-4.68) ms to 65 (56-85) us with 5,000 bars, and now the same
  with 50 bars.
* CI: a Node job (Linux, macOS, Windows: addon, typecheck, tests, Node
  load) and checks that the generated TypeScript sources are current; ruff
  is pinned. New `Release` workflow (crates.io, PyPI wheels, npm platform
  packages, binaries, GHCR image and chart, GitHub release) and `Soak`
  workflow with `scripts/soak.sh` (fault injection against a paper account).

Conformance with IBKR's published documentation (Web API and TWS API
references, FIX Implementation Manual Revision 2025-001, TWS API 10.52.01):

* `ibkit-tws`: server versions 223 to 226 (`v157..226`):
  `Order::hedge_max_size`, `Order::conditions_include_overnight`,
  `ContractDetails::settlement_method`, odd-lot tick types 105 to 110
  (`NOT_SET` is now 111). `scripts/gen_protobuf.py` keeps messages that a
  newer schema dropped.
* `ibkit`: `Ticker` odd-lot fields.
* `ibkit-web`: Account Management, pre-trade compliance, third-party and echo
  endpoints (`/gw/api`, OAuth 2.0 access token, signed JWT bodies),
  `gw_request`, `oauth2_access_token`, `sso_browser_session`, third-party
  OAuth 1.0a `oauth_request_token`/`oauth_access_token`, `pa_allocation`,
  FA model rebalancing, cash analyzer, `is-full-master`, `tws-invest-divest`
  and allocation models. Breaking: `WebOrder` sends `outsideRTH` (reads
  `outsideRth` too) and has `manual_indicator`, `ext_operator`,
  `all_or_none`, `tax_optimizer_id`, `json_payload`; `cancel_order` takes the
  CME 536-B fields; several methods take the documented optional query
  parameters; WebSocket `subscribe_orders` takes several filters; fixed the
  `upl+{}`, `usd+<acct>+{}` and `uld+<acct>+{}` unsubscribe strings; option
  exercise topics; `flex::download_from` takes a `FlexPeriod`.
* `ibkit-fix`: messages follow IB's manual (see README). Breaking:
  HandlInst is 2; `new_order_single`, `cancel_replace` and
  `multileg_cancel_replace` require `order.account`; `cancel_request` takes
  the account; `order_status_request(account, cl_ord_id)`; `Leg` has
  `open_close`; `allocation_instruction` takes the transaction type,
  RefAllocID and account and sends AllocAvgPx; algos send IB's algo tags
  instead of 848; `order_mass_status_request` and
  `order_mass_cancel_request` are removed (IB rejects 35=AF and 35=q).
  New: `multileg_cancel_replace`, `OrderCancelReject`, `Bulletin`,
  `AllocationAck`, FIXCOMP compression, heartbeat interval from IB's Logon,
  Reject of a decreasing SequenceReset.
* Second pass: `WebClient` paces requests to IBKR's documented rate limits
  (`WebConfig::pacing`); `tax_voucher_download` returns bytes; endpoints
  IBKR deprecated or no longer documents are `#[deprecated]`;
  `ibkit_fix::ib::OrderExtras`, `new_order_single_with` (1028, 8089 and 142
  required on futures), `option_exercise`, pegged to benchmark (40=PB), pair
  trades (6665/6666), BOX auction strategy (6173), ContractID (6008), more
  ExecutionReport fields, `SessionConfig::on_behalf_of_sub_id`, a warning at
  IB's MsgSeqNum limit, constants for every tag of the FIX manual's
  appendices; Python `newOrderSingle(**extras)`, `optionExercise`,
  `WebClient(pacing=...)`. Removed unused dependencies; fixed the ruff
  finding in `scripts/check-tui.py`.
* `ibkit-gateway`, `ibkit-cli`, `ibkit-gateway-core` (comparison with IBC,
  IBAutomater, ib-gateway-docker, ibkr-docker and ibg-controller): unknown
  dialogs are never clicked and fail only after `unknown_dialog_timeout_secs`
  (the "Gateway" login progress dialog no longer fails the login); API port
  change confirmation, newer-version, Login Error, "not currently
  available", Tip of the Day and shutdown progress dialogs; Mobile
  Authenticator (TOTP) second factor (`totp_secret_env`, `totp_secret_file`)
  and passkey prompts (`passkey_authenticate`); AuthenticationNotAnswered
  when IBKR drops two login requests; `master_client_id`, `java_heap_mb`;
  `reconnect-account` and `reconnect-data` controls; port field found by
  component name; Linux accessibility bridge disabled; inherited open-file
  limit capped; `displayedproxymsg` and `TimeZone` in new `jts.ini` files;
  faster agent (cached class references, reaction to window events,
  immediate next step after an action).
* `ibkit-py`: follows the Rust changes (`ibkit.fix.cancelReplace`,
  `cancelRequest`, `orderStatusRequest`, `multilegCancelReplace`,
  `FixSession.connect(compression=...)`, `WebClient.gwRequest`,
  `oauthRequestToken`, `oauthAccessToken`, `paAllocation`,
  `cancelOrder(manualIndicator=..., extOperator=...)`,
  `WsClient.exerciseOptions`).


* `ibkit-gateway`, `ibkit-cli`, `ibkit-gateway-core`: moved in from the
  ibkit-rs repository with their history. The Dockerfile, Helm chart
  (`charts/ibkit-gateway`), scripts and docs (`docs/gateway`) came with them.
* `ibkit-tws`: protobuf protocol for server versions 201 to 222 (raw-int
  message ids, protobuf requests by version, decoders for all protobuf server
  messages), `cancelContractData`, `cancelHistoricalTicks`, `reqConfig`,
  `updateConfig`, new order, execution and contract detail fields,
  `scripts/gen_protobuf.py`.
* `ibkit`: `IB::set_timeout` (idle watch for `timeoutEvent`), `req_config`,
  `update_config`.
* `ibkit-web`: OAuth 2.0, Flex Web Service, alerts, event contracts, FYI
  settings and devices, FA allocation groups, `md/snapshot`, `pa/summary`,
  server prompt replies; WebSocket waits for the session, renews market data,
  `WsSender` for concurrent sends.
* `ibkit-fix`: CompID and SendingTime checks with Reject, BusinessMessageReject,
  persisted resend store, TLS, multi-leg, algo, allocation and mass
  status/cancel builders.
* `ibkit-py`: low-level `Client`/`Wrapper` with every request, `WsClient`,
  tick bar operators, missing `util` functions, `IBDefaults`, `IBC`,
  `Watchdog`, WSH subscriptions, `OAuth2Config`, FIX TLS and builders,
  `reqConfig`/`updateConfig`.

* `ibkit-tws`: TWS API text protocol for server versions 157 to 200: field
  codec, frame buffer, encoders for all 81 request types, decoder for all 84
  server message types, async client with handshake and request throttling,
  in-process fake TWS server for tests.
* `ibkit`: ib_async-style high-level client: startup synchronization, trades,
  fills, commission reports, live tickers (market data, tick-by-tick, depth),
  historical and real time bars, scanners, P&L, account values, positions,
  portfolio, news, options calculations, FA, WSH.
* `ibkit-web`: Web API REST endpoints, WebSocket streaming, OAuth 1.0a.
* `ibkit-fix`: FIX 4.2 codec, session state machine, TCP transport, IB order
  messages and ExecutionReport view.
* `ibkit-py`: PyO3 extension and `ibkit` Python package with the ib_async API,
  including `WebClient`, `FixSession` and `FlexReport`.
