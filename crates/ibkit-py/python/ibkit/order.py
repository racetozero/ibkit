"""Orders, order conditions and trades (ib_async ``order.py``)."""

from __future__ import annotations

import dataclasses
import datetime as dt
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, ClassVar, NamedTuple

from . import _types, util
from ._types import (
    UNSET_DOUBLE,
    CommissionReport,
    Fill,
    OrderAllocation,
    OrderComboLeg,
    SoftDollarTier,
)
from .event import Event

if TYPE_CHECKING:
    from ._ibkit import TradeCore

__all__ = [
    'BracketOrder',
    'ExecutionCondition',
    'LimitOrder',
    'MarginCondition',
    'MarketOrder',
    'Order',
    'OrderAllocation',
    'OrderComboLeg',
    'OrderCondition',
    'OrderState',
    'OrderStateNumeric',
    'OrderStatus',
    'PercentChangeCondition',
    'PriceCondition',
    'SoftDollarTier',
    'StopLimitOrder',
    'StopOrder',
    'TimeCondition',
    'Trade',
    'VolumeCondition',
]

Order = _types.Order


class LimitOrder(Order):
    def __init__(
        self, action: str, totalQuantity: float, lmtPrice: float, **kwargs: Any
    ) -> None:
        Order.__init__(
            self,
            orderType='LMT',
            action=action,
            totalQuantity=totalQuantity,
            lmtPrice=lmtPrice,
            **kwargs,
        )


class MarketOrder(Order):
    def __init__(self, action: str, totalQuantity: float, **kwargs: Any) -> None:
        Order.__init__(
            self, orderType='MKT', action=action, totalQuantity=totalQuantity, **kwargs
        )


class StopOrder(Order):
    def __init__(
        self, action: str, totalQuantity: float, stopPrice: float, **kwargs: Any
    ) -> None:
        Order.__init__(
            self,
            orderType='STP',
            action=action,
            totalQuantity=totalQuantity,
            auxPrice=stopPrice,
            **kwargs,
        )


class StopLimitOrder(Order):
    def __init__(
        self,
        action: str,
        totalQuantity: float,
        lmtPrice: float,
        stopPrice: float,
        **kwargs: Any,
    ) -> None:
        Order.__init__(
            self,
            orderType='STP LMT',
            action=action,
            totalQuantity=totalQuantity,
            lmtPrice=lmtPrice,
            auxPrice=stopPrice,
            **kwargs,
        )


class OrderStatus(_types.OrderStatus):
    PendingSubmit: ClassVar[str] = 'PendingSubmit'
    PendingCancel: ClassVar[str] = 'PendingCancel'
    PreSubmitted: ClassVar[str] = 'PreSubmitted'
    Submitted: ClassVar[str] = 'Submitted'
    ApiPending: ClassVar[str] = 'ApiPending'
    ApiCancelled: ClassVar[str] = 'ApiCancelled'
    ApiUpdate: ClassVar[str] = 'ApiUpdate'
    Cancelled: ClassVar[str] = 'Cancelled'
    Filled: ClassVar[str] = 'Filled'
    Inactive: ClassVar[str] = 'Inactive'
    ValidationError: ClassVar[str] = 'ValidationError'
    DoneStates: ClassVar[frozenset[str]] = frozenset(
        ['Filled', 'Cancelled', 'ApiCancelled', 'Inactive']
    )
    ActiveStates: ClassVar[frozenset[str]] = frozenset(
        [
            'PendingSubmit',
            'ApiPending',
            'PreSubmitted',
            'Submitted',
            'ValidationError',
            'ApiUpdate',
        ]
    )
    WaitingStates: ClassVar[frozenset[str]] = frozenset(
        ['PendingSubmit', 'ApiPending', 'PreSubmitted']
    )
    WorkingStates: ClassVar[frozenset[str]] = frozenset(
        ['Submitted', 'ValidationError', 'ApiUpdate']
    )


util.CLASSES['OrderStatus'] = OrderStatus


class OrderState(_types.OrderState):
    def transform(self, transformer: Any) -> Any:
        """Copy with `transformer` applied to the margin and commission fields."""
        t = transformer
        return dataclasses.replace(
            self,
            initMarginBefore=t(self.initMarginBefore),
            maintMarginBefore=t(self.maintMarginBefore),
            equityWithLoanBefore=t(self.equityWithLoanBefore),
            initMarginChange=t(self.initMarginChange),
            maintMarginChange=t(self.maintMarginChange),
            equityWithLoanChange=t(self.equityWithLoanChange),
            initMarginAfter=t(self.initMarginAfter),
            maintMarginAfter=t(self.maintMarginAfter),
            equityWithLoanAfter=t(self.equityWithLoanAfter),
            commission=t(self.commission),
            minCommission=t(self.minCommission),
            maxCommission=t(self.maxCommission),
        )

    def numeric(self, digits: int = 2) -> OrderStateNumeric:
        """Copy with the margin strings as rounded floats (None when unset or empty)."""

        def to_float(value: Any) -> float | None:
            try:
                f = float(value)
            except (TypeError, ValueError):
                return None
            return None if f == UNSET_DOUBLE else round(f, digits)

        return self.transform(to_float)

    def formatted(self, digits: int = 2) -> Any:
        """Copy with the margin fields as strings such as '300,000.21'."""
        return self.numeric(8).transform(
            lambda x: f'{x:,.{digits}f}' if x is not None else None
        )


