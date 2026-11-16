// IBKR Web API (Client Portal API) REST and WebSocket clients, and Flex.
import { guard, guarded } from './errors.js';
import { type Json, native, type WebCore, type WsCore } from './native.js';

export const GATEWAY_URL = 'https://localhost:5000/v1/api';
export const OAUTH_URL = 'https://api.ibkr.com/v1/api';

/** OAuth 1.0a credentials. Keys are PEM text. */
export interface OAuthConfig {
  consumerKey: string;
  accessToken: string;
  accessTokenSecret: string;
  encryptionKey: string;
  signatureKey: string;
  dhPrime: string;
  dhGenerator?: number;
  realm?: string;
}

/** OAuth 2.0 (`private_key_jwt`) credentials. `privateKey` is PEM text;
 * `ip` is the public address the requests come from. */
export interface OAuth2Config {
  clientId: string;
  keyId: string;
  credential: string;
  privateKey: string;
  ip: string;
  scope?: string;
  authUrl?: string;
}

export interface WebClientOptions {
  baseUrl?: string;
  /** Default: true for localhost (the Client Portal Gateway's own certificate). */
  acceptInvalidCerts?: boolean;
  /** Seconds. */
  timeout?: number;
  oauth?: OAuthConfig;
  oauth2?: OAuth2Config;
  /** Wait as needed to stay within IBKR's rate limits (default on); IBKR
   * blocks an address that exceeds them for 10 to 15 minutes. */
  pacing?: boolean;
}

type Query = Record<string, string | number | boolean | null | undefined>;
const local = (url: string) => url.includes('localhost') || url.includes('127.0.0.1');

/** REST client; each method returns the decoded JSON body. */
export class WebClient {
  readonly core: WebCore;

  constructor(options: WebClientOptions = {}) {
    let baseUrl = options.baseUrl ?? GATEWAY_URL;
    if ((options.oauth || options.oauth2) && options.baseUrl === undefined) baseUrl = OAUTH_URL;
    this.core = guard(
      () =>
        new native.WebCore({
          baseUrl,
          acceptInvalidCerts: options.acceptInvalidCerts ?? local(baseUrl),
          timeout: options.timeout ?? 30,
          oauth: options.oauth ?? null,
          oauth2: options.oauth2 ?? null,
          pacing: options.pacing ?? true,
        }),
    );
  }

  /** Send any request; `path` is relative to the base URL. */
  request(method: string, path: string, query?: Query, body?: unknown): Promise<Json> {
    return guarded(this.core.request(method, path, query ?? null, body ?? null));
  }
  /** A `gw/api/...` request (Account Management, pre-trade compliance);
   * needs OAuth 2.0; POST and PATCH bodies are sent as signed JWTs. */
  gwRequest(method: string, path: string, query?: Query, body?: unknown): Promise<Json> {
    return guarded(this.core.gwRequest(method, path, query ?? null, body ?? null));
  }
  /** Third-party OAuth 1.0a: temporary token for the user to authorize. */
  oauthRequestToken(): Promise<string> {
    return guarded(this.core.oauthRequestToken());
  }
  oauthAccessToken(requestToken: string, verifier: string): Promise<Json> {
    return guarded(this.core.oauthAccessToken(requestToken, verifier));
  }
  oauthInit(compete = true): Promise<Json> {
    return guarded(this.core.oauthInit(compete));
  }
  /** Get an OAuth 2.0 bearer token and open a brokerage session. */
  oauth2Init(compete = true): Promise<Json> {
    return guarded(this.core.oauth2Init(compete));
  }
  /** The OAuth 2.0 bearer token (for `WsClient.connect({ bearerToken })`). */
  bearerToken(): string | null {
    return this.core.liveSessionToken();
  }

