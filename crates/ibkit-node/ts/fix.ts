// FIX 4.2 initiator session (TCP or TLS) and IB order message builders.
//
// The builders follow the IBKR FIX Implementation Manual: HandlInst 2,
// Account (tag 1) on orders, cancels and status requests, IB's order type
// codes and custom tags. IB accepts the message types D, F, G, H, AB, AC and
// J (and sends 8, 9, B and P).
import type { ContractInput } from './contract.js';
import { guard, guarded } from './errors.js';
import { type FixCore, native } from './native.js';
import type { OrderInput } from './order.js';

export type FixFields = [tag: number, value: string][];

export type FixEvent =
  | { type: 'app'; msgType: string; fields: FixFields }
  | { type: 'session'; event: string }
  | { type: 'disconnected'; reason: string };

export interface FixConnectOptions {
  host: string;
  port: number;
  senderCompId: string;
  targetCompId?: string;
  heartbeatInterval?: number;
  resetOnLogon?: boolean;
  /** File that keeps sequence numbers and sent messages across restarts. */
  seqFile?: string;
  logonFields?: FixFields;
  /** PEM of the CA or server certificate; enables TLS. */
  tlsCaFile?: string;
  tlsServerName?: string;
  /** Seconds an incoming SendingTime may differ from the local clock. */
  maxLatency?: number | null;
  /** Send outgoing messages as FIXCOMP blocks. */
  compression?: boolean;
}

/** IB order tags without a TWS `Order` field. */
export interface OrderExtras {
  parentClOrdId?: string;
  senderLocationId?: string;
  allocations?: [account: string, quantity: number][];
  disableFaCloseFirst?: boolean;
  strictPerAccountAllocation?: boolean;
  deltaHedge?: boolean;
  fxConversion?: boolean;
  useNetPrice?: boolean;
  noDarkPools?: boolean;
  staged?: boolean;
}

const build = (kind: string, ...args: unknown[]): FixFields => guard(() => native.fixBuild(kind, args));

/** NewOrderSingle (35=D) fields from a contract and an order. */
export const newOrderSingle = (clOrdId: string, contract: ContractInput, order: OrderInput, extras: OrderExtras = {}) =>
  build('newOrderSingle', clOrdId, contract, order, extras);
/** Option exercise or lapse request. */
export const optionExercise = (
  clOrdId: string,
  contract: ContractInput,
  order: OrderInput,
  exercise: 'exercise' | 'lapse',
  revocable = false,
) => build('optionExercise', clOrdId, contract, order, exercise, revocable);
/** OrderCancelReplaceRequest (35=G). */
export const cancelReplace = (origClOrdId: string, clOrdId: string, contract: ContractInput, order: OrderInput) =>
  build('cancelReplace', origClOrdId, clOrdId, contract, order);
/** OrderCancelRequest (35=F). */
export const cancelRequest = (
  origClOrdId: string,
  clOrdId: string,
  account: string,
  contract: ContractInput,
  action: string,
  quantity: number,
) => build('cancelRequest', origClOrdId, clOrdId, account, contract, action, quantity);
/** OrderStatusRequest (35=H); `clOrdId` "*" asks for all open orders. */
export const orderStatusRequest = (account: string, clOrdId = '*') => build('orderStatusRequest', account, clOrdId);
/** NewOrderMultileg (35=AB); legs are `[contract, ratio, action, openClose]`. */
export const newOrderMultileg = (
  clOrdId: string,
  combo: ContractInput,
  legs: [ContractInput, number, string, string][],
  order: OrderInput,
) => build('newOrderMultileg', clOrdId, combo, legs, order);
/** MultilegCancelReplace (35=AC), quantity and price only. */
export const multilegCancelReplace = (origClOrdId: string, clOrdId: string, combo: ContractInput, order: OrderInput) =>
  build('multilegCancelReplace', origClOrdId, clOrdId, combo, order);
/** AllocationInstruction (35=J); allocations are `[clearing broker, quantity]`. */
export const allocationInstruction = (
  allocId: string,
  transType: 'new' | 'replace' | 'cancel',
  refAllocId: string | null,
  account: string,
  contract: ContractInput,
  action: string,
  quantity: number,
  avgPx: number,
  tradeDate: string,
  allocations: [string, number][],
) =>
  build(
    'allocationInstruction',
    allocId,
    transType,
    refAllocId,
    account,
    contract,
    action,
    quantity,
    avgPx,
    tradeDate,
    allocations,
  );

/** A logged-on FIX session. */
export class FixSession {
  constructor(readonly core: FixCore) {}

  static async connect(options: FixConnectOptions): Promise<FixSession> {
    return new FixSession(
      await guarded(native.fixConnect({ targetCompId: 'IB', heartbeatInterval: 30, maxLatency: 120, ...options })),
    );
  }

  send(msgType: string, fields: [number, string | number][]): void {
    guard(() => this.core.send(msgType, fields.map(([tag, value]) => [tag, String(value)])));
  }
  /** Send a NewOrderSingle built from a contract and an order. */
  sendOrder(clOrdId: string, contract: ContractInput, order: OrderInput, extras: OrderExtras = {}): void {
    this.send('D', newOrderSingle(clOrdId, contract, order, extras));
  }
  /** The next event, or null after `timeoutMs`. */
  poll(timeoutMs = 1000): Promise<FixEvent | null> {
    return guarded(this.core.poll(timeoutMs));
  }
  logout(text = ''): void {
    guard(() => this.core.logout(text));
  }
}
