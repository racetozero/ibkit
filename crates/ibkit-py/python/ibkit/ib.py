"""High-level client with the ib_async ``IB`` interface on the Rust core."""

from __future__ import annotations

import asyncio
import datetime as dt
import logging
import time
from collections.abc import Callable, Iterator
from enum import IntFlag
from typing import Any, ClassVar, Literal, Self, TypeVar, overload

from . import _ibkit, util
from ._types import (
    AccountValue,
    CommissionReport,
    ContractDescription,
    ContractDetails,
    DepthMktDataDescription,
    Execution,
    ExecutionFilter,
    FamilyCode,
    Fill,
    HistogramData,
    HistoricalNews,
    HistoricalSchedule,
    HistoricalTick,
    HistoricalTickBidAsk,
    HistoricalTickLast,
    NewsArticle,
    NewsBulletin,
    NewsProvider,
    NewsTick,
    OptionChain,
    OptionComputation,
    PnL,
    PnLSingle,
    PortfolioItem,
    Position,
    PriceIncrement,
    ScanData,
    ScannerSubscription,
    SmartComponent,
    SoftDollarTier,
    TagValue,
    WshEventData,
)
from .client import Client, callback_name
from .contract import Contract
from .errors import ConnectionError
from .event import Event
from .objects import (
    BarDataList,
    ConnectionStats,
    IBDefaults,
    LivePnL,
    RealTimeBarList,
    ScanDataList,
)
from .order import BracketOrder, Order, OrderState, Trade
from .ticker import Ticker
from .util import from_dict, to_dict

__all__ = ['IB', 'StartupFetch', 'StartupFetchALL', 'StartupFetchNONE']

_logger = logging.getLogger('ibkit.ib')


class StartupFetch(IntFlag):
    POSITIONS = 1
    ORDERS_OPEN = 2
    ORDERS_COMPLETE = 4
    ACCOUNT_UPDATES = 8
    SUB_ACCOUNT_UPDATES = 16
    EXECUTIONS = 32


StartupFetchNONE: StartupFetch = StartupFetch(0)
StartupFetchALL: StartupFetch = StartupFetch(63)


_T = TypeVar('_T')

#: A date, a datetime or a string in TWS format; empty means now.
DateTime = dt.date | dt.datetime | str | None


def _list(cls: type[_T], items: list[dict[str, Any]]) -> list[_T]:
    return [from_dict(cls.__name__, i) for i in items]


def _qualified_into(target: Contract, d: dict[str, Any]) -> Contract:
    """Copy qualified fields into the caller's contract object (ib_async
    updates contracts in place)."""
    q = from_dict('Contract', d)
    util.dataclassUpdate(target, q)
    return target


