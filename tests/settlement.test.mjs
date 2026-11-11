import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync, mkdirSync, rmSync } from 'node:fs';
import { ChainVerifier, RejectedPurchase } from '../services/settlement/verifier.js';
import { Reconciler } from '../services/settlement/reconciler.js';
import { ProcessedStore } from '../services/settlement/store.js';
import { RpcError, isUnknownBlock, httpsEndpoint, transactionsInBlock, scanBlock } from '../services/settlement/index.js';
import { SettlementSupabaseClient } from '../services/settlement/supabase.js';

const fixture = JSON.parse(readFileSync(
  new URL('./fixtures/near-settlement-rpc.json', import.meta.url), 'utf8',
));
const clone = (value) => structuredClone(value);
const listingId = '11111111-1111-4111-8111-111111111111';
const marketplaceCodeHash = 'F1LjFY79WBVaTiLJ2hExa7rTBwZJWvm38aB7prE6w1us';
const nftCodeHash = 'CzPafxtJmM1FnRoasKWAVhceJzZzkz9RKUBQQ4kY9V1v';
const blockHeights = new Map([
  ['6RWmTYhXCzjMjoY3Mz1rfFcnBm8E6XeDDbFEPUA4sv1w', 100],
  ['9SP8Y3sVADWNN5QoEB5CsvPUE5HT4o8YfBaCnhLss87K', 101],
  ['3rEx3xmgLCRgUfSgueD71YNrJYQYNvhtkXaqVQxdmj4U', 102],
  ['Ayxj8iVFTJMzZa7estoTtuBKJaUNhoipaaM7WmTjkkiS', 103],
]);
const blockHashes = new Map([...blockHeights].map(([hash, height]) => [height, hash]));
const contractListingId = 'nft.testnet|token-1|seller.testnet|1';
const purchaseSaleId = `${contractListingId}|buyer.testnet|1`;
const listingJson = '{"nft_contract_id":"nft.testnet","token_id":"token-1","seller":"seller.testnet","approval_id":1,"price":1000000000000000000000000,"fee_bps":100,"fee_account":null,"created_at":1,"cancelled_at":null,"active":false}';
const listingJsonV2 = '{"nft_contract_id":"nft.testnet","token_id":"token-1","seller":"seller.testnet","approval_id":"1","price":"1000000000000000000000000","fee_bps":100,"fee_account":"owner.testnet","created_at":"1","cancelled_at":null,"active":false}';

function queryResult(raw, blockHash) {
  return {
    block_hash: blockHash,
    block_height: blockHeights.get(blockHash),
    logs: [],
    result: [...Buffer.from(raw)],
  };
}

function makeRpc(transaction = fixture.transaction, options = {}) {
  const receipts = new Map(fixture.transaction.receipts.map((receipt) => [receipt.receipt_id, receipt]));
  return async (method, params) => {
    if (method === 'status') {
      return { chain_id: 'testnet', sync_info: { syncing: false, latest_block_height: 110 } };
    }
    if (method === 'EXPERIMENTAL_tx_status') return clone(transaction);
    if (method === 'EXPERIMENTAL_receipt') {
      if (options.missingReceipt === params.receipt_id) return null;
      return clone(receipts.get(params.receipt_id));
    }
    if (method === 'block') {
      if (params.finality === 'final') {
        return { header: { height: 110, hash: 'J3KbUXF9YPu2eGnbDCACxGvmMDZMdP7acGYhVLHGu9y2' }, chunks: [] };
      }
      const blockHash = typeof params.block_id === 'number' ? blockHashes.get(params.block_id) : params.block_id;
      return { header: { height: blockHeights.get(blockHash), hash: blockHash }, chunks: [] };
    }
    if (method === 'query' && params.request_type === 'view_account') {
      return {
        block_hash: params.block_id,
        block_height: blockHeights.get(params.block_id),
        code_hash: params.account_id === 'market.testnet' ? marketplaceCodeHash : nftCodeHash,
      };
    }
    if (method === 'query' && params.request_type === 'call_function') {
      const args = JSON.parse(Buffer.from(params.args_base64, 'base64'));
      if (params.account_id === 'market.testnet' && params.method_name === 'config') {
        if (options.currentAbi) return queryResult('{"nft_contract_id":"nft.testnet","primary_nft_contract_id":"nft.testnet","owner":"owner.testnet","fee_bps":100,"fee_account":"owner.testnet","allowed_nft_contracts":["nft.testnet"],"trading_enabled":true,"state_version":2}', params.block_id);
        return queryResult('{"nft_contract_id":"nft.testnet","owner":"owner.testnet","fee_bps":100,"fee_account":null}', params.block_id);
      }
      if (params.account_id === 'nft.testnet' && params.method_name === 'marketplace_id') {
        return queryResult('"market.testnet"', params.block_id);
      }
      if (params.account_id === 'market.testnet' && params.method_name === 'sale' && args.sale_id === purchaseSaleId) {
        if (options.currentAbi) return queryResult(`{"listing_id":"${contractListingId}","listing":${listingJsonV2},"buyer":"buyer.testnet","purchase_generation":"1","created_at":"1","settled":true,"nft_delivered":true}`, params.block_id);
        return queryResult(`{"listing_id":"${purchaseSaleId}","listing":${listingJson},"buyer":"buyer.testnet","purchase_generation":1,"created_at":1,"settled":true,"nft_delivered":true}`, params.block_id);
      }
      if (params.account_id === 'market.testnet' && params.method_name === 'listing') {
        if (options.currentAbi) return queryResult(listingJsonV2, params.block_id);
        return queryResult(listingJson, params.block_id);
      }
      if (options.currentAbi && params.account_id === 'market.testnet' && params.method_name === 'settlement') {
        return queryResult(JSON.stringify({
          sale_id: purchaseSaleId, listing_id: contractListingId, nft_contract_id: 'nft.testnet',
          token_id: 'token-1', seller: 'seller.testnet', buyer: 'buyer.testnet', approval_id: 1,
          generation: 1, price: '1000000000000000000000000',
          seller_share: '990000000000000000000000', fee_share: '10000000000000000000000',
          fee_recipient: 'owner.testnet', status: 'settled', created_at: '1', updated_at: '2',
        }), params.block_id);
      }
      if (params.account_id === 'nft.testnet' && params.method_name === 'nft_token') {
        return queryResult('{"token_id":"token-1","owner_id":"buyer.testnet","metadata":{},"approved_account_ids":{}}', params.block_id);
      }
    }
    throw new Error(`Unexpected RPC ${method} ${JSON.stringify(params)}`);
  };
}

