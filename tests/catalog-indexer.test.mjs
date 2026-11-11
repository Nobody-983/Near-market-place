import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { CatalogChainVerifier, RejectedIndexEvent } = require('../services/indexer/verifier.js');
const { CatalogReconciler } = require('../services/indexer/reconciler.js');
const { scanCatalogBlock } = require('../services/indexer/index.js');

const hashes = {
  tx: '6RWmTYhXCzjMjoY3Mz1rfFcnBm8E6XeDDbFEPUA4sv1w',
  receipt: '9SP8Y3sVADWNN5QoEB5CsvPUE5HT4o8YfBaCnhLss87K',
  inclusion: '3rEx3xmgLCRgUfSgueD71YNrJYQYNvhtkXaqVQxdmj4U',
  event: 'Ayxj8iVFTJMzZa7estoTtuBKJaUNhoipaaM7WmTjkkiS',
};
const marketplaceCodeHash = 'F1LjFY79WBVaTiLJ2hExa7rTBwZJWvm38aB7prE6w1us';
const nftCodeHash = 'CzPafxtJmM1FnRoasKWAVhceJzZzkz9RKUBQQ4kY9V1v';
const catalogId = '11111111-1111-4111-8111-111111111111';
const tokenId = `catalog-${catalogId}`;
let verifiedMintFacts;

function action(method, args, deposit = '0') {
  return { FunctionCall: { method_name: method, args: Buffer.from(JSON.stringify(args)).toString('base64'), gas: 100_000_000_000_000, deposit } };
}

function evidence(tx, executor, log) {
  return {
    final_execution_status: 'FINAL', transaction: tx, status: { SuccessValue: '' },
    transaction_outcome: { id: tx.hash, block_hash: hashes.inclusion, outcome: {
      executor_id: tx.signer_id, logs: [], receipt_ids: [hashes.receipt], status: { SuccessReceiptId: hashes.receipt },
    } },
    receipts_outcome: [{ id: hashes.receipt, block_hash: hashes.event, outcome: {
      executor_id: executor, logs: [log], receipt_ids: [], status: { SuccessValue: '' },
    } }],
  };
}

function rpcFor(tx, executor, log, views, options = {}) {
  const result = evidence(tx, executor, log);
  options.mutateEvidence?.(result);
  return async (method, params) => {
    if (method === 'EXPERIMENTAL_tx_status') return result;
    if (method === 'block') {
      if (params.finality === 'final') return { header: { hash: hashes.event, height: 102 } };
      if (params.block_id === hashes.inclusion || params.block_id === 100) {
        return { header: { hash: options.inclusionCanonicalHash || hashes.inclusion, height: 100 } };
      }
      if (params.block_id === hashes.event || params.block_id === 102) return { header: { hash: hashes.event, height: 102 } };
    }
    if (method === 'query' && params.request_type === 'view_account') {
      return { block_hash: hashes.event, block_height: 102,
        code_hash: options.codeHashes?.[params.account_id] ||
          (params.account_id === 'market.testnet' ? marketplaceCodeHash : nftCodeHash) };
    }
    if (method === 'query' && params.request_type === 'call_function') {
      const key = `${params.account_id}:${params.method_name}`;
      const value = views[key];
      assert.notEqual(value, undefined, `unexpected view ${key}`);
      return { block_hash: hashes.event, block_height: 102, logs: [], result: [...Buffer.from(JSON.stringify(value))] };
    }
    throw new Error(`unexpected RPC ${method}`);
  };
}

function verifier(rpc) {
  return new CatalogChainVerifier({
    networkId: 'testnet', rpc, marketplaceContractId: 'market.testnet', nftContractId: 'nft.testnet',
    marketplaceCodeHash, nftCodeHash,
  });
}

test('final mint evidence binds the deterministic catalog token and owner', async () => {
  const metadata = { title: 'Artwork', media: 'https://image.example/art.png', extra: JSON.stringify({ catalog_id: catalogId }) };
  const tx = { hash: hashes.tx, signer_id: 'seller.testnet', receiver_id: 'nft.testnet',
    actions: [action('nft_mint', { token_id: tokenId, token_owner_id: 'seller.testnet', token_metadata: metadata }, '50000000000000000000000')] };
  const log = `EVENT_JSON:${JSON.stringify({ standard: 'nep171', version: '1.0.0', event: 'nft_mint',
    data: [{ owner_id: 'seller.testnet', token_ids: [tokenId] }] })}`;
  const facts = await verifier(rpcFor(tx, 'nft.testnet', log, {
    'nft.testnet:nft_token': { token_id: tokenId, owner_id: 'seller.testnet', metadata },
  })).verify(tx, { hash: hashes.inclusion, height: 100 });
  assert.deepEqual({ kind: facts.kind, catalogId: facts.catalogId, tokenId: facts.tokenId, owner: facts.ownerWalletAddress },
    { kind: 'mint', catalogId, tokenId, owner: 'seller.testnet' });
  verifiedMintFacts = facts;
});

