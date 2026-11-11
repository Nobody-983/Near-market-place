// Reconciles verified on-chain facts with Supabase.
// Idempotent: duplicate receipts are rejected by the ProcessedStore and by
// the Supabase unique indexes on tx_hash and contract_sale_id.

const { ProcessedStore } = require('./store');
const { isVerifiedFacts } = require('./verifier');
const { accountId, hash, uint } = require('./validation');

class Reconciler {
  constructor(config, supabase) {
    this.config = config;
    this.supabase = supabase;
    this.store = config.processedStore || new ProcessedStore(config.processedPath);
  }

  async reconcile(facts) {
    if (!isVerifiedFacts(facts)) return { ok: false, error: 'Facts were not produced by ChainVerifier' };
    if (!hash(facts.txHash) || !accountId(facts.buyerWalletAddress) ||
        !accountId(facts.sellerWalletAddress) || facts.networkId !== this.config.networkId ||
        facts.marketplaceContractId !== this.config.marketplaceContractId ||
        facts.nftContractId !== this.config.nftContractId || !facts.contractSaleId ||
        !facts.purchaseSaleId || !facts.tokenId) {
      return { ok: false, error: 'Verified facts conflict with settlement configuration' };
    }
    try { uint(facts.priceYocto, undefined, true); } catch (error) {
      return { ok: false, error: error.message };
    }

    // Resolve the database UUID from service-side indexed chain identity. This
    // UUID is never accepted from a wallet or browser transaction response.
    const lookup = await this.supabase
      .from('marketplace_listings')
      .select('id')
      .eq('network_id', facts.networkId)
      .eq('marketplace_contract_id', facts.marketplaceContractId)
      .eq('contract_sale_id', facts.contractSaleId)
      .limit(2);
    if (lookup.error) return { ok: false, error: lookup.error.message };
    if (!Array.isArray(lookup.data) || lookup.data.length !== 1 || !lookup.data[0]?.id) {
      return { ok: false, error: 'Verified sale does not resolve to exactly one listing' };
    }
    const listingId = lookup.data[0].id;

    const { data, error } = await this.supabase.rpc('reconcile_nft_purchase', {
      p_listing_id: listingId,
      p_transaction_hash: facts.txHash,
      p_buyer_wallet_address: facts.buyerWalletAddress,
      p_seller_wallet_address: facts.sellerWalletAddress,
      p_network_id: facts.networkId,
      p_marketplace_contract_id: facts.marketplaceContractId,
      p_contract_sale_id: facts.contractSaleId,
      p_nft_contract_id: facts.nftContractId,
      p_token_id: facts.tokenId,
      p_price_yocto: facts.priceYocto,
    });

    if (error) {
      // Do not mark as processed on SQL failure; allow safe retry.
      return { ok: false, error: error.message };
    }

    // The secured RPC returns the listing UUID for both first application and
    // an exact replay. Always invoke it; the local file is only an audit cache.
    if (data !== listingId) return { ok: false, error: 'Unexpected reconciliation result' };
    const duplicate = this.store.has(facts.txHash);
    this.store.add(facts.txHash);
    return { ok: true, duplicate, listingId };
  }
}

module.exports = { Reconciler };
