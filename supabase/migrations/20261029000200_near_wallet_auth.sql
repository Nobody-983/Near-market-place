-- Apply AFTER 20261029000100, only after review. Prepared, not applied.
-- No existing RLS policy or client grant is relaxed. These RPCs are service-only;
-- the Edge Function verifies NEP-413 and current NEAR FullAccess authority first.
-- Auth users are created through the Auth Admin API, never INSERTed into auth.users.
begin;

create table marketplace_private.wallet_auth_challenges (
  id uuid primary key,
  account_id text not null check (
    char_length(account_id) between 2 and 64 and account_id ~ '^[a-z0-9]+([._-][a-z0-9]+)*$'
  ),
  network_id text not null check (network_id in ('testnet', 'mainnet')),
  purpose text not null check (purpose in ('login', 'session')),
  user_id uuid references public.profiles(id) on delete cascade,
  session_id uuid references auth.sessions(id) on delete cascade,
  secret_hash text not null check (secret_hash ~ '^[0-9a-f]{64}$'),
  payload jsonb not null,
  created_at timestamptz not null default now(),
  expires_at timestamptz not null,
  consumed_at timestamptz,
  verified_at timestamptz,
  check (expires_at > created_at and expires_at <= created_at + interval '5 minutes'),
  check (
    (purpose = 'login' and user_id is null and session_id is null) or
    (purpose = 'session' and user_id is not null and session_id is not null)
  )
);
create index wallet_auth_challenges_rate_idx
  on marketplace_private.wallet_auth_challenges(account_id, network_id, created_at desc);
create index wallet_auth_challenges_expiry_idx on marketplace_private.wallet_auth_challenges(expires_at);
create index wallet_auth_challenges_user_idx on marketplace_private.wallet_auth_challenges(user_id);
create index wallet_auth_challenges_session_idx on marketplace_private.wallet_auth_challenges(session_id);
alter table marketplace_private.wallet_auth_challenges enable row level security;
revoke all on marketplace_private.wallet_auth_challenges from public, anon, authenticated;
grant select, insert, update, delete on marketplace_private.wallet_auth_challenges to service_role;
-- No client policies. Store no passwords, signatures, OTPs or session tokens here.

create function public.wallet_auth_identity(p_account_id text, p_network_id text)
returns jsonb language plpgsql security definer set search_path = ''
as $$
declare v_identity jsonb;
begin
  select jsonb_build_object('id', p.id, 'email', u.email, 'metadata', u.raw_app_meta_data -> 'near_wallet')
    into v_identity from public.profiles p join auth.users u on u.id = p.id
    where p.wallet_address = p_account_id;
  if not found then return null; end if;
  -- Never adopt a legacy password account based only on a matching email/wallet.
  if v_identity #>> '{metadata,version}' is distinct from '1' or
     v_identity #>> '{metadata,account_id}' is distinct from p_account_id or
     v_identity #>> '{metadata,network_id}' is distinct from p_network_id then
    raise exception 'Legacy wallet identity requires administrator review';
  end if;
  return v_identity - 'metadata';
end;
$$;

create function public.wallet_auth_provision(p_account_id text, p_network_id text, p_user_id uuid)
returns jsonb language plpgsql security definer set search_path = ''
as $$
declare v_identity jsonb; v_metadata jsonb;
begin
  -- Serialize first-login races, including profile creation after an Admin call.
  perform pg_advisory_xact_lock(hashtextextended('near-wallet:' || p_account_id, 0));
  v_identity := public.wallet_auth_identity(p_account_id, p_network_id);
  if v_identity is not null then return v_identity; end if;
  select raw_app_meta_data -> 'near_wallet' into v_metadata from auth.users where id = p_user_id;
  if not found or v_metadata ->> 'version' is distinct from '1' or
     v_metadata ->> 'account_id' is distinct from p_account_id or
     v_metadata ->> 'network_id' is distinct from p_network_id then
    raise exception 'Trusted Auth identity is required';
  end if;
  insert into public.profiles(id, wallet_address, username, display_name)
    values (p_user_id, p_account_id, 'near_' || left(replace(p_user_id::text, '-', ''), 25),
      'NEAR ' || left(p_account_id, 12));
  return public.wallet_auth_identity(p_account_id, p_network_id);
