# Persistent NEAR runtime adapter — design specification

Status: design only, reviewed against the current working tree and
[architecture plan](../../MARKETPLACE_ARCHITECTURE_PLAN.md), especially sections
4–7 and 9–12. No transaction ABI, wallet integration, recovery, deployment or
activation is authorized here. Native NEAR, fixed prices, our individually verified
NFT only, no royalties; frontend/NEP-413 unchanged and both trading gates false.

Current foundations supply bounded record access, maintained payment totals,
deficit-repair accounting and disabled public actions. They are an offline `rlib`,
not persistent or authenticated contract code. The NFT has permanent IDs and measured
approval storage charges, but lacks the approval-only sale wrapper and immutable
marketplace binding. Historical implementation observations in the architecture
plan must not be mistaken for current deployment evidence.

## 1. Runtime authority and evidence boundaries

The adapter constructs context from NEAR host functions, never from request fields,
NEP-413 sessions, Supabase records or supplied `BalanceObservation`/`CallbackResult`.
Use host current/predecessor/signer IDs, attached deposit, block timestamp, gas and
storage observations; callers cannot supply authorization time or accounting facts.

| Operation | Required source of authority |
| --- | --- |
| Approval hook | `env::predecessor_account_id() == pinned_nft`. The hook's owner/approval fields remain subject to NFT-state verification. A root signer claiming to be the NFT is insufficient. |
| Seller cancellation | Predecessor equals the stored seller; exactly one yoctoNEAR; only the identified unlocked generation. Cancellation does not revoke NFT approval. |
| Purchase | Buyer and NFT recipient are the paying predecessor; `env::attached_deposit()` equals stored price exactly; buyer differs from seller/marketplace. Signer identity cannot redirect the buyer, refund or proceeds. |
| Payment dispatch | Keeper/claimant funds execution gas, but recipient/amount come only from the stored claim. No caller-selected transfer payload. |
| Callback | Private method, predecessor equals current marketplace, expected promise-result count/order, and matching persisted operation/source/generation/attempt. Ignore signer identity for callback authority. |
| Administration | Only separately approved, purpose-specific predecessor roles. No implicit owner/signer superuser or generic external-call authority. |

Record `signer_account_id` separately for transaction attribution; never substitute
it for predecessor. Direct-wallet versus contract-account payer admission must be
explicit before public methods exist; do not silently add a signer-equals-predecessor
rule. After bootstrap, both NFT and marketplace need immutable reviewed code and
empty transaction-signing access-key lists, including function-call keys. Otherwise
an account key can bypass contract-controlled custody restrictions.

**Two distinct evidence paths:** an on-chain callback reads the host result of its
own scheduled dependency on the executing chain branch. It does not fetch RPC,
prove a receipt ID or independently establish finality. The external verifier
establishes finalized history for projections and operations monitoring. No mock,
caller-provided JSON, hash, event text or current-owner view proves historical
receipt authenticity. Supabase never instructs settlement or changes a claim.

## 2. Exact approval and NFT compatibility

An authenticated fresh hook creates a funded `VERIFYING` generation and consumes
its exact approval identity only after validating bounded/versioned terms, price,
expiry, approved fee-policy snapshot and prepaid storage. Schedule only the pinned NFT's `nft_token` and
`nft_is_approved(token, marketplace, Some(exact_id))`; persist their expected result
order and verification attempt. Activate only for the same candidate, matching
token/seller and exact approval, before expiry. Missing/invalid verification retires
the candidate; late results cannot revive it. Views can race and are provisional.

Every eventual purchase must call only
`marketplace_transfer_approved(expected_seller_id, token_id, approval_id, receiver_id)`.
In one synchronous NFT receipt it must check one yoctoNEAR, configured marketplace
predecessor, actual owner equal to expected seller, seller/receiver exclusions, and
an active marketplace approval with exactly that ID **before any transfer**. It then
updates ownership/enumeration, clears approvals and emits the NFT event. Require a
reviewed bounded synchronous success ABI; never return a later promise as the sale
result. Pin the final ABI/encoding before implementation, including lossless u64
approval handling and tested public numeric bounds.

