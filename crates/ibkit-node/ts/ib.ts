// High-level TWS API client (ib_async `IB`) on the Rust core.
import { EventEmitter } from 'node:events';
import { Client } from './client.js';
import type { ContractInput } from './contract.js';
import { guard, guarded } from './errors.js';
import {
  type BarsCore,
  type IbCore,
  type Json,
  native,
  type PnLCore,
  type PnLSingleCore,
  type RealTimeBarsCore,
  type ScanCore,
  type TickerCore,
  type TradeCore,
} from './native.js';
import type { BracketOrder, OrderInput } from './order.js';
import type * as T from './types.js';
import { formatIBDatetime } from './util.js';

/** What `connect` loads before it returns (bits; `All` by default). */
export enum StartupFetch {
  Positions = 1,
  OrdersOpen = 2,
  OrdersComplete = 4,
  AccountUpdates = 8,
  SubAccountUpdates = 16,
  Executions = 32,
  All = 63,
}

export interface ConnectOptions {
  host?: string;
  port?: number;
  clientId?: number;
  /** Seconds to wait for the connection and the initial synchronization. */
  timeout?: number;
  readonly?: boolean;
  account?: string;
  raiseSyncErrors?: boolean;
  /** Reject a request when IB answers it with an error (else the result is empty). */
  raiseRequestErrors?: boolean;
  /** Seconds after which a request fails with `TimeoutError`; 0 waits forever. */
  requestTimeout?: number;
  /** Also emit every decoded TWS message as a `message` event. */
  emitMessages?: boolean;
  fetchFields?: StartupFetch | number;
  /** Request pacing: at most `maxRequests` per `requestsInterval` seconds. */
  maxRequests?: number;
  requestsInterval?: number;
  connectOptions?: string;
}

export interface ConnectionStats {
  startTime: number;
  duration: number;
  numBytesRecv: number;
  numBytesSent: number;
  numMsgRecv: number;
  numMsgSent: number;
}

const num = (value: unknown): number => (typeof value === 'number' ? value : Number.NaN);

/** The serde form of a ticker; numeric fields not yet received are `null`. */
export type TickerData = { contract: T.Contract } & Record<string, unknown>;

/** Source of a ticker's state: the connection that forwards its deltas. */
interface TickerSource {
  tickerState(ticker: TickerCore): { seq: number; data: TickerData };
}

/**
 * Live market data of one contract. Emits `update` on each change. Events
 * carry only the fields that changed, merged into `data` in order (`seq`).
 */
export class Ticker extends EventEmitter<{ update: [Ticker] }> {
  /** Sequence number of the state in `data`. */
  seq = 0;
  data: TickerData;
  constructor(
    readonly core: TickerCore,
    private readonly source: TickerSource,
  ) {
    super();
    const state = source.tickerState(core);
    this.data = state.data;
    this.seq = state.seq;
  }
  /** Read the current state from the core now. */
  refresh(): this {
    const state = this.source.tickerState(this.core);
    this.data = state.data;
    this.seq = state.seq;
    return this;
  }
  get contract(): T.Contract {
    return this.data.contract;
  }
  /** Unix time in seconds of the last tick. */
  get time(): number | null {
    return (this.data.time as number | null) ?? null;
  }
  get bid(): number {
    return num(this.data.bid);
  }
  get bidSize(): number {
    return num(this.data.bidSize);
  }
  get ask(): number {
    return num(this.data.ask);
  }
  get askSize(): number {
    return num(this.data.askSize);
  }
  get last(): number {
    return num(this.data.last);
  }
  get lastSize(): number {
    return num(this.data.lastSize);
  }
  get volume(): number {
    return num(this.data.volume);
  }
  get open(): number {
    return num(this.data.open);
  }
  get high(): number {
    return num(this.data.high);
  }
  get low(): number {
    return num(this.data.low);
  }
  get close(): number {
    return num(this.data.close);
  }
  get vwap(): number {
    return num(this.data.vwap);
  }
  get markPrice(): number {
    return num(this.data.markPrice);
  }
  get halted(): number {
    return num(this.data.halted);
  }
  /** Any field by its camelCase name; unset numbers read as NaN. */
  field(name: string): unknown {
    const value = this.data[name];
    return value === null ? Number.NaN : value;
  }
  get ticks(): T.TickData[] {
    return this.data.ticks as T.TickData[];
  }
  get domBids(): T.DOMLevel[] {
    return this.data.domBids as T.DOMLevel[];
  }
  get domAsks(): T.DOMLevel[] {
    return this.data.domAsks as T.DOMLevel[];
  }
  get modelGreeks(): T.OptionComputation | null {
    return (this.data.modelGreeks as T.OptionComputation | null) ?? null;
  }
  midpoint(): number {
    return this.core.midpoint();
  }
  marketPrice(): number {
    return this.core.marketPrice();
  }
  hasBidAsk(): boolean {
    return this.core.hasBidAsk();
  }
}

