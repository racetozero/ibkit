"""Client for the ibkit control API.

``ibkit gateway run`` with ``control_listen`` set serves a REST and WebSocket API
that reports health and status, accepts commands, and changes settings,
credentials and the Mobile Authenticator secret::

    from ibkit.gateway import Control, GatewayClient

    gw = GatewayClient('http://127.0.0.1:5800')  # token from IBKIT_CONTROL_TOKEN
    print(gw.status()['state'])
    gw.control(Control.RECONNECT_ACCOUNT)
    for event in gw.events():
        print(event['event'])
"""

from __future__ import annotations

import os
from collections.abc import AsyncIterator, Iterator, Mapping
from typing import Any

from . import _ibkit
from ._gateway_types import (
    AgentStatus,
    Alert,
    AlertKind,
    Config,
    ConfigUpdate,
    Control,
    Dialog,
    ErrorCode,
    Event,
    Health,
    RecoveryState,
    StartResult,
    State,
    Status,
    _decode_Config,
    _decode_Event,
    _decode_Health,
    _decode_Status,
)
from .errors import GatewayError, InvalidArgumentError

__all__ = [
    'DEFAULT_URL',
    'AgentStatus',
    'Alert',
    'AlertKind',
    'Config',
    'ConfigUpdate',
    'Control',
    'Dialog',
    'ErrorCode',
    'Event',
    'GatewayClient',
    'GatewayError',
    'GatewayEvents',
    'Health',
    'RecoveryState',
    'SettingValue',
    'StartResult',
    'State',
    'Status',
]

DEFAULT_URL = 'http://127.0.0.1:5800'

#: A setting value for ``updateConfig``; ``None`` removes a value set through the API.
SettingValue = str | int | bool | None


class GatewayEvents:
    """Controller events from the moment of connection. ``recv`` returns one
    event, or ``None`` on timeout or when the controller closed the stream;
    iterate (``for`` or ``async for``) to read until the stream closes."""

    def __init__(self, core: _ibkit.GatewayEventsCore) -> None:
        self._core = core

    def recv(self, timeout: float | None = None) -> Event | None:
        event = self._core.recv(timeout, True)
        return None if event is None else _decode_Event(event)

    async def recvAsync(self, timeout: float | None = None) -> Event | None:
        event = await self._core.recv(timeout, False)
        return None if event is None else _decode_Event(event)

    def __iter__(self) -> Iterator[Event]:
        while (event := self.recv()) is not None:
            yield event

    async def __aiter__(self) -> AsyncIterator[Event]:
        while (event := await self.recvAsync()) is not None:
            yield event


class GatewayClient:
    """Control API client. Each method has an ``...Async`` variant.

    ``token`` defaults to the ``IBKIT_CONTROL_TOKEN`` environment variable, the
    controller's default token source.
    """

    def __init__(self, url: str = DEFAULT_URL, token: str | None = None) -> None:
        token = token if token is not None else os.environ.get('IBKIT_CONTROL_TOKEN')
        if not token:
            raise InvalidArgumentError(
                'No control API token: pass token= or set IBKIT_CONTROL_TOKEN to the '
                "controller's control_token_env value."
            )
        self.url = url
        self._core = _ibkit.GatewayCore(url, token)

    def health(self) -> Health:
        """Liveness; a halted controller is live. No token is needed."""
        return _decode_Health(self._core.health(True))

    async def healthAsync(self) -> Health:
        return _decode_Health(await self._core.health(False))

    def ready(self) -> Health:
        """Logged in, and the IB API socket answered a handshake."""
        return _decode_Health(self._core.ready(True))

    async def readyAsync(self) -> Health:
        return _decode_Health(await self._core.ready(False))

    def status(self) -> Status:
        return _decode_Status(self._core.status(True))

    async def statusAsync(self) -> Status:
        return _decode_Status(await self._core.status(False))

    def metrics(self) -> str:
        """Prometheus text metrics."""
        return self._core.metrics(True)

    async def metricsAsync(self) -> str:
        return await self._core.metrics(False)

    def openapi(self) -> dict[str, Any]:
        """The OpenAPI 3.1 document of the control API."""
        return self._core.openapi(True)

    async def openapiAsync(self) -> dict[str, Any]:
        return await self._core.openapi(False)

    def control(self, control: Control | str) -> None:
        """Queue a command; the controller acts on it within a second."""
        self._core.control(Control(control).value, True)

    async def controlAsync(self, control: Control | str) -> None:
        await self._core.control(Control(control).value, False)

    def config(self) -> Config:
        """The effective settings (file, API settings, flags)."""
        return _decode_Config(self._core.config(True))

    async def configAsync(self) -> Config:
        return _decode_Config(await self._core.config(False))

    def updateConfig(self, values: Mapping[str, SettingValue]) -> Config:
        """Save settings (``None`` removes a value set through the API) and
        start a new Gateway session with them. Returns the new settings."""
        return _decode_Config(self._core.update_config(dict(values), True))

    async def updateConfigAsync(self, values: Mapping[str, SettingValue]) -> Config:
        return _decode_Config(await self._core.update_config(dict(values), False))

    def clearConfig(self) -> Config:
        """Remove every setting saved through the API, so the file, flags and
        variables apply again, and reload. Returns the new settings."""
        return _decode_Config(self._core.clear_config(True))

    async def clearConfigAsync(self) -> Config:
        return _decode_Config(await self._core.clear_config(False))

    def setCredentials(self, username: str, password: str) -> None:
        """Save the IBKR login in private files in the controller's state_dir."""
        self._core.set_credentials(username, password, True)

    async def setCredentialsAsync(self, username: str, password: str) -> None:
        await self._core.set_credentials(username, password, False)

    def setTotpSecret(self, secret: str) -> None:
        """Save the base32 Mobile Authenticator secret; login codes are then
        entered automatically."""
        self._core.set_totp_secret(secret, True)

    async def setTotpSecretAsync(self, secret: str) -> None:
        await self._core.set_totp_secret(secret, False)

    def clearTotpSecret(self) -> None:
        """Return to IB Key approval on the phone."""
        self._core.clear_totp_secret(True)

    async def clearTotpSecretAsync(self) -> None:
        await self._core.clear_totp_secret(False)

    def events(self) -> GatewayEvents:
        return GatewayEvents(self._core.events(True))

    async def eventsAsync(self) -> GatewayEvents:
        return GatewayEvents(await self._core.events(False))
