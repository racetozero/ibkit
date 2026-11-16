"""ib_async compatibility pieces: tick bar operators, util functions,
IBDefaults, OrderState.numeric, setTimeout, WSH subscriptions, IBC."""

from __future__ import annotations

import asyncio
import dataclasses
import datetime as dt
import math

from ibkit import (
    IB,
    IBC,
    ConnectionStats,
    Event,
    IBDefaults,
    LimitOrder,
    Stock,
    TickerUpdateEvent,
    WshEventData,
    util,
)
from ibkit._types import TickData

T0 = dt.datetime(2026, 1, 2, tzinfo=dt.UTC)


@dataclasses.dataclass
class FakeTicker:
    ticks: list[TickData]
    time: dt.datetime = T0

    def midpoint(self) -> float:
        return 1.5


def trade_ticks(*pairs: tuple[float, float]) -> FakeTicker:
    return FakeTicker([TickData(T0, 4, p, s) for p, s in pairs])


def test_tick_and_volume_bars_aggregate_trades() -> None:
    ev = TickerUpdateEvent('updateEvent')
    tick_bars = ev.trades().tickbars(2)
    volume_bars = ev.trades().volumebars(10)
    done: list[int] = []
    tick_bars += lambda bars: done.append(len(bars))
    ev.emit(trade_ticks((10, 3), (12, 4)))
    ev.emit(trade_ticks((9, 5)))
    ev.emit(FakeTicker([TickData(T0, 1, 99, 1)]))  # bid tick: filtered out
    assert done == [1]
    first, second = tick_bars.bars
    assert (
        first.open,
        first.high,
        first.low,
        first.close,
        first.volume,
        first.count,
    ) == (10, 12, 10, 12, 7, 2)
    assert (second.open, second.count) == (9, 1)
    assert [(b.volume, b.count) for b in volume_bars.bars] == [(12, 3)]


def test_time_bars_follow_the_timer() -> None:
    ev = TickerUpdateEvent('updateEvent')
    timer = Event('timer')
    bars = ev.trades().timebars(timer)
    completed: list[object] = []
    bars += completed.append
    ev.emit(trade_ticks((5, 1)))  # before the first timer tick: ignored
    timer.emit(T0)
    ev.emit(trade_ticks((5, 1), (7, 2)))
    timer.emit(T0 + dt.timedelta(seconds=5))
    timer.emit(T0 + dt.timedelta(seconds=10))  # empty bar takes the last close
    assert [(b.open, b.high, b.close, b.volume) for b in completed] == [
        (5, 7, 7, 3),
        (7, 7, 7, 0),
    ]
    assert len(bars.bars) == 3


def test_midpoints_emit_once_per_update() -> None:
    ev = TickerUpdateEvent('updateEvent')
    got: list[tuple[object, ...]] = []
    ev.midpoints().connect(lambda *a: got.append(a))
    ev.emit(trade_ticks((1, 1), (2, 1)))
    ev.emit(FakeTicker([]))
    assert got == [(T0, 1.5, 0)]


def test_util_functions() -> None:
    assert util.formatSI(1234) == '1.23 k'
    assert util.formatSI(0.00012) == '120 u'
    assert util.formatSI(-5) == '-5 '
    stock = Stock('AAPL', 'SMART', 'USD')
    assert (
        util.dataclassRepr(stock)
        == "Stock(symbol='AAPL', secType='STK', exchange='SMART', currency='USD')"
    )
    assert util.tree([ConnectionStats(1.0, 2.0, 3, 4, 5, 6), dt.date(2026, 1, 2)]) == [
        {
            'startTime': 1.0,
            'duration': 2.0,
            'numBytesRecv': 3,
            'numBytesSent': 4,
            'numMsgRecv': 5,
            'numMsgSent': 6,
        },
        '2026-01-02',
    ]
    assert util.isnamedtupleinstance(ConnectionStats(0, 0, 0, 0, 0, 0))
    assert not util.isnamedtupleinstance((1, 2))
    start = dt.datetime.now(dt.UTC) + dt.timedelta(milliseconds=20)
    times = list(util.timeRange(start, start + dt.timedelta(milliseconds=50), 0.025))
    assert len(times) == 3
    fired: list[int] = []
    util.schedule(
        dt.datetime.now(dt.UTC) + dt.timedelta(milliseconds=10), fired.append, 1
    )
    util.run(asyncio.sleep(0.05))
    assert fired == [1]
    assert IB.run is util.run


def test_ibc_command() -> None:
    ibc = IBC(1030, gateway=True, tradingMode='paper', ibcPath='/opt/ibc', userid='me')
    assert ibc.command() == [
        '/opt/ibc/scripts/ibcstart.sh',
        '1030',
        '--gateway',
        '--mode=paper',
        '--ibc-path=/opt/ibc',
        '--user=me',
    ]