export interface TradeData {
  contract: T.Contract;
  order: T.Order;
  orderStatus: T.OrderStatus;
  fills: T.Fill[];
  log: T.TradeLogEntry[];
  advancedError: string;
}

type TradeEvents = {
  status: [Trade];
  modify: [Trade];
  cancel: [Trade];
  filled: [Trade];
  cancelled: [Trade];
  fill: [Trade, T.Fill];
  commissionReport: [Trade, T.Fill, T.CommissionReport];
};

/** A live object whose data the core updates (trades, bars, scans, PnL). */
export class Live<D, E extends Record<string, unknown[]>, C extends { snapshot(): Json } = { snapshot(): Json }> extends EventEmitter<E> {
  constructor(
    readonly core: C,
    public data: D,
  ) {
    super();
  }
  /** Read the current state from the core. */
  refresh(): this {
    this.data = this.core.snapshot();
    return this;
  }
}

/** Source of a trade's state: the connection that forwards its changes. */
interface TradeSource {
  tradeState(trade: TradeCore): { seq: number; data: TradeData };
}

/** A trade change in an event: the full state on the first send, then the
 * changed fields (`data`) and the new items of lists that grew (`append`). */
interface TradeRef {
  id: number;
  seq: number;
  full?: boolean;
  data: Partial<TradeData>;
  append?: Partial<Record<keyof TradeData, unknown[]>>;
}

/** An order with its contract, status, fills and log. Events carry only
 * the changes, merged into `data` in order (`seq`). */
export class Trade extends Live<TradeData, TradeEvents, TradeCore> {
  /** Sequence number of the state in `data`. */
  seq: number;
  constructor(
    core: TradeCore,
    private readonly source: TradeSource,
  ) {
    const state = source.tradeState(core);
    super(core, state.data);
    this.seq = state.seq;
  }
  /** Read the current state from the core now. */
  override refresh(): this {
    const state = this.source.tradeState(this.core);
    this.data = state.data;
    this.seq = state.seq;
    return this;
  }
  /** Apply a change from an event. A change that is not newer than `data`
   * (read by `refresh`) is already in it. */
  apply(ref: TradeRef): void {
    if (ref.seq <= this.seq) return;
    if (ref.full) {
      this.data = ref.data as TradeData;
    } else {
      Object.assign(this.data, ref.data);
      for (const [key, items] of Object.entries(ref.append ?? {})) {
        (this.data[key as keyof TradeData] as unknown[]).push(...(items as unknown[]));
      }
    }
    this.seq = ref.seq;
  }
  get contract(): T.Contract {
    return this.data.contract;
  }
  get order(): T.Order {
    return this.data.order;
  }
  get orderStatus(): T.OrderStatus {
    return this.data.orderStatus;
  }
  get fills(): T.Fill[] {
    return this.data.fills;
  }
  get log(): T.TradeLogEntry[] {
    return this.data.log;
  }
  get advancedError(): string {
    return this.data.advancedError;
  }
  isWaiting(): boolean {
    return this.core.isWaiting();
  }
  isWorking(): boolean {
    return this.core.isWorking();
  }
  isActive(): boolean {
    return this.core.isActive();
  }
  isDone(): boolean {
    return this.core.isDone();
  }
  filled(): number {
    return this.core.filled();
  }
  remaining(): number {
    return this.core.remaining();
  }
}

export interface BarDataListData {
  reqId: number;
  contract: T.Contract;
  bars: T.BarData[];
  keepUpToDate: boolean;
  [field: string]: unknown;
}

/** A bar list that the core extends. */
class LiveBars<B, D extends { reqId: number; bars: B[] }, E extends Record<string, unknown[]>> extends Live<D, E, BarsCore> {
  get bars(): B[] {
    return this.data.bars;
  }
  get reqId(): number {
    return this.data.reqId;
  }
  /** Read the last bar and the new ones from the core; the bars before
   * the last one never change. */
  update(): this {
    const bars = this.data.bars;
    const from = Math.max(0, bars.length - 1);
    const tail = this.core.barsFrom(from);
    if (tail.len < from) return this.refresh();
    bars.length = from;
    for (const bar of tail.bars) bars.push(bar);
    return this;
  }
}

/** Historical bars; with keepUpToDate the list grows and emits `update`. */
export class BarDataList extends LiveBars<T.BarData, BarDataListData, { update: [BarDataList, boolean] }> {}

export interface RealTimeBarListData {
  reqId: number;
  contract: T.Contract;
  bars: T.RealTimeBar[];
  [field: string]: unknown;
}

/** 5-second real-time bars; emits `update` per bar. */
export class RealTimeBarList extends LiveBars<T.RealTimeBar, RealTimeBarListData, { update: [RealTimeBarList, boolean] }> {}

export interface ScanDataListData {
  reqId: number;
  subscription: T.ScannerSubscription;
  rows: T.ScanData[];
  [field: string]: unknown;
}

