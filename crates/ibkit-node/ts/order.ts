// Order helpers (ib_async `order.py`).
import type { Order, OrderCondition } from './types.js';

export type OrderInput = Partial<Order>;

export function MarketOrder(action: string, totalQuantity: number, fields: OrderInput = {}): OrderInput {
  return { orderType: 'MKT', action, totalQuantity, ...fields };
}

export function LimitOrder(action: string, totalQuantity: number, lmtPrice: number, fields: OrderInput = {}): OrderInput {
  return { orderType: 'LMT', action, totalQuantity, lmtPrice, ...fields };
}

export function StopOrder(action: string, totalQuantity: number, stopPrice: number, fields: OrderInput = {}): OrderInput {
  return { orderType: 'STP', action, totalQuantity, auxPrice: stopPrice, ...fields };
}

export function StopLimitOrder(
  action: string,
  totalQuantity: number,
  lmtPrice: number,
  stopPrice: number,
  fields: OrderInput = {},
): OrderInput {
  return { orderType: 'STP LMT', action, totalQuantity, lmtPrice, auxPrice: stopPrice, ...fields };
}

/** Put `orders` in one OCA group: when one fills, IB cancels the others. */
export function oneCancelsAll(orders: OrderInput[], ocaGroup: string, ocaType: number): OrderInput[] {
  for (const order of orders) Object.assign(order, { ocaGroup, ocaType });
  return orders;
}

/** Condition kinds (`condType`). */
export enum ConditionType {
  Price = '1',
  Time = '3',
  Margin = '4',
  Execution = '5',
  Volume = '6',
  PercentChange = '7',
}

type Conjunction = { conjunction?: 'a' | 'o' };

export const PriceCondition = (
  c: { isMore?: boolean; price: number; conId: number; exch: string; triggerMethod?: number } & Conjunction,
): OrderCondition => ({ condType: ConditionType.Price, conjunction: 'a', isMore: true, triggerMethod: 0, ...c });

export const TimeCondition = (c: { isMore?: boolean; time: string } & Conjunction): OrderCondition => ({
  condType: ConditionType.Time,
  conjunction: 'a',
  isMore: true,
  ...c,
});

export const MarginCondition = (c: { isMore?: boolean; percent: number } & Conjunction): OrderCondition => ({
  condType: ConditionType.Margin,
  conjunction: 'a',
  isMore: true,
  ...c,
});

export const ExecutionCondition = (c: { secType: string; exch: string; symbol: string } & Conjunction): OrderCondition => ({
  condType: ConditionType.Execution,
  conjunction: 'a',
  ...c,
});

export const VolumeCondition = (
  c: { isMore?: boolean; volume: number; conId: number; exch: string } & Conjunction,
): OrderCondition => ({ condType: ConditionType.Volume, conjunction: 'a', isMore: true, ...c });

export const PercentChangeCondition = (
  c: { isMore?: boolean; changePercent: number; conId: number; exch: string } & Conjunction,
): OrderCondition => ({ condType: ConditionType.PercentChange, conjunction: 'a', isMore: true, ...c });

export interface BracketOrder {
  parent: Order;
  takeProfit: Order;
  stopLoss: Order;
}
