import { Buffer } from "buffer";
import { supabase } from "../lib/supabase";
import { NETWORK_ID } from "../config/near";

const PENDING_KEY = "near-wallet-auth-pending-v1";
let generation = 0;
let loginQueue = Promise.resolve();
let activeController;
let sessionAccount = null;

function abortError() {
  return new DOMException("Wallet authentication was cancelled", "AbortError");
}

async function request(action, body, token, signal) {
  const { data, error } = await supabase.functions.invoke("near-wallet-auth", {
    body: { action, networkId: NETWORK_ID, ...body },
    signal: AbortSignal.any([AbortSignal.timeout(15000), ...(signal ? [signal] : [])]),
    ...(token ? { headers: { Authorization: `Bearer ${token}` } } : {}),
  });
  if (signal?.aborted) throw abortError();
  if (error || data?.error) {
    const detail = error?.context?.clone ? await error.context.clone().json().catch(() => null) : data;
    const failure = new Error(detail?.error || "Secure wallet authentication service is unavailable. Please try again later.");
    failure.code = detail?.code || "SERVICE_UNAVAILABLE";
    failure.status = error?.context?.status || 503;
    throw failure;
  }
  return data;
}

function signWithWallet(wallet, params, signal) {
  return new Promise((resolve, reject) => {
    if (signal.aborted) return reject(abortError());
    const onAbort = () => reject(abortError());
    signal.addEventListener("abort", onAbort, { once: true });
    Promise.resolve().then(() => wallet.signMessage(params)).then(resolve, reject)
      .finally(() => signal.removeEventListener("abort", onAbort));
  });
}

async function requestHash(secret) {
  const bytes = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(secret)));
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function takeRedirectProof(accountId) {
  const fragment = new URLSearchParams(window.location.hash.slice(1));
  if (!fragment.has("signature") && !fragment.has("errorCode")) return null;
  // Remove proof material before subsequent navigation or application logging.
  window.history.replaceState(null, "", window.location.pathname + window.location.search);
  let pending;
  try { pending = JSON.parse(sessionStorage.getItem(PENDING_KEY)); } catch { /* Invalid local state fails closed. */ }
  sessionStorage.removeItem(PENDING_KEY);
  if (!pending || fragment.get("state") !== pending.challenge.id || pending.challenge.accountId !== accountId ||
      pending.challenge.networkId !== NETWORK_ID || Date.parse(pending.challenge.expiresAt) <= Date.now()) {
    throw new Error("Wallet signing response expired or did not match this browser's request. Reconnect to retry.");
  }
  if (fragment.has("errorCode")) throw new Error("Wallet message signing was cancelled or rejected.");
  return { ...pending, signed: { accountId: fragment.get("accountId"),
    publicKey: fragment.get("publicKey"), signature: fragment.get("signature") } };
}

function verifiedResult(result, accountId) {
  if (!result?.verified || result.profile?.wallet_address !== accountId || result.user?.id !== result.profile?.id ||
      typeof result.sessionId !== "string" ||
      !Number.isFinite(Date.parse(result.verifiedUntil)) || Date.parse(result.verifiedUntil) <= Date.now()) {
    throw new Error("Wallet session has not been verified by the server.");
  }
  return result;
}

async function clearSession(expectedAccount) {
  const { data } = await supabase.auth.getSession();
  // Prefer the account actually attached to the current session when switching
  // wallets; a stale caller-provided account must not revoke the wrong session.
  const accountId = sessionAccount || expectedAccount;
  let revokeError;
  try {
    if (data?.session && accountId) await request("logout", { accountId }, data.session.access_token);
  } catch (error) { revokeError = error; }
  const { error } = await supabase.auth.signOut({ scope: "local" });
  sessionAccount = null;
  if (revokeError || error) throw new Error("Disconnected, but server session revocation could not be confirmed. Please retry when online.");
}

