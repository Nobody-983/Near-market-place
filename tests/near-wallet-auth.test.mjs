// Offline protocol/integration tests. Supabase/NEAR HTTP and database RPCs are
// mocked; SQL execution and real wallet/browser tests remain deployment checks.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import vm from 'node:vm';
import { createWalletAuthHandler } from '../supabase/functions/near-wallet-auth/handler.mjs';
import { base64, serializeNep413, sha256Hex, verifyNep413 } from '../supabase/functions/near-wallet-auth/nep413.mjs';

const require = createRequire(new URL('../frontend/Nft/package.json', import.meta.url));
const near = require('near-api-js');
const sdk = require('@near-wallet-selector/core');
const { Buffer: BrowserBuffer } = require('buffer/');
const key = near.KeyPair.fromRandom('ed25519');
const accountId = 'alice.testnet';
const userId = '11111111-1111-4111-8111-111111111111';
const sessionId = '22222222-2222-4222-8222-222222222222';
const otherSession = '33333333-3333-4333-8333-333333333333';
const secret = 'ab'.repeat(32);
const origin = 'https://market.example';
const config = { supabaseUrl: 'https://project.example', appOrigin: origin,
  networkId: 'testnet', serviceRoleKey: 'server-only-test-credential' };
const jwt = (sid = sessionId) => 'header.' + Buffer.from(JSON.stringify({ sub: userId, session_id: sid, role: 'authenticated' })).toString('base64url') + '.signature';
const sign = (challenge, signer = key) => ({ accountId: challenge.accountId, publicKey: signer.getPublicKey().toString(),
  signature: base64(signer.sign(createHash('sha256').update(sdk.serializeNep413({ ...challenge, nonce: Buffer.from(challenge.nonce, 'base64') })).digest()).signature) });