/** Scanner subscription results; emits `update` per scan. */
export class ScanDataList extends Live<ScanDataListData, { update: [ScanDataList] }> {
  get rows(): T.ScanData[] {
    return this.data.rows;
  }
  get reqId(): number {
    return this.data.reqId;
  }
}

/** Live account PnL (`reqPnL`). */
export class PnLSubscription extends Live<T.PnL, Record<string, never>> {}
/** Live position PnL (`reqPnLSingle`). */
export class PnLSingleSubscription extends Live<T.PnLSingle, Record<string, never>> {}

export interface IBEvents {
  connected: [];
  disconnected: [reason: string];
  update: [];
  pendingTickers: [tickers: Ticker[]];
  barUpdate: [bars: BarDataList | RealTimeBarList, hasNewBar: boolean];
  newOrder: [trade: Trade];
  orderModify: [trade: Trade];
  cancelOrder: [trade: Trade];
  openOrder: [trade: Trade];
  orderStatus: [trade: Trade];
  execDetails: [trade: Trade, fill: T.Fill];
  commissionReport: [trade: Trade, fill: T.Fill, report: T.CommissionReport];
  updatePortfolio: [item: T.PortfolioItem];
  position: [position: T.Position];
  accountValue: [value: T.AccountValue];
  accountSummary: [value: T.AccountValue];
  pnl: [pnl: T.PnL];
  pnlSingle: [pnl: T.PnLSingle];
  scannerData: [scan: ScanDataList];
  tickNews: [news: T.NewsTick];
  newsBulletin: [bulletin: T.NewsBulletin];
  wshMetaEvent: [data: string];
  wshEvent: [data: string];
  /** IB's error and warning messages (ib_async `errorEvent`). Not named
   * `error`, which Node throws when nobody listens. */
  apiError: [reqId: number, code: number, message: string, contract: T.Contract | null];
  timeout: [idlePeriod: number];
  /** Every decoded TWS message; only with `connect({ emitMessages: true })`. */
  message: [name: string, fields: Json];
}


/** Order events: the name the Trade emits and the name the IB emits. */
const TRADE_EVENTS = {
  newOrder: [null, 'newOrder'],
  orderModify: ['modify', 'orderModify'],
  cancelOrder: ['cancel', 'cancelOrder'],
  openOrder: [null, 'openOrder'],
  orderStatus: ['status', 'orderStatus'],
  filled: ['filled', null],
  cancelled: ['cancelled', null],
} as const;

/**
 * TWS API client with ib_async's methods and events.
 *
 * ```ts
 * const ib = new IB();
 * await ib.connect({ port: 7497, clientId: 1 });
 * const [aapl] = await ib.qualifyContracts(Stock('AAPL', 'SMART', 'USD'));
 * const trade = ib.placeOrder(aapl, LimitOrder('BUY', 100, 150));
 * trade.on('status', (t) => console.log(t.orderStatus.status));
 * ```
 */
export class IB extends EventEmitter<IBEvents> {
  private c: IbCore | null = null;
  private readonly tradeMap = new Map<number, Trade>();
  private readonly tickerMap = new Map<number, Ticker>();
  private readonly lists = new Map<number, BarDataList | RealTimeBarList | ScanDataList>();
  private pending: Ticker[] = [];
  private wshMetaReqId = 0;
  private wshEventReqId = 0;

  private get core(): IbCore {
    if (this.c === null) throw new Error('IB is not connected; call connect() first.');
    return this.c;
  }

  /** Connect to TWS or IB Gateway and synchronize state. */
  async connect(options: ConnectOptions = {}): Promise<this> {
    const { fetchFields = StartupFetch.All, timeout = 4, requestTimeout = 0, ...rest } = options;
    const core = await guarded(
      native.ibConnect({ host: '127.0.0.1', port: 7497, clientId: 1, ...rest, timeout, requestTimeout, fetchFields }),
    );
    this.c = core;
    void this.pump(core);
    this.emit('connected');
    return this;
  }

  disconnect(): void {
    this.c?.disconnect();
  }

  isConnected(): boolean {
    return this.c?.isConnected() ?? false;
  }

  /** Low-level client on this connection (its messages arrive as `message` events). */
  get client(): Client {
    return new Client(this.core.client());
  }

  /** Wait until the next event batch has been handled; `false` on timeout or disconnect. */
  waitOnUpdate(timeoutMs = 0): Promise<boolean> {
    if (!this.isConnected()) return Promise.resolve(false);
    return new Promise((resolve) => {
      const timer = timeoutMs > 0 ? setTimeout(() => done(false), timeoutMs) : undefined;
      const done = (value: boolean) => {
        clearTimeout(timer);
        this.off('update', onUpdate);
        this.off('disconnected', onDisconnected);
        resolve(value);
      };
      const onUpdate = () => done(true);
      const onDisconnected = () => done(false);
      this.on('update', onUpdate);
      this.on('disconnected', onDisconnected);
    });
  }