Marketplace custody or a direct deposit grants no sale authority. Ordinary
`nft_transfer` retains its SDK owner exception and is forbidden as a purchase or
retry fallback. No generic forwarding, arbitrary call/batch or administrative NFT
transfer exists. Unsolicited custody remains quarantined; this adapter scope grants
no deposit-return authority. Re-approval cannot replace an in-flight sale's approval;
a terminal generation requires a fresh hook/ID and a new generation.

## 3. Persisted operation context and callbacks

Generate operation commitments internally as
`SHA256(domain_tag || Borsh(OperationContextV1))`, with a fixed, distinct runtime
operation tag. Include schema, immutable artifact/deployment domain, manifest hash,
network/genesis commitment, NFT/market accounts, operation kind, source ID,
generation, attempt nonce, immutable source snapshot/fee-plan hash, exact target,
method/typed arguments, attached amount, gas budgets and expected callback ABI/count.
Encode bounded fields canonically; publish cross-language vectors. This is an
intent commitment, **not a protocol receipt hash or evidence of execution**.

Callbacks reload their source from storage and read bounded
`env::promise_result_checked` results in the recorded order. Only this internal
classifier may produce the foundation's result enum. A verification join has two
ordered dependencies; an approved transfer or native payment has one. Native
payments contain one `Transfer` action, not a function call or multi-action batch.

| Authenticated dependency result | Permitted economic transition |
| --- | --- |
| Exact admitted synchronous wrapper succeeds with its specified ABI | Held principal becomes the immutable seller/fee claims exactly once; retire generation and release sale lock. |
| That exact synchronous wrapper receipt fails | Full principal becomes one buyer-refund claim; no fee; retire generation and release sale lock. This rule cannot be applied to a generic returned promise that failed after an earlier transfer. |
| Wrapper success is oversized, malformed or ABI-inconsistent | Retain principal and lock in `Unknown/Review`; no refund, payout or fallback transfer. |
| Exact native Transfer succeeds | Matching in-flight/unknown claim becomes paid once. |
| Exact native Transfer fails | Matching claim becomes retryable debt; this does not establish that the protocol refund is already spendable. |
| Callback missing, panicked or exhausted gas | Persisted pending/unknown operation remains locked/reserved. No timeout resend or inferred failure. |
| Wrong caller/context/count, stale attempt or terminal replay | Reject or no-op without economic mutation; never touch a newer listing/sale. |

An unknown flag must not change the immutable attempt identity or prevent a genuine
delayed callback from resolving that attempt. Mutable notification versions are
separate from authorization context. Recovery adjudication, manual result injection
and operator settlement overrides are absent. Lost callbacks can therefore cause
indefinite lockup; this design makes no liveness or trustless-recovery guarantee.

The external verifier independently discovers and validates finalized receipt/action
graphs: approved network/genesis and executing code/account history; predecessor,
receiver, exact actions/arguments/deposits; execution status/return data; dependency
edges; authentic successful NFT/market logs; native payments and relevant refund
receipts. Follow returned-promise dependencies and account for every descendant
needed for the conclusion. Root transaction success is insufficient. Archive
receipt/block identifiers and finality evidence; reject incomplete, conflicting or
wrong-network evidence. Index idempotently by deployment, receipt and event/action
index, not merely transaction hash. Ownership and payment finality are separate
projections. Provider trust, independent-source agreement and closure schema must
be specified and audited; this verifier gets no on-chain outcome-writing authority.

## 4. Persistent records, bounded writes and atomicity

Use a versioned bounded root containing configuration/domain commitments, paused/
financial-hold flags, counters, maintained totals and collection handles. Store
token slots, generations, sales, claims, attempts, storage accounts, funding records
and reservations individually under disjoint versioned Borsh prefixes. Prefer the
pinned SDK's eager `collections::LookupMap` for explicit writes; any cached-store
alternative needs tested flush timing. Never serialize the existing whole
`Marketplace`/`Ledger` or reconstruct one with `new()` per invocation.
Initialization must reject existing state and missing required configuration; use
a reviewed atomic deploy/initialize bootstrap to prevent initializer capture and
start paused. Source-state migration is a separate authorized procedure.

