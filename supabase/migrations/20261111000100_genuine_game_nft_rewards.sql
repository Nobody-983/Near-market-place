-- Migration: 20261111000100_genuine_game_nft_rewards.sql
-- Enables genuine on-chain NEP-171 NFT minting and marketplace listing for game rewards.
-- Preserves existing claim history and off-chain records while unlocking on-chain lifecycle.

begin;

-- 1. Remove the off-chain restriction on game rewards
alter table public.nfts drop constraint if exists game_rewards_are_offchain;

-- 2. Update bind_verified_nft_mint to allow binding verified on-chain mints for game rewards
create or replace function public.bind_verified_nft_mint(
  p_catalog_id uuid,
  p_transaction_hash text,
  p_owner_wallet_address text,
  p_network_id text,
  p_nft_contract_id text,
  p_token_id text
)
returns uuid language plpgsql
set search_path = ''
as $$
declare
  v_nft public.nfts%rowtype;
  v_owner uuid;
  v_current_owner uuid;
begin
  if p_transaction_hash !~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$' or
     p_owner_wallet_address !~ '^[a-z0-9]+([._-][a-z0-9]+)*$' or
     p_network_id !~ '^[a-z0-9_-]+$' or
     p_nft_contract_id !~ '^[a-z0-9]+([._-][a-z0-9]+)*$' or
     p_token_id !~ '^catalog-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$' or
     p_token_id <> 'catalog-' || p_catalog_id::text then
    raise exception 'Invalid verified mint facts' using errcode = '22023';
  end if;

  select * into strict v_nft from public.nfts where id = p_catalog_id for update;
  select id into v_owner from public.profiles
    where wallet_address = p_owner_wallet_address for share;
  select owner_id into v_current_owner from public.nft_ownership
    where nft_id = p_catalog_id for update;

  if v_owner is null or v_owner is distinct from v_nft.creator_id or
     v_current_owner is distinct from v_owner or
     (v_nft.source <> 'user' and v_nft.source <> 'game_reward') or
     v_nft.network_id is distinct from p_network_id then
    raise exception 'Mint owner does not match the catalog creator' using errcode = '23514';
  end if;

  if v_nft.onchain_status = 'confirmed' then
    if v_nft.mint_tx_hash = p_transaction_hash and
       v_nft.nft_contract_id = p_nft_contract_id and v_nft.token_id = p_token_id then
      return v_nft.id;
    end if;
    raise exception 'Catalog NFT is already bound to different chain facts' using errcode = '23514';
  end if;

  if v_nft.token_id is not null or v_nft.nft_contract_id is not null or v_nft.mint_tx_hash is not null then
    raise exception 'Catalog NFT has an unverified chain identity' using errcode = '23514';
  end if;

  update public.nfts set token_id = p_token_id, nft_contract_id = p_nft_contract_id,
    mint_tx_hash = p_transaction_hash, onchain_status = 'confirmed', updated_at = statement_timestamp()
    where id = p_catalog_id;
  return p_catalog_id;
end;
$$;

-- 3. Update validate_listing to allow on-chain minted game rewards to be listed
create or replace function marketplace_private.validate_listing()
returns trigger language plpgsql security definer
set search_path = ''
as $$
declare
  v_nft public.nfts%rowtype;
begin
  if tg_op = 'UPDATE' then
    if new.id <> old.id or new.nft_id <> old.nft_id or new.seller_id <> old.seller_id or
       new.network_id <> old.network_id or new.marketplace_contract_id <> old.marketplace_contract_id or
       new.contract_sale_id <> old.contract_sale_id or new.listing_tx_hash <> old.listing_tx_hash or
       new.approval_id <> old.approval_id or new.listing_generation <> old.listing_generation or
       new.created_at <> old.created_at then
      raise exception 'Listing immutable columns cannot be modified' using errcode = '23514';
    end if;
  end if;
  select * into strict v_nft from public.nfts where id = new.nft_id for update;
  if (v_nft.source = 'game_reward' and v_nft.token_id is null) or v_nft.network_id <> new.network_id then
    raise exception 'NFT is not eligible for this marketplace listing' using errcode = '23514';
  end if;
  if new.status in ('active', 'sold') and v_nft.token_id is null then
    raise exception 'Unminted NFTs cannot have active or sold listings' using errcode = '23514';
  end if;
  return new;
end;
$$;

-- 4. Update claim_game_reward to support Bronze (500), Silver (2000), Gold (5000), and Legendary (10000)
create or replace function public.claim_game_reward(p_reward_level integer)
returns uuid language plpgsql security definer
set search_path = ''
as $$
declare
  v_user uuid := auth.uid();
  v_taps integer;
  v_required integer;
  v_name text;
  v_nft uuid;