function verifier(transaction = fixture.transaction, overrides = {}, rpcOptions = {}) {
  return new ChainVerifier({
    networkId: 'testnet',
    marketplaceContractId: 'market.testnet',
    nftContractId: 'nft.testnet',
    marketplaceCodeHash,
    nftCodeHash,
    rpc: makeRpc(transaction, rpcOptions),
    ...overrides,
  });
}

async function verifiedFacts(transaction = fixture.transaction, overrides = {}, rpcOptions = {}) {
  return verifier(transaction, overrides, rpcOptions).verifyTransaction(transaction.transaction, {
    hash: fixture.block.header.hash,
    height: fixture.block.header.height,
  });
}

function makeSupabase(responses = []) {
  const calls = [];
  const lookup = { data: [{ id: listingId }], error: null };
  const supabase = {
    from(table) {
      calls.push({ type: 'from', table, filters: [] });
      const call = calls.at(-1);
      const builder = {
        select(columns) { call.columns = columns; return builder; },
        eq(column, value) { call.filters.push([column, value]); return builder; },
        async limit(value) { call.limit = value; return lookup; },
      };
      return builder;
    },
    async rpc(name, params) {
      calls.push({ type: 'rpc', name, params });
      return responses.shift() || { data: listingId, error: null };
    },
  };
  return { supabase, calls, lookup };
}

function testPath(name) {
  const directory = `/tmp/nft-settlement-${process.pid}`;
  mkdirSync(directory, { recursive: true });
  const path = `${directory}/${name}.jsonl`;
  rmSync(path, { force: true });
  return path;
}

test('blocks with multiple chunks yield transactions from included chunks', async () => {
  const calls = [];
  const rpc = async (method, params) => {
    calls.push([method, params]);
    return clone(fixture.chunks[params.chunk_id]);
  };
  const txs = await transactionsInBlock(rpc, clone(fixture.block));
  assert.equal(txs.length, 1);
  assert.equal(txs[0].receiver_id, 'market.testnet');
  assert.equal(calls.length, 2);
});

test('only an RPC UNKNOWN_BLOCK response is classified as a skipped height', () => {
  const missing = new RpcError('missing', { cause: { name: 'UNKNOWN_BLOCK' } });
  assert.equal(isUnknownBlock(missing), true);
  assert.equal(isUnknownBlock(new RpcError('pruned', { cause: { name: 'INTERNAL_ERROR' } })), false);
  assert.equal(isUnknownBlock(new Error('missing')), false);
});

test('scanner discovers a marketplace transaction through chunks', async () => {
  const facts = Object.freeze({ txHash: fixture.transaction.transaction.hash });
  const seen = [];
  const results = await scanBlock({
    rpc: async (_method, params) => clone(fixture.chunks[params.chunk_id]),
    block: clone(fixture.block),
    verifier: {
      matchesMarketplace: (tx) => tx.receiver_id === 'market.testnet',
      async verifyTransaction(tx, inclusion) { seen.push({ tx, inclusion }); return facts; },
    },
    reconciler: { async reconcile(value) { assert.equal(value, facts); return { ok: true }; } },
  });
  assert.equal(seen.length, 1);
  assert.equal(seen[0].inclusion.hash, fixture.block.header.hash);
  assert.equal(results.length, 1);
});