function backend() {
  const state = { now: Date.now(), challenges: new Map(), calls: [], identity: null, permission: 'FullAccess',
    verified: false, sessionActive: true, failUser: false, failNear: false, provisioned: 0,
    issueChallengeError: null };
  const response = (data, status = 200) => new Response(JSON.stringify(data), { status, headers: { 'Content-Type': 'application/json' } });
  const reject = (message) => response({ message }, 400);
  const profile = { id: userId, wallet_address: accountId, username: 'alice' };
  const context = (sid = sessionId) => ({ user: { id: userId }, profile, sessionId: sid, verified: state.verified,
    verifiedUntil: state.verified ? new Date(state.now + 3600000).toISOString() : null });
  const handler = createWalletAuthHandler(config, { now: () => state.now, fetchImpl: async (url, options) => {
    const body = options.body ? JSON.parse(options.body) : null;
    state.calls.push({ url, body, headers: options.headers });
    if (url === 'https://rpc.testnet.near.org') {
      assert.equal(options.headers.Authorization, undefined);
      assert.equal(body.params.finality, 'final');
      assert.equal(body.params.account_id, accountId);
      assert.equal(body.params.public_key, key.getPublicKey().toString());
      if (state.failNear) throw new Error('offline');
      return response({ result: { permission: state.permission } });
    }
    assert.ok(url.startsWith(config.supabaseUrl + '/'), 'no unconfigured HTTP destination');
    assert.equal(options.headers.apikey, config.serviceRoleKey);
    if (url.endsWith('/auth/v1/user')) {
      if (state.failUser || ![jwt(), jwt(otherSession)].includes(options.headers.Authorization.slice(7))) return response({}, 401);
      return response({ id: userId });
    }
    assert.equal(options.headers.Authorization, `Bearer ${config.serviceRoleKey}`);
    if (state.rpcUnavailable && url.includes('/rest/v1/rpc/')) return response({ code: 'PGRST202' }, 404);
    if (url.endsWith('/auth/v1/admin/users')) {
      assert.equal(body.password, undefined);
      assert.equal(body.app_metadata.near_wallet.account_id, accountId);
      assert.ok(body.email.endsWith('@wallet.invalid'));
      state.provisioned++;
      return response({ id: userId });
    }
    if (url.endsWith('/auth/v1/admin/generate_link')) return response({ id: userId, hashed_token: 'server-only-otp', action_link: 'server-only-link' });
    if (url.endsWith('/auth/v1/verify')) {
      assert.deepEqual(body, { type: 'magiclink', token_hash: 'server-only-otp' });
      return response({ user: { id: userId }, access_token: jwt(), refresh_token: 'test-refresh' });
    }
    const name = url.split('/').pop();
    if (name === 'wallet_auth_issue_challenge') {
      if (state.issueChallengeError) return response({ message: state.issueChallengeError,
        code: 'P0001', details: 'private database detail', hint: 'private database hint' }, 400);
      state.challenges.set(body.p_challenge.id, { payload: structuredClone(body.p_challenge), hash: body.p_secret_hash, consumed: false });
      return response(body.p_challenge);
    }
    if (name === 'wallet_auth_take_challenge') {
      const item = state.challenges.get(body.p_id);
      if (!item || item.hash !== body.p_secret_hash || item.consumed || Date.parse(item.payload.expiresAt) <= state.now) return reject('Challenge expired, invalid or already used');
      item.consumed = true;
      return response(item.payload);
    }
    if (name === 'wallet_auth_identity') return state.legacy ? reject('Legacy wallet identity requires administrator review') : response(state.identity);
    if (name === 'wallet_auth_provision') {
      state.identity = { id: userId, email: 'internal@wallet.invalid' };
      return response(state.identity);
    }
    if (name === 'wallet_auth_context') {
      if (!state.sessionActive || body.p_user_id !== userId || ![sessionId, otherSession].includes(body.p_session_id) || body.p_account_id !== accountId) return reject('Wallet and Auth session do not match');
      return response(context(body.p_session_id));
    }
    if (name === 'wallet_auth_attach_session') {
      const item = state.challenges.get(body.p_challenge_id);
      assert.equal(item.payload.phase, 'session'); assert.equal(item.consumed, true);
      state.verified = true;
      return response(context());
    }
    if (name === 'wallet_auth_revoke_session') {
      state.verified = false;
      for (const [id, item] of state.challenges) if (item.payload.sessionId === body.p_session_id) state.challenges.delete(id);
      return response(null);
    }
    throw new Error(`Unexpected offline test request: ${url}`);
  } });
  const call = async (body, token, requestOrigin = origin) => {
    const result = await handler(new Request(config.supabaseUrl + '/functions/v1/near-wallet-auth', {
      method: 'POST', headers: { Origin: requestOrigin, 'Content-Type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
      body: JSON.stringify({ networkId: 'testnet', ...body }),
    }));
    return { status: result.status, body: await result.json() };
  };
  const challenge = async (phase = 'login', token, redirect = false) => {
    const result = await call({ action: 'challenge', accountId, phase, requestHash: await sha256Hex(secret), redirect }, token);
    assert.equal(result.status, 200);
    return result.body.challenge;
  };
  const verify = (challenge, token, signed = sign(challenge), requestSecret = secret) => call({ action: 'verify', challengeId: challenge.id, signed, requestSecret }, token);
  return { state, call, challenge, verify, handler };
}

await test('NEP-413 ASCII challenges match the installed Wallet Selector SDK, including optional callback', async () => {
  for (const callbackUrl of [undefined, `${origin}/login`]) {
    const challenge = { accountId, message: 'Sign in to NEAR NFT', nonce: base64(new Uint8Array(32).fill(7)), recipient: 'market.example', callbackUrl };
    assert.deepEqual(Buffer.from(serializeNep413(challenge)), sdk.serializeNep413({ ...challenge, nonce: Buffer.from(challenge.nonce, 'base64') }));
    const signed = sign(challenge);
    assert.equal(await verifyNep413(challenge, signed), true);
    for (const changed of [{ ...challenge, message: 'different' }, { ...challenge, recipient: 'attacker.example' },
      { ...challenge, nonce: base64(new Uint8Array(32).fill(8)) }, { ...challenge, callbackUrl: 'https://other.example' }]) {
      assert.equal(await verifyNep413(changed, signed), false);
    }
    for (const changed of [{ ...signed, accountId: 'other.testnet' }, { ...signed, signature: 'bad' }, { ...signed, publicKey: 'secp256k1:bad' }]) {
      assert.equal(await verifyNep413(challenge, changed), false);
    }
  }
});

await test('Borsh string serialization uses UTF-8 byte lengths, not JavaScript UTF-16 lengths', () => {
  // Independent fixed wire fixture: u32 tag, u32 length 6, UTF-8 "é🎨", 32 zero
  // nonce bytes, u32 recipient length 1, ASCII x, absent callback discriminator.
  // Installed borsh@1.0.0 has a Unicode bug; production challenges are ASCII.
  const expected = Buffer.from('9d01008006000000c3a9f09f8ea8' + '00'.repeat(32) + '010000007800', 'hex');
  assert.deepEqual(Buffer.from(serializeNep413({ message: 'é🎨', nonce: base64(new Uint8Array(32)), recipient: 'x' })), expected);
});

await test('challenge issuance rejects contention and bounds anonymous growth without a victim-controlled login quota', async () => {
  const sql = await readFile(new URL('../supabase/migrations/20261029000200_near_wallet_auth.sql', import.meta.url), 'utf8');
  const issue = sql.match(/create function public\.wallet_auth_issue_challenge[\s\S]*?(?=create function public\.wallet_auth_take_challenge)/)?.[0];
  assert.ok(issue, 'challenge issuance function must exist');
  assert.match(issue,
    /if not pg_try_advisory_xact_lock\(hashtextextended\('near-wallet:challenge-issuance', 0\)\) then\s+raise exception 'Too many wallet challenges; retry later';\s+end if;/);
  assert.doesNotMatch(issue,
    /pg_advisory_xact_lock\(hashtextextended\('near-wallet:challenge-issuance', 0\)\)/);
  assert.match(issue, /delete from marketplace_private\.wallet_auth_challenges where expires_at <= v_now/);
  assert.match(issue, /where created_at > v_now - interval '1 minute'\) >= 60/);
  const sessionBranch = issue.indexOf("if p_challenge ->> 'phase' = 'session' then");
  const accountQuota = issue.indexOf('where account_id =');
  assert.ok(sessionBranch >= 0 && accountQuota > sessionBranch,
    'only authenticated session challenges may consume an account-scoped quota');
  assert.equal([...issue.matchAll(/where account_id =/g)].length, 1);

  for (const phase of ['login', 'session']) {
    const b = backend();
    const token = phase === 'session' ? jwt() : undefined;
    b.state.issueChallengeError = 'Too many wallet challenges; retry later';
    const denied = await b.call({ action: 'challenge', accountId, phase, requestHash: await sha256Hex(secret) }, token);
    assert.equal(denied.status, 429);
    assert.deepEqual(denied.body, { error: 'Too many attempts; retry later', code: 'AUTH_REJECTED' });
    assert.equal(b.state.challenges.size, 0);
    assert.equal(b.state.provisioned, 0);
    assert.deepEqual(b.state.calls.map(({ url }) => new URL(url).pathname), [
      ...(phase === 'session' ? ['/auth/v1/user', '/rest/v1/rpc/wallet_auth_context'] : []),
      '/rest/v1/rpc/wallet_auth_issue_challenge',
    ], 'a rejected challenge must not be retried or proceed to proof/provisioning');
    b.state.issueChallengeError = null;
    await b.challenge(phase, token);
    assert.equal(b.state.challenges.size, 1, 'a later admitted request still succeeds');
  }
});

