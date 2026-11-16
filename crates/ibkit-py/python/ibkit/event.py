"""A small event class compatible with the parts of ``eventkit`` that
ib_async users rely on: ``event += handler``, ``event -= handler``,
``event.emit(*args)``, ``await event``, ``async for args in event`` and
operators (``Op``) that listen to a source event."""

from __future__ import annotations

import asyncio
import logging
from collections.abc import Callable, Generator
from typing import Any, Generic, Self, TypeVar, TypeVarTuple, Unpack, overload

_logger = logging.getLogger('ibkit.event')

# Running coroutine handlers: the event loop keeps only weak references to
# tasks, so a task without a reference here can be collected while it runs.
_handler_tasks: set[asyncio.Future[Any]] = set()

_T = TypeVar('_T')
_Ts = TypeVarTuple('_Ts')


class Event(Generic[*_Ts]):
    """An event whose handlers take the arguments ``_Ts``; for example
    ``Event[Trade, Fill]`` calls ``handler(trade, fill)``. ``await event``
    gives the argument of a one-argument event, or else a tuple."""

    __slots__ = ('_handlers', '_waiters', 'name')

    def __init__(self, name: str = '') -> None:
        self.name: str = name
        self._handlers: list[Callable[[Unpack[_Ts]], object]] = []
        self._waiters: list[asyncio.Future[Any]] = []

    def connect(self, handler: Callable[[Unpack[_Ts]], object]) -> Self:
        if handler not in self._handlers:
            self._handlers.append(handler)
        return self

    def disconnect(self, handler: Callable[[Unpack[_Ts]], object]) -> Self:
        if handler in self._handlers:
            self._handlers.remove(handler)
        return self

    __iadd__ = connect
    __isub__ = disconnect

    def clear(self) -> None:
        self._handlers.clear()

    def __len__(self) -> int:
        return len(self._handlers)

    def emit(self, *args: *_Ts) -> None:
        for handler in list(self._handlers):
            try:
                result = handler(*args)
                if asyncio.iscoroutine(result):
                    task = asyncio.ensure_future(result)
                    _handler_tasks.add(task)
                    task.add_done_callback(self._handler_done)
            except Exception:
                _logger.exception('handler %r of event %s raised', handler, self.name)
        if self._waiters:
            value: Any = args[0] if len(args) == 1 else args
            waiters, self._waiters = self._waiters, []
            for fut in waiters:
                if not fut.done():
                    fut.get_loop().call_soon_threadsafe(_set_result, fut, value)

    __call__ = emit

    @overload
    def wait(self: Event[_T]) -> asyncio.Future[_T]: ...
    @overload
    def wait(self) -> asyncio.Future[tuple[*_Ts]]: ...
    def wait(self) -> asyncio.Future[Any]:
        """Future that completes with the next emitted value."""
        fut: asyncio.Future[Any] = asyncio.get_running_loop().create_future()
        self._waiters.append(fut)
        # A waiter that timed out or was cancelled leaves the list at once.
        fut.add_done_callback(self._waiter_done)
        return fut

    def _waiter_done(self, fut: asyncio.Future[Any]) -> None:
        if fut.cancelled() and fut in self._waiters:
            self._waiters.remove(fut)

    def _handler_done(self, task: asyncio.Future[Any]) -> None:
        _handler_tasks.discard(task)
        if not task.cancelled() and (error := task.exception()) is not None:
            _logger.error('handler of event %s raised', self.name, exc_info=error)

    @overload
    def __await__(self: Event[_T]) -> Generator[Any, None, _T]: ...
    @overload
    def __await__(self) -> Generator[Any, None, tuple[*_Ts]]: ...
    def __await__(self) -> Generator[Any, None, Any]:
        return self.wait().__await__()

    def __aiter__(self) -> Self:
        return self

    @overload
    async def __anext__(self: Event[_T]) -> _T: ...
    @overload
    async def __anext__(self) -> tuple[*_Ts]: ...
    async def __anext__(self) -> Any:
        return await self.wait()

    def set_done(self) -> None:
        """Kept for eventkit compatibility; has no effect."""

    def __repr__(self) -> str:
        return f'Event<{self.name}, {len(self._handlers)} handlers>'


class Op(Event[*_Ts]):
    """Event fed by a source event (eventkit ``Op``). Subclasses override
    ``on_source``; the default passes the values through unchanged."""

    __slots__ = ('_source',)

    def __init__(self, source: Event[*tuple[Any, ...]] | None = None) -> None:
        super().__init__(type(self).__name__)
        self._source = source
        if source is not None:
            source.connect(self.on_source)

    def on_source(self, *args: Any, **kwargs: Any) -> None:
        self.emit(*args)  # type: ignore[arg-type]


def _set_result(fut: asyncio.Future[Any], value: Any) -> None:
    if not fut.done():
        fut.set_result(value)