test('successful FINAL receipt graph produces exact verified facts', async () => {
  const facts = await verifiedFacts();
  assert.equal(facts.contractSaleId, contractListingId);
  assert.equal(facts.purchaseSaleId, purchaseSaleId);
  assert.equal(facts.buyerWalletAddress, 'buyer.testnet');
  assert.equal(facts.sellerWalletAddress, 'seller.testnet');
  assert.equal(facts.priceYocto, '1000000000000000000000000');
  assert.equal(facts.nftReceiptId, '8ZD92cLpoCEU46hPGFfk3VqZpU8s6DoQhZ4pCMqWwDT6');
});

test('current settlement ABI authenticates approval and accounting from contract state', async () => {
  const transaction = clone(fixture.transaction);
  transaction.receipts_outcome[0].outcome.logs = [`EVENT_JSON:{"standard":"near_marketplace","version":"1.0.0","event":"sale_pending","data":{"sale_id":"${purchaseSaleId}"}}`];
  transaction.receipts_outcome[2].outcome.logs = [`EVENT_JSON:{"standard":"near_marketplace","version":"1.0.0","event":"sale_settled","data":{"sale_id":"${purchaseSaleId}"}}`];
  const callback = transaction.receipts.find((receipt) => receipt.receiver_id === 'market.testnet' &&
    receipt.predecessor_id === 'market.testnet');
  callback.receipt.Action.actions[0].FunctionCall.args = Buffer.from(JSON.stringify({ sale_id: purchaseSaleId })).toString('base64');
  const facts = await verifiedFacts(transaction, {}, { currentAbi: true });
  assert.equal(facts.approvalId, '1');
  assert.equal(facts.generation, '1');
  assert.equal(facts.priceYocto, '1000000000000000000000000');
});

test('failed receipt is rejected', async () => {
  const transaction = clone(fixture.transaction);
  transaction.receipts_outcome[1].outcome.status = { Failure: { ActionError: { index: 0, kind: 'FunctionCallError' } } };
  await assert.rejects(verifiedFacts(transaction), RejectedPurchase);
});

test('missing receipt outcome rejects an incomplete graph', async () => {
  const transaction = clone(fixture.transaction);
  transaction.receipts_outcome.splice(1, 1);
  await assert.rejects(verifiedFacts(transaction), /Incomplete receipt graph/);
});

test('missing receipt body is rejected even when its outcome exists', async () => {
  const transaction = clone(fixture.transaction);
  transaction.receipts = transaction.receipts.filter((receipt) =>
    receipt.receipt_id !== '8ZD92cLpoCEU46hPGFfk3VqZpU8s6DoQhZ4pCMqWwDT6');
  const v = verifier(transaction, {}, { missingReceipt: '8ZD92cLpoCEU46hPGFfk3VqZpU8s6DoQhZ4pCMqWwDT6' });
  await assert.rejects(v.verifyTransaction(transaction.transaction, {
    hash: fixture.block.header.hash, height: 100,
  }), /Missing receipt body/);
});

test('malformed NFT event is rejected', async () => {
  const transaction = clone(fixture.transaction);
  transaction.receipts_outcome[1].outcome.logs[0] = 'EVENT_JSON:{"standard":"nep171","event":"nft_transfer"}';
  await assert.rejects(verifiedFacts(transaction), /Malformed or unrelated NFT event/);
});

test('wrong marketplace contract is rejected', async () => {
  await assert.rejects(verifiedFacts(fixture.transaction, {
    marketplaceContractId: 'other-market.testnet',
  }), /Invalid marketplace transaction discovery/);
});

test('wrong NFT contract is rejected', async () => {
  await assert.rejects(verifiedFacts(fixture.transaction, {
    nftContractId: 'other-nft.testnet',
  }), /Invalid contract listing identity/);
});

test('reconciler accepts only verifier-produced facts and sends exact RPC facts', async () => {
  const facts = await verifiedFacts();
  const { supabase, calls } = makeSupabase();
  const reconciler = new Reconciler({
    networkId: 'testnet', marketplaceContractId: 'market.testnet', nftContractId: 'nft.testnet',
    processedPath: testPath('exact'),
  }, supabase);
  assert.equal((await reconciler.reconcile({ ...facts })).ok, false);
  const result = await reconciler.reconcile(facts);
  assert.deepEqual(result, { ok: true, duplicate: false, listingId });
  const rpc = calls.find((call) => call.type === 'rpc');
  assert.equal(rpc.name, 'reconcile_nft_purchase');
  assert.equal(rpc.params.p_listing_id, listingId);
  assert.equal(rpc.params.p_contract_sale_id, contractListingId);
  assert.equal(rpc.params.p_price_yocto, '1000000000000000000000000');
});

