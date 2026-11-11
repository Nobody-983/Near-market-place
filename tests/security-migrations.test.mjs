// Offline SQL source/constraint checks; these do not execute either migration.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile } from 'node:fs/promises';

const walletAuthSql = await readFile(
  new URL('../supabase/migrations/20261029000200_near_wallet_auth.sql', import.meta.url),
  'utf8',
);
const marketplaceSql = await readFile(
  new URL('../supabase/migrations/20261029000100_near_marketplace.sql', import.meta.url),
  'utf8',
);
const settlementHardeningSql = await readFile(
  new URL('../supabase/migrations/20261103000100_harden_settlement_reconcile.sql', import.meta.url),
  'utf8',
);
const catalogIndexingSql = await readFile(
  new URL('../supabase/migrations/20261105000100_catalog_chain_indexing.sql', import.meta.url),
  'utf8',
);
const withdrawalSql = await readFile(
  new URL('../supabase/migrations/20261106000100_withdrawal_reconciliation.sql', import.meta.url),
  'utf8',
);
const deploymentDocs = await readFile(
  new URL('../supabase/NEAR_WALLET_AUTH.md', import.meta.url),
  'utf8',
);

function functionDefinition(sql, name) {
  const start = sql.indexOf(`create function ${name}(`);
  const end = sql.indexOf('\n$$;', start);
  assert.ok(start >= 0 && end > start, `${name} must exist`);
  return sql.slice(start, end).replace(/--[^\n]*/g, '');
}

const issueChallengeSql = functionDefinition(walletAuthSql, 'public.wallet_auth_issue_challenge');
const prepareImageSql = functionDefinition(marketplaceSql, 'marketplace_private.prepare_nft_image');

test('challenge issuance rejects global lock contention without waiting', () => {
  assert.match(
    issueChallengeSql,
    /begin\s+if not pg_try_advisory_xact_lock\(hashtextextended\('near-wallet:challenge-issuance', 0\)\) then\s+raise exception 'Too many wallet challenges; retry later';\s+end if;/,
  );
  assert.doesNotMatch(
    issueChallengeSql,
    /pg_advisory_xact_lock\(hashtextextended\('near-wallet:challenge-issuance', 0\)\)/,
  );
});

test('challenge cleanup, quota and insert stay in one transaction after the issuance gate', () => {
  const steps = [
    "pg_try_advisory_xact_lock(hashtextextended('near-wallet:challenge-issuance', 0))",
    'delete from marketplace_private.wallet_auth_challenges where expires_at <= v_now;',
    "where created_at > v_now - interval '1 minute') >= 60",
    'insert into marketplace_private.wallet_auth_challenges(',
    'return p_challenge;',
  ].map((statement) => issueChallengeSql.indexOf(statement));
  assert.ok(steps.every((offset, index) => offset >= 0 && (index === 0 || offset > steps[index - 1])));
  assert.doesNotMatch(issueChallengeSql, /\b(?:commit|rollback|exception\s+when|pg_advisory_unlock\w*)\b/i,
    'quota errors must propagate and transaction-scoped locks must not be released early');
  assert.match(walletAuthSql, /\bbegin;\s+create table/);
  assert.match(walletAuthSql, /\bcommit;\s*$/);
});

test('anonymous callers cannot bypass the Edge Function via the challenge RPC', () => {
  assert.match(walletAuthSql,
    /revoke all on function[^;]*public\.wallet_auth_issue_challenge\(jsonb, text\)[^;]*from public, anon, authenticated;/);
  assert.match(walletAuthSql,
    /grant execute on function[^;]*public\.wallet_auth_issue_challenge\(jsonb, text\)[^;]*to service_role;/);
  assert.doesNotMatch(walletAuthSql, /grant execute on function[^;]*to\b[^;]*\b(?:public|anon|authenticated)\b/);
});

test('user NFT images require the configured Supabase public bucket prefix', () => {
  const expectedPrefix =
    'https://llqwiqhwrbfoxoqqgczy.supabase.co/storage/v1/object/public/nft-images/';
  assert.ok(prepareImageSql.includes(`v_storage_public_prefix constant text :=\n    '${expectedPrefix}'`));
  assert.match(prepareImageSql, /v_url_path text;/, 'unmatched URLs must leave the path NULL');
  assert.match(prepareImageSql,
    /if starts_with\(new\.image_url, v_storage_public_prefix\) then\s+v_url_path := nullif\(substring\(new\.image_url from char_length\(v_storage_public_prefix\) \+ 1\), ''\);\s+end if;/);
  assert.match(prepareImageSql,
    /if new\.image_path is null then\s+new\.image_path := v_url_path;\s+end if;/);
  assert.match(prepareImageSql,
    /if new\.image_path is null or new\.image_path is distinct from v_url_path or\s+split_part\(new\.image_path, '\/', 1\) <> new\.creator_id::text then\s+raise exception [^;]+using errcode = '23514';/);
  assert.match(prepareImageSql,
    /if not exists \(\s+select 1 from storage\.objects o\s+where o\.bucket_id = 'nft-images' and o\.name = new\.image_path\s+and o\.owner_id = new\.creator_id::text\s+\) then\s+raise exception [^;]+using errcode = '23514';/);
  assert.match(marketplaceSql,
    /create trigger nfts_prepare_image before insert or update of image_url, image_path on public\.nfts\s+for each row execute function marketplace_private\.prepare_nft_image\(\);/);
});

