# NEAR wallet authentication

Implementation prepared locally; no SQL was applied, project contacted, or function deployed.

The original `auth.js` derived a public password from a wallet name, wrote profiles
from the browser, and treated Wallet Selector connection as authentication. There
was no backend, signature verifier, nonce store, or session-verification writer.
The existing database deliberately rejects those client writes.

The smallest backend supplied here is one Supabase Edge Function, with no runtime
package dependencies, and a new migration containing private challenge storage
and service-only RPCs. The marketplace migration also binds user NFT image URLs
to this deployment's exact Supabase Storage public bucket prefix.

## Flow and trust boundaries

1. The browser generates a 32-byte request secret and sends only its SHA-256 hash
   when requesting a challenge. The server selects a fresh 32-byte NEP-413 nonce,
   a four-minute expiry, and the intended account, network, application origin,
   Supabase project and purpose. These values are in the signed message. The
   recipient is the configured application host; redirect callbacks are fixed to
   that application's existing `/login` route.
2. The wallet signs NEP-413. The server atomically consumes the challenge using
   its ID and request-secret hash, verifies Ed25519 over the tagged Borsh payload's
   SHA-256 digest, then queries the configured network at `final` finality. The
   exact signing key must currently have `FullAccess` on the claimed account.
   A valid signature alone, or a restricted FunctionCall key, is insufficient.
3. Only after that proof does the server provision an Auth user through the Admin
   API, with a random internal `@wallet.invalid` email, no password, and trusted
   `app_metadata.near_wallet`. A locked database RPC provisions the profile and
   resolves concurrent first-login races. The browser never creates profiles or
   supplies an Auth user ID. Legacy identities are rejected for administrator
   review; a matching email or user-editable metadata is never sufficient.
4. The server generates and immediately consumes an internal magic-link OTP to
   obtain a **real Supabase access/refresh session**. The admin link and OTP stay
   server-side. This initial session remains unverified for marketplace RLS.
5. A second NEP-413 challenge includes that actual Auth user UUID and session UUID.
   The server first authenticates the incoming JWT with this project's Auth
   `/user` endpoint, verifies that the session exists and belongs to the mapped
   profile, and checks the second signature and live NEAR key. Only then does it
   insert `marketplace_private.verified_wallet_sessions`. Verification lasts at
   most one hour, also bounded by `auth.sessions.not_after`.
6. Restoring a browser session requires a server status check. JWT refresh can
   continue within the same verified session; a fresh password/OTP session cannot
   inherit the grant. Expired verification needs a new session-bound signature.
   Logout revokes the grant, invalidates outstanding session challenges under the
   same session lock used for attachment, and signs out the local Supabase session.

Two initial signatures are intentional: a new Auth session cannot be named in a
signature before it exists, and the requirements prohibit creating an Auth user
before successful wallet proof. The second signature preserves the original
migration's explicit binding to both the user and the session.

Challenges are single-use even after invalid signatures or temporary RPC errors;
retry with a new challenge. Requests have bounded bodies and upstream timeouts.
Only server-selected project/RPC URLs receive requests, and service credentials
are sent exclusively to the configured Supabase origin. Responses disable caching
and never include arbitrary upstream errors or admin credentials.

## Wallet and frontend behavior

- Existing MyNearWallet, Meteor, HERE, and Sender providers remain registered.
  NEP-413 signatures are required; unsupported or rejected signing fails closed.
- MyNearWallet redirects preserve the challenge/request secret in tab-local
  session storage, bind returned `state`, and immediately remove signature
  material from the URL. They support both signing stages without new routes.
  Other providers use their direct `signMessage` response. Callback parameters
  are not supplied to Meteor, whose installed adapter omits them when signing.
- No application session tokens are stored in the pending challenge record;
  existing Supabase session persistence handles actual sessions.
- `isConnected` becomes true only after server verification, so the existing
  login page cannot navigate on mere wallet selection. Preview remains available.
- Account changes, logout and effect cleanup cancel pending authentication;
  late wallet responses cannot install a newer account's session. Auth events
  are compared against the verified session UUID, not just its user UUID.
