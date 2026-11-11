# Marketplace foundations — offline only

This package implements deterministic state/accounting primitives from
[`MARKETPLACE_ARCHITECTURE_PLAN.md`](../../MARKETPLACE_ARCHITECTURE_PLAN.md).
It is an `rlib`, **not a deployable NEAR marketplace**. It exports no NEAR
transaction ABI, makes no promises, loads no credentials, and sends no money.
`authorize_public_action` rejects minting, deposits, listing, purchase,
withdrawal and recovery in every build. Neither Cargo environment feature
enables an action. The existing frontend and NEP-413 authentication are untouched;
`LISTING_ACTIONS_ENABLED` and `PURCHASES_ENABLED` remain literal `false`.

## Implemented behavior

- `state.rs`: one configured NFT identity; structured token keys; immutable
  listing/sale snapshots; non-reused generation/sale identifiers; approval
  high-water records; provisional owner/exact-approval verification; cancellation,
  invalidation and exclusive purchase locks. Unknown transfer outcomes retain
  principal and locks. Conclusive normal callback data allocates claims once
  before releasing the lock. Later listings cannot rewrite old sales/debts.
- `ledger.rs`: held principal, seller/fee claims on delivery, or a full buyer
  refund on failure. Overlapping seller/treasury recipients merge while retaining
  role allocations; zero fees create no empty claim. Payments have fixed
  recipients/amounts, monotonic attempts, cooldown checks and distinct available,
  in-flight, unknown, retryable and paid states. Unknown attempts cannot resend.
  Duplicate/stale results reject without changing records.
- Principal, attributed operating capital, operating reservations, storage stake,
  storage credits, queued debits and unattributed value remain separate. The
  reserve calculator implements `p * B(N) + N * 1 yocto + M` and checks overflow.
  Admission calculations enforce explicit hard-floor/target inputs. Reserve
  allocation cannot use principal or unidentified surplus. There is no reserve
  spending, release, withdrawal, deposit-admission or return method.
- Backing checks protect every locally payable liability and pending debit.
  `check_health` separately latches a financial hold; there is no clear/resume
  method. Already-issued outcome recording remains possible during a hold.
- Each identified contribution binds before/after balance snapshots with unchanged
  protected partitions and exactly the credited amount of new cash. It first repairs
  the pre-existing backing deficit, including storage/reserved-capital shortfalls;
  only the excess increases free capital. Partial repairs are accepted and recorded.
  No debt is reduced and no financial hold is cleared by a contribution.
- Fixed-size `LedgerTotals` maintain held/available/in-flight/paid principal, queued
  debits, operating reservations and contribution/repair totals. Backing checks use
  constant-size arithmetic without historical scans. Each sale has at most two
  claims. The offline record store exposes point access only; its historical
  iterator exists solely in unit tests for an independent accounting audit.
- `isolation.rs`: mutually exclusive testnet/production compilation; absent mode
  rejects deployment identity checks. Exact network/genesis/account/code/ABI
  comparisons and complete resource-category inventories detect mismatches and
  shared production resources. Test activation does not exist in either build.
- `persistence/`: v1 record codecs, typed intent commitments, a bounded repository
  planner using the existing transition rules, and an atomic byte-store harness.
  Reopen restores counters/totals and individual records; stale prepared batches
  cannot overwrite newer state. This is the bounded storage/encoding milestone
  from [RUNTIME_ADAPTER_SPEC.md](RUNTIME_ADAPTER_SPEC.md), not a NEAR host adapter.

The companion changes in `../nft/src/{lib,tests}.rs` add a permanent exact-ID
registry independent of live ownership, a live/burned status view, duplicate
protection and mint storage charging that includes retained ID bytes. Public
`nft_mint` unconditionally rejects while mint policy is unapproved. The mint
primitive is private Rust code, not a callable alternate entry point. No burn
entry point was added. Burn fixtures remove live data and demonstrate that the
registry and approval high-water mark survive; they are not a burn implementation.
Approval writes now charge their measured storage delta, including full long-ID
keys, map overhead and the first retained counter. Re-approval charges only new
bytes. Revocation refunds measured released map bytes and retains counter/registry
backing. Existing SDK transfer/transfer-call refunds remain SDK behavior and need
runtime accounting tests before release. The [NFT migration design](../nft/MIGRATION.md)
and offline planner reject live-only historical inventories; no migration entry
point or deployment has been added.

## Boundaries that must not be mistaken for completed contract security

These APIs model transitions for local tests. They do **not** authenticate callers,
receipts, deposits, block times, fee-policy approvals or balance observations.
The future contract adapter must authenticate and validate every input; exposing
these methods directly as NEAR methods is unsafe. `CallbackResult` is data, not
proof or an operator attestation. The original model accepts supplied, nonzero
operation hashes; the new repository computes typed commitments internally and
checks them against stored source records. Binding those intents to actual
wrapper/payment actions still belongs to the runtime adapter. Borsh derives support local
record round trips; they are not a finalized external ABI or a safe decoder for
untrusted state imports.

