"""Start TWS or IB Gateway through IBC and keep it running (ib_async ``ibcontroller``)."""

from __future__ import annotations

import asyncio
import contextlib
import logging
import sys
from dataclasses import dataclass, field
from typing import Any, ClassVar, Self

from . import util
from .contract import Contract, Forex
from .event import Event
from .ib import IB

__all__ = ['IBC', 'Watchdog']

# Field name -> (Unix option, Windows option) of the IBC start script.
_IBC_ARGS: dict[str, tuple[str, str]] = {
    'twsVersion': ('', ''),
    'gateway': ('--gateway', '/Gateway'),
    'tradingMode': ('--mode=', '/Mode:'),
    'twsPath': ('--tws-path=', '/TwsPath:'),
    'twsSettingsPath': ('--tws-settings-path=', ''),
    'ibcPath': ('--ibc-path=', '/IbcPath:'),
    'ibcIni': ('--ibc-ini=', '/Config:'),
    'javaPath': ('--java-path=', '/JavaPath:'),
    'userid': ('--user=', '/User:'),
    'password': ('--pw=', '/PW:'),
    'fixuserid': ('--fix-user=', '/FIXUser:'),
    'fixpassword': ('--fix-pw=', '/FIXPW:'),
    'on2fatimeout': ('--on2fatimeout=', '/On2FATimeout:'),
}


@dataclass
class IBC:
    """Run TWS or IB Gateway with IBC (https://github.com/IbcAlpha/IBC).

    ``IBC(1030, gateway=True, tradingMode='paper')``; ``start()`` launches the
    IBC script, ``terminate()`` stops it. The script's output is logged."""

    IbcLogLevel: ClassVar[int] = logging.DEBUG

    twsVersion: int = 0
    gateway: bool = False
    tradingMode: str = ''
    twsPath: str = ''
    twsSettingsPath: str = ''
    ibcPath: str = ''
    ibcIni: str = ''
    javaPath: str = ''
    userid: str = ''
    password: str = ''
    fixuserid: str = ''
    fixpassword: str = ''
    on2fatimeout: str = ''

    def __post_init__(self) -> None:
        self._isWindows: bool = sys.platform == 'win32'
        if not self.ibcPath:
            self.ibcPath = 'C:\\IBC' if self._isWindows else '/opt/ibc'
        self._proc: asyncio.subprocess.Process | None = None
        self._monitor: asyncio.Future[None] | None = None
        self._logger = logging.getLogger('ibkit.IBC')

    def __enter__(self) -> Self:
        self.start()
        return self

    def __exit__(self, *_exc: object) -> None:
        self.terminate()

    def start(self) -> None:
        util.run(self.startAsync())

    def terminate(self) -> None:
        util.run(self.terminateAsync())

    def command(self) -> list[str]:
        """The IBC start command for the configured fields."""
        cmd = [
            f'{self.ibcPath}\\scripts\\StartIBC.bat'
            if self._isWindows
            else f'{self.ibcPath}/scripts/ibcstart.sh'
        ]
        for name, value in util.dataclassAsDict(self).items():
            arg = _IBC_ARGS[name][self._isWindows]
            if not value:
                continue
            if arg.endswith(('=', ':')):
                cmd.append(f'{arg}{value}')
            else:
                cmd.append(arg or str(value))
        return cmd

    async def startAsync(self) -> None:
        if self._proc:
            return
        self._logger.info('Starting')
        self._proc = await asyncio.create_subprocess_exec(
            *self.command(), stdout=asyncio.subprocess.PIPE
        )
        self._monitor = asyncio.ensure_future(self.monitorAsync())

    async def terminateAsync(self) -> None:
        if not self._proc:
            return
        self._logger.info('Terminating')
        if self._monitor:
            self._monitor.cancel()
            self._monitor = None
        if self._isWindows:
            kill = await asyncio.create_subprocess_exec(
                'taskkill', '/F', '/T', '/PID', str(self._proc.pid)
            )
            await kill.wait()
        else:
            with contextlib.suppress(ProcessLookupError):
                self._proc.terminate()
                await self._proc.wait()
        self._proc = None

    async def monitorAsync(self) -> None:
        while self._proc and self._proc.stdout:
            line = await self._proc.stdout.readline()
            if not line:
                break
            self._logger.log(IBC.IbcLogLevel, line.strip().decode())


