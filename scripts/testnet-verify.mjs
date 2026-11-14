const RPC_URL = process.env.NEAR_RPC_URL || 'https://rpc.testnet.near.org';
const NFT_CONTRACT = process.env.NFT_CONTRACT || process.env.VITE_NFT_CONTRACT_ID || 'nft-v2.roundfrog-dev.testnet';
const MARKET_CONTRACT = process.env.MARKET_CONTRACT || process.env.VITE_MARKETPLACE_CONTRACT_ID || 'market-v2.roundfrog-dev.testnet';
const SELLER = process.env.SELLER || 'roundfrog-dev.testnet';
const OPERATOR = process.env.OPERATOR || 'roundfrog-dev.testnet';
const BUYER = process.env.BUYER || 'buyer.roundfrog-dev.testnet';

async function rpc(method, params) {
  const res = await fetch(RPC_URL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ jsonrpc: '2.0', id: 'verify', method, params }),
  });
  const data = await res.json();
  if (data.error) throw new Error(data.error.data || data.error.message || JSON.stringify(data.error));
  return data.result;
}

async function view(contractId, methodName, args = {}) {
  try {
    const res = await rpc('query', {
      request_type: 'call_function',
      account_id: contractId,
      method_name: methodName,
      args_base64: Buffer.from(JSON.stringify(args)).toString('base64'),
      finality: 'final',
    });
    return JSON.parse(Buffer.from(res.result).toString());
  } catch (err) {
    return { error: err.message };
  }
}

async function balance(accountId) {
  try {
    const acc = await rpc('query', {
      request_type: 'view_account',
      account_id: accountId,
      finality: 'final',
    });
    return (Number(BigInt(acc.amount) / 10n ** 20n) / 10000).toFixed(4) + ' NEAR';
  } catch (err) {
    return 'error: ' + err.message;
  }
}

async function main() {
  console.log('========================================================================');
  console.log(' NEAR NFT Marketplace — Testnet State Verification');
  console.log('========================================================================');
  console.log('RPC Endpoint:        ', RPC_URL);
  console.log('NFT Contract:        ', NFT_CONTRACT);
  console.log('Marketplace Contract:', MARKET_CONTRACT);
  console.log('');

  console.log('[*] Account Balances:');
  console.log('    Seller:   ', SELLER, '->', await balance(SELLER));
  console.log('    Operator: ', OPERATOR, '->', await balance(OPERATOR));
  console.log('    Buyer:    ', BUYER, '->', await balance(BUYER));
  console.log('');

  console.log('[*] NFT Contract State:');
  const mintAuth = await view(NFT_CONTRACT, 'testnet_mint_authority');
  const marketId = await view(NFT_CONTRACT, 'marketplace_id');
  const totalSupply = await view(NFT_CONTRACT, 'nft_total_supply');
  console.log('    testnet_mint_authority: ', mintAuth);
  console.log('    marketplace_id:         ', marketId);
  console.log('    nft_total_supply:       ', totalSupply);
  console.log('');

  console.log('[*] Marketplace Contract State:');
  const config = await view(MARKET_CONTRACT, 'config');
  const storage = await view(MARKET_CONTRACT, 'storage_balance_of', { account_id: SELLER });
  const sellerProceeds = await view(MARKET_CONTRACT, 'seller_proceeds', { seller_id: SELLER });
  const feeProceeds = await view(MARKET_CONTRACT, 'fee_proceeds', { account_id: OPERATOR });
  const buyerRefund = await view(MARKET_CONTRACT, 'buyer_refund', { buyer_id: BUYER });
  const listings = await view(MARKET_CONTRACT, 'listings', { limit: 10 });
  const transactions = await view(MARKET_CONTRACT, 'transactions', { limit: 10 });

  console.log('    trading_enabled: ', config.trading_enabled);
  console.log('    owner:           ', config.owner);
  console.log('    fee_bps:         ', config.fee_bps);
  console.log('    seller storage:  ', storage);
  console.log('    seller proceeds: ', sellerProceeds);
  console.log('    fee proceeds:    ', feeProceeds);
  console.log('    buyer refund:    ', buyerRefund);
  console.log('    active listings: ', Array.isArray(listings) ? listings.filter(l => l.active).length : listings);
  console.log('    transactions:    ', Array.isArray(transactions) ? transactions.length : transactions);
  console.log('========================================================================');
}

main().catch(console.error);
