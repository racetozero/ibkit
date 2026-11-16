"""IBKR Web API (Client Portal API) client on the Rust core."""

from __future__ import annotations

from collections.abc import Awaitable
from dataclasses import dataclass
from typing import Any, Generic, TypeAlias, TypeVar

from . import _ibkit

GATEWAY_URL = 'https://localhost:5000/v1/api'
OAUTH_URL = 'https://api.ibkr.com/v1/api'


@dataclass
class OAuthConfig:
    """OAuth 1.0a credentials. Keys are PEM text."""

    consumerKey: str
    accessToken: str
    accessTokenSecret: str
    encryptionKey: str
    signatureKey: str
    dhPrime: str
    dhGenerator: int = 2
    realm: str = 'limited_poa'


@dataclass
class OAuth2Config:
    """OAuth 2.0 (``private_key_jwt``) credentials. ``privateKey`` is PEM
    text; ``ip`` is the public address the requests come from."""

    clientId: str
    keyId: str
    credential: str
    privateKey: str
    ip: str
    scope: str = 'sso-sessions.write'
    authUrl: str = 'https://api.ibkr.com'


#: A decoded JSON body. IBKR publishes no schema for the Web API responses,
#: so the bodies are not typed further.
Json: TypeAlias = Any

_R = TypeVar('_R')


