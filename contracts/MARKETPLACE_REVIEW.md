# Marketplace contract and integration review

Repository review: 2026-10-29. Source inspection and offline tests only. Neither
NEAR RPC nor a live Supabase project was contacted. No transaction, deployment,
migration, contract change, or frontend design change was made.

## Configured accounts versus verified deployments

Only the public NEAR settings were extracted from `frontend/Nft/.env`:

| Setting | Repository value | Verification status |
| --- | --- | --- |
| `VITE_NEAR_NETWORK_ID` | `testnet` | Configured network, not proof of a deployed environment |
| `VITE_NEAR_NODE_URL` | `https://rpc.testnet.near.org` | Configured public RPC; not contacted |
| `VITE_NFT_CONTRACT_ID` | `nft-preview.testnet` | Configured account ID; existence, code and initialization unverified |
| `VITE_MARKETPLACE_CONTRACT_ID` | `market-preview.testnet` | Configured account ID; existence, code and initialization unverified |

[`config/near.js`](../frontend/Nft/src/config/near.js) reads these build-time
variables; `WalletProvider` initializes Wallet Selector with `NETWORK_ID`.
`.env.example` contains template account IDs. Documentation contains setup
examples, not deployment evidence. No marketplace Rust/source package, generated
marketplace ABI, deployment receipt, pinned deployed code hash, deployment
manifest, or CI deployment configuration was found. No WASM/ABI artifact was
found in the NFT package's local target directory either.

**A compatible deployed marketplace has not been established.** This does not
prove that the configured account is absent on Testnet. There is no source-to-code
mapping with which to verify its methods. Keep both safety gates false. Future
deployment verification must record the network, account, code hash, source
revision, reproducible WASM/ABI, initialization and upgrade authority.

## Interface confirmed from NFT source

The interface evidence is [`nft/src/lib.rs`](nft/src/lib.rs), backed by the locked
`near-contract-standards` 5.30.0 implementation. There is no standalone generated
ABI in the repository. The following are NFT operations, not marketplace sales:

| Source interface | Confirmed behavior and authorization |
| --- | --- |
| `new(owner_id, metadata)` / `new_default_meta(owner_id, name, symbol, icon)` | Initialization and contract metadata; an owner is recorded |
| `nft_mint(token_id, token_owner_id, token_metadata)` | Payable, permissionless minting; rejects blank/duplicate IDs, validates metadata structure, charges actual storage and refunds excess |
| `nft_transfer(receiver_id, token_id, approval_id, memo)` | Token owner or approved account may transfer; exactly one yoctoNEAR required; approvals cleared by the standard |
| `nft_transfer_call(receiver_id, token_id, approval_id, memo, msg)` | Transfer plus receiver callback and resolution; initial transfer is not proof of terminal ownership |
| `nft_resolve_transfer(previous_owner_id, receiver_id, token_id, approved_account_ids)` | Private callback delegates return/keep resolution to the standard |
| `nft_approve(token_id, account_id, msg)` | Token-owner-only approval, storage deposit, optional standard `nft_on_approve` callback to the approved account |
| `nft_revoke(token_id, account_id)` / `nft_revoke_all(token_id)` | Token-owner-only revocation with exactly one yoctoNEAR |
| `nft_token`, enumeration, `nft_is_approved`, `nft_metadata`, `contract_owner_id` | Token/approval/collection view methods |

`nft_mint` at line 57 has no minter check. The SDK's `internal_mint` delegates to
`internal_mint_with_refund`, which explicitly does not check the caller against
the contract owner. The constructor's `owner_id` does not make minting owner-only.
Anyone funding storage can choose an unused token ID, recipient and metadata.
Confirm whether that policy is intended; a curated collection needs an explicit
authorization design. The contract does not validate media against Supabase's
Storage prefix or verify the bytes of a supplied media hash.

There are **no** `list_nft`, `buy_nft`, sale lookup/cancellation, marketplace
receiver, royalty `nft_payout`/`nft_transfer_payout`, seller payout or buyer refund
methods here. A marketplace requiring royalty-payout methods is not compatible
with this NFT interface as written. Sending a price through `nft_transfer` is
also not a sale: that method requires exactly one yoctoNEAR and has no settlement.

## Options and required marketplace interface

These are design options, not discovered contracts or verified addresses:

