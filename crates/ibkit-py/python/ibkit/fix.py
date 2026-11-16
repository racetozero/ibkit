"""FIX 4.2 initiator session (TCP or TLS) and IB order message builders on the Rust core.

The builders follow the IBKR FIX Implementation Manual: HandlInst 2, Account
(tag 1) on orders, cancels and status requests, IB's order type codes and
custom tags. IB supports the message types D, F, G, H, AB, AC and J (and
sends 8, 9, B and P); it rejects others.
"""

from __future__ import annotations

from collections.abc import AsyncIterator
from typing import TYPE_CHECKING, Any, Literal

from . import _ibkit
from .util import to_dict

if TYPE_CHECKING:
    from .contract import Contract
    from .order import Order


#: A FIX message as ``(tag, value)`` pairs.
Fields = list[tuple[int, str]]
#: One session event: ``('app', msgType, fields)``, ``('session', text)``
#: or ``('disconnected', reason)``.
FixEvent = (
    tuple[Literal['app'], str, Fields]
    | tuple[Literal['session'], str]
    | tuple[Literal['disconnected'], str]
)


class FixSession:
    """A FIX session. ``poll()`` (or ``await pollAsync()``) returns the next
    ``FixEvent``, or ``None`` on timeout; ``async for event in session``
    reads events until the session disconnects."""

    def __init__(self, core: _ibkit.FixCore) -> None:
        self._core = core

    @classmethod
    def connect(
        cls,
        host: str,
        port: int,
        senderCompId: str,
        targetCompId: str = 'IB',
        heartbeatInterval: int = 30,
        resetOnLogon: bool = False,
        seqFile: str | None = None,
        logonFields: list[tuple[int, str]] | None = None,
        tlsCaFile: str | None = None,
        tlsServerName: str | None = None,
        maxLatency: float | None = 120,
        compression: bool = False,
    ) -> FixSession:
        """Connect and log on. With ``seqFile`` the sequence numbers and sent
        messages survive restarts. With ``tlsCaFile`` (PEM of the CA or
        server certificate) the connection uses TLS; the certificate must name
        ``tlsServerName`` (default ``host``). Incoming SendingTime more than
        ``maxLatency`` seconds off the local clock ends the session. With
        ``compression`` outgoing messages are sent as FIXCOMP blocks (IB
        enables compression per session on request)."""
        core = _ibkit.FixCore.connect(
            host,
            port,
            senderCompId,
            targetCompId,
            heartbeatInterval,
            resetOnLogon,
            seqFile,
            logonFields or [],
            tlsCaFile,
            tlsServerName,
            maxLatency,
            compression,
            True,
        )
        return cls(core)

    @classmethod
    async def connectAsync(
        cls,
        host: str,
        port: int,
        senderCompId: str,
        targetCompId: str = 'IB',
        heartbeatInterval: int = 30,
        resetOnLogon: bool = False,
        seqFile: str | None = None,
        logonFields: list[tuple[int, str]] | None = None,
        tlsCaFile: str | None = None,
        tlsServerName: str | None = None,
        maxLatency: float | None = 120,
        compression: bool = False,
    ) -> FixSession:
        """``connect`` for asyncio."""
        core = await _ibkit.FixCore.connect(
            host,
            port,
            senderCompId,
            targetCompId,
            heartbeatInterval,
            resetOnLogon,
            seqFile,
            logonFields or [],
            tlsCaFile,
            tlsServerName,
            maxLatency,
            compression,
            False,
        )
        return cls(core)

    def send(self, msgType: str, fields: list[tuple[int, Any]]) -> None:
        self._core.send(msgType, [(int(t), str(v)) for t, v in fields])

    def sendOrder(self, clOrdId: str, contract: Contract, order: Order) -> None:
        """Send a NewOrderSingle built from a contract and an order."""
        self._core.send('D', newOrderSingle(clOrdId, contract, order))

    def poll(self, timeout: float = 1.0) -> FixEvent | None:
        return self._core.poll(timeout, True)

    async def pollAsync(self, timeout: float = 1.0) -> FixEvent | None:
        return await self._core.poll(timeout, False)

    def __aiter__(self) -> AsyncIterator[FixEvent]:
        return self._events()

    async def _events(self) -> AsyncIterator[FixEvent]:
        while True:
            event = await self.pollAsync(3600)
            if event is None:
                continue
            yield event
            if event[0] == 'disconnected':
                return

    def logout(self, text: str = '') -> None:
        self._core.logout(text)