The original model retains records in an in-memory point-access store. `Marketplace` and
`Ledger` deliberately do not implement Borsh serialization: a future contract
cannot silently serialize all history as its root state. Record/totals encoding
remains available. The new repository loads and commits only bounded point records
against a byte backend. Compile-fail tests enforce the aggregate-encoding
restriction. The included backend remains offline, not measured on-chain persistence.
It conservatively rejects aggregate historical principal beyond `u128::MAX`.
Purchase recording models principal acceptance and wrapper scheduling together;
it does not implement pre-send gas/storage/reserve admission. Reservation inputs
and fee snapshots are checked calculations, not a governance/configuration system.
Tests use synthetic accounts, amounts and delays; none selects production policy.

Before any contract adapter can be deployed, remaining work includes:

- The mandatory NFT approval-only transfer wrapper and immutable marketplace
  binding. Existing SDK owner transfers still skip approval-ID checks. No purchase
  may fall back to ordinary `nft_transfer` or a generic forwarding method.
- Explicitly approved mint mode, issuer/recipient/creator rules and bounded
  metadata/storage behavior. NFT state layout now includes `ever_minted`; it is
  incompatible with previous deployed state. There is no migration. Historical
  deployments/imports require complete minted-ID history including burns and a
  reviewed migration; never redeploy/reset state to work around that requirement.
  Follow `../nft/MIGRATION.md`, including legacy approval-storage funding and receipt
  closure. The pure preflight helpers cannot prove real history completeness.
- A NEAR host backend for the bounded records, prepaid callback/claim storage, wrapper ABI/result
  validation, private callback/result-count checks, exact payer/deposit checks,
  approved fee/treasury governance and actual chain/code/key verification.
- A pinned-runtime adapter that accounts for `A/S/Q/L/U/C` without double counting.
  `record_debit` must run exactly when runtime balance already excludes that
  commitment, never on a balance-change guess. Failed sends restore debt; an
  expected runtime refund is not cash. Incoming contributions must be identified,
  approved and actually credited before `record_operating_credit`; the accounting
  API now separates deficit repair from new free capital, but does not authorize
  funding terms, authenticate balance evidence or provide a hold-clearance process.
- Measured byte/gas/attempt bounds and approved margins, headroom, sponsor terms,
  cooldowns and hold/resume authority. No reservation-consumption, replenishment
  service, insolvency distribution or debt forgiveness is implemented.
- Independent inventory discovery (including provider aliases), trusted genesis
  observations, physical credential/process isolation, release manifests and
  bounded test sessions. Matching supplied fingerprints is not evidence of actual
  deployment identity or complete operational isolation.

Recovery, quorum voting, evidence adjudication, deposit provenance/return
authorization, treasury replacement and activation remain unimplemented. They
require the outstanding human/operator decisions in the plan. No recovery model,
financial values, governance identities or production parameters were selected.
Without a conclusive normal callback, an unknown operation can remain locked
indefinitely; this foundation contains no administrative escape hatch.

## Storage/encoding milestone and compatibility

`persistence::Repository` executes explicit **offline** commands against an owned
view of only the necessary records, with the persisted counters and maintained
totals restored. It calls the original state/ledger transitions, validates linked
sale/principal/claim/operation records, independently checks the changed principal
buckets, and emits an opaque `Prepared` batch. `OfflineCommand` has no wire codec;
never expose it as a contract/request API or treat its result/balance fields as
authenticated observations. Root state stays paused; this harness models actions
without granting permission to perform them on NEAR.

The new encoding is additive. Existing listing/sale/claim/token-slot/principal
Borsh payload field order and existing public model APIs are unchanged. Only
crate-internal visibility was expanded to share the rules with the bounded view.
There was no deployed marketplace storage schema to upgrade. `open` rejects
missing/corrupt/legacy root bytes and wrong domains; it never initializes, resets,
or guesses another codec. `new_offline` creates a fresh in-memory fixture only.
Existing NFT state and its separate migration requirements are unchanged.

