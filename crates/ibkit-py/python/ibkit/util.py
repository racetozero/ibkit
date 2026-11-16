"""Conversion helpers and ib_async-compatible utilities."""

from __future__ import annotations

import asyncio
import dataclasses
import datetime as dt
import functools
import logging
import math
import signal
import sys
import time
from collections.abc import AsyncIterator, Callable, Iterable, Iterator
from typing import Any, Protocol, runtime_checkable
from zoneinfo import ZoneInfo

from . import _types
from ._types import UNSET_DOUBLE, UNSET_INTEGER

__all__ = [
    'UNSET_DOUBLE',
    'UNSET_INTEGER',
    'allowCtrlC',
    'barplot',
    'dataclassAsDict',
    'dataclassAsTuple',
    'dataclassNonDefaults',
    'dataclassRepr',
    'dataclassUpdate',
    'df',
    'formatIBDatetime',
    'formatSI',
    'from_dict',
    'getLoop',
    'isNan',
    'isnamedtupleinstance',
    'logToConsole',
    'logToFile',
    'parseIBDatetime',
    'patchAsyncio',
    'run',
    'schedule',
    'sleep',
    'startLoop',
    'timeRange',
    'timeRangeAsync',
    'to_dict',
    'tree',
    'useQt',
    'waitUntil',
    'waitUntilAsync',
]

Time_t = dt.time | dt.datetime

# Generated class name -> the class to construct (hand-written subclasses
# register themselves here, for example Contract and OrderStatus).
CLASSES: dict[str, type] = {name: getattr(_types, name) for name in _types.GENERATED}

# Generated class name -> function choosing the class from the field values
# (Contract picks Stock, Option, ... by secType).
FACTORIES: dict[str, Any] = {}

# Fields that hold seconds since the epoch and become datetime objects.
_TIME_FIELDS: dict[str, tuple[str, ...]] = {
    'Fill': ('time',),
    'TradeLogEntry': ('time',),
    'TickData': ('time',),
    'MktDepthData': ('time',),
}


@functools.cache
def _field_names(cls: type) -> frozenset[str]:
    return frozenset(f.name for f in dataclasses.fields(cls))


@functools.cache
def _generated_name(cls: type) -> str | None:
    for klass in cls.__mro__:
        if klass.__module__ == _types.__name__ and klass.__name__ in _types.GENERATED:
            return klass.__name__
    return None


def to_dict(obj: Any) -> Any:
    """Convert dataclasses, NamedTuples and lists to the dict form the Rust core reads."""
    if isinstance(obj, _types.TagValue):
        return {'tag': obj.tag, 'value': obj.value}
    if isinstance(obj, _types.OrderCondition):
        d = {f.name: to_dict(getattr(obj, f.name)) for f in dataclasses.fields(obj)}
        d['condType'] = str(obj.condType)
        return d
    if dataclasses.is_dataclass(obj) and not isinstance(obj, type):
        return {f.name: to_dict(getattr(obj, f.name)) for f in dataclasses.fields(obj)}
    if isinstance(obj, tuple) and isinstance(obj, _NamedTuple):
        return {k: to_dict(v) for k, v in obj._asdict().items()}
    if isinstance(obj, (list, tuple)):
        return [to_dict(v) for v in obj]
    if isinstance(obj, dict):
        return {k: to_dict(v) for k, v in obj.items()}
    if isinstance(obj, dt.datetime):
        return obj.timestamp()
    return obj


@runtime_checkable
class _NamedTuple(Protocol):
    def _asdict(self) -> dict[str, Any]: ...


def _utc(t: float | None) -> dt.datetime | None:
    if t is None or (isinstance(t, float) and math.isnan(t)):
        return None
    return dt.datetime.fromtimestamp(t, dt.UTC)


def from_dict(cls: type | str, d: Any) -> Any:
    """Build an object of generated class `cls` from the Rust dict form."""
    if d is None:
        return None
    name = cls if isinstance(cls, str) else (_generated_name(cls) or cls.__name__)
    if name == 'TagValue':
        return _types.TagValue(d.get('tag', ''), d.get('value', ''))
    if name == 'OrderCondition':
        from .order import OrderCondition

        return OrderCondition.from_dict(d)
    target = CLASSES.get(name) if isinstance(cls, str) else cls
    if target is None:
        raise ValueError(f'unknown type {name!r}; expected one of {sorted(CLASSES)}')
    nested = _types.NESTED.get(name, {})
    lists = _types.LISTS.get(name, {})
    values: dict[str, Any] = {}
    names = _field_names(target)
    for key, value in d.items():
        if key not in names:
            continue
        if key in nested and isinstance(value, dict):
            value = from_dict(nested[key], value)
        elif key in lists and isinstance(value, list):
            value = [
                from_dict(lists[key], v) if isinstance(v, dict) else v for v in value
            ]
        values[key] = value
    for key in _TIME_FIELDS.get(name, ()):
        if key in values and isinstance(values[key], (int, float)):
            values[key] = _utc(values[key])
    if name == 'Execution' and isinstance(values.get('time'), str):
        values['time'] = parseIBDatetime(values['time'])
    factory = FACTORIES.get(name)
    if factory is not None and isinstance(cls, str):
        target = factory(values)
    base = getattr(_types, name)
    obj = base(**values)
    if target is not base:
        obj.__class__ = target
    return obj


