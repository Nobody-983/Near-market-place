# Testnet end-to-end readiness

Date: 2026-11-06. Scope: post-deployment completion of the RoundFrog NEAR NFT
marketplace. No contract was deployed or upgraded, no transaction was signed or
broadcast, no account was created or funded, no Supabase project was modified,
and no trading gate was changed. All checks below are local/offline unless
explicitly marked as an operator action.

## 2026-11-09 continuation verification

A read-only RPC probe at finalized block
`C1SEwsExG5pC54oPGbaZcVadwqhFrCAphnaVjNStMFCi` (height `276970841`) confirmed
that the configured NFT and marketplace accounts exist, their live code hashes
match the pinned values below, the NFT is bound to the marketplace, and the
mint authority is `seller.roundfrog-dev.testnet`. Marketplace config reports
`state_version: 2`, a 1% fee, the expected NFT allowlist, and
`trading_enabled: false`. No transaction was submitted.

The current checkout does **not** reproduce those pinned artifacts:

| Contract | Local release SHA-256 / size | Pinned SHA-256 / size |
| --- | --- | --- |
| NFT | `495a841317b370871ace20b1616463244482d0147e803083942210fcc7cd4bf3` / 484734 bytes | `b8a500297d6e93bc79c3f06834d09f31bf384816301d179d1aabb425a6a4a84d` / 484725 bytes |
| Marketplace | `930294146258f0ccc90d2b6a8d4cfed94e989cc31db5b76d2bb87f3cdbade7ed` / 477336 bytes | `dc10f32c3680fa1d403baf5bbec2a71634d4e343b5a7a783a84a965aff94e846` / 475046 bytes |

Therefore the live accounts are verified against the recorded hashes, but this
working-tree source is **not proven to be the source of the deployed code**.
The deployment script's artifact-hash guard would reject the locally built
files. Keep minting and listing/cancellation disabled until the reviewed source,
build configuration, and deployed artifacts are reconciled and the signed
Testnet lifecycle plus hosted catalog reconciliation are independently verified.

Deployed Testnet identity assumed for this report (provided by the operator and
matched against the reviewed artifacts in the repository):

- NFT: `nft.roundfrog-dev.testnet`
- Marketplace: `market.roundfrog-dev.testnet`
- Operator / fee recipient: `operator.roundfrog-dev.testnet`
- Test seller: `seller.roundfrog-dev.testnet`
- Test buyer: `buyer.roundfrog-dev.testnet`
- Marketplace state version: 2, fee: 1%, `trading_enabled: false`

## 1. What was inspected

- Repository layout, Git status/diff and untracked files; no user changes were
  discarded (`frontend` working-tree edits, `contracts/`, `services/`,
  `supabase/` and `tests/` were preserved).
- Frontend entry points, routes, wallet context, NEP-413 auth flow, Supabase
  client, catalog/marketplace services, preview data and action-gate config.
- Both Rust contracts (`contracts/nft`, `contracts/marketplace-deploy`) including
  the withdrawal, storage, purchase and event modules.
- Catalog indexer and settlement worker, their verification boundaries,
  checkpointing and Supabase clients.
- All Supabase migrations, the Edge Function, RLS policies and existing SQL
  regression tests.
- Deployment/testnet documentation and artifact hashes.
- Existing JavaScript and Rust test suites.

Key findings before changes:

- The local `frontend/Nft/.env` still pointed at non-existent
  `nft-preview.testnet` / `market-preview.testnet`; `.env.example` was correct.
- Preview marketplace data was silently substituted for real listings whenever
  the catalog was empty or errored, and NFT details fabricated a preview NFT for
  any unknown ID.
- The settlement worker verified purchases but had no withdrawal/refund
  verification at all; no database table or RPC represented withdrawal
  lifecycle, so pending/succeeded/failed withdrawals could not be reconciled.
- `SETTLEMENT_START_HEIGHT` did not exist, so the settlement worker could only
  start 100 blocks behind the head.
- `MINTING_ACTIONS_ENABLED`, `LISTING_ACTIONS_ENABLED` and `PURCHASES_ENABLED`
  were already `false` and remain `false`.