  /** Emit `timeout` once no message arrived for `seconds`; 0 stops. */
  setTimeout(seconds: number): Promise<void> {
    return guarded(this.core.setTimeout(seconds));
  }

  private async pump(core: IbCore): Promise<void> {
    while (this.c === core) {
      let events: Json[];
      try {
        events = await core.nextEvents(250);
      } catch {
        break;
      }
      for (const event of events) {
        // A listener that throws must not end the pump (and with it every
        // later event), as an unhandled rejection would.
        try {
          this.dispatch(event);
        } catch (error) {
          console.error('ibkit: an event listener threw', error);
        }
      }
      if (!core.isConnected() && events.length === 0) break;
    }
  }

  private wrapTrade(core: TradeCore): Trade {
    const id = core.id();
    let trade = this.tradeMap.get(id);
    if (trade === undefined) {
      trade = new Trade(core, this.core);
      this.tradeMap.set(id, trade);
    } else {
      trade.refresh();
    }
    return trade;
  }

  private wrapTicker(core: TickerCore): Ticker {
    const id = core.id();
    let ticker = this.tickerMap.get(id);
    if (ticker === undefined) {
      ticker = new Ticker(core, this.core);
      this.tickerMap.set(id, ticker);
    } else {
      ticker.refresh();
    }
    return ticker;
  }

  private tradeOf(ref: TradeRef): Trade | null {
    let trade = this.tradeMap.get(ref.id);
    if (trade === undefined) {
      const core = this.core.tradeById(ref.id);
      if (core === null) return null;
      trade = new Trade(core, this.core);
      this.tradeMap.set(ref.id, trade);
    }
    trade.apply(ref);
    return trade;
  }

  private dispatch(e: Json): void {
    const names = TRADE_EVENTS[e.type as keyof typeof TRADE_EVENTS];
    if (names !== undefined) {
      const trade = this.tradeOf(e.trade);
      if (trade === null) return;
      const [own, ib] = names;
      if (own !== null) trade.emit(own, trade);
      if (ib !== null) this.emit(ib, trade);
      return;
    }
    switch (e.type) {
      case 'connected':
        return;
      case 'disconnected':
        this.emit('disconnected', e.reason);
        return;
      case 'update':
        this.emit('update');
        return;
      case 'pendingTickers': {
        const tickers: Ticker[] = [];
        for (const ref of e.tickers as { id: number; seq: number; full?: boolean; data: TickerData }[]) {
          let ticker = this.tickerMap.get(ref.id);
          if (ticker === undefined) {
            const core = this.core.tickerById(ref.id);
            if (core === null) continue;
            ticker = this.wrapTicker(core);
          }
          // A delta older than the state read by refresh() is already in it.
          if (ref.seq > ticker.seq) {
            ticker.data = ref.full ? ref.data : Object.assign(ticker.data, ref.data);
            ticker.seq = ref.seq;
          }
          tickers.push(ticker);
        }
        this.pending = tickers;
        for (const ticker of tickers) ticker.emit('update', ticker);
        this.emit('pendingTickers', tickers);
        return;
      }
      case 'execDetails': {
        const trade = this.tradeOf(e.trade);
        if (trade === null) return;
        trade.emit('fill', trade, e.fill);
        this.emit('execDetails', trade, e.fill);
        return;
      }
      case 'commissionReport': {
        const trade = this.tradeOf(e.trade);
        if (trade === null) return;
        trade.emit('commissionReport', trade, e.fill, e.report);
        this.emit('commissionReport', trade, e.fill, e.report);
        return;
      }
      case 'barUpdate': {
        const bars = this.lists.get(e.reqId);
        if (bars instanceof BarDataList || bars instanceof RealTimeBarList) {
          bars.update();
          (bars as BarDataList).emit('update', bars as BarDataList, e.hasNewBar);
          this.emit('barUpdate', bars, e.hasNewBar);
        }
        return;
      }
      case 'scannerData': {
        const scan = this.lists.get(e.reqId);
        if (scan instanceof ScanDataList) {
          scan.refresh();
          scan.emit('update', scan);
          this.emit('scannerData', scan);
        }
        return;
      }
      case 'updatePortfolio':
        this.emit('updatePortfolio', e.item);
        return;
      case 'position':
        this.emit('position', e.position);
        return;
      case 'accountValue':
        this.emit('accountValue', e.value);
        return;
      case 'accountSummary':
        this.emit('accountSummary', e.value);
        return;
      case 'pnl':
        this.emit('pnl', e.pnl);
        return;
      case 'pnlSingle':
        this.emit('pnlSingle', e.pnl);
        return;
      case 'tickNews':
        this.emit('tickNews', e.news);
        return;
      case 'newsBulletin':
        this.emit('newsBulletin', e.bulletin);
        return;
      case 'wshMeta':
        this.emit('wshMetaEvent', e.data);
        return;
      case 'wshEvent':
        this.emit('wshEvent', e.data);
        return;
      case 'error':
        this.emit('apiError', e.reqId, e.code, e.message, e.contract ?? null);
        return;
      case 'timeout':
        this.emit('timeout', e.idlePeriod);
        return;
      case 'message':
        this.emit('message', e.message[0], e.message[1]);
        return;
    }
  }