class IB:
    """Interactive Brokers client (ib_async compatible).

    Synchronous use::

        ib = IB()
        ib.connect('127.0.0.1', 7497, clientId=1)
        contract = Stock('AAPL', 'SMART', 'USD')
        ib.qualifyContracts(contract)
        ticker = ib.reqMktData(contract)
        ib.sleep(2)

    Every request also has an ``...Async`` coroutine variant for asyncio.
    """

    events: ClassVar[tuple[str, ...]] = (
        'connectedEvent',
        'disconnectedEvent',
        'updateEvent',
        'pendingTickersEvent',
        'barUpdateEvent',
        'newOrderEvent',
        'orderModifyEvent',
        'cancelOrderEvent',
        'openOrderEvent',
        'orderStatusEvent',
        'execDetailsEvent',
        'commissionReportEvent',
        'updatePortfolioEvent',
        'positionEvent',
        'accountValueEvent',
        'accountSummaryEvent',
        'pnlEvent',
        'pnlSingleEvent',
        'scannerDataEvent',
        'tickNewsEvent',
        'newsBulletinEvent',
        'wshMetaEvent',
        'wshEvent',
        'errorEvent',
        'timeoutEvent',
        'messageEvent',
    )

    connectedEvent: Event[()]
    disconnectedEvent: Event[()]
    updateEvent: Event[()]
    pendingTickersEvent: Event[set[Ticker]]
    barUpdateEvent: Event[BarDataList | RealTimeBarList, bool]
    newOrderEvent: Event[Trade]
    orderModifyEvent: Event[Trade]
    cancelOrderEvent: Event[Trade]
    openOrderEvent: Event[Trade]
    orderStatusEvent: Event[Trade]
    execDetailsEvent: Event[Trade, Fill]
    commissionReportEvent: Event[Trade, Fill, CommissionReport]
    updatePortfolioEvent: Event[PortfolioItem]
    positionEvent: Event[Position]
    accountValueEvent: Event[AccountValue]
    accountSummaryEvent: Event[AccountValue]
    pnlEvent: Event[PnL]
    pnlSingleEvent: Event[PnLSingle]
    scannerDataEvent: Event[ScanDataList]
    tickNewsEvent: Event[NewsTick]
    newsBulletinEvent: Event[NewsBulletin]
    wshMetaEvent: Event[str]
    wshEvent: Event[str]
    #: ``(reqId, errorCode, errorString, contract)``.
    errorEvent: Event[int, int, str, Contract | None]
    #: ``(idlePeriod)`` in seconds.
    timeoutEvent: Event[float]
    #: ``(callbackName, fields)``; only with ``EmitMessages``.
    messageEvent: Event[str, Any]

    RequestTimeout: float = 0
    RaiseRequestErrors: bool = False
    MaxSyncedSubAccounts: int = 50
    #: Emit every decoded message as ``messageEvent(name, fields)`` (costs time per message).
    EmitMessages: bool = False

    run: ClassVar[Callable[..., Any]] = staticmethod(util.run)

    def __init__(self, defaults: IBDefaults | None = None) -> None:
        self.defaults: IBDefaults = defaults or IBDefaults()
        self._wshMetaReqId = 0
        self._wshEventReqId = 0
        for name in self.events:
            setattr(self, name, Event(name))
        self._core: _ibkit.IBCore | None = None
        self._tickers: dict[int, Ticker] = {}
        self._trades: dict[int, Trade] = {}
        self._bars: dict[int, Any] = {}
        self._pump: asyncio.Task[None] | None = None
        self._pending: set[Ticker] = set()

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *_exc: object) -> None:
        self.disconnect()

    def __repr__(self) -> str:
        core = self._core
        state = (
            f'clientId={core.client_id()}, serverVersion={core.server_version()}'
            if core is not None and self.isConnected()
            else 'not connected'
        )
        return f'<IB {state}>'

    # ---- connection -------------------------------------------------------

    def _connect_args(
        self,
        host: str,
        port: int,
        clientId: int,
        timeout: float | None,
        readonly: bool,
        account: str,
        raiseSyncErrors: bool,
        fetchFields: StartupFetch,
    ) -> dict[str, Any]:
        return {
            'host': host,
            'port': port,
            'clientId': clientId,
            'timeout': timeout or 0,
            'readonly': readonly,
            'account': account,
            'raiseSyncErrors': raiseSyncErrors,
            'raiseRequestErrors': self.RaiseRequestErrors,
            'requestTimeout': self.RequestTimeout or 0,
            'emitMessages': self.EmitMessages,
            'fetchFields': int(fetchFields),
            'emptyPrice': self.defaults.emptyPrice,
            'emptySize': self.defaults.emptySize,
        }

    def connect(
        self,
        host: str = '127.0.0.1',
        port: int = 7497,
        clientId: int = 1,
        timeout: float | None = 4,
        readonly: bool = False,
        account: str = '',
        raiseSyncErrors: bool = False,
        fetchFields: StartupFetch = StartupFetchALL,
    ) -> IB:
        """Connect to TWS or IB Gateway and synchronize state."""
        args = self._connect_args(
            host,
            port,
            clientId,
            timeout,
            readonly,
            account,
            raiseSyncErrors,
            fetchFields,
        )
        self._core = _ibkit.IBCore.connect(args, True)
        self.connectedEvent.emit()
        return self

    async def connectAsync(
        self,
        host: str = '127.0.0.1',
        port: int = 7497,
        clientId: int = 1,
        timeout: float | None = 4,
        readonly: bool = False,
        account: str = '',
        raiseSyncErrors: bool = False,
        fetchFields: StartupFetch = StartupFetchALL,
    ) -> IB:
        args = self._connect_args(
            host,
            port,
            clientId,
            timeout,
            readonly,
            account,
            raiseSyncErrors,
            fetchFields,
        )
        self._core = await _ibkit.IBCore.connect(args, False)
        self._pump = asyncio.ensure_future(self._pump_loop())
        self.connectedEvent.emit()
        return self

    def disconnect(self) -> None:
        if self._core is not None:
            self._core.disconnect()
            if self._pump is None:
                # The reader thread reports the closed socket a moment later;
                # wait for it, so that disconnectedEvent fires before return.
                deadline = time.monotonic() + 1.0
                while (left := deadline - time.monotonic()) > 0:
                    events = self._core.poll_events(left)
                    self._dispatch(events)
                    if any(e[0] == 'disconnected' for e in events):
                        break
        if self._pump is not None:
            self._pump.cancel()
            self._pump = None

    def isConnected(self) -> bool:
        return self._core is not None and self._core.is_connected()

    @property
    def client(self) -> Client:
        """Low-level client on this connection: every TWS request by IB name."""
        return Client(self._c.client())

    @property
    def _c(self) -> _ibkit.IBCore:
        if self._core is None:
            raise ConnectionError('Not connected; call IB.connect() first')
        return self._core

    # ---- event dispatch ---------------------------------------------------

    def _ticker(self, core: Any) -> Ticker:
        t = self._tickers.get(core.id())
        if t is None:
            t = self._tickers[core.id()] = Ticker(core, self.defaults.unset)
        return t

    def _trade(self, core: Any) -> Trade:
        t = self._trades.get(core.id())
        if t is None:
            t = self._trades[core.id()] = Trade(core, self.defaults.timezone)
        return t

    def _dispatch(self, events: list[tuple[Any, ...]]) -> None:
        for ev in events:
            kind = ev[0]
            if kind == 'update':
                self.updateEvent.emit()
            elif kind == 'pendingTickers':
                tickers = {self._ticker(c) for c in ev[1]}
                self._pending = tickers
                for t in tickers:
                    t.updateEvent.emit(t)
                self.pendingTickersEvent.emit(tickers)
            elif kind in (
                'newOrder',
                'orderModify',
                'cancelOrder',
                'openOrder',
                'orderStatus',
                'filled',
                'cancelled',
            ):
                trade = self._trade(ev[1])
                trade._invalidate()
                if kind == 'newOrder':
                    self.newOrderEvent.emit(trade)
                elif kind == 'orderModify':
                    trade.modifyEvent.emit(trade)
                    self.orderModifyEvent.emit(trade)
                elif kind == 'cancelOrder':
                    trade.cancelEvent.emit(trade)
                    self.cancelOrderEvent.emit(trade)
                elif kind == 'openOrder':
                    self.openOrderEvent.emit(trade)
                elif kind == 'orderStatus':
                    trade.statusEvent.emit(trade)
                    self.orderStatusEvent.emit(trade)
                elif kind == 'filled':
                    trade.filledEvent.emit(trade)
                else:
                    trade.cancelledEvent.emit(trade)
            elif kind == 'execDetails':
                trade = self._trade(ev[1])
                trade._invalidate()
                fill = from_dict('Fill', ev[2])
                trade.fillEvent.emit(trade, fill)
                self.execDetailsEvent.emit(trade, fill)
            elif kind == 'commissionReport':
                trade = self._trade(ev[1])
                trade._invalidate()
                fill, report = (
                    from_dict('Fill', ev[2]),
                    from_dict('CommissionReport', ev[3]),
                )
                trade.commissionReportEvent.emit(trade, fill, report)
                self.commissionReportEvent.emit(trade, fill, report)
            elif kind == 'barUpdate':
                bars = self._bars.get(ev[1])
                if bars is not None:
                    bars._refresh()
                    bars.updateEvent.emit(bars, ev[2])
                    self.barUpdateEvent.emit(bars, ev[2])
            elif kind == 'scannerData':
                data = self._bars.get(ev[1])
                if isinstance(data, ScanDataList):
                    data._refresh()
                    data.updateEvent.emit(data)
                    self.scannerDataEvent.emit(data)
            elif kind == 'updatePortfolio':
                self.updatePortfolioEvent.emit(from_dict('PortfolioItem', ev[1]))
            elif kind == 'position':
                self.positionEvent.emit(from_dict('Position', ev[1]))
            elif kind == 'accountValue':
                self.accountValueEvent.emit(from_dict('AccountValue', ev[1]))
            elif kind == 'accountSummary':
                self.accountSummaryEvent.emit(from_dict('AccountValue', ev[1]))
            elif kind == 'pnl':
                self.pnlEvent.emit(from_dict('PnL', ev[1]))
            elif kind == 'pnlSingle':
                self.pnlSingleEvent.emit(from_dict('PnLSingle', ev[1]))
            elif kind == 'tickNews':
                self.tickNewsEvent.emit(from_dict('NewsTick', ev[1]))
            elif kind == 'newsBulletin':
                self.newsBulletinEvent.emit(from_dict('NewsBulletin', ev[1]))
            elif kind == 'wshMeta':
                self.wshMetaEvent.emit(ev[1])
            elif kind == 'wshEvent':
                self.wshEvent.emit(ev[1])
            elif kind == 'error':
                contract = from_dict('Contract', ev[4]) if ev[4] else None
                self.errorEvent.emit(ev[1], ev[2], ev[3], contract)
            elif kind == 'timeout':
                self.timeoutEvent.emit(ev[1])
            elif kind == 'disconnected':
                self.disconnectedEvent.emit()
            elif kind == 'message':
                name, fields = ev[1]
                self.messageEvent.emit(callback_name(name), fields)
            elif kind == 'connected':
                pass

    def _drain(self, timeout: float = 0.0) -> bool:
        if self._core is None:
            return False
        events = self._core.poll_events(timeout)
        self._dispatch(events)
        return bool(events)

    async def _pump_loop(self) -> None:
        core = self._core
        while core is not None:
            events = await core.next_events()
            self._dispatch(events)
            if any(e[0] == 'disconnected' for e in events):
                return

    def _sync(self, result: _T) -> _T:
        """After a blocking call, deliver the events that it produced."""
        if self._pump is None:
            self._drain()
        return result

    # ---- waiting ------------------------------------------------------------

    def sleep(self, secs: float = 0.02) -> bool:
        """Wait while processing events (ib_async ``IB.sleep``)."""
        end = time.monotonic() + secs
        while True:
            left = end - time.monotonic()
            if left <= 0:
                break
            self._drain(left)
        self._drain()
        return True

    def waitOnUpdate(self, timeout: float = 0) -> bool:
        """Wait for the next network packet to be processed."""
        if self._pump is not None:
            raise RuntimeError(
                'waitOnUpdate blocks; inside asyncio use "await ib.updateEvent"'
            )
        deadline = time.monotonic() + timeout if timeout else None
        while True:
            left = None if deadline is None else max(0.0, deadline - time.monotonic())
            events = self._c.poll_events(left if left is not None else 3600.0)
            self._dispatch(events)
            if any(e[0] == 'update' for e in events):
                return True
            if any(e[0] == 'disconnected' for e in events) or not self.isConnected():
                return False
            if deadline is not None and time.monotonic() >= deadline:
                return False

    def loopUntil(
        self, condition: Callable[[], _T] | None = None, timeout: float = 0
    ) -> Iterator[_T | None]:
        """Yield the condition's value after each update until it is truthy."""
        end = time.monotonic() + timeout
        while True:
            test = condition and condition()
            if test:
                yield test
                return
            if timeout and time.monotonic() > end:
                yield test
                return
            self.waitOnUpdate(end - time.monotonic() if timeout else 0)
            yield test

    def setTimeout(self, timeout: float = 60) -> None:
        """Emit ``timeoutEvent(idleSeconds)`` once no data arrived for `timeout`
        seconds; it then stops until called again. 0 stops it."""
        self._c.set_timeout(timeout)

    # ---- state accessors ------------------------------------------------------

    def managedAccounts(self) -> list[str]:
        return self._c.managed_accounts()

    def accountValues(self, account: str = '') -> list[AccountValue]:
        return _list(AccountValue, self._c.account_values(account))

    def accountSummary(self, account: str = '') -> list[AccountValue]:
        return _list(AccountValue, self._c.account_summary(account))

    def portfolio(self, account: str = '') -> list[PortfolioItem]:
        return _list(PortfolioItem, self._c.portfolio(account))

    def positions(self, account: str = '') -> list[Position]:
        return _list(Position, self._c.positions(account))

    def pnl(self, account: str = '', modelCode: str = '') -> list[PnL]:
        return _list(PnL, self._c.pnl(account, modelCode))

    def pnlSingle(
        self, account: str = '', modelCode: str = '', conId: int = 0
    ) -> list[PnLSingle]:
        return _list(PnLSingle, self._c.pnl_single(account, modelCode, conId))

    def trades(self) -> list[Trade]:
        return [self._trade(c) for c in self._c.trades()]

    def openTrades(self) -> list[Trade]:
        return [self._trade(c) for c in self._c.open_trades()]

    def orders(self) -> list[Order]:
        return [t.order for t in self.trades()]

    def openOrders(self) -> list[Order]:
        return [t.order for t in self.openTrades()]

    def fills(self) -> list[Fill]:
        return _list(Fill, self._c.fills())

    def executions(self) -> list[Execution]:
        return [f.execution for f in self.fills()]

    def ticker(self, contract: Contract) -> Ticker | None:
        core = self._c.ticker(to_dict(contract))
        return self._ticker(core) if core is not None else None

    def tickers(self) -> list[Ticker]:
        return [self._ticker(c) for c in self._c.tickers()]

    def pendingTickers(self) -> list[Ticker]:
        return list(self._pending)

    def realtimeBars(self) -> list[BarDataList | RealTimeBarList]:
        return [
            b
            for b in self._bars.values()
            if isinstance(b, (BarDataList, RealTimeBarList))
        ]

    def newsTicks(self) -> list[NewsTick]:
        return _list(NewsTick, self._c.news_ticks())

    def newsBulletins(self) -> list[NewsBulletin]:
        return _list(NewsBulletin, self._c.news_bulletins())

    def connectionStats(self) -> ConnectionStats:
        return ConnectionStats(**self._c.connection_stats())

    # ---- orders -----------------------------------------------------------------

    def placeOrder(self, contract: Contract, order: Order) -> Trade:
        """Place a new order or modify an existing one (same orderId)."""
        core = self._c.place_order(to_dict(contract), to_dict(order))
        trade = self._trade(core)
        trade._invalidate()
        order.orderId = trade.order.orderId
        order.clientId = trade.order.clientId
        return self._sync(trade)

    def cancelOrder(
        self, order: Order, manualCancelOrderTime: str = ''
    ) -> Trade | None:
        core = self._c.cancel_order(to_dict(order), manualCancelOrderTime)
        return self._sync(self._trade(core) if core is not None else None)

    def reqGlobalCancel(self) -> None:
        self._c.req_global_cancel()

    def bracketOrder(
        self,
        action: str,
        quantity: float,
        limitPrice: float,
        takeProfitPrice: float,
        stopLossPrice: float,
        **kwargs: Any,
    ) -> BracketOrder:
        parent, tp, sl = self._c.bracket_order(
            action, quantity, limitPrice, takeProfitPrice, stopLossPrice
        )
        orders = [from_dict('Order', o) for o in (parent, tp, sl)]
        for o in orders:
            util.dataclassUpdate(o, **kwargs)
        return BracketOrder(*orders)

    @staticmethod
    def oneCancelsAll(orders: list[Order], ocaGroup: str, ocaType: int) -> list[Order]:
        for o in orders:
            o.ocaGroup = ocaGroup
            o.ocaType = ocaType
        return orders

    def whatIfOrder(self, contract: Contract, order: Order) -> OrderState:
        return self._sync(
            from_dict(
                'OrderState',
                self._c.what_if_order(to_dict(contract), to_dict(order), True),
            )
        )

    async def whatIfOrderAsync(self, contract: Contract, order: Order) -> OrderState:
        return from_dict(
            'OrderState',
            await self._c.what_if_order(to_dict(contract), to_dict(order), False),
        )

    def reqOpenOrders(self) -> list[Trade]:
        return self._sync(
            [self._trade(c) for c in self._c.req_open_orders(False, True)]
        )

    async def reqOpenOrdersAsync(self) -> list[Trade]:
        return [self._trade(c) for c in await self._c.req_open_orders(False, False)]

    def reqAllOpenOrders(self) -> list[Trade]:
        return self._sync([self._trade(c) for c in self._c.req_open_orders(True, True)])

    async def reqAllOpenOrdersAsync(self) -> list[Trade]:
        return [self._trade(c) for c in await self._c.req_open_orders(True, False)]

    def reqCompletedOrders(self, apiOnly: bool) -> list[Trade]:
        return self._sync(
            [self._trade(c) for c in self._c.req_completed_orders(apiOnly, True)]
        )

    async def reqCompletedOrdersAsync(self, apiOnly: bool) -> list[Trade]:
        return [
            self._trade(c) for c in await self._c.req_completed_orders(apiOnly, False)
        ]

    def reqAutoOpenOrders(self, autoBind: bool = True) -> None:
        self._c.req_auto_open_orders(autoBind)

    def reqExecutions(self, execFilter: ExecutionFilter | None = None) -> list[Fill]:
        f = to_dict(execFilter) if execFilter is not None else {}
        return self._sync(_list(Fill, self._c.req_executions(f, True)))

    async def reqExecutionsAsync(self, execFilter: ExecutionFilter | None = None) -> list[Fill]:
        f = to_dict(execFilter) if execFilter is not None else {}
        return _list(Fill, await self._c.req_executions(f, False))

    # ---- account ----------------------------------------------------------------

    def reqPositions(self) -> list[Position]:
        return self._sync(_list(Position, self._c.req_positions(True)))

    async def reqPositionsAsync(self) -> list[Position]:
        return _list(Position, await self._c.req_positions(False))

    def reqAccountUpdates(self, account: str = '') -> None:
        self._sync(self._c.req_account_updates(account, True))

    async def reqAccountUpdatesAsync(self, account: str = '') -> None:
        await self._c.req_account_updates(account, False)

    def reqAccountUpdatesMulti(self, account: str = '', modelCode: str = '') -> None:
        self._sync(self._c.req_account_updates_multi(account, modelCode, True))

    async def reqAccountUpdatesMultiAsync(
        self, account: str = '', modelCode: str = ''
    ) -> None:
        await self._c.req_account_updates_multi(account, modelCode, False)

    def reqAccountSummary(self) -> None:
        self._sync(self._c.req_account_summary(True))

    async def reqAccountSummaryAsync(self) -> None:
        await self._c.req_account_summary(False)

    async def accountSummaryAsync(self, account: str = '') -> list[AccountValue]:
        if not self.accountSummary(account):
            await self.reqAccountSummaryAsync()
        return self.accountSummary(account)

    def reqPnL(self, account: str, modelCode: str = '') -> LivePnL:
        return LivePnL(self._c.req_pnl(account, modelCode))

    def cancelPnL(self, account: str, modelCode: str = '') -> None:
        self._c.cancel_pnl(account, modelCode)

    def reqPnLSingle(self, account: str, modelCode: str, conId: int) -> LivePnL:
        return LivePnL(self._c.req_pnl_single(account, modelCode, conId))

    def cancelPnLSingle(self, account: str, modelCode: str, conId: int) -> None:
        self._c.cancel_pnl_single(account, modelCode, conId)

    # ---- contracts -----------------------------------------------------------------

    def reqContractDetails(self, contract: Contract) -> list[ContractDetails]:
        return self._sync(
            _list(ContractDetails, self._c.req_contract_details(to_dict(contract), True)
            )
        )

    async def reqContractDetailsAsync(self, contract: Contract) -> list[ContractDetails]:
        return _list(ContractDetails, await self._c.req_contract_details(to_dict(contract), False),
        )

    def qualifyContracts(self, *contracts: Contract) -> list[Contract]:
        """Fill in conId and other fields in place; returns the qualified ones."""
        result = self._c.qualify_contracts([to_dict(c) for c in contracts], True)
        return self._sync(
            [_qualified_into(c, q) for c, q in zip(contracts, result) if q is not None]
        )

    @overload
    async def qualifyContractsAsync(
        self, *contracts: Contract, returnAll: Literal[False] = False
    ) -> list[Contract]: ...

    @overload
    async def qualifyContractsAsync(
        self, *contracts: Contract, returnAll: Literal[True]
    ) -> list[Contract | None]: ...

    async def qualifyContractsAsync(
        self, *contracts: Contract, returnAll: bool = False
    ) -> list[Contract] | list[Contract | None]:
        """Fill in conId and other fields in place. Returns the qualified
        contracts, or with ``returnAll`` one entry per contract (``None``
        for a contract that IB does not know)."""
        result = await self._c.qualify_contracts([to_dict(c) for c in contracts], False)
        if returnAll:
            return [
                _qualified_into(c, q) if q is not None else None
                for c, q in zip(contracts, result)
            ]
        return [
            _qualified_into(c, q) for c, q in zip(contracts, result) if q is not None
        ]

    def reqMatchingSymbols(self, pattern: str) -> list[ContractDescription]:
        return self._sync(
            _list(ContractDescription, self._c.req_matching_symbols(pattern, True))
        )

    async def reqMatchingSymbolsAsync(self, pattern: str) -> list[ContractDescription]:
        return _list(ContractDescription, await self._c.req_matching_symbols(pattern, False)
        )

    def reqMarketRule(self, marketRuleId: int) -> list[PriceIncrement]:
        return self._sync(
            _list(PriceIncrement, self._c.req_market_rule(marketRuleId, True))
        )

    async def reqMarketRuleAsync(self, marketRuleId: int) -> list[PriceIncrement]:
        return _list(PriceIncrement, await self._c.req_market_rule(marketRuleId, False)
        )

    def reqSecDefOptParams(
        self,
        underlyingSymbol: str,
        futFopExchange: str,
        underlyingSecType: str,
        underlyingConId: int,
    ) -> list[OptionChain]:
        return self._sync(
            _list(OptionChain, self._c.req_sec_def_opt_params(
                    underlyingSymbol,
                    futFopExchange,
                    underlyingSecType,
                    underlyingConId,
                    True,
                ),
            )
        )

    async def reqSecDefOptParamsAsync(
        self,
        underlyingSymbol: str,
        futFopExchange: str,
        underlyingSecType: str,
        underlyingConId: int,
    ) -> list[OptionChain]:
        return _list(OptionChain, await self._c.req_sec_def_opt_params(
                underlyingSymbol,
                futFopExchange,
                underlyingSecType,
                underlyingConId,
                False,
            ),
        )

    # ---- market data ---------------------------------------------------------------

    def reqMarketDataType(self, marketDataType: int) -> None:
        """1 live, 2 frozen, 3 delayed, 4 delayed frozen."""
        self._c.req_market_data_type(marketDataType)

    def reqMktData(
        self,
        contract: Contract,
        genericTickList: str = '',
        snapshot: bool = False,
        regulatorySnapshot: bool = False,
        mktDataOptions: list[TagValue] | None = None,
    ) -> Ticker:
        core = self._c.req_mkt_data(
            to_dict(contract),
            genericTickList,
            snapshot,
            regulatorySnapshot,
            to_dict(mktDataOptions or []),
        )
        return self._ticker(core)

    def cancelMktData(self, contract: Contract) -> bool:
        return self._c.cancel_mkt_data(to_dict(contract))

    def reqTickers(
        self, *contracts: Contract, regulatorySnapshot: bool = False
    ) -> list[Ticker]:
        cores = self._c.req_tickers(
            [to_dict(c) for c in contracts], regulatorySnapshot, True
        )
        return self._sync([self._ticker(c) for c in cores])

    async def reqTickersAsync(
        self, *contracts: Contract, regulatorySnapshot: bool = False
    ) -> list[Ticker]:
        cores = await self._c.req_tickers(
            [to_dict(c) for c in contracts], regulatorySnapshot, False
        )
        return [self._ticker(c) for c in cores]

    def reqTickByTickData(
        self,
        contract: Contract,
        tickType: str,
        numberOfTicks: int = 0,
        ignoreSize: bool = False,
    ) -> Ticker:
        return self._ticker(
            self._c.req_tick_by_tick_data(
                to_dict(contract), tickType, numberOfTicks, ignoreSize
            )
        )

    def cancelTickByTickData(self, contract: Contract, tickType: str) -> bool:
        return self._c.cancel_tick_by_tick_data(to_dict(contract), tickType)

    def reqMktDepth(
        self,
        contract: Contract,
        numRows: int = 5,
        isSmartDepth: bool = False,
        mktDepthOptions: list[TagValue] | None = None,
    ) -> Ticker:
        return self._ticker(
            self._c.req_mkt_depth(
                to_dict(contract), numRows, isSmartDepth, to_dict(mktDepthOptions or [])
            )
        )

    def cancelMktDepth(self, contract: Contract, isSmartDepth: bool = False) -> bool:
        return self._c.cancel_mkt_depth(to_dict(contract), isSmartDepth)

    def reqMktDepthExchanges(self) -> list[DepthMktDataDescription]:
        return self._sync(
            _list(DepthMktDataDescription, self._c.req_mkt_depth_exchanges(True))
        )

    async def reqMktDepthExchangesAsync(self) -> list[DepthMktDataDescription]:
        return _list(DepthMktDataDescription, await self._c.req_mkt_depth_exchanges(False)
        )

    def reqSmartComponents(self, bboExchange: str) -> list[SmartComponent]:
        return self._sync(
            _list(SmartComponent, self._c.req_smart_components(bboExchange, True))
        )

    async def reqSmartComponentsAsync(self, bboExchange: str) -> list[SmartComponent]:
        return _list(SmartComponent, await self._c.req_smart_components(bboExchange, False)
        )

    # ---- historical data ------------------------------------------------------------

    def _register_bars(self, core: Any) -> BarDataList:
        bars = BarDataList(core)
        if bars.keepUpToDate:
            self._bars[bars.reqId] = bars
        return bars

    def reqHistoricalData(
        self,
        contract: Contract,
        endDateTime: DateTime,
        durationStr: str,
        barSizeSetting: str,
        whatToShow: str,
        useRTH: bool,
        formatDate: int = 1,
        keepUpToDate: bool = False,
        chartOptions: list[TagValue] | None = None,
        timeout: float = 60,
    ) -> BarDataList:
        core = self._c.req_historical_data(
            to_dict(contract),
            util.formatIBDatetime(endDateTime),
            durationStr,
            barSizeSetting,
            whatToShow,
            useRTH,
            formatDate,
            keepUpToDate,
            to_dict(chartOptions or []),
            True,
        )
        return self._sync(self._register_bars(core))

    async def reqHistoricalDataAsync(
        self,
        contract: Contract,
        endDateTime: DateTime,
        durationStr: str,
        barSizeSetting: str,
        whatToShow: str,
        useRTH: bool,
        formatDate: int = 1,
        keepUpToDate: bool = False,
        chartOptions: list[TagValue] | None = None,
        timeout: float = 60,
    ) -> BarDataList:
        coro = self._c.req_historical_data(
            to_dict(contract),
            util.formatIBDatetime(endDateTime),
            durationStr,
            barSizeSetting,
            whatToShow,
            useRTH,
            formatDate,
            keepUpToDate,
            to_dict(chartOptions or []),
            False,
        )
        core = await (asyncio.wait_for(coro, timeout) if timeout else coro)
        return self._register_bars(core)

    def cancelHistoricalData(self, bars: BarDataList) -> None:
        self._c.cancel_historical_data(bars._core)
        self._bars.pop(bars.reqId, None)

    def reqHistoricalSchedule(
        self,
        contract: Contract,
        numDays: int,
        endDateTime: DateTime = '',
        useRTH: bool = True,
    ) -> HistoricalSchedule:
        return self._sync(
            from_dict(
                'HistoricalSchedule',
                self._c.req_historical_schedule(
                    to_dict(contract),
                    numDays,
                    util.formatIBDatetime(endDateTime),
                    useRTH,
                    True,
                ),
            )
        )

    async def reqHistoricalScheduleAsync(
        self,
        contract: Contract,
        numDays: int,
        endDateTime: DateTime = '',
        useRTH: bool = True,
    ) -> HistoricalSchedule:
        return from_dict(
            'HistoricalSchedule',
            await self._c.req_historical_schedule(
                to_dict(contract),
                numDays,
                util.formatIBDatetime(endDateTime),
                useRTH,
                False,
            ),
        )

    @staticmethod
    def _ticks(whatToShow: str, ticks: list[dict[str, Any]]) -> list[HistoricalTick | HistoricalTickLast | HistoricalTickBidAsk]:
        cls = {'TRADES': 'HistoricalTickLast', 'BID_ASK': 'HistoricalTickBidAsk'}.get(
            whatToShow.upper(), 'HistoricalTick'
        )
        out = []
        for t in ticks:
            obj = from_dict(cls, t)
            obj.time = dt.datetime.fromtimestamp(obj.time, dt.UTC)
            out.append(obj)
        return out

    def reqHistoricalTicks(
        self,
        contract: Contract,
        startDateTime: DateTime,
        endDateTime: DateTime,
        numberOfTicks: int,
        whatToShow: str,
        useRth: bool,
        ignoreSize: bool = False,
        miscOptions: list[TagValue] | None = None,
    ) -> list[HistoricalTick | HistoricalTickLast | HistoricalTickBidAsk]:
        ticks = self._c.req_historical_ticks(
            to_dict(contract),
            util.formatIBDatetime(startDateTime),
            util.formatIBDatetime(endDateTime),
            numberOfTicks,
            whatToShow,
            useRth,
            ignoreSize,
            to_dict(miscOptions or []),
            True,
        )
        return self._sync(self._ticks(whatToShow, ticks))

    async def reqHistoricalTicksAsync(
        self,
        contract: Contract,
        startDateTime: DateTime,
        endDateTime: DateTime,
        numberOfTicks: int,
        whatToShow: str,
        useRth: bool,
        ignoreSize: bool = False,
        miscOptions: list[TagValue] | None = None,
    ) -> list[HistoricalTick | HistoricalTickLast | HistoricalTickBidAsk]:
        ticks = await self._c.req_historical_ticks(
            to_dict(contract),
            util.formatIBDatetime(startDateTime),
            util.formatIBDatetime(endDateTime),
            numberOfTicks,
            whatToShow,
            useRth,
            ignoreSize,
            to_dict(miscOptions or []),
            False,
        )
        return self._ticks(whatToShow, ticks)

    def reqHeadTimeStamp(
        self, contract: Contract, whatToShow: str, useRTH: bool, formatDate: int = 1
    ) -> dt.date | dt.datetime | str:
        return self._sync(
            util.parseIBDatetime(
                self._c.req_head_time_stamp(
                    to_dict(contract), whatToShow, useRTH, formatDate, True
                )
            )
        )

    async def reqHeadTimeStampAsync(
        self, contract: Contract, whatToShow: str, useRTH: bool, formatDate: int = 1
    ) -> dt.date | dt.datetime | str:
        return util.parseIBDatetime(
            await self._c.req_head_time_stamp(
                to_dict(contract), whatToShow, useRTH, formatDate, False
            )
        )

    def reqHistogramData(
        self, contract: Contract, useRTH: bool, period: str
    ) -> list[HistogramData]:
        return self._sync(
            _list(HistogramData, self._c.req_histogram_data(to_dict(contract), useRTH, period, True),
            )
        )

    async def reqHistogramDataAsync(
        self, contract: Contract, useRTH: bool, period: str
    ) -> list[HistogramData]:
        return _list(HistogramData, await self._c.req_histogram_data(to_dict(contract), useRTH, period, False),
        )

    def reqRealTimeBars(
        self,
        contract: Contract,
        barSize: int,
        whatToShow: str,
        useRTH: bool,
        realTimeBarsOptions: list[TagValue] | None = None,
    ) -> RealTimeBarList:
        bars = RealTimeBarList(
            self._c.req_real_time_bars(
                to_dict(contract),
                barSize,
                whatToShow,
                useRTH,
                to_dict(realTimeBarsOptions or []),
            )
        )
        self._bars[bars.reqId] = bars
        return bars

    def cancelRealTimeBars(self, bars: RealTimeBarList) -> None:
        self._c.cancel_real_time_bars(bars._core)
        self._bars.pop(bars.reqId, None)

    # ---- scanners --------------------------------------------------------------------

    def reqScannerData(
        self,
        subscription: ScannerSubscription,
        scannerSubscriptionOptions: list[TagValue] | None = None,
        scannerSubscriptionFilterOptions: list[TagValue] | None = None,
    ) -> list[ScanData]:
        rows = self._c.req_scanner_data(
            to_dict(subscription),
            to_dict(scannerSubscriptionOptions or []),
            to_dict(scannerSubscriptionFilterOptions or []),
            True,
        )
        return self._sync(_list(ScanData, rows))

    async def reqScannerDataAsync(
        self,
        subscription: ScannerSubscription,
        scannerSubscriptionOptions: list[TagValue] | None = None,
        scannerSubscriptionFilterOptions: list[TagValue] | None = None,
    ) -> list[ScanData]:
        rows = await self._c.req_scanner_data(
            to_dict(subscription),
            to_dict(scannerSubscriptionOptions or []),
            to_dict(scannerSubscriptionFilterOptions or []),
            False,
        )
        return _list(ScanData, rows)

    def reqScannerSubscription(
        self,
        subscription: ScannerSubscription,
        scannerSubscriptionOptions: list[TagValue] | None = None,
        scannerSubscriptionFilterOptions: list[TagValue] | None = None,
    ) -> ScanDataList:
        core = self._c.req_scanner_subscription(
            to_dict(subscription),
            to_dict(scannerSubscriptionOptions or []),
            to_dict(scannerSubscriptionFilterOptions or []),
        )
        data = ScanDataList(core)
        self._bars[data.reqId] = data
        return data

    def cancelScannerSubscription(self, dataList: ScanDataList) -> None:
        self._c.cancel_scanner_subscription(dataList._core)
        self._bars.pop(dataList.reqId, None)

    def reqScannerParameters(self) -> str:
        return self._sync(self._c.req_scanner_parameters(True))

    async def reqScannerParametersAsync(self) -> str:
        return await self._c.req_scanner_parameters(False)

    # ---- options --------------------------------------------------------------------

    def calculateImpliedVolatility(
        self,
        contract: Contract,
        optionPrice: float,
        underPrice: float,
        implVolOptions: list[TagValue] | None = None,
    ) -> OptionComputation:
        r = self._c.calculate_implied_volatility(
            to_dict(contract),
            optionPrice,
            underPrice,
            to_dict(implVolOptions or []),
            True,
        )
        return self._sync(from_dict('OptionComputation', r))

    async def calculateImpliedVolatilityAsync(
        self,
        contract: Contract,
        optionPrice: float,
        underPrice: float,
        implVolOptions: list[TagValue] | None = None,
    ) -> OptionComputation:
        return from_dict(
            'OptionComputation',
            await self._c.calculate_implied_volatility(
                to_dict(contract),
                optionPrice,
                underPrice,
                to_dict(implVolOptions or []),
                False,
            ),
        )

    def calculateOptionPrice(
        self,
        contract: Contract,
        volatility: float,
        underPrice: float,
        optPrcOptions: list[TagValue] | None = None,
    ) -> OptionComputation:
        r = self._c.calculate_option_price(
            to_dict(contract),
            volatility,
            underPrice,
            to_dict(optPrcOptions or []),
            True,
        )
        return self._sync(from_dict('OptionComputation', r))

    async def calculateOptionPriceAsync(
        self,
        contract: Contract,
        volatility: float,
        underPrice: float,
        optPrcOptions: list[TagValue] | None = None,
    ) -> OptionComputation:
        return from_dict(
            'OptionComputation',
            await self._c.calculate_option_price(
                to_dict(contract),
                volatility,
                underPrice,
                to_dict(optPrcOptions or []),
                False,
            ),
        )

    def exerciseOptions(
        self,
        contract: Contract,
        exerciseAction: int,
        exerciseQuantity: int,
        account: str,
        override: int,
    ) -> None:
        self._c.exercise_options(
            to_dict(contract), exerciseAction, exerciseQuantity, account, override
        )

    # ---- news, fundamentals, FA, WSH ------------------------------------------------

    def reqNewsProviders(self) -> list[NewsProvider]:
        return self._sync(_list(NewsProvider, self._c.req_news_providers(True)))

    async def reqNewsProvidersAsync(self) -> list[NewsProvider]:
        return _list(NewsProvider, await self._c.req_news_providers(False))

    def reqNewsArticle(
        self,
        providerCode: str,
        articleId: str,
        newsArticleOptions: list[TagValue] | None = None,
    ) -> NewsArticle:
        return self._sync(
            from_dict(
                'NewsArticle',
                self._c.req_news_article(
                    providerCode, articleId, to_dict(newsArticleOptions or []), True
                ),
            )
        )

    async def reqNewsArticleAsync(
        self,
        providerCode: str,
        articleId: str,
        newsArticleOptions: list[TagValue] | None = None,
    ) -> NewsArticle:
        return from_dict(
            'NewsArticle',
            await self._c.req_news_article(
                providerCode, articleId, to_dict(newsArticleOptions or []), False
            ),
        )

    def reqHistoricalNews(
        self,
        conId: int,
        providerCodes: str,
        startDateTime: DateTime,
        endDateTime: DateTime,
        totalResults: int,
        historicalNewsOptions: list[TagValue] | None = None,
    ) -> list[HistoricalNews]:
        r = self._c.req_historical_news(
            conId,
            providerCodes,
            util.formatIBDatetime(startDateTime),
            util.formatIBDatetime(endDateTime),
            totalResults,
            to_dict(historicalNewsOptions or []),
            True,
        )
        return self._sync(_list(HistoricalNews, r))

    async def reqHistoricalNewsAsync(
        self,
        conId: int,
        providerCodes: str,
        startDateTime: DateTime,
        endDateTime: DateTime,
        totalResults: int,
        historicalNewsOptions: list[TagValue] | None = None,
    ) -> list[HistoricalNews]:
        r = await self._c.req_historical_news(
            conId,
            providerCodes,
            util.formatIBDatetime(startDateTime),
            util.formatIBDatetime(endDateTime),
            totalResults,
            to_dict(historicalNewsOptions or []),
            False,
        )
        return _list(HistoricalNews, r)

    def reqNewsBulletins(self, allMessages: bool) -> None:
        self._c.req_news_bulletins(allMessages)

    def cancelNewsBulletins(self) -> None:
        self._c.cancel_news_bulletins()

    def reqFundamentalData(
        self,
        contract: Contract,
        reportType: str,
        fundamentalDataOptions: list[TagValue] | None = None,
    ) -> str:
        return self._sync(
            self._c.req_fundamental_data(
                to_dict(contract),
                reportType,
                to_dict(fundamentalDataOptions or []),
                True,
            )
        )

    async def reqFundamentalDataAsync(
        self,
        contract: Contract,
        reportType: str,
        fundamentalDataOptions: list[TagValue] | None = None,
    ) -> str:
        return await self._c.req_fundamental_data(
            to_dict(contract), reportType, to_dict(fundamentalDataOptions or []), False
        )

    def requestFA(self, faDataType: int) -> str:
        return self._sync(self._c.request_fa(faDataType, True))

    async def requestFAAsync(self, faDataType: int) -> str:
        return await self._c.request_fa(faDataType, False)

    def replaceFA(self, faDataType: int, xml: str) -> str:
        return self._sync(self._c.replace_fa(faDataType, xml, True))

    def reqWshMetaData(self) -> None:
        """Subscribe to Wall Street Horizon metadata; it arrives as ``wshMetaEvent``."""
        if self._wshMetaReqId:
            _logger.warning('reqWshMetaData already active')
            return
        self._wshMetaReqId = self._c.next_req_id()
        self.client.reqWshMetaData(self._wshMetaReqId)

    def cancelWshMetaData(self) -> None:
        if not self._wshMetaReqId:
            _logger.warning('reqWshMetaData not active')
            return
        self.client.cancelWshMetaData(self._wshMetaReqId)
        self._wshMetaReqId = 0

    def reqWshEventData(self, data: WshEventData) -> None:
        """Subscribe to Wall Street Horizon events; they arrive as ``wshEvent``."""
        if self._wshEventReqId:
            _logger.warning('reqWshEventData already active')
            return
        self._wshEventReqId = self._c.next_req_id()
        self.client.reqWshEventData(self._wshEventReqId, data)

    def cancelWshEventData(self) -> None:
        if not self._wshEventReqId:
            _logger.warning('reqWshEventData not active')
            return
        self.client.cancelWshEventData(self._wshEventReqId)
        self._wshEventReqId = 0

    def getWshMetaData(self) -> str:
        return self._sync(self._c.get_wsh_meta_data(True))

    async def getWshMetaDataAsync(self) -> str:
        return await self._c.get_wsh_meta_data(False)

    def getWshEventData(self, data: WshEventData) -> str:
        return self._sync(self._c.get_wsh_event_data(to_dict(data), True))

    async def getWshEventDataAsync(self, data: WshEventData) -> str:
        return await self._c.get_wsh_event_data(to_dict(data), False)

    def reqConfig(self) -> dict[str, Any]:
        """TWS API configuration (server 219+), IB's ``ConfigResponse`` as a dict."""
        return self._sync(self._c.req_config(True))

    async def reqConfigAsync(self) -> dict[str, Any]:
        return await self._c.req_config(False)

    def updateConfig(self, request: dict[str, Any]) -> dict[str, Any]:
        """Change the TWS API configuration (server 221+). `request` is IB's
        ``UpdateConfigRequest`` as a dict without ``reqId``, for example
        ``{'api': {'settings': {'readOnlyApi': False}}}``."""
        return self._sync(self._c.update_config(request, True))

    async def updateConfigAsync(self, request: dict[str, Any]) -> dict[str, Any]:
        return await self._c.update_config(request, False)

    def reqUserInfo(self) -> str:
        return self._sync(self._c.req_user_info(True))

    async def reqUserInfoAsync(self) -> str:
        return await self._c.req_user_info(False)

    def reqCurrentTime(self) -> dt.datetime:
        return self._sync(
            dt.datetime.fromtimestamp(self._c.req_current_time(True), dt.UTC)
        )

    async def reqCurrentTimeAsync(self) -> dt.datetime:
        return dt.datetime.fromtimestamp(
            await self._c.req_current_time(False), dt.UTC
        )

    def reqCurrentTimeInMillis(self) -> int:
        return self._sync(self._c.req_current_time_in_millis(True))

    async def reqCurrentTimeInMillisAsync(self) -> int:
        return await self._c.req_current_time_in_millis(False)

    def reqFamilyCodes(self) -> list[FamilyCode]:
        return self._sync(_list(FamilyCode, self._c.req_family_codes(True)))

    async def reqFamilyCodesAsync(self) -> list[FamilyCode]:
        return _list(FamilyCode, await self._c.req_family_codes(False))

    def reqSoftDollarTiers(self) -> list[SoftDollarTier]:
        return self._sync(_list(SoftDollarTier, self._c.req_soft_dollar_tiers(True)))

    async def reqSoftDollarTiersAsync(self) -> list[SoftDollarTier]:
        return _list(SoftDollarTier, await self._c.req_soft_dollar_tiers(False))
