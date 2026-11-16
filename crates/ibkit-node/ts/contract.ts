// Contract helpers (ib_async `contract.py`). Each returns a plain partial
// `Contract`; the Rust side fills the other fields with their defaults.
import type { Contract } from './types.js';

export type ContractInput = Partial<Contract>;

export function Stock(symbol = '', exchange = '', currency = '', fields: ContractInput = {}): ContractInput {
  return { secType: 'STK', symbol, exchange, currency, ...fields };
}

export function Option(
  symbol = '',
  lastTradeDateOrContractMonth = '',
  strike = 0,
  right = '',
  exchange = '',
  multiplier = '',
  currency = '',
  fields: ContractInput = {},
): ContractInput {
  return { secType: 'OPT', symbol, lastTradeDateOrContractMonth, strike, right, exchange, multiplier, currency, ...fields };
}

export function FuturesOption(...args: Parameters<typeof Option>): ContractInput {
  return { ...Option(...args), secType: 'FOP' };
}

export function Future(
  symbol = '',
  lastTradeDateOrContractMonth = '',
  exchange = '',
  localSymbol = '',
  multiplier = '',
  currency = '',
  fields: ContractInput = {},
): ContractInput {
  return { secType: 'FUT', symbol, lastTradeDateOrContractMonth, exchange, localSymbol, multiplier, currency, ...fields };
}

export function ContFuture(
  symbol = '',
  exchange = '',
  localSymbol = '',
  multiplier = '',
  currency = '',
  fields: ContractInput = {},
): ContractInput {
  return { secType: 'CONTFUT', symbol, exchange, localSymbol, multiplier, currency, ...fields };
}

/** A currency pair, for example `Forex('EURUSD')`. */
export function Forex(pair = '', exchange = 'IDEALPRO', fields: ContractInput = {}): ContractInput {
  if (pair && pair.length !== 6) {
    throw new RangeError(`Forex pair ${JSON.stringify(pair)} must have 6 letters, for example "EURUSD".`);
  }
  return { secType: 'CASH', symbol: pair.slice(0, 3), currency: pair.slice(3), exchange, ...fields };
}

const simple =
  (secType: string) =>
  (symbol = '', exchange = '', currency = '', fields: ContractInput = {}): ContractInput => ({
    secType,
    symbol,
    exchange,
    currency,
    ...fields,
  });

export const Index = simple('IND');
export const CFD = simple('CFD');
export const Commodity = simple('CMDTY');
export const Crypto = simple('CRYPTO');

export const Bond = (fields: ContractInput = {}): ContractInput => ({ secType: 'BOND', ...fields });
export const MutualFund = (fields: ContractInput = {}): ContractInput => ({ secType: 'FUND', ...fields });
export const Warrant = (fields: ContractInput = {}): ContractInput => ({ secType: 'WAR', ...fields });
export const Bag = (fields: ContractInput = {}): ContractInput => ({ secType: 'BAG', ...fields });

/** The currency pair of a CASH contract, else ''. */
export function pair(contract: ContractInput): string {
  return contract.secType === 'CASH' ? `${contract.symbol ?? ''}${contract.currency ?? ''}` : '';
}