Refactor the current concrete in-memory store behind a bounded repository/transition
interface. Load only the operation's records; validate immutable fields, current
state/attempt and checked counters; compute a bounded write set and totals delta.
Commit records, totals, locks and the corresponding promise construction in the
same NEAR receipt. Each sale creates at most two claims; begin with one payment
dispatch per invocation. Persist one current attempt plus a compact terminal
summary, not growing retry arrays. Retain paid/generation/approval tombstones.
No global history scan belongs in admission, callbacks, backing checks or withdrawals;
full reconciliation belongs offline, with bounded checkpointed repair/migration
design required separately.

Before outbound construction, reserve worst-case callback/claim storage and gas.
A panic or storage/gas failure must roll back this receipt's writes and newly
scheduled promises together; it cannot undo a previously executed NFT/payment
receipt. Failed purchase admission creates no sale/claim: follow the protocol's
attached-deposit refund, rather than adding a second application refund. A
non-panicking rejection must never leave accepted value unaccounted for. A separate
bounded health check records financial holds; a latch written inside a reverted
receipt did not persist.

Retry only a conclusively failed claim, with unchanged recipient/amount, fresh
checked nonce/commitment, cooldown and actual backing. Unknown attempts cannot
dispatch. Concurrent purchases, cancellation and retries serialize through the
same stored lock/attempt; conclusive settlement consumes its transition once.
In the eventual runtime, trading pause stops new listings/purchases but preserves
authentic callbacks, valid cancellation and adequately backed fixed claims. A
financial hold additionally blocks new outbound sends, while callbacks continue.
This distinction does not enable any currently disabled action.

## 5. Runtime balance and storage accounting

Construct `A` from host account balance, `S` from actual storage usage × byte cost,
and `Q/L/U/C` from maintained, disjoint adapter records as defined in architecture
section 6. Protect all other liabilities and reservations on every admission/send;
unknown debited sends remain in-flight but are not subtracted from `A` twice.

Define an explicit promise-materialization boundary. The locally inspected SDK
5.30.0/VM-runner 0.38.0 source includes attached deposit in host balance and deducts
outgoing value when a transfer action is constructed. Require explicit construction
of the complete promise/callback chain before the post-send balance check; do not
rely on deferred Rust destruction after accounting. Match the host debit to the
newly constructed actions, then mark `record_debit` once in the same receipt.
That marks cash committed, not payment success. Prove these semantics with the
pinned sandbox/protocol before accepting this implementation strategy; local
source inspection is not deployment/runtime verification.

Complete reservation consumption and storage-account accounting: move `C` into
materialized `S`, pending `Q` or recorded operating expenditure exactly once.
The foundation currently allocates reservations but cannot consume them; leaving
them counted after spending would fabricate a deficit. Pre-fund durable bytes,
including token/generation/claim tombstones and contribution records; charge actual
deltas within approved bounds without touching principal. Track SDK NFT refunds
separately. Unexpected value, rewards and unattributed refunds remain `U`; a balance
increase alone neither proves a failed payment nor grants operating capital.

For an eventual approved reserve-funding method, derive the contribution from the
actual attached deposit and recorded funding purpose, never supplied balance
snapshots. At a defined pre-write boundary, construct before/after snapshots with
identical protected partitions and exactly that credited delta; allocate disclosed
storage charges separately. Preserve deficit-repair versus new-capital accounting.
Top-ups do not release holds or enable trading. Underfunding rejects sends/admission
while authentic callbacks retain their normal outcome rules. Refund availability,
unknown incoming-value attribution and operating-spend conservation need runtime
tests before this accounting can be used with real assets.

## 6. NFT migration and deployment isolation

Follow [NFT MIGRATION.md](../nft/MIGRATION.md): pin exact source/target layouts and
code, freeze new mutations, resolve outstanding receipts, verify lifetime mint/burn/
approval history and existing registry keys, preserve exact IDs/counters/ownership,
fund retained and legacy approval storage, import in bounded committed batches,
then verify finalized state. Adding the currently absent marketplace binding may
change `RegistryV1` again: assign an explicit new target schema/codec, not an
unannounced append or live-token-only backfill. Planning enums/digests are not proof
of complete history. Missing evidence blocks migration and admission.