- Layouts/pages/routes are unchanged. The provider also removes the stale
  subscription/account closure and uses the selector ref for balance refresh.
- The installed `borsh@1.0.0` SDK serializer mishandles non-ASCII strings. Generated
  challenges are ASCII; tests compare those against Wallet Selector and separately
  check a fixed UTF-8 Borsh fixture. The verifier implements NEP-413 UTF-8 correctly.

## Still required before live use

These are operator/deployment prerequisites, not completed actions:

1. Review and validate both migrations on a disposable Supabase project, in order:
   `20261029000100_near_marketplace.sql`, then `20261029000200_near_wallet_auth.sql`.
   The new migration assumes current Supabase Auth tables, including
   `auth.sessions.not_after`, and the service role. Do not expose
   `marketplace_private` in API schemas. SQL Editor and CLI migration histories
   must be kept consistent. Do not reset or replace existing marketplace data.
2. Configure **server-side** Edge Function secrets:

   | Setting | Required value |
   | --- | --- |
   | `SUPABASE_URL` | Intended new project's URL (normally supplied by Supabase) |
   | `SUPABASE_SERVICE_ROLE_KEY` | That project's server-only service credential |
   | `NEAR_AUTH_APP_ORIGIN` | Exact canonical application origin, no trailing slash |
   | `NEAR_AUTH_NETWORK_ID` | `testnet` or `mainnet`, matching the frontend |

   HTTPS is required except for a localhost application origin. Only one network
   and application origin are configured per deployment. No new frontend secret
   or `.env` change is needed by this implementation.
   The marketplace migration's `v_storage_public_prefix` is currently
   `https://llqwiqhwrbfoxoqqgczy.supabase.co/storage/v1/object/public/nft-images/`,
   matching the configured frontend project. When preparing another project,
   update that literal together with `VITE_SUPABASE_URL` and `SUPABASE_URL` before
   applying the migration. Keep the exact origin, bucket and trailing slash;
   arbitrary HTTPS hosts, URL query/fragment suffixes and encoded/traversal paths
   are rejected. Supplying `image_path` does not bypass the URL, canonical path,
   creator or Storage object ownership checks.
3. Deploy the `near-wallet-auth` Edge Function with gateway JWT verification
   disabled **for this function only** (`--no-verify-jwt`), since its bootstrap
   challenge has no Auth session yet. The function itself authenticates every
   session action and enforces wallet proof for issuance/verification. Keeping
   the gateway's default JWT requirement can block bootstrap with publishable keys.
4. Disable public and anonymous Auth signups. Keep the email provider enabled for
   the server's internal Admin-generated magic-link exchange. Admin creation must
   be permitted; no SMTP delivery is used. Other enabled identity providers must
   not be treated as wallet proof. These settings cannot be enforced by frontend
   code or the marketplace RLS migration.
5. If any predictable-password accounts already exist, audit and migrate each
   through trusted wallet proof while retaining its existing Auth UUID/assets.
   Rotate/remove the old credentials and insecure email mapping, revoke existing
   sessions/grants, and assign trusted metadata only after proof. Do not simply
   stamp every legacy profile as verified. This implementation intentionally
   refuses to perform that potentially destructive migration automatically.