  authStatus = () => this.request('POST', 'iserver/auth/status');
  tickle = () => this.request('POST', 'tickle');
  logout = () => this.request('POST', 'logout');
  accounts = () => this.request('GET', 'iserver/accounts');
  portfolioAccounts = () => this.request('GET', 'portfolio/accounts');
  positions = (accountId: string, page = 0) => this.request('GET', `portfolio/${accountId}/positions/${page}`);
  searchContract = (symbol: string, secType?: string) =>
    this.request('GET', 'iserver/secdef/search', { symbol, secType });
  snapshot = (conids: number[], fields: string[]) =>
    this.request('GET', 'iserver/marketdata/snapshot', { conids: conids.join(','), fields: fields.join(',') });
  history = (
    conid: number,
    bar: string,
    options: { period?: string; exchange?: string; outsideRth?: boolean; startTime?: string; direction?: number; source?: string } = {},
  ) => this.request('GET', 'iserver/marketdata/history', { conid, bar, ...options });
  liveOrders = () => this.request('GET', 'iserver/account/orders');
  placeOrder = (accountId: string, orders: Record<string, unknown>[]) =>
    this.request('POST', `iserver/account/${accountId}/orders`, undefined, { orders });
  reply = (replyId: string, confirmed = true) => this.request('POST', `iserver/reply/${replyId}`, undefined, { confirmed });
  /** Cancel an order (-1: all open orders). US futures need `manualIndicator`
   * and `extOperator` (CME Rule 536-B). */
  cancelOrder = (
    accountId: string,
    orderId: number,
    options: { manualIndicator?: boolean; extOperator?: string; manualCancelTime?: string } = {},
  ) => this.request('DELETE', `iserver/account/${accountId}/order/${orderId}`, options);
  trades = () => this.request('GET', 'iserver/account/trades');
  /** Create an alert, or modify one when `alert` has `orderId`. */
  createAlert = (accountId: string, alert: Record<string, unknown>) =>
    this.request('POST', `iserver/account/${accountId}/alert`, undefined, alert);
  alerts = (accountId: string) => this.request('GET', `iserver/account/${accountId}/alerts`);
  alertDetails = (alertId: number) => this.request('GET', `iserver/account/alert/${alertId}`, { type: 'Q' });
  activateAlert = (accountId: string, alertId: number, active = true) =>
    this.request('POST', `iserver/account/${accountId}/alert/activate`, undefined, { alertId, alertActive: Number(active) });
  deleteAlert = (accountId: string, alertId: number) => this.request('DELETE', `iserver/account/${accountId}/alert/${alertId}`);
  mtaAlert = () => this.request('GET', 'iserver/account/mta');
  forecastCategoryTree = () => this.request('GET', 'forecast/category/tree');
  forecastMarket = (underlyingConid: number, exchange?: string) =>
    this.request('GET', 'forecast/contract/market', { underlyingConid, exchange });
  forecastContractDetails = (conid: number) => this.request('GET', 'forecast/contract/details', { conid });
  forecastContractRules = (conid: number) => this.request('GET', 'forecast/contract/rules', { conid });
  forecastContractSchedules = (conid: number) => this.request('GET', 'forecast/contract/schedules', { conid });
  fyiSetSetting = (typecode: string, enabled: boolean) => this.request('POST', `fyi/settings/${typecode}`, undefined, { enabled });
  fyiMarkDisclaimerRead = (typecode: string) => this.request('PUT', `fyi/disclaimer/${typecode}`);
  fyiSetEmailDelivery = (enabled: boolean) => this.request('PUT', 'fyi/deliveryoptions/email', { enabled });
  fyiSetDeviceDelivery = (deviceName: string, deviceId: string, uiName: string, enabled: boolean) =>
    this.request('POST', 'fyi/deliveryoptions/device', undefined, { devicename: deviceName, deviceId, uiName, enabled });
  fyiDeleteDevice = (deviceId: string) => this.request('DELETE', `fyi/deliveryoptions/${deviceId}`);
  fyiNotificationsMore = (id: string) => this.request('GET', 'fyi/notifications/more', { id });
  faAllocationGroupAdd = (group: Record<string, unknown>) => this.request('POST', 'iserver/account/allocation/group', undefined, group);
  faAllocationGroupModify = (group: Record<string, unknown>) => this.request('PUT', 'iserver/account/allocation/group', undefined, group);
  faAllocationGroup = (name: string) => this.request('POST', 'iserver/account/allocation/group/single', undefined, { name });
  faAllocationGroupDelete = (name: string) => this.request('POST', 'iserver/account/allocation/group/delete', undefined, { name });
  faAllocationPresetsSet = (presets: Record<string, unknown>) =>
    this.request('POST', 'iserver/account/allocation/presets', undefined, presets);
  /** Snapshot without a brokerage session (OAuth only). */
  mdSnapshot = (conids: number[], fields: string[]) =>
    this.request('GET', 'md/snapshot', { conids: conids.join(','), fields: fields.join(',') });
  paSummary = (accountIds: string[]) => this.request('POST', 'pa/summary', undefined, { acctIds: accountIds });
  /** Allocation by `type` (`'ASSET_CLASS'`, `'SECTOR'`, ...). */
  paAllocation = (accountIds: string[], type: string, options: { currency?: string; date?: string; model?: string } = {}) =>
    this.request('POST', 'pa/allocation', undefined, { acctIds: accountIds, type, ...options });
  /** Answer a server prompt from an `ntf` WebSocket message. */
  serverPromptReply = (orderId: number, reqId: string, text: string) =>
    this.request('POST', 'iserver/notification', undefined, { orderId, reqId, text });
}