test('duplicate transaction replay always reaches idempotent SQL', async () => {
  const facts = await verifiedFacts();
  const { supabase, calls } = makeSupabase();
  const reconciler = new Reconciler({
    networkId: 'testnet', marketplaceContractId: 'market.testnet', nftContractId: 'nft.testnet',
    processedPath: testPath('duplicate-tx'),
  }, supabase);
  assert.equal((await reconciler.reconcile(facts)).duplicate, false);
  assert.equal((await reconciler.reconcile(facts)).duplicate, true);
  assert.equal(calls.filter((call) => call.type === 'rpc').length, 2);
});

test('restart reloads audit state but still replays through SQL', async () => {
  const facts = await verifiedFacts();
  const path = testPath('restart');
  const firstDb = makeSupabase();
  const config = {
    networkId: 'testnet', marketplaceContractId: 'market.testnet', nftContractId: 'nft.testnet',
    processedPath: path,
  };
  await new Reconciler(config, firstDb.supabase).reconcile(facts);
  const secondDb = makeSupabase();
  const replay = await new Reconciler(config, secondDb.supabase).reconcile(facts);
  assert.equal(replay.duplicate, true);
  assert.equal(secondDb.calls.filter((call) => call.type === 'rpc').length, 1);
});

test('same sale ID with a different verified transaction cannot be credited twice', async () => {
  const first = await verifiedFacts();
  const transaction = clone(fixture.transaction);
  const secondHash = 'J3KbUXF9YPu2eGnbDCACxGvmMDZMdP7acGYhVLHGu9y2';
  transaction.transaction.hash = secondHash;
  transaction.transaction_outcome.id = secondHash;
  const second = await verifier(transaction).verifyTransaction(transaction.transaction, {
    hash: fixture.block.header.hash, height: 100,
  });
  assert.equal(first.contractSaleId, second.contractSaleId);
  assert.notEqual(first.txHash, second.txHash);
  const { supabase } = makeSupabase([
    { data: listingId, error: null },
    { data: null, error: { message: 'Listing already settled with different purchase facts' } },
  ]);
  const reconciler = new Reconciler({
    networkId: 'testnet', marketplaceContractId: 'market.testnet', nftContractId: 'nft.testnet',
    processedPath: testPath('duplicate-sale'),
  }, supabase);
  assert.equal((await reconciler.reconcile(first)).ok, true);
  const conflict = await reconciler.reconcile(second);
  assert.equal(conflict.ok, false);
  assert.match(conflict.error, /already settled/);
});

test('processed store persists block checkpoint atomically', () => {
  const path = testPath('checkpoint');
  const first = new ProcessedStore(path);
  first.setCursor(103, 'Ayxj8iVFTJMzZa7estoTtuBKJaUNhoipaaM7WmTjkkiS');
  first.add(fixture.transaction.transaction.hash);
  const restarted = new ProcessedStore(path);
  assert.deepEqual(restarted.getCursor(), {
    height: 103, hash: 'Ayxj8iVFTJMzZa7estoTtuBKJaUNhoipaaM7WmTjkkiS',
  });
  assert.equal(restarted.has(fixture.transaction.transaction.hash), true);
});

test('server-only Supabase client sends service credentials only in headers', async () => {
  assert.throws(() => httpsEndpoint('NEAR_RPC_URL', 'http://rpc.testnet.near.org'), /HTTPS/);
  assert.throws(() => new SettlementSupabaseClient('http://project.supabase.co', 'secret'), /HTTPS/);
  const originalFetch = globalThis.fetch;
  const requests = [];
  globalThis.fetch = async (url, init) => {
    requests.push({ url: String(url), init });
    const body = String(url).includes('/rpc/') ? JSON.stringify(listingId) : JSON.stringify([{ id: listingId }]);
    return new Response(body, { status: 200, headers: { 'Content-Type': 'application/json' } });
  };
  try {
    const client = new SettlementSupabaseClient('https://project.supabase.co', 'service-secret');
    const lookup = await client.from('marketplace_listings').select('id')
      .eq('network_id', 'testnet').eq('marketplace_contract_id', 'market.testnet')
      .eq('contract_sale_id', contractListingId).limit(2);
    const rpc = await client.rpc('reconcile_nft_purchase', { p_listing_id: listingId });
    assert.deepEqual(lookup.data, [{ id: listingId }]);
    assert.equal(rpc.data, listingId);
    assert.equal(requests.length, 2);
    assert.ok(requests.every((request) => request.init.headers.Authorization === 'Bearer service-secret'));
    assert.ok(requests.every((request) => !request.url.includes('service-secret')));
  } finally {
    globalThis.fetch = originalFetch;
  }
});