class _WebApi(Generic[_R]):
    """The Web API endpoints. ``WebClient`` returns each decoded JSON body;
    ``AsyncWebClient`` returns an awaitable of it.

    Use ``request(method, path, query, body)`` for any endpoint; the common
    ones have named methods. With ``oauth`` set, call ``oauthInit()`` first;
    with ``oauth2`` set, call ``oauth2Init()`` first.
    """

    def __init__(
        self,
        baseUrl: str = GATEWAY_URL,
        acceptInvalidCerts: bool | None = None,
        timeout: float = 30,
        oauth: OAuthConfig | None = None,
        oauth2: OAuth2Config | None = None,
        pacing: bool = True,
    ) -> None:
        """With ``pacing`` (default) requests wait as needed to stay within
        IBKR's rate limits (10 per second, and per-endpoint limits); IBKR
        blocks an IP address that exceeds them for 10 to 15 minutes."""
        if (oauth is not None or oauth2 is not None) and baseUrl == GATEWAY_URL:
            baseUrl = OAUTH_URL
        if acceptInvalidCerts is None:
            acceptInvalidCerts = 'localhost' in baseUrl or '127.0.0.1' in baseUrl
        self._core = _ibkit.WebCore(
            baseUrl,
            acceptInvalidCerts,
            timeout,
            None if oauth is None else dict(oauth.__dict__),
            None if oauth2 is None else dict(oauth2.__dict__),
            pacing,
        )

    def request(
        self,
        method: str,
        path: str,
        query: dict[str, Any] | None = None,
        body: Json = None,
    ) -> _R:
        raise NotImplementedError

    def bearerToken(self) -> str | None:
        """The OAuth 2.0 bearer token (for ``WsClient.connect(bearerToken=...)``)."""
        return self._core.live_session_token()

    def authStatus(self) -> _R:
        return self.request('POST', 'iserver/auth/status')

    def tickle(self) -> _R:
        return self.request('POST', 'tickle')

    def logout(self) -> _R:
        return self.request('POST', 'logout')

    def accounts(self) -> _R:
        return self.request('GET', 'iserver/accounts')

    def portfolioAccounts(self) -> _R:
        return self.request('GET', 'portfolio/accounts')

    def positions(self, accountId: str, page: int = 0) -> _R:
        return self.request('GET', f'portfolio/{accountId}/positions/{page}')

    def searchContract(self, symbol: str, secType: str | None = None) -> _R:
        return self.request(
            'GET', 'iserver/secdef/search', {'symbol': symbol, 'secType': secType}
        )

    def snapshot(self, conids: list[int], fields: list[str]) -> _R:
        return self.request(
            'GET',
            'iserver/marketdata/snapshot',
            {'conids': ','.join(map(str, conids)), 'fields': ','.join(fields)},
        )

    def history(
        self,
        conid: int,
        bar: str,
        period: str | None = None,
        exchange: str | None = None,
        outsideRth: bool | None = None,
        startTime: str | None = None,
        direction: int | None = None,
        source: str | None = None,
    ) -> _R:
        return self.request(
            'GET',
            'iserver/marketdata/history',
            {
                'conid': conid,
                'bar': bar,
                'period': period,
                'exchange': exchange,
                'outsideRth': outsideRth,
                'startTime': startTime,
                'direction': direction,
                'source': source,
            },
        )

    def liveOrders(self) -> _R:
        return self.request('GET', 'iserver/account/orders')

    def placeOrder(self, accountId: str, orders: list[dict[str, Any]]) -> _R:
        return self.request(
            'POST', f'iserver/account/{accountId}/orders', body={'orders': orders}
        )

    def reply(self, replyId: str, confirmed: bool = True) -> _R:
        return self.request(
            'POST', f'iserver/reply/{replyId}', body={'confirmed': confirmed}
        )

    def cancelOrder(
        self,
        accountId: str,
        orderId: int,
        manualIndicator: bool | None = None,
        extOperator: str | None = None,
        manualCancelTime: str | None = None,
    ) -> _R:
        """Cancel an order (-1: all open orders). US futures need
        `manualIndicator` and `extOperator` (CME Rule 536-B)."""
        return self.request(
            'DELETE',
            f'iserver/account/{accountId}/order/{orderId}',
            {
                'manualIndicator': manualIndicator,
                'extOperator': extOperator,
                'manualCancelTime': manualCancelTime,
            },
        )

    def trades(self) -> _R:
        return self.request('GET', 'iserver/account/trades')

    # ---- alerts

    def createAlert(self, accountId: str, alert: dict[str, Any]) -> _R:
        """Create an alert, or modify one when `alert` has ``orderId``."""
        return self.request('POST', f'iserver/account/{accountId}/alert', body=alert)

    def alerts(self, accountId: str) -> _R:
        return self.request('GET', f'iserver/account/{accountId}/alerts')

    def alertDetails(self, alertId: int) -> _R:
        return self.request('GET', f'iserver/account/alert/{alertId}', {'type': 'Q'})

    def activateAlert(self, accountId: str, alertId: int, active: bool = True) -> _R:
        return self.request(
            'POST',
            f'iserver/account/{accountId}/alert/activate',
            body={'alertId': alertId, 'alertActive': int(active)},
        )

    def deleteAlert(self, accountId: str, alertId: int) -> _R:
        return self.request('DELETE', f'iserver/account/{accountId}/alert/{alertId}')

    def mtaAlert(self) -> _R:
        return self.request('GET', 'iserver/account/mta')

    # ---- event contracts

    def forecastCategoryTree(self) -> _R:
        return self.request('GET', 'forecast/category/tree')

    def forecastMarket(self, underlyingConid: int, exchange: str | None = None) -> _R:
        return self.request(
            'GET',
            'forecast/contract/market',
            {'underlyingConid': underlyingConid, 'exchange': exchange},
        )

    def forecastContractDetails(self, conid: int) -> _R:
        return self.request('GET', 'forecast/contract/details', {'conid': conid})

    def forecastContractRules(self, conid: int) -> _R:
        return self.request('GET', 'forecast/contract/rules', {'conid': conid})

    def forecastContractSchedules(self, conid: int) -> _R:
        return self.request('GET', 'forecast/contract/schedules', {'conid': conid})

    # ---- notifications (FYI)

    def fyiSetSetting(self, typecode: str, enabled: bool) -> _R:
        return self.request(
            'POST', f'fyi/settings/{typecode}', body={'enabled': enabled}
        )

    def fyiMarkDisclaimerRead(self, typecode: str) -> _R:
        return self.request('PUT', f'fyi/disclaimer/{typecode}')

    def fyiSetEmailDelivery(self, enabled: bool) -> _R:
        return self.request('PUT', 'fyi/deliveryoptions/email', {'enabled': enabled})

    def fyiSetDeviceDelivery(
        self, deviceName: str, deviceId: str, uiName: str, enabled: bool
    ) -> _R:
        body = {
            'devicename': deviceName,
            'deviceId': deviceId,
            'uiName': uiName,
            'enabled': enabled,
        }
        return self.request('POST', 'fyi/deliveryoptions/device', body=body)

    def fyiDeleteDevice(self, deviceId: str) -> _R:
        return self.request('DELETE', f'fyi/deliveryoptions/{deviceId}')

    def fyiNotificationsMore(self, id: str) -> _R:
        return self.request('GET', 'fyi/notifications/more', {'id': id})

    # ---- FA allocation groups

    def faAllocationGroupAdd(self, group: dict[str, Any]) -> _R:
        return self.request('POST', 'iserver/account/allocation/group', body=group)

    def faAllocationGroupModify(self, group: dict[str, Any]) -> _R:
        return self.request('PUT', 'iserver/account/allocation/group', body=group)

    def faAllocationGroup(self, name: str) -> _R:
        return self.request(
            'POST', 'iserver/account/allocation/group/single', body={'name': name}
        )

    def faAllocationGroupDelete(self, name: str) -> _R:
        return self.request(
            'POST', 'iserver/account/allocation/group/delete', body={'name': name}
        )

    def faAllocationPresetsSet(self, presets: dict[str, Any]) -> _R:
        return self.request('POST', 'iserver/account/allocation/presets', body=presets)

    # ---- other

    def mdSnapshot(self, conids: list[int], fields: list[str]) -> _R:
        """Snapshot without a brokerage session (OAuth only)."""
        return self.request(
            'GET',
            'md/snapshot',
            {'conids': ','.join(map(str, conids)), 'fields': ','.join(fields)},
        )

    def paSummary(self, accountIds: list[str]) -> _R:
        return self.request('POST', 'pa/summary', body={'acctIds': accountIds})

    def paAllocation(
        self,
        accountIds: list[str],
        type: str,
        currency: str | None = None,
        date: str | None = None,
        model: str | None = None,
    ) -> _R:
        """Allocation by ``type`` (``'ASSET_CLASS'``, ``'SECTOR'``, ...)."""
        body = {'acctIds': accountIds, 'type': type}
        body.update(
            {
                k: v
                for k, v in (('currency', currency), ('date', date), ('model', model))
                if v
            }
        )
        return self.request('POST', 'pa/allocation', body=body)

    def serverPromptReply(self, orderId: int, reqId: str, text: str) -> _R:
        """Answer a server prompt from an ``ntf`` WebSocket message."""
        return self.request(
            'POST',
            'iserver/notification',
            body={'orderId': orderId, 'reqId': reqId, 'text': text},
        )