test('seller-signed approval is verified against final listing and NFT approval state', async () => {
  const terms = { standard: 'near_marketplace', version: '1.0.0', marketplace_id: 'market.testnet',
    nft_contract_id: 'nft.testnet', token_id: tokenId, seller_id: 'seller.testnet',
    price: '1000000000000000000000000', fee_bps: 100, fee_account: 'operator.testnet', expires_at: null };
  const tx = { hash: hashes.tx, signer_id: 'seller.testnet', receiver_id: 'nft.testnet',
    actions: [action('nft_approve', { token_id: tokenId, account_id: 'market.testnet', msg: JSON.stringify(terms) }, '10000000000000000000000')] };
  const listingId = `nft.testnet|${tokenId}|seller.testnet|1`;
  const log = `EVENT_JSON:${JSON.stringify({ standard: 'near_marketplace', version: '1.0.0', event: 'listing_created', data: {
    listing_id: listingId, nft_contract_id: 'nft.testnet', token_id: tokenId, seller_id: 'seller.testnet',
    approval_id: '1', generation: '1', price: terms.price, expires_at: null,
  } })}`;
  const facts = await verifier(rpcFor(tx, 'market.testnet', log, {
    'market.testnet:listing': { active: true, nft_contract_id: 'nft.testnet', token_id: tokenId,
      seller: 'seller.testnet', approval_id: '1', price: terms.price },
    'market.testnet:listing_metadata': { status: 'active', generation: '1' },
    'nft.testnet:nft_token': { token_id: tokenId, owner_id: 'seller.testnet', approved_account_ids: { 'market.testnet': '1' } },
  })).verify(tx, { hash: hashes.inclusion, height: 100 });
  assert.equal(facts.kind, 'listing');
  assert.equal(facts.contractListingId, listingId);
  assert.equal(facts.priceYocto, terms.price);
});

test('cancellation requires the final cancelled listing state', async () => {
  const listingId = `nft.testnet|${tokenId}|seller.testnet|1`;
  const tx = { hash: hashes.tx, signer_id: 'seller.testnet', receiver_id: 'market.testnet',
    actions: [action('cancel_listing', { listing_id: listingId }, '1')] };
  const log = `EVENT_JSON:${JSON.stringify({ standard: 'near_marketplace', version: '1.0.0', event: 'listing_cancelled',
    data: { listing_id: listingId, generation: '1' } })}`;
  const facts = await verifier(rpcFor(tx, 'market.testnet', log, {
    'market.testnet:listing': { active: false, seller: 'seller.testnet' },
    'market.testnet:listing_metadata': { status: 'cancelled', generation: '1' },
  })).verify(tx, { hash: hashes.inclusion, height: 100 });
  assert.equal(facts.kind, 'cancellation');
});

test('failed receipts, non-canonical execution and unreviewed code are rejected', async () => {
  const metadata = { title: 'Artwork', media: 'https://image.example/art.png' };
  const tx = { hash: hashes.tx, signer_id: 'seller.testnet', receiver_id: 'nft.testnet',
    actions: [action('nft_mint', { token_id: tokenId, token_owner_id: 'seller.testnet', token_metadata: metadata })] };
  const log = `EVENT_JSON:${JSON.stringify({ standard: 'nep171', version: '1.0.0', event: 'nft_mint',
    data: [{ owner_id: 'seller.testnet', token_ids: [tokenId] }] })}`;
  const views = { 'nft.testnet:nft_token': { token_id: tokenId, owner_id: 'seller.testnet', metadata } };
  await assert.rejects(verifier(rpcFor(tx, 'nft.testnet', log, views, {
    mutateEvidence: (value) => { value.receipts_outcome[0].outcome.status = { Failure: { ActionError: {} } }; },
  })).verify(tx, { hash: hashes.inclusion, height: 100 }), RejectedIndexEvent);
  await assert.rejects(verifier(rpcFor(tx, 'nft.testnet', log, views, {
    inclusionCanonicalHash: hashes.event,
  })).verify(tx, { hash: hashes.inclusion, height: 100 }), /Missing event execution block|not canonical/);
  await assert.rejects(verifier(rpcFor(tx, 'nft.testnet', log, views, {
    codeHashes: { 'nft.testnet': marketplaceCodeHash },
  })).verify(tx, { hash: hashes.inclusion, height: 100 }), /unreviewed contract code/);
});