  // ---- state ------------------------------------------------------------------

  managedAccounts(): string[] {
    return this.core.managedAccounts();
  }
  accountValues(account = ''): T.AccountValue[] {
    return this.core.call('accountValues', [account]);
  }
  accountSummary(account = ''): T.AccountValue[] {
    return this.core.call('accountSummary', [account]);
  }
  portfolio(account = ''): T.PortfolioItem[] {
    return this.core.call('portfolio', [account]);
  }
  positions(account = ''): T.Position[] {
    return this.core.call('positions', [account]);
  }
  pnl(account = '', modelCode = ''): T.PnL[] {
    return this.core.call('pnl', [account, modelCode]);
  }
  pnlSingle(account = '', modelCode = '', conId = 0): T.PnLSingle[] {
    return this.core.call('pnlSingle', [account, modelCode, conId]);
  }
  trades(): Trade[] {
    return this.core.trades().map((c) => this.wrapTrade(c));
  }
  openTrades(): Trade[] {
    return this.core.openTrades().map((c) => this.wrapTrade(c));
  }
  orders(): T.Order[] {
    return this.trades().map((t) => t.order);
  }
  openOrders(): T.Order[] {
    return this.openTrades().map((t) => t.order);
  }
  fills(): T.Fill[] {
    return this.core.call('fills', []);
  }
  executions(): T.Execution[] {
    return this.fills().map((f) => f.execution);
  }
  ticker(contract: ContractInput): Ticker | null {
    const core = guard(() => this.core.ticker(contract));
    return core === null ? null : this.wrapTicker(core);
  }
  tickers(): Ticker[] {
    return this.core.tickers().map((c) => this.wrapTicker(c));
  }
  pendingTickers(): Ticker[] {
    return [...this.pending];
  }
  realtimeBars(): (BarDataList | RealTimeBarList)[] {
    return [...this.lists.values()].filter(
      (b): b is BarDataList | RealTimeBarList => b instanceof BarDataList || b instanceof RealTimeBarList,
    );
  }
  newsTicks(): T.NewsTick[] {
    return this.core.call('newsTicks', []);
  }
  newsBulletins(): T.NewsBulletin[] {
    return this.core.call('newsBulletins', []);
  }
  connectionStats(): ConnectionStats {
    return this.core.connectionStats();
  }

  // ---- orders -----------------------------------------------------------------

  /** Place a new order, or modify one (same orderId). Sets `order.orderId`. */
  placeOrder(contract: ContractInput, order: OrderInput): Trade {
    const trade = this.wrapTrade(guard(() => this.core.placeOrder(contract, order)));
    order.orderId = trade.order.orderId;
    order.clientId = trade.order.clientId;
    return trade;
  }
  cancelOrder(order: OrderInput, manualCancelOrderTime = ''): Trade | null {
    const core = guard(() => this.core.cancelOrder(order, manualCancelOrderTime));
    return core === null ? null : this.wrapTrade(core);
  }
  reqGlobalCancel(): void {
    guard(() => this.core.call('reqGlobalCancel', []));
  }
  /** Parent limit order with a take-profit and a stop-loss child. */
  bracketOrder(
    action: string,
    quantity: number,
    limitPrice: number,
    takeProfitPrice: number,
    stopLossPrice: number,
    fields: OrderInput = {},
  ): BracketOrder {
    const [parent, takeProfit, stopLoss] = guard(() =>
      this.core.call('bracketOrder', [action, quantity, limitPrice, takeProfitPrice, stopLossPrice]),
    ) as T.Order[];
    for (const o of [parent, takeProfit, stopLoss]) Object.assign(o as T.Order, fields);
    return { parent: parent as T.Order, takeProfit: takeProfit as T.Order, stopLoss: stopLoss as T.Order };
  }
  whatIfOrder(contract: ContractInput, order: OrderInput): Promise<T.OrderState> {
    return this.request('whatIfOrder', contract, order);
  }
  async reqOpenOrders(): Promise<Trade[]> {
    return (await guarded(this.core.reqOpenOrders(false))).map((c) => this.wrapTrade(c));
  }
  async reqAllOpenOrders(): Promise<Trade[]> {
    return (await guarded(this.core.reqOpenOrders(true))).map((c) => this.wrapTrade(c));
  }
  async reqCompletedOrders(apiOnly = false): Promise<Trade[]> {
    return (await guarded(this.core.reqCompletedOrders(apiOnly))).map((c) => this.wrapTrade(c));
  }
  reqAutoOpenOrders(autoBind = true): void {
    guard(() => this.core.call('reqAutoOpenOrders', [autoBind]));
  }
  reqExecutions(execFilter: Partial<T.ExecutionFilter> = {}): Promise<T.Fill[]> {
    return this.request('reqExecutions', execFilter);
  }

