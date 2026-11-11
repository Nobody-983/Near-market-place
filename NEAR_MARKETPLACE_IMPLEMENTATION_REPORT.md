# NEAR Marketplace Implementation Report

Date: 2026-11-04  
Status at time of writing: implemented and locally verified; not deployed; trading
disabled.

> **Update 2026-11-06:** the contracts are now deployed at
> `nft.roundfrog-dev.testnet` and `market.roundfrog-dev.testnet` and verified by
> the operator. This report is retained as the implementation record; the
> current runbook, test counts, environment variables and remaining blockers are
> in [TESTNET_E2E_READINESS.md](TESTNET_E2E_READINESS.md).

## Scope and repository audit

The existing project was preserved. No unrelated application was created, the
frontend was not redesigned, Supabase authentication and NEAR Wallet Selector
remain in place, and Agent Leash was not modified.

The repository contains three marketplace-related Rust packages with distinct
roles:

- `contracts/marketplace` is a non-deployable accounting and persistence
  foundation with extensive model tests.
- `contracts/marketplace/runtime` is a deliberately disabled host-storage
  prototype. It requires an explicit artifact feature and exposes no production
  trading workflow.
- `contracts/marketplace-deploy` is the conventional deployable NEAR contract
  already expected by the frontend and settlement service. This package was
  completed instead of duplicating the marketplace in a new project.

The NFT contract in `contracts/nft` implements NEP-171 core, enumeration and
metadata interfaces and NEP-178 approvals. It also contains an immutable
marketplace binding and `marketplace_transfer_approved`, which authenticates the
marketplace predecessor, seller, token, exact approval ID and receiver before a
synchronous transfer. NEP-199 royalty/payout interfaces are not implemented.
The marketplace therefore pays the configured marketplace fee and seller
remainder only.

Deployment configuration already existed through pinned Cargo locks, explicit
`testnet`/`production` build features and reviewed build scripts. No live
contract account, deployment receipt or deployed code hash is established by
this work.

Frontend integration points remain:

- NEAR Wallet Selector and wallet state in
  `frontend/Nft/src/context/walletContext.jsx`.
- Supabase authentication, storage and application data through
  `frontend/Nft/src/lib/supabase.jsx` and `frontend/Nft/src/services`.
- A trusted settlement/indexing service in `services/settlement`; Supabase is an
  index and is not accepted as payment evidence.
- Fixed frontend safety gates in
  `frontend/Nft/src/config/marketplace.js`. Both remain `false`.

## Implemented contract architecture

The deployable marketplace is an approval-based, fixed-price, native-NEAR
contract.

1. A seller pre-funds NEP-145 storage and calls the NFT's `nft_approve` with
   structured, versioned terms.
2. `nft_on_approve` authenticates the allowlisted NFT predecessor and original
   seller signer. Terms bind marketplace, NFT contract, token, seller, exact
   yoctoNEAR price, fee snapshot, fee recipient and optional expiration.
3. Each NFT/token asset has a monotonically increasing approval watermark and
   immutable marketplace generation. Replaced generations remain replay
   barriers.
4. `buy_nft` accepts only the exact price, locks the listing before dispatch,
   records a pending sale and durable settlement, then calls the NFT's exact
   approval-transfer wrapper.
5. The private callback accepts only its stored sale ID and derives all seller,
   fee and refund accounting from authenticated contract state.
6. Successful delivery credits pull-payment balances. Failed or malformed NFT
   delivery creates one durable buyer refund.
7. Seller, fee and buyer-refund withdrawals use nonce-bound pending records.
   Failed transfers restore the exact source balance; stale callbacks and
   callback replay cannot pay twice.
8. NEP-145 registration excess and storage withdrawals use exact direct refund
   transfers, avoiding callback bookkeeping that would consume the balance being
   refunded.
9. Listings, sale records, settlement records, liabilities and bounded
   NEP-297-style events provide an auditable on-chain trail.

Public views include configuration, ledger totals, listings, listing metadata,
sales, settlements, seller proceeds, fee proceeds, buyer refunds, pending
withdrawals and paginated transaction history. Pagination is capped at 100.

## Security decisions

