// NEAR Marketplace Settlement Service.
// Polls NEAR RPC for marketplace transactions and reconciles verified
// on-chain outcomes with Supabase. On-chain state is authoritative.

const { ChainVerifier } = require('./verifier');
const { Reconciler } = require('./reconciler');
const { WithdrawalVerifier, WithdrawalReconciler } = require('./withdrawals');
const { ProcessedStore } = require('./store');
const { SettlementSupabaseClient } = require('./supabase');
const { check, hash, height } = require('./validation');

class RpcError extends Error {
  constructor(message, rpcError = null) {
    super(message);
    this.name = 'RpcError';
    this.rpcError = rpcError;
  }
}

function isUnknownBlock(error) {
  return error instanceof RpcError && error.rpcError?.cause?.name === 'UNKNOWN_BLOCK';
}

function env(name, required = true) {
  const v = process.env[name];
  if (required && !v) throw new Error(`Missing env: ${name}`);
  return v;
}

function httpsEndpoint(name, value) {
  const url = new URL(value);
  if (url.protocol !== 'https:') throw new Error(`${name} must use HTTPS`);
  return url.toString();
}

async function rpcJson(url, method, params) {
  try {
    const res = await fetch(url, {
      method: 'POST', signal: AbortSignal.timeout(20_000),
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', id: 'settlement', method, params }),
    });
    if (!res.ok) throw new RpcError(`RPC HTTP ${res.status}`);
    const raw = await res.text();
    if (raw.length > 20_000_000) throw new RpcError('RPC response exceeds size limit');
    const json = JSON.parse(raw);
    if (json.error) throw new RpcError(`RPC error: ${JSON.stringify(json.error)}`, json.error);
    if (!Object.hasOwn(json, 'result')) throw new RpcError('RPC result missing');
    return json.result;
  } catch (error) {
    if (error instanceof RpcError) throw error;
    throw new RpcError(error.message);
  }
}

async function transactionsInBlock(rpc, block) {
  check(hash(block?.header?.hash) && height(block.header.height) && Array.isArray(block.chunks),
    'Malformed block response');
  const transactions = [];
  for (const reference of block.chunks) {
    check(hash(reference?.chunk_hash) && Number.isSafeInteger(reference.shard_id) &&
      height(reference.height_included), 'Malformed chunk reference');
    // A block may carry a previous chunk header when this shard produced no new
    // chunk. Only chunks included at this height contain this block's txs.
    if (reference.height_included !== block.header.height) continue;
    const chunk = await rpc('chunk', { chunk_id: reference.chunk_hash });
    check(chunk?.header?.chunk_hash === reference.chunk_hash &&
      chunk.header.height_included === block.header.height && Array.isArray(chunk.transactions),
    'Chunk response does not match its block reference');
    for (const tx of chunk.transactions) {
      check(hash(tx?.hash) && typeof tx.signer_id === 'string' &&
        typeof tx.receiver_id === 'string' && Array.isArray(tx.actions), 'Malformed chunk transaction');
      transactions.push(tx);
    }
  }
  return transactions;
}

async function scanBlock({ rpc, verifier, reconciler, withdrawalVerifier, withdrawalReconciler, block }) {
  const results = [];
  for (const tx of await transactionsInBlock(rpc, block)) {
    let route;
    if (withdrawalVerifier?.matchesWithdrawal(tx)) {
      route = { type: 'withdrawal', verifier: withdrawalVerifier, reconciler: withdrawalReconciler };
    } else if (typeof verifier.matchesPurchase === 'function'
      ? verifier.matchesPurchase(tx) : verifier.matchesMarketplace(tx)) {
      route = { type: 'purchase', verifier, reconciler };
    } else continue;
    try {
      const facts = await route.verifier.verifyTransaction(tx, {
        hash: block.header.hash, height: block.header.height,
      });
      const result = await route.reconciler.reconcile(facts);
      if (!result.ok) throw new Error(`Reconciliation failed: ${result.error}`);
      results.push({ txHash: tx.hash, type: route.type, result });
    } catch (error) {
      // Failed/malformed/unrelated transactions are evidence rejection, not a
      // reason to lose the block checkpoint. RPC and database errors retry.
      if (error.name === 'RejectedPurchase' || error.name === 'RejectedWithdrawal') {
        results.push({ txHash: tx.hash, type: route.type, rejected: true, error: error.message });
      } else throw error;
    }
  }
  return results;
}