export interface WsConnectOptions {
  /** Session id from `POST /tickle` (gateway or OAuth 1.0a). */
  session?: string;
  oauthToken?: string;
  bearerToken?: string;
  acceptInvalidCerts?: boolean;
  /** Seconds between `tic` heartbeats; 0 disables. */
  heartbeat?: number;
  /** Seconds to wait for IBKR's session confirmation. */
  confirmTimeout?: number;
  /** Seconds between market data re-subscriptions (IBKR ends a stream after 15 min). */
  renewMarketData?: number;
}

/** One message: `[topic, args, data]`, for example `['smd', ['265598'], {...}]`. */
export type WsMessage = [topic: string | null, args: string[], data: Json];

/** Web API WebSocket. `connect` returns after IBKR confirms the session. */
export class WsClient {
  constructor(readonly core: WsCore) {}

  static async connect(url = 'wss://localhost:5000/v1/api/ws', options: WsConnectOptions = {}): Promise<WsClient> {
    const core = await guarded(
      native.wsConnect(url, {
        heartbeat: 30,
        confirmTimeout: 10,
        renewMarketData: 600,
        acceptInvalidCerts: local(url),
        ...options,
      }),
    );
    return new WsClient(core);
  }

  private call(method: string, ...args: unknown[]): void {
    guard(() => this.core.call(method, args));
  }
  send = (text: string) => this.call('send', text);
  subscribeMarketData = (conid: number, fields: string[]) => this.call('subscribeMarketData', conid, fields);
  unsubscribeMarketData = (conid: number) => this.call('unsubscribeMarketData', conid);
  subscribeOrders = (filters: string[] = []) => this.call('subscribeOrders', filters);
  exerciseOptions = (conid: number) => this.call('exerciseOptions', conid);
  subscribePnl = () => this.call('subscribePnl');
  subscribeTrades = (realtimeUpdatesOnly = false, days?: number) => this.call('subscribeTrades', realtimeUpdatesOnly, days ?? null);
  subscribeAccountSummary = (accountId: string) => this.call('subscribeAccountSummary', accountId);
  subscribeAccountLedger = (accountId: string) => this.call('subscribeAccountLedger', accountId);
  subscribeBookTrader = (accountId: string, conid: number, exchange?: string) =>
    this.call('subscribeBookTrader', accountId, conid, exchange ?? null);

  /** The next message, or null on timeout or close. */
  recv(timeoutMs?: number): Promise<WsMessage | null> {
    return guarded(this.core.recv(timeoutMs));
  }

  async *[Symbol.asyncIterator](): AsyncGenerator<WsMessage> {
    for (;;) {
      const message = await this.recv();
      if (message === null) return;
      yield message;
    }
  }
}

/** Download a Flex query report (XML) for its saved period. */
export function downloadFlexReport(token: string, queryId: string): Promise<string> {
  return guarded(native.flexDownload(token, queryId));
}
