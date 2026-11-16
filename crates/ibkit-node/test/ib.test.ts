// End-to-end tests of the TypeScript API against the in-process fake TWS
// server (the same scripted answers as the Python tests, server version 178).
import { afterEach, beforeEach, expect, test } from 'bun:test';
import {
  Client,
  ConnectionError,
  fix,
  IB,
  InvalidArgumentError,
  LimitOrder,
  native,
  RequestError,
  Stock,
  type Trade,
} from '../ts/index.js';
import type { FakeTwsCore } from '../ts/native.js';

const MAX = '1.7976931348623157e308';
const ACCOUNT = 'DU123456';
const m = (...fields: unknown[]) => fields.map(String);

// biome-ignore format: TWS field lists
const openOrder = (status: string) => [
  ...m(5, '{1}', 265598, 'AAPL', 'STK', '', 0, '', '', 'SMART', 'USD', 'AAPL', 'NMS', 'BUY', 100, 'LMT', 150, '', 'DAY', '', ACCOUNT, 'O', 0, '', 1, 777, 0, 0, 0, '', ''),
  ...m('', '', '', '', '', '', '', '', 0, '', -1),
  ...m(0, '', '', '', '', '', 0, 0, 0, 0, '', 0, 0, 0, '', 0, 0),
  ...m('', 0, '', '', 0, 0, '', '', '', '', '', 0, 0, 0, '', '', '', '', 0, '', '', 0, 0, ''),
  ...m(0, 0, status, '', '', '', '', '', '', '', '', '', 1.5, '', '', 'USD', ''),
  ...m(0, 0, 0, '', MAX, MAX, MAX, MAX, MAX, MAX, 0, '', '', '', MAX, 0, 0, 0, 0, '', '', 0, '', '', '', '', ''),
];
const orderStatus = (id: string, status: string, filled: number, remaining: number, avg: number) =>
  m(3, id, status, filled, remaining, avg, 777, 0, avg, 1, '', 0);
// biome-ignore format: TWS field lists
const CONTRACT_AAPL = m('AAPL', 'STK', '', 0, '', 'SMART', 'USD', 'AAPL', 'NMS', 'NMS', 265598, 0.01, 1, 'LMT', 'SMART,NASDAQ', 1, 0, 'APPLE INC', 'NASDAQ', '', 'Technology', '', '', 'US/Eastern', '', '', '', '', 0, 1, 'AAPL', '', '26', '', 'COMMON', 0.0001, 0.0001, 100);
const BARS = m(2, '20260102', 1, 2, 0.5, 1.5, 100, 1.2, 10, '20260105', 1.5, 2.5, 1, 2, 200, 1.8, 20);

// biome-ignore format: TWS field lists
const RULES = [
  { msgId: '61', reply: [m(61, 3, ACCOUNT, 8314, 'IBM', 'STK', '', 0, '', '', 'NYSE', 'USD', 'IBM', 'IBM', 10, 120.5), m(62, 1)] },
  { msgId: '5', reply: [m(53, 1)] },
  { msgId: '99', reply: [m(102)] },
  { msgId: '6', reply: [m(6, 2, 'NetLiquidation', '100000', 'USD', ACCOUNT), m(54, 1, ACCOUNT)] },
  { msgId: '76', reply: [m(74, 1, '{2}')] },
  { msgId: '7', reply: [m(55, 1, '{2}')] },
  { msgId: '9', field: 4, equals: 'XYZ', reply: [m(4, 2, '{2}', 200, 'No security definition has been found for the request', '')] },
  { msgId: '9', reply: [[...m(10, '{2}'), ...CONTRACT_AAPL], m(52, 1, '{2}')] },
  {
    msgId: '3',
    reply: [
      openOrder('Submitted'),
      orderStatus('{1}', 'Submitted', 0, 100, 0),
      m(11, -1, '{1}', 265598, 'AAPL', 'STK', '', 0, '', '', 'ISLAND', 'USD', 'AAPL', 'NMS', 'e1', '20260102 10:00:00', ACCOUNT, 'ISLAND', 'BOT', 100, 150, 777, 1, 0, 100, 150, '', '', '', '', 1, 0),
      orderStatus('{1}', 'Filled', 100, 0, 150),
      m(59, 1, 'e1', 1.0, 'USD', MAX, MAX, ''),
    ],
  },
  { msgId: '20', reply: [[...m(17, '{1}', 'a', 'b'), ...BARS]] },
  { msgId: '1', reply: [m(1, 6, '{2}', 1, 150.25, 300, 1), m(1, 6, '{2}', 2, 150.5, 200, 1), m(1, 6, '{2}', 4, 150.4, 5, 0), m(2, 6, '{2}', 8, 12345)] },
  { msgId: '49', reply: [m(49, 1, 1767225600)] },
];

