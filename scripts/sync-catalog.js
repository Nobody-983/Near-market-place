#!/usr/bin/env node
// ==============================================================================
// scripts/sync-catalog.js
// On-demand catalog synchronization utility using verified chain facts.
// Usage:
//   node scripts/sync-catalog.js --scan [blocks]
//   node scripts/sync-catalog.js --tx <transactionHash> --signer <signerId>
// ==============================================================================

const { CatalogChainVerifier, RejectedIndexEvent } = require('../services/indexer/verifier');
const { CatalogReconciler } = require('../services/indexer/reconciler');
const { SettlementSupabaseClient } = require('../services/settlement/supabase');
const { rpcJson, transactionsInBlock } = require('../services/settlement/index');
const { scanCatalogBlock } = require('../services/indexer/index');

const RPC_URL = process.env.NEAR_RPC_URL || 'https://rpc.testnet.near.org';
const SUPABASE_URL = process.env.SUPABASE_URL || 'https://llqwiqhwrbfoxoqqgczy.supabase.co';
const SERVICE_KEY = process.env.SUPABASE_SERVICE_ROLE_KEY;

const config = {
  networkId: process.env.NEAR_NETWORK_ID || 'testnet',
  rpcUrl: RPC_URL,
  marketplaceContractId: process.env.NEAR_MARKETPLACE_CONTRACT_ID || 'market.roundfrog-dev.testnet',
  nftContractId: process.env.NEAR_NFT_CONTRACT_ID || 'nft.roundfrog-dev.testnet',
  marketplaceCodeHash: process.env.NEAR_MARKETPLACE_CODE_HASH || 'Fp3ggF3WmSMQC7m9FU717jA8nB897sYW6bik7Ysqipw7',
  nftCodeHash: process.env.NEAR_NFT_CODE_HASH || 'DRmwjtfeBzXW2XgXMumytEY5scsDzEtCGsbVNKjdEtW8',
};

config.rpc = (method, params) => rpcJson(config.rpcUrl, method, params);

async function syncTx(txHash, signerId) {
  if (!SERVICE_KEY) {
    console.error('[-] SUPABASE_SERVICE_ROLE_KEY required for catalog reconciliation.');
    process.exit(1);
  }
  const verifier = new CatalogChainVerifier(config);
  const reconciler = new CatalogReconciler(config, new SettlementSupabaseClient(SUPABASE_URL, SERVICE_KEY));

  console.log(`[*] Querying transaction ${txHash} for signer ${signerId}...`);
  const status = await config.rpc('EXPERIMENTAL_tx_status', {
    tx_hash: txHash,
    sender_account_id: signerId,
    wait_until: 'FINAL',
  });

  const blockHash = status.transaction_outcome?.block_hash;
  if (!blockHash) throw new Error('Transaction outcome missing block hash');

  const block = await config.rpc('block', { block_id: blockHash });
  const inclusion = { hash: block.header.hash, height: block.header.height };

  console.log(`[*] Verifying chain evidence in block ${inclusion.height} (${inclusion.hash})...`);
  const facts = await verifier.verify(status.transaction, inclusion);
  console.log('[+] Verified chain facts:', facts);

  console.log('[*] Reconciling with Supabase catalog...');
  const result = await reconciler.reconcile(facts);
  console.log('[+] Reconciliation result:', result);
  return result;
}

async function scanRecent(numBlocks = 20) {
  if (!SERVICE_KEY) {
    console.error('[-] SUPABASE_SERVICE_ROLE_KEY required for catalog reconciliation.');
    process.exit(1);
  }
  const verifier = new CatalogChainVerifier(config);
  const reconciler = new CatalogReconciler(config, new SettlementSupabaseClient(SUPABASE_URL, SERVICE_KEY));

  const finalBlock = await config.rpc('block', { finality: 'final' });
  const endHeight = finalBlock.header.height;
  const startHeight = endHeight - numBlocks;

  console.log(`[*] Scanning Testnet blocks from ${startHeight} to ${endHeight} for marketplace events...`);
  let synced = 0;

  for (let h = startHeight; h <= endHeight; h++) {
    let block;
    try {
      block = await config.rpc('block', { block_id: h });
    } catch {
      continue;
    }
    const results = await scanCatalogBlock({ rpc: config.rpc, verifier, reconciler, block });
    for (const r of results) {
      if (!r.rejected) {
        console.log(`[+] Synced event in block ${h}:`, r);
        synced++;
      }
    }
  }

  console.log(`[*] Scan complete. Total events synced: ${synced}`);
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes('--tx')) {
    const txIndex = args.indexOf('--tx');
    const signerIndex = args.indexOf('--signer');
    const tx = args[txIndex + 1];
    const signer = signerIndex >= 0 ? args[signerIndex + 1] : 'seller.roundfrog-dev.testnet';
    await syncTx(tx, signer);
  } else if (args.includes('--scan')) {
    const idx = args.indexOf('--scan');
    const blocks = args[idx + 1] ? parseInt(args[idx + 1], 10) : 20;
    await scanRecent(blocks);
  } else {
    console.log('Usage:');
    console.log('  node scripts/sync-catalog.js --scan [numBlocks]');
    console.log('  node scripts/sync-catalog.js --tx <txHash> --signer <signerAccount>');
  }
}

if (require.main === module) {
  main().catch(console.error);
}

module.exports = { syncTx, scanRecent };