test('the stored canonical path constraint excludes URL suffix and traversal tricks', () => {
  const source = marketplaceSql.match(/constraint canonical_image_path check \(\s+image_path is null or image_path ~\s+'([^']+)'/)?.[1];
  assert.ok(source, 'canonical image path constraint must exist');
  // Exercise the actual ASCII regexp from the CHECK, not a simulated SQL trigger.
  const pattern = new RegExp(source);
  const owner = '11111111-1111-4111-8111-111111111111';
  const object = '22222222-2222-4222-8222-222222222222';
  const path = `${owner}/${object}.png`;
  for (const extension of ['png', 'jpg', 'jpeg', 'webp']) {
    assert.equal(pattern.exec(`${owner}/${object}.${extension}`)?.[0], `${owner}/${object}.${extension}`);
  }
  for (const invalid of [
    '', '/', `/${path}`, `${owner}//${object}.png`, `${owner}/../${path}`,
    `${owner}/%2e%2e/${object}.png`, `${owner}%2f${object}.png`,
    `${owner}\\${object}.png`, `${owner}/${object}%2epng`,
    `${path}?redirect=https://attacker.example`, `${path}#fragment`,
    `${path}/more`, `${path}.svg`, `https://attacker.example/${path}`,
  ]) {
    assert.equal(pattern.test(invalid), false, `must reject ${JSON.stringify(invalid)}`);
  }
});

test('deployment docs leave trusted per-IP challenge limiting explicitly unresolved', () => {
  assert.match(deploymentDocs, /\*\*Unresolved: configure a trusted gateway rate limit before production\.\*\*/);
  assert.match(deploymentDocs, /10 requests per rolling 60 seconds per directly observed client source IP/);
  assert.match(deploymentDocs, /burst of 10/);
  assert.match(deploymentDocs, /shared across all gateway replicas and both challenge phases/);
  assert.match(deploymentDocs, /never from a client-supplied `X-Forwarded-For`/);
  assert.match(deploymentDocs, /subpaths and query strings/);
  assert.match(deploymentDocs, /direct `https:\/\/<project-ref>\.supabase\.co\/functions\/v1\/near-wallet-auth`/);
  assert.match(deploymentDocs, /reject traffic that bypasses the limiter/);
  assert.match(deploymentDocs, /11th request from one IP/);
  assert.match(deploymentDocs, /challenge denial-of-service\s+remains unresolved/);
});

test('favorites and deletion retain session verification, ownership and listing-history restrictions', () => {
  const deletion = marketplaceSql.match(/create policy nfts_delete\b[\s\S]*?;/)?.[0];
  assert.ok(deletion);
  for (const clause of [
    'creator_id = (select auth.uid())', 'marketplace_private.has_verified_wallet_session()',
    "source = 'user'", 'token_id is null', 'o.owner_id = (select auth.uid())',
    'not exists (select 1 from public.marketplace_listings l where l.nft_id = nfts.id)',
  ]) assert.ok(deletion.includes(clause), clause);
  assert.doesNotMatch(deletion, /l\.status/, 'all listing history prevents deletion');
  for (const name of ['likes_insert', 'likes_delete']) {
    const policy = marketplaceSql.match(new RegExp(`create policy ${name}\\b[\\s\\S]*?;`))?.[0];
    assert.ok(policy.includes('user_id = (select auth.uid())'));
    assert.ok(policy.includes('marketplace_private.has_verified_wallet_session()'));
  }
});