class WebClient(_WebApi[Json]):
    """REST client. Each method returns the decoded JSON body; see
    ``AsyncWebClient`` for asyncio. With ``oauth`` set, call ``oauthInit()``
    first; with ``oauth2`` set, call ``oauth2Init()`` first."""

    def request(
        self,
        method: str,
        path: str,
        query: dict[str, Any] | None = None,
        body: Json = None,
    ) -> Json:
        return self._core.request(method, path, query, body, True)

    async def requestAsync(
        self,
        method: str,
        path: str,
        query: dict[str, Any] | None = None,
        body: Json = None,
    ) -> Json:
        return await self._core.request(method, path, query, body, False)

    def gwRequest(
        self,
        method: str,
        path: str,
        query: dict[str, Any] | None = None,
        body: dict[str, Any] | None = None,
    ) -> Json:
        """Send a request to a ``gw/api/...`` path (Account Management,
        pre-trade compliance, third-party and echo endpoints). Needs OAuth 2.0
        with the endpoint's scope in ``OAuth2Config.scope``; POST and PATCH
        bodies are sent as signed JWTs."""
        return self._core.gw_request(method, path, query, body, True)

    def oauthRequestToken(self) -> str:
        """Third-party OAuth 1.0a: temporary token for the user to authorize."""
        return self._core.oauth_request_token(True)

    def oauthAccessToken(self, requestToken: str, verifier: str) -> Json:
        """Third-party OAuth 1.0a: exchange the authorized token and verifier."""
        return self._core.oauth_access_token(requestToken, verifier, True)

    def oauthInit(self, compete: bool = True) -> Json:
        return self._core.oauth_init(compete, True)

    def oauth2Init(self, compete: bool = True) -> Json:
        """Get an OAuth 2.0 bearer token and open a brokerage session."""
        return self._core.oauth2_init(compete, True)


class AsyncWebClient(_WebApi[Awaitable[Json]]):
    """``WebClient`` for asyncio: each method returns an awaitable, for
    example ``await client.accounts()``."""

    def request(
        self,
        method: str,
        path: str,
        query: dict[str, Any] | None = None,
        body: Json = None,
    ) -> Awaitable[Json]:
        return self._core.request(method, path, query, body, False)

    def gwRequest(
        self,
        method: str,
        path: str,
        query: dict[str, Any] | None = None,
        body: dict[str, Any] | None = None,
    ) -> Awaitable[Json]:
        """``WebClient.gwRequest`` for asyncio."""
        return self._core.gw_request(method, path, query, body, False)

    def oauthRequestToken(self) -> Awaitable[str]:
        return self._core.oauth_request_token(False)

    def oauthAccessToken(self, requestToken: str, verifier: str) -> Awaitable[Json]:
        return self._core.oauth_access_token(requestToken, verifier, False)

    def oauthInit(self, compete: bool = True) -> Awaitable[Json]:
        return self._core.oauth_init(compete, False)

    def oauth2Init(self, compete: bool = True) -> Awaitable[Json]:
        return self._core.oauth2_init(compete, False)


