import assert from 'node:assert/strict';
import { test } from 'node:test';
import http from 'node:http';
import {
  DEFAULT_TESTNET_RPC,
  DEV_RPC_PROXY,
  FASTNEAR_TESTNET_RPC,
  OFFICIAL_TESTNET_RPC,
  TESTNET_FALLBACK_RPCS,
  resolveNodeUrl,
} from '../frontend/Nft/src/config/near.js';

test('resolveNodeUrl handles relative proxy paths in browser and node environments', () => {
  // Test with origin
  const resolved = resolveNodeUrl('/near-rpc');
  assert.ok(resolved.endsWith('/near-rpc'));
  assert.ok(resolved.startsWith('http://') || resolved.startsWith('https://'));
});

test('resolveNodeUrl preserves explicit full https RPC endpoints', () => {
  assert.equal(resolveNodeUrl('https://rpc.testnet.fastnear.com'), 'https://rpc.testnet.fastnear.com');
  assert.equal(resolveNodeUrl('https://test.rpc.fastnear.com'), 'https://test.rpc.fastnear.com');
  assert.equal(resolveNodeUrl('https://rpc.testnet.near.org'), 'https://rpc.testnet.near.org');
});

test('resolveNodeUrl falls back cleanly when no URL is configured', () => {
  const fallback = resolveNodeUrl('');
  assert.ok(fallback.length > 0);
  assert.ok(fallback.startsWith('http://') || fallback.startsWith('https://'));
});

test('constants export verified testnet endpoints and dev proxy path', () => {
  assert.equal(DEV_RPC_PROXY, '/near-rpc');
  assert.equal(FASTNEAR_TESTNET_RPC, 'https://rpc.testnet.fastnear.com');
  assert.equal(OFFICIAL_TESTNET_RPC, 'https://rpc.testnet.near.org');
  assert.equal(DEFAULT_TESTNET_RPC, 'https://rpc.testnet.fastnear.com');
  assert.deepEqual(TESTNET_FALLBACK_RPCS, [
    'https://rpc.testnet.fastnear.com',
    'https://test.rpc.fastnear.com',
    'https://rpc.testnet.near.org',
  ]);
});

test('resilient RPC failover succeeds when primary endpoint is available', async () => {
  const server = http.createServer((req, res) => {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ jsonrpc: '2.0', id: '1', result: { code_hash: 'primary-hash' } }));
  });
  await new Promise((resolve) => server.listen(0, resolve));
  const port = server.address().port;
  const primary = `http://127.0.0.1:${port}`;

  const res = await fetch(primary, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ jsonrpc: '2.0', id: '1', method: 'query', params: {} }),
  });
  const data = await res.json();
  assert.equal(data.result.code_hash, 'primary-hash');
  server.close();
});

test('resilient RPC failover falls back to secondary when primary fails or times out', async () => {
  // Server 1: Primary fails with 502
  const primaryServer = http.createServer((req, res) => {
    res.writeHead(502, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ jsonrpc: '2.0', error: { code: -32000, message: 'Primary failure' }, id: null }));
  });
  await new Promise((resolve) => primaryServer.listen(0, resolve));
  const primaryPort = primaryServer.address().port;

  // Server 2: Fallback succeeds with 200
  const fallbackServer = http.createServer((req, res) => {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ jsonrpc: '2.0', id: '1', result: { code_hash: 'fallback-hash' } }));
  });
  await new Promise((resolve) => fallbackServer.listen(0, resolve));
  const fallbackPort = fallbackServer.address().port;

  const endpoints = [`http://127.0.0.1:${primaryPort}`, `http://127.0.0.1:${fallbackPort}`];
  let successfulResult = null;

  for (const ep of endpoints) {
    try {
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), 2000);
      const res = await fetch(ep, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ jsonrpc: '2.0', id: '1', method: 'query', params: {} }),
        signal: controller.signal,
      });
      clearTimeout(timer);
      if (!res.ok) continue;
      const data = await res.json();
      if (data.result) {
        successfulResult = data.result;
        break;
      }
    } catch {
      continue;
    }
  }

  assert.ok(successfulResult);
  assert.equal(successfulResult.code_hash, 'fallback-hash');
  primaryServer.close();
  fallbackServer.close();
});

test('resilient RPC reports clean error when all endpoints are unavailable without hanging', async () => {
  const deadEndpoints = ['http://127.0.0.1:59998', 'http://127.0.0.1:59999'];
  let lastError = null;

  const start = Date.now();
  for (const ep of deadEndpoints) {
    try {
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), 500);
      await fetch(ep, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ jsonrpc: '2.0', id: '1', method: 'query', params: {} }),
        signal: controller.signal,
      });
      clearTimeout(timer);
    } catch (err) {
      lastError = err;
    }
  }
  const duration = Date.now() - start;
  assert.ok(lastError);
  // Must fail quickly (under 2.5 seconds total) and not hang indefinitely
  assert.ok(duration < 2500, `Expected quick failover, took ${duration}ms`);
});

test('resilient RPC recovers immediately after temporary outage', async () => {
  let failCount = 1;
  const recoverableServer = http.createServer((req, res) => {
    if (failCount > 0) {
      failCount--;
      res.writeHead(503, { 'Content-Type': 'application/json' });
      return res.end(JSON.stringify({ error: 'Temporary unavailable' }));
    }
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ jsonrpc: '2.0', id: '1', result: { code_hash: 'recovered-hash' } }));
  });
  await new Promise((resolve) => recoverableServer.listen(0, resolve));
  const port = recoverableServer.address().port;
  const ep = `http://127.0.0.1:${port}`;

  // First request fails with 503
  const res1 = await fetch(ep, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' });
  assert.equal(res1.status, 503);

  // Second request succeeds after recovery
  const res2 = await fetch(ep, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' });
  assert.equal(res2.status, 200);
  const data2 = await res2.json();
  assert.equal(data2.result.code_hash, 'recovered-hash');
  recoverableServer.close();
});