def dataclassAsDict(obj: Any) -> dict[str, Any]:
    return {f.name: getattr(obj, f.name) for f in dataclasses.fields(obj)}


def dataclassAsTuple(obj: Any) -> tuple[Any, ...]:
    return tuple(getattr(obj, f.name) for f in dataclasses.fields(obj))


def dataclassUpdate(obj: Any, *srcObjs: Any, **kwargs: Any) -> Any:
    """Update fields of `obj` from other dataclasses and keyword arguments."""
    for src in srcObjs:
        obj.__dict__.update(dataclassAsDict(src))
    obj.__dict__.update(**kwargs)
    return obj


def isNan(x: float) -> bool:
    return math.isnan(x)


def df(objs: Iterable[Any], labels: list[str] | None = None) -> Any:
    """Create a pandas DataFrame from a list of dataclasses or NamedTuples."""
    import pandas as pd  # pyright: ignore[reportMissingImports]

    rows = []
    for o in objs:
        if dataclasses.is_dataclass(o):
            rows.append(dataclassAsDict(o))
        elif hasattr(o, '_asdict'):
            rows.append(o._asdict())
        else:
            rows.append(o)
    frame = pd.DataFrame(rows)
    if labels:
        frame = frame[labels]
    return frame


def formatIBDatetime(t: dt.date | dt.datetime | str | None) -> str:
    """Format a date or datetime the way TWS expects (UTC with dash for aware datetimes)."""
    if not t:
        return ''
    if isinstance(t, str):
        return t
    if isinstance(t, dt.datetime):
        if t.tzinfo:
            return t.astimezone(dt.UTC).strftime('%Y%m%d-%H:%M:%S')
        return t.strftime('%Y%m%d %H:%M:%S')
    return t.strftime('%Y%m%d 23:59:59')


def parseIBDatetime(s: str) -> dt.date | dt.datetime | str:
    """Parse the date formats TWS sends; returns the input if unknown."""
    s = s.strip()
    if not s:
        return s
    if s.isdigit():
        if len(s) == 8:
            return dt.date(int(s[:4]), int(s[4:6]), int(s[6:8]))
        return dt.datetime.fromtimestamp(int(s), dt.UTC)
    parts = s.replace('-', ' ', 1).split()
    try:
        stamp = dt.datetime.strptime(f'{parts[0]} {parts[1]}', '%Y%m%d %H:%M:%S')
    except (ValueError, IndexError):
        return s
    if len(parts) > 2:
        try:
            return stamp.replace(tzinfo=ZoneInfo(parts[2]))
        except (KeyError, ValueError):
            return stamp
    if '-' in s[:9]:
        return stamp.replace(tzinfo=dt.UTC)
    return stamp


def getLoop() -> asyncio.AbstractEventLoop:
    """The running event loop, else this thread's loop (created when missing or closed)."""
    try:
        return asyncio.get_running_loop()
    except RuntimeError:
        pass
    global _loop
    if _loop is None or _loop.is_closed():
        _loop = asyncio.new_event_loop()
        asyncio.set_event_loop(_loop)
    return _loop


_loop: asyncio.AbstractEventLoop | None = None


def run(*awaitables: Any, timeout: float | None = None) -> Any:
    """Run awaitables to completion on the event loop of `getLoop()`; with no
    awaitables, run the loop until it is stopped. The loop stays open, so
    tasks such as an ``IB.connectAsync`` connection survive between calls."""
    loop = getLoop()
    if loop.is_running():
        raise RuntimeError(
            'util.run was called from a running event loop; await the coroutine '
            'instead, or call util.patchAsyncio() first (needs nest_asyncio)'
        )
    if not awaitables:
        return loop.run_forever()

    async def main() -> Any:
        if len(awaitables) == 1:
            return await asyncio.wait_for(awaitables[0], timeout)
        return await asyncio.wait_for(asyncio.gather(*awaitables), timeout)

    return loop.run_until_complete(main())


