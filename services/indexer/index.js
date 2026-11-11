const { CatalogChainVerifier, RejectedIndexEvent } = require('./verifier');
const { CatalogReconciler } = require('./reconciler');
const { SettlementSupabaseClient } = require('../settlement/supabase');
const { ProcessedStore } = require('../settlement/store');
const { rpcJson, transactionsInBlock, isUnknownBlock } = require('../settlement/index');
const { check, height } = require('../settlement/validation');

function required(name) {
  const value = process.env[name];
  if (!value) throw new Error(`Missing env: ${name}`);
  return value;
}

async function scanCatalogBlock({ rpc, verifier, reconciler, block }) {
  const results = [];
  for (const tx of await transactionsInBlock(rpc, block)) {
    if (!verifier.matches(tx)) continue;
    try {
      const facts = await verifier.verify(tx, { hash: block.header.hash, height: block.header.height });
      const result = await reconciler.reconcile(facts);
      if (!result.ok) throw new Error(`Catalog reconciliation failed: ${result.error}`);
      results.push({ txHash: tx.hash, result });
    } catch (error) {
      if (error instanceof RejectedIndexEvent) results.push({ txHash: tx.hash, rejected: true, error: error.message });
      else throw error;
    }
  }
  return results;
}

async function main() {
  const rpcUrl = new URL(required('NEAR_RPC_URL'));
  const supabaseUrl = new URL(required('SUPABASE_URL'));
  if (rpcUrl.protocol !== 'https:' || supabaseUrl.protocol !== 'https:') throw new Error('HTTPS endpoints required');
  const config = {
    networkId: required('NEAR_NETWORK_ID'), rpcUrl: rpcUrl.toString(),
    marketplaceContractId: required('NEAR_MARKETPLACE_CONTRACT_ID'),
    nftContractId: required('NEAR_NFT_CONTRACT_ID'),
    marketplaceCodeHash: required('NEAR_MARKETPLACE_CODE_HASH'),
    nftCodeHash: required('NEAR_NFT_CODE_HASH'),
  };
  config.rpc = (method, params) => rpcJson(config.rpcUrl, method, params);
  const verifier = new CatalogChainVerifier(config);
  const reconciler = new CatalogReconciler(config,
    new SettlementSupabaseClient(supabaseUrl.toString(), required('SUPABASE_SERVICE_ROLE_KEY')));
  const store = new ProcessedStore(process.env.CATALOG_INDEXER_PROCESSED_PATH || './catalog-processed.jsonl');
  let cursor = store.getCursor();
  for (;;) {
    try {
      const status = await config.rpc('status', []);
      check(status?.chain_id === config.networkId && status?.sync_info?.syncing === false &&
        height(status.sync_info.latest_block_height), 'RPC network mismatch or node is syncing');
      const finalBlock = await config.rpc('block', { finality: 'final' });
      check(height(finalBlock?.header?.height), 'Malformed final block');
      if (!cursor) {
        const configuredStart = process.env.CATALOG_INDEXER_START_HEIGHT;
        if (configuredStart !== undefined) {
          const start = Number(configuredStart);
          check(Number.isSafeInteger(start) && start >= 1 && start <= finalBlock.header.height,
            'Invalid CATALOG_INDEXER_START_HEIGHT');
          cursor = { height: start - 1, hash: '' };
        } else cursor = { height: Math.max(0, finalBlock.header.height - 100), hash: '' };
      }
      const earliest = status.sync_info.earliest_block_height;
      if (height(earliest) && cursor.height + 1 < earliest) {
        throw new Error(`RPC history begins at ${earliest}; an archival RPC is required after checkpoint ${cursor.height}`);
      }
      if (cursor.hash) {
        const saved = await config.rpc('block', { block_id: cursor.height });
        check(saved?.header?.hash === cursor.hash, 'Saved catalog checkpoint is not on the canonical chain');
      }
      for (let current = cursor.height + 1; current <= finalBlock.header.height; current++) {
        let block;
        try { block = await config.rpc('block', { block_id: current }); }
        catch (error) {
          if (isUnknownBlock(error)) continue;
          throw error;
        }
        const results = await scanCatalogBlock({ rpc: config.rpc, verifier, reconciler, block });
        for (const result of results) console.log(result.rejected ? 'Rejected catalog event' : 'Indexed catalog event', result);
        store.setCursor(block.header.height, block.header.hash);
        cursor = store.getCursor();
      }
    } catch (error) {
      console.error('Catalog indexer error', error.message);
    }
    await new Promise((resolve) => setTimeout(resolve, 5000));
  }
}

if (require.main === module) main().catch((error) => { console.error(error); process.exit(1); });

module.exports = { scanCatalogBlock, main };
