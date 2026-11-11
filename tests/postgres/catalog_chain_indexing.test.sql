\set ON_ERROR_STOP on

insert into auth.users(id, email) values
  ('11111111-1111-4111-8111-111111111111', 'seller@example.invalid'),
  ('22222222-2222-4222-8222-222222222222', 'buyer@example.invalid');
insert into public.profiles(id, wallet_address, username) values
  ('11111111-1111-4111-8111-111111111111', 'seller.testnet', 'seller'),
  ('22222222-2222-4222-8222-222222222222', 'buyer.testnet', 'buyer');

insert into storage.objects(bucket_id, name, owner_id) values
  ('nft-images', '11111111-1111-4111-8111-111111111111/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa.png',
   '11111111-1111-4111-8111-111111111111'),
  ('nft-images', '11111111-1111-4111-8111-111111111111/bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb.png',
   '11111111-1111-4111-8111-111111111111');

insert into public.nfts(id, creator_id, name, image_url, category) values
  ('aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', '11111111-1111-4111-8111-111111111111',
   'Indexed NFT',
   'https://llqwiqhwrbfoxoqqgczy.supabase.co/storage/v1/object/public/nft-images/11111111-1111-4111-8111-111111111111/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa.png',
   'Art'),
  ('bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb', '11111111-1111-4111-8111-111111111111',
   'Second NFT',
   'https://llqwiqhwrbfoxoqqgczy.supabase.co/storage/v1/object/public/nft-images/11111111-1111-4111-8111-111111111111/bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb.png',
   'Art');

do $$
declare
  v_nft uuid := 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa';
  v_second uuid := 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb';
  v_listing uuid;
  v_replay uuid;
  v_replacement uuid;
begin
  if has_function_privilege('authenticated',
      'public.bind_verified_nft_mint(uuid,text,text,text,text,text)', 'EXECUTE') or
     has_function_privilege('anon',
      'public.publish_verified_listing(text,text,text,text,text,text,text,numeric,numeric,numeric)', 'EXECUTE') or
     not has_function_privilege('service_role',
      'public.cancel_verified_listing(text,text,text,text,text)', 'EXECUTE') then
    raise exception 'trusted indexing function ACLs are incorrect';
  end if;

  perform public.bind_verified_nft_mint(
    v_nft, repeat('A', 32), 'seller.testnet', 'testnet',
    'nft.roundfrog-dev.testnet', 'catalog-' || v_nft::text);
  perform public.bind_verified_nft_mint(
    v_nft, repeat('A', 32), 'seller.testnet', 'testnet',
    'nft.roundfrog-dev.testnet', 'catalog-' || v_nft::text);
  if not exists (select 1 from public.nfts where id = v_nft and onchain_status = 'confirmed'
      and mint_tx_hash = repeat('A', 32)) then
    raise exception 'verified mint was not bound';
  end if;

  begin
    perform public.bind_verified_nft_mint(
      v_nft, repeat('B', 32), 'seller.testnet', 'testnet',
      'nft.roundfrog-dev.testnet', 'catalog-' || v_nft::text);
    raise exception 'conflicting mint replay was accepted';
  exception when check_violation then null;
  end;

  begin
    perform public.bind_verified_nft_mint(
      v_second, repeat('A', 32), 'seller.testnet', 'testnet',
      'nft.roundfrog-dev.testnet', 'catalog-' || v_second::text);
    raise exception 'duplicate mint transaction was accepted';
  exception when unique_violation then null;
  end;

  begin
    perform public.publish_verified_listing(
      repeat('C', 32), 'buyer.testnet', 'testnet', 'market.roundfrog-dev.testnet',
      'sale-1', 'nft.roundfrog-dev.testnet', 'catalog-' || v_nft::text,
      1250000000000000000000000, 1, 1);
    raise exception 'non-owner listing was accepted';
  exception when check_violation then null;
  end;

  v_listing := public.publish_verified_listing(
    repeat('C', 32), 'seller.testnet', 'testnet', 'market.roundfrog-dev.testnet',
    'sale-1', 'nft.roundfrog-dev.testnet', 'catalog-' || v_nft::text,
    1250000000000000000000000, 1, 1);
  v_replay := public.publish_verified_listing(
    repeat('C', 32), 'seller.testnet', 'testnet', 'market.roundfrog-dev.testnet',
    'sale-1', 'nft.roundfrog-dev.testnet', 'catalog-' || v_nft::text,
    1250000000000000000000000, 1, 1);
  if v_listing is distinct from v_replay then raise exception 'listing replay changed identity'; end if;

  v_replacement := public.publish_verified_listing(
    repeat('D', 32), 'seller.testnet', 'testnet', 'market.roundfrog-dev.testnet',
    'sale-2', 'nft.roundfrog-dev.testnet', 'catalog-' || v_nft::text,
    2000000000000000000000000, 2, 2);
  if not exists (select 1 from public.marketplace_listings where id = v_listing and status = 'cancelled') or
     not exists (select 1 from public.marketplace_listings where id = v_replacement and status = 'active') then
    raise exception 'listing replacement did not preserve one active generation';
  end if;

  v_replay := public.cancel_verified_listing(
    repeat('E', 32), 'seller.testnet', 'testnet', 'market.roundfrog-dev.testnet', 'sale-2');
  if v_replay is distinct from v_replacement then raise exception 'wrong listing cancelled'; end if;
  v_replay := public.cancel_verified_listing(
    repeat('E', 32), 'seller.testnet', 'testnet', 'market.roundfrog-dev.testnet', 'sale-2');
  if v_replay is distinct from v_replacement then raise exception 'cancellation replay changed identity'; end if;
  begin
    perform public.cancel_verified_listing(
      repeat('F', 32), 'seller.testnet', 'testnet', 'market.roundfrog-dev.testnet', 'sale-2');
    raise exception 'conflicting cancellation replay was accepted';
  exception when check_violation then null;
  end;

  v_listing := public.publish_verified_listing(
    repeat('G', 32), 'seller.testnet', 'testnet', 'market.roundfrog-dev.testnet',
    'sale-3', 'nft.roundfrog-dev.testnet', 'catalog-' || v_nft::text,
    3000000000000000000000000, 3, 3);
  perform public.reconcile_nft_purchase(
    v_listing, repeat('H', 32), 'buyer.testnet', 'seller.testnet', 'testnet',
    'market.roundfrog-dev.testnet', 'sale-3', 'nft.roundfrog-dev.testnet',
    'catalog-' || v_nft::text, 3000000000000000000000000);
  perform public.reconcile_nft_purchase(
    v_listing, repeat('H', 32), 'buyer.testnet', 'seller.testnet', 'testnet',
    'market.roundfrog-dev.testnet', 'sale-3', 'nft.roundfrog-dev.testnet',
    'catalog-' || v_nft::text, 3000000000000000000000000);
  if not exists (select 1 from public.marketplace_listings where id = v_listing
      and status = 'sold' and buyer_id = '22222222-2222-4222-8222-222222222222') or
     not exists (select 1 from public.nft_ownership where nft_id = v_nft
      and owner_id = '22222222-2222-4222-8222-222222222222') then
    raise exception 'purchase reconciliation did not atomically settle ownership';
  end if;
end;
$$;

select 'catalog chain indexing behavior passed' as result;