def sleep(secs: float = 0.02) -> bool:
    time.sleep(secs)
    return True


def _fillDate(t: Time_t) -> dt.datetime:
    return (
        dt.datetime.combine(dt.datetime.now(t.tzinfo).date(), t)
        if isinstance(t, dt.time)
        else t
    )


def schedule(
    t: Time_t, callback: Callable[..., Any], *args: Any
) -> asyncio.TimerHandle:
    """Call `callback(*args)` at time `t` (a time of day means today)."""
    when = _fillDate(t)
    delay = (when - dt.datetime.now(when.tzinfo)).total_seconds()
    return getLoop().call_later(delay, callback, *args)


def waitUntil(t: Time_t) -> bool:
    """Wait (running the event loop) until time `t`."""
    when = _fillDate(t)
    run(asyncio.sleep((when - dt.datetime.now(when.tzinfo)).total_seconds()))
    return True


async def waitUntilAsync(t: Time_t) -> bool:
    when = _fillDate(t)
    await asyncio.sleep((when - dt.datetime.now(when.tzinfo)).total_seconds())
    return True


def _range_start(start: Time_t, step: float) -> tuple[dt.datetime, dt.timedelta]:
    if step <= 0:
        raise ValueError(
            f'timeRange got step={step}; use a step of more than 0 seconds'
        )
    delta = dt.timedelta(seconds=step)
    t = _fillDate(start)
    now = dt.datetime.now(dt.UTC if t.tzinfo else None)
    while t < now:
        t += delta
    return t, delta


def timeRange(start: Time_t, end: Time_t, step: float) -> Iterator[dt.datetime]:
    """Yield times from `start` to `end` every `step` seconds, waiting for each."""
    t, delta = _range_start(start, step)
    while t <= _fillDate(end):
        waitUntil(t)
        yield t
        t += delta


async def timeRangeAsync(
    start: Time_t, end: Time_t, step: float
) -> AsyncIterator[dt.datetime]:
    t, delta = _range_start(start, step)
    while t <= _fillDate(end):
        await waitUntilAsync(t)
        yield t
        t += delta


def allowCtrlC() -> None:
    """Let Ctrl-C end the program at once."""
    signal.signal(signal.SIGINT, signal.SIG_DFL)


def patchAsyncio() -> None:
    """Allow nested event loops (needs the nest_asyncio package)."""
    import nest_asyncio  # pyright: ignore[reportMissingImports]

    nest_asyncio.apply()


def startLoop() -> None:
    """Use asyncio in a notebook: patch it to allow nested event loops."""
    patchAsyncio()


def useQt(qtLib: str = 'PyQt5', period: float = 0.01) -> None:
    """Run the Qt event loop from the asyncio loop every `period` seconds."""
    if qtLib not in {'PyQt5', 'PyQt6', 'PySide2', 'PySide6'}:
        raise RuntimeError(
            f'useQt got qtLib={qtLib!r}; use one of PyQt5, PyQt6, PySide2, PySide6'
        )
    from importlib import import_module

    qc = import_module(qtLib + '.QtCore')
    qw = import_module(qtLib + '.QtWidgets')
    app = qw.QApplication.instance() or qw.QApplication(sys.argv)
    loop = getLoop()
    stack: list[Any] = []

    def qt_step() -> None:
        loop.call_later(period, qt_step)
        if not stack:
            qloop = qc.QEventLoop()
            timer = qc.QTimer()
            timer.timeout.connect(qloop.quit)
            stack.append((qloop, timer))
        qloop, timer = stack.pop()
        timer.start(0)
        qloop.exec() if qtLib == 'PyQt6' else qloop.exec_()
        timer.stop()
        stack.append((qloop, timer))
        app.processEvents()

    qt_step()


def logToConsole(level: int = logging.INFO) -> None:
    """Log to stderr; if a stderr handler exists, only set the ibkit level."""
    logger = logging.getLogger()
    if any(
        type(h) is logging.StreamHandler and h.stream is sys.stderr
        for h in logger.handlers
    ):
        logging.getLogger('ibkit').setLevel(level)
        return
    logger.setLevel(level)
    handler = logging.StreamHandler()
    handler.setFormatter(
        logging.Formatter('%(asctime)s %(name)s %(levelname)s %(message)s')
    )
    logger.addHandler(handler)