Bind root state and operation domains to separate testnet/production artifacts and
approved manifests; reject missing/mismatched modes and cross-domain imports.
The contract cannot discover/prove genesis from a configured string. Independent
deployment verification and isolated signing keys, accounts, RPC credentials,
databases/storage and signer processes remain mandatory. No test-session or
production activation path is introduced by this specification.

## 7. Missing prerequisites and implementation stop points

All items below are outstanding for a usable runtime adapter; existing model tests
do not complete them. Architecture decision IDs retain their original meanings.

| Prerequisite | Required evidence/decision and consequence |
| --- | --- |
| Bounded persistent core | Repository/transition refactor, exact root/prefix/schema codecs, atomic write sets, stored operation domains, migration of any offline fixtures, persistence/restart tests and checked maintained totals. No direct wrapping of current aggregates. |
| Admitted NFT | Approval-only wrapper, immutable binding, synchronous success ABI, no forwarding escape hatch, full source/code/key verification; adversarial custody/revocation/re-approval tests. Current NFT is incompatible with sale execution. |
| Callback/encoding contract | Exact operation Borsh/domain vectors, ABI/result bounds and lossless approval-ID rules; private callback/count/context tests and malformed/late/duplicate-result fixtures. Hashes supplied to today's model are not sufficient. |
| Runtime accounting | Measured debit/refund/flush timing, `S/Q/L/U/C` attribution, reservation-to-storage/expenditure transitions, contribution-storage funding and gas/storage headroom. C2 requires operator measurements and independent review. |
| Finality verifier | Pinned protocol/action/dependency schema, complete archive access, provider trust/independence policy, refund/receipt closure, idempotent projections and conflicting-evidence handling. C1/C3; no callback can substitute for this evidence service. |
| Fee policy | A3: fee rate/cap, treasury, governance/reviewer, delays/verification age and treasury acknowledgement/control evidence. No defaults or activation without approval. |
| Funding and incident authority | A2/A7: sponsor terms/payer allocation, margins, headroom, attempts/cooldown/response targets, financial-health review and hold-clearance/release authority. Stop before implementing these governed methods/configuration. |
| Issuance/admission policy | A4: mint mode, issuer/recipient consent, creator attribution and freshness bounds; explicit contract-account payer admission. Permanent ID non-reuse was subsequently implemented, but neither minting nor these policies were approved by that work. |
| Recovery and custody returns | A1/A2 remain unapproved. No quorum, return entitlement, rescue or outcome-override path in this adapter scope. Architecture requires approved and tested handling before a trading release. |
| NFT upgrade provenance | Complete historical evidence, exact bound-NFT target schema, funded/authorized bridge, receipt drain and sandbox migration rehearsal. No in-place legacy upgrade using the current initializer. |
| Deployment and operations | A6/A7: exact manifests, isolated test identities/limits/authorities, verified bootstrap/key removal, production release authority; stage-3A verifier tooling before testnet. No deployment authorized. |
| Acceptance and audit | Persistent Wasm sandbox: authentic predecessor versus signer, wrapper checks, real host callbacks, callback OOG, atomic rollback, double purchase, replay/restart, long-ID storage, paid/unknown/refunded conservation, deficit repairs, migration interruption and environment isolation. Independent audit, findings closure, archive/on-call readiness and A8 budgets remain required before real trading. |

This specification selects no financial values or governance identities. Technical
storage/encoding work can be planned independently, but governance-dependent runtime
implementation stops at the unresolved A1–A4/A6–A8 inputs above. No such code has
been implemented in this design task.

## 8. Next bounded handoff: storage and encoding internals

This slice needs no selected financial values or governance identities. It does
not authorize transaction entry points or turn supplied data into runtime evidence.
Implementing source code for this slice is a separate scope choice from completing
the design specification.

