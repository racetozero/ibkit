"""More end-to-end and unit tests of the Python package."""

from __future__ import annotations

import asyncio
import datetime as dt
import http.server
import json
import threading

import pytest
from ibkit import (
    IB,
    Event,
    FixSession,
    LimitOrder,
    MarketOrder,
    OrderStatus,
    Stock,
    WebClient,
    util,
)
from ibkit.fix import newOrderSingle


def connect(tws) -> IB:
    ib = IB()
    ib.RequestTimeout = 5
    ib.connect('127.0.0.1', tws.port, clientId=1)
    return ib


def test_cancel_order(tws):
    ib = connect(tws)
    cancelled = []
    trade = ib.placeOrder(
        Stock('MSFT', 'SMART', 'USD', conId=272093), LimitOrder('BUY', 100, 150.0)
    )
    trade.cancelledEvent += cancelled.append
    for _ in ib.loopUntil(lambda: trade.orderStatus.status == 'Submitted', timeout=5):
        pass
    ib.cancelOrder(trade.order)
    assert trade.orderStatus.status in (
        OrderStatus.PendingCancel,
        OrderStatus.Cancelled,
    )
    for _ in ib.loopUntil(lambda: cancelled, timeout=5):
        pass
    assert trade.orderStatus.status == OrderStatus.Cancelled
    assert cancelled == [trade]


def test_rejected_order_emits_error_and_cancels(tws):
    ib = connect(tws)
    errors = []
    ib.errorEvent += lambda reqId, code, msg, contract: errors.append(code)
    trade = ib.placeOrder(Stock('REJ', 'SMART', 'USD'), MarketOrder('BUY', 1))
    for _ in ib.loopUntil(lambda: errors, timeout=5):
        pass
    assert errors == [201]
    assert trade.orderStatus.status == 'Cancelled'
    assert trade.log[-1].errorCode == 201


def test_what_if_and_bracket(tws):
    ib = connect(tws)
    state = ib.whatIfOrder(Stock('WHATIF', 'SMART', 'USD'), LimitOrder('BUY', 1, 1.0))
    assert (state.initMarginChange, state.commission) == ('2500.5', 1.5)
    parent, tp, sl = ib.bracketOrder('BUY', 10, 100.0, 110.0, 95.0, tif='GTC')
    assert tp.parentId == sl.parentId == parent.orderId
    assert (parent.transmit, sl.transmit, sl.orderType, tp.tif) == (
        False,
        True,
        'STP',
        'GTC',
    )


def test_depth_snapshot_and_keep_up_to_date_bars(tws):
    ib = connect(tws)
    aapl = Stock('AAPL', 'SMART', 'USD', conId=265598)
    ticker = ib.reqMktDepth(aapl)
    for _ in ib.loopUntil(lambda: ticker.domAsks, timeout=5):
        pass
    assert (ticker.domBids[0].price, ticker.domAsks[0].price) == (150.25, 150.5)
    [snap] = ib.reqTickers(aapl)
    assert snap.last == 150.4
    updates = []
    ib.barUpdateEvent += lambda b, new: updates.append(new)
    bars = ib.reqHistoricalData(
        aapl, '', '2 D', '1 day', 'TRADES', True, keepUpToDate=True
    )
    for _ in ib.loopUntil(lambda: len(bars) == 3, timeout=5):
        pass
    assert bars[-1].date == dt.date(2026, 1, 6)
    assert updates == [True]
    ib.cancelHistoricalData(bars)


def test_disconnect_emits_event(tws):
    ib = connect(tws)
    seen = []
    ib.disconnectedEvent += lambda: seen.append(True)
    ib.disconnect()
    assert seen == [True]
    assert not ib.isConnected()
    assert ib.positions(), 'cached state stays readable, like ib_async'
    with pytest.raises(ConnectionError):
        ib.reqCurrentTime()


def test_event_handlers_and_await():
    ev = Event('x')
    got = []
    ev += got.append
    ev += got.append
    ev.emit(1)
    assert got == [1]
    ev -= got.append
    ev.emit(2)
    assert got == [1]

    async def main() -> object:
        asyncio.get_running_loop().call_later(0.01, ev.emit, 'done')
        return await ev

    assert asyncio.run(main()) == 'done'


def test_ib_datetime_helpers():
    assert util.parseIBDatetime('20260102') == dt.date(2026, 1, 2)
    assert util.parseIBDatetime('20260102 10:00:00') == dt.datetime(2026, 1, 2, 10)
    assert util.parseIBDatetime('20260102-10:00:00').tzinfo == dt.UTC
    assert util.parseIBDatetime('20260102 10:00:00 US/Eastern').tzinfo is not None
    assert util.parseIBDatetime('1767225600') == dt.datetime(
        2026, 1, 1, tzinfo=dt.UTC
    )
    assert (
        util.formatIBDatetime(dt.datetime(2026, 1, 2, 10, tzinfo=dt.UTC))
        == '20260102-10:00:00'
    )
    assert util.formatIBDatetime(dt.date(2026, 1, 2)) == '20260102 23:59:59'
    assert util.formatIBDatetime('') == ''


