// Error classes. The native addon prefixes each message with its kind.

/** Base class of every ibkit error. */
export class IbkitError extends Error {
  override name = 'IbkitError';
}

/** IB answered a request with an error message. */
export class RequestError extends IbkitError {
  override name = 'RequestError';
  constructor(
    readonly reqId: number,
    readonly code: number,
    message: string,
  ) {
    super(`API error ${code} for request ${reqId}: ${message}`);
  }
}

/** The connection is closed or could not be opened. */
export class ConnectionError extends IbkitError {
  override name = 'ConnectionError';
}

/** A request did not finish in time. */
export class TimeoutError extends IbkitError {
  override name = 'TimeoutError';
}

/** An argument has the wrong form or value. */
export class InvalidArgumentError extends IbkitError {
  override name = 'InvalidArgumentError';
}

/** Convert an error thrown by the native addon to its class. */
export function convert(error: unknown): unknown {
  if (!(error instanceof Error)) return error;
  const colon = error.message.indexOf(':');
  const kind = error.message.slice(0, colon);
  const text = error.message.slice(colon + 1);
  switch (kind) {
    case 'RequestError': {
      const r = JSON.parse(text) as { reqId: number; code: number; message: string };
      return new RequestError(r.reqId, r.code, r.message);
    }
    case 'ConnectionError':
      return new ConnectionError(text);
    case 'TimeoutError':
      return new TimeoutError(text);
    case 'InvalidArgumentError':
      return new InvalidArgumentError(text);
    case 'IbkitError':
      return new IbkitError(text);
    default:
      return error;
  }
}

/** Run `f` and convert a native error. */
export function guard<T>(f: () => T): T {
  try {
    return f();
  } catch (error) {
    throw convert(error);
  }
}

/** Await `p` and convert a native error. */
export async function guarded<T>(p: Promise<T>): Promise<T> {
  try {
    return await p;
  } catch (error) {
    throw convert(error);
  }
}