await test('client-supplied IP headers are not forwarded as trusted challenge metadata', async () => {
  const b = backend();
  const untrusted = {
    'X-Forwarded-For': '198.51.100.8, 203.0.113.9',
    Forwarded: 'for=198.51.100.8;proto=https',
    'X-Real-IP': '198.51.100.8',
    'CF-Connecting-IP': '198.51.100.8',
  };
  const result = await b.handler(new Request(config.supabaseUrl + '/functions/v1/near-wallet-auth', {
    method: 'POST', headers: { ...untrusted, Origin: origin, 'Content-Type': 'application/json' },
    body: JSON.stringify({ action: 'challenge', networkId: 'testnet', accountId,
      phase: 'login', requestHash: await sha256Hex(secret) }),
  }));
  assert.equal(result.status, 200);
  assert.equal(b.state.calls.length, 1);
  const upstream = b.state.calls[0];
  for (const header of Object.keys(untrusted)) assert.equal(new Headers(upstream.headers).has(header), false);
  assert.deepEqual(Object.keys(upstream.body).sort(), ['p_challenge', 'p_secret_hash']);
  assert.doesNotMatch(JSON.stringify(upstream.body), /198\.51\.100\.8|203\.0\.113\.9/);
});

await test('initial proof provisions only after FullAccess verification; second proof binds the real session', async () => {
  const b = backend();
  const initial = await b.challenge();
  assert.equal(Buffer.from(initial.nonce, 'base64').length, 32);
  assert.equal(initial.userId, null); assert.equal(initial.sessionId, null);
  assert.match(initial.message, /^[\x00-\x7f]*$/);
  assert.equal(b.state.provisioned, 0);
  const issued = await b.verify(initial);
  assert.equal(issued.status, 200); assert.equal(b.state.provisioned, 1); assert.equal(b.state.verified, false);
  assert.deepEqual(Object.keys(issued.body), ['session']);
  assert.ok(!JSON.stringify(issued.body).includes('server-only'));
  const bound = await b.challenge('session', issued.body.session.access_token);
  assert.equal(bound.userId, userId); assert.equal(bound.sessionId, sessionId);
  assert.ok(bound.message.includes(userId) && bound.message.includes(sessionId) && bound.message.includes(config.supabaseUrl));
  const completed = await b.verify(bound, jwt());
  assert.equal(completed.status, 200); assert.equal(completed.body.verified, true);
  assert.equal(completed.body.profile.wallet_address, accountId);
  const status = await b.call({ action: 'status', accountId }, jwt());
  assert.equal(status.body.verified, true);
  await b.call({ action: 'logout', accountId }, jwt());
  assert.equal(b.state.verified, false);
});