class WsClient:
    """Web API WebSocket. ``connect`` returns after IBKR confirms the session
    (frames sent earlier are dropped by the gateway). ``recv`` returns
    ``(topic, args, data)``, for example ``('smd', ['265598'], {...})``, or
    ``None`` on timeout; ``async for msg in ws`` streams messages.

    Market data subscriptions are re-sent every 10 minutes (IBKR ends a stream
    after 15); ``tic`` keeps the socket open every 30 seconds.
    """

    def __init__(self, core: _ibkit.WsCore) -> None:
        self._core = core

    @staticmethod
    def _kwargs(
        session: str | None,
        oauthToken: str | None,
        bearerToken: str | None,
        acceptInvalidCerts: bool | None,
        url: str,
        heartbeat: float | None,
        confirmTimeout: float | None,
        renewMarketData: float | None,
    ) -> dict[str, Any]:
        if acceptInvalidCerts is None:
            acceptInvalidCerts = 'localhost' in url or '127.0.0.1' in url
        return {
            'session': session,
            'oauth_token': oauthToken,
            'bearer_token': bearerToken,
            'accept_invalid_certs': acceptInvalidCerts,
            'heartbeat': heartbeat,
            'confirm_timeout': confirmTimeout,
            'renew_market_data': renewMarketData,
        }

    @classmethod
    def connect(
        cls,
        url: str = 'wss://localhost:5000/v1/api/ws',
        session: str | None = None,
        oauthToken: str | None = None,
        acceptInvalidCerts: bool | None = None,
        heartbeat: float | None = 30,
        bearerToken: str | None = None,
        confirmTimeout: float | None = 10,
        renewMarketData: float | None = 600,
    ) -> WsClient:
        kw = cls._kwargs(
            session,
            oauthToken,
            bearerToken,
            acceptInvalidCerts,
            url,
            heartbeat,
            confirmTimeout,
            renewMarketData,
        )
        return cls(_ibkit.WsCore.connect(url, **kw, blocking=True))

    @classmethod
    async def connectAsync(
        cls,
        url: str = 'wss://localhost:5000/v1/api/ws',
        session: str | None = None,
        oauthToken: str | None = None,
        acceptInvalidCerts: bool | None = None,
        heartbeat: float | None = 30,
        bearerToken: str | None = None,
        confirmTimeout: float | None = 10,
        renewMarketData: float | None = 600,
    ) -> WsClient:
        kw = cls._kwargs(
            session,
            oauthToken,
            bearerToken,
            acceptInvalidCerts,
            url,
            heartbeat,
            confirmTimeout,
            renewMarketData,
        )
        return cls(await _ibkit.WsCore.connect(url, **kw, blocking=False))

    def send(self, text: str) -> None:
        self._core.send(text)

    def subscribeMarketData(self, conid: int, fields: list[str]) -> None:
        self._core.subscribe_market_data(conid, fields)

    def unsubscribeMarketData(self, conid: int) -> None:
        self._core.unsubscribe_market_data(conid)

    def subscribeOrders(self, filters: str | list[str] | None = None) -> None:
        """Live order updates, optionally only some statuses (``'submitted'``, ``'filled'``, ...)."""
        if isinstance(filters, str):
            filters = [filters]
        self._core.subscribe_orders(filters or [])

    def exerciseOptions(self, conid: int) -> None:
        """Start an option exercise; answer the prompts with ``send('inp+exercise+...')``."""
        self._core.exercise_options(conid)

    def subscribePnl(self) -> None:
        self._core.subscribe_pnl()

    def subscribeTrades(
        self, realtimeUpdatesOnly: bool = False, days: int | None = None
    ) -> None:
        self._core.subscribe_trades(realtimeUpdatesOnly, days)

    def subscribeAccountSummary(self, accountId: str) -> None:
        self._core.subscribe_account_summary(accountId)

    def subscribeAccountLedger(self, accountId: str) -> None:
        self._core.subscribe_account_ledger(accountId)

    def subscribeBookTrader(
        self, accountId: str, conid: int, exchange: str | None = None
    ) -> None:
        self._core.subscribe_book_trader(accountId, conid, exchange)

    def recv(
        self, timeout: float | None = None
    ) -> tuple[str | None, list[str], Any] | None:
        return self._core.recv(timeout, True)

    async def recvAsync(
        self, timeout: float | None = None
    ) -> tuple[str | None, list[str], Any] | None:
        return await self._core.recv(timeout, False)

    def __aiter__(self) -> WsClient:
        return self

    async def __anext__(self) -> tuple[str | None, list[str], Any]:
        msg = await self.recvAsync()
        if msg is None:
            raise StopAsyncIteration
        return msg
