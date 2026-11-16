// Low-level TWS client (IB's EClient/EWrapper, ib_async `Client`).
import { ClientRequests } from './client_requests.js';
import { guard, guarded } from './errors.js';
import { type ClientCore, type Json, native } from './native.js';

// Rust variant names whose IB callback name is not the lower-camel form.
const CALLBACK_NAMES: Record<string, string> = {
  ReceiveFa: 'receiveFA',
  ReplaceFaEnd: 'replaceFAEnd',
  TickEfp: 'tickEFP',
  Pnl: 'pnl',
  PnlSingle: 'pnlSingle',
  VerifyMessageApi: 'verifyMessageAPI',
  VerifyAndAuthMessageApi: 'verifyAndAuthMessageAPI',
};

/** IB callback name of a decoded message variant (`TickPrice` -> `tickPrice`). */
export function callbackName(variant: string): string {
  return CALLBACK_NAMES[variant] ?? variant.slice(0, 1).toLowerCase() + variant.slice(1);
}

export interface ClientConnectOptions {
  host?: string;
  port?: number;
  clientId?: number;
  /** Seconds. */
  timeout?: number;
  connectOptions?: string;
  optionalCapabilities?: string;
  maxRequests?: number;
  requestsInterval?: number;
}

/** Message handlers by IB callback name; `unhandled` gets the others. */
export type Wrapper = { [callback: string]: ((fields: Json) => void) | undefined } & {
  unhandled?: (name: string, fields: Json) => void;
};

/**
 * Every TWS request is a method with IB's camelCase name and argument order,
 * for example `client.reqMktData(reqId, contract, '', false, false, [])`.
 * Messages arrive as `[name, fields]` pairs with IB's callback names.
 */
export class Client extends ClientRequests {
  constructor(readonly core: ClientCore) {
    super();
  }

  static async connect(options: ClientConnectOptions = {}): Promise<Client> {
    const core = await guarded(native.clientConnect({ host: '127.0.0.1', port: 7497, clientId: 1, timeout: 4, ...options }));
    return new Client(core);
  }

  protected send(method: string, args: unknown[]): void {
    guard(() => this.core.request(method, args));
  }

  disconnect(): void {
    this.core.disconnect();
  }
  isConnected(): boolean {
    return this.core.isConnected();
  }
  serverVersion(): number {
    return this.core.serverVersion();
  }
  /** Next request or order id. */
  getReqId(): number {
    return this.core.nextReqId();
  }
  updateReqId(minReqId: number): void {
    this.core.updateReqId(minReqId);
  }
  getAccounts(): string[] {
    return this.core.managedAccounts();
  }
  twsConnectionTime(): string {
    return this.core.connectionTime();
  }

  /** Wait up to `timeoutMs` and return every available message. */
  async messages(timeoutMs = 0): Promise<[string, Json][]> {
    const batch = await guarded(this.core.poll(timeoutMs));
    return batch.map(([name, fields]) => [callbackName(name), fields]);
  }

  /** Messages until the connection closes. */
  async *[Symbol.asyncIterator](): AsyncGenerator<[string, Json]> {
    while (this.isConnected()) {
      for (const message of await this.messages(250)) yield message;
    }
  }

  /** Call the handler of each message until the connection closes. */
  async run(wrapper: Wrapper): Promise<void> {
    for await (const [name, fields] of this) {
      const handler = wrapper[name];
      if (handler) handler(fields);
      else wrapper.unhandled?.(name, fields);
    }
  }
}
