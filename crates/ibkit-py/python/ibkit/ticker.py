"""Live market data (ib_async ``Ticker``)."""

from __future__ import annotations

import datetime as dt
import math
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

from . import objects, util
from .event import Event, Op

if TYPE_CHECKING:
    from ._ibkit import TickerCore
    from ._types import (
        Dividends,
        DOMLevel,
        EfpData,
        MktDepthData,
        OptionComputation,
        TickData,
    )
    from .contract import Contract

_TIME_FIELDS = frozenset({'time', 'lastTimestamp', 'rtTime', 'delayedLastTimestamp'})
_GREEKS = frozenset(
    {'bidGreeks', 'askGreeks', 'lastGreeks', 'modelGreeks', 'custGreeks'}
)
_EFP = frozenset(
    {'bidEfp', 'askEfp', 'lastEfp', 'openEfp', 'highEfp', 'lowEfp', 'closeEfp'}
)


class Ticker:
    """Latest market data of one contract. Every attribute is read from the
    Rust core on access, so it is always current. ``updateEvent(ticker)`` fires
    after each network packet that changed it."""

    __slots__ = ('_contract', '_core', '_unset', 'updateEvent')

    # The fields, read from the Rust core by __getattr__. A number not yet
    # received reads as the unset value (NaN by default).
    time: dt.datetime | None
    marketDataType: int
    minTick: float
    bidExchange: str
    askExchange: str
    lastExchange: str
    lastTimestamp: dt.datetime | None
    rtTime: dt.datetime | None
    lastRegTime: str
    optionBidExch: str
    optionAskExch: str
    delayedLastTimestamp: dt.datetime | None
    reutersMutualFunds: str
    socialMarketAnalytics: str
    oddLotBidExchange: str
    oddLotAskExchange: str
    bboExchange: str
    snapshotPermissions: int
    dividends: Dividends | None
    fundamentalRatios: objects.FundamentalRatios | None
    ticks: list[TickData]
    tickByTicks: list[objects.TickByTickAllLast | objects.TickByTickBidAsk | objects.TickByTickMidPoint]
    domBids: list[DOMLevel]
    domAsks: list[DOMLevel]
    domTicks: list[MktDepthData]
    bidGreeks: OptionComputation | None
    askGreeks: OptionComputation | None
    lastGreeks: OptionComputation | None
    modelGreeks: OptionComputation | None
    custGreeks: OptionComputation | None
    bidEfp: EfpData | None
    askEfp: EfpData | None
    lastEfp: EfpData | None
    openEfp: EfpData | None
    highEfp: EfpData | None
    lowEfp: EfpData | None
    closeEfp: EfpData | None
    bid: float
    bidSize: float
    ask: float
    askSize: float
    last: float
    lastSize: float
    prevBid: float
    prevBidSize: float
    prevAsk: float
    prevAskSize: float
    prevLast: float
    prevLastSize: float
    volume: float
    open: float
    high: float
    low: float
    close: float
    vwap: float
    low13week: float
    high13week: float
    low26week: float
    high26week: float
    low52week: float
    high52week: float
    bidYield: float
    askYield: float
    lastYield: float
    markPrice: float
    halted: float
    rtHistVolatility: float
    rtVolume: float
    rtTradeVolume: float
    avVolume: float
    tradeCount: float
    tradeRate: float
    volumeRate: float
    volumeRate3Min: float
    volumeRate5Min: float
    volumeRate10Min: float
    shortable: float
    shortableShares: float
    indexFuturePremium: float
    futuresOpenInterest: float
    putOpenInterest: float
    callOpenInterest: float
    putVolume: float
    callVolume: float
    avOptionVolume: float
    histVolatility: float
    impliedVolatility: float
    openInterest: float
    lastRthTrade: float
    bondFactorMultiplier: float
    creditmanMarkPrice: float
    creditmanSlowMarkPrice: float
    delayedHalted: float
    etfNavClose: float
    etfNavPriorClose: float
    etfNavBid: float
    etfNavAsk: float
    etfNavLast: float
    etfFrozenNavLast: float
    etfNavHigh: float
    etfNavLow: float
    estimatedIpoMidpoint: float
    finalIpoLast: float
    auctionVolume: float
    auctionPrice: float
    auctionImbalance: float
    regulatoryImbalance: float
    oddLotBid: float
    oddLotAsk: float
    oddLotBidSize: float
    oddLotAskSize: float

    def __init__(self, core: TickerCore, unset: Any = math.nan) -> None:
        self._core = core
        self._contract: Any = None
        self._unset = unset
        self.updateEvent: TickerUpdateEvent = TickerUpdateEvent('updateEvent')

    @property
    def contract(self) -> Contract:
        if self._contract is None:
            self._contract = util.from_dict('Contract', self._core.field('contract'))
        return self._contract

    def __getattr__(self, name: str) -> Any:
        if name.startswith('_'):
            raise AttributeError(name)
        value = self._core.field(name)
        if name in _TIME_FIELDS:
            return (
                None
                if value is None
                else dt.datetime.fromtimestamp(value, dt.UTC)
            )
        if name == 'ticks':
            return [util.from_dict('TickData', t) for t in value]
        if name == 'tickByTicks':
            return [objects.tick_by_tick(t) for t in value]
        if name in ('domBids', 'domAsks'):
            return [util.from_dict('DOMLevel', d) for d in value]
        if name == 'domTicks':
            return [util.from_dict('MktDepthData', d) for d in value]
        if name in _GREEKS:
            return util.from_dict('OptionComputation', value)
        if name in _EFP:
            return util.from_dict('EfpData', value)
        if name == 'dividends':
            return util.from_dict('Dividends', value)
        if name == 'fundamentalRatios':
            return None if value is None else objects.FundamentalRatios(**value)
        if isinstance(value, float) and math.isnan(value):
            return self._unset
        return value

    def hasBidAsk(self) -> bool:
        return self._core.has_bid_ask()

    def midpoint(self) -> float:
        return self._core.midpoint()

    def marketPrice(self) -> float:
        return self._core.market_price()

    @staticmethod
    def isUnset(value: float) -> bool:
        return value is None or (isinstance(value, float) and math.isnan(value))

    def __repr__(self) -> str:
        c = self.contract
        return f'Ticker({c.symbol!r}, bid={self.bid}, ask={self.ask}, last={self.last})'

    def __hash__(self) -> int:
        return self._core.id()

    def __eq__(self, other: object) -> bool:
        return isinstance(other, Ticker) and other._core.id() == self._core.id()


