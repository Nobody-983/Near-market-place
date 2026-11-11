-- NEAR marketplace: initial schema for a NEW Supabase project, PostgreSQL 15+.
-- Prepared only; do not run against the old project or on top of its schema.
-- This is deliberately not idempotent: existing tables/functions/bucket abort the
-- transaction instead of silently retaining incompatible columns or policies.
--
-- Sources inspected (working tree, including uncommitted changes):
--   frontend/Nft/src/lib/supabase.jsx; services/{auth,nftService,marketService}.js;
--   pages/{Account,createNft,dashboard,market,nftDetails}.jsx; pages/game/Game.jsx;
--   context/walletContext.jsx; config/near.js; lib/near/contract.js.
-- Historical baseline: 94538cf:backend/supabase/migrations/20260904_init_schema.sql.
-- Its four tables lack ownership, game rewards, chain identity, and safe writes.
--
-- Identity and deployment prerequisites:
-- * UUIDs are Supabase auth/profile IDs, never NEAR account IDs. NEAR accounts
--   are lowercase text; token and sale IDs are opaque text, not UUIDs/numbers.
-- * auth.js uses publicly derivable passwords and collision-prone email names.
--   auth.uid(), email, user_metadata, and a connected browser wallet do NOT
--   establish wallet ownership. No trigger imports auth.users metadata.
--   Replace that login and retire its credentials/sessions before real use;
--   SQL session gating cannot secure a shared account's Auth administration.
-- * A TRUSTED backend must verify a fresh wallet-signed, single-use challenge,
--   bound to this project/domain, network, auth user AND auth session, with an
--   expiry. Verify the account's key authority on the intended NEAR network.
--   It provisions profiles.wallet_address and inserts a short-lived row in
--   marketplace_private.verified_wallet_sessions for that exact auth.sessions
--   row. Never approve a user globally or copy verification to a password login.
--   Password-created sessions cannot write unless independently verified.
-- * Session verification expires within 24 hours; backend deletion revokes it.
--   Session/user consistency is checked against auth.sessions on every write.
--   The private schema must NOT be added to Supabase's exposed API schemas.
--   Provision verification with a trusted SQL connection, never a browser key.
-- * One NEAR network per project is assumed (current frontend default: testnet).
--   Chain rows also carry network_id to namespace token/sale/transaction IDs.
--   The Storage URL prefix below is bound to the configured Supabase project;
--   update it together with VITE_SUPABASE_URL when preparing another project.
--   No contract IDs, users, or on-chain assets are seeded here.
--
-- Compatibility and intentional security boundaries:
-- * Public catalog/profile/like reads and FK joins match the frontend. Wallet
--   bindings are backend-only and immutable once set. Profile creation in the
--   current wallet login will fail until the trusted verifier provisions it.
-- * Verified sessions can edit cosmetics, increment taps by one, upload images,
--   create unminted catalog entries, delete their own never-listed/unminted
--   entries, and manage their own likes. Ownership starts at the creator via a
--   trigger; clients cannot write ownership rows or claim on-chain identity.
-- * Listings are backend-only. Publish/cancel only after verifying chain state;
--   pending rows support staging, while active/sold require confirmed chain
--   identity. Current createListing/cancelListing browser writes are blocked.
--   The UI ignores onchain_status, so permitting unverified active sales is unsafe.
-- * complete_nft_purchase(uuid,text) rejects the frontend's fabricated near_...
--   fallback. reconcile_nft_purchase is service-role-only and requires verified
--   chain facts; SQL itself cannot authenticate NEAR transactions. A verifier
--   must check FINAL successful receipts (including transfer callbacks), the
--   configured/allowlisted network and contracts, exact token/sale, buyer,
--   seller, price/payment, transfer and current owner. A submitted hash alone
--   is not proof. Never expose a service-role proxy that just forwards inputs.
-- * One purchase per transaction is assumed by the current single buy_nft call.
--   Batch purchases need a future migration using receipt/event identifiers.
-- * Game awards are OFF-CHAIN collectibles, never tradable/minted by this schema.
--   Taps are monotonic and paced but still automatable, not proof of human play.
--   Existing /rewards/1.jpeg, /2.jpeg, /3.jpeg illustrate Bronze/Silver/Gold;
--   this artwork mapping is an explicit new assumption, not historical seed data.
-- * Uploads are public (getPublicUrl), max 5 MiB, PNG/JPEG/WebP, in uid/uuid.ext.
--   Browser uploads are append-only: no overwrite/move/delete permissions.
--   Trusted cleanup must recheck references before removing orphaned objects;
--   current browser cleanup/deleteNFTImage cannot remove them. Retention protects
--   purchased artwork. image_path is derived/validated when createNFT omits it.
-- * nftService.js still sends a session token to a hard-coded OLD project URL
--   during upload preflight. SQL cannot fix that; do not use live sessions in
--   that flow until separately corrected. No storage.buckets SELECT is granted
--   to make that diagnostic endpoint work.
-- * nftDetails.jsx incorrectly uses wallet strings in nft_likes.user_id (UUID),
--   and passes listing.id to buyNFT instead of contract_sale_id. Neither is
--   accommodated by weakening types or identity checks. Mint/list helper calls
--   are not wired into createNft.jsx. A trusted indexer/verifier is still needed.
-- * Prices are exact decimal NEAR, <= 24 fractional digits and bounded by u128
--   yoctoNEAR. The frontend currently converts through JS Number; SQL cannot
--   recover precision already lost by the caller. Settlement uses exact amounts.
--
-- Safe application (operator steps; this file has NOT been applied):
-- 1. Review the identity/indexer prerequisites and blocked flows above. Confirm
--    the NEW project's reference in the dashboard, never rely on an old CLI link.
-- 2. Test the full file on a disposable new Supabase project first. It requires
--    Supabase-managed auth.users/auth.sessions, auth.uid/auth.jwt, storage schema,
--    and anon/authenticated/service_role roles; plain PostgreSQL is insufficient.
-- 3. In the NEW project's SQL Editor, as the migration administrator, run the
--    complete BEGIN...COMMIT file once. Do not run snippets or the old migration.
--    On any error roll back and investigate; do not drop existing data to force it.
--    Pre-existing Storage write policies also abort this fresh-project migration;
--    review those separately, since permissive policies combine with OR.
--    If later adopting CLI migrations, reconcile its migration history first:
--    SQL Editor execution does not register this version in the CLI ledger.
-- 4. Verify anonymous reads and denied writes; ordinary/forged-metadata sessions
--    denied all writes; verified users cannot alter another user, bind wallets,
--    skip taps, overwrite artwork, activate sales, transfer ownership, or call
--    reconciliation. Test duplicate/concurrent rewards and purchase replay with
--    trusted fixtures; invalid/reused payment data must leave both rows unchanged.
-- 5. Verify the bucket's public flag, 5242880-byte limit, MIME allowlist, scoped
--    object policies, FK embeds, and updated_at behavior before production use.
--    Keep service-role credentials exclusively server-side. Use a forward
--    migration for changes; do not reset a populated project.