  // ---- account ----------------------------------------------------------------

  reqPositions(): Promise<T.Position[]> {
    return this.request('reqPositions');
  }
  reqAccountUpdates(account = ''): Promise<void> {
    return this.request('reqAccountUpdates', account);
  }
  reqAccountUpdatesMulti(account = '', modelCode = ''): Promise<void> {
    return this.request('reqAccountUpdatesMulti', account, modelCode);
  }
  reqAccountSummary(): Promise<T.AccountValue[]> {
    return this.request('reqAccountSummary');
  }
  reqPnL(account: string, modelCode = ''): PnLSubscription {
    const core: PnLCore = guard(() => this.core.reqPnl(account, modelCode));
    return new PnLSubscription(core, core.snapshot());
  }
  cancelPnL(account: string, modelCode = ''): void {
    guard(() => this.core.call('cancelPnL', [account, modelCode]));
  }
  reqPnLSingle(account: string, modelCode: string, conId: number): PnLSingleSubscription {
    const core: PnLSingleCore = guard(() => this.core.reqPnlSingle(account, modelCode, conId));
    return new PnLSingleSubscription(core, core.snapshot());
  }
  cancelPnLSingle(account: string, modelCode: string, conId: number): void {
    guard(() => this.core.call('cancelPnLSingle', [account, modelCode, conId]));
  }

  // ---- contracts ----------------------------------------------------------------

  reqContractDetails(contract: ContractInput): Promise<T.ContractDetails[]> {
    return this.request('reqContractDetails', contract);
  }
  /** Fill in conId and the other fields of each contract in place; returns
   * the contracts IB found (unknown or ambiguous ones are left out). */
  async qualifyContracts(...contracts: ContractInput[]): Promise<T.Contract[]> {
    const result: (T.Contract | null)[] = await this.request('qualifyContracts', contracts);
    const out: T.Contract[] = [];
    result.forEach((q, i) => {
      if (q !== null) out.push(Object.assign(contracts[i] as ContractInput, q) as T.Contract);
    });
    return out;
  }
  reqMatchingSymbols(pattern: string): Promise<T.ContractDescription[]> {
    return this.request('reqMatchingSymbols', pattern);
  }
  reqMarketRule(marketRuleId: number): Promise<T.PriceIncrement[]> {
    return this.request('reqMarketRule', marketRuleId);
  }
  reqSecDefOptParams(
    underlyingSymbol: string,
    futFopExchange: string,
    underlyingSecType: string,
    underlyingConId: number,
  ): Promise<T.OptionChain[]> {
    return this.request('reqSecDefOptParams', underlyingSymbol, futFopExchange, underlyingSecType, underlyingConId);
  }

  // ---- market data --------------------------------------------------------------

  /** 1 live, 2 frozen, 3 delayed, 4 delayed frozen. */
  reqMarketDataType(marketDataType: number): void {
    guard(() => this.core.call('reqMarketDataType', [marketDataType]));
  }
  reqMktData(
    contract: ContractInput,
    genericTickList = '',
    snapshot = false,
    regulatorySnapshot = false,
    mktDataOptions?: T.TagValue[],
  ): Ticker {
    return this.wrapTicker(
      guard(() => this.core.reqMktData(contract, genericTickList, snapshot, regulatorySnapshot, mktDataOptions ?? [])),
    );
  }
  cancelMktData(contract: ContractInput): boolean {
    return guard(() => this.core.call('cancelMktData', [contract]));
  }
  /** Snapshot tickers of the contracts. */
  async reqTickers(...contracts: ContractInput[]): Promise<Ticker[]> {
    return (await guarded(this.core.reqTickers(contracts, false))).map((c) => this.wrapTicker(c));
  }
  reqTickByTickData(contract: ContractInput, tickType: string, numberOfTicks = 0, ignoreSize = false): Ticker {
    return this.wrapTicker(guard(() => this.core.reqTickByTickData(contract, tickType, numberOfTicks, ignoreSize)));
  }
  cancelTickByTickData(contract: ContractInput, tickType: string): boolean {
    return guard(() => this.core.call('cancelTickByTickData', [contract, tickType]));
  }
  reqMktDepth(contract: ContractInput, numRows = 5, isSmartDepth = false, mktDepthOptions?: T.TagValue[]): Ticker {
    return this.wrapTicker(guard(() => this.core.reqMktDepth(contract, numRows, isSmartDepth, mktDepthOptions ?? [])));
  }
  cancelMktDepth(contract: ContractInput, isSmartDepth = false): boolean {
    return guard(() => this.core.call('cancelMktDepth', [contract, isSmartDepth]));
  }
  reqMktDepthExchanges(): Promise<T.DepthMktDataDescription[]> {
    return this.request('reqMktDepthExchanges');
  }
  reqSmartComponents(bboExchange: string): Promise<T.SmartComponent[]> {
    return this.request('reqSmartComponents', bboExchange);
  }

