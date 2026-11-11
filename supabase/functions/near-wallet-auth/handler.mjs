import { base64, sha256Hex, verifyNep413 } from './nep413.mjs';

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
class AuthError extends Error {
  constructor(message, status = 400, code = 'AUTH_REJECTED') {
    super(message); this.status = status; this.code = code;
  }
}
function requireValue(condition, message, status, code) {
  if (!condition) throw new AuthError(message, status, code);
}
function account(value) {
  requireValue(typeof value === 'string' && value.length >= 2 && value.length <= 64 &&
    /^[a-z0-9]+([._-][a-z0-9]+)*$/.test(value) && value.trim() === value, 'Invalid NEAR account');
  return value;
}
async function readBody(request) {
  const reader = request.body?.getReader();
  requireValue(reader, 'JSON body required');
  const chunks = []; let size = 0;
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > 16384) { await reader.cancel(); throw new AuthError('Request too large', 413); }
    chunks.push(value);
  }
  const bytes = new Uint8Array(size); let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
  try { return JSON.parse(new TextDecoder().decode(bytes)); }
  catch { throw new AuthError('Invalid JSON'); }
}

export function createWalletAuthHandler(config, { fetchImpl = fetch, now = Date.now } = {}) {
  const project = new URL(config.supabaseUrl);
  const origin = new URL(config.appOrigin);
  if (project.protocol !== 'https:' || project.username || project.password || project.search || project.hash ||
      origin.origin !== config.appOrigin ||
      (origin.protocol !== 'https:' && !['localhost', '127.0.0.1'].includes(origin.hostname)) ||
      !['testnet', 'mainnet'].includes(config.networkId) || !config.serviceRoleKey) {
    throw new Error('Invalid server-side wallet auth configuration');
  }
  const projectUrl = project.href.replace(/\/$/, '');
  const rpcUrl = config.networkId === 'mainnet' ? 'https://rpc.mainnet.near.org' : 'https://rpc.testnet.near.org';
  const isLocalOrigin = (url) => {
    try {
      const u = typeof url === 'string' ? new URL(url) : url;
      return (u.protocol === 'http:' || u.protocol === 'https:') &&
        ['localhost', '127.0.0.1'].includes(u.hostname);
    } catch { return false; }
  };
  const isLocalDev = isLocalOrigin(origin);
  const isOriginAllowed = (reqOrigin) => {
    if (!reqOrigin) return false;
    if (reqOrigin === origin.origin) return true;
    if (isLocalDev && isLocalOrigin(reqOrigin)) {
      const reqUrl = new URL(reqOrigin);
      const targetPort = origin.port || (origin.protocol === 'https:' ? '443' : '80');
      const reqPort = reqUrl.port || (reqUrl.protocol === 'https:' ? '443' : '80');
      return reqPort === targetPort;
    }
    return false;
  };
  const headers = {
    'Access-Control-Allow-Origin': origin.origin,
    'Access-Control-Allow-Headers': 'authorization, x-client-info, apikey, content-type',
    'Access-Control-Allow-Methods': 'POST, OPTIONS', 'Vary': 'Origin',
    'Cache-Control': 'no-store', 'Referrer-Policy': 'no-referrer',
  };

  async function api(path, body, { method = 'POST', token = config.serviceRoleKey } = {}) {
    let response;
    try {
      response = await fetchImpl(`${projectUrl}${path}`, {
        method, headers: { apikey: config.serviceRoleKey, Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
        signal: AbortSignal.timeout(10000), redirect: 'error',
      });
    } catch { throw new AuthError('Authentication service unavailable; retry later', 503, 'SERVICE_UNAVAILABLE'); }
    const data = await response.json().catch(() => null);
    if (!response.ok) {
      // Never relay arbitrary upstream errors, tokens, link responses or secrets.
      if (data?.message === 'Legacy wallet identity requires administrator review') {
        throw new AuthError('Existing wallet account requires a server-side security migration', 403, 'LEGACY_IDENTITY');
      }
      if (data?.message === 'Too many wallet challenges; retry later') throw new AuthError('Too many attempts; retry later', 429);
      if (path === '/auth/v1/user') throw new AuthError('Supabase session is invalid', 401, 'SESSION_INVALID');
      if (data?.code === 'PGRST202' || response.status >= 500) {
        throw new AuthError('Secure wallet authentication is not configured or unavailable', 503, 'SERVICE_UNAVAILABLE');
      }
      throw new AuthError('Authentication request expired or was rejected; start again');
    }
    return data;
  }
  const rpc = (name, body) => api(`/rest/v1/rpc/${name}`, body);

  async function sessionContext(request, accountId) {
    const token = request.headers.get('authorization')?.match(/^Bearer (.+)$/i)?.[1];
    requireValue(token && token.length < 16384, 'Supabase session required', 401, 'SESSION_INVALID');
    const user = await api('/auth/v1/user', undefined, { method: 'GET', token });
    // Decode only AFTER this project's Auth server has authenticated the JWT.
    let claims;
    try {
      const encoded = token.split('.')[1].replace(/-/g, '+').replace(/_/g, '/');
      claims = JSON.parse(atob(encoded));
    } catch { throw new AuthError('Invalid session claims', 401, 'SESSION_INVALID'); }
    requireValue(UUID.test(claims.session_id) && user?.id === claims.sub && claims.role === 'authenticated',
      'Invalid session identity', 401, 'SESSION_INVALID');
    const context = await rpc('wallet_auth_context', {
      p_account_id: accountId, p_network_id: config.networkId, p_user_id: user.id, p_session_id: claims.session_id,
    });
    return { ...context, user, userId: user.id, sessionId: claims.session_id };
  }

  async function checkWallet(challenge, signed) {
    requireValue(await verifyNep413(challenge, signed), 'Wallet signature is invalid', 401);
    let response;
    try {
      response = await fetchImpl(rpcUrl, {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, redirect: 'error',
        body: JSON.stringify({ jsonrpc: '2.0', id: challenge.id, method: 'query', params: {
          request_type: 'view_access_key', finality: 'final', account_id: challenge.accountId, public_key: signed.publicKey,
        } }), signal: AbortSignal.timeout(10000),
      });
    } catch { throw new AuthError('NEAR key verification unavailable; start again', 503); }
    const result = await response.json().catch(() => null);
    requireValue(response.ok && !result?.error && result?.result?.permission === 'FullAccess',
      'Signing key is not a current FullAccess key for this account', 403);
    requireValue(Date.parse(challenge.expiresAt) > now(), 'Challenge expired; start again');
  }

  async function provisionSession(accountId) {
    let identity = await rpc('wallet_auth_identity', { p_account_id: accountId, p_network_id: config.networkId });
    if (!identity) {
      // Random, non-deliverable internal email. No wallet-derived credentials or
      // password; only Admin API can create this identity after a valid proof.
      const created = await api('/auth/v1/admin/users', {
        email: `${crypto.randomUUID()}@wallet.invalid`, email_confirm: true,
        app_metadata: { near_wallet: { version: 1, account_id: accountId, network_id: config.networkId } },
      });
      const createdId = (created?.user ?? created)?.id;
      requireValue(UUID.test(createdId), 'Auth provisioning failed', 503);
      try {
        identity = await rpc('wallet_auth_provision', { p_account_id: accountId, p_network_id: config.networkId, p_user_id: createdId });
      } catch (error) {
        // If provisioning committed despite a lost response, the existing profile
        // FK prevents deleting its Auth user. Only our newly created ID is tried.
        await api(`/auth/v1/admin/users/${createdId}`, undefined, { method: 'DELETE' }).catch(() => {});
        throw error;
      }
      if (identity.id !== createdId) {
        await api(`/auth/v1/admin/users/${createdId}`, undefined, { method: 'DELETE' }).catch(() => {});
      }
    }
    const link = await api('/auth/v1/admin/generate_link', { type: 'magiclink', email: identity.email });
    requireValue(link?.id === identity.id && typeof link.hashed_token === 'string', 'Auth identity mismatch', 503);
    // Consume the OTP on the server. Do not return the admin link or hashed token.
    const session = await api('/auth/v1/verify', { type: 'magiclink', token_hash: link.hashed_token });
    requireValue(session?.user?.id === identity.id && session.access_token && session.refresh_token,
      'Auth session issuance failed', 503);
    return { access_token: session.access_token, refresh_token: session.refresh_token };
  }

  return async function handle(request) {
    const reqOrigin = request.headers.get('origin');
    const allowOrigin = isOriginAllowed(reqOrigin) ? reqOrigin : origin.origin;
    const responseHeaders = {
      ...headers,
      'Access-Control-Allow-Origin': allowOrigin,
    };
    const reply = (body, status = 200) => new Response(body === null ? null : JSON.stringify(body), {
      status, headers: { ...responseHeaders, 'Content-Type': 'application/json' },
    });
    try {
      requireValue(isOriginAllowed(reqOrigin), 'Origin not allowed', 403);
      if (request.method === 'OPTIONS') return reply(null, 204);
      requireValue(request.method === 'POST', 'Method not allowed', 405);
      requireValue(request.headers.get('content-type')?.split(';')[0] === 'application/json', 'JSON required', 415);
      const body = await readBody(request);
      requireValue(body && body.networkId === config.networkId, 'Network does not match this marketplace');

      if (body.action === 'challenge') {
        const accountId = account(body.accountId);
        requireValue(['login', 'session'].includes(body.phase) && /^[0-9a-f]{64}$/.test(body.requestHash), 'Invalid challenge request');
        const context = body.phase === 'session' ? await sessionContext(request, accountId) : null;
        const reqHost = new URL(allowOrigin).host;
        const challenge = {
          id: crypto.randomUUID(), phase: body.phase, accountId, networkId: config.networkId,
          origin: allowOrigin, project: projectUrl, recipient: reqHost,
          nonce: base64(crypto.getRandomValues(new Uint8Array(32))),
          issuedAt: new Date(now()).toISOString(), expiresAt: new Date(now() + 240000).toISOString(),
          userId: context?.userId ?? null, sessionId: context?.sessionId ?? null,
          ...(body.redirect === true ? { callbackUrl: `${allowOrigin}/login` } : {}),
        };
        challenge.message = 'Authenticate to NEAR NFT marketplace. No transaction or payment.\n' + JSON.stringify({
          purpose: challenge.phase, account: accountId, network: config.networkId, origin: challenge.origin,
          project: projectUrl, challenge: challenge.id, issuedAt: challenge.issuedAt, expiresAt: challenge.expiresAt,
          requestBinding: body.requestHash, authUser: challenge.userId, authSession: challenge.sessionId,
        }, null, 2);
        await rpc('wallet_auth_issue_challenge', { p_challenge: challenge, p_secret_hash: body.requestHash });
        return reply({ challenge });
      }

      if (body.action === 'verify') {
        requireValue(UUID.test(body.challengeId) && typeof body.requestSecret === 'string' &&
          /^[0-9a-f]{64}$/.test(body.requestSecret), 'Invalid proof request');
        const challenge = await rpc('wallet_auth_take_challenge', {
          p_id: body.challengeId, p_secret_hash: await sha256Hex(body.requestSecret),
        });
        requireValue(challenge.origin === origin.origin && challenge.project === projectUrl &&
          challenge.networkId === config.networkId && Date.parse(challenge.expiresAt) > now(), 'Challenge context mismatch');
        if (challenge.phase === 'session') {
          const context = await sessionContext(request, challenge.accountId);
          requireValue(context.userId === challenge.userId && context.sessionId === challenge.sessionId, 'Session binding mismatch', 403);
        }
        await checkWallet(challenge, body.signed);
        if (challenge.phase === 'login') return reply({ session: await provisionSession(challenge.accountId) });
        requireValue(challenge.phase === 'session', 'Invalid challenge phase');
        return reply(await rpc('wallet_auth_attach_session', { p_challenge_id: challenge.id }));
      }

      if (body.action === 'status' || body.action === 'logout') {
        const context = await sessionContext(request, account(body.accountId));
        if (body.action === 'logout') {
          await rpc('wallet_auth_revoke_session', { p_user_id: context.userId, p_session_id: context.sessionId });
          return reply({ revoked: true });
        }
        return reply({ user: { id: context.userId }, sessionId: context.sessionId, profile: context.profile,
          verified: context.verified, verifiedUntil: context.verifiedUntil });
      }
      throw new AuthError('Unknown auth action');
    } catch (error) {
      return reply({ error: error instanceof AuthError ? error.message : 'Authentication service unavailable',
        code: error instanceof AuthError ? error.code : 'SERVICE_UNAVAILABLE' }, error instanceof AuthError ? error.status : 503);
    }
  };
}
