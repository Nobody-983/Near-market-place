-- Remove the later text overload. It bypassed the original listing/NFT checks,
-- referenced a nonexistent transaction_hash column, and inherited PUBLIC
-- execute. The UUID/numeric function from 20261029000100 is the sole sink.
begin;

drop function if exists public.reconcile_nft_purchase(
  text, text, text, text, text, text, text, text, text, text
);

-- A contract listing identity is immutable and may settle only once, including
-- after it leaves the pending/active partial index.
create unique index if not exists listings_contract_sale_identity_idx
  on public.marketplace_listings(network_id, marketplace_contract_id, contract_sale_id)
  where contract_sale_id is not null;

-- PostgreSQL grants function execution to PUBLIC by default. Restate the exact
-- service-role-only boundary after removing the overload.
revoke all on function public.reconcile_nft_purchase(
  uuid, text, text, text, text, text, text, text, text, numeric
) from public, anon, authenticated;
grant execute on function public.reconcile_nft_purchase(
  uuid, text, text, text, text, text, text, text, text, numeric
) to service_role;

commit;
