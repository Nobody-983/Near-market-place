# Production readiness

This assessment covers the current repository, not a deployed system. No live
Supabase project or wallet extension was contacted. Read-only NEAR Testnet RPC
checks were performed on 2026-11-04. Buying, listing, and cancellation must
remain disabled in
[`config/marketplace.js`](frontend/Nft/src/config/marketplace.js).

See the [contract/interface review](contracts/MARKETPLACE_REVIEW.md) for the
Testnet account IDs, source-confirmed NFT methods, marketplace design options
and required acceptance tests. The current post-deployment runbook, exact
commands, test counts and blockers are in
[TESTNET_E2E_READINESS.md](TESTNET_E2E_READINESS.md).

Current release statuses (2026-11-06):

- Settlement service: **IMPLEMENTED, UNIT-TESTED** (purchases, withdrawals and
  registration-excess refunds)
- Marketplace contract fixes: **IMPLEMENTED, UNIT-TESTED**
- NFT contract: **IMPLEMENTED, UNIT-TESTED**
- Contracts: **TESTNET-DEPLOYED** at `nft.roundfrog-dev.testnet` and
  `market.roundfrog-dev.testnet` (operator-verified; deployed hashes match the
  reviewed artifacts)
- Purchase/receipt lifecycle: **LOCAL EVIDENCE ONLY**; no live Testnet purchase
  has been exercised while `trading_enabled` is false
- Hosted reconciliation: **BLOCKED** until the migrations are applied to the
  intended Supabase project and hosted RLS/PostgREST is validated

## Settlement hardening result

The settlement worker now scans finalized block chunks, retrieves transactions
from `chunk.transactions`, requests full transaction/receipt results with
`wait_until: "FINAL"`, validates the complete promise graph and NEP-171 transfer
log, pins both contract code hashes, and confirms historical marketplace sale,
listing, binding and NFT ownership state at the callback block. Reconciliation
accepts only verifier-produced facts and always reaches the database's locked,
idempotent service-role RPC. A durable block checkpoint supports restarts without
limiting catch-up to the last 100 blocks.

The marketplace contract emits no sale event. Its evidence is the linked
`buy_nft` receipt, NFT `marketplace_transfer_approved` receipt, private
`on_transfer_result` callback, and settled `sale(...)` state. The NFT contract
emits the NEP-171 `EVENT_JSON` `nft_transfer` log.

Local verification includes 17 settlement and 11 withdrawal fixture/service
regressions based on actual NEAR RPC field layouts, plus a PostgreSQL 15
execution of all migrations with atomic reconciliation, replay, conflict,
uniqueness, withdrawal-completion and privilege checks. This does not
demonstrate a real marketplace purchase or withdrawal on Testnet.

The marketplace and NFT contracts are actively deployed to Testnet. Read-only
RPC checks against finalized blocks confirm `state_version: 2`, a 1% fee,
`trading_enabled: false`, correct NFT/marketplace binding, mint authority
`seller.roundfrog-dev.testnet`, and live on-chain code hashes matching the pinned
reviewed artifacts.

Exact validation on 2026-11-10:

- all 113 JavaScript tests passed (15 consistency, 9 safety, 14 price, 21 wallet
  auth, 12 migration-security, 7 catalog-indexer, 17 settlement, 11 withdrawal,
  theme and motion);
- all 12 Python testnet tests passed;
- the frontend production build and ESLint passed (Vite built in 2.62s);
- all 104 NFT Rust tests (51 default + 53 testnet feature), formatting and Clippy passed;
- all 33 deployable marketplace Rust tests, formatting and Clippy passed;
- both wasm32 and host Clippy targets compile cleanly with zero warnings;
- all six Supabase migrations executed on PostgreSQL 15, including withdrawal
  reconciliation and catalog chain indexing SQL test suites;
- contract artifact reproducibility is verified via `scripts/verify-contracts.sh`
  with the NFT Wasm matching on-chain code byte-for-byte (`b8a50029...4a84d`).

## What is present

