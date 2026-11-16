// Loader and type declarations of the native addon (src/lib.rs).
import { createRequire } from 'node:module';

/** Plain JSON data as the addon returns it. */
export type Json = any; // eslint-disable-line @typescript-eslint/no-explicit-any

interface Handle {
  id(): number;
  snapshot(): Json;
}

export interface TickerCore extends Handle {
  midpoint(): number;
  marketPrice(): number;
  hasBidAsk(): boolean;
}

export interface TradeCore extends Handle {
  status(): string;
  isDone(): boolean;
  isActive(): boolean;
  isWorking(): boolean;
  isWaiting(): boolean;
  filled(): number;
  remaining(): number;
}

export interface BarsCore extends Handle {
  /** `{len, bars}`: the list length and the bars from index `from` on. */
  barsFrom(from: number): { len: number; bars: Json[] };
}
export type RealTimeBarsCore = BarsCore;
export type ScanCore = Handle;
export type PnLCore = Handle;
export type PnLSingleCore = Handle;

export interface IbCore {
  isConnected(): boolean;
  disconnect(): void;
  clientId(): number;
  serverVersion(): number;
  managedAccounts(): string[];
  nextReqId(): number;
  client(): ClientCore;
  connectionStats(): Json;
  setTimeout(seconds: number): Promise<void>;
  nextEvents(timeoutMs?: number): Promise<Json[]>;
  trades(): TradeCore[];
  openTrades(): TradeCore[];
  tickers(): TickerCore[];
  ticker(contract: Json): TickerCore | null;
  tradeById(id: number): TradeCore | null;
  tickerById(id: number): TickerCore | null;
  tickerState(ticker: TickerCore): { seq: number; data: Json };
  tradeState(trade: TradeCore): { seq: number; data: Json };
  realtimeBars(): RealTimeBarsCore[];
  placeOrder(contract: Json, order: Json): TradeCore;
  cancelOrder(order: Json, manualCancelOrderTime: string): TradeCore | null;
  reqMktData(contract: Json, genericTickList: string, snapshot: boolean, regulatorySnapshot: boolean, options: Json): TickerCore;
  reqTickByTickData(contract: Json, tickType: string, numberOfTicks: number, ignoreSize: boolean): TickerCore;
  reqMktDepth(contract: Json, numRows: number, isSmartDepth: boolean, options: Json): TickerCore;
  reqRealTimeBars(contract: Json, barSize: number, whatToShow: string, useRth: boolean, options: Json): RealTimeBarsCore;
  cancelRealTimeBars(bars: RealTimeBarsCore): void;
  cancelHistoricalData(bars: BarsCore): void;
  reqPnl(account: string, modelCode: string): PnLCore;
  reqPnlSingle(account: string, modelCode: string, conId: number): PnLSingleCore;
  reqScannerSubscription(subscription: Json, options: Json, filterOptions: Json): ScanCore;
  cancelScannerSubscription(scan: ScanCore): void;
  reqTickers(contracts: Json, regulatorySnapshot: boolean): Promise<TickerCore[]>;
  reqHistoricalData(
    contract: Json,
    endDateTime: string,
    durationStr: string,
    barSizeSetting: string,
    whatToShow: string,
    useRth: boolean,
    formatDate: number,
    keepUpToDate: boolean,
    chartOptions: Json,
  ): Promise<BarsCore>;
  reqOpenOrders(allClients: boolean): Promise<TradeCore[]>;
  reqCompletedOrders(apiOnly: boolean): Promise<TradeCore[]>;
  call(method: string, args: unknown[]): Json;
  request(method: string, args: unknown[]): Promise<Json>;
}

export interface ClientCore {
  isConnected(): boolean;
  disconnect(): void;
  serverVersion(): number;
  clientId(): number;
  connectionTime(): string;
  nextReqId(): number;
  updateReqId(minId: number): void;
  managedAccounts(): string[];
  poll(timeoutMs: number): Promise<[string, Json][]>;
  request(method: string, args: unknown[]): Json;
}

export interface WebCore {
  request(method: string, path: string, query?: Json, body?: Json): Promise<Json>;
  gwRequest(method: string, path: string, query?: Json, body?: Json): Promise<Json>;
  oauthRequestToken(): Promise<string>;
  oauthAccessToken(requestToken: string, verifier: string): Promise<Json>;
  oauthInit(compete: boolean): Promise<Json>;
  oauth2Init(compete: boolean): Promise<Json>;
  liveSessionToken(): string | null;
}

export interface WsCore {
  call(method: string, args: unknown[]): Json;
  recv(timeoutMs?: number): Promise<[string | null, string[], Json] | null>;
}

export interface FixCore {
  send(msgType: string, fields: [number, string][]): void;
  logout(text: string): void;
  poll(timeoutMs: number): Promise<Json>;
}

export interface FakeTwsCore {
  readonly port: number;
  requests(): string[][];
  send(fields: string[]): void;
  close(): void;
}

export interface Native {
  ibConnect(args: Json): Promise<IbCore>;
  clientConnect(args: Json): Promise<ClientCore>;
  clientRequestNames(): string[];
  WebCore: new (args: Json) => WebCore;
  wsConnect(url: string, args: Json): Promise<WsCore>;
  flexDownload(token: string, queryId: string): Promise<string>;
  fixConnect(args: Json): Promise<FixCore>;
  fixBuild(kind: string, args: unknown[]): [number, string][];
  fakeTws(args?: Json): Promise<FakeTwsCore>;
  version(): string;
}

/** The npm package that holds the addon built for this platform. */
function platformPackage(): string {
  const { platform, arch } = process;
  if (platform === 'linux') {
    const report = process.report?.getReport() as { header?: { glibcVersionRuntime?: string } } | undefined;
    return `@ibkit/node-linux-${arch}-${report?.header?.glibcVersionRuntime ? 'gnu' : 'musl'}`;
  }
  return `@ibkit/node-${platform}-${arch}${platform === 'win32' ? '-msvc' : ''}`;
}

/** The addon: `ibkit.node` at the package root when built from source
 * (`bun run build:native`), else the platform package from npm. */
export const native: Native = (() => {
  const require = createRequire(import.meta.url);
  try {
    return require('../ibkit.node');
  } catch (local) {
    const name = platformPackage();
    try {
      return require(name);
    } catch (installed) {
      throw new Error(
        `The ibkit native addon did not load. ../ibkit.node: ${(local as Error).message}. ${name}: ${(installed as Error).message}. Install @ibkit/node with its optional dependencies for this platform, or build the addon with \`bun run build:native\`.`,
      );
    }
  }
})();