let tws: FakeTwsCore;
let ib: IB;

beforeEach(async () => {
  tws = await native.fakeTws({ serverVersion: 178, nextValidId: 1000, accounts: ACCOUNT, rules: RULES });
  ib = await new IB().connect({ port: tws.port, clientId: 1, requestTimeout: 5 });
});

afterEach(() => {
  ib.disconnect();
  tws.close();
});

/** Resolve once `condition` holds, checking after every event batch. */
async function until(condition: () => boolean, timeoutMs = 5000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!condition()) {
    if (Date.now() > deadline) throw new Error('condition not reached');
    await ib.waitOnUpdate(100);
  }
}

test('connect synchronizes positions and account values', async () => {
  expect(ib.isConnected()).toBe(true);
  expect(ib.managedAccounts()).toEqual([ACCOUNT]);
  const [position] = ib.positions();
  expect([position?.contract.symbol, position?.position, position?.avgCost]).toEqual(['IBM', 10, 120.5]);
  expect(ib.accountValues().some((v) => v.tag === 'NetLiquidation' && v.value === '100000')).toBe(true);
  expect(await ib.reqCurrentTime()).toBe(1767225600);
});

test('qualifyContracts updates contracts in place and leaves out unknown ones', async () => {
  const aapl = Stock('AAPL', 'SMART', 'USD');
  const xyz = Stock('XYZ', 'SMART', 'USD');
  const qualified = await ib.qualifyContracts(aapl, xyz);
  expect(qualified).toEqual([aapl]);
  expect([aapl.conId, aapl.exchange, aapl.primaryExchange]).toEqual([265598, 'SMART', 'NASDAQ']);
  expect(xyz.conId).toBeUndefined();
  const [details] = await ib.reqContractDetails(aapl);
  expect(details?.longName).toBe('APPLE INC');
});

test('a placed order reports its status, fill and commission through events', async () => {
  const statuses: string[] = [];
  const commissions: number[] = [];
  ib.on('orderStatus', (trade) => statuses.push(trade.orderStatus.status));
  ib.on('commissionReport', (_trade, _fill, report) => commissions.push(report.commission));
  const order = LimitOrder('BUY', 100, 150);
  const trade: Trade = ib.placeOrder(Stock('AAPL', 'SMART', 'USD', { conId: 265598 }), order);
  const fills: number[] = [];
  trade.on('fill', (_t, fill) => fills.push(fill.execution.price));
  expect(order.orderId).toBeGreaterThanOrEqual(1000);
  await until(() => commissions.length > 0);
  expect(trade.orderStatus.status).toBe('Filled');
  expect(trade.isDone()).toBe(true);
  expect(trade.filled()).toBe(100);
  expect(fills).toEqual([150]);
  expect(commissions).toEqual([1]);
  expect(statuses.at(-1)).toBe('Filled');
  // Events carry only changes; merged, they equal a fresh read.
  expect(trade.fills[0]?.commissionReport.commission).toBe(1);
  expect(trade.log.map((entry) => entry.status)).toContain('Filled');
  const merged = structuredClone(trade.data);
  expect(trade.refresh().data).toEqual(merged);
  expect(ib.trades().at(-1)).toBe(trade);
  expect(ib.openTrades()).toEqual([]);
});