await test('wrong request secret cannot consume the nonce; concurrent correct replays have one winner', async () => {
  const b = backend(); const challenge = await b.challenge();
  assert.equal((await b.verify(challenge, undefined, sign(challenge), 'cd'.repeat(32))).status, 400);
  assert.equal(b.state.challenges.get(challenge.id).consumed, false);
  const results = await Promise.all([b.verify(challenge), b.verify(challenge)]);
  assert.deepEqual(results.map((r) => r.status).sort(), [200, 400]);
  assert.equal(b.state.provisioned, 1);
});

await test('expired and forged signatures never provision an Auth user', async () => {
  const expired = backend(); const old = await expired.challenge(); expired.state.now += 240001;
  assert.equal((await expired.verify(old)).status, 400); assert.equal(expired.state.provisioned, 0);
  const b = backend(); const challenge = await b.challenge();
  const signed = sign({ ...challenge, message: 'attacker message' });
  assert.equal((await b.verify(challenge, undefined, signed)).status, 401);
  assert.equal(b.state.provisioned, 0);
  assert.equal((await b.verify(challenge)).status, 400);
});

await test('function-call keys, revoked keys, and RPC outages never provision', async () => {
  for (const permission of [{ FunctionCall: { receiver_id: 'contract.testnet' } }, null]) {
    const b = backend(); b.state.permission = permission;
    assert.equal((await b.verify(await b.challenge())).status, 403); assert.equal(b.state.provisioned, 0);
  }
  const b = backend(); b.state.failNear = true;
  assert.equal((await b.verify(await b.challenge())).status, 503); assert.equal(b.state.provisioned, 0);
});

await test('a session proof cannot authorize a different session, fake JWT, or revoked Auth session', async () => {
  const b = backend(); const bound = await b.challenge('session', jwt());
  assert.equal((await b.verify(bound, jwt(otherSession))).status, 403); assert.equal(b.state.verified, false);
  const fake = await b.call({ action: 'challenge', accountId, phase: 'session', requestHash: await sha256Hex(secret) }, jwt() + 'forged');
  assert.equal(fake.status, 401);
  const fresh = await b.challenge('session', jwt()); b.state.sessionActive = false;
  assert.equal((await b.verify(fresh, jwt())).status, 400); assert.equal(b.state.verified, false);
});

await test('origin/network mismatch is rejected before any upstream call; callback is server-selected', async () => {
  const b = backend();
  assert.equal((await b.call({ action: 'challenge' }, undefined, 'https://attacker.example')).status, 403);
  assert.equal((await b.call({ action: 'challenge', networkId: 'mainnet' })).status, 400);
  assert.equal(b.state.calls.length, 0);
  const challenge = await b.challenge('login', undefined, true);
  assert.equal(challenge.callbackUrl, `${origin}/login`);
  assert.equal(challenge.origin, origin); assert.equal(challenge.networkId, 'testnet');
});

await test('legacy identities are refused and logout invalidates outstanding session proofs', async () => {
  const legacy = backend(); legacy.state.legacy = true;
  const denied = await legacy.verify(await legacy.challenge());
  assert.equal(denied.status, 403); assert.equal(denied.body.code, 'LEGACY_IDENTITY');
  assert.equal(legacy.state.provisioned, 0);
  const b = backend(); const pending = await b.challenge('session', jwt());
  await b.call({ action: 'logout', accountId }, jwt());
  assert.equal((await b.verify(pending, jwt())).status, 400); assert.equal(b.state.verified, false);
});