class OrderStateNumeric(OrderState):
    """Type of `OrderState.numeric()`: the margin fields are floats or None."""


util.CLASSES['OrderState'] = OrderState


class OrderCondition(_types.OrderCondition):
    conjunction: str

    @staticmethod
    def createClass(condType: int) -> type[OrderCondition]:
        return _CONDITIONS[condType]

    @staticmethod
    def from_dict(d: dict[str, Any]) -> OrderCondition:
        cls = _CONDITIONS[int(d['condType'])]
        names = {f.name for f in dataclasses.fields(cls)}
        values: dict[str, Any] = {k: v for k, v in d.items() if k in names}
        values['condType'] = int(d['condType'])
        return cls(**values)

    def And(self) -> OrderCondition:
        self.conjunction = 'a'
        return self

    def Or(self) -> OrderCondition:
        self.conjunction = 'o'
        return self


@dataclass
class PriceCondition(OrderCondition):
    condType: int = 1
    conjunction: str = 'a'
    isMore: bool = True
    price: float = 0.0
    conId: int = 0
    exch: str = ''
    triggerMethod: int = 0


@dataclass
class TimeCondition(OrderCondition):
    condType: int = 3
    conjunction: str = 'a'
    isMore: bool = True
    time: str = ''


@dataclass
class MarginCondition(OrderCondition):
    condType: int = 4
    conjunction: str = 'a'
    isMore: bool = True
    percent: int = 0


@dataclass
class ExecutionCondition(OrderCondition):
    condType: int = 5
    conjunction: str = 'a'
    secType: str = ''
    exch: str = ''
    symbol: str = ''


@dataclass
class VolumeCondition(OrderCondition):
    condType: int = 6
    conjunction: str = 'a'
    isMore: bool = True
    volume: int = 0
    conId: int = 0
    exch: str = ''


@dataclass
class PercentChangeCondition(OrderCondition):
    condType: int = 7
    conjunction: str = 'a'
    isMore: bool = True
    changePercent: float = 0.0
    conId: int = 0
    exch: str = ''


_CONDITIONS: dict[int, type[OrderCondition]] = {
    1: PriceCondition,
    3: TimeCondition,
    4: MarginCondition,
    5: ExecutionCondition,
    6: VolumeCondition,
    7: PercentChangeCondition,
}


class BracketOrder(NamedTuple):
    parent: Order
    takeProfit: Order
    stopLoss: Order


class Trade:
    """Live view of an order: contract, order, status, fills and log.

    The fields are read from the Rust core on access, so they are always
    current. Events: ``statusEvent``, ``modifyEvent``, ``fillEvent``,
    ``commissionReportEvent``, ``filledEvent``, ``cancelEvent``,
    ``cancelledEvent``."""

    statusEvent: Event[Trade]
    modifyEvent: Event[Trade]
    fillEvent: Event[Trade, Fill]
    commissionReportEvent: Event[Trade, Fill, CommissionReport]
    filledEvent: Event[Trade]
    cancelEvent: Event[Trade]
    cancelledEvent: Event[Trade]

    events: ClassVar[tuple[str, ...]] = (
        'statusEvent',
        'modifyEvent',
        'fillEvent',
        'commissionReportEvent',
        'filledEvent',
        'cancelEvent',
        'cancelledEvent',
    )

    def __init__(self, core: TradeCore, timezone: dt.tzinfo = dt.UTC) -> None:
        self._core = core
        self._timezone = timezone
        self._cache: dict[str, Any] | None = None
        self._objects: dict[str, Any] = {}
        for name in self.events:
            setattr(self, name, Event(name))

    def _invalidate(self) -> None:
        self._cache = None
        self._objects.clear()

    def _snap(self) -> dict[str, Any]:
        snap = self._cache
        if snap is None:
            snap = self._cache = self._core.snapshot()
        return snap

    def _obj(self, key: str, build: Any) -> Any:
        if key not in self._objects:
            self._objects[key] = build(self._snap()[key])
        return self._objects[key]

    @property
    def contract(self) -> Any:
        return self._obj('contract', lambda d: util.from_dict('Contract', d))

    @property
    def order(self) -> Order:
        return self._obj('order', lambda d: util.from_dict('Order', d))

    @property
    def orderStatus(self) -> OrderStatus:
        return self._obj('orderStatus', lambda d: util.from_dict('OrderStatus', d))

    @property
    def fills(self) -> list[Any]:
        return self._obj('fills', lambda d: [util.from_dict('Fill', f) for f in d])

    @property
    def log(self) -> list[Any]:
        return self._obj(
            'log',
            lambda d: [
                dataclasses.replace(
                    e, time=e.time and e.time.astimezone(self._timezone)
                )
                for e in (util.from_dict('TradeLogEntry', e) for e in d)
            ],
        )

    @property
    def advancedError(self) -> str:
        return self._snap()['advancedError']

    def isWaiting(self) -> bool:
        return self._core.is_waiting()

    def isWorking(self) -> bool:
        return self._core.is_working()

    def isActive(self) -> bool:
        return self._core.is_active()

    def isDone(self) -> bool:
        return self._core.is_done()

    def filled(self) -> float:
        return self._core.filled()

    def remaining(self) -> float:
        return self._core.remaining()

    def __repr__(self) -> str:
        c = self.contract
        return f'Trade(contract={type(c).__name__}({c.symbol!r}, conId={c.conId}), orderId={self.order.orderId}, status={self._core.status()!r})'
