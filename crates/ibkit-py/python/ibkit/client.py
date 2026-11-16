"""Low-level TWS client (ib_async ``Client``, IB's ``EClient``/``EWrapper``).

Every TWS request is a method with IB's camelCase name and argument order,
for example ``client.reqMktData(reqId, contract, '', False, False, [])``.
Server messages arrive as ``(name, fields)`` pairs, ``name`` being the IB
callback name (``tickPrice``, ``orderStatus``, ...) and ``fields`` a dict.
"""

from __future__ import annotations

import asyncio
import time
from collections.abc import Callable
from typing import Any

from . import _ibkit
from .util import to_dict

# Rust variant names whose IB callback name is not the lower-camel form.
_CALLBACK_NAMES = {
    'ReceiveFa': 'receiveFA',
    'ReplaceFaEnd': 'replaceFAEnd',
    'TickEfp': 'tickEFP',
    'Pnl': 'pnl',
    'PnlSingle': 'pnlSingle',
    'VerifyMessageApi': 'verifyMessageAPI',
    'VerifyAndAuthMessageApi': 'verifyAndAuthMessageAPI',
}


def callback_name(variant: str) -> str:
    """IB callback name of a decoded message variant (``TickPrice`` -> ``tickPrice``)."""
    return _CALLBACK_NAMES.get(variant) or variant[:1].lower() + variant[1:]


def _dispatch(wrapper: Wrapper, name: str, fields: dict[str, Any]) -> None:
    handler = getattr(wrapper, name, None)
    if handler is None:
        wrapper.unhandled(name, fields)
    else:
        handler(fields)


class Wrapper:
    """Base class for message handlers. Define methods named like IB's
    callbacks (``tickPrice``, ``error``, ...); each receives the fields dict.
    Messages without a handler go to ``unhandled``."""

    def unhandled(self, name: str, fields: dict[str, Any]) -> None:
        pass


class Client:
    """Low-level client. Create with ``Client.connect(...)`` for its own
    connection, or use ``ib.client`` to send requests on an ``IB`` connection
    (whose messages then arrive through the ``IB`` events)."""

    def __init__(self, core: _ibkit.ClientCore) -> None:
        self._core = core

    @staticmethod
    def _args(
        host: str,
        port: int,
        clientId: int,
        timeout: float | None,
        connectOptions: str,
        optionalCapabilities: str,
    ) -> dict[str, Any]:
        return {
            'host': host,
            'port': port,
            'clientId': clientId,
            'timeout': timeout or 0,
            'connectOptions': connectOptions,
            'optionalCapabilities': optionalCapabilities,
        }

    @classmethod
    def connect(
        cls,
        host: str = '127.0.0.1',
        port: int = 7497,
        clientId: int = 1,
        timeout: float | None = 4,
        connectOptions: str = '',
        optionalCapabilities: str = '',
    ) -> Client:
        return cls(
            _ibkit.ClientCore.connect(
                cls._args(
                    host, port, clientId, timeout, connectOptions, optionalCapabilities
                ),
                True,
            )
        )

    @classmethod
    async def connectAsync(
        cls,
        host: str = '127.0.0.1',
        port: int = 7497,
        clientId: int = 1,
        timeout: float | None = 4,
        connectOptions: str = '',
        optionalCapabilities: str = '',
    ) -> Client:
        return cls(
            await _ibkit.ClientCore.connect(
                cls._args(
                    host, port, clientId, timeout, connectOptions, optionalCapabilities
                ),
                False,
            )
        )

    def disconnect(self) -> None:
        self._core.disconnect()

    def isConnected(self) -> bool:
        return self._core.is_connected()

    def serverVersion(self) -> int:
        return self._core.server_version()

    def getReqId(self) -> int:
        """Next request or order id (ib_async name)."""
        return self._core.next_req_id()

    def updateReqId(self, minReqId: int) -> None:
        self._core.update_req_id(minReqId)

    def getAccounts(self) -> list[str]:
        return self._core.managed_accounts()

    def twsConnectionTime(self) -> str:
        return self._core.connection_time()

    def messages(self, timeout: float = 0.0) -> list[tuple[str, dict[str, Any]]]:
        """Wait up to ``timeout`` seconds and return all available messages."""
        return [
            (callback_name(name), fields) for name, fields in self._core.poll(timeout)
        ]

    def run(
        self,
        wrapper: Wrapper,
        timeout: float | None = None,
        until: Callable[[], bool] | None = None,
    ) -> None:
        """Dispatch messages to ``wrapper`` until ``until()`` is true, the
        timeout passes or the connection closes."""
        end = None if timeout is None else time.monotonic() + timeout
        while self.isConnected() and not (until and until()):
            left = 0.5 if end is None else end - time.monotonic()
            if left <= 0:
                return
            for name, fields in self.messages(min(left, 0.5)):
                _dispatch(wrapper, name, fields)

    async def runAsync(
        self, wrapper: Wrapper, until: Callable[[], bool] | None = None
    ) -> None:
        while self.isConnected() and not (until and until()):
            for name, fields in await asyncio.to_thread(self.messages, 0.5):
                _dispatch(wrapper, name, fields)

    def __getattr__(self, name: str) -> Callable[..., None]:
        method = getattr(self._core, name)

        def request(*args: Any) -> None:
            method(*[to_dict(a) for a in args])

        request.__name__ = name
        return request