end;
$$;

create function public.wallet_auth_context(
  p_account_id text, p_network_id text, p_user_id uuid, p_session_id uuid
)
returns jsonb language plpgsql security definer set search_path = ''
as $$
declare v_identity jsonb; v_profile jsonb; v_expiry timestamptz;
begin
  v_identity := public.wallet_auth_identity(p_account_id, p_network_id);
  if v_identity is null or (v_identity ->> 'id')::uuid is distinct from p_user_id or not exists (
    select 1 from auth.sessions where id = p_session_id and user_id = p_user_id
      and (not_after is null or not_after > clock_timestamp())
  ) then
    raise exception 'Wallet and Auth session do not match';
  end if;
  select to_jsonb(p) into v_profile from public.profiles p where id = p_user_id;
  select expires_at into v_expiry from marketplace_private.verified_wallet_sessions
    where session_id = p_session_id and user_id = p_user_id and wallet_address = p_account_id
      and verified_at <= clock_timestamp() and expires_at > clock_timestamp();
  return jsonb_build_object('user', jsonb_build_object('id', p_user_id), 'sessionId', p_session_id, 'profile', v_profile,
    'verified', v_expiry is not null, 'verifiedUntil', v_expiry);
end;
$$;

create function public.wallet_auth_issue_challenge(p_challenge jsonb, p_secret_hash text)
returns jsonb language plpgsql security definer set search_path = ''
as $$
declare v_now timestamptz := clock_timestamp();
begin
  -- Login callers are unauthenticated, so an account-scoped quota would let an
  -- attacker spend a victim's allowance. Serialize a bounded global issuance gate
  -- instead, and remove expired rows so rotating account IDs cannot grow storage.
  -- Reject contention before any writes. The transaction owns the lock; commit
  -- or rollback releases it. Let quota errors abort the RPC and roll back cleanup.
  if not pg_try_advisory_xact_lock(hashtextextended('near-wallet:challenge-issuance', 0)) then
    raise exception 'Too many wallet challenges; retry later';
  end if;
  delete from marketplace_private.wallet_auth_challenges where expires_at <= v_now;
  if (select count(*) from marketplace_private.wallet_auth_challenges
      where created_at > v_now - interval '1 minute') >= 60 then
    raise exception 'Too many wallet challenges; retry later';
  end if;
  if p_challenge ->> 'phase' = 'session' then
    -- Session challenges are authenticated and bound to this wallet/session, so
    -- retaining the narrower quota cannot be spent by an anonymous caller.
    perform pg_advisory_xact_lock(hashtextextended('near-wallet:' || (p_challenge ->> 'accountId'), 0));
    if (select count(*) from marketplace_private.wallet_auth_challenges
        where account_id = p_challenge ->> 'accountId' and network_id = p_challenge ->> 'networkId'
          and purpose = 'session' and created_at > v_now - interval '1 minute') >= 6 then
      raise exception 'Too many wallet challenges; retry later';
    end if;
    perform public.wallet_auth_context(p_challenge ->> 'accountId', p_challenge ->> 'networkId',
      (p_challenge ->> 'userId')::uuid, (p_challenge ->> 'sessionId')::uuid);
  end if;
  if (p_challenge ->> 'expiresAt')::timestamptz <= v_now then
    raise exception 'Challenge expired before issuance';
  end if;
  insert into marketplace_private.wallet_auth_challenges(
    id, account_id, network_id, purpose, user_id, session_id, secret_hash, payload, expires_at
  ) values (
    (p_challenge ->> 'id')::uuid, p_challenge ->> 'accountId', p_challenge ->> 'networkId',
    p_challenge ->> 'phase', (p_challenge ->> 'userId')::uuid, (p_challenge ->> 'sessionId')::uuid,
    p_secret_hash, p_challenge, (p_challenge ->> 'expiresAt')::timestamptz
  );
  return p_challenge;
end;
$$;

