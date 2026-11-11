-- Trusted marketplace withdrawal lifecycle: seller proceeds, marketplace fees,
-- buyer refunds, storage withdrawals and registration-excess refunds.
--
-- Rows begin as 'pending' only after the trusted worker has verified the signed
-- request against a canonical final transaction. They may become 'succeeded' or
-- 'failed' only together with the callback block evidence. SQL never infers a
-- completed payout from a request transaction, a browser claim or a bare event.
begin;

create table public.marketplace_withdrawals (
  id uuid primary key default gen_random_uuid(),
  network_id text not null check (network_id in ('testnet', 'mainnet')),
  marketplace_contract_id text not null check (
    char_length(marketplace_contract_id) between 2 and 64
    and marketplace_contract_id ~ '^[a-z0-9]+([._-][a-z0-9]+)*$'
  ),
  account_wallet_address text not null check (
    char_length(account_wallet_address) between 2 and 64
    and account_wallet_address ~ '^[a-z0-9]+([._-][a-z0-9]+)*$'
  ),
  recipient_wallet_address text not null check (
    char_length(recipient_wallet_address) between 2 and 64
    and recipient_wallet_address ~ '^[a-z0-9]+([._-][a-z0-9]+)*$'
  ),
  kind text not null check (kind in (
    'seller_proceeds', 'marketplace_fees', 'buyer_refund',
    'storage_balance', 'registration_excess'
  )),
  amount_yocto numeric not null check (
    amount_yocto > 0
    and scale(amount_yocto) = 0
    and amount_yocto <= 340282366920938463463374607431768211455
  ),
  -- NEAR withdrawal nonce, bounded by u64.
  nonce numeric(20, 0) not null check (nonce > 0 and nonce <= 18446744073709551615),
  -- Marketplace sale ID for buyer refunds; null for every other kind.
  reference_sale_id text check (octet_length(reference_sale_id) between 1 and 1024),
  request_transaction_hash text not null check (
    request_transaction_hash ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$'
  ),
  request_block_hash text not null check (
    request_block_hash ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$'
  ),
  request_block_height bigint not null check (request_block_height > 0),
  status text not null default 'pending' check (status in ('pending', 'succeeded', 'failed')),
  outcome_event text check (outcome_event in ('withdrawal_succeeded', 'withdrawal_failed')),
  failure_reason text check (char_length(failure_reason) <= 500),
  callback_block_hash text check (callback_block_hash ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$'),
  callback_block_height bigint check (callback_block_height > 0),
  confirmed_at timestamptz,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  -- Completion requires callback evidence from a block at or after the request.
  constraint withdrawal_completion_evidence check (
    (status = 'pending' and outcome_event is null and callback_block_hash is null
      and callback_block_height is null and confirmed_at is null)
    or
    (status = 'succeeded' and outcome_event = 'withdrawal_succeeded'
      and callback_block_hash is not null and callback_block_height is not null
      and callback_block_height >= request_block_height and confirmed_at is not null)
    or
    (status = 'failed' and outcome_event = 'withdrawal_failed'
      and callback_block_hash is not null and callback_block_height is not null
      and callback_block_height >= request_block_height and confirmed_at is not null)
  ),
  constraint buyer_refund_has_sale check (
    (kind = 'buyer_refund' and reference_sale_id is not null)
    or (kind <> 'buyer_refund' and reference_sale_id is null)
  ),
  unique (network_id, marketplace_contract_id, request_transaction_hash),
  unique (network_id, marketplace_contract_id, account_wallet_address, nonce)
);

create index marketplace_withdrawals_pending_idx
  on public.marketplace_withdrawals(created_at)
  where status = 'pending';
create index marketplace_withdrawals_account_idx
  on public.marketplace_withdrawals(network_id, marketplace_contract_id, account_wallet_address, created_at desc);
create index marketplace_withdrawals_kind_idx
  on public.marketplace_withdrawals(kind, status, created_at desc);

create trigger withdrawals_updated_at before update on public.marketplace_withdrawals
  for each row execute function marketplace_private.set_updated_at();

alter table public.marketplace_withdrawals enable row level security;
revoke all on public.marketplace_withdrawals from public, anon, authenticated;
grant select, insert, update, delete on public.marketplace_withdrawals to service_role;

-- Operator-only view: pending intents are safe to retry; they never claim a
-- completed payout. Granting it to service_role keeps it off the public API.
create view public.marketplace_withdrawal_reconciliation as
  select id, network_id, marketplace_contract_id, account_wallet_address,
    recipient_wallet_address, kind, amount_yocto, nonce, reference_sale_id,
    request_transaction_hash, request_block_height, status, created_at,
    now() - created_at as pending_for
  from public.marketplace_withdrawals
  where status = 'pending';
revoke all on public.marketplace_withdrawal_reconciliation from public, anon, authenticated;
grant select on public.marketplace_withdrawal_reconciliation to service_role;

-- Record (or replay) a verified withdrawal request. The worker calls this only
-- after validating the signed call and the final transaction receipt graph.
create function public.record_withdrawal_intent(
  p_transaction_hash text,
  p_network_id text,
  p_marketplace_contract_id text,
  p_account_wallet_address text,
  p_recipient_wallet_address text,
  p_kind text,
  p_amount_yocto numeric,
  p_nonce numeric,
  p_reference_sale_id text,
  p_request_block_hash text,
  p_request_block_height bigint
)
returns uuid language plpgsql
set search_path = ''
as $$
declare
  v_existing public.marketplace_withdrawals%rowtype;
  v_id uuid;
begin
  if p_transaction_hash is null or p_transaction_hash !~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$' or
     p_request_block_hash is null or p_request_block_hash !~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$' or
     p_request_block_height is null or p_request_block_height <= 0 then
    raise exception 'Invalid withdrawal request evidence' using errcode = '22023';
  end if;
  if p_network_id not in ('testnet', 'mainnet') or
     p_marketplace_contract_id is null or
     p_marketplace_contract_id !~ '^[a-z0-9]+([._-][a-z0-9]+)*$' or
     p_account_wallet_address is null or
     p_account_wallet_address !~ '^[a-z0-9]+([._-][a-z0-9]+)*$' or
     p_recipient_wallet_address is null or
     p_recipient_wallet_address !~ '^[a-z0-9]+([._-][a-z0-9]+)*$' then
    raise exception 'Invalid withdrawal identity' using errcode = '22023';
  end if;
  if p_kind is null or p_kind not in (
    'seller_proceeds', 'marketplace_fees', 'buyer_refund',
    'storage_balance', 'registration_excess'
  ) then
    raise exception 'Invalid withdrawal kind' using errcode = '22023';
  end if;
  if p_amount_yocto is null or p_amount_yocto <= 0 or scale(p_amount_yocto) <> 0 or
     p_amount_yocto > 340282366920938463463374607431768211455 or
     p_nonce is null or p_nonce <= 0 or p_nonce > 18446744073709551615 or
     scale(p_nonce) <> 0 then
    raise exception 'Invalid withdrawal amount or nonce' using errcode = '22023';
  end if;
  if (p_kind = 'buyer_refund') <> (p_reference_sale_id is not null) then
    raise exception 'Only buyer refunds carry a sale reference' using errcode = '22023';
  end if;

  select * into v_existing from public.marketplace_withdrawals
    where network_id = p_network_id
      and marketplace_contract_id = p_marketplace_contract_id
      and request_transaction_hash = p_transaction_hash
    for update;
  if found then
    if v_existing.account_wallet_address <> p_account_wallet_address or
       v_existing.recipient_wallet_address <> p_recipient_wallet_address or
       v_existing.kind <> p_kind or v_existing.amount_yocto <> p_amount_yocto or
       v_existing.nonce <> p_nonce or
       v_existing.reference_sale_id is distinct from p_reference_sale_id or
       v_existing.request_block_hash <> p_request_block_hash or
       v_existing.request_block_height <> p_request_block_height then
      raise exception 'Withdrawal transaction conflicts with recorded facts' using errcode = '23514';
    end if;
    return v_existing.id;
  end if;

  if exists (
    select 1 from public.marketplace_withdrawals
    where network_id = p_network_id
      and marketplace_contract_id = p_marketplace_contract_id
      and account_wallet_address = p_account_wallet_address
      and nonce = p_nonce
  ) then
    raise exception 'Withdrawal nonce was already used by another transaction' using errcode = '23505';
  end if;

  insert into public.marketplace_withdrawals(
    network_id, marketplace_contract_id, account_wallet_address,
    recipient_wallet_address, kind, amount_yocto, nonce, reference_sale_id,
    request_transaction_hash, request_block_hash, request_block_height
  ) values (
    p_network_id, p_marketplace_contract_id, p_account_wallet_address,
    p_recipient_wallet_address, p_kind, p_amount_yocto, p_nonce, p_reference_sale_id,
    p_transaction_hash, p_request_block_hash, p_request_block_height
  ) returning id into v_id;
  return v_id;
end;
$$;

-- Promote a pending intent only after the transfer callback succeeded. Replaying
-- identical chain facts is idempotent; conflicting terminal facts are rejected.
create function public.confirm_withdrawal(
  p_transaction_hash text,
  p_network_id text,
  p_marketplace_contract_id text,
  p_nonce numeric,
  p_amount_yocto numeric,
  p_callback_block_hash text,
  p_callback_block_height bigint
)
returns uuid language plpgsql
set search_path = ''
as $$
declare
  v_row public.marketplace_withdrawals%rowtype;
begin
  if p_callback_block_hash is null or p_callback_block_hash !~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$' or
     p_callback_block_height is null or p_callback_block_height <= 0 then
    raise exception 'Withdrawal completion requires callback block evidence' using errcode = '22023';
  end if;
  select * into strict v_row from public.marketplace_withdrawals
    where network_id = p_network_id
      and marketplace_contract_id = p_marketplace_contract_id
      and request_transaction_hash = p_transaction_hash
    for update;
  if v_row.nonce <> p_nonce or v_row.amount_yocto <> p_amount_yocto then
    raise exception 'Withdrawal confirmation conflicts with recorded facts' using errcode = '23514';
  end if;
  if v_row.status = 'succeeded' then
    if v_row.callback_block_hash = p_callback_block_hash and
       v_row.callback_block_height = p_callback_block_height then
      return v_row.id;
    end if;
    raise exception 'Withdrawal was already completed with different block evidence' using errcode = '23514';
  end if;
  if v_row.status = 'failed' then
    raise exception 'Withdrawal was already recorded as failed' using errcode = '23514';
  end if;
  update public.marketplace_withdrawals set status = 'succeeded',
    outcome_event = 'withdrawal_succeeded',
    callback_block_hash = p_callback_block_hash,
    callback_block_height = p_callback_block_height,
    confirmed_at = statement_timestamp()
    where id = v_row.id;
  return v_row.id;
end;
$$;

-- Record a verified transfer failure. The withdrawal remains retryable on chain
-- with a new nonce; this row must never be promoted to success afterwards.
create function public.fail_withdrawal(
  p_transaction_hash text,
  p_network_id text,
  p_marketplace_contract_id text,
  p_nonce numeric,
  p_amount_yocto numeric,
  p_callback_block_hash text,
  p_callback_block_height bigint,
  p_failure_reason text default null
)
returns uuid language plpgsql
set search_path = ''
as $$
declare
  v_row public.marketplace_withdrawals%rowtype;
begin
  if p_callback_block_hash is null or p_callback_block_hash !~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$' or
     p_callback_block_height is null or p_callback_block_height <= 0 then
    raise exception 'Withdrawal failure requires callback block evidence' using errcode = '22023';
  end if;
  if p_failure_reason is not null and char_length(p_failure_reason) > 500 then
    raise exception 'Withdrawal failure reason is too long' using errcode = '22023';
  end if;
  select * into strict v_row from public.marketplace_withdrawals
    where network_id = p_network_id
      and marketplace_contract_id = p_marketplace_contract_id
      and request_transaction_hash = p_transaction_hash
    for update;
  if v_row.nonce <> p_nonce or v_row.amount_yocto <> p_amount_yocto then
    raise exception 'Withdrawal failure conflicts with recorded facts' using errcode = '23514';
  end if;
  if v_row.status = 'failed' then
    if v_row.callback_block_hash = p_callback_block_hash and
       v_row.callback_block_height = p_callback_block_height then
      return v_row.id;
    end if;
    raise exception 'Withdrawal was already recorded as failed with different evidence' using errcode = '23514';
  end if;
  if v_row.status = 'succeeded' then
    raise exception 'Withdrawal was already completed' using errcode = '23514';
  end if;
  update public.marketplace_withdrawals set status = 'failed',
    outcome_event = 'withdrawal_failed',
    failure_reason = p_failure_reason,
    callback_block_hash = p_callback_block_hash,
    callback_block_height = p_callback_block_height,
    confirmed_at = statement_timestamp()
    where id = v_row.id;
  return v_row.id;
end;
$$;

revoke all on function public.record_withdrawal_intent(
  text, text, text, text, text, text, numeric, numeric, text, text, bigint
) from public, anon, authenticated;
revoke all on function public.confirm_withdrawal(
  text, text, text, numeric, numeric, text, bigint
) from public, anon, authenticated;
revoke all on function public.fail_withdrawal(
  text, text, text, numeric, numeric, text, bigint, text
) from public, anon, authenticated;
grant execute on function public.record_withdrawal_intent(
  text, text, text, text, text, text, numeric, numeric, text, text, bigint
) to service_role;
grant execute on function public.confirm_withdrawal(
  text, text, text, numeric, numeric, text, bigint
) to service_role;
grant execute on function public.fail_withdrawal(
  text, text, text, numeric, numeric, text, bigint, text
) to service_role;

commit;
