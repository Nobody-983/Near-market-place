-- Trusted NEAR mint/listing indexing. Apply only after the four earlier
-- migrations and only to an explicitly approved disposable project.
begin;

alter table public.nfts
  add column mint_tx_hash text,
  add column onchain_status text not null default 'unverified'
    check (onchain_status in ('unverified', 'confirmed', 'failed'));

alter table public.nfts
  add constraint nft_mint_tx_hash_format check (
    mint_tx_hash is null or mint_tx_hash ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$'
  ),
  add constraint confirmed_nfts_have_chain_identity check (
    onchain_status <> 'confirmed' or
    (token_id is not null and nft_contract_id is not null and mint_tx_hash is not null)
  );

create unique index nfts_mint_tx_identity_idx
  on public.nfts(network_id, mint_tx_hash) where mint_tx_hash is not null;

alter table public.marketplace_listings
  add column listing_tx_hash text,
  add column cancellation_tx_hash text,
  add column approval_id numeric(20, 0),
  add column listing_generation numeric(20, 0);

alter table public.marketplace_listings
  add constraint listing_tx_hash_format check (
    listing_tx_hash is null or listing_tx_hash ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$'
  ),
  add constraint cancellation_tx_hash_format check (
    cancellation_tx_hash is null or cancellation_tx_hash ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$'
  ),
  add constraint listing_chain_counters check (
    (approval_id is null and listing_generation is null) or
    (approval_id > 0 and approval_id <= 18446744073709551615 and
     listing_generation > 0 and listing_generation <= 18446744073709551615)
  );

create unique index listings_publication_tx_idx
  on public.marketplace_listings(network_id, listing_tx_hash)
  where listing_tx_hash is not null;
create unique index listings_cancellation_tx_idx
  on public.marketplace_listings(network_id, cancellation_tx_hash)
  where cancellation_tx_hash is not null;

create function public.bind_verified_nft_mint(
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
     p_network_id not in ('testnet', 'mainnet') or
     p_token_id is distinct from ('catalog-' || p_catalog_id::text) then
    raise exception 'Invalid verified mint identity' using errcode = '22023';
  end if;

  select * into strict v_nft from public.nfts where id = p_catalog_id for update;
  select id into v_owner from public.profiles
    where wallet_address = p_owner_wallet_address for share;
  select owner_id into v_current_owner from public.nft_ownership
    where nft_id = p_catalog_id for update;

  if v_owner is null or v_owner is distinct from v_nft.creator_id or
     v_current_owner is distinct from v_owner or v_nft.source <> 'user' or
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

create function public.publish_verified_listing(
  p_transaction_hash text,
  p_seller_wallet_address text,
  p_network_id text,
  p_marketplace_contract_id text,
  p_contract_listing_id text,
  p_nft_contract_id text,
  p_token_id text,
  p_price_yocto numeric,
  p_approval_id numeric,
  p_generation numeric
)
returns uuid language plpgsql
set search_path = ''
as $$
declare
  v_nft public.nfts%rowtype;
  v_seller uuid;
  v_owner uuid;
  v_existing public.marketplace_listings%rowtype;
  v_listing_id uuid;
  v_price numeric;
begin
  if p_transaction_hash !~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$' or p_price_yocto <= 0 or
     scale(p_price_yocto) <> 0 or p_approval_id <= 0 or p_generation <= 0 then
    raise exception 'Invalid verified listing facts' using errcode = '22023';
  end if;
  v_price := p_price_yocto / 1000000000000000000000000::numeric;

  select * into strict v_nft from public.nfts
    where network_id = p_network_id and nft_contract_id = p_nft_contract_id and token_id = p_token_id
    for update;
  select id into v_seller from public.profiles
    where wallet_address = p_seller_wallet_address for share;
  select owner_id into v_owner from public.nft_ownership where nft_id = v_nft.id for update;
  if v_seller is null or v_owner is distinct from v_seller or v_nft.onchain_status <> 'confirmed' then
    raise exception 'Verified seller does not own the indexed NFT' using errcode = '23514';
  end if;

  select * into v_existing from public.marketplace_listings
    where network_id = p_network_id and marketplace_contract_id = p_marketplace_contract_id
      and contract_sale_id = p_contract_listing_id for update;
  if found then
    if v_existing.nft_id = v_nft.id and v_existing.seller_id = v_seller and
       v_existing.price = v_price and v_existing.listing_tx_hash = p_transaction_hash and
       v_existing.approval_id = p_approval_id and v_existing.listing_generation = p_generation then
      return v_existing.id;
    end if;
    raise exception 'Listing identity conflicts with indexed chain facts' using errcode = '23514';
  end if;

  update public.marketplace_listings set status = 'cancelled', onchain_status = 'confirmed',
    updated_at = statement_timestamp()
    where nft_id = v_nft.id and status = 'active';

  insert into public.marketplace_listings(
    nft_id, seller_id, price, currency, status, network_id, marketplace_contract_id,
    contract_sale_id, onchain_status, listing_tx_hash, approval_id, listing_generation
  ) values (
    v_nft.id, v_seller, v_price, 'NEAR', 'active', p_network_id, p_marketplace_contract_id,
    p_contract_listing_id, 'confirmed', p_transaction_hash, p_approval_id, p_generation
  ) returning id into v_listing_id;
  return v_listing_id;
end;
$$;

create function public.cancel_verified_listing(
  p_transaction_hash text,
  p_seller_wallet_address text,
  p_network_id text,
  p_marketplace_contract_id text,
  p_contract_listing_id text
)
returns uuid language plpgsql
set search_path = ''
as $$
declare
  v_listing public.marketplace_listings%rowtype;
  v_seller uuid;
begin
  if p_transaction_hash !~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$' then
    raise exception 'Invalid cancellation transaction' using errcode = '22023';
  end if;
  select id into v_seller from public.profiles where wallet_address = p_seller_wallet_address for share;
  select * into strict v_listing from public.marketplace_listings
    where network_id = p_network_id and marketplace_contract_id = p_marketplace_contract_id
      and contract_sale_id = p_contract_listing_id for update;
  if v_seller is null or v_listing.seller_id is distinct from v_seller then
    raise exception 'Cancellation seller mismatch' using errcode = '23514';
  end if;
  if v_listing.status = 'cancelled' then
    if v_listing.cancellation_tx_hash = p_transaction_hash then
      return v_listing.id;
    end if;
    raise exception 'Listing was cancelled by different chain facts' using errcode = '23514';
  end if;
  if v_listing.status <> 'active' then
    raise exception 'Only an active indexed listing can be cancelled' using errcode = '23514';
  end if;
  update public.marketplace_listings set status = 'cancelled', onchain_status = 'confirmed',
    cancellation_tx_hash = p_transaction_hash,
    updated_at = statement_timestamp() where id = v_listing.id;
  return v_listing.id;
end;
$$;

revoke all on function public.bind_verified_nft_mint(uuid, text, text, text, text, text),
  public.publish_verified_listing(text, text, text, text, text, text, text, numeric, numeric, numeric),
  public.cancel_verified_listing(text, text, text, text, text)
  from public, anon, authenticated;
grant execute on function public.bind_verified_nft_mint(uuid, text, text, text, text, text),
  public.publish_verified_listing(text, text, text, text, text, text, text, numeric, numeric, numeric),
  public.cancel_verified_listing(text, text, text, text, text)
  to service_role;

commit;
