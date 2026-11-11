# RoundFrog blocker resolution — 2026-11-06 (local dev, safe/authorized)

**Verdict:** PARTIALLY READY (local read-only/Testnet preparation). **Not production-ready; no signed lifecycle validation.**

## Local application
- Vite config: `frontend/Nft/vite.config.js` → `server.host=true`, `port=5173`. **Exact local URL:** `http://127.0.0.1:5173/` (also `http://localhost:5173/`).
- Dev server started and responding HTTP 200 for `/`, `/marketplace`, `/dashboard`, `/login` (SPA routes). Background process `bgp_1b1a025b7001Fy0qlTvp5Ewo7w` running. **I cannot see browser console/network interactively**; run in browser for console/runtime checks.

## near-wallet-auth (CRITICAL fix applied for local dev)
- Root cause of prior 500: `NEAR_AUTH_APP_ORIGIN` missing/invalid → handler startup failure (`WORKER_ERROR`). Handler explicitly allows `http://localhost` / `http://127.0.0.1` origins (HTTPS required otherwise).
- **Action (authorized, no values printed):** set secret name `NEAR_AUTH_APP_ORIGIN=http://localhost:5173` via `supabase secrets set` (project `llqwiqhwrbfoxoqqgczy`), redeployed existing `near-wallet-auth` with `--no-verify-jwt --yes`. Security model unchanged (origin check, NEP-413, FullAccess key RPC, replay/expiry, session-bound auth, no service-role/admin token leakage).
- **Deployed:** `near-wallet-auth` **ACTIVE v4** (2026-11-06 16:09:06Z).
- **Safe challenge tests (publishable key, no wallet signature):**
  - `Origin: http://localhost:5173`, login `challenge` (testnet, valid account/hash) → **HTTP 200** with challenge (`phase=login`, correct origin/account, nonce/id/expiresAt present).
  - `Origin: https://evil.example` → **HTTP 403** `{"error":"Origin not allowed"}`.
  - Wrong network (`mainnet`) → **HTTP 400** `Network does not match this marketplace`.
  - Unknown/malformed action → **HTTP 400** rejected.
  - `GET` method → **HTTP 405** `Method not allowed`.
  - `status` without bearer session → **HTTP 401** `Supabase session is invalid`.
- **Not tested (requires user):** real NEP-413 wallet signature, session issuance/restore, account switching in browser. Expiry/replay enforced by implementation/SQL (challenge consumed atomically); do not attempt real signature without approval.

## Hosted Supabase security (read-only)
- Project `llqwiqhwrbfoxoqqgczy`; 6 migrations match remote.
- Live probes (publishable key): `wallet_auth_identity` RPC → **42501 permission denied**; `marketplace_withdrawals` SELECT → **42501 permission denied**; `record_withdrawal_intent` (empty params) → **404/PGRST202**. **Full RLS/storage/cross-user/role matrix NOT VERIFIED live** — run SQL Editor checks (see prior report / PRODUCTION_READINESS). No hosted policies/migrations changed.

## Tests/builds (exact)
- JS full: `node --test --experimental-vm-modules tests/*.test.mjs` → **106 passed, 0 failed**.
- NFT Rust: `cargo test --manifest-path contracts/nft/Cargo.toml --target x86_64-unknown-linux-gnu` → **51 passed, 0 failed**.
- Marketplace Rust: `cargo test --manifest-path contracts/marketplace-deploy/Cargo.toml --target x86_64-unknown-linux-gnu` → **33 passed, 0 failed**.
- Rust fmt (both): **PASS**.
- Clippy (NFT, wasm32) `-- -D warnings`: **PASS**. Clippy (Marketplace, wasm32): **PASS**. Host-target clippy previously failed due env dependency resolution; wasm32 target is appropriate for NEAR contracts and passes.
- Frontend lint: **PASS**. Frontend build: **PASS** (existing `js-sha256` eval + chunk warnings).

## Features status (honest)
- **Mocked/local verified:** JS unit/integration tests for wallet auth flow logic, favorites/consistency, price precision, marketplace safety/gates, settlement/withdrawal/catalog verifiers, migration/security source checks.
- **Read-only/hosted verified (limited):** Edge Function challenge/auth rejections, key RPC/table denials, migration sync, function deployment.
- **Not proven against real Testnet:** NEP-413 signed login in browser, mint, approval, listing create/cancel, purchase, settlement, withdrawals/refunds, storage ops, failed-tx recovery, duplicate/replayed event handling on live chain (requires user-approved signed txs; gates disabled).

## Trading gates
`MINTING_ACTIONS_ENABLED=false`, `LISTING_ACTIONS_ENABLED=false`, `PURCHASES_ENABLED=false` (unchanged).

## User-approved next steps (signed Testnet)
1. In browser at `http://localhost:5173`, connect Testnet wallet and complete NEP-413 auth (you sign). If wallet redirect/origin differs, set matching `NEAR_AUTH_APP_ORIGIN` securely and redeploy.
2. Run hosted RLS/storage/cross-user SQL checks (SQL Editor, read-only).
3. Approve worker dry-run/start only after reviewing RPC, contract IDs, pinned hashes, `*_START_HEIGHT`, checkpoints, service-role handling. Do not start hosted writers without approval.
4. Approve signed tx sequence (test accounts/small amounts): mint → index/bind → list/cancel → buy → settlement → withdraw/refund/storage → failure/replay. Record tx hashes/receipts; keep gates disabled unless explicitly/minimally approved.

## Safety confirmations
No contract redeployed/upgraded. No funds moved. No signed transactions executed by agent. No trading gates enabled. No secrets exposed. No commits/pushes. No destructive git commands. No hosted migrations/policy changes. No hosted workers started.