def logToFile(path: str, level: int = logging.INFO) -> None:
    """Also log to the file at `path`."""
    logger = logging.getLogger()
    if logger.handlers:
        logging.getLogger('ibkit').setLevel(level)
    else:
        logger.setLevel(level)
    handler = logging.FileHandler(path)
    handler.setFormatter(
        logging.Formatter('%(asctime)s %(name)s %(levelname)s %(message)s')
    )
    logger.addHandler(handler)


def isnamedtupleinstance(x: Any) -> bool:
    t = type(x)
    return (
        isinstance(x, tuple)
        and len(t.__bases__) == 1
        and isinstance(t.__dict__.get('_fields'), tuple)
        and all(isinstance(n, str) for n in t._fields)
    )


def dataclassNonDefaults(obj: Any) -> dict[str, Any]:
    """Fields of dataclass `obj` that differ from their default (NaN, None and empty containers left out)."""
    if not dataclasses.is_dataclass(obj):
        raise TypeError(
            f'dataclassNonDefaults got {type(obj).__name__}; pass a dataclass instance'
        )
    return {
        f.name: v
        for f, v in zip(dataclasses.fields(obj), dataclassAsTuple(obj), strict=True)
        if v is not None
        and v != f.default
        and not (isinstance(v, float) and math.isnan(v))
        and v != []
        and v != {}
    }


def dataclassRepr(obj: Any) -> str:
    kwargs = ', '.join(f'{k}={v!r}' for k, v in dataclassNonDefaults(obj).items())
    return f'{type(obj).__qualname__}({kwargs})'


def tree(obj: Any) -> Any:
    """Convert nested dataclasses, NamedTuples and containers to plain dicts and lists."""
    if isinstance(obj, bool | int | float | str | bytes):
        return obj
    if isinstance(obj, dt.date | dt.time):
        return obj.isoformat()
    if isinstance(obj, dict):
        return {k: tree(v) for k, v in obj.items()}
    if isnamedtupleinstance(obj):
        return {k: tree(v) for k, v in obj._asdict().items()}
    if isinstance(obj, list | tuple | set):
        return [tree(i) for i in obj]
    if dataclasses.is_dataclass(obj):
        return {type(obj).__qualname__: tree(dataclassNonDefaults(obj))}
    return str(obj)


def formatSI(n: float) -> str:
    """Format `n` with an SI prefix, for example 1234 -> '1.23 k'."""
    s = ''
    if n < 0:
        n = -n
        s = '-'
    if isinstance(n, int) and n < 1000:
        return f'{s}{n} '
    if n < 1e-22:
        return f'{s}0.00 '
    if n >= 9.99e26:
        raise ValueError(f'formatSI got {n}; values must be below 9.99e26')
    i, j = divmod(math.floor(math.log10(n)), 3)
    val = f'{n * 10 ** (-3 * i):.{2 - j}f}'
    if val == '1000':
        i += 1
        j = 0
        val = f'{n * 10 ** (-3 * i):.{2 - j}f}'
    return f'{s}{val} ' + ('yzafpnum kMGTPEZY'[i + 8] if i else '')


def barplot(
    bars: Any, title: str = '', upColor: str = 'blue', downColor: str = 'red'
) -> Any:
    """Candlestick chart of `bars` (BarData list or DataFrame) as a matplotlib Figure."""
    import matplotlib.pyplot as plt  # pyright: ignore[reportMissingImports]
    import pandas as pd  # pyright: ignore[reportMissingImports]
    from matplotlib.lines import Line2D  # pyright: ignore[reportMissingImports]
    from matplotlib.patches import Rectangle  # pyright: ignore[reportMissingImports]

    if isinstance(bars, pd.DataFrame):
        ohlc = [tuple(v) for v in bars[['open', 'high', 'low', 'close']].values]
    else:
        ohlc = [(b.open, b.high, b.low, b.close) for b in bars]
    fig, ax = plt.subplots()
    ax.set_title(title)
    ax.grid(True)
    fig.set_size_inches(10, 6)
    for n, (open_, high, low, close) in enumerate(ohlc):
        color, hi, lo = (
            (upColor, close, open_) if close >= open_ else (downColor, open_, close)
        )
        ax.add_line(Line2D(xdata=(n, n), ydata=(low, lo), color=color, linewidth=1))
        ax.add_line(Line2D(xdata=(n, n), ydata=(high, hi), color=color, linewidth=1))
        ax.add_patch(
            Rectangle(
                xy=(n - 0.3, lo),
                width=0.6,
                height=hi - lo,
                edgecolor=color,
                facecolor=color,
                alpha=0.4,
                antialiased=True,
            )
        )
    ax.autoscale_view()
    return fig