## 2. What was implemented and exact files changed

### Hash discrepancy resolved

The local built marketplace WASM hash `dc10f32c3680fa1d403baf5bbec2a71634d4e343b5a7a783a84a965aff94e846` matches the `deploy_roundfrog_testnet.sh` deployment script and `ROUNDFROG_TESTNET_DEPLOYMENT.md`. The `8150745c511842db2cd264d9399ad49efd5c1f7e9b61f01abfcc086d0163ab27` hash documented in the 2026-11-04 `FINAL_REPORT.md` appears to originate from a different build configuration/Rust version and does not match the actual built artifact. The NFT WASM hash `b8a500297d6e93bc79c3f06834d09f31bf384816301d179d1aabb425a6a4a84d` is consistent across all documents and matches the actual built file.

### Frontend configuration

- `frontend/Nft/.env` (local, git-ignored): deployed contract IDs, deposit
  variables and `VITE_PREVIEW_DATA_ENABLED=false`.
- `frontend/Nft/.env.example`: documented deployed IDs, preview opt-in and the
  "no secret keys in Vite" rule.
- `frontend/Nft/src/config/preview.js` (new): explicit `VITE_PREVIEW_DATA_ENABLED`
  gate, default off.
- `frontend/Nft/src/lib/supabaseKey.js` (new): rejects `sb_secret_*`,
  `service_role*` and JWT keys whose payload role is `service_role`.
- `frontend/Nft/src/lib/supabase.jsx`: uses the publishable-key guard.
- `frontend/Nft/src/services/marketService.js`: real listings are never replaced
  by demo rows; query failures surface instead of silently showing preview data;
  `selectMarketplaceListings` centralizes the opt-in decision.
- `frontend/Nft/src/pages/nftDetails.jsx`: no fabricated NFT/listing for unknown
  IDs; preview rows only load for explicit preview IDs when enabled.
- `frontend/Nft/src/pages/dashboard.jsx`: trending fallback to preview data only
  when explicitly enabled.

### Withdrawal and refund reconciliation

- `services/settlement/withdrawals.js` (new): `WithdrawalVerifier` and
  `WithdrawalReconciler`.
  - Verifies `withdraw_proceeds`, `withdraw_fees`, `withdraw_refund`,
    `storage_withdraw` and registration-only `storage_deposit`.
  - Requires the signed call, exact yoctoNEAR/deposit, direct marketplace call
    receipt, transfer receipt, private `on_withdrawal_result` callback, promise
    data dependency, exactly one `withdrawal_succeeded`/`withdrawal_failed`
    event, boolean callback result, canonical finalized blocks, pinned code hash
    and historical pre/post views.
  - Compares `pending_withdrawal`, `ledger`, proceeds/refund/storage balances and
    buyer-refund settlement status; failed transfers must restore balances and
    successful ones must debit exactly once.
  - Facts are opaque (`WeakSet`-guarded) and only the reconciler can consume them.
- `services/settlement/index.js`: routes purchase and withdrawal transactions to
  separate verifiers in the same scan; adds `SETTLEMENT_START_HEIGHT`.
- `services/settlement/verifier.js`: adds `matchesPurchase` and exports
  `executionStatus`; purchase behavior is unchanged.
- `services/settlement/supabase.js`: allows the three withdrawal RPCs.
- `supabase/migrations/20261106000100_withdrawal_reconciliation.sql` (new):
  `marketplace_withdrawals` table, completion-evidence constraint, unique
  transaction/nonce indexes, `record_withdrawal_intent`, `confirm_withdrawal`,
  `fail_withdrawal`, service-role-only ACLs/RLS and a pending-reconciliation view.

### Environment templates and documentation

- `services/settlement/settlement.env.example` (new)
- `services/indexer/catalog-indexer.env.example` (new)
- `services/settlement/README.md`, `services/indexer/README.md`: run commands,
  start-height semantics and withdrawal lifecycle.
- `contracts/testnet/roundfrog.env.example`: adds `SETTLEMENT_START_HEIGHT` and
  worker template references.
- `PRODUCTION_READINESS.md`: current status and remaining blockers.
- This report.

### Tests added/corrected

