"""Value objects and live containers (ib_async ``objects.py``)."""

from __future__ import annotations

import datetime as dt
import math
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, NamedTuple

from . import util
from ._types import *
from ._types import BarData, RealTimeBar, TickAttribBidAsk, TickAttribLast
from .event import Event

if TYPE_CHECKING:
    from ._ibkit import BarsCore, PnLCore, PnLSingleCore, RealTimeBarsCore, ScanCore
    from .contract import Contract


@dataclass
class IBDefaults:
    """Values `IB(defaults=...)` uses when TWS sends no data."""

    #: Price and size TWS uses for "no quote" (-1 and 0) are replaced by these.
    emptyPrice: Any = -1
    emptySize: Any = 0
    #: Value of ticker fields that have not been received.
    unset: Any = math.nan
    #: Time zone of trade log entries.
    timezone: dt.tzinfo = dt.UTC


class ConnectionStats(NamedTuple):
    startTime: float
    duration: float
    numBytesRecv: int
    numBytesSent: int
    numMsgRecv: int
    numMsgSent: int


class DynamicObject:
    def __init__(self, **kwargs: Any) -> None:
        self.__dict__.update(kwargs)

    def __repr__(self) -> str:
        kwargs = ', '.join(f'{k}={v!r}' for k, v in self.__dict__.items())
        return f'{type(self).__name__}({kwargs})'


class FundamentalRatios(DynamicObject):
    """Generic tick 258 values, one attribute per ratio tag (for example ``PEEXCLXOR``)."""


class TickByTickAllLast(NamedTuple):
    tickType: int
    time: dt.datetime
    price: float
    size: float
    tickAttribLast: TickAttribLast
    exchange: str
    specialConditions: str


class TickByTickBidAsk(NamedTuple):
    time: dt.datetime
    bidPrice: float
    askPrice: float
    bidSize: float
    askSize: float
    tickAttribBidAsk: TickAttribBidAsk


class TickByTickMidPoint(NamedTuple):
    time: dt.datetime
    midPoint: float


def tick_by_tick(
    d: dict[str, Any],
) -> TickByTickAllLast | TickByTickBidAsk | TickByTickMidPoint:
    t = dt.datetime.fromtimestamp(d['time'], dt.UTC)
    kind = d.get('kind')
    if kind == 'allLast':
        return TickByTickAllLast(
            d['tickType'],
            t,
            d['price'],
            d['size'],
            util.from_dict('TickAttribLast', d['tickAttribLast']),
            d['exchange'],
            d['specialConditions'],
        )
    if kind == 'bidAsk':
        return TickByTickBidAsk(
            t,
            d['bidPrice'],
            d['askPrice'],
            d['bidSize'],
            d['askSize'],
            util.from_dict('TickAttribBidAsk', d['tickAttribBidAsk']),
        )
    return TickByTickMidPoint(t, d['midPoint'])


def bar(d: dict[str, Any]) -> BarData:
    b = util.from_dict('BarData', d)
    b.date = util.parseIBDatetime(b.date)
    return b


def realtime_bar(d: dict[str, Any]) -> RealTimeBar:
    b = util.from_dict('RealTimeBar', d)
    b.time = dt.datetime.fromtimestamp(b.time, dt.UTC)
    return b


class _LiveList(list[Any]):
    """A list the Rust core keeps filling; compared by identity like ib_async."""

    def __init__(self, core: Any) -> None:
        super().__init__()
        self._core = core
        meta = core.snapshot()
        self.reqId: int = meta['reqId']
        self.updateEvent: Any = Event('updateEvent')
        self._load(meta)
        self._refresh()

    def _load(self, meta: dict[str, Any]) -> None:
        pass

    def _refresh(self) -> None:
        raise NotImplementedError

    def __eq__(self, other: object) -> bool:
        return self is other

    # Hashable by identity, as in ib_async (lists of live data go in sets).
    __hash__ = object.__hash__  # pyright: ignore[reportAssignmentType]


class BarDataList(_LiveList):
    """Historical bars; with ``keepUpToDate`` it keeps updating and emits
    ``updateEvent(bars, hasNewBar)``."""

    updateEvent: Event[BarDataList, bool]

    _core: BarsCore

    def _load(self, meta: dict[str, Any]) -> None:
        self.contract: Contract = util.from_dict('Contract', meta['contract'])
        self.endDateTime: Any = meta['endDateTime']
        self.durationStr: str = meta['durationStr']
        self.barSizeSetting: str = meta['barSizeSetting']
        self.whatToShow: str = meta['whatToShow']
        self.useRTH: bool = meta['useRth']
        self.formatDate: int = meta['formatDate']
        self.keepUpToDate: bool = meta['keepUpToDate']
        self.chartOptions: list[Any] = meta['chartOptions']

    def _refresh(self) -> None:
        self[:] = [bar(b) for b in self._core.bars()]


class RealTimeBarList(_LiveList):
    """Five-second bars; emits ``updateEvent(bars, hasNewBar)``."""

    updateEvent: Event[RealTimeBarList, bool]

    _core: RealTimeBarsCore

    def _load(self, meta: dict[str, Any]) -> None:
        self.contract: Contract = util.from_dict('Contract', meta['contract'])
        self.barSize: int = meta['barSize']
        self.whatToShow: str = meta['whatToShow']
        self.useRTH: bool = meta['useRth']
        self.realTimeBarsOptions: list[Any] = meta['realTimeBarsOptions']

    def _refresh(self) -> None:
        self[:] = [realtime_bar(b) for b in self._core.bars()]


class ScanDataList(_LiveList):
    """Scanner results; emits ``updateEvent(list)`` on new results."""

    updateEvent: Event[ScanDataList]

    _core: ScanCore

    def _load(self, meta: dict[str, Any]) -> None:
        self.subscription: Any = util.from_dict(
            'ScannerSubscription', meta['subscription']
        )

    def _refresh(self) -> None:
        self[:] = [util.from_dict('ScanData', r) for r in self._core.snapshot()['rows']]


class LivePnL:
    """Live P&L (ib_async ``PnL``); fields read from the Rust core on access."""

    def __init__(self, core: PnLCore | PnLSingleCore) -> None:
        self._core = core

    def __getattr__(self, name: str) -> Any:
        snap = self._core.snapshot()
        if name in snap:
            return snap[name]
        raise AttributeError(name)

    def __repr__(self) -> str:
        return f'{type(self).__name__}({self._core.snapshot()})'