async function authenticate(accountId, { selector, signal }, attempt) {
  const assertCurrent = () => { if (signal?.aborted || attempt !== generation) throw abortError(); };
  const finish = async (result) => {
    verifiedResult(result, accountId);
    const current = await supabase.auth.getSession();
    assertCurrent();
    let sessionId;
    try {
      const payload = current.data?.session?.access_token?.split(".")[1].replace(/-/g, "+").replace(/_/g, "/");
      sessionId = JSON.parse(Buffer.from(payload, "base64").toString()).session_id;
    } catch { /* Comparing to an already server-verified identity, not trusting decoded claims. */ }
    if (current.error || current.data?.session?.user?.id !== result.user.id || sessionId !== result.sessionId) {
      throw new Error("Supabase session changed during wallet verification. Reconnect to retry.");
    }
    verifiedResult(result, accountId);
    sessionAccount = accountId;
    return result;
  };
  assertCurrent();
  if (typeof accountId !== "string" || accountId.length < 2 || accountId.length > 64 ||
      !/^[a-z0-9]+([._-][a-z0-9]+)*$/.test(accountId) || accountId.trim() !== accountId) {
    throw new Error("A canonical NEAR account ID is required.");
  }
  if (!selector || selector.options.network.networkId !== NETWORK_ID) {
    throw new Error("Wallet network does not match this marketplace.");
  }
  const wallet = await selector.wallet();
  assertCurrent();
  if (typeof wallet.signMessage !== "function") {
    throw new Error("This wallet does not support NEP-413 message signing. Choose a compatible NEAR wallet.");
  }
  const { data, error } = await supabase.auth.getSession();
  if (error) throw new Error("Unable to read the Supabase session.");
  let session = data?.session;
  let pending = takeRedirectProof(accountId);
  assertCurrent();

  if (!pending && session) {
    try {
      const status = await request("status", { accountId }, session.access_token, signal);
      assertCurrent();
      if (status.verified) return await finish(status);
    } catch (failure) {
      assertCurrent();
      if (failure.code === "LEGACY_IDENTITY" || failure.status >= 500) throw failure;
      await supabase.auth.signOut({ scope: "local" });
      sessionAccount = null;
      session = null;
    }
  }

  for (let phase = pending?.challenge.phase || (session ? "session" : "login");;) {
    assertCurrent();
    if (!pending) {
      const secret = Array.from(crypto.getRandomValues(new Uint8Array(32)), (byte) => byte.toString(16).padStart(2, "0")).join("");
      const { challenge } = await request("challenge", {
        accountId, phase, requestHash: await requestHash(secret), redirect: wallet.id === "my-near-wallet",
      }, session?.access_token, signal);
      assertCurrent();
      if (challenge?.accountId !== accountId || challenge.networkId !== NETWORK_ID ||
          challenge.origin !== window.location.origin || challenge.phase !== phase ||
          !Number.isFinite(Date.parse(challenge.expiresAt)) || Date.parse(challenge.expiresAt) <= Date.now() ||
          (challenge.callbackUrl && challenge.callbackUrl !== `${window.location.origin}/login`)) {
        throw new Error("Server returned an invalid wallet challenge.");
      }
      const nonce = Buffer.from(challenge.nonce, "base64");
      if (nonce.length !== 32 || nonce.toString("base64") !== challenge.nonce) throw new Error("Invalid wallet nonce.");
      pending = { challenge, secret };
      sessionStorage.setItem(PENDING_KEY, JSON.stringify(pending));
      let signed;
      try {
        signed = await signWithWallet(wallet, { message: challenge.message, recipient: challenge.recipient, nonce,
          ...(challenge.callbackUrl ? { callbackUrl: challenge.callbackUrl } : {}), state: challenge.id }, signal);
      } catch {
        sessionStorage.removeItem(PENDING_KEY);
        assertCurrent();
        throw new Error("Wallet message signing was rejected or is unsupported. Reconnect with a NEP-413 compatible wallet.");
      }
      assertCurrent();
      if (!signed) {
        if (challenge.callbackUrl) return { redirecting: true };
        sessionStorage.removeItem(PENDING_KEY);
        throw new Error("Wallet did not return a NEP-413 signature.");
      }
      pending.signed = signed;
      sessionStorage.removeItem(PENDING_KEY);
    }
    if (pending.signed?.accountId !== accountId || (phase === "session" && !session)) {
      throw new Error("Wallet signature or Supabase session does not match this login.");
    }
    const result = await request("verify", { challengeId: pending.challenge.id,
      requestSecret: pending.secret, signed: pending.signed }, session?.access_token, signal);
    assertCurrent();
    if (phase === "session") return finish(result);
    if (!result?.session?.access_token || !result.session.refresh_token) throw new Error("Server did not issue a Supabase session.");
    const installed = await supabase.auth.setSession(result.session);
    if (signal?.aborted || attempt !== generation) {
      await supabase.auth.signOut({ scope: "local" });
      sessionAccount = null;
      throw abortError();
    }
    if (installed.error || !installed.data?.session) throw new Error("Unable to install the verified wallet's Supabase session.");
    sessionAccount = accountId;
    session = installed.data.session;
    // The first proof authorizes issuance; the second signs the ACTUAL Auth user
    // and session IDs. Until it completes the existing RLS denies protected writes.
    phase = "session";
    pending = null;
  }
}

export function loginWithWallet(accountId, options = {}) {
  const { previousAccount, signal: outerSignal, ...rest } = options;
  activeController?.abort();
  activeController = new AbortController();
  const signal = AbortSignal.any([activeController.signal, ...(outerSignal ? [outerSignal] : [])]);
  const attempt = ++generation;
  // Prevent late wallet responses/account switches from overwriting a newer login.
  const operation = loginQueue.catch(() => {}).then(async () => {
    if (attempt !== generation) throw abortError();
    const accountToRevoke = sessionAccount && sessionAccount !== accountId ? sessionAccount
      : !sessionAccount && previousAccount && previousAccount !== accountId ? previousAccount : null;
    // A→B account switches must invalidate A before B can receive a verified
    // session. If revocation cannot be confirmed, fail closed.
    if (accountToRevoke) {
      await clearSession(accountToRevoke);
      if (attempt !== generation) throw abortError();
    }
    return authenticate(accountId, { ...rest, signal }, attempt);
  });
  loginQueue = operation;
  return operation;
}

export function logoutUser(accountId) {
  activeController?.abort();
  generation++;
  sessionStorage.removeItem(PENDING_KEY);
  const operation = loginQueue.catch(() => {}).then(() => clearSession(accountId));
  loginQueue = operation;
  return operation;
}
