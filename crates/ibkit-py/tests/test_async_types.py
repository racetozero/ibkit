"""Error classes, the warning for blocking calls in an event loop, and the
asyncio variants of the Web API and FIX clients."""

from __future__ import annotations

import asyncio
import datetime as dt
import http.server
import json
import socket
import threading
from collections.abc import Iterator

import ibkit.errors
import pytest
from ibkit import IB, AsyncWebClient, FixSession, IbkitError, RequestError, Stock
from ibkit.gateway import GatewayClient


def test_all_lists_every_public_name() -> None:
    import types

    import ibkit

    public = {
        name
        for name, value in vars(ibkit).items()
        if not name.startswith('_') and not isinstance(value, types.ModuleType)
    }
    exported = set(ibkit.__all__)
    assert public - exported == set(), 'add these names to ibkit.__all__'
    assert exported - set(vars(ibkit)) == set(), 'ibkit.__all__ lists absent names'


def test_error_classes_keep_the_builtin_bases(tws) -> None:
    ib = IB()
    ib.RaiseRequestErrors = True
    ib.RequestTimeout = 5
    ib.connect('127.0.0.1', tws.port, clientId=1)
    with pytest.raises(RequestError) as info:
        ib.reqContractDetails(Stock('XYZ', 'SMART', 'USD'))
    assert (info.value.code, info.value.reqId > 0) == (200, True)
    assert isinstance(info.value, IbkitError)
    ib.disconnect()

    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        port: int = s.getsockname()[1]
    with pytest.raises(ibkit.errors.ConnectionError) as refused:
        IB().connect('127.0.0.1', port, clientId=1, timeout=2)
    assert isinstance(refused.value, ConnectionError)
    assert isinstance(refused.value, IbkitError)
    with pytest.raises(ibkit.errors.InvalidArgumentError) as invalid:
        GatewayClient('http://127.0.0.1:1', '')
    assert isinstance(invalid.value, ValueError)


async def test_a_blocking_call_in_a_running_loop_warns(tws) -> None:
    ib = IB()
    ib.RequestTimeout = 5
    await ib.connectAsync('127.0.0.1', tws.port, clientId=1)
    aapl = Stock('AAPL', 'SMART', 'USD')
    with pytest.warns(RuntimeWarning, match='running asyncio event loop'):
        ib.reqContractDetails(aapl)
    assert (await ib.reqContractDetailsAsync(aapl))[0].contract.conId == 265598
    ib.disconnect()


@pytest.fixture
def web_server() -> Iterator[tuple[str, list[str]]]:
    seen: list[str] = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            seen.append(self.path)
            body = json.dumps([{'accountId': 'U1'}]).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, format: str, *args: object) -> None:
            pass

    server = http.server.HTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    yield f'http://127.0.0.1:{server.server_port}/v1/api', seen
    server.shutdown()


async def test_async_web_client(web_server: tuple[str, list[str]]) -> None:
    url, seen = web_server
    client = AsyncWebClient(url)
    assert (await client.portfolioAccounts())[0]['accountId'] == 'U1'
    assert (await client.request('GET', 'iserver/accounts'))[0]['accountId'] == 'U1'
    assert seen == ['/v1/api/portfolio/accounts', '/v1/api/iserver/accounts']


def fix(*fields: tuple[int, str]) -> bytes:
    """One FIX 4.2 message with its length and checksum."""
    body = ''.join(f'{t}={v}\x01' for t, v in fields).encode()
    head = f'8=FIX.4.2\x019={len(body)}\x01'.encode()
    total = sum(head + body) % 256
    return head + body + f'10={total:03d}\x01'.encode()


async def test_fix_session_for_asyncio() -> None:
    async def acceptor(
        reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        await reader.readuntil(b'\x0110=')
        await reader.readuntil(b'\x01')
        now = dt.datetime.now(dt.UTC).strftime('%Y%m%d-%H:%M:%S')
        common = ((49, 'IB'), (56, 'ME'), (52, now))
        writer.write(fix((35, 'A'), (34, '1'), *common, (98, '0'), (108, '30')))
        writer.write(
            fix((35, '8'), (34, '2'), *common, (11, 'C1'), (39, '0'), (150, '0'))
        )
        await writer.drain()
        writer.close()

    server = await asyncio.start_server(acceptor, '127.0.0.1', 0)
    port: int = server.sockets[0].getsockname()[1]
    async with server:
        session = await FixSession.connectAsync('127.0.0.1', port, 'ME')
        events = [event async for event in session]
    app = [e for e in events if e[0] == 'app']
    assert app and app[0][1] == '8' and (11, 'C1') in app[0][2], events
    assert events[-1][0] == 'disconnected'


def test_declared_types_match_the_runtime_values() -> None:
    from conftest import m, responder
    from ibkit import LimitOrder, Ticker

    def extended(req: list[str]) -> list[list[str]]:
        msg_id = int(req[0])
        if msg_id == 50:  # reqRealTimeBars: [50, version, reqId, ...]
            return [m(50, 3, req[2], 1767225600, 1, 2, 0.5, 1.5, 100, 1.2, 10)]
        if msg_id == 96:  # reqHistoricalTicks: [96, reqId, ...]
            return [m(98, req[1], 1, 1767225600, 0, 150.0, 10, 'NASDAQ', '', 1)]
        return responder(req)

    from ibkit import _ibkit

    server = _ibkit.FakeTws(178, 1000, 'DU123456', extended)
    try:
        ib = IB()
        ib.RequestTimeout = 5
        ib.connect('127.0.0.1', server.port, clientId=1)
        aapl = Stock('AAPL', 'SMART', 'USD', conId=265598)
        trade = ib.placeOrder(aapl, LimitOrder('BUY', 100, 150))
        for _ in ib.loopUntil(lambda: trade.fills, timeout=5):
            pass
        times = [trade.fills[0].time, trade.fills[0].execution.time, trade.log[0].time]
        ticker = ib.reqMktData(aapl)
        for _ in ib.loopUntil(lambda: ticker.ticks, timeout=5):
            pass
        times.append(ticker.ticks[0].time)
        depth = ib.reqMktDepth(aapl)
        for _ in ib.loopUntil(lambda: depth.domTicks, timeout=5):
            pass
        times.append(depth.domTicks[0].time)
        bars = ib.reqRealTimeBars(aapl, 5, 'TRADES', True)
        for _ in ib.loopUntil(lambda: len(bars) > 0, timeout=5):
            pass
        times.append(bars[0].time)
        ticks = ib.reqHistoricalTicks(aapl, '', '20260102 10:00:00', 1, 'TRADES', True)
        times.append(ticks[0].time)
        assert all(isinstance(t, dt.datetime) for t in times), times
        # Every declared Ticker field exists in the core.
        for name in Ticker.__annotations__:
            ticker.__getattr__(name)
        ib.disconnect()
    finally:
        server.close()
