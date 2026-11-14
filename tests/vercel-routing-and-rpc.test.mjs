import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile } from 'node:fs/promises';

test('vercel.json is valid and contains SPA rewrite and RPC proxy rewrite in both locations', async () => {
  const rootConfigRaw = await readFile(new URL('../vercel.json', import.meta.url), 'utf8');
  const frontendConfigRaw = await readFile(new URL('../frontend/Nft/vercel.json', import.meta.url), 'utf8');

  const rootConfig = JSON.parse(rootConfigRaw);
  const frontendConfig = JSON.parse(frontendConfigRaw);

  for (const [name, config] of [['root', rootConfig], ['frontend', frontendConfig]]) {
    assert.ok(Array.isArray(config.rewrites), `${name} vercel.json must have rewrites array`);

    const rpcRewrite = config.rewrites.find((r) => r.source === '/near-rpc');
    assert.ok(rpcRewrite, `${name} vercel.json must contain /near-rpc rewrite`);
    assert.equal(rpcRewrite.destination, 'https://rpc.testnet.fastnear.com');

    const spaRewrite = config.rewrites.find((r) => r.destination === '/index.html');
    assert.ok(spaRewrite, `${name} vercel.json must contain SPA rewrite to /index.html`);
  }
});

test('production NEAR Testnet RPC endpoint responds successfully to JSON-RPC requests', async () => {
  const rpcUrl = 'https://rpc.testnet.fastnear.com';
  const response = await fetch(rpcUrl, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: 'test-status',
      method: 'status',
      params: [],
    }),
  });

  assert.equal(response.status, 200);
  const data = await response.json();
  assert.equal(data.jsonrpc, '2.0');
  assert.equal(data.id, 'test-status');
  assert.equal(data.result?.chain_id, 'testnet');
  assert.ok(data.result?.sync_info?.latest_block_height > 0);
});

test('production NEAR Testnet RPC endpoint query for NFT contract code hash matches deployment', async () => {
  const rpcUrl = 'https://rpc.testnet.fastnear.com';
  const response = await fetch(rpcUrl, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: 'test-contract',
      method: 'query',
      params: {
        request_type: 'view_code',
        finality: 'final',
        account_id: 'nft-v2.roundfrog-dev.testnet',
      },
    }),
  });

  assert.equal(response.status, 200);
  const data = await response.json();
  assert.equal(data.jsonrpc, '2.0');
  assert.equal(data.result?.hash, '7Xwtpb4JsXxmcdgCNEJdQVnAUQqYjWYFAvbtK7vXexh7');
});