| Component | V1 encoding/invariant |
| --- | --- |
| Root key | Exact SDK key `STATE`; an envelope binds it to the expected deployment commitment. |
| Other keys | `nmp:r:` + little-endian u16 schema 1 + 32-byte domain commitment + Borsh `RecordKey`. Tags 1–10 cover token, listing, sale, principal, claim, verification, transfer, payment, reservation and contribution. Tag 11 is reserved and rejected; storage-account semantics are still outstanding. |
| Envelope | `NMPREC01`, u16 schema 1, domain commitment, typed key identity, nonzero u64 revision, tagged value, then SHA-256 of `near-marketplace:record:v1\0` + preceding bytes. The checksum detects corruption; it is not authenticated evidence against a malicious state supplier. |
| Decode | Maximum 16 KiB per envelope, allocation-free length preflight, AccountId/token/fee/state/conservation validation, exact input consumption. Token IDs retain exact UTF-8 bytes, bounded at the existing 1,024 bytes. Principal claim vectors have at most two entries. |
| Operations | SHA-256 of `near-marketplace:operation:v1\0` + Borsh context: schema, deployment, closed descriptor, attempt nonce and explicit budgets/ABI commitments. Integers are little endian; strings use u32 byte lengths. Verification/approved-transfer/native-payment tags are 0/1/2. Source fields, target, attached value and dependency order are fixed by the descriptor. Mutable unknown/terminal progress is outside the attempt commitment. |
| Atomic batches | Expected full record bytes/revisions and required absence checks; no external builder or deserializer. Operation-specific read/write caps are 6/4/8/12/16 records, including root. Every mutation advances root revision; changed records advance their own revisions with checked overflow. No deletion or history scan. |
| Accounting | Changed held/available/in-flight/paid/queued buckets reconcile with touched records; root retains principal conservation and `free + reserved = contributions - repairs`. This slice introduces no operating spend, reserve consumption or storage-credit transition. |

Budgets and ABI hashes are required explicit inputs, not selected deployment
parameters. The approval-transfer descriptor names the existing specified intent;
there is no invented NFT JSON/result ABI, promise constructor or ordinary owner
transfer fallback. Six golden vectors cover all three operation kinds in both
domains, 1,024-byte multibyte IDs and u64/u128 boundaries, checked independently in
Rust and Python. Review and freeze schema tags/limits before any actual storage
deployment; changes after deployment require a versioned migration.

`AtomicStore` is a trusted backend contract. Its implementation must compare all
preconditions and apply all writes atomically. `MemoryStore` uses bounded preimages
to roll back injected errors/panics after every write. Tests reopen serialized
backing fixtures and check replay, stale attempts, corruption, conflicting writes,
orphan claims and recipient substitution. This does **not** establish disk crash
durability, NEAR storage staking, promise atomicity, callback authentication, gas
sufficiency, receipt finality or protection from malicious backend replacement.
The 16 KiB envelope/checksum overhead must be measured and funded in the host
adapter. There is no NEAR host backend, fixture importer, storage-account ledger,
funding/hold-clearance path or governance implementation in this milestone.

Before runtime integration, human/operator review is still required for the
schema and bounded-access strategy, trusted backend implementation, exact admitted
NFT ABI/binding and migration evidence, measured storage/gas budgets, deployment
manifests, and the architecture's unapproved fee/funding/mint/recovery/release
policies. No defaults or approvals are inferred from synthetic tests. Mainnet
release still requires authenticated host integration, sandbox receipt tests,
independent audit and the remaining architecture release gates.

## Local verification

Run from the repository root using cached dependencies:

```sh
cargo test --locked --offline --manifest-path contracts/nft/Cargo.toml
cargo test --locked --offline --manifest-path contracts/marketplace/Cargo.toml
cargo test --locked --offline --manifest-path contracts/marketplace/Cargo.toml --features testnet
cargo test --locked --offline --manifest-path contracts/marketplace/Cargo.toml --features production
cargo clippy --locked --offline --manifest-path contracts/marketplace/Cargo.toml --all-targets -- -D warnings
python3 contracts/marketplace/tests/encoding_vectors.py
```

This command must fail with the explicit environment-conflict compile error:

```sh
cargo check --locked --offline --manifest-path contracts/marketplace/Cargo.toml --features testnet,production
```

Tests cover principal conservation, duplicate/out-of-order callbacks, unknown
outcomes, partial payment failure, returned-liquidity/cooldown constraints,
purchase/cancellation races, stale generations/approvals, custody rejection at
listing verification, counter/amount overflow, reserve isolation/exhaustion,
financial holds, missing configuration, environment mismatch and resource overlap.
An independent test audit recomputes maintained totals after every payment transition;
a 2,000-sale regression exercises retained history without backing scans. Contribution
tests cover exact/partial/surplus deficit repair, replay, overflow, reserved capital
and attempts to reclassify protected cash. NFT tests inspect measured long-ID
approval charges, revocation refunds, retained counter/registry bytes, counter
exhaustion and migration layout/history checks.
The NEAR mock runtime does not roll back writes on caught panics; atomic receipt
rollback still needs sandbox tests. No sandbox/Wasm/deployment/transaction test,
recovery drill, independent audit or live environment verification is claimed.