class TickerUpdateEvent(Event[Ticker]):
    """``Ticker.updateEvent`` with ib_async's tick filters."""

    __slots__ = ()

    def trades(self) -> Tickfilter:
        return Tickfilter((4, 5, 48, 68, 71), self)

    def bids(self) -> Tickfilter:
        return Tickfilter((0, 1, 66, 69), self)

    def asks(self) -> Tickfilter:
        return Tickfilter((2, 3, 67, 70), self)

    def bidasks(self) -> Tickfilter:
        return Tickfilter((0, 1, 66, 69, 2, 3, 67, 70), self)

    def midpoints(self) -> Tickfilter:
        return Midpoints((), self)


class Tickfilter(Op[dt.datetime | None, float, float]):
    """Emits ``(time, price, size)`` for each tick of the given tick types."""

    __slots__ = ('_tickTypes',)

    def __init__(self, tickTypes: Any, source: Event[*tuple[Any, ...]] | None = None) -> None:
        super().__init__(source)
        self._tickTypes: set[int] = set(tickTypes)

    def on_source(self, ticker: Ticker) -> None:
        for t in ticker.ticks:
            if t.tickType in self._tickTypes:
                self.emit(t.time, t.price, t.size)

    def timebars(self, timer: Event[Any]) -> TimeBars:
        """Bars that start each time `timer` emits a time; emits each completed Bar."""
        return TimeBars(timer, self)

    def tickbars(self, count: int) -> TickBars:
        """Bars of `count` ticks each; emits the BarList when a bar completes."""
        return TickBars(count, self)

    def volumebars(self, volume: int) -> VolumeBars:
        """Bars of at least `volume` each; emits the BarList when a bar completes."""
        return VolumeBars(volume, self)