6. **Unresolved: configure a trusted gateway rate limit before production.** This
   repository has no supported ingress, WAF, reverse-proxy, or Supabase gateway
   configuration. The browser's `supabase.functions.invoke` calls the hosted
   function directly; `functions/near-wallet-auth/index.ts` has no documented
   trusted client-IP contract. No application-side per-IP limiter was added.

   At the deployment's trusted ingress, match every POST routed to
   `/functions/v1/near-wallet-auth`, including subpaths and query strings, whose
   parsed JSON body has `action: "challenge"`. Match JSON using the function's
   parsing semantics, including escaped values and duplicate keys. If that is
   unsupported or ambiguous, limit all POST requests routed to this function.
   Allow **10 requests per rolling 60 seconds per directly observed client source IP**,
   with a burst of 10 within that same quota (no additional burst allowance).
   Count each forwarded request, including ones that later fail verification;
   keep counters shared across all gateway replicas and both challenge phases.
   Changing account IDs, headers or sessions must not reset the IP quota.
   Return HTTP 429 without invoking the function or challenge RPC when exceeded,
   with `Retry-After` set to the seconds until another request can be admitted,
   `Cache-Control: no-store`, the configured application's CORS origin and the
   sanitized body `{"error":"Too many attempts; retry later","code":"AUTH_REJECTED"}`.

   Key the quota from canonical IPv4/IPv6 ingress connection metadata (or
   vendor-authenticated client-IP metadata after stripping inbound copies and
   restricting origin access to that trusted proxy), never from a client-supplied `X-Forwarded-For`,
   `Forwarded`, `X-Real-IP`, `CF-Connecting-IP`, or similar header. Apply the rule
   on every public ingress, including custom domains and the
   direct `https://<project-ref>.supabase.co/functions/v1/near-wallet-auth` URL;
   reject traffic that bypasses the limiter. A WAF on only the frontend/custom
   domain leaves the hosted function reachable. If the hosting provider cannot
   enforce this rule on the direct URL or restrict it to the trusted gateway,
   this requirement is unmet. Keep challenge RPC execution restricted to
   `service_role`; CORS, the public API key and account quotas do not enforce an
   IP limit.

   Verify that the 11th request from one IP is rejected before any function/RPC
   invocation, another IP retains its allowance, and changing forwarding headers,
   account IDs, challenge phase, subpaths or ingress hostname cannot bypass the
   rule. Check admission after the rolling window expires and monitor rejections.
   Until that configuration is deployed and verified, challenge denial-of-service
   remains unresolved. The database's non-blocking issuance gate caps committed
   challenges at 60/minute and rejects lock contention immediately; authenticated
   session challenges also remain limited to six per account/network/minute.
   The transaction-scoped lock is released on commit/rollback. The existing
   `P0001` exception aborts rejected issuance (including rolling back expired-row
   cleanup after a quota failure); the handler maps its sanitized message to 429.
7. Run real browser/provider tests, Auth session-issuance tests, and database
   concurrency/RLS tests before enabling live writes. Verify that anonymous and
   ordinary Auth sessions cannot execute the new RPCs or write verification rows.
   Verify duplicate nonce consumption under simultaneous requests, account
   provisioning races, expired/revoked sessions, and cross-session proof rejection.
   For the issuance gate, hold the advisory lock in one transaction and verify
   that a second issuance fails immediately without writes; after commit or
   rollback, a request within quota must succeed. Check that quota/insert errors
   roll back cleanup and release the lock. These database checks remain unrun.

Function/DB/NEAR availability errors leave authentication incomplete. Offline
logout may be unable to confirm server revocation; the UI clears identity and
reports that condition. Already issued verification grants expire within one hour.
For immediate key compromise/revocation, a trusted process must also revoke the
associated Auth sessions/grants; live key checks occur on proof verification,
not on every marketplace query. MFA/SSO configuration that changes the internal
Auth exchange requires separate integration validation; it must not be bypassed.

## Local checks

Run from the repository root, with the existing frontend dependencies installed:

```sh
node tests/security-migrations.test.mjs
node --experimental-vm-modules tests/near-wallet-auth.test.mjs
```

The migration suite checks SQL source invariants and the stored ASCII path
regexp without executing migrations. It covers gate ordering, transaction
boundaries, service-only RPC grants, the exact Storage prefix and malformed paths.
It does not execute the image trigger or exercise PostgreSQL locking/rollback.

The protocol tests use actual Ed25519 signatures and the installed Wallet Selector
serializer, with mocked Supabase/NEAR HTTP. They exercise proof tampering, replay,
expiry, access-key permission, session binding, redirects, cancellation and the
full browser-to-handler protocol, including sanitized challenge quota failures
for both phases and ignoring untrusted client-IP headers. Database RPC behavior
is mocked: this is not a
claim that migrations, real Auth issuance, RLS concurrency, Deno hosting, or wallet
extensions were exercised. No environment file or live project is read by tests.