- Browser or Supabase payment confirmations are never trusted.
- Purchases require exact attached yoctoNEAR and checked arithmetic.
- Listing terms are seller-authorized and fee policy is snapshotted at listing
  creation, preventing later administrative fee changes from rewriting a sale.
- NFT contract allowlisting and immutable NFT-side marketplace binding prevent
  arbitrary callback sources.
- The callback verifies stored NFT contract, token, seller, approval ID, price,
  fee split and sale generation before accounting.
- Listing state is locked before the cross-contract call, preventing concurrent
  purchases and cancellation races.
- Failed NFT delivery produces one refund liability; terminal settlement states
  make repeated callbacks idempotent.
- Seller, fee and refund transfers use pull payments with failure restoration.
- Storage is prefunded and charged before state growth. Listing creation reserves
  bounded sale storage in advance. Registration-only deposits now keep only the
  registration cost and refund all excess; maximum storage withdrawal is also
  covered by regression tests.
- Owner administration requires one yoctoNEAR and predecessor authentication.
- Fresh initialization requires a direct zero-deposit self-call. Migration is a
  direct self-call, preserves known liabilities and always leaves trading
  disabled.
- Cross-contract gas is bounded: 30 Tgas for NFT transfer, 40 Tgas for settlement
  callback and 20 Tgas for monetary withdrawal callbacks.
- Contract methods bound token IDs, approval messages and history pages.

Previously identified stale-approval, seller-consent, authenticated accounting,
storage-backing and callback-gas risks are addressed by the mechanisms above.
Approval storage/refund behavior remains owned by the existing NFT contract.

## Files implemented or integrated

Marketplace contract:

- `contracts/marketplace-deploy/src/lib.rs`
- `contracts/marketplace-deploy/src/listing.rs`
- `contracts/marketplace-deploy/src/purchase.rs`
- `contracts/marketplace-deploy/src/withdraw.rs`
- `contracts/marketplace-deploy/src/events.rs`
- `contracts/marketplace-deploy/src/storage.rs`
- `contracts/marketplace-deploy/src/migration.rs`
- `contracts/marketplace-deploy/src/tests.rs`
- `contracts/marketplace-deploy/README.md`

Frontend preparation, with the existing design and disabled safety gates:

- `frontend/Nft/src/lib/near/contract.js`
- `frontend/Nft/src/pages/createNft.jsx`
- `frontend/Nft/src/pages/wallet.jsx`
- `frontend/Nft/src/config/marketplace.js`

Trusted settlement verification:

- `services/settlement/verifier.js`
- `services/settlement/README.md`
- `tests/settlement.test.mjs`

This report does not claim unrelated dirty-worktree files as marketplace changes.

## Frontend integration status

The existing UI now has contract helpers and prepared flows for:

- wallet-backed listing approval with structured terms;
- listing cancellation and expiration;
- exact-price purchases and final transaction polling;
- storage balance, seller proceeds, fee proceeds and buyer-refund views;
- proceeds, fee and refund withdrawals;
- settlement and transaction-history display.

The browser does not publish or settle Supabase listings based on its own
transaction claims. A trusted indexer must publish verified listing state and the
settlement worker must reconcile finalized purchases. Trading remains disabled:

```js
export const LISTING_ACTIONS_ENABLED = false;
export const PURCHASES_ENABLED = false;
```

## Test and build evidence

Marketplace native tests:

```text
CARGO_ENCODED_RUSTFLAGS='' cargo test --manifest-path contracts/marketplace-deploy/Cargo.toml --locked --target x86_64-unknown-linux-gnu
test result: ok. 21 passed; 0 failed; 0 ignored
```

Marketplace lint:

```text
CARGO_ENCODED_RUSTFLAGS='' cargo clippy --manifest-path contracts/marketplace-deploy/Cargo.toml --locked --target x86_64-unknown-linux-gnu --all-targets -- -D warnings
Finished `dev` profile ...
```

NFT contract:

```text
test result: ok. 51 passed; 0 failed; 0 ignored
```

Marketplace foundation:

```text
unit tests: 50 passed
foundations integration tests: 27 passed
isolation integration tests: 4 passed
doc tests: 2 passed
```