class Midpoints(Tickfilter):
    __slots__ = ()

    def on_source(self, ticker: Ticker) -> None:
        if ticker.ticks:
            self.emit(ticker.time, ticker.midpoint(), 0)


@dataclass
class Bar:
    time: dt.datetime | None
    open: float = math.nan
    high: float = math.nan
    low: float = math.nan
    close: float = math.nan
    volume: float = 0
    count: int = 0


class BarList(list[Bar]):
    def __init__(self, *args: Any) -> None:
        super().__init__(*args)
        #: ``(bars, hasNewBar)``.
        self.updateEvent: Event[BarList, bool] = Event('updateEvent')

    def __eq__(self, other: object) -> bool:
        return self is other

    # Hashable by identity, as in ib_async (lists of live data go in sets).
    __hash__ = object.__hash__  # pyright: ignore[reportAssignmentType]


def _add(bar: Bar, price: float, size: float) -> None:
    bar.high = max(bar.high, price)
    bar.low = min(bar.low, price)
    bar.close = price
    bar.volume += size
    bar.count += 1


class TimeBars(Op[Bar]):
    __slots__ = ('_timer', 'bars')

    def __init__(self, timer: Event[Any], source: Event[*tuple[Any, ...]] | None = None) -> None:
        super().__init__(source)
        self._timer = timer
        self.bars: BarList = BarList()
        timer.connect(self._on_timer)

    def on_source(self, time: Any, price: float, size: float) -> None:
        if not self.bars:
            return
        bar = self.bars[-1]
        if math.isnan(bar.open):
            bar.open = bar.high = bar.low = price
        _add(bar, price, size)
        self.bars.updateEvent.emit(self.bars, False)

    def _on_timer(self, time: Any) -> None:
        if self.bars:
            bar = self.bars[-1]
            if math.isnan(bar.close) and len(self.bars) > 1:
                bar.open = bar.high = bar.low = bar.close = self.bars[-2].close
            self.bars.updateEvent.emit(self.bars, True)
            self.emit(bar)
        self.bars.append(Bar(time))


class TickBars(Op[BarList]):
    __slots__ = ('_count', 'bars')

    def __init__(self, count: int, source: Event[*tuple[Any, ...]] | None = None) -> None:
        super().__init__(source)
        self._count = count
        self.bars: BarList = BarList()

    def on_source(self, time: Any, price: float, size: float) -> None:
        if not self.bars or self.bars[-1].count == self._count:
            bar = Bar(time, price, price, price, price, size, 1)
            self.bars.append(bar)
        else:
            bar = self.bars[-1]
            _add(bar, price, size)
        if bar.count == self._count:
            self.bars.updateEvent.emit(self.bars, True)
            self.emit(self.bars)


class VolumeBars(Op[BarList]):
    __slots__ = ('_volume', 'bars')

    def __init__(self, volume: int, source: Event[*tuple[Any, ...]] | None = None) -> None:
        super().__init__(source)
        self._volume = volume
        self.bars: BarList = BarList()

    def on_source(self, time: Any, price: float, size: float) -> None:
        if not self.bars or self.bars[-1].volume >= self._volume:
            bar = Bar(time, price, price, price, price, size, 1)
            self.bars.append(bar)
        else:
            bar = self.bars[-1]
            _add(bar, price, size)
        if bar.volume >= self._volume:
            self.bars.updateEvent.emit(self.bars, True)
            self.emit(self.bars)