  // ---- historical data ------------------------------------------------------------

  async reqHistoricalData(
    contract: ContractInput,
    endDateTime: string | Date,
    durationStr: string,
    barSizeSetting: string,
    whatToShow: string,
    useRTH: boolean,
    formatDate = 1,
    keepUpToDate = false,
    chartOptions?: T.TagValue[],
  ): Promise<BarDataList> {
    const core: BarsCore = await guarded(
      this.core.reqHistoricalData(
        contract,
        formatIBDatetime(endDateTime),
        durationStr,
        barSizeSetting,
        whatToShow,
        useRTH,
        formatDate,
        keepUpToDate,
        chartOptions ?? [],
      ),
    );
    const bars = new BarDataList(core, core.snapshot());
    if (keepUpToDate) this.lists.set(bars.reqId, bars);
    return bars;
  }
  cancelHistoricalData(bars: BarDataList): void {
    guard(() => this.core.cancelHistoricalData(bars.core as BarsCore));
    this.lists.delete(bars.reqId);
  }
  reqHistoricalSchedule(
    contract: ContractInput,
    numDays: number,
    endDateTime: string | Date = '',
    useRTH = true,
  ): Promise<T.HistoricalSchedule> {
    return this.request('reqHistoricalSchedule', contract, numDays, formatIBDatetime(endDateTime), useRTH);
  }
  reqHistoricalTicks(
    contract: ContractInput,
    startDateTime: string | Date,
    endDateTime: string | Date,
    numberOfTicks: number,
    whatToShow: string,
    useRth: boolean,
    ignoreSize = false,
    miscOptions?: T.TagValue[],
  ): Promise<(T.HistoricalTick | T.HistoricalTickBidAsk | T.HistoricalTickLast)[]> {
    return this.request(
      'reqHistoricalTicks',
      contract,
      formatIBDatetime(startDateTime),
      formatIBDatetime(endDateTime),
      numberOfTicks,
      whatToShow,
      useRth,
      ignoreSize,
      miscOptions ?? [],
    );
  }
  reqHeadTimeStamp(contract: ContractInput, whatToShow: string, useRTH: boolean, formatDate = 1): Promise<string> {
    return this.request('reqHeadTimeStamp', contract, whatToShow, useRTH, formatDate);
  }
  reqHistogramData(contract: ContractInput, useRTH: boolean, period: string): Promise<T.HistogramData[]> {
    return this.request('reqHistogramData', contract, useRTH, period);
  }
  reqRealTimeBars(
    contract: ContractInput,
    barSize: number,
    whatToShow: string,
    useRTH: boolean,
    realTimeBarsOptions?: T.TagValue[],
  ): RealTimeBarList {
    const core: RealTimeBarsCore = guard(() =>
      this.core.reqRealTimeBars(contract, barSize, whatToShow, useRTH, realTimeBarsOptions ?? []),
    );
    const bars = new RealTimeBarList(core, core.snapshot());
    this.lists.set(bars.reqId, bars);
    return bars;
  }
  cancelRealTimeBars(bars: RealTimeBarList): void {
    guard(() => this.core.cancelRealTimeBars(bars.core as RealTimeBarsCore));
    this.lists.delete(bars.reqId);
  }

  // ---- scanners -------------------------------------------------------------------

  reqScannerData(
    subscription: Partial<T.ScannerSubscription>,
    scannerSubscriptionOptions?: T.TagValue[],
    scannerSubscriptionFilterOptions?: T.TagValue[],
  ): Promise<T.ScanData[]> {
    return this.request(
      'reqScannerData',
      subscription,
      scannerSubscriptionOptions ?? [],
      scannerSubscriptionFilterOptions ?? [],
    );
  }
  reqScannerSubscription(
    subscription: Partial<T.ScannerSubscription>,
    scannerSubscriptionOptions?: T.TagValue[],
    scannerSubscriptionFilterOptions?: T.TagValue[],
  ): ScanDataList {
    const core: ScanCore = guard(() =>
      this.core.reqScannerSubscription(
        subscription,
        scannerSubscriptionOptions ?? [],
        scannerSubscriptionFilterOptions ?? [],
      ),
    );
    const scan = new ScanDataList(core, core.snapshot());
    this.lists.set(scan.reqId, scan);
    return scan;
  }
  cancelScannerSubscription(scan: ScanDataList): void {
    guard(() => this.core.cancelScannerSubscription(scan.core as ScanCore));
    this.lists.delete(scan.reqId);
  }
  /** Scanner parameters XML. */
  reqScannerParameters(): Promise<string> {
    return this.request('reqScannerParameters');
  }

  // ---- options ----------------------------------------------------------------------

