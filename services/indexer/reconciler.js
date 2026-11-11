const { isVerifiedCatalogFacts } = require('./verifier');

class CatalogReconciler {
  constructor(config, supabase) {
    this.config = config;
    this.supabase = supabase;
  }

  async reconcile(facts) {
    if (!isVerifiedCatalogFacts(facts)) return { ok: false, error: 'Unverified catalog facts' };
    let name;
    let params;
    if (facts.kind === 'mint') {
      name = 'bind_verified_nft_mint';
      params = {
        p_catalog_id: facts.catalogId, p_transaction_hash: facts.txHash,
        p_owner_wallet_address: facts.ownerWalletAddress, p_network_id: facts.networkId,
        p_nft_contract_id: facts.nftContractId, p_token_id: facts.tokenId,
      };
    } else if (facts.kind === 'listing') {
      name = 'publish_verified_listing';
      params = {
        p_transaction_hash: facts.txHash, p_seller_wallet_address: facts.sellerWalletAddress,
        p_network_id: facts.networkId, p_marketplace_contract_id: facts.marketplaceContractId,
        p_contract_listing_id: facts.contractListingId, p_nft_contract_id: facts.nftContractId,
        p_token_id: facts.tokenId, p_price_yocto: facts.priceYocto,
        p_approval_id: facts.approvalId, p_generation: facts.generation,
      };
    } else if (facts.kind === 'cancellation') {
      name = 'cancel_verified_listing';
      params = {
        p_transaction_hash: facts.txHash, p_seller_wallet_address: facts.sellerWalletAddress,
        p_network_id: facts.networkId, p_marketplace_contract_id: facts.marketplaceContractId,
        p_contract_listing_id: facts.contractListingId,
      };
    } else return { ok: false, error: 'Unknown catalog fact kind' };
    const { data, error } = await this.supabase.rpc(name, params);
    return error ? { ok: false, error: error.message } : { ok: true, id: data, kind: facts.kind };
  }
}

module.exports = { CatalogReconciler };