def test_defaults_stats_numeric_timeout_and_wsh(tws) -> None:
    ib = IB(
        defaults=IBDefaults(unset=None, timezone=dt.timezone(dt.timedelta(hours=-5)))
    )
    ib.RequestTimeout = 5
    ib.connect('127.0.0.1', tws.port, clientId=1)
    try:
        stats = ib.connectionStats()
        assert isinstance(stats, ConnectionStats) and stats.numMsgRecv > 0

        ticker = ib.reqMktData(Stock('AAPL', 'SMART', 'USD'), snapshot=True)
        ib.sleep(0.3)
        assert ticker.bid == 150.25
        assert ticker.halted is None  # never sent: IBDefaults.unset

        state = ib.whatIfOrder(
            Stock('WHATIF', 'SMART', 'USD'), LimitOrder('BUY', 1, 1.0)
        )
        numeric = state.numeric()
        assert (
            numeric.initMarginChange,
            numeric.initMarginBefore,
            numeric.commission,
        ) == (2500.5, None, 1.5)
        assert state.formatted().initMarginChange == '2,500.50'

        trade = ib.placeOrder(
            Stock('AAPL', 'SMART', 'USD'), LimitOrder('BUY', 100, 150.0)
        )
        ib.sleep(0.2)
        assert trade.log[0].time.utcoffset() == dt.timedelta(hours=-5)

        idle: list[float] = []
        ib.timeoutEvent += idle.append
        ib.setTimeout(0.2)
        ib.sleep(0.6)
        assert len(idle) == 1 and idle[0] >= 0.2

        meta: list[str] = []
        events: list[str] = []
        ib.wshMetaEvent += meta.append
        ib.wshEvent += events.append
        ib.reqWshMetaData()
        ib.reqWshEventData(WshEventData(conId=8314))
        ib.sleep(0.3)
        assert (meta, events) == (['{"meta":1}'], ['{"event":8314}'])
        ib.cancelWshMetaData()
        ib.cancelWshEventData()
    finally:
        ib.disconnect()


def test_web_oauth2_and_new_endpoints_against_local_server() -> None:
    import http.server
    import json
    import threading

    import pytest
    from ibkit import OAuth2Config, WebClient

    seen: list[tuple[str, str, object]] = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def _answer(self) -> None:
            length = int(self.headers.get('Content-Length') or 0)
            raw = self.rfile.read(length) if length else b''
            seen.append((self.command, self.path, json.loads(raw) if raw else None))
            body = b'{}'
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        do_GET = do_POST = do_PUT = do_DELETE = _answer

        def log_message(self, *_args: object) -> None:
            pass

    server = http.server.HTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f'http://127.0.0.1:{server.server_port}/v1/api'
    try:
        web = WebClient(base)
        web.activateAlert('DU1', 9, False)
        web.forecastMarket(766914406, 'FORECASTX')
        web.fyiSetEmailDelivery(True)
        web.paSummary(['DU1'])
        assert seen == [
            (
                'POST',
                '/v1/api/iserver/account/DU1/alert/activate',
                {'alertId': 9, 'alertActive': 0},
            ),
            (
                'GET',
                '/v1/api/forecast/contract/market?underlyingConid=766914406&exchange=FORECASTX',
                None,
            ),
            ('PUT', '/v1/api/fyi/deliveryoptions/email?enabled=true', None),
            ('POST', '/v1/api/pa/summary', {'acctIds': ['DU1']}),
        ]
        oauth2 = WebClient(
            base, oauth2=OAuth2Config('client', 'kid', 'user', 'not a key', '1.2.3.4')
        )
        with pytest.raises(ValueError, match='call oauth2_init'):
            oauth2.accounts()
        with pytest.raises(Exception, match='PEM text of the key'):
            oauth2.oauth2Init()
    finally:
        server.shutdown()