  calculateImpliedVolatility(
    contract: ContractInput,
    optionPrice: number,
    underPrice: number,
    implVolOptions?: T.TagValue[],
  ): Promise<T.OptionComputation | null> {
    return this.request('calculateImpliedVolatility', contract, optionPrice, underPrice, implVolOptions ?? []);
  }
  calculateOptionPrice(
    contract: ContractInput,
    volatility: number,
    underPrice: number,
    optPrcOptions?: T.TagValue[],
  ): Promise<T.OptionComputation | null> {
    return this.request('calculateOptionPrice', contract, volatility, underPrice, optPrcOptions ?? []);
  }
  exerciseOptions(
    contract: ContractInput,
    exerciseAction: number,
    exerciseQuantity: number,
    account: string,
    override: number,
  ): void {
    guard(() => this.core.call('exerciseOptions', [contract, exerciseAction, exerciseQuantity, account, override]));
  }

  // ---- news ---------------------------------------------------------------------------

  reqNewsProviders(): Promise<T.NewsProvider[]> {
    return this.request('reqNewsProviders');
  }
  reqNewsArticle(providerCode: string, articleId: string, newsArticleOptions?: T.TagValue[]): Promise<T.NewsArticle> {
    return this.request('reqNewsArticle', providerCode, articleId, newsArticleOptions ?? []);
  }
  reqHistoricalNews(
    conId: number,
    providerCodes: string,
    startDateTime: string | Date,
    endDateTime: string | Date,
    totalResults: number,
    historicalNewsOptions?: T.TagValue[],
  ): Promise<T.HistoricalNews[]> {
    return this.request(
      'reqHistoricalNews',
      conId,
      providerCodes,
      formatIBDatetime(startDateTime),
      formatIBDatetime(endDateTime),
      totalResults,
      historicalNewsOptions ?? [],
    );
  }
  reqNewsBulletins(allMessages: boolean): void {
    guard(() => this.core.call('reqNewsBulletins', [allMessages]));
  }
  cancelNewsBulletins(): void {
    guard(() => this.core.call('cancelNewsBulletins', []));
  }

  // ---- fundamentals, FA, WSH, configuration ----------------------------------------------

  reqFundamentalData(contract: ContractInput, reportType: string, fundamentalDataOptions?: T.TagValue[]): Promise<string> {
    return this.request('reqFundamentalData', contract, reportType, fundamentalDataOptions ?? []);
  }
  /** Financial advisor configuration XML (1 groups, 3 aliases). */
  requestFA(faDataType: number): Promise<string> {
    return this.request('requestFA', faDataType);
  }
  replaceFA(faDataType: number, xml: string): Promise<string> {
    return this.request('replaceFA', faDataType, xml);
  }
  /** Subscribe to Wall Street Horizon metadata; it arrives as `wshMetaEvent`. */
  reqWshMetaData(): void {
    if (this.wshMetaReqId) return;
    this.wshMetaReqId = this.core.nextReqId();
    this.client.reqWshMetaData(this.wshMetaReqId);
  }
  cancelWshMetaData(): void {
    if (!this.wshMetaReqId) return;
    this.client.cancelWshMetaData(this.wshMetaReqId);
    this.wshMetaReqId = 0;
  }
  /** Subscribe to Wall Street Horizon events; they arrive as `wshEvent`. */
  reqWshEventData(data: Partial<T.WshEventData>): void {
    if (this.wshEventReqId) return;
    this.wshEventReqId = this.core.nextReqId();
    this.client.reqWshEventData(this.wshEventReqId, data);
  }
  cancelWshEventData(): void {
    if (!this.wshEventReqId) return;
    this.client.cancelWshEventData(this.wshEventReqId);
    this.wshEventReqId = 0;
  }
  getWshMetaData(): Promise<string> {
    return this.request('getWshMetaData');
  }
  getWshEventData(data: Partial<T.WshEventData>): Promise<string> {
    return this.request('getWshEventData', data);
  }
  /** TWS API configuration (server 219+), IB's `ConfigResponse`. */
  reqConfig(): Promise<Json> {
    return this.request('reqConfig');
  }
  /** Change the TWS API configuration (IB's `UpdateConfigRequest` as an object). */
  updateConfig(request: Json): Promise<Json> {
    return this.request('updateConfig', request);
  }
  reqUserInfo(): Promise<string> {
    return this.request('reqUserInfo');
  }
  /** Server time in Unix seconds. */
  reqCurrentTime(): Promise<number> {
    return this.request('reqCurrentTime');
  }
  reqCurrentTimeInMillis(): Promise<number> {
    return this.request('reqCurrentTimeInMillis');
  }
  reqFamilyCodes(): Promise<T.FamilyCode[]> {
    return this.request('reqFamilyCodes');
  }
  reqSoftDollarTiers(): Promise<T.SoftDollarTier[]> {
    return this.request('reqSoftDollarTiers');
  }

  private request<R>(method: string, ...args: unknown[]): Promise<R> {
    return guarded(this.core.request(method, args));
  }
}