- `tests/withdrawals.test.mjs` (new): 11 withdrawal verifier/reconciler tests.
- `tests/postgres/withdrawal_reconciliation.test.sql` (new): idempotency,
  conflicting replay, nonce reuse, failure evidence, ACLs and completion
  constraints, executed on PostgreSQL 15.
- `tests/security-migrations.test.mjs`: source-level withdrawal security checks.
- `tests/marketplace-safety.test.mjs`: all three gates closed, publishable-key
  guard, env template assertions and preview opt-in.
- `tests/near-price-precision.test.mjs`: preview fallback expectations updated
  for the explicit opt-in.

## 3. Tests and builds run (all passed)

| Check | Result |
| --- | --- |
| `node tests/marketplace-safety.test.mjs` | 9 passed, 0 failed |
| `node tests/settlement.test.mjs` | 17 passed, 0 failed |
| `node tests/security-migrations.test.mjs` | 12 passed, 0 failed |
| `node tests/catalog-indexer.test.mjs` | 7 passed, 0 failed |
| `node tests/withdrawals.test.mjs` | 11 passed, 0 failed |
| `node --experimental-vm-modules tests/near-price-precision.test.mjs` | 14 passed, 0 failed |
| `node --experimental-vm-modules tests/near-wallet-auth.test.mjs` | 21 passed, 0 failed |
| `node --experimental-vm-modules tests/marketplace-consistency.test.mjs` | 15 passed, 0 failed |
| Total JavaScript | **106 passed, 0 failed** |
| `cargo test --manifest-path contracts/marketplace-deploy/Cargo.toml --locked --offline --target x86_64-unknown-linux-gnu` | 33 passed, 0 failed |
| `cargo test --manifest-path contracts/nft/Cargo.toml --locked --offline --target x86_64-unknown-linux-gnu` | 51 passed, 0 failed |
| `cargo fmt --check` (both contracts) | clean |
| `cargo clippy --all-targets -- -D warnings` (both contracts) | clean |
| `npm --prefix frontend/Nft run lint` (ESLint) | clean |
| `npm --prefix frontend/Nft run build` | succeeded (existing `js-sha256` direct-eval and >500 kB chunk warnings only) |
| Full migration chain on scratch PostgreSQL 15 + withdrawal and catalog SQL tests | all applied/passed |
| `git diff --check` | clean |

Limitations: the JavaScript tests use mocked HTTP/wallets and synthetic NEAR RPC
receipt shapes. The SQL run used local PostgreSQL stubs for Supabase
Auth/Storage, not hosted RLS/PostgREST/GoTrue. No deployed Testnet receipt was
fetched or replayed. No browser/wallet interaction was performed.

## 4. Environment variables to fill locally (names only)

Frontend (`frontend/Nft/.env`, publishable values only):

- `VITE_SUPABASE_URL`
- `VITE_SUPABASE_PUBLISHABLE_KEY`
- `VITE_NEAR_NETWORK_ID`
- `VITE_NEAR_NODE_URL`
- `VITE_NEAR_WALLET_URL`
- `VITE_NEAR_HELPER_URL`
- `VITE_NEAR_EXPLORER_URL`
- `VITE_NFT_CONTRACT_ID`
- `VITE_MARKETPLACE_CONTRACT_ID`
- `VITE_NFT_MINT_DEPOSIT_YOCTO`
- `VITE_NFT_APPROVAL_DEPOSIT_YOCTO`
- `VITE_PREVIEW_DATA_ENABLED`

Catalog indexer (`services/indexer/catalog-indexer.env`):

- `NEAR_NETWORK_ID`
- `NEAR_RPC_URL`
- `NEAR_MARKETPLACE_CONTRACT_ID`
- `NEAR_NFT_CONTRACT_ID`
- `NEAR_MARKETPLACE_CODE_HASH`
- `NEAR_NFT_CODE_HASH`
- `SUPABASE_URL`
- `SUPABASE_SERVICE_ROLE_KEY`
- `CATALOG_INDEXER_PROCESSED_PATH`
- `CATALOG_INDEXER_START_HEIGHT`

Settlement worker (`services/settlement/settlement.env`):