| Option | Work required before integration |
| --- | --- |
| Adopt an existing reviewed marketplace | Obtain its actual source, ABI, supported NFT standards, deployment/code-hash evidence and settlement semantics; adapt to those methods rather than frontend assumptions |
| Build an approval-based marketplace | Owner retains the NFT until purchase; authenticate the standard approval callback's NFT predecessor, owner and approval ID; handle revoked/stale approvals, transfers, cancellation, failed purchase and refunds |
| Build an escrow-based marketplace | Authenticate the standard transfer receiver callback and custody; handle keep/return resolution, seller-authorized cancellation, NFT return, purchase transfer and settlement callbacks |
| Keep catalog-only operation | Retain all trading gates; no marketplace deployment is needed for catalog browsing |

Any chosen implementation needs the following operations and evidence. Method
names and wire arguments must come from that implementation's ABI:

| Operation | Required interface/behavior |
| --- | --- |
| Publish a sale | Authenticated seller and NFT identity; exact positive yoctoNEAR price; verifiable escrow or approval; documented storage/gas costs; opaque sale ID with a distinguishable sale generation |
| Read a sale | Seller, NFT contract/token, sale identity/generation, currency, exact price, active/pending/terminal state and authorization/custody evidence; namespace the result by independently configured network and marketplace |
| Purchase | Exact sale identity and live price; buyer authorization and attached deposit checks; single-winner concurrency; a terminal outcome covering NFT transfer and economic settlement |
| Cancel | Seller-only authorization, no cancellation of another seller's sale or a completed purchase, correct escrow/approval treatment, verifiable terminal state |
| Complete or fail settlement | Private callbacks, final receipt/return-value evidence, payout or credited-balance semantics, refunds on failed transfer, retry/replay handling and attributable events/state |

Current database constraints support NEAR only, u128-bounded yoctoNEAR prices,
opaque sale/token IDs of at most 1024 bytes, and one purchase per transaction.
FT payments, royalties requiring a different NFT interface, or batch purchases
need a reviewed integration/schema design. Preserve the existing restrictions
while choosing that design.

## Exact integration gaps

| Location | Present behavior | Minimal required replacement before opening gates |
| --- | --- | --- |
| [`services/nftService.js`](../frontend/Nft/src/services/nftService.js), `createNFT` | Creates an unminted database catalog entry | If minting is offered, submit the actual NFT method with appropriate storage funding and attach network/contract/token identity only after trusted final verification; bind catalog creator/media to the chosen mint policy |
| [`lib/near/contract.js`](../frontend/Nft/src/lib/near/contract.js), `listNFTForSale` | Assumes `list_nft({nft_contract_id, token_id, price})` and 0.01 NEAR storage funding | Replace with the chosen contract's actual publication/authorization ABI; current helper calls do not prove those methods exist |
| [`services/marketService.js`](../frontend/Nft/src/services/marketService.js), `createListing` / `cancelListing` | Database-only insert/update; existing browser grants/RLS deny them | Verify final publication/cancellation on chain and let a trusted writer update database state; preserve the read APIs and UI design |
| [`pages/nftDetails.jsx`](../frontend/Nft/src/pages/nftDetails.jsx), `handleBuy` | Dormant path passes database `listing.id` to `buyNFT`, which assumes `buy_nft({sale_id})` | Resolve and validate `contract_sale_id`, network, marketplace, NFT/token, seller, buyer and live price before the wallet request; do not use a database UUID as an on-chain sale ID |
| Same purchase path and contract helper | Awaiting a wallet response is followed by “Transaction sent”; no final outcome verifier exists | Handle rejection, returned `Failure`, redirects/undefined responses and pending receipts; show completion only after trusted terminal settlement |
| Backend | Wallet-auth Edge Function and SQL settlement sink only | Add a trusted verifier/indexer for the selected contract; no client-supplied facts or fabricated hashes may substitute for verification |

The frontend's fabricated database purchase fallback is already removed. Current
gates prevent the dormant purchase/listing/cancellation paths from running. No
frontend redesign is needed to keep these controls safely unavailable.

## Supabase and session review

[`20261029000100_near_marketplace.sql`](../supabase/migrations/20261029000100_near_marketplace.sql)
requires verified session UUIDs for protected user writes. Listing/ownership
writes are backend-only. Active/sold rows require confirmed chain identity;
`reconcile_nft_purchase` compares network, marketplace, sale, NFT/token, buyer,
seller and exact price, updates listing/ownership atomically and prevents reuse
of a network/transaction pair. Execution is service-role-only.

