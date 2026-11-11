-- Idempotent reconciliation of verified on-chain marketplace purchases.
-- On-chain state is authoritative; this layer persists verified facts only.
-- Wallet addresses from the chain are resolved to profile UUIDs via
-- public.profiles.wallet_address.

CREATE OR REPLACE FUNCTION reconcile_nft_purchase(
  p_listing_id TEXT,
  p_transaction_hash TEXT,
  p_buyer_wallet_address TEXT,
  p_seller_wallet_address TEXT,
  p_network_id TEXT,
  p_marketplace_contract_id TEXT,
  p_contract_sale_id TEXT,
  p_nft_contract_id TEXT,
  p_token_id TEXT,
  p_price_yocto TEXT
) RETURNS JSONB AS $$
DECLARE
  v_listing RECORD;
  v_buyer_id UUID;
  v_seller_id UUID;
BEGIN
  -- Reject duplicate transaction hashes.
  IF EXISTS (
    SELECT 1 FROM marketplace_listings
    WHERE transaction_hash = p_transaction_hash
  ) THEN
    RETURN jsonb_build_object('ok', true, 'duplicate', true);
  END IF;

  SELECT id INTO v_buyer_id FROM public.profiles WHERE wallet_address = p_buyer_wallet_address;
  SELECT id INTO v_seller_id FROM public.profiles WHERE wallet_address = p_seller_wallet_address;

  SELECT * INTO v_listing
  FROM marketplace_listings
  WHERE id = p_listing_id::uuid
  FOR UPDATE;

  IF NOT FOUND THEN
    RETURN jsonb_build_object('ok', false, 'error', 'Listing not found');
  END IF;

  IF v_listing.status = 'sold' AND v_listing.contract_sale_id = p_contract_sale_id THEN
    RETURN jsonb_build_object('ok', true, 'duplicate', true);
  END IF;

  UPDATE marketplace_listings
  SET
    buyer_id = v_buyer_id,
    seller_id = COALESCE(v_seller_id, v_listing.seller_id),
    contract_sale_id = p_contract_sale_id,
    tx_hash = p_transaction_hash,
    onchain_status = 'confirmed',
    status = 'sold',
    network_id = p_network_id,
    marketplace_contract_id = p_marketplace_contract_id,
    nft_contract_id = p_nft_contract_id,
    token_id = p_token_id,
    price = p_price_yocto,
    sold_at = NOW(),
    updated_at = NOW()
  WHERE id = p_listing_id::uuid;

  RETURN jsonb_build_object('ok', true, 'duplicate', false);
END;
$$ LANGUAGE plpgsql;