async function loadAuth({ supabase, storage = new Map(), location = { origin, hash: '', pathname: '/login', search: '' } }) {
  const source = await readFile(new URL('../frontend/Nft/src/services/auth.js', import.meta.url), 'utf8');
  const sessionStorage = { getItem: (k) => storage.get(k) ?? null, setItem: (k, v) => storage.set(k, v), removeItem: (k) => storage.delete(k) };
  const context = vm.createContext({ crypto, TextEncoder, URLSearchParams, DOMException, AbortController, AbortSignal,
    sessionStorage, window: { location, history: { replaceState: () => { location.hash = ''; } } } });
  const module = new vm.SourceTextModule(source, { context });
  const modules = { buffer: { Buffer: BrowserBuffer }, '../lib/supabase': { supabase }, '../config/near': { NETWORK_ID: 'testnet' } };
  await module.link((name) => new vm.SyntheticModule(Object.keys(modules[name]), function () {
    for (const [key, value] of Object.entries(modules[name])) this.setExport(key, value);
  }, { context }));
  await module.evaluate();
  return { api: module.namespace, module, storage, location, sessionStorage };
}

async function browser(b, { stored, storage = new Map(), location = { origin, hash: '', pathname: '/login', search: '' } } = {}) {
  let session = stored || null;
  const calls = { installed: 0, cleared: 0, signed: [], actions: [] };
  const supabase = {
    auth: {
      getSession: async () => ({ data: { session }, error: null }),
      setSession: async (tokens) => { calls.installed++; session = { ...tokens, user: { id: userId } }; return { data: { session }, error: null }; },
      signOut: async () => { calls.cleared++; session = null; return { error: null }; },
    },
    functions: { invoke: async (name, options) => {
      assert.equal(name, 'near-wallet-auth'); calls.actions.push(options.body.action);
      const response = await b.handler(new Request(config.supabaseUrl + '/functions/v1/' + name, {
        method: 'POST', headers: { Origin: origin, 'Content-Type': 'application/json',
          Authorization: options.headers?.Authorization || `Bearer ${session?.access_token || 'public-key'}` },
        body: JSON.stringify(options.body),
      }));
      if (!response.ok) return { data: null, error: { context: response } };
      return { data: await response.json(), error: null };
    } },
  };
  const loaded = await loadAuth({ supabase, storage, location });
  const wallet = { id: 'meteor-wallet', signMessage: async (params) => {
    calls.signed.push(params);
    return sign({ ...params, accountId, nonce: params.nonce.toString('base64') });
  } };
  const selector = { options: { network: { networkId: 'testnet' } }, wallet: async () => wallet };
  return { ...loaded, calls, selector, wallet, getSession: () => session };
}