async function main() {
  const config = {
    networkId: env('NEAR_NETWORK_ID'),
    rpcUrl: httpsEndpoint('NEAR_RPC_URL', env('NEAR_RPC_URL')),
    marketplaceContractId: env('NEAR_MARKETPLACE_CONTRACT_ID'),
    nftContractId: env('NEAR_NFT_CONTRACT_ID'),
    marketplaceCodeHash: env('NEAR_MARKETPLACE_CODE_HASH'),
    nftCodeHash: env('NEAR_NFT_CODE_HASH'),
    supabaseUrl: httpsEndpoint('SUPABASE_URL', env('SUPABASE_URL')),
    supabaseKey: env('SUPABASE_SERVICE_ROLE_KEY'),
    processedPath: env('SETTLEMENT_PROCESSED_PATH', false) || './processed.jsonl',
  };

  config.rpc = (method, params) => rpcJson(config.rpcUrl, method, params);
  const checkpoint = new ProcessedStore(config.processedPath);
  config.processedStore = checkpoint;
  const verifier = new ChainVerifier(config);
  const withdrawalVerifier = new WithdrawalVerifier(config);
  const supabase = new SettlementSupabaseClient(config.supabaseUrl, config.supabaseKey);
  const reconciler = new Reconciler(config, supabase);
  const withdrawalReconciler = new WithdrawalReconciler(config, supabase);

  console.log('Settlement service starting', {
    network: config.networkId,
    marketplace: config.marketplaceContractId,
  });

  let cursor = checkpoint.getCursor();
  for (;;) {
    try {
      const status = await verifier.status();
      const finalBlock = await config.rpc('block', { finality: 'final' });
      check(height(finalBlock?.header?.height), 'Malformed final block');
      if (!cursor) {
        const configuredStart = process.env.SETTLEMENT_START_HEIGHT;
        if (configuredStart !== undefined) {
          const start = Number(configuredStart);
          check(Number.isSafeInteger(start) && start >= 1 && start <= finalBlock.header.height,
            'Invalid SETTLEMENT_START_HEIGHT');
          cursor = { height: start - 1, hash: '' };
        } else cursor = { height: Math.max(0, finalBlock.header.height - 100), hash: '' };
      }
      const earliest = status.sync_info.earliest_block_height;
      if (height(earliest) && cursor.height + 1 < earliest) {
        throw new Error(`RPC history begins at ${earliest}; an archival RPC is required after checkpoint ${cursor.height}`);
      }
      if (cursor.hash) {
        const saved = await config.rpc('block', { block_id: cursor.height });
        check(saved?.header?.hash === cursor.hash, 'Saved checkpoint is not on the canonical chain');
      }
      for (let h = cursor.height + 1; h <= finalBlock.header.height; h++) {
        let block;
        try {
          block = await config.rpc('block', { block_id: h });
        } catch (error) {
          // Finalized NEAR block heights may be skipped when no block was
          // produced. Pruned history is rejected above using earliest height.
          if (isUnknownBlock(error)) continue;
          throw error;
        }
        const results = await scanBlock({
          rpc: config.rpc, verifier, reconciler, withdrawalVerifier, withdrawalReconciler, block,
        });
        for (const item of results) console.log(item.rejected ? 'Rejected' : 'Reconciled', item);
        checkpoint.setCursor(block.header.height, block.header.hash);
        cursor = checkpoint.getCursor();
      }
    } catch (err) {
      console.error('Settlement poll error', err.message);
    }
    await new Promise((r) => setTimeout(r, 5000));
  }
}

if (require.main === module) {
  main().catch((err) => {
    console.error('Settlement service fatal', err);
    process.exit(1);
  });
}

module.exports = { RpcError, isUnknownBlock, env, httpsEndpoint, rpcJson, transactionsInBlock, scanBlock, main };
