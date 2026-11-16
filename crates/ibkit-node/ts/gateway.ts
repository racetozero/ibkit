// Client for the ibkit control API (REST and WebSocket). Plain fetch and
// WebSocket: it does not load the native addon, so `@ibkit/node/gateway`
// also runs in Deno, edge runtimes and other places without it.
import { ConnectionError, IbkitError, TimeoutError } from './errors.js';

import type { ConfigUpdate, Control, GatewayConfig, GatewayEvent, Health, Status } from './gateway_types.js';

export * from './gateway_types.js';

export const DEFAULT_URL = 'http://127.0.0.1:5800';

/** A setting value for `updateConfig`; null removes a value set through the API. */
export type SettingValue = ConfigUpdate[string];

/** The control API answered with an error status. */
export class GatewayError extends IbkitError {
  override name = 'GatewayError';
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

/** Controller events from the moment of connection. */
export class GatewayEvents {
  private readonly queue: (GatewayEvent | null)[] = [];
  private readonly waiters: ((event: GatewayEvent | null) => void)[] = [];
  private closed = false;

  constructor(private readonly socket: WebSocket) {
    socket.addEventListener('message', (message) => {
      if (typeof message.data === 'string') this.push(JSON.parse(message.data) as GatewayEvent);
    });
    socket.addEventListener('close', () => {
      this.closed = true;
      for (const waiter of this.waiters.splice(0)) waiter(null);
    });
  }

  private push(event: GatewayEvent): void {
    const waiter = this.waiters.shift();
    if (waiter) waiter(event);
    else this.queue.push(event);
  }

  /** The next event, or null on timeout or when the stream closed. */
  recv(timeoutMs?: number): Promise<GatewayEvent | null> {
    const queued = this.queue.shift();
    if (queued !== undefined) return Promise.resolve(queued);
    if (this.closed) return Promise.resolve(null);
    return new Promise((resolve) => {
      const waiter = (event: GatewayEvent | null) => {
        clearTimeout(timer);
        resolve(event);
      };
      const timer =
        timeoutMs === undefined
          ? undefined
          : setTimeout(() => {
              this.waiters.splice(this.waiters.indexOf(waiter), 1);
              resolve(null);
            }, timeoutMs);
      this.waiters.push(waiter);
    });
  }

  close(): void {
    this.socket.close();
  }

  async *[Symbol.asyncIterator](): AsyncGenerator<GatewayEvent> {
    for (;;) {
      const event = await this.recv();
      if (event === null) return;
      yield event;
    }
  }
}

/** Control API client. `token` defaults to the IBKIT_CONTROL_TOKEN variable. */
export class GatewayClient {
  readonly url: string;
  private readonly token: string;

  constructor(url = DEFAULT_URL, token = globalThis.process?.env.IBKIT_CONTROL_TOKEN) {
    if (!/^https?:\/\//.test(url)) {
      throw new IbkitError(`You gave the control API address ${JSON.stringify(url)}. Use an http:// or https:// URL, for example ${DEFAULT_URL}.`);
    }
    if (!token) {
      throw new IbkitError(
        "No control API token: pass a token or set IBKIT_CONTROL_TOKEN to the controller's control_token_env value.",
      );
    }
    this.url = url.replace(/\/+$/, '');
    this.token = token;
  }

  /** Send a request; `accept` lists error statuses whose body is a result. */
  private async send(method: string, path: string, body?: unknown, accept: number[] = []): Promise<Response> {
    let response: Response;
    try {
      response = await fetch(`${this.url}${path}`, {
        method,
        headers: {
          authorization: `Bearer ${this.token}`,
          ...(body === undefined ? {} : { 'content-type': 'application/json' }),
        },
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: AbortSignal.timeout(30000),
      });
    } catch (cause) {
      if ((cause as Error).name === 'TimeoutError') {
        throw new TimeoutError(`The control API at ${this.url} did not answer ${method} ${path} in 30 seconds. Check the controller and the network path to it.`);
      }
      throw new ConnectionError(
        `Cannot reach the control API at ${this.url} (${(cause as Error).message}). Check that the controller runs with control_listen set to this address.`,
      );
    }
    if (response.ok || accept.includes(response.status)) return response;
    const text = await response.text();
    let message = text;
    try {
      message = (JSON.parse(text) as { error?: string }).error ?? text;
    } catch {}
    throw new GatewayError(response.status, `The control API answered ${response.status}: ${message}`);
  }

  private async json<R>(method: string, path: string, body?: unknown, accept?: number[]): Promise<R> {
    return (await (await this.send(method, path, body, accept)).json()) as R;
  }

  private async empty(method: string, path: string, body?: unknown): Promise<void> {
    await (await this.send(method, path, body)).body?.cancel();
  }

  health = () => this.json<Health>('GET', '/v1/health', undefined, [503]);
  ready = () => this.json<Health>('GET', '/v1/health/ready', undefined, [503]);
  status = () => this.json<Status>('GET', '/v1/status');
  /** Prometheus text metrics. */
  metrics = async () => (await this.send('GET', '/v1/metrics')).text();
  /** The OpenAPI 3.1 document of the control API. */
  openapi = () => this.json<Record<string, unknown>>('GET', '/v1/openapi.json');
  /** Queue a command; the controller acts on it within a second. */
  control = (control: Control) => this.empty('POST', '/v1/control', { control });
  /** The effective settings (file, API settings, flags). */
  config = () => this.json<GatewayConfig>('GET', '/v1/config');
  /** Save settings (null removes a value set through the API) and start a
   * new Gateway session with them. Returns the new settings. */
  updateConfig = (values: ConfigUpdate) => this.json<GatewayConfig>('PATCH', '/v1/config', values);
  /** Remove every setting saved through the API (the file, flags and
   * variables apply again) and reload. Returns the new settings. */
  clearConfig = () => this.json<GatewayConfig>('DELETE', '/v1/config');
  /** Save the IBKR login in private files in the controller's state_dir. */
  setCredentials = (username: string, password: string) => this.empty('PUT', '/v1/credentials', { username, password });
  /** Save the base32 Mobile Authenticator secret; codes are then entered at login. */
  setTotpSecret = (secret: string) => this.empty('PUT', '/v1/totp', { secret });
  /** Return to IB Key approval on the phone. */
  clearTotpSecret = () => this.empty('DELETE', '/v1/totp');

  /** Open the event stream. Needs a WebSocket that sends headers (Node 22+,
   * Bun, Deno); a browser WebSocket cannot send the bearer token. */
  events(): Promise<GatewayEvents> {
    const url = `ws${this.url.slice('http'.length)}/v1/events`;
    // The headers option is a Node (undici), Bun and Deno extension.
    const Socket = WebSocket as unknown as new (url: string, init: { headers: Record<string, string> }) => WebSocket;
    const socket = new Socket(url, { headers: { authorization: `Bearer ${this.token}` } });
    return new Promise((resolve, reject) => {
      socket.addEventListener('open', () => resolve(new GatewayEvents(socket)), { once: true });
      socket.addEventListener(
        'error',
        () => reject(new ConnectionError(`Cannot open the control API event stream at ${url}. Check the address and the bearer token.`)),
        { once: true },
      );
    });
  }
}