- [`contracts/nft/src/lib.rs`](contracts/nft/src/lib.rs) implements NFT minting,
  transfer/transfer-call with a private resolver, approvals/revocation, metadata,
  and enumeration using `near-contract-standards` 5.30.0. It contains **no sale,
  purchase, cancellation, payout, or marketplace receiver implementation**.
- `nft_mint` is permissionless: the wrapper and the SDK's `internal_mint` do not
  require the contract owner. It accepts a caller-selected token ID, recipient,
  and metadata; the caller pays actual storage costs. Confirm this mint policy
  before deploying or presenting the collection as curated. The frontend's
  fixed mint deposit is not a storage estimate for arbitrary metadata.
- [`contracts/marketplace-deploy`](contracts/marketplace-deploy) contains the
  approval-based `nft_on_approve`, `buy_nft`, callback and withdrawal contract
  reviewed by the settlement verifier. The operator reports it deployed at
  `market.roundfrog-dev.testnet` with hashes matching the reviewed artifacts.
- Catalog creation is an off-chain Supabase insert. It does not call `nft_mint`.
  Only a trusted process may attach verified chain identity to a catalog row.
- Favorites use the server-verified WalletContext user UUID. Writes require a
  returned matching row; stale identity/view responses are ignored. Public counts
  and favorites are snapshots, subject to the API's row limit; other-tab changes
  and large collections still need refresh/pagination work.
- NFT deletion requires a returned ID. UI eligibility uses creator/current owner,
  `source = 'user'`, unminted status, and **no listing history of any status**.
  RLS makes the final decision when the displayed metadata becomes stale.
- The NEP-413 authentication function and private session-bound SQL checks are
  present. The settlement RPC remains a persistence boundary; the trusted
  worker performs chain verification before calling it.

## Required before enabling trading

1. **Confirm the deployed contract identity and pin its real code hashes.**
   Verify initialization, marketplace `config.nft_contract_id`, NFT
   `marketplace_id`, source/build reproducibility and the exact deployed code
   hashes before starting the workers. The operator reports these match the
   reviewed artifacts; the workers must be configured with those exact hashes.
2. **Verify listing publication and cancellation on chain.** Confirm token owner,
   authorization/approval ID or escrow, exact price and currency, and active sale
   state. Publish an active database listing only after final successful receipts.
   Cancel only after verified final chain state. Browser `createListing` and
   `cancelListing` still perform database-only writes which existing RLS denies;
   replace these paths with the verified workflow, never relax those grants.
3. **Validate purchase identity before requesting a wallet transaction.** Resolve
   the database UUID to the opaque `contract_sale_id`, and match `network_id`,
   `marketplace_contract_id`, NFT contract/token, seller, buyer and live sale price
   against allowlisted chain state. The disabled handler now supplies the opaque
   `contract_sale_id`, validates network/marketplace identity, uses exact yoctoNEAR
   and waits for final status. A trusted live listing-state quote check is still
   required before this gate can open.
4. **Validate native-payment failure recovery against live receipts.** Per-seller
   proceeds, post-delivery liability credit, failed-transfer buyer refunds and
   callback-safe withdrawals are implemented and unit-tested. The settlement
   worker now verifies and reconciles pending/succeeded/failed seller, fee,
   refund, storage and registration-excess withdrawals into
   `marketplace_withdrawals`, and refuses to complete them without callback
   block evidence. Live Testnet receipts for each path are still required.
5. **Fund storage safely.** Marketplace listing/sale storage is currently
   sponsored by the contract without caller storage deposits or quotas. Bound or
   charge this growth before enabling untrusted listings.
6. **Validate the complete flow in an isolated environment before opening gates.**
   Use a disposable Supabase project and NEAR Testnet accounts/contracts after
   deployment authorization. Test wrong network/sale/owner/price,
   insufficient deposit, revoked approvals, failed cross-contract callbacks,
   buyer refunds, seller isolation, wallet cancellation/redirects, concurrent
   purchases and replay. Retain the raw final RPC responses and demonstrate the
   worker reconciling them into the disposable database. Keep the gates disabled
   until these checks pass.
