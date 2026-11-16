"""Error classes; the same kinds as in the Node package.

``ConnectionError``, ``TimeoutError`` and ``InvalidArgumentError`` also
derive from the built-in ``ConnectionError``, ``TimeoutError`` and
``ValueError``, so ``except TimeoutError`` keeps working. They are not in
``ibkit.__all__``: ``from ibkit import *`` must not hide the built-ins.
"""

from __future__ import annotations

import builtins

__all__ = [
    'ConnectionError',
    'GatewayError',
    'IbkitError',
    'InvalidArgumentError',
    'RequestError',
    'TimeoutError',
]


class IbkitError(Exception):
    """Base class of every ibkit error."""


class RequestError(IbkitError):
    """IB answered request ``reqId`` with error ``code``."""

    def __init__(self, reqId: int, code: int, message: str) -> None:
        super().__init__(reqId, code, message)
        self.reqId: int = reqId
        self.code: int = code
        self.message: str = message

    def __str__(self) -> str:
        return f'API error {self.code} for request {self.reqId}: {self.message}'


class ConnectionError(IbkitError, builtins.ConnectionError):
    """The connection is closed or could not be opened."""


class TimeoutError(IbkitError, builtins.TimeoutError):
    """An answer did not arrive in time."""


class InvalidArgumentError(IbkitError, ValueError):
    """An argument has the wrong form or value."""


class GatewayError(IbkitError):
    """The ibkit control API answered with error ``status``."""

    def __init__(self, status: int, message: str) -> None:
        super().__init__(status, message)
        self.status: int = status
        self.message: str = message

    def __str__(self) -> str:
        return f'The control API answered {self.status}: {self.message}'