test('a market data ticker is live and unset numbers read as NaN', async () => {
  const aapl = Stock('AAPL', 'SMART', 'USD', { conId: 265598 });
  const ticker = ib.reqMktData(aapl);
  let updates = 0;
  ticker.on('update', () => updates++);
  await until(() => ticker.volume === 12345 && updates > 0);
  expect([ticker.bid, ticker.bidSize, ticker.ask, ticker.askSize, ticker.last]).toEqual([150.25, 300, 150.5, 200, 150.4]);
  // The fake server never sends a close price.
  expect(Number.isNaN(ticker.close)).toBe(true);
  expect(ticker.midpoint()).toBe(150.375);
  expect(ticker.marketPrice()).toBe(150.4);
  expect(updates).toBeGreaterThan(0);
  // A later tick arrives as a delta; merged in order, the deltas equal the full state.
  const before = ticker.seq;
  const request = tws.requests().find((fields) => fields[0] === '1');
  // Let earlier batches finish, so the next update belongs to this tick.
  while (await ib.waitOnUpdate(200)) {}
  tws.send(m(1, 6, request?.[2], 1, 151, 100, 1));
  // The ticker data of a batch is applied before its update event fires.
  expect(await ib.waitOnUpdate(5000)).toBe(true);
  expect(ticker.bid).toBe(151);
  expect(ticker.seq).toBeGreaterThan(before);
  const merged = structuredClone(ticker.data);
  expect(ticker.refresh().data).toEqual(merged);
  expect(ticker.data).toEqual(ticker.core.snapshot());
  expect(ib.ticker(aapl)).toBe(ticker);
  expect(ib.cancelMktData(aapl)).toBe(true);
});

test('historical data returns bars', async () => {
  const bars = await ib.reqHistoricalData(Stock('AAPL', 'SMART', 'USD', { conId: 265598 }), '', '2 D', '1 day', 'TRADES', true);
  expect(bars.bars.length).toBe(2);
  expect([bars.bars[1]?.close, bars.bars[1]?.barCount, bars.bars[1]?.average]).toEqual([2, 20, 1.8]);
});

test('a kept-up-to-date bar list changes its last bar and grows', async () => {
  const bars = await ib.reqHistoricalData(Stock('AAPL', 'SMART', 'USD', { conId: 265598 }), '', '2 D', '1 day', 'TRADES', true, 1, true);
  const updates: boolean[] = [];
  bars.on('update', (_bars, hasNewBar) => updates.push(hasNewBar));
  tws.send(m(90, bars.reqId, -1, '20260105', 1.5, 2.25, 2.5, 1, 1.8, 250));
  tws.send(m(90, bars.reqId, -1, '20260106', 2.25, 3, 3, 2, 2.5, 50));
  await until(() => updates.length === 2);
  expect(updates).toEqual([false, true]);
  expect(bars.bars.map((bar) => [bar.date, bar.close])).toEqual([
    ['20260102', 1.5],
    ['20260105', 2.25],
    ['20260106', 3],
  ]);
  const merged = structuredClone(bars.data);
  expect(bars.refresh().data).toEqual(merged);
});

test('errors arrive as typed classes', async () => {
  expect(() => ib.placeOrder(Stock('AAPL'), { totalQuantity: 'many' as never })).toThrow(InvalidArgumentError);
  // Windows retries a refused localhost connect for about 2 s before failing.
  await expect(new IB().connect({ port: 1, clientId: 9, timeout: 5 })).rejects.toBeInstanceOf(ConnectionError);
  const raising = await new IB().connect({ port: tws.port, clientId: 2, requestTimeout: 5, raiseRequestErrors: true });
  // XYZ has no security definition: with raiseRequestErrors the request fails.
  await expect(raising.reqContractDetails(Stock('XYZ', 'SMART', 'USD'))).rejects.toBeInstanceOf(RequestError);
  raising.disconnect();
}, 15_000);

test('the low-level client has every native request and receives messages', async () => {
  for (const name of native.clientRequestNames()) {
    expect(typeof (Client.prototype as unknown as Record<string, unknown>)[name]).toBe('function');
  }
  const client = await Client.connect({ port: tws.port, clientId: 3 });
  client.reqCurrentTime();
  const deadline = Date.now() + 5000;
  let time: unknown;
  while (time === undefined && Date.now() < deadline) {
    for (const [name, fields] of await client.messages(200)) if (name === 'currentTime') time = fields.time;
  }
  expect(time).toBe(1767225600);
  client.disconnect();
});

test('FIX builders follow the IB manual', () => {
  const fields = fix.newOrderSingle('c1', Stock('AAPL', 'SMART', 'USD'), { ...LimitOrder('BUY', 100, 150), account: ACCOUNT });
  const tags = new Map(fields);
  expect(tags.get(21)).toBe('2');
  expect(tags.get(1)).toBe(ACCOUNT);
  expect(tags.get(55)).toBe('AAPL');
  expect(new Map(fix.orderStatusRequest(ACCOUNT)).get(11)).toBe('*');
  expect(() => fix.optionExercise('c2', Stock('AAPL'), {}, 'jump' as never)).toThrow(InvalidArgumentError);
});