def newOrderSingle(
    clOrdId: str, contract: Contract, order: Order, **extras: Any
) -> list[tuple[int, str]]:
    """NewOrderSingle fields (IB conventions) for a contract and an order.

    Keyword arguments set IB tags without a TWS Order field: ``parentClOrdId``
    (6107), ``senderLocationId`` (142, required on futures), ``allocations``
    (``[(subAccount, quantity)]``, 78/79/80), ``disableFaCloseFirst``,
    ``strictPerAccountAllocation``, ``deltaHedge``, ``fxConversion``,
    ``useNetPrice``, ``noDarkPools``, ``staged``."""
    return _ibkit.fix_new_order_single(
        clOrdId, to_dict(contract), to_dict(order), extras or None
    )


def optionExercise(
    clOrdId: str,
    contract: Contract,
    order: Order,
    exercise: str = 'exercise',
    revocable: bool = False,
) -> list[tuple[int, str]]:
    """Option exercise (``'exercise'``) or lapse (``'lapse'``, irrevocable only)
    fields; IB enables FIX exercise per session."""
    return _ibkit.fix_option_exercise(
        clOrdId, to_dict(contract), to_dict(order), exercise, revocable
    )


def cancelReplace(
    origClOrdId: str, clOrdId: str, contract: Contract, order: Order
) -> list[tuple[int, str]]:
    """OrderCancelReplaceRequest (35=G) fields; ``order.deactivate`` pauses the order."""
    return _ibkit.fix_cancel_replace(
        origClOrdId, clOrdId, to_dict(contract), to_dict(order)
    )


def cancelRequest(
    origClOrdId: str,
    clOrdId: str,
    account: str,
    contract: Contract,
    action: str,
    quantity: float,
) -> list[tuple[int, str]]:
    """OrderCancelRequest (35=F) fields; `action` and `quantity` match the order."""
    return _ibkit.fix_cancel_request(
        origClOrdId, clOrdId, account, to_dict(contract), action, quantity
    )


def orderStatusRequest(account: str, clOrdId: str = '*') -> list[tuple[int, str]]:
    """OrderStatusRequest (35=H) fields; ``'*'`` asks for every open order."""
    return _ibkit.fix_order_status_request(account, clOrdId)


def newOrderMultileg(
    clOrdId: str, combo: Contract, legs: list[tuple[Any, ...]], order: Order
) -> list[tuple[int, str]]:
    """NewOrderMultileg (35=AB) fields; `legs` are ``(contract, ratio, action)``
    or ``(contract, ratio, action, openClose)`` with openClose ``'O'`` or ``'C'``."""
    return _ibkit.fix_new_order_multileg(
        clOrdId,
        to_dict(combo),
        [
            (to_dict(leg[0]), float(leg[1]), leg[2], leg[3] if len(leg) > 3 else '')
            for leg in legs
        ],
        to_dict(order),
    )


def multilegCancelReplace(
    origClOrdId: str, clOrdId: str, combo: Contract, order: Order
) -> list[tuple[int, str]]:
    """MultilegCancelReplace (35=AC) fields; IB changes only quantity and price."""
    return _ibkit.fix_multileg_cancel_replace(
        origClOrdId, clOrdId, to_dict(combo), to_dict(order)
    )


def allocationInstruction(
    allocId: str,
    account: str,
    contract: Contract,
    action: str,
    quantity: float,
    avgPx: float,
    tradeDate: str,
    allocations: list[tuple[str, float]],
    transType: str = 'new',
    refAllocId: str | None = None,
) -> list[tuple[int, str]]:
    """Post-trade AllocationInstruction (35=J) fields; `allocations` are
    ``(clearing broker, quantity)``. For ``transType`` ``'replace'`` or
    ``'cancel'`` pass the AllocID being changed as `refAllocId`."""
    return _ibkit.fix_allocation_instruction(
        allocId,
        transType,
        refAllocId,
        account,
        to_dict(contract),
        action,
        quantity,
        avgPx,
        tradeDate,
        allocations,
    )