def test_fix_new_order_single_fields():
    fields = dict(
        newOrderSingle(
            'C1',
            Stock('IBM', 'SMART', 'USD'),
            LimitOrder('SELL', 5, 80.5, account='U1'),
        )
    )
    assert (
        fields[11],
        fields[55],
        fields[167],
        fields[54],
        fields[40],
        fields[44],
        fields[100],
    ) == ('C1', 'IBM', 'CS', '2', '2', '80.5', 'SMART')
    with pytest.raises(ConnectionError, match='could not connect to the FIX gateway'):
        FixSession.connect('127.0.0.1', 9, 'ME')


def test_web_client_against_local_server():
    seen = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self) -> None:
            seen.append(self.path)
            body = json.dumps([{'conid': 265598, '31': '150.25'}]).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_args: object) -> None:
            pass

    server = http.server.HTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        web = WebClient(f'http://127.0.0.1:{server.server_port}/v1/api')
        assert web.snapshot([265598], ['31'])[0]['31'] == '150.25'
        assert seen == ['/v1/api/iserver/marketdata/snapshot?conids=265598&fields=31']
    finally:
        server.shutdown()


def test_low_level_client_sends_any_request_and_receives_typed_messages(tws):
    from ibkit import Client, Wrapper

    client = Client.connect('127.0.0.1', tws.port, clientId=2)
    assert client.serverVersion() == 178
    assert client.getAccounts() == ['DU123456']

    class W(Wrapper):
        def __init__(self) -> None:
            self.times: list[int] = []
            self.ids: list[int] = []

        def currentTime(self, fields: dict) -> None:
            self.times.append(fields['time'])

        def nextValidId(self, fields: dict) -> None:
            self.ids.append(fields['orderId'])

    w = W()
    client.reqCurrentTime()
    client.run(w, timeout=5, until=lambda: w.times)
    assert w.times == [1767225600]
    assert w.ids == [1000], 'messages received during connect are delivered too'
    client.reqMktData(
        client.getReqId(),
        Stock('AAPL', 'SMART', 'USD', conId=265598),
        '',
        False,
        False,
        [],
    )
    names = {n for n, _ in client.messages(2.0)}
    assert {'tickPrice', 'tickSize'} <= names
    with pytest.raises(ValueError, match='contract is not valid'):
        client.reqMktData(1, {'conId': 'x'}, '', False, False, [])
    client.disconnect()


def test_ib_client_shares_the_connection_and_message_events(tws):
    ib = IB()
    ib.EmitMessages = True
    ib.RequestTimeout = 5
    seen = []
    ib.messageEvent += lambda name, fields: seen.append((name, fields))
    ib.connect('127.0.0.1', tws.port, clientId=1)
    ib.client.reqCurrentTime()
    for _ in ib.loopUntil(lambda: any(n == 'currentTime' for n, _ in seen), timeout=5):
        pass
    assert ('currentTime', {'time': 1767225600}) in seen
    with pytest.raises(ValueError, match='shares an IB connection'):
        ib.client.messages(0)


def test_websocket_client_against_local_server():
    import socket
    import subprocess
    import sys
    import textwrap

    from ibkit import WsClient

    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        port = s.getsockname()[1]
    server = textwrap.dedent(f"""
        import asyncio, base64, hashlib, json
        async def handle(r, w):
            head = (await r.readuntil(b'\\r\\n\\r\\n')).decode()
            key = [l.split(': ', 1)[1] for l in head.split('\\r\\n') if l.lower().startswith('sec-websocket-key')][0]
            acc = base64.b64encode(hashlib.sha1((key + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').encode()).digest()).decode()
            w.write(f'HTTP/1.1 101 Switching Protocols\\r\\nUpgrade: websocket\\r\\nConnection: Upgrade\\r\\nSec-WebSocket-Accept: {{acc}}\\r\\n\\r\\n'.encode())
            def frame(t):
                b = t.encode(); return bytes([0x81, len(b)]) + b
            w.write(frame(json.dumps({{'topic': 'system', 'success': 'u'}})))
            await w.drain()
            while True:
                h = await r.readexactly(2); n = h[1] & 0x7f; mask = await r.readexactly(4)
                data = bytes(b ^ mask[i % 4] for i, b in enumerate(await r.readexactly(n))).decode()
                if data.startswith('smd+'):
                    w.write(frame(json.dumps({{'topic': 'smd+1', '31': '9.5'}}))); await w.drain()
        async def main():
            async with await asyncio.start_server(handle, '127.0.0.1', {port}):
                await asyncio.sleep(30)
        asyncio.run(main())
    """)
    proc = subprocess.Popen([sys.executable, '-c', server])
    try:
        for _ in range(100):
            try:
                ws = WsClient.connect(
                    f'ws://127.0.0.1:{port}/v1/api/ws', heartbeat=None
                )
                break
            except ConnectionError:
                import time

                time.sleep(0.05)
        assert ws.recv(5)[0] == 'system'
        ws.subscribeMarketData(1, ['31'])
        topic, args, data = ws.recv(5)
        assert (topic, args, data['31']) == ('smd', ['1'], '9.5')
        assert ws.recv(0.1) is None
    finally:
        proc.kill()