begin;

do $$
begin
  if (
    select count(*) from pg_catalog.pg_class
    where oid in ('storage.objects'::regclass, 'storage.buckets'::regclass) and relrowsecurity
  ) <> 2 then
    raise exception 'Expected Supabase storage.objects and storage.buckets with RLS enabled';
  end if;
  if exists (
    select 1 from pg_catalog.pg_policies
    where schemaname = 'storage' and tablename in ('objects', 'buckets') and cmd <> 'SELECT'
  ) then
    raise exception 'Expected fresh Storage policies; audit existing write policies before proceeding';
  end if;
end;
$$;

create schema marketplace_private;
revoke all on schema marketplace_private from public, anon, authenticated;
grant usage on schema marketplace_private to authenticated, service_role;

create table public.profiles (
  id uuid primary key references auth.users(id) on delete restrict,
  username text unique check (char_length(username) between 1 and 30),
  display_name text check (char_length(display_name) <= 100),
  avatar_url text,
  bio text check (char_length(bio) <= 2000),
  wallet_address text unique check (
    char_length(wallet_address) between 2 and 64
    and wallet_address ~ '^[a-z0-9]+([._-][a-z0-9]+)*$'
  ),
  tap_count integer not null default 0 check (tap_count >= 0),
  last_tap_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

-- Only a trusted verifier writes this table. A profile-level "verified" flag
-- would be unsafe: anyone could sign into that profile with its known password.
create table marketplace_private.verified_wallet_sessions (
  session_id uuid primary key references auth.sessions(id) on delete cascade,
  user_id uuid not null references public.profiles(id) on delete cascade,
  wallet_address text not null,
  verified_at timestamptz not null default now(),
  expires_at timestamptz not null,
  constraint verification_lifetime check (
    expires_at > verified_at and expires_at <= verified_at + interval '24 hours'
  )
);
create index verified_wallet_sessions_user_idx
  on marketplace_private.verified_wallet_sessions(user_id);
alter table marketplace_private.verified_wallet_sessions enable row level security;
revoke all on marketplace_private.verified_wallet_sessions from public, anon, authenticated;
grant select, insert, update, delete on marketplace_private.verified_wallet_sessions to service_role;
-- No client RLS policies: service_role is the trusted BYPASSRLS role.

create function marketplace_private.has_verified_wallet_session()
returns boolean
language sql stable security definer
set search_path = ''
as $$
  select exists (
    select 1
    from marketplace_private.verified_wallet_sessions v
    join auth.sessions s on s.id = v.session_id and s.user_id = v.user_id
    join public.profiles p on p.id = v.user_id and p.wallet_address = v.wallet_address
    where v.user_id = (select auth.uid())
      and v.session_id::text = (select auth.jwt() ->> 'session_id')
      and (s.not_after is null or s.not_after > clock_timestamp())
      and v.verified_at <= statement_timestamp()
      and v.expires_at > statement_timestamp()
  );
$$;
revoke all on function marketplace_private.has_verified_wallet_session() from public, anon, authenticated;
grant execute on function marketplace_private.has_verified_wallet_session() to authenticated, service_role;

create table public.nfts (
  id uuid primary key default gen_random_uuid(),
  creator_id uuid not null references public.profiles(id) on delete restrict,
  name text not null check (char_length(btrim(name)) between 1 and 100),
  description text check (char_length(description) <= 1000),
  image_url text not null check (char_length(btrim(image_url)) > 0),
  image_path text unique,
  category text not null check (char_length(btrim(category)) between 1 and 100),
  price numeric not null default 0 check (
    price >= 0 and price <= 340282366920938.463463374607431768211455
    and scale(price) <= 24
  ),
  currency text not null default 'NEAR' check (currency = 'NEAR'),
  source text not null default 'user' check (source in ('user', 'game_reward')),
  network_id text not null default 'testnet' check (network_id in ('testnet', 'mainnet')),
  token_id text check (octet_length(token_id) between 1 and 1024),
  nft_contract_id text check (
    char_length(nft_contract_id) between 2 and 64
    and nft_contract_id ~ '^[a-z0-9]+([._-][a-z0-9]+)*$'
  ),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint complete_token_identity check ((token_id is null) = (nft_contract_id is null)),
  constraint game_rewards_are_offchain check (source <> 'game_reward' or token_id is null),
  constraint canonical_image_path check (
    image_path is null or image_path ~
      '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\.(png|jpg|jpeg|webp)$'
  ),
  unique (network_id, nft_contract_id, token_id)
);

create table public.nft_ownership (
  nft_id uuid primary key references public.nfts(id) on delete cascade,
  owner_id uuid not null references public.profiles(id) on delete restrict,
  acquired_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

create table public.marketplace_listings (
  id uuid primary key default gen_random_uuid(),
  nft_id uuid not null references public.nfts(id) on delete restrict,
  seller_id uuid not null references public.profiles(id) on delete restrict,
  buyer_id uuid references public.profiles(id) on delete restrict,
  price numeric not null check (
    price > 0 and price <= 340282366920938.463463374607431768211455
    and scale(price) <= 24
  ),
  currency text not null default 'NEAR' check (currency = 'NEAR'),
  status text not null default 'pending' check (status in ('pending', 'active', 'sold', 'cancelled')),
  network_id text not null default 'testnet' check (network_id in ('testnet', 'mainnet')),
  marketplace_contract_id text check (
    char_length(marketplace_contract_id) between 2 and 64
    and marketplace_contract_id ~ '^[a-z0-9]+([._-][a-z0-9]+)*$'
  ),
  contract_sale_id text check (octet_length(contract_sale_id) between 1 and 1024),
  -- Final purchase transaction only; listing-publication hashes are not stored here.
  tx_hash text check (tx_hash ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$'),
  onchain_status text not null default 'unverified'
    check (onchain_status in ('unverified', 'pending', 'confirmed', 'failed')),
  sold_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint complete_sale_identity check ((marketplace_contract_id is null) = (contract_sale_id is null)),
  constraint active_sales_are_verified check (
    status not in ('active', 'sold') or
    (onchain_status = 'confirmed' and contract_sale_id is not null and marketplace_contract_id is not null)
  ),
  constraint complete_purchase_record check (
    (status = 'sold' and buyer_id is not null and sold_at is not null and tx_hash is not null)
    or (status <> 'sold' and buyer_id is null and sold_at is null and tx_hash is null)
  ),
  constraint no_self_purchase check (buyer_id is null or buyer_id <> seller_id)
);

create table public.nft_likes (
  id uuid primary key default gen_random_uuid(),
  nft_id uuid not null references public.nfts(id) on delete cascade,
  user_id uuid not null references public.profiles(id) on delete cascade,
  created_at timestamptz not null default now(),
  unique (nft_id, user_id)
);

create table public.reward_claims (
  id uuid primary key default gen_random_uuid(),
  user_id uuid not null references public.profiles(id) on delete restrict,
  reward_level integer not null check (reward_level in (1, 2, 3)),
  nft_id uuid not null unique references public.nfts(id) on delete restrict,
  created_at timestamptz not null default now(),
  unique (user_id, reward_level)
);

-- FK indexes and indexes matching collection, market, likes and leaderboard reads.
create index profiles_leaderboard_idx on public.profiles(tap_count desc, id);
create index nfts_creator_idx on public.nfts(creator_id, created_at desc);
create index nfts_created_idx on public.nfts(created_at desc);
create index nft_ownership_owner_idx on public.nft_ownership(owner_id, acquired_at desc);
create index listings_nft_idx on public.marketplace_listings(nft_id);
create index listings_seller_idx on public.marketplace_listings(seller_id, created_at desc);
create index listings_buyer_idx on public.marketplace_listings(buyer_id) where buyer_id is not null;
create index listings_active_created_idx on public.marketplace_listings(created_at desc) where status = 'active';
create unique index listings_one_active_per_nft_idx on public.marketplace_listings(nft_id) where status = 'active';
create unique index listings_live_contract_sale_idx
  on public.marketplace_listings(network_id, marketplace_contract_id, contract_sale_id)
  where status in ('pending', 'active') and contract_sale_id is not null;
create unique index listings_purchase_tx_idx
  on public.marketplace_listings(network_id, tx_hash) where tx_hash is not null;
create index nft_likes_user_idx on public.nft_likes(user_id, nft_id);

create function marketplace_private.set_updated_at()
returns trigger language plpgsql
set search_path = ''
as $$
begin
  new.updated_at := statement_timestamp();
  return new;
end;
$$;

create function marketplace_private.guard_profile_update()
returns trigger language plpgsql
set search_path = ''
as $$
begin
  -- Wallet identities are historical identities, not editable profile cosmetics.
  if new.id is distinct from old.id or
     (old.wallet_address is not null and new.wallet_address is distinct from old.wallet_address) then
    raise exception 'Profile identity is immutable' using errcode = '23514';
  end if;
  -- current_user is the caller: this trigger is intentionally SECURITY INVOKER.
  if current_user = 'authenticated' then
    if new.tap_count is distinct from old.tap_count then
      if new.tap_count::bigint <> old.tap_count::bigint + 1 then
        raise exception 'Tap count must increase by exactly one' using errcode = '23514';
      end if;
      if old.last_tap_at is not null and
         clock_timestamp() < old.last_tap_at + interval '100 milliseconds' then
        raise exception 'Tap submitted too quickly' using errcode = '23514';
      end if;
      new.last_tap_at := clock_timestamp();
    end if;
  end if;
  return new;
end;
$$;

create function marketplace_private.prepare_nft_image()
returns trigger language plpgsql security definer
set search_path = ''
as $$
declare
  v_url_path text;
  v_storage_public_prefix constant text :=
    'https://llqwiqhwrbfoxoqqgczy.supabase.co/storage/v1/object/public/nft-images/';
begin
  if new.source = 'game_reward' then
    return new;
  end if;
  -- Compare the raw deployment prefix, then require the canonical_image_path
  -- constraint. A supplied image_path must match too; it cannot bypass the URL.
  if starts_with(new.image_url, v_storage_public_prefix) then
    v_url_path := nullif(substring(new.image_url from char_length(v_storage_public_prefix) + 1), '');
  end if;
  if new.image_path is null then
    new.image_path := v_url_path;
  end if;
  if new.image_path is null or new.image_path is distinct from v_url_path or
     split_part(new.image_path, '/', 1) <> new.creator_id::text then
    raise exception 'NFT image must reference the creator''s nft-images upload' using errcode = '23514';
  end if;
  if not exists (
    select 1 from storage.objects o
    where o.bucket_id = 'nft-images' and o.name = new.image_path
      and o.owner_id = new.creator_id::text
  ) then
    raise exception 'NFT image upload does not exist or belongs to another user' using errcode = '23514';
  end if;
  return new;
end;
$$;

create function marketplace_private.assign_initial_ownership()
returns trigger language plpgsql security definer
set search_path = ''
as $$
begin
  insert into public.nft_ownership(nft_id, owner_id) values (new.id, new.creator_id);
  return new;
end;
$$;

create function marketplace_private.validate_listing()
returns trigger language plpgsql security definer
set search_path = ''
as $$
declare
  v_nft public.nfts%rowtype;
  v_owner uuid;
begin
  if tg_op = 'UPDATE' then
    if new.id is distinct from old.id or new.nft_id is distinct from old.nft_id or
       new.seller_id is distinct from old.seller_id or new.network_id is distinct from old.network_id then
      raise exception 'Listing identity is immutable' using errcode = '23514';
    end if;
    if old.status in ('sold', 'cancelled') then
      raise exception 'Terminal listings are immutable; create a new listing' using errcode = '23514';
    end if;
  end if;
  select * into strict v_nft from public.nfts where id = new.nft_id for update;
  if v_nft.source = 'game_reward' or v_nft.network_id <> new.network_id then
    raise exception 'NFT is not eligible for this marketplace listing' using errcode = '23514';
  end if;
  if new.status in ('active', 'sold') and v_nft.token_id is null then
    raise exception 'On-chain token identity is required' using errcode = '23514';
  end if;
  if new.status in ('pending', 'active') then
    select owner_id into v_owner from public.nft_ownership where nft_id = new.nft_id for update;
    if v_owner is distinct from new.seller_id then
      raise exception 'Seller is not the current owner' using errcode = '23514';
    end if;
  end if;
  return new;
end;
$$;

create trigger profiles_guard before update on public.profiles
  for each row execute function marketplace_private.guard_profile_update();
create trigger profiles_updated_at before update on public.profiles
  for each row execute function marketplace_private.set_updated_at();
create trigger nfts_prepare_image before insert or update of image_url, image_path on public.nfts
  for each row execute function marketplace_private.prepare_nft_image();
create trigger nfts_initial_ownership after insert on public.nfts
  for each row execute function marketplace_private.assign_initial_ownership();
create trigger nfts_updated_at before update on public.nfts
  for each row execute function marketplace_private.set_updated_at();
create trigger ownership_updated_at before update on public.nft_ownership
  for each row execute function marketplace_private.set_updated_at();
create trigger listings_validate before insert or update on public.marketplace_listings
  for each row execute function marketplace_private.validate_listing();
create trigger listings_updated_at before update on public.marketplace_listings
  for each row execute function marketplace_private.set_updated_at();

-- Explicit table AND column privileges: RLS alone does not protect sensitive fields.
revoke all on public.profiles, public.nfts, public.nft_ownership,
  public.marketplace_listings, public.nft_likes, public.reward_claims
  from public, anon, authenticated;
grant select on public.profiles, public.nfts, public.nft_ownership,
  public.marketplace_listings, public.nft_likes to anon, authenticated;
grant select on public.reward_claims to authenticated;
grant update (username, display_name, avatar_url, bio, tap_count, updated_at)
  on public.profiles to authenticated;
grant insert (creator_id, name, description, image_url, image_path, category, price, currency)
  on public.nfts to authenticated;
grant delete on public.nfts to authenticated;
grant insert (nft_id, user_id) on public.nft_likes to authenticated;
grant delete on public.nft_likes to authenticated;
grant select, insert, update, delete on public.profiles, public.nfts,
  public.nft_ownership, public.marketplace_listings, public.nft_likes,
  public.reward_claims to service_role;

alter table public.profiles enable row level security;
alter table public.nfts enable row level security;
alter table public.nft_ownership enable row level security;
alter table public.marketplace_listings enable row level security;
alter table public.nft_likes enable row level security;
alter table public.reward_claims enable row level security;

create policy profiles_read on public.profiles for select to anon, authenticated using (true);
create policy profiles_update on public.profiles for update to authenticated
  using (id = (select auth.uid()) and (select marketplace_private.has_verified_wallet_session()))
  with check (id = (select auth.uid()) and (select marketplace_private.has_verified_wallet_session()));

create policy nfts_read on public.nfts for select to anon, authenticated using (true);
create policy nfts_insert on public.nfts for insert to authenticated
  with check (
    creator_id = (select auth.uid()) and (select marketplace_private.has_verified_wallet_session())
    and source = 'user' and token_id is null and nft_contract_id is null
  );
create policy nfts_delete on public.nfts for delete to authenticated
  using (
    creator_id = (select auth.uid()) and (select marketplace_private.has_verified_wallet_session())
    and source = 'user' and token_id is null
    and exists (select 1 from public.nft_ownership o where o.nft_id = nfts.id and o.owner_id = (select auth.uid()))
    and not exists (select 1 from public.marketplace_listings l where l.nft_id = nfts.id)
  );

create policy ownership_read on public.nft_ownership for select to anon, authenticated using (true);
create policy listings_read on public.marketplace_listings for select to anon, authenticated using (true);
-- Ownership/listing writes have no client policies or grants, including DELETE.

create policy likes_read on public.nft_likes for select to anon, authenticated using (true);
create policy likes_insert on public.nft_likes for insert to authenticated
  with check (user_id = (select auth.uid()) and (select marketplace_private.has_verified_wallet_session()));
create policy likes_delete on public.nft_likes for delete to authenticated
  using (user_id = (select auth.uid()) and (select marketplace_private.has_verified_wallet_session()));
create policy rewards_read on public.reward_claims for select to authenticated
  using (user_id = (select auth.uid()) and (select marketplace_private.has_verified_wallet_session()));
-- Claim writes exist only inside the checked RPC (or the trusted backend).

create function public.claim_game_reward(p_reward_level integer)
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
    else raise exception 'Reward level must be 1, 2, or 3' using errcode = '22023';
  end case;
  -- Serialize claims/taps per user; duplicates return the original award.
  select tap_count into strict v_taps from public.profiles where id = v_user for update;
  select nft_id into v_nft from public.reward_claims where user_id = v_user and reward_level = p_reward_level;
  if found then return v_nft; end if;
  if v_taps < v_required then
    raise exception 'Not enough taps for this reward' using errcode = '23514';
  end if;
  insert into public.nfts(creator_id, name, description, image_url, category, price, source)
  values (v_user, v_name, 'Off-chain NEAR Tap Game collectible; no on-chain token or cash value.',
    '/rewards/' || p_reward_level::text || '.jpeg', 'Gaming', 0, 'game_reward')
  returning id into v_nft;
  -- Initial ownership is assigned by the same trigger used for catalog entries.
  insert into public.reward_claims(user_id, reward_level, nft_id) values (v_user, p_reward_level, v_nft);
  return v_nft;
end;
$$;

-- Retain the exact legacy RPC signature solely to reject the unpaid fallback.
create function public.complete_nft_purchase(p_listing_id uuid, p_transaction_hash text)
returns uuid language plpgsql
set search_path = ''
as $$
begin
  raise exception 'Purchases require final on-chain verification by the trusted backend; browser settlement is disabled'
    using errcode = '42501';
end;
$$;

-- Trusted reconciliation, NOT a chain verifier. Input facts must come from
-- independently verified final chain state. Matching text/hash syntax is not proof.
-- Invoker security + EXECUTE ACL: only service_role (and migration owner) can run.
create function public.reconcile_nft_purchase(
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
     v_nft.source = 'game_reward' then
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
  -- Both changes are atomic; the unique network/tx index prevents payment reuse
  -- across listings, including concurrent attempts. Errors roll back this call.
  update public.marketplace_listings
    set status = 'sold', buyer_id = v_buyer, tx_hash = p_transaction_hash,
        sold_at = statement_timestamp(), onchain_status = 'confirmed'
    where id = v_listing.id;
  update public.nft_ownership set owner_id = v_buyer, acquired_at = statement_timestamp()
    where nft_id = v_listing.nft_id;
  return v_listing.id;
end;
$$;

-- Functions otherwise inherit PostgreSQL's default PUBLIC EXECUTE privilege.
revoke all on function marketplace_private.set_updated_at(),
  marketplace_private.guard_profile_update(), marketplace_private.prepare_nft_image(),
  marketplace_private.assign_initial_ownership(), marketplace_private.validate_listing()
  from public, anon, authenticated;
revoke all on function public.claim_game_reward(integer),
  public.complete_nft_purchase(uuid, text),
  public.reconcile_nft_purchase(uuid, text, text, text, text, text, text, text, text, numeric)
  from public, anon, authenticated;
grant execute on function public.claim_game_reward(integer), public.complete_nft_purchase(uuid, text)
  to authenticated;
grant execute on function public.reconcile_nft_purchase(uuid, text, text, text, text, text, text, text, text, numeric)
  to service_role;

insert into storage.buckets(id, name, public, file_size_limit, allowed_mime_types)
values ('nft-images', 'nft-images', true, 5242880, array['image/png', 'image/jpeg', 'image/webp']);

-- storage.objects RLS is managed/enabled by Supabase. Do not alter ownership or
-- blanket grants on its system tables. Bucket-scoped policies are additive; this
-- fresh-project precondition matters (audit any future policies for broad grants).
create policy nft_images_read on storage.objects for select to anon, authenticated
  using (bucket_id = 'nft-images');
create policy nft_images_insert on storage.objects for insert to authenticated
  with check (
    bucket_id = 'nft-images'
    and (select marketplace_private.has_verified_wallet_session())
    and owner_id = (select auth.uid())::text
    and name ~ ('^' || (select auth.uid())::text ||
      '/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\.(png|jpg|jpeg|webp)$')
  );
-- No UPDATE/DELETE policy: upload(..., { upsert: false }) is supported, mutable
-- artwork and browser deletion are not. MIME/size checks come from Storage API;
-- a backend accepting direct object writes must perform equivalent validation.

commit;