async function switchingFrontend({ initialAccount = 'alice.testnet', failLogout = false, failVerify = false } = {}) {
  const accounts = {
    'alice.testnet': { id: 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', sessionId: '11111111-1111-4111-8111-111111111111' },
    'bob.testnet': { id: 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb', sessionId: '22222222-2222-4222-8222-222222222222' },
    'carol.testnet': { id: 'cccccccc-cccc-4ccc-8ccc-cccccccccccc', sessionId: '33333333-3333-4333-8333-333333333333' },
  };
  const tokenFor = (accountId) => 'header.' + Buffer.from(JSON.stringify({
    sub: accounts[accountId].id, session_id: accounts[accountId].sessionId, role: 'authenticated',
  })).toString('base64') + '.signature';
  const claims = (token) => JSON.parse(Buffer.from(token.split('.')[1], 'base64').toString());
  const resultFor = (accountId) => ({ verified: true, user: { id: accounts[accountId].id },
    profile: { id: accounts[accountId].id, wallet_address: accountId },
    sessionId: accounts[accountId].sessionId, verifiedUntil: new Date(Date.now() + 3600000).toISOString() });
  const calls = { installed: 0, cleared: 0, signed: [] };
  const actions = [], revocations = [], logoutAttempts = [], challenges = new Map();
  const verified = new Set(initialAccount ? [initialAccount] : []);
  let sessionAccount = initialAccount;
  let session = initialAccount ? { access_token: tokenFor(initialAccount), refresh_token: 'test-refresh',
    user: { id: accounts[initialAccount].id } } : null;
  let signingAccount;
  const failure = (message, status) => ({ data: null, error: { context: new Response(JSON.stringify({ error: message }),
    { status, headers: { 'Content-Type': 'application/json' } }) } });
  const supabase = {
    auth: {
      getSession: async () => ({ data: { session }, error: null }),
      setSession: async (tokens) => {
        calls.installed++;
        sessionAccount = Object.keys(accounts).find((id) => accounts[id].sessionId === claims(tokens.access_token).session_id);
        session = { ...tokens, user: { id: claims(tokens.access_token).sub } };
        return { data: { session }, error: null };
      },
      signOut: async () => { calls.cleared++; session = null; sessionAccount = null; return { error: null }; },
    },
    functions: { invoke: async (name, options) => {
      assert.equal(name, 'near-wallet-auth');
      const body = options.body;
      actions.push({ action: body.action, accountId: body.accountId ?? null });
      if (body.action === 'status') {
        if (!session || sessionAccount !== body.accountId || !verified.has(body.accountId)) return failure('Wallet and Auth session do not match', 400);
        return { data: resultFor(body.accountId), error: null };
      }
      if (body.action === 'challenge') {
        const challenge = { id: crypto.randomUUID(), phase: body.phase, accountId: body.accountId,
          networkId: 'testnet', origin, recipient: new URL(origin).host, message: 'Sign in to NEAR NFT',
          nonce: Buffer.from(crypto.getRandomValues(new Uint8Array(32))).toString('base64'),
          issuedAt: new Date().toISOString(), expiresAt: new Date(Date.now() + 240000).toISOString() };
        challenges.set(challenge.id, challenge);
        signingAccount = body.accountId;
        return { data: { challenge }, error: null };
      }
      if (body.action === 'verify') {
        const challenge = challenges.get(body.challengeId);
        if (!challenge) return failure('Challenge expired, invalid or already used', 400);
        if (failVerify) return failure('Service unavailable', 503);
        if (challenge.phase === 'login') return { data: { session: {
          access_token: tokenFor(challenge.accountId), refresh_token: 'test-refresh' } }, error: null };
        verified.add(challenge.accountId);
        return { data: resultFor(challenge.accountId), error: null };
      }
      if (body.action === 'logout') {
        logoutAttempts.push(body.accountId);
        if (failLogout) return failure('Service unavailable', 503);
        verified.delete(body.accountId);
        revocations.push(body.accountId);
        return { data: { revoked: true }, error: null };
      }
      throw new Error(`Unexpected switching test action: ${body.action}`);
    } },
  };
  const loaded = await loadAuth({ supabase });
  const wallet = { id: 'meteor-wallet', signMessage: async (params) => {
    calls.signed.push(params);
    return { accountId: signingAccount, publicKey: 'ed25519:test', signature: 'test' };
  } };
  const selector = { options: { network: { networkId: 'testnet' } }, wallet: async () => wallet };
  return { ...loaded, calls, wallet, selector, actions, revocations, logoutAttempts, getSession: () => session,
    getSessionAccount: () => sessionAccount, getSigningAccount: () => signingAccount,
    signCurrentAccount: () => ({ accountId: signingAccount, publicKey: 'ed25519:test', signature: 'test' }) };
}

await test('browser completes two real NEP-413 proofs before reporting a verified login, then restores without signing', async () => {
  const b = backend(); const front = await browser(b);
  const result = await front.api.loginWithWallet(accountId, { selector: front.selector });
  assert.equal(result.verified, true); assert.equal(front.calls.signed.length, 2); assert.equal(front.calls.installed, 1);
  assert.equal(front.calls.signed[0].callbackUrl, undefined);
  const restored = await front.api.loginWithWallet(accountId, { selector: front.selector });
  assert.equal(restored.verified, true); assert.equal(front.calls.signed.length, 2);
  await front.api.logoutUser(accountId);
  assert.equal(front.getSession(), null); assert.equal(b.state.verified, false);
});

await test('MyNearWallet redirects resume both stages using tab-bound state without persisting proof tokens', async () => {
  const b = backend(); let front = await browser(b); const storage = front.storage;
  for (let stage = 0; stage < 2; stage++) {
    front.wallet.id = 'my-near-wallet';
    let signature;
    front.wallet.signMessage = async (params) => { signature = sign({ ...params, accountId, nonce: params.nonce.toString('base64') }); return undefined; };
    const result = await front.api.loginWithWallet(accountId, { selector: front.selector });
    assert.equal(result.redirecting, true);
    const pending = JSON.parse(storage.get('near-wallet-auth-pending-v1'));
    assert.equal(pending.challenge.phase, stage === 0 ? 'login' : 'session');
    assert.ok(!JSON.stringify(pending).includes('access_token'));
    const location = { origin, pathname: '/login', search: '', hash: '#' + new URLSearchParams({ ...signature, state: pending.challenge.id }).toString() };
    front = await browser(b, { storage, location, stored: front.getSession() });
    if (stage === 0) {
      // The resumed login issues a session and immediately requests the second signature.
      front.wallet.id = 'my-near-wallet';
      let secondSignature;
      front.wallet.signMessage = async (params) => { secondSignature = sign({ ...params, accountId, nonce: params.nonce.toString('base64') }); return undefined; };
      assert.equal((await front.api.loginWithWallet(accountId, { selector: front.selector })).redirecting, true);
      const second = JSON.parse(storage.get('near-wallet-auth-pending-v1'));
      assert.equal(second.challenge.phase, 'session');
      front = await browser(b, { storage, stored: front.getSession(), location: { origin, pathname: '/login', search: '',
        hash: '#' + new URLSearchParams({ ...secondSignature, state: second.challenge.id }).toString() } });
      assert.equal((await front.api.loginWithWallet(accountId, { selector: front.selector })).verified, true);
      assert.equal(storage.size, 0);
      break;
    }
  }
});

await test('unsupported wallets and mismatched callback state never authenticate', async () => {
  const b = backend(); const front = await browser(b);
  front.wallet.signMessage = undefined;
  await assert.rejects(front.api.loginWithWallet(accountId, { selector: front.selector }), /does not support/);
  assert.equal(front.calls.installed, 0);
  const bad = await browser(backend(), { location: { origin, pathname: '/login', search: '', hash: '#signature=bad&state=unrequested' } });
  await assert.rejects(bad.api.loginWithWallet(accountId, { selector: bad.selector }), /did not match/);
  assert.equal(bad.location.hash, ''); assert.equal(bad.calls.installed, 0);
});

await test('a rejected wallet proof leaves no profile/session; logout cancels an unresolved wallet prompt', async () => {
  const front = await browser(backend());
  front.wallet.signMessage = async () => { throw new Error('rejected'); };
  await assert.rejects(front.api.loginWithWallet(accountId, { selector: front.selector }), /rejected or is unsupported/);
  assert.equal(front.calls.installed, 0);
  let promptStarted;
  const started = new Promise((resolve) => { promptStarted = resolve; });
  front.wallet.signMessage = () => { promptStarted(); return new Promise(() => {}); };
  const login = front.api.loginWithWallet(accountId, { selector: front.selector });
  const cancelled = assert.rejects(login, { name: 'AbortError' });
  await started;
  await front.api.logoutUser(accountId);
  await cancelled;
  assert.equal(front.getSession(), null); assert.equal(front.calls.installed, 0);
});

await test('a missing backend fails closed before signing or installing an Auth session', async () => {
  const b = backend(); b.state.rpcUnavailable = true;
  const front = await browser(b);
  await assert.rejects(front.api.loginWithWallet(accountId, { selector: front.selector }), /not configured or unavailable/);
  assert.equal(front.calls.signed.length, 0); assert.equal(front.calls.installed, 0);
});

await test('a superseded wallet response cannot overwrite the newer login', async () => {
  const front = await browser(backend());
  let started, release, oldParams;
  const prompt = new Promise((resolve) => { started = resolve; });
  front.wallet.signMessage = (params) => {
    oldParams = params; started(); return new Promise((resolve) => { release = resolve; });
  };
  const first = front.api.loginWithWallet(accountId, { selector: front.selector });
  const aborted = assert.rejects(first, { name: 'AbortError' });
  await prompt;
  front.wallet.signMessage = async (params) => sign({ ...params, accountId, nonce: params.nonce.toString('base64') });
  const second = await front.api.loginWithWallet(accountId, { selector: front.selector });
  assert.equal(second.verified, true);
  release(sign({ ...oldParams, accountId, nonce: oldParams.nonce.toString('base64') }));
  await aborted;
  assert.equal(front.calls.installed, 1);
});

await test('switching from account A to B revokes A before establishing B', async () => {
  const front = await switchingFrontend({ initialAccount: 'alice.testnet' });
  assert.equal((await front.api.loginWithWallet('alice.testnet', { selector: front.selector })).verified, true);
  const switched = await front.api.loginWithWallet('bob.testnet', { selector: front.selector, previousAccount: 'alice.testnet' });
  assert.equal(switched.verified, true);
  assert.deepEqual(front.revocations, ['alice.testnet']);
  const logout = front.actions.findIndex((entry) => entry.action === 'logout');
  const bobChallenge = front.actions.findIndex((entry) => entry.action === 'challenge' && entry.accountId === 'bob.testnet');
  assert.ok(logout >= 0 && bobChallenge > logout, 'A must be signed out before B requests a challenge');
  assert.equal(front.getSessionAccount(), 'bob.testnet');
});

await test('a failed revocation during wallet switching leaves no authenticated session', async () => {
  const front = await switchingFrontend({ initialAccount: 'alice.testnet', failLogout: true });
  assert.equal((await front.api.loginWithWallet('alice.testnet', { selector: front.selector })).verified, true);
  await assert.rejects(front.api.loginWithWallet('bob.testnet', { selector: front.selector, previousAccount: 'alice.testnet' }),
    /revocation could not be confirmed/);
  assert.deepEqual(front.logoutAttempts, ['alice.testnet']);
  assert.equal(front.getSession(), null); assert.equal(front.getSessionAccount(), null);
  assert.equal(front.actions.some((entry) => entry.action === 'challenge' && entry.accountId === 'bob.testnet'), false);
});

await test('a new wallet proof failure cannot leave the previous account authenticated', async () => {
  const front = await switchingFrontend({ initialAccount: 'alice.testnet', failVerify: true });
  assert.equal((await front.api.loginWithWallet('alice.testnet', { selector: front.selector })).verified, true);
  await assert.rejects(front.api.loginWithWallet('bob.testnet', { selector: front.selector, previousAccount: 'alice.testnet' }),
    /Service unavailable/);
  assert.deepEqual(front.revocations, ['alice.testnet']);
  assert.equal(front.getSession(), null); assert.equal(front.getSessionAccount(), null);
});

await test('overlapping account switches cannot restore the superseded wallet session', async () => {
  const front = await switchingFrontend({ initialAccount: 'alice.testnet' });
  assert.equal((await front.api.loginWithWallet('alice.testnet', { selector: front.selector })).verified, true);
  let prompted;
  const bobPrompt = new Promise((resolve) => { prompted = resolve; });
  front.wallet.signMessage = (params) => {
    if (front.getSigningAccount() === 'bob.testnet') {
      prompted();
      return new Promise((resolve) => { front.releaseBob = () => resolve(front.signCurrentAccount()); });
    }
    front.calls.signed.push(params);
    return Promise.resolve(front.signCurrentAccount());
  };
  const bobLogin = front.api.loginWithWallet('bob.testnet', { selector: front.selector, previousAccount: 'alice.testnet' });
  const bobAborted = assert.rejects(bobLogin, { name: 'AbortError' });
  await bobPrompt;
  const carol = await front.api.loginWithWallet('carol.testnet', { selector: front.selector, previousAccount: 'bob.testnet' });
  assert.equal(carol.verified, true);
  front.releaseBob?.();
  await bobAborted;
  assert.ok(front.revocations.includes('alice.testnet'));
  assert.equal(front.getSessionAccount(), 'carol.testnet');
  assert.equal(front.calls.installed, 1);
});

await test('local dev origin allows both localhost:5173 and 127.0.0.1:5173', async () => {
  const localConfig = {
    supabaseUrl: 'https://project.example',
    appOrigin: 'http://localhost:5173',
    networkId: 'testnet',
    serviceRoleKey: 'server-only-test-credential',
  };
  const handler = createWalletAuthHandler(localConfig, {
    now: () => Date.now(),
    fetchImpl: async () => new Response('{}', { status: 200 }),
  });
  const opt1 = await handler(new Request('https://project.example/functions/v1/near-wallet-auth', {
    method: 'OPTIONS',
    headers: { Origin: 'http://localhost:5173' },
  }));
  assert.equal(opt1.status, 204);
  assert.equal(opt1.headers.get('Access-Control-Allow-Origin'), 'http://localhost:5173');

  const opt2 = await handler(new Request('https://project.example/functions/v1/near-wallet-auth', {
    method: 'OPTIONS',
    headers: { Origin: 'http://127.0.0.1:5173' },
  }));
  assert.equal(opt2.status, 204);
  assert.equal(opt2.headers.get('Access-Control-Allow-Origin'), 'http://127.0.0.1:5173');

  const optDenied = await handler(new Request('https://project.example/functions/v1/near-wallet-auth', {
    method: 'OPTIONS',
    headers: { Origin: 'http://evil.com' },
  }));
  assert.equal(optDenied.status, 403);
});