7. **Define the production RPC trust model.** The worker requires HTTPS and
   checks final canonical blocks and code hashes, but it trusts the configured
   RPC node's chain view. Use a reviewed archival provider or add independent
   light-client/proof verification and provider disagreement handling.

## Deployment boundary

No account creation, funding, deployment or signed transaction was performed.
The current reviewed deployment plan is in
[`contracts/marketplace-deploy/README.md`](contracts/marketplace-deploy/README.md).
At the observed Testnet storage price, raw code storage requires 2.91327 NEAR for
the marketplace and 4.84725 NEAR for the NFT. The fresh accounts should be
created/funded to at least 4 and 6 Testnet NEAR respectively before atomic
deployment and initialization. The owner, mint authority, fee policy and final
collection metadata still require authorized choices.

## Supabase, authentication and retained images

The migrations are prepared for a **new** Supabase project. All six executed on
a temporary PostgreSQL 15 database with minimal Supabase Auth/Storage schema
stubs, including settlement locking, withdrawal completion constraints and
rollback behavior. Hosted Supabase Auth,
Storage, RLS gateway and PostgREST behavior has not been exercised.
Follow the exact prerequisites in
[`supabase/NEAR_WALLET_AUTH.md`](supabase/NEAR_WALLET_AUTH.md#still-required-before-live-use).
In particular:

- Verify NEP-413 login and the actual Auth user/session binding, expiry, logout,
  account switching and revoked sessions. Anonymous, ordinary/unverified, expired
  and other-user sessions must fail protected writes; zero-row deletes must not
  look successful. Keep `marketplace_private` unexposed. Public catalog, ownership,
  listing and like reads do not confer write authority.
- Disable public/anonymous signup, keep the required server Admin Auth exchange,
  configure canonical origin/network and server-only secrets, and retire any
  legacy predictable credentials through verified account migration. Never put a
  service-role key in a `VITE_*` variable or browser request.
- **Challenge DoS is unresolved.** There is no trusted gateway enforcement point
  in this repo. Configure the documented 10 requests/rolling 60 seconds per
  observed source IP, covering the hosted direct URL as well as custom domains,
  before the function/RPC. Strip untrusted forwarding headers and block bypasses.
  The database's non-blocking global gate does not replace this requirement.
- Verify the exact configured public Storage URL prefix, canonical user/object
  path, object ownership, MIME and size limits. Browser Storage is append-only.
  Deleted catalog entries and failed/ambiguous catalog inserts retain uploaded
  images. No UI flow calls the legacy `deleteNFTImage` helper, which is unsupported
  by current policies. A trusted cleanup job remains missing: use a retention
  grace period, recheck database and chain references immediately before removal,
  and account for inserts whose response was lost. Do not grant browser deletion
  or remove artwork merely because one client saw an error.

## Local verification

Run from the repository root with existing dependencies installed:

```sh
node tests/marketplace-safety.test.mjs
node tests/settlement.test.mjs
node tests/withdrawals.test.mjs
node tests/security-migrations.test.mjs
node --experimental-vm-modules tests/marketplace-consistency.test.mjs
node --experimental-vm-modules tests/near-price-precision.test.mjs
node --experimental-vm-modules tests/near-wallet-auth.test.mjs
cargo test --locked --offline --manifest-path contracts/nft/Cargo.toml --target x86_64-unknown-linux-gnu
cargo test --locked --offline --manifest-path contracts/marketplace-deploy/Cargo.toml --target x86_64-unknown-linux-gnu
bash contracts/marketplace-deploy/build.sh testnet
bash contracts/nft/build.sh testnet
npm --prefix frontend/Nft run build
git diff --check
```

JavaScript regressions use mocked HTTP/wallets and deterministic hook/handler
execution. Settlement and withdrawal tests use realistic RPC-shaped fixtures. The
reconciliation migrations were also executed in temporary PostgreSQL 15,
including the withdrawal lifecycle, but that is not a hosted Supabase
validation. Rust tests use the SDK's mocked VM. A production
build does not validate wallets, deployed contracts, Testnet receipts or hosted
RLS/PostgREST behavior.