**Record layout.** Keep the bounded root under the SDK's root-state key. Give every
record family a disjoint, versioned prefix: token slots, listings, sales, principals,
claims, operation slots, storage accounts, operating reservations and contributions.
Encode suffixes with Borsh typed IDs or `TokenKey`, never joined user strings. Each
value envelope carries schema version, deployment-domain commitment, record kind,
record identity and checked revision. Persist the root's counters and maintained
totals with every affected record; never reset them during reopen/restart.

Readers must verify envelope/key identity, expected schema/domain, complete input
consumption, bounded payload lengths and record invariants before returning a value.
Reject corrupt/unknown encodings; do not substitute a default empty record. Optional
lookup absence is distinct from decoding failure. Derived Borsh deserialization
alone does not enforce `TokenKey`/fee constructor checks or conservation: validate
them again at the storage boundary. A missing root is acceptable only to the
separately authorized initializer, never to an ordinary operation.

**Repository boundary.** Expose owned point reads and explicit writes; persistent
`LookupMap` cannot safely emulate today's borrowed `get_mut` references. Refactor
the transition layer to consume only the bounded records required by its operation
and return proposed mutations. Keep an in-memory repository for differential tests;
the production interface must not expose whole-history iteration, clearing or
aggregate serialization. Future persistent code must share transition rules with
the existing model rather than duplicate settlement logic in callback handlers.

**Atomic write set.** Each internally generated set identifies the source domain,
operation/attempt, expected record revisions, required absent keys, resulting
records and totals delta. Validate all reads, bounds, conservation and counter
increments before applying any mutation. Reject duplicate mutation keys and
conflicting preconditions. The key/record count is bounded by the operation kind,
not a caller-sized vector. Commit all writes, root revision and any corresponding
typed promise construction in one receipt; reject stale or repeated sets. A write
set is never accepted from a public request, database worker or signed attestation.
Plain Rust error returns do not roll back earlier storage writes: the eventual
adapter must enforce host rollback on post-write failure and test it explicitly.

**Operation encoding.** Use a closed descriptor enum for listing verification,
approved NFT transfer and fixed-claim native payment. Derive targets, method names,
arguments and recipients from immutable stored source records. Do not store an
executable arbitrary method/arguments payload. Separate the execution-attempt nonce
from mutable liveness/notification revisions so flagging `Unknown` preserves the
valid delayed callback. Callback expectations bind the descriptor purpose, ordered
dependencies, source identity and ABI version. No decoder may promote these
descriptors or their hashes to proof that a receipt ran.

Pin the domain tag, enum discriminants, integer widths/endianness, field order and
length bounds in golden vectors before persisting this schema. Vectors must include
multiple domains and operation kinds, long/multibyte token IDs, u64/u128 boundary
values, and independent encoding/hash implementations. Actual account names, fee
policies and governance values in fixtures remain synthetic, not configuration.

| Acceptance gate | Required regression |
| --- | --- |
| Keys and decoding | Cross-family/domain collisions, mismatched key/value IDs, invalid constructors encoded directly, oversized/truncated/trailing bytes and unsupported schema all reject. |
| Restart | Reopen the same store; generations, consumed approvals, paid claims, unknown attempts, reservations and maintained totals survive without reinitialization. |
| Atomicity | Failure before commit changes nothing; injected failure after each write/promise-construction stage rolls the whole receipt back. No partial claim set or changed total survives. |
| Replay | Old revisions, duplicate mutation keys, consumed operation sets and delayed callbacks cannot overwrite a newer generation or debit/credit twice. |
| Bounded work | Each transition touches only its declared point-read/write set regardless of retained history; compare maintained totals with an independent test-only audit. |
| Encoding | Golden Borsh/SHA-256 vectors agree across independent implementations; changes to domain, purpose, source, attempt, recipient, amount or callback expectation change the commitment. |

Record codecs, an in-memory transactional harness and encoding tests can precede
runtime integration. Host storage/receipt atomicity remains unverified until the
later persistent sandbox stage. Stop before policy-bearing configuration, treasury
verification, funding/hold-clearance methods, recovery, NFT wrapper entry points,
deployment or activation; the prerequisites in section 7 still apply.