create function public.wallet_auth_take_challenge(p_id uuid, p_secret_hash text)
returns jsonb language plpgsql security definer set search_path = ''
as $$
declare v_payload jsonb;
begin
  -- A single UPDATE is the replay/concurrency boundary, before external calls.
  -- Failed signatures or RPC failures consume the attempt; start a fresh challenge.
  update marketplace_private.wallet_auth_challenges set consumed_at = clock_timestamp()
    where id = p_id and secret_hash = p_secret_hash and consumed_at is null
      and expires_at > clock_timestamp()
    returning payload into v_payload;
  if not found then raise exception 'Challenge expired, invalid or already used'; end if;
  return v_payload;
end;
$$;

create function public.wallet_auth_attach_session(p_challenge_id uuid)
returns jsonb language plpgsql security definer set search_path = ''
as $$
declare v_challenge marketplace_private.wallet_auth_challenges%rowtype;
  v_now timestamptz; v_expiry timestamptz;
begin
  -- Only call AFTER server-side signature and final FullAccess verification.
  select * into strict v_challenge from marketplace_private.wallet_auth_challenges where id = p_challenge_id;
  -- Match logout's lock order so a delayed proof cannot re-grant a revoked session.
  perform 1 from auth.sessions where id = v_challenge.session_id and user_id = v_challenge.user_id for update;
  if not found then raise exception 'Auth session no longer exists'; end if;
  select * into strict v_challenge from marketplace_private.wallet_auth_challenges
    where id = p_challenge_id for update;
  v_now := clock_timestamp();
  if v_challenge.purpose <> 'session' or v_challenge.consumed_at is null or
     v_challenge.verified_at is not null or v_challenge.expires_at <= v_now then
    raise exception 'Session challenge is not eligible for verification';
  end if;
  perform public.wallet_auth_context(v_challenge.account_id, v_challenge.network_id,
    v_challenge.user_id, v_challenge.session_id);
  select least(v_now + interval '1 hour', not_after) into v_expiry from auth.sessions
    where id = v_challenge.session_id and user_id = v_challenge.user_id for share;
  insert into marketplace_private.verified_wallet_sessions(
    session_id, user_id, wallet_address, verified_at, expires_at
  ) values (v_challenge.session_id, v_challenge.user_id, v_challenge.account_id, v_now, v_expiry)
  on conflict (session_id) do update set user_id = excluded.user_id,
    wallet_address = excluded.wallet_address, verified_at = excluded.verified_at, expires_at = excluded.expires_at;
  update marketplace_private.wallet_auth_challenges set verified_at = v_now where id = p_challenge_id;
  return public.wallet_auth_context(v_challenge.account_id, v_challenge.network_id,
    v_challenge.user_id, v_challenge.session_id);
end;
$$;

create function public.wallet_auth_revoke_session(p_user_id uuid, p_session_id uuid)
returns void language plpgsql security definer set search_path = ''
as $$
begin
  perform 1 from auth.sessions where id = p_session_id and user_id = p_user_id for update;
  delete from marketplace_private.wallet_auth_challenges
    where user_id = p_user_id and session_id = p_session_id;
  delete from marketplace_private.verified_wallet_sessions
    where user_id = p_user_id and session_id = p_session_id;
end;
$$;

revoke all on function public.wallet_auth_identity(text, text),
  public.wallet_auth_provision(text, text, uuid), public.wallet_auth_context(text, text, uuid, uuid),
  public.wallet_auth_issue_challenge(jsonb, text), public.wallet_auth_take_challenge(uuid, text),
  public.wallet_auth_attach_session(uuid), public.wallet_auth_revoke_session(uuid, uuid)
  from public, anon, authenticated;
grant execute on function public.wallet_auth_identity(text, text),
  public.wallet_auth_provision(text, text, uuid), public.wallet_auth_context(text, text, uuid, uuid),
  public.wallet_auth_issue_challenge(jsonb, text), public.wallet_auth_take_challenge(uuid, text),
  public.wallet_auth_attach_session(uuid), public.wallet_auth_revoke_session(uuid, uuid)
  to service_role;
commit;