These are database consistency checks, **not transaction verification**. The RPC
cannot determine whether a hash exists, a transfer callback reverted, a seller
was paid, or an NFT ended up with the buyer. The worker must derive those facts
from final execution, all relevant receipts/return values and the selected
protocol's terminal state. Escrow custody must be distinguished from beneficial
seller ownership. Same-transaction batches and reused sale IDs need explicit
handling rather than assumptions about UUIDs or event names.

[`near-wallet-auth/handler.mjs`](../supabase/functions/near-wallet-auth/handler.mjs)
checks NEP-413 signatures and **final access-key state**, not transaction finality.
It authenticates the JWT with Supabase before checking user/session IDs. The
second wallet proof attaches verification to the actual Auth session.

Wallet switching in [`auth.js`](../frontend/Nft/src/services/auth.js) cancels older
attempts, serializes login, and requires successful revocation of A before
establishing B. [`walletContext.jsx`](../frontend/Nft/src/context/walletContext.jsx)
clears identity on account/session change or expiry. The
[wallet-auth migration](../supabase/migrations/20261029000200_near_wallet_auth.sql)
serializes attachment/revocation on the Auth session row and removes associated
challenges and grants on logout. Offline revocation can remain unconfirmed;
grants expire within one hour, and key revocation is not checked on every write.
Real Auth/RLS concurrency behavior remains untested.

A future purchase verifier must attribute an already broadcast transaction to
its actual signer/buyer even if the browser switches wallets. Switching must
stop stale UI updates/new requests; it cannot undo an accepted chain transaction.
Keep service-role credentials exclusively in trusted server code. Trusted ingress
IP limiting also remains unresolved; see the exact
[authentication deployment requirements](../supabase/NEAR_WALLET_AUTH.md#still-required-before-live-use).

## Image cleanup

The marketplace migration binds catalog images to the configured public Storage
prefix, canonical user/object path and owned Storage row. It permits public reads
and verified append-only uploads, with no browser UPDATE/DELETE policy. NFT
deletion additionally requires creator/current ownership, unminted user source
and no listing history, including cancelled listings.

The UI now retains artwork after deletion or failed/ambiguous catalog creation.
The legacy `deleteNFTImage` helper remains unsupported and is not called by the
UI. A trusted cleanup job is absent. Any future job needs retention and fresh
database/chain reference checks, including inserts whose response was lost and
on-chain media references that outlive catalog records. The permissionless NFT
mint method makes “no database reference” alone insufficient proof of an orphan.

## Tests run and tests still needed

Existing tests rerun during this review, with no test-source modifications:

| Command from repository root | Result |
| --- | --- |
| `node tests/marketplace-safety.test.mjs` | 7 passed |
| `node tests/security-migrations.test.mjs` | 8 passed; SQL source checks only |
| `node --experimental-vm-modules tests/marketplace-consistency.test.mjs` | 14 passed |
| `node --experimental-vm-modules tests/near-price-precision.test.mjs` | 10 passed |
| `node --experimental-vm-modules tests/near-wallet-auth.test.mjs` | 21 passed |
| `cargo test --locked --offline --manifest-path contracts/nft/Cargo.toml` | 9 passed; SDK mocked VM |

Additional acceptance tests required before trading:

- Explicit non-owner minting/recipient tests to lock the selected mint policy;
  insufficient storage, oversized metadata, unauthorized approval/revocation,
  stale approval IDs, and private resolver rejection. Exercise receiver success,
  rejection, panic and transfer rollback; the current transfer-call test covers
  initiation only. Check media/royalty compatibility with the chosen marketplace.
- Contract/adapter tests against the real ABI: wrong network/contract/sale/token,
  stale quote, revoked approval, forged receiver callbacks, unauthorized cancel,
  simultaneous buyers and cancel-versus-buy races, transfer failure, payout/refund
  failure, repeated events, reused sale IDs and duplicate settlement.
- Disposable Supabase integration tests for grants, session expiry/revocation,
  zero-row deletes, listing history, Storage prefix/ownership rules, exact prices,
  atomic settlement rollback/replay and concurrent challenge issuance/logout/
  attachment. Verify the gateway limiter on every ingress without trusting headers.
- Real Wallet Selector/provider tests for rejected signatures, redirects, Auth
  events, A-to-B-to-A switches and delayed responses/transactions. The current
  JavaScript suites mock HTTP/wallets and do not exercise these live boundaries.

This review does not establish account existence, a deployed code hash, a live
marketplace ABI, successful payment, live policy enforcement or live revocation.