Disabled runtime prototype, using its required artifact domain:

```text
cargo test ... --features testnet
test result: ok. 39 passed; 0 failed; 0 ignored
```

Running that prototype without `testnet` or `production` intentionally fails with
`select exactly one runtime artifact`; this is a build guard, not a test failure.

Local signer/tooling crate:

```text
test result: ok. 1 passed; 0 failed
```

JavaScript tests:

```text
node --experimental-vm-modules --test tests/*.test.mjs
tests 6; pass 6; fail 0
```

Python Testnet tooling tests:

```text
python3 -m unittest discover -s contracts/testnet -p 'test_*.py'
Ran 12 tests ... OK
```

Frontend production build:

```text
npm run build --prefix frontend/Nft
3021 modules transformed
build completed successfully
```

The frontend build still reports dependency externalization, direct `eval` in
`js-sha256`, and large-chunk warnings. Full repository ESLint still fails on
existing React effect/fast-refresh/memoization findings in unrelated UI files;
the new NEAR contract helper and wallet integration files pass targeted ESLint.

NEAR Sandbox was not run. The environment has no `near-sandbox` or `neard`
binary and no `near-workspaces` dependency. Sandbox success is therefore not
claimed.

## Rebuilt Testnet artifacts

These are local build artifacts, not deployed-code evidence.

| Contract | Bytes | SHA-256 | NEAR base58 code hash |
| --- | ---: | --- | --- |
| Marketplace | 475046 | `dc10f32c3680fa1d403baf5bbec2a71634d4e343b5a7a783a84a965aff94e846` | `Fp3ggF3WmSMQC7m9FU717jA8nB897sYW6bik7Ysqipw7` |
| NFT | 484725 | `b8a500297d6e93bc79c3f06834d09f31bf384816301d179d1aabb425a6a4a84d` | `DRmwjtfeBzXW2XgXMumytEY5scsDzEtCGsbVNKjdEtW8` |

## Remaining blockers

- No deployment or real Testnet receipt graph has been exercised.
- NEAR Sandbox is unavailable in the current environment.
- Trusted listing publication/indexing must be implemented or configured; the
  browser must not directly mark a listing as verified.
- The hosted settlement worker and Supabase reconciliation RPC require
  end-to-end validation against finalized Testnet receipts and historical state.
- Contract account IDs, owner/fee account, fee policy and operational storage
  deposits require an explicit release decision.
- Reviewed deployed code hashes must be pinned in the settlement worker.
- NEP-199 royalties are unsupported and require a separate interface,
  accounting and security review if desired.
- Existing frontend lint debt and bundle warnings remain release-quality work,
  although the production build succeeds.

## Testnet integration procedure

Do not perform these steps without explicit deployment authorization.

1. Select isolated Testnet NFT, marketplace, owner/fee and test-user accounts.
   Do not place keys in the repository or browser configuration.
2. Rebuild both Wasm artifacts from the reviewed commit and independently verify
   byte sizes and hashes.
3. Deploy the marketplace and NFT from their own contract accounts so the
   direct-self initialization guards are satisfied.
4. Initialize the NFT with `new_with_marketplace`, binding the exact marketplace
   account. Initialize the marketplace with the exact NFT account, owner and fee
   policy.
5. Confirm `config().trading_enabled == false` and verify the NFT's immutable
   `marketplace_id` before funding storage or testing.
6. Register owner and seller storage, then exercise listing, replacement,
   cancellation, expiration, exact purchase, incorrect amount, concurrent
   purchase, failed NFT transfer, refund, proceeds, fee and failed withdrawal
   flows using isolated test accounts.
7. Retain final transaction and receipt RPC responses. Verify deployed code
   hashes and historical views at the callback block.
8. Configure the trusted settlement worker with the reviewed account IDs and
   deployed code hashes, then validate Supabase reconciliation and replay
   idempotency.
9. Keep both frontend gates disabled until those checks pass. Enabling on-chain
   trading and enabling browser actions must be separate, explicitly reviewed
   actions.

No Testnet or Mainnet deployment was performed, no private key was requested or
read, and no trading gate was enabled.