def test_fix_builders_and_tls_option() -> None:
    import pytest
    from ibkit import FixSession, Option
    from ibkit.fix import (
        allocationInstruction,
        cancelRequest,
        multilegCancelReplace,
        newOrderMultileg,
        orderStatusRequest,
    )

    call = Option('AAPL', '20261218', 200, 'C', 'SMART', currency='USD')
    put = Option('AAPL', '20261218', 190, 'P', 'SMART', currency='USD')
    combo = Stock('AAPL', 'SMART', 'USD')
    order = LimitOrder('BUY', 1, 1.25, account='U1')
    fields = dict(
        newOrderMultileg('C1', combo, [(call, 1, 'BUY'), (put, 1, 'SELL', 'C')], order)
    )
    assert (fields[555], fields[167], fields[44], fields[21]) == (
        '2',
        'MULTILEG',
        '1.25',
        '2',
    )
    ac = dict(multilegCancelReplace('C1', 'C2', combo, order))
    assert (ac[41], ac[11], ac[1]) == ('C1', 'C2', 'U1')
    alloc = allocationInstruction(
        'A1', 'F1', call, 'BUY', 10, 1.5, '20260102', [('U1', 6), ('U2', 4)]
    )
    assert [f for f in alloc if f[0] in (79, 80, 153)] == [
        (79, 'U1'),
        (80, '6'),
        (153, '1.5'),
        (79, 'U2'),
        (80, '4'),
        (153, '1.5'),
    ]
    with pytest.raises(ValueError, match='refAllocId|ref_alloc_id'):
        allocationInstruction(
            'A2', 'F1', call, 'BUY', 10, 1.5, '20260102', [], 'cancel'
        )
    assert orderStatusRequest('U1') == [(1, 'U1'), (11, '*')]
    from ibkit import Future
    from ibkit.fix import newOrderSingle, optionExercise

    es = Future('ES', '202612', 'CME', currency='USD')
    fut = LimitOrder('BUY', 1, 6000, account='U1', manualOrderIndicator=0, extOperator='bot1')
    with pytest.raises(ValueError, match='sender_location_id'):
        newOrderSingle('F1', es, fut)
    f = dict(newOrderSingle('F2', es, fut, senderLocationId='US,NY', parentClOrdId='P1'))
    assert (f[142], f[6107], f[1028], f[8089]) == ('US,NY', 'P1', 'N', 'bot1')
    ex = dict(optionExercise('X1', call, order, 'exercise', True))
    assert (ex[6809], ex[8012]) == ('1', '1')
    assert dict(cancelRequest('C1', 'C3', 'U1', call, 'BUY', 1))[1] == 'U1'
    with pytest.raises(ValueError, match='tlsCaFile'):
        FixSession.connect('127.0.0.1', 9, 'ME', tlsCaFile='/no/such/file.pem')


def test_ib_against_a_protobuf_server() -> None:
    from ibkit import _ibkit

    pb = _ibkit.FakeTws.protobuf
    seen: list[str] = []

    def responder(req: list[str]) -> list[list[str]]:
        seen.append(req[0])
        if len(req) < 3 or req[1] != 'protobuf':
            return []
        msg_id, hex_body = req[0], req[2]
        if msg_id == '61':
            position = {
                'account': 'DU1',
                'contract': {'conId': 8314, 'symbol': 'IBM', 'secType': 'STK'},
                'position': '10',
                'avgCost': 120.5,
            }
            return [pb(61, 'Position', position), pb(62, 'PositionEnd', {})]
        if msg_id == '5':
            return [pb(53, 'OpenOrdersEnd', {})]
        if msg_id == '99':
            return [pb(102, 'CompletedOrdersEnd', {})]
        if msg_id == '6':
            return [pb(54, 'AccountDataEnd', {'accountName': 'DU1'})]
        if msg_id == '76':
            req_id = _ibkit.FakeTws.parse('AccountUpdatesMultiRequest', hex_body)[
                'reqId'
            ]
            return [pb(74, 'AccountUpdateMultiEnd', {'reqId': req_id})]
        if msg_id == '7':
            req_id = _ibkit.FakeTws.parse('ExecutionRequest', hex_body)['reqId']
            return [pb(55, 'ExecutionDetailsEnd', {'reqId': req_id})]
        if msg_id == '49':
            return [pb(49, 'CurrentTime', {'currentTime': 1767225600})]
        if msg_id == '108':
            req_id = _ibkit.FakeTws.parse('ConfigRequest', hex_body)['reqId']
            return [
                pb(
                    110,
                    'ConfigResponse',
                    {'reqId': req_id, 'lockAndExit': {'autoLogoffTime': '11:45 PM'}},
                )
            ]
        return []

    server = _ibkit.FakeTws(226, 1000, 'DU1', responder)
    ib = IB()
    ib.RequestTimeout = 5
    try:
        ib.connect('127.0.0.1', server.port, clientId=1, raiseSyncErrors=True)
        assert ib.client.serverVersion() == 226
        assert [(p.contract.symbol, p.position) for p in ib.positions()] == [
            ('IBM', 10.0)
        ]
        assert ib.reqCurrentTime() == dt.datetime(2026, 1, 1, tzinfo=dt.UTC)
        assert ib.reqConfig()['lockAndExit'] == {'autoLogoffTime': '11:45 PM'}
        assert {'61', '5', '7', '108'} <= set(seen)
    finally:
        ib.disconnect()
        server.close()