test('listing rejects wrong owner, price, approval and stale final state', async () => {
  const terms = { standard: 'near_marketplace', version: '1.0.0', marketplace_id: 'market.testnet',
    nft_contract_id: 'nft.testnet', token_id: tokenId, seller_id: 'seller.testnet',
    price: '1000000000000000000000000', fee_bps: 100, fee_account: 'operator.testnet', expires_at: null };
  const tx = { hash: hashes.tx, signer_id: 'seller.testnet', receiver_id: 'nft.testnet',
    actions: [action('nft_approve', { token_id: tokenId, account_id: 'market.testnet', msg: JSON.stringify(terms) })] };
  const listingId = `nft.testnet|${tokenId}|seller.testnet|1`;
  const log = `EVENT_JSON:${JSON.stringify({ standard: 'near_marketplace', version: '1.0.0', event: 'listing_created', data: {
    listing_id: listingId, nft_contract_id: 'nft.testnet', token_id: tokenId, seller_id: 'seller.testnet',
    approval_id: '1', generation: '1', price: terms.price, expires_at: null,
  } })}`;
  const base = {
    'market.testnet:listing': { active: true, nft_contract_id: 'nft.testnet', token_id: tokenId,
      seller: 'seller.testnet', approval_id: '1', price: terms.price },
    'market.testnet:listing_metadata': { status: 'active', generation: '1' },
    'nft.testnet:nft_token': { token_id: tokenId, owner_id: 'seller.testnet', approved_account_ids: { 'market.testnet': '1' } },
  };
  const variants = [
    { ...base, 'market.testnet:listing': { ...base['market.testnet:listing'], price: '2' } },
    { ...base, 'market.testnet:listing_metadata': { status: 'cancelled', generation: '1' } },
    { ...base, 'nft.testnet:nft_token': { ...base['nft.testnet:nft_token'], owner_id: 'buyer.testnet' } },
    { ...base, 'nft.testnet:nft_token': { ...base['nft.testnet:nft_token'], approved_account_ids: { 'market.testnet': '2' } } },
  ];
  for (const views of variants) {
    await assert.rejects(verifier(rpcFor(tx, 'market.testnet', log, views)).verify(
      tx, { hash: hashes.inclusion, height: 100 }), /Final listing, approval or ownership state conflicts/);
  }
});

test('reconciler rejects caller-created facts and invokes only the verified service RPC', async () => {
  const calls = [];
  const reconciler = new CatalogReconciler({}, { rpc: async (name, params) => {
    calls.push({ name, params }); return { data: catalogId, error: null };
  } });
  assert.equal((await reconciler.reconcile({ kind: 'mint' })).ok, false);
  assert.deepEqual(calls, []);
  const result = await reconciler.reconcile(verifiedMintFacts);
  assert.equal(result.ok, true);
  assert.equal(calls[0].name, 'bind_verified_nft_mint');
  assert.equal(calls[0].params.p_catalog_id, catalogId);
});

test('database interruption fails the whole block so its checkpoint can be retried', async () => {
  const tx = { hash: hashes.tx, signer_id: 'seller.testnet', receiver_id: 'nft.testnet', actions: [] };
  const block = { header: { hash: hashes.inclusion, height: 100 }, chunks: [{
    chunk_hash: hashes.receipt, shard_id: 0, height_included: 100,
  }] };
  const rpc = async (method) => {
    assert.equal(method, 'chunk');
    return { header: { chunk_hash: hashes.receipt, height_included: 100 }, transactions: [tx] };
  };
  await assert.rejects(scanCatalogBlock({
    rpc, block,
    verifier: { matches: () => true, verify: async () => verifiedMintFacts },
    reconciler: { reconcile: async () => ({ ok: false, error: 'temporary database outage' }) },
  }), /temporary database outage/);
});
