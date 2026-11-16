"""End-to-end tests of the ib_async-style API against the fake TWS server."""

from __future__ import annotations

import asyncio
import datetime as dt
import math

import pytest
from ibkit import (
    IB,
    BarDataList,
    Contract,
    LimitOrder,
    Order,
    OrderStatus,
    PriceCondition,
    Stock,
    Ticker,
    Trade,
    _types,
    util,
)


def connect(tws) -> IB:
    ib = IB()
    ib.RequestTimeout = 5
    ib.connect('127.0.0.1', tws.port, clientId=1)
    return ib


def test_connect_synchronizes_positions_and_account(tws):
    ib = connect(tws)
    assert ib.isConnected()
    assert ib.managedAccounts() == ['DU123456']
    [pos] = ib.positions()
    assert (pos.contract.symbol, pos.position, pos.avgCost) == ('IBM', 10.0, 120.5)
    assert type(pos.contract).__name__ == 'Stock'
    assert any(
        v.tag == 'NetLiquidation' and v.value == '100000' for v in ib.accountValues()
    )
    assert dt.datetime(2026, 1, 1, tzinfo=dt.UTC) == ib.reqCurrentTime()
    ib.disconnect()
    assert not ib.isConnected()


def test_qualify_contracts_updates_in_place(tws):
    ib = connect(tws)
    aapl, xyz = Stock('AAPL', 'SMART', 'USD'), Stock('XYZ', 'SMART', 'USD')
    qualified = ib.qualifyContracts(aapl, xyz)
    assert qualified == [aapl]
    assert (aapl.conId, aapl.exchange, aapl.primaryExchange) == (
        265598,
        'SMART',
        'NASDAQ',
    )
    assert xyz.conId == 0
    details = ib.reqContractDetails(aapl)
    assert details[0].longName == 'APPLE INC'
    assert details[0].contract.conId == 265598


def test_place_order_until_filled_with_events(tws):
    ib = connect(tws)
    seen: list[str] = []
    ib.orderStatusEvent += lambda trade: seen.append(trade.orderStatus.status)
    commissions = []
    ib.commissionReportEvent += lambda trade, fill, report: commissions.append(
        report.commission
    )
    aapl = Stock('AAPL', 'SMART', 'USD', conId=265598)
    order = LimitOrder('BUY', 100, 150.0)
    trade = ib.placeOrder(aapl, order)
    assert isinstance(trade, Trade)
    assert order.orderId >= 1000
    for _ in ib.loopUntil(lambda: commissions, timeout=5):
        pass
    assert trade.orderStatus.status == OrderStatus.Filled
    assert trade.isDone()
    assert trade.filled() == 100
    assert trade.fills[0].execution.price == 150
    assert trade.fills[0].commissionReport.commission == 1.0
    assert isinstance(trade.fills[0].time, dt.datetime)
    assert commissions == [1.0]
    assert seen[-1] == 'Filled'
    assert [e.status for e in trade.log] == [
        'PendingSubmit',
        'Submitted',
        'Submitted',
        'Filled',
    ]
    assert ib.trades()[-1] is trade, 'the same Trade object is returned every time'
    assert ib.openTrades() == []


def test_market_data_ticker_is_live(tws):
    ib = connect(tws)
    aapl = Stock('AAPL', 'SMART', 'USD', conId=265598)
    ticker = ib.reqMktData(aapl)
    assert isinstance(ticker, Ticker)
    assert math.isnan(ticker.bid)
    updates = []
    ticker.updateEvent += updates.append
    for _ in ib.loopUntil(lambda: ticker.volume == 12345, timeout=5):
        pass
    assert (ticker.bid, ticker.bidSize, ticker.ask, ticker.askSize, ticker.last) == (
        150.25,
        300,
        150.5,
        200,
        150.4,
    )
    assert ticker.midpoint() == 150.375
    assert ticker.marketPrice() == 150.4
    assert ticker.contract.symbol == 'AAPL'
    assert isinstance(ticker.time, dt.datetime)
    assert updates and updates[0] is ticker
    assert ib.ticker(aapl) is ticker
    assert ib.cancelMktData(aapl)


def test_historical_data(tws):
    ib = connect(tws)
    aapl = Stock('AAPL', 'SMART', 'USD', conId=265598)
    bars = ib.reqHistoricalData(aapl, '', '2 D', '1 day', 'TRADES', True)
    assert isinstance(bars, BarDataList)
    assert len(bars) == 2
    assert bars[1].date == dt.date(2026, 1, 5)
    assert (bars[1].close, bars[1].barCount, bars[1].average) == (2.0, 20, 1.8)


def test_async_api(tws):
    async def main() -> None:
        ib = IB()
        ib.RequestTimeout = 5
        await ib.connectAsync('127.0.0.1', tws.port, clientId=1)
        [aapl] = await ib.qualifyContractsAsync(Stock('AAPL', 'SMART', 'USD'))
        assert aapl.conId == 265598
        trade = ib.placeOrder(aapl, LimitOrder('BUY', 100, 150.0))
        done = asyncio.get_running_loop().create_future()
        trade.filledEvent += lambda t: done.done() or done.set_result(t)
        assert (await asyncio.wait_for(done, 5)) is trade
        positions = await ib.reqPositionsAsync()
        assert positions[0].contract.symbol == 'IBM'
        ib.disconnect()

    asyncio.run(main())


def test_request_errors_raise_when_configured(tws):
    ib = IB()
    ib.RaiseRequestErrors = True
    ib.RequestTimeout = 5
    ib.connect('127.0.0.1', tws.port, clientId=1)
    from ibkit import RequestError

    with pytest.raises(RequestError) as info:
        ib.reqContractDetails(Stock('XYZ', 'SMART', 'USD'))
    assert info.value.args[1] == 200


def test_connection_refused_raises_connection_error():
    import socket

    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        port = s.getsockname()[1]
    with pytest.raises(ConnectionError, match='Check that TWS or IB Gateway runs'):
        IB().connect('127.0.0.1', port, clientId=1, timeout=2)


def test_order_round_trips_through_rust():
    o = LimitOrder(
        'SELL',
        5,
        10.5,
        tif='GTC',
        conditions=[PriceCondition(price=99.0, conId=1, exch='SMART')],
    )
    d = util.to_dict(o)
    assert d['conditions'][0]['condType'] == '1'
    back = util.from_dict('Order', d)
    assert isinstance(back, Order)
    assert back.conditions[0] == PriceCondition(price=99.0, conId=1, exch='SMART')
    assert (back.lmtPrice, back.tif, back.minQty) == (10.5, 'GTC', util.UNSET_INTEGER)


def test_generated_types_cover_ib_async_contract_fields():
    names = {f for f in _types.Contract.__dataclass_fields__}
    for ib_async_field in [
        'secType',
        'conId',
        'symbol',
        'lastTradeDateOrContractMonth',
        'strike',
        'right',
        'multiplier',
        'exchange',
        'primaryExchange',
        'currency',
        'localSymbol',
        'tradingClass',
        'includeExpired',
        'secIdType',
        'secId',
        'description',
        'issuerId',
        'comboLegsDescrip',
        'comboLegs',
        'deltaNeutralContract',
    ]:
        assert ib_async_field in names
    assert (
        Contract.create(secType='CASH', symbol='EUR', currency='USD').pair() == 'EURUSD'
    )
