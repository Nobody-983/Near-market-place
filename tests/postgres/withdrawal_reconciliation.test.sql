\set ON_ERROR_STOP on

-- Runs after all migrations. Only service_role may record or complete payouts.
do $$
declare
  v_id uuid;
  v_replay uuid;
  v_pending integer;
begin
  if has_function_privilege('authenticated',
      'public.record_withdrawal_intent(text,text,text,text,text,text,numeric,numeric,text,text,bigint)', 'EXECUTE') or
     has_function_privilege('anon',
      'public.confirm_withdrawal(text,text,text,numeric,numeric,text,bigint)', 'EXECUTE') or
     not has_function_privilege('service_role',
      'public.fail_withdrawal(text,text,text,numeric,numeric,text,bigint,text)', 'EXECUTE') or
     has_table_privilege('authenticated', 'public.marketplace_withdrawals', 'SELECT') or
     not has_table_privilege('service_role', 'public.marketplace_withdrawals', 'UPDATE') then
    raise exception 'withdrawal reconciliation ACLs are incorrect';
  end if;
end;
$$;

insert into auth.users(id, email) values
  ('33333333-3333-4333-8333-333333333333', 'withdrawer@example.invalid');
insert into public.profiles(id, wallet_address, username) values
  ('33333333-3333-4333-8333-333333333333', 'withdrawer.testnet', 'withdrawer');

do $$
declare
  v_id uuid;
  v_replay uuid;
  v_pending integer;
begin
  -- A verified request starts pending and cannot claim completion.
  v_id := public.record_withdrawal_intent(
    repeat('A', 32), 'testnet', 'market.roundfrog-dev.testnet',
    'withdrawer.testnet', 'withdrawer.testnet', 'seller_proceeds',
    1000, 1, null, repeat('B', 32), 100);
  if not exists (
    select 1 from public.marketplace_withdrawals
    where id = v_id and status = 'pending' and outcome_event is null and confirmed_at is null
  ) then
    raise exception 'withdrawal intent was not recorded as pending';
  end if;

  v_replay := public.record_withdrawal_intent(
    repeat('A', 32), 'testnet', 'market.roundfrog-dev.testnet',
    'withdrawer.testnet', 'withdrawer.testnet', 'seller_proceeds',
    1000, 1, null, repeat('B', 32), 100);
  if v_replay <> v_id then
    raise exception 'exact withdrawal replay was not idempotent';
  end if;

  begin
    perform public.record_withdrawal_intent(
      repeat('A', 32), 'testnet', 'market.roundfrog-dev.testnet',
      'withdrawer.testnet', 'withdrawer.testnet', 'seller_proceeds',
      999, 1, null, repeat('B', 32), 100);
    raise exception 'conflicting withdrawal replay was accepted';
  exception when check_violation then null;
  end;

  begin
    perform public.record_withdrawal_intent(
      repeat('C', 32), 'testnet', 'market.roundfrog-dev.testnet',
      'withdrawer.testnet', 'withdrawer.testnet', 'seller_proceeds',
      1000, 1, null, repeat('B', 32), 100);
    raise exception 'withdrawal nonce reuse was accepted';
  exception when unique_violation then null;
  end;

  begin
    perform public.record_withdrawal_intent(
      repeat('D', 32), 'testnet', 'market.roundfrog-dev.testnet',
      'withdrawer.testnet', 'withdrawer.testnet', 'buyer_refund',
      1000, 2, null, repeat('B', 32), 100);
    raise exception 'buyer refund without a sale reference was accepted';
  exception when invalid_parameter_value then null;
  end;

  begin
    perform public.confirm_withdrawal(
      repeat('A', 32), 'testnet', 'market.roundfrog-dev.testnet', 1, 1000, 'not-a-hash', 101);
    raise exception 'completion without callback evidence was accepted';
  exception when invalid_parameter_value then null;
  end;

  v_replay := public.confirm_withdrawal(
    repeat('A', 32), 'testnet', 'market.roundfrog-dev.testnet', 1, 1000, repeat('E', 32), 101);
  if v_replay <> v_id then
    raise exception 'withdrawal confirmation returned a different row';
  end if;
  if not exists (
    select 1 from public.marketplace_withdrawals
    where id = v_id and status = 'succeeded'
      and outcome_event = 'withdrawal_succeeded'
      and callback_block_hash = repeat('E', 32)
      and callback_block_height = 101 and confirmed_at is not null
  ) then
    raise exception 'confirmed withdrawal lacks callback evidence';
  end if;

  v_replay := public.confirm_withdrawal(
    repeat('A', 32), 'testnet', 'market.roundfrog-dev.testnet', 1, 1000, repeat('E', 32), 101);
  if v_replay <> v_id then
    raise exception 'exact confirmation replay was not idempotent';
  end if;

  begin
    perform public.confirm_withdrawal(
      repeat('A', 32), 'testnet', 'market.roundfrog-dev.testnet', 1, 1000, repeat('F', 32), 102);
    raise exception 'completion with different block evidence was accepted';
  exception when check_violation then null;
  end;

  begin
    perform public.fail_withdrawal(
      repeat('A', 32), 'testnet', 'market.roundfrog-dev.testnet', 1, 1000, repeat('F', 32), 102);
    raise exception 'a completed withdrawal was marked failed';
  exception when check_violation then null;
  end;

  -- A verified transfer failure is terminal for that request but remains visible.
  v_id := public.record_withdrawal_intent(
    repeat('G', 32), 'testnet', 'market.roundfrog-dev.testnet',
    'withdrawer.testnet', 'withdrawer.testnet', 'storage_balance',
    750, 2, null, repeat('B', 32), 100);
  perform public.fail_withdrawal(
    repeat('G', 32), 'testnet', 'market.roundfrog-dev.testnet', 2, 750,
    repeat('H', 32), 102, 'transfer failed');
  if not exists (
    select 1 from public.marketplace_withdrawals
    where id = v_id and status = 'failed' and outcome_event = 'withdrawal_failed'
      and callback_block_height = 102 and failure_reason = 'transfer failed'
  ) then
    raise exception 'failed withdrawal lacks callback evidence';
  end if;

  begin
    perform public.confirm_withdrawal(
      repeat('G', 32), 'testnet', 'market.roundfrog-dev.testnet', 2, 750, repeat('H', 32), 102);
    raise exception 'a failed withdrawal was promoted to success';
  exception when check_violation then null;
  end;

  -- Direct table writes cannot fabricate a completed row either.
  begin
    insert into public.marketplace_withdrawals(
      network_id, marketplace_contract_id, account_wallet_address,
      recipient_wallet_address, kind, amount_yocto, nonce,
      request_transaction_hash, request_block_hash, request_block_height,
      status, outcome_event, confirmed_at
    ) values (
      'testnet', 'market.roundfrog-dev.testnet', 'withdrawer.testnet',
      'withdrawer.testnet', 'seller_proceeds', 1000, 3,
      repeat('J', 32), repeat('B', 32), 100,
      'succeeded', 'withdrawal_succeeded', now());
    raise exception 'completion without callback fields was accepted';
  exception when check_violation then null;
  end;

  select count(*) into v_pending
    from public.marketplace_withdrawal_reconciliation;
  if v_pending <> 0 then
    raise exception 'pending reconciliation view should be empty';
  end if;
end;
$$;