test('settlement stays service-only and browser Storage remains append-only', () => {
  const sql = marketplaceSql.replace(/--[^\n]*/g, '');
  assert.match(sql, /revoke all on function[^;]*public\.reconcile_nft_purchase\([^;]*from public, anon, authenticated;/);
  assert.match(sql, /grant execute on function public\.reconcile_nft_purchase\([^;]*to service_role;/);
  assert.doesNotMatch(sql, /grant execute on function[^;]*reconcile_nft_purchase[^;]*to (?:public|anon|authenticated)/);
  assert.match(functionDefinition(sql, 'public.complete_nft_purchase'), /raise exception '[^']*'\s+using errcode = '42501'/);
  const storagePolicies = sql.match(/create policy\s+\w+\s+on storage\.objects[\s\S]*?;/g) || [];
  assert.equal(storagePolicies.length, 2);
  assert.ok(storagePolicies.every((policy) => /for (?:select|insert)\b/.test(policy)));
  assert.doesNotMatch(sql, /grant\s+(?:insert|update|delete|all)[^;]*public\.(?:marketplace_listings|nft_ownership)[^;]*to\b[^;]*\b(?:anon|authenticated)\b/);
});

test('settlement hardening removes the unsafe text overload and makes sale identity permanent', () => {
  const sql = settlementHardeningSql.replace(/--[^\n]*/g, '');
  assert.match(sql, /drop function if exists public\.reconcile_nft_purchase\(\s*text(?:\s*,\s*text){9}\s*\);/);
  assert.match(sql, /create unique index if not exists listings_contract_sale_identity_idx[\s\S]*where contract_sale_id is not null;/);
  assert.match(sql, /revoke all on function public\.reconcile_nft_purchase\([\s\S]*?numeric\s*\) from public, anon, authenticated;/);
  assert.match(sql, /grant execute on function public\.reconcile_nft_purchase\([\s\S]*?numeric\s*\) to service_role;/);
});

test('trusted mint and listing indexing RPCs remain service-role only', () => {
  const sql = catalogIndexingSql.replace(/--[^\n]*/g, '');
  for (const signature of [
    'public\\.bind_verified_nft_mint\\(uuid, text, text, text, text, text\\)',
    'public\\.publish_verified_listing\\(text, text, text, text, text, text, text, numeric, numeric, numeric\\)',
    'public\\.cancel_verified_listing\\(text, text, text, text, text\\)',
  ]) {
    assert.match(sql, new RegExp(`revoke all on function[\\s\\S]*?${signature}[\\s\\S]*?from public, anon, authenticated;`));
    assert.match(sql, new RegExp(`grant execute on function[\\s\\S]*?${signature}[\\s\\S]*?to service_role;`));
  }
});

test('withdrawals cannot be completed without callback block evidence', () => {
  const sql = withdrawalSql.replace(/--[^\n]*/g, '');
  assert.match(sql, /constraint withdrawal_completion_evidence check \([\s\S]*?status = 'pending' and outcome_event is null and callback_block_hash is null[\s\S]*?status = 'succeeded' and outcome_event = 'withdrawal_succeeded'[\s\S]*?callback_block_height >= request_block_height[\s\S]*?\)/);
  assert.match(sql, /unique \(network_id, marketplace_contract_id, request_transaction_hash\)/);
  assert.match(sql, /unique \(network_id, marketplace_contract_id, account_wallet_address, nonce\)/);
  for (const name of [
    'public.record_withdrawal_intent\\(\\s*text, text, text, text, text, text, numeric, numeric, text, text, bigint',
    'public.confirm_withdrawal\\(\\s*text, text, text, numeric, numeric, text, bigint',
    'public.fail_withdrawal\\(\\s*text, text, text, numeric, numeric, text, bigint, text',
  ]) {
    assert.match(sql, new RegExp(`revoke all on function ${name}[\\s\\S]*?from public, anon, authenticated;`));
    assert.match(sql, new RegExp(`grant execute on function ${name}[\\s\\S]*?to service_role;`));
  }
  assert.doesNotMatch(sql, /grant execute on function[^;]*withdrawal[^;]*to (?:public|anon|authenticated)/i);
  assert.match(sql, /revoke all on public\.marketplace_withdrawal_reconciliation from public, anon, authenticated;/);
  assert.match(sql, /alter table public\.marketplace_withdrawals enable row level security;/);
  const intent = functionDefinition(sql, 'public.record_withdrawal_intent');
  assert.doesNotMatch(intent, /callback_block_(?:hash|height)\s*=/);
  assert.match(functionDefinition(sql, 'public.confirm_withdrawal'), /p_callback_block_hash is null or p_callback_block_hash !~ /);
  assert.match(functionDefinition(sql, 'public.fail_withdrawal'), /p_callback_block_hash is null or p_callback_block_hash !~ /);
});

test('cancellation replay is read-only and conflicting terminal facts are rejected', () => {
  const body = functionDefinition(catalogIndexingSql, 'public.cancel_verified_listing');
  const replayBranch = body.slice(
    body.indexOf("if v_listing.status = 'cancelled'"),
    body.indexOf("if v_listing.status <> 'active'"),
  );
  assert.match(body,
    /if v_listing\.status = 'cancelled' then\s+if v_listing\.cancellation_tx_hash = p_transaction_hash then\s+return v_listing\.id;/);
  assert.doesNotMatch(replayBranch, /update public\.marketplace_listings/);
  assert.match(body, /raise exception 'Listing was cancelled by different chain facts'/);
});