- `NEAR_NETWORK_ID`
- `NEAR_RPC_URL`
- `NEAR_MARKETPLACE_CONTRACT_ID`
- `NEAR_NFT_CONTRACT_ID`
- `NEAR_MARKETPLACE_CODE_HASH`
- `NEAR_NFT_CODE_HASH`
- `SUPABASE_URL`
- `SUPABASE_SERVICE_ROLE_KEY`
- `SETTLEMENT_PROCESSED_PATH`
- `SETTLEMENT_START_HEIGHT`

Do not put `SUPABASE_SERVICE_ROLE_KEY` or any other secret in a `VITE_*` variable.
No credential values are recorded in this repository.

## 5. Exact commands to start each component

Frontend (development):

```sh
npm --prefix frontend/Nft install
npm --prefix frontend/Nft run dev
```

Production build/preview:

```sh
npm --prefix frontend/Nft run build
npm --prefix frontend/Nft run preview
```

Catalog indexer:

```sh
set -a; . ./services/indexer/catalog-indexer.env; set +a
node services/indexer/index.js
```

Settlement worker (purchases **and** withdrawals/refunds):

```sh
set -a; . ./services/settlement/settlement.env; set +a
node services/settlement/index.js
```

Start both workers before the first demo transaction. Record the current
finalized head as `CATALOG_INDEXER_START_HEIGHT` and `SETTLEMENT_START_HEIGHT`,
or accept the 100-block lookback for a brand-new checkpoint. Keep the
`*-processed.jsonl` files on persistent storage; Supabase remains the
authoritative idempotency boundary.

Apply the Supabase migrations in order (`tests/postgres/supabase_stubs.sql` is
only for local PostgreSQL, never for hosted Supabase). The new migration is:

```text
supabase/migrations/20261106000100_withdrawal_reconciliation.sql
```

## 6. Remaining blockers to a safe Testnet end-to-end run

1. **Hosted Supabase not validated.** The new migration and reconcile RPCs were
   executed only against local PostgreSQL 15 with Auth/Storage stubs. Apply the
   migration to the intended project, confirm the `marketplace_private` schema is
   not exposed, and run hosted RLS/PostgREST/Auth smoke tests.
2. **No wallet-auth provisioning evidence.** Indexers resolve wallet addresses to
   profile UUIDs, so seller/buyer/operator profiles must exist with the exact
   Testnet account IDs and valid verified sessions.
3. **Trading is disabled by design.** `trading_enabled` is `false` and
   `MINTING_ACTIONS_ENABLED`, `LISTING_ACTIONS_ENABLED` and
   `PURCHASES_ENABLED` must remain `false`. A live purchase, proceeds/fee
   withdrawal or buyer refund cannot be exercised until the operator separately
   approves enabling trading and the write paths.
4. **No live receipt evidence retained.** The verifier is tested against
   synthetic graphs; it must be run against real final Testnet transactions and
   the raw `EXPERIMENTAL_tx_status`, block and historical view responses kept for
   review before the gates open.
5. **RPC history retention.** The configured public Testnet RPC may prune old
   blocks. Use an archival endpoint or start at a recorded finalized height
   before the demo; the worker refuses to cross pruned history.
6. **Storage funding.** The seller/marketplace storage deposit must be funded and
   the storage-withdrawal/registration-excess cases exercised with real receipts.
7. **Contract account/key custody.** Deploy keys, the Supabase service-role key
   and any signing keys must stay outside the repository and browser. None were
   requested or used here.

## 7. Checks that require explicit approval or live action

- Applying the new migration (and any RLS changes) to a hosted Supabase project.
- Any funding, account creation, mint, approval/listing, cancellation, purchase,
  storage deposit/withdraw, proceeds/fee withdraw or refund withdrawal.
- Enabling `trading_enabled` on the marketplace and enabling any frontend action
  gate. These are separate, explicitly reviewed actions and were **not**
  performed.
- Producing/retaining live Testnet receipt and historical-state evidence, then
  replaying it through the workers against the hosted database.

Until those are approved and completed, the correct state is: contracts deployed,
workers ready, frontend pointed at the deployed contracts, and all trading/
minting/listing actions disabled.
