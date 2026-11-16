// WebClient against a local HTTP server, and GatewayClient against a real
// `ibkit gateway run` with a stand-in launcher.
import { expect, test } from 'bun:test';
import { mkdtempSync, writeFileSync, chmodSync, existsSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { WebClient } from '../ts/index.js';
import { Control, GatewayClient, GatewayError, GatewayState } from '../ts/gateway.js';

test('WebClient sends requests and decodes JSON', async () => {
  const seen: string[] = [];
  const server = Bun.serve({
    port: 0,
    hostname: '127.0.0.1',
    fetch(request) {
      const url = new URL(request.url);
      seen.push(`${request.method} ${url.pathname}${url.search}`);
      return Response.json({ path: url.pathname, ok: true });
    },
  });
  try {
    const web = new WebClient({ baseUrl: `http://127.0.0.1:${server.port}/v1/api` });
    expect(await web.accounts()).toEqual({ path: '/v1/api/iserver/accounts', ok: true });
    await web.searchContract('AAPL');
    await web.snapshot([265598, 8314], ['31', '84']);
    expect(seen).toEqual([
      'GET /v1/api/iserver/accounts',
      'GET /v1/api/iserver/secdef/search?symbol=AAPL',
      'GET /v1/api/iserver/marketdata/snapshot?conids=265598%2C8314&fields=31%2C84',
    ]);
  } finally {
    server.stop(true);
  }
});

const BINARY = join(import.meta.dir, '..', '..', '..', 'target', 'debug', 'ibkit');
const TOKEN = 'node-test-token-0123456789';

async function freePort(): Promise<number> {
  return new Promise((resolve) => {
    const server = createServer().listen(0, '127.0.0.1', () => {
      const port = (server.address() as { port: number }).port;
      server.close(() => resolve(port));
    });
  });
}

test.skipIf(!existsSync(BINARY))('GatewayClient controls a running controller', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'ibkit-node-gateway-'));
  const launcher = join(dir, 'launcher');
  writeFileSync(launcher, '#!/bin/sh\nexec sleep 60\n');
  chmodSync(launcher, 0o700);
  const port = await freePort();
  const config = join(dir, 'config.toml');
  writeFileSync(
    config,
    [
      `executable = ${JSON.stringify(launcher)}`,
      `agent = ${JSON.stringify(launcher)}`,
      `state_dir = ${JSON.stringify(join(dir, 'state'))}`,
      `settings_dir = ${JSON.stringify(join(dir, 'settings'))}`,
      'auto_login = false',
      'shutdown_timeout_secs = 1',
      `control_listen = "127.0.0.1:${port}"`,
      '',
    ].join('\n'),
  );
  const child = Bun.spawn([BINARY, 'gateway', '--config', config, 'run'], {
    env: { ...process.env, IBKIT_CONTROL_TOKEN: TOKEN },
    stdout: 'ignore',
  });
  try {
    const gw = new GatewayClient(`http://127.0.0.1:${port}`, TOKEN);
    const deadline = Date.now() + 10000;
    for (;;) {
      try {
        if ((await gw.health()).live) break;
      } catch {}
      if (Date.now() > deadline) throw new Error('the controller did not start');
      await Bun.sleep(50);
    }
    const status = await gw.status();
    expect(status.controller_pid).toBe(child.pid);
    expect((await gw.ready()).ready).toBe(false);
    await expect(new GatewayClient(gw.url, 'wrong-token-0123456789').status()).rejects.toBeInstanceOf(GatewayError);
    expect(await gw.metrics()).toContain('ibkit_gateway_up 1');
    expect(((await gw.openapi()).paths as Record<string, unknown>)['/v1/metrics']).toBeDefined();
    await expect(gw.setTotpSecret('123456')).rejects.toThrow(/base32/);

    const events = await gw.events();
    expect((await gw.updateConfig({ api_port: 4100 })).api_port).toBe(4100);
    for (;;) {
      const event = await events.recv(Math.max(0, deadline + 10000 - Date.now()));
      expect(event).not.toBeNull();
      if (event?.event === 'started' && event.session !== status.session) break;
    }
    events.close();
    while ((await gw.status()).state === GatewayState.Exited) await Bun.sleep(50);
    await gw.control(Control.Stop);
    expect(await child.exited).toBe(0);
  } finally {
    child.kill();
  }
  // Two sessions and their stops take about 5 s, bun's default limit.
}, 30000);