begin
  if v_user is null or not marketplace_private.has_verified_wallet_session() then
    raise exception 'A verified wallet session is required' using errcode = '42501';
  end if;
  case p_reward_level
    when 1 then v_required := 500; v_name := 'Bronze NFT';
    when 2 then v_required := 2000; v_name := 'Silver NFT';
    when 3 then v_required := 5000; v_name := 'Gold NFT';
    when 4 then v_required := 10000; v_name := 'Legendary Champion NFT';
    else raise exception 'Reward level must be 1, 2, 3, or 4' using errcode = '22023';
  end case;
  select tap_count into strict v_taps from public.profiles where id = v_user for update;
  select nft_id into v_nft from public.reward_claims where user_id = v_user and reward_level = p_reward_level;
  if found then return v_nft; end if;
  if v_taps < v_required then
    raise exception 'Not enough taps for this reward' using errcode = '23514';
  end if;
  insert into public.nfts(creator_id, name, description, image_url, category, price, source)
  values (v_user, v_name, 'NEAR Tap Game collectible eligible for on-chain NEP-171 minting.',
    '/rewards/' || p_reward_level::text || '.jpeg', 'Gaming', 0, 'game_reward')
  returning id into v_nft;
  insert into public.reward_claims(user_id, reward_level, nft_id) values (v_user, p_reward_level, v_nft);
  return v_nft;
end;
$$;

-- 5. Update reconcile_nft_purchase to allow on-chain minted game rewards to settle
create or replace function public.reconcile_nft_purchase(
  p_listing_id uuid,
  p_transaction_hash text,
  p_buyer_wallet_address text,
  p_seller_wallet_address text,
  p_network_id text,
  p_marketplace_contract_id text,
  p_contract_sale_id text,
  p_nft_contract_id text,
  p_token_id text,
  p_price_yocto numeric
)
returns uuid language plpgsql
set search_path = ''
as $$
declare
  v_listing public.marketplace_listings%rowtype;
  v_nft public.nfts%rowtype;
  v_buyer uuid;
  v_seller_wallet text;
  v_owner uuid;
begin
  if p_transaction_hash is null or p_transaction_hash !~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$' then
    raise exception 'A real NEAR transaction hash is required' using errcode = '22023';
  end if;
  select * into strict v_listing from public.marketplace_listings where id = p_listing_id for update;
  select * into strict v_nft from public.nfts where id = v_listing.nft_id for update;
  select id into v_buyer from public.profiles where wallet_address = p_buyer_wallet_address for share;
  select wallet_address into v_seller_wallet from public.profiles where id = v_listing.seller_id for share;
  if v_buyer is null or v_buyer = v_listing.seller_id or v_seller_wallet is null or
     v_seller_wallet is distinct from p_seller_wallet_address then
    raise exception 'Buyer/seller verified profile mapping is invalid' using errcode = '23514';
  end if;
  if p_network_id is distinct from v_listing.network_id or
     p_network_id is distinct from v_nft.network_id or
     v_listing.marketplace_contract_id is null or
     p_marketplace_contract_id is distinct from v_listing.marketplace_contract_id or
     v_listing.contract_sale_id is null or p_contract_sale_id is distinct from v_listing.contract_sale_id or
     v_nft.nft_contract_id is null or p_nft_contract_id is distinct from v_nft.nft_contract_id or
     v_nft.token_id is null or p_token_id is distinct from v_nft.token_id or
     p_price_yocto is distinct from (v_listing.price * 1000000000000000000000000) or
     (v_nft.source = 'game_reward' and v_nft.token_id is null) then
    raise exception 'Verified chain facts do not match this listing' using errcode = '23514';
  end if;
  if v_listing.status = 'sold' then
    if v_listing.tx_hash = p_transaction_hash and v_listing.buyer_id = v_buyer then
      return v_listing.id;
    end if;
    raise exception 'Listing already settled with different purchase facts' using errcode = '23514';
  end if;
  if v_listing.status <> 'active' or v_listing.onchain_status <> 'confirmed' then
    raise exception 'Listing is not active and confirmed' using errcode = '23514';
  end if;
  select owner_id into v_owner from public.nft_ownership where nft_id = v_listing.nft_id for update;
  if v_owner is distinct from v_listing.seller_id then
    raise exception 'Seller no longer owns this NFT' using errcode = '23514';
  end if;
  update public.marketplace_listings
    set status = 'sold', buyer_id = v_buyer, tx_hash = p_transaction_hash,
        sold_at = statement_timestamp(), onchain_status = 'confirmed'
    where id = v_listing.id;
  update public.nft_ownership set owner_id = v_buyer, acquired_at = statement_timestamp()
    where nft_id = v_listing.nft_id;
  return v_listing.id;
end;
$$;

revoke all on function public.bind_verified_nft_mint(uuid, text, text, text, text, text) from public, anon, authenticated;
grant execute on function public.bind_verified_nft_mint(uuid, text, text, text, text, text) to service_role;
revoke all on function public.claim_game_reward(integer) from public, anon, authenticated;
grant execute on function public.claim_game_reward(integer) to authenticated;
revoke all on function public.reconcile_nft_purchase(uuid, text, text, text, text, text, text, text, text, numeric) from public, anon, authenticated;
grant execute on function public.reconcile_nft_purchase(uuid, text, text, text, text, text, text, text, text, numeric) to service_role;

commit;
