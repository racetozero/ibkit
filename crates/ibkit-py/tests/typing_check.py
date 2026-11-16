"""Type checks of the public API as a user sees it. Pyright checks this file
(CI: ``pyright tests/typing_check.py``) against the installed package; it is
not a pytest module and never runs."""

from __future__ import annotations

import datetime as dt
from typing import assert_type

from ibkit import (
    IB,
    AccountValue,
    BarDataList,
    CommissionReport,
    Contract,
    ContractDetails,
    Fill,
    IbkitError,
    OptionComputation,
    PortfolioItem,
    Position,
    RequestError,
    Stock,
    Ticker,
    Trade,
)
from ibkit.errors import ConnectionError as IbkitConnectionError
from ibkit.event import Event
from ibkit.gateway import Control, ErrorCode, GatewayClient, State
from ibkit.gateway import Event as GatewayEvent
from ibkit.web import AsyncWebClient, Json, WebClient


async def ib_api(ib: IB) -> None:
    assert_type(ib.orderStatusEvent, Event[Trade])
    assert_type(ib.execDetailsEvent, Event[Trade, Fill])
    assert_type(ib.commissionReportEvent, Event[Trade, Fill, CommissionReport])
    assert_type(ib.pendingTickersEvent, Event[set[Ticker]])
    assert_type(ib.errorEvent, Event[int, int, str, Contract | None])
    assert_type(await ib.orderStatusEvent, Trade)
    assert_type(await ib.execDetailsEvent, tuple[Trade, Fill])

    def on_fill(trade: Trade, fill: Fill) -> None: ...

    ib.execDetailsEvent += on_fill
    assert_type(ib.positions(), list[Position])
    assert_type(ib.portfolio(), list[PortfolioItem])
    assert_type(ib.accountValues(), list[AccountValue])
    assert_type(ib.fills(), list[Fill])
    aapl = Stock('AAPL', 'SMART', 'USD')
    assert_type(ib.reqContractDetails(aapl), list[ContractDetails])
    assert_type(await ib.reqContractDetailsAsync(aapl), list[ContractDetails])
    assert_type(await ib.qualifyContractsAsync(aapl), list[Contract])
    assert_type(
        await ib.qualifyContractsAsync(aapl, returnAll=True), list[Contract | None]
    )
    assert_type(await ib.reqCurrentTimeInMillisAsync(), int)
    ticker = ib.reqMktData(aapl)
    assert_type(ticker, Ticker)
    assert_type(ticker.bid, float)
    assert_type(ticker.time, dt.datetime | None)
    assert_type(ticker.contract, Contract)
    assert_type(ticker.modelGreeks, OptionComputation | None)
    bars = ib.reqHistoricalData(aapl, '', '1 D', '1 hour', 'TRADES', True)
    assert_type(bars, BarDataList)
    assert_type(bars.updateEvent, Event[BarDataList, bool])
    trade = ib.placeOrder(aapl, ib.bracketOrder('BUY', 1, 1, 2, 0.5).parent)
    assert_type(trade.fillEvent, Event[Trade, Fill])


def errors(error: Exception) -> None:
    if isinstance(error, RequestError):
        assert_type(error.code, int)
    assert isinstance(IbkitConnectionError('x'), ConnectionError)
    assert isinstance(IbkitConnectionError('x'), IbkitError)


async def gateway(gw: GatewayClient) -> None:
    status = gw.status()
    assert_type(status['state'], State)
    assert_type(status['last_start_result']['error_code'], ErrorCode)
    recovery = status['recovery']
    if recovery is not None and recovery['phase'] == 'blocked':
        assert_type(recovery['reason'], ErrorCode)
    gw.control(Control.RECONNECT_ACCOUNT)
    async for event in await gw.eventsAsync():
        assert_type(event, GatewayEvent)
        if event['event'] == 'state_changed':
            assert_type(event['state'], State)
    assert_type(gw.config()['api_port'], int)


async def web(client: WebClient, aclient: AsyncWebClient) -> None:
    assert_type(client.accounts(), Json)
    assert_type(await aclient.accounts(), Json)
    assert_type(await client.requestAsync('GET', 'iserver/accounts'), Json)