@dataclass
class Watchdog:
    """Keep TWS or IB Gateway running: start it with `controller`, connect
    `ib`, and restart both when the connection drops, error 100 or 1100
    arrives, or no data arrives for `appTimeout` seconds and a historical data
    probe also gets no answer within `probeTimeout` seconds."""

    events: ClassVar[tuple[str, ...]] = (
        'startingEvent',
        'startedEvent',
        'stoppingEvent',
        'stoppedEvent',
        'softTimeoutEvent',
        'hardTimeoutEvent',
    )

    controller: IBC
    ib: IB
    host: str = '127.0.0.1'
    port: int = 7497
    clientId: int = 1
    connectTimeout: float = 2
    appStartupTime: float = 30
    appTimeout: float = 20
    retryDelay: float = 2
    readonly: bool = False
    account: str = ''
    raiseSyncErrors: bool = False
    probeContract: Contract = field(default_factory=lambda: Forex('EURUSD'))
    probeTimeout: float = 4

    def __post_init__(self) -> None:
        self.startingEvent: Event[Watchdog] = Event('startingEvent')
        self.startedEvent: Event[Watchdog] = Event('startedEvent')
        self.stoppingEvent: Event[Watchdog] = Event('stoppingEvent')
        self.stoppedEvent: Event[Watchdog] = Event('stoppedEvent')
        self.softTimeoutEvent: Event[Watchdog] = Event('softTimeoutEvent')
        self.hardTimeoutEvent: Event[Watchdog] = Event('hardTimeoutEvent')
        if self.ib.isConnected():
            raise ValueError(
                'Watchdog got a connected IB; pass an IB that is not connected, '
                'the watchdog connects it'
            )
        self._runner: asyncio.Future[None] | None = None
        self._logger = logging.getLogger('ibkit.Watchdog')

    def start(self) -> asyncio.Future[None]:
        self._logger.info('Starting')
        self.startingEvent.emit(self)
        self._runner = asyncio.ensure_future(self.runAsync())
        return self._runner

    def stop(self) -> None:
        self._logger.info('Stopping')
        self.stoppingEvent.emit(self)
        self.ib.disconnect()
        self._runner = None

    async def runAsync(self) -> None:
        waiter: asyncio.Future[None] = asyncio.get_running_loop().create_future()

        def onTimeout(_idle: float) -> None:
            if not waiter.done():
                waiter.set_result(None)

        def onError(_reqId: int, errorCode: int, *_args: Any) -> None:
            if errorCode in {100, 1100} and not waiter.done():
                waiter.set_exception(Warning(f'Error {errorCode}'))

        def onDisconnected() -> None:
            if not waiter.done():
                waiter.set_exception(Warning('Disconnected'))

        while self._runner:
            try:
                await self.controller.startAsync()
                await asyncio.sleep(self.appStartupTime)
                await self.ib.connectAsync(
                    self.host,
                    self.port,
                    self.clientId,
                    self.connectTimeout,
                    self.readonly,
                    self.account,
                    self.raiseSyncErrors,
                )
                self.startedEvent.emit(self)
                self.ib.setTimeout(self.appTimeout)
                self.ib.timeoutEvent += onTimeout
                self.ib.errorEvent += onError
                self.ib.disconnectedEvent += onDisconnected
                while self._runner:
                    waiter = asyncio.get_running_loop().create_future()
                    await waiter
                    self._logger.debug('Soft timeout')
                    self.softTimeoutEvent.emit(self)
                    probe = self.ib.reqHistoricalDataAsync(
                        self.probeContract, '', '30 S', '5 secs', 'MIDPOINT', False
                    )
                    bars = None
                    with contextlib.suppress(asyncio.TimeoutError):
                        bars = await asyncio.wait_for(probe, self.probeTimeout)
                    if not bars:
                        self.hardTimeoutEvent.emit(self)
                        raise Warning('Hard timeout')
                    self.ib.setTimeout(self.appTimeout)
            except ConnectionError:
                pass
            except Warning as w:
                self._logger.warning(w)
            except Exception:
                self._logger.exception('watchdog cycle failed')
            finally:
                self.ib.timeoutEvent -= onTimeout
                self.ib.errorEvent -= onError
                self.ib.disconnectedEvent -= onDisconnected
                self.ib.disconnect()
                await self.controller.terminateAsync()
                self.stoppedEvent.emit(self)
                if self._runner:
                    await asyncio.sleep(self.retryDelay)
