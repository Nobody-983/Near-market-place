# Authenticated NFT runtime integration — controlled admission dispatch

Implemented candidate code; public listing intake and purchases remain disabled.
The frontend gates remain false and `authorize_public_action(List/Purchase)` still
rejects unconditionally. Owner-created admissions can now be explicitly dispatched
into the pinned NFT verification flow by the immutable admission owner. No
deployment, funding, recovery, financial-policy installer, purchase/payment
dispatch or NFT change is included. Earlier specifications that describe the
runtime as storage-only predate this milestone.

## Actual receiverless ABI

| Method | JSON arguments | Return / behavior |
| --- | --- | --- |
| `bootstrap_with_admission_owner` | `manifest: Base64VecU8`, `admission_owner_id: AccountId` | Fresh direct self-bootstrap only; installs an immutable admission-only owner. Existing `bootstrap(manifest)` remains compatible and installs no owner. |
| `create_nft_admission` | `operation: Base64VecU8`, `storage_owner_id: AccountId`, `reserve_amount: U128 string`, `creation_bytes: U64 string`, `local_gas: U64 string` | Owner predecessor, zero deposit and zero promise results. Returns `{"generation":"…","attempt_nonce":"1","state":"ready"}` or the retained status for an exact retry. No promises. |
| `dispatch_nft_admission` | `admission_id: Base64VecU8`, exactly 32 decoded bytes | Owner predecessor, zero deposit/results. Selects an existing managed admission only. Returns `{"generation":"…","attempt_nonce":"1","state":"dispatched"}`; a repeated pending request returns the same status without writes or promises. |
| `release_nft_admission` | `token_id: string`, `owner_id: AccountId` (expected NFT seller), `approval_id: U64 string`, `generation: U64 string` | Same owner authorization. Returns status `"released"` only for an unused admission; reclassifies remaining credit internally, with no transfer. |
| `nft_on_approve` | `token_id: string`, `owner_id: AccountId`, `approval_id: u64 number`, `msg: string` | Authenticates the immutable NFT predecessor, then **aborts while the listing gate is closed**. The implemented internal admission path returns `{"generation":"…","attempt_nonce":"1"}` after persisting and scheduling verification. |
| `on_nft_verified` | `generation: U64 decimal string`, `attempt_nonce: U64 decimal string` | Private, zero deposit; returns JSON `"active"`, `"invalid"`, or `"ignored"`. Authentication/context/count/storage errors abort. |

All methods are receiverless: SDK marker serialization never overwrites `STATE`.
They reload the immutable bootstrap manifest and validate the existing v1 root,
artifact environment, marketplace account and host executing code hash. NFT identity
comes from that manifest, never hook arguments. Signer identity confers no authority.
No method accepts receipt results as evidence. Canonical operation bytes include
commitments to intent and proposed policy; they are not proof of NFT ownership,
approval, approved fee governance or any receipt. SDK-ignored extra JSON fields
cannot replace host observations.

`msg` is at most 4,096 UTF-8 bytes and has exactly these required fields:

```json
{"version":1,"price":"1000","expires_at_ns":"1000000","policy_version":"7"}
```

Numbers here illustrate encoding, not approved prices, fees or policy values.
Reject unknown/duplicate/missing fields, zero price/policy version, wrong version,
expired terms, invalid accounts and token IDs outside the existing 1,024-byte bound.
Token, seller, price, expiration, exact approval and policy version must match the
stored admission; caller-supplied fee, treasury, payer or refund fields are rejected.
SDK argument deserialization precedes method checks and remains subject to protocol
input/gas limits; this is not a pre-deserialization size guard.

## Owner authorization and lifecycle

Fresh bootstrap can explicitly name one external marketplace admission owner,
distinct from the marketplace and NFT accounts. The role is immutable: no setter,
rotation, upgrade bypass, treasury/governance, recovery or trading-activation authority is
added. Bootstrap requires predecessor and signer equal to the current marketplace,
zero deposit/results and absence of all existing roots/sentinels, including the
owner sidecar. Its measured storage must be backed by bootstrap funding. Ordinary
creation/dispatch/release checks **predecessor**, never signer identity. The owner may be a
separately controlled account/contract; control and custody require human review.

Creation's canonical v1 verification operation must decode strictly, bind the
immutable domain/NFT/ABIs and exactly match a freshly prepared listing generation,
nonce one, seller, token, approval and terms. `storage_owner_id` must equal the
authenticated admission owner. The request explicitly consents to spending only
that owner's already recorded prepaid credits for this exact operation. It cannot
charge a seller or another contributor. This narrow owner-funded staging mechanism
is not a general seller-pays/sponsor-pays policy. No public funding API exists;
unexplained account balance is never adopted as a credit. Fresh bootstrap without
prepaid credits therefore cannot create an admission.

The seller and approval in this administrative intent are **unverified claims**.
The owner-controlled dispatch obtains observations through actual NEAR promise
dependencies; no owner assertion can supply verification success. The public NFT
hook remains disabled. The owner can consume its
credits and occupy a token's pending slot; it cannot sell the token, supply a
successful view result, bypass seller approval, or change fee governance. Before
public intake is enabled, approved fee/treasury/seller-term authorization and
funding attribution still need their separate installer and review.

Creation allocates the generation immediately: existing root, token slot, listing
`VERIFYING`, and canonical pending operation are committed with `NMPADM01` plus
an additive fixed-size `NMPALIF1` lifecycle record. The lifecycle disambiguates a
prepared, **unsent** operation from an outstanding callback. The original admission
encoding is unchanged. Lifecycle keys bind the exact operation-derived reservation;
checksummed records bind operation, original amount and state. A missing lifecycle
with an existing admission guard fails closed instead of falling back to legacy.

| Lifecycle | Storage consumption cursor | Permitted next step |
| --- | --- | --- |
| `ready` | 1: measured creation charge | Owner dispatch, or owner unused release |
| `dispatched` | 2: zero-growth dispatch update | Matching private callback; repeated owner dispatch is a read-only status check, never another send or release |
| `completed` | 3: zero-growth callback update | Retained; active/invalid listing state remains authoritative; no release in this milestone |
| `released` | Historical cursor 1, guard permanently released | No new use; exact create/release retries return this retained status |

Different tokens allocate distinct global generations in receipt execution order.
An intent prepared against an older root generation is rejected and must be
recomputed explicitly; the contract never edits the caller's commitment. A live
candidate blocks a competing admission for the same token. Release cancels the
candidate and terminalizes its verification, retaining admission replay history.
A replacement requires a fresh higher approval ID and next generation. Exact
repeated creation (including amount, budgets and all intent bytes) is a no-op;
changed duplicates fail. Released keys, reservation IDs and generations are never
reused. Delayed callbacks cannot affect a later generation. Only the separately
owner-authorized dispatch method can start managed verification while public
approval intake remains disabled.

## Exact dispatch locator and authorization

Call `dispatch_nft_admission({"admission_id":"<base64 of 32 bytes>"})`, with zero
attached NEAR. The locator is the existing admission key's final 32 bytes:

```text
SHA256("nmp:nft-admission:v1\0" || deployment.commitment()
       || Borsh((token_id: String, expected_seller: AccountId, approval_id: u64)))
```

The tag ends in a zero byte; Borsh uses length-prefixed UTF-8 strings and a
little-endian u64. `deployment.commitment()` is the existing domain commitment,
not a caller-chosen network name. Derive this identifier from the original
owner-authorized creation intent. No new index or encoding is required, and the
creation ABI remains unchanged. Dispatch prepends the fixed admission namespace,
strictly decodes its record, and recomputes the whole key from persisted terms and
the runtime domain. The hash is **only a locator**, never a receipt proof or bearer
capability. Wrong length/domain, missing records, substitutions, or corruption fail.

Dispatch requires the immutable owner's predecessor and the same original storage
owner, zero deposit and zero dependency results. No seller/NFT/token/approval,
generation, attempt, call budget, or callback result is a dispatch argument.
SDK-ignored extra JSON fields cannot override any persisted data. Legacy admissions
without a managed lifecycle cannot be dispatched through this new entrypoint.

For `ready`, revalidate the exact pending operation/commitment/nonce, `VERIFYING`
listing, token-slot generation and consumed approval, absence of a sale lock,
reservation attribution and cursor 1, expiry, both financial holds, storage backing
and checked gas budget. Before sending, persist `dispatched` and consume cursor 2
through the existing metered storage boundary. It uses only preallocated records:
assert zero storage growth and preserve protected totals. There is no minimum new
credit policy: an otherwise backed reservation with zero remaining credit can
support this zero-growth transition. No funds or protocol refunds are reclassified.

An already `dispatched` request validates the same links and cursor and returns
its original status without writes, counter advancement, or another promise graph.
That observation remains permitted after expiry or during a hold because it cannot
create new risk or settle an unknown outcome. `completed`, `released`, stale,
expired-but-unsent, contradictory and legacy admissions reject new dispatch. If
release and dispatch contend for `ready`, receipt order decides: release first
permanently prevents dispatch; dispatch first permanently prevents unused release.
Generation and attempt never change on retry or restart.

## Reservation accounting and safe release proof

Reserve creation moves `available -> reserved` for that one owner; no liability or
free capital is created. Require checked `reserve_amount >= creation_bytes *
storage_byte_cost`, sufficient available credit and fully backed principal/claims,
operating capital, storage liabilities and unattributed balances. Holds reject new
admissions. The byte ceiling is an explicitly supplied bound, not a measured gas
or recommended funding policy.

The runtime measures host usage **before reservation allocation**, then includes
reservation/guard metadata, admission/lifecycle records and root/listing/operation/
token writes in one charge. Actual growth moves `reserved -> staked` exactly once
at cursor 1. Updating already allocated accounting records must keep their encoded
lengths unchanged. Exceeding the authorized byte ceiling, insufficient backing,
overflow, invalid records or write failures abort the receipt. Fixed-size dispatch
and callback updates advance cursors without charging the same bytes again.
Creation has at most 16 bounded host writes across three internal CAS batches and
two sidecars; release at most eight. The batches share one synchronous receipt:
there is no interleaving or ordinary error return after the first write. Atomic
rollback across those writes relies on NEAR receipt semantics and still needs a
node test. Storage lookups are bounded point reads, never historical scans.

The additive storage codec appends key/value discriminant 5 (`AdmissionGuard`),
containing the exact operation commitment and released flag. Existing discriminants,
`NMPSTG01` envelopes and reservation validation are unchanged. Reserve creates this
guard in advance; release never needs unbudgeted metadata. The immutable historical
reservation retains `remaining + staked == allocated`; after release its remaining
field is history, **not live reserved credit**. All live reservation getters and
consumption consult the guard and reject a released reservation. Reconciliation
must interpret both records together, not sum historical reservation amounts.
Old readers cannot process the new guard schema and must not be rolled back over it.

Release requires all of: authenticated owner matching original storage attribution;
exact token/seller/approval/generation; lifecycle `ready`; matching pending operation
and commitment; listing `VERIFYING` with no sale; current token slot with the same
generation/consumed approval and no sale lock; untouched dispatch cursor 1; matching
unreleased guard; and valid storage backing. In one receipt it cancels/terminalizes
the candidate, marks lifecycle and guard released, and moves only **remaining**
`reserved -> available` on the original owner's account. Occupied/tombstone stake
is retained permanently. Credits stay protected liabilities; this is no refund,
withdrawal, storage deletion, or free-capital attribution. Sticky holds, principal,
claims and unattributed balances remain unchanged. An existing sticky hold does
not forbid this liability-neutral release if current backing is sufficient; a
current deficit does forbid it, without clearing any holds.

Release is disabled for legacy admissions (unproven dispatch history), any
`dispatched` admission (including absent/aborted/malformed-count callbacks), unknown
sale/transfer/payment locks, and even `completed` admissions. Completion alone does
not prove that a future sale cannot need its remaining budget. No timeout, operator
assertion, caller hash, failed callback or presumed protocol refund substitutes for
closure. Such reservations remain locked until a separately reviewed lifecycle can
prove all consumers are closed. Loss of the immutable owner also leaves unused
reservations locked; there is no recovery/rotation workaround in this milestone.

## Promise graph and observation rules

```text
immutable owner -> dispatch_nft_admission(admission_id)
  (public nft_on_approve stays disabled)
  require owner-created READY generation, exact nonce=1 operation
  persist DISPATCHED and advance storage cursor 1 -> 2, reserve stays attributed
  +-- pinned NFT.nft_token(token_id), deposit=0 ------------------+
  +-- pinned NFT.nft_is_approved(token_id, marketplace, exact ID),|
      deposit=0 -----------------------------------------------+
      ordered Promise.and -> self.on_nft_verified(generation, nonce)
```

Verified against installed SDK 5.30.0: `Promise::function_call_weight`, `and`, `then`,
`detach`, `env::promise_results_count` and `env::promise_result_checked`. All call
names/targets/arguments are derived internally, with explicit gas and unused-gas
weight zero. Start checks checked total call/callback/local gas against remaining
prepaid gas. The complete zero-value promise graph is constructed before the final
backing check. No ordinary error returns after the first write; later failures
abort. A scheduling failure in that receipt must roll back its local transition
and scheduled actions under NEAR semantics; the mock does not establish that
rollback. A later lost/aborted callback does not roll back the successful dispatch.
These checks are not gas profiling or node rollback evidence.

The callback requires predecessor=current marketplace, exact stored generation,
nonce, descriptor/terms/commitment, current token-slot generation, admission and
reservation cursor. Managed callbacks additionally require `dispatched` lifecycle
and cursor 2; a fabricated callback for an unsent `ready` candidate is rejected.
A pending verification requires exactly two dependency results
in token-view / approval-view order. Read token output within the committed bound
(maximum 16 KiB), and bool output within five bytes, before allocating/parsing it.
Require matching token ID, seller, exact marketplace approval in the token map,
and true from `nft_is_approved`. Reject duplicate JSON fields/map keys, over 32
approval entries, malformed/missing fields and stringified standard numeric IDs.

| Observation | Transition |
| --- | --- |
| Both matching views, before expiry | Same `VERIFYING` generation becomes internal `ACTIVE`; operation terminal, lifecycle `completed`, cursor 3. This is provisional verification, not enabled public trading. |
| Exactly two host results, but a failed view, null/malformed/oversized result, mismatch, revoked/newer approval, false approval or expiry | Candidate becomes `INVALID`; operation terminal, lifecycle `completed`, cursor 3; no payment or refund. This means verification did not establish eligibility, not that NFT ownership/transfer failure was proven. |
| Missing callback, missing/extra dependency count, wrong context, unrecognized host result status, callback abort/OOG, or unavailable storage backing | Attempt remains `dispatched`/pending at cursor 2. Outcome is unresolved; no resend, release, refund, or timeout inference. A later authenticated callback may still complete that exact attempt. |
| Duplicate/terminal/stale callback | No-op or reject; no new generation touched. |
| Callback referencing locked/review sale generation | No-op; principal and unknown transfer/payment locks remain unchanged. |

`nft_token` and `nft_is_approved` are the existing NFT/SDK 5.30.0 interfaces.
`nft_token` returns `Token | null`; `nft_is_approved(token_id, approved_account_id,
approval_id: Option<u64>)` returns bool, and the installed implementation panics for
a missing token. Always supply the exact numeric u64 approval (including full width),
not `None`. A missing token can therefore produce both a null token view and a
failed approval dependency; neither allows activation. This task adds no NFT ABI.

The SDK's `promise_result_checked` distinguishes successful bounded bytes from
`Failed`/`TooLong`, and aborts on an unrecognized host result status. Exact count and
stored-context validation precede those reads. A failed/malformed **view** provides
no proof that the seller lost ownership or that a transfer failed. It only retires
this candidate, with its reservation retained. Missing callbacks cannot be
classified conclusively by the present architecture and remain unresolved; there
is no recovery or redispatch path that guesses their outcome.

Two copies of one result cannot validate both distinct response types; extra or
missing result counts reject. Results are authenticated host dependencies on the
executing branch, **not independent finalized receipt proofs**. No caller or
operator injects a conclusive outcome.

NEP-178 has no revocation hook. Views can race with later revocation/re-approval,
so `ACTIVE` is provisional, never transfer authority. Here it is the existing
internal listing-state label for successful verification observations, not an
active public marketplace offer or validated commercial policy. Neither it nor
owner-controlled dispatch enables the listing/purchase gates. Seller commercial
consent, approved fees/treasury/funding, and eventual purchase-time exact approval
checks remain separate requirements. Every eventual purchase must
recheck the exact seller/approval in the synchronous NFT wrapper. New hooks cannot
replace live/in-flight generations; conflict and authenticated high-water rules remain.
Rejected hooks are not queued; a fresh owner approval is needed after retirement.

## Purchase preparation only

`prepare_purchase_call` is a Rust helper, **not a NEAR export or send capability**.
It reloads a persisted pending sale and matching transfer attempt, recomputes the
v1 context, pins the reviewed wrapper ABI, and constructs only:

```text
pinned NFT.marketplace_transfer_approved(
  expected_seller_id = persisted seller,
  token_id = persisted token,
  approval_id = exact persisted U64 decimal string,
  receiver_id = persisted buyer
), deposit = exactly 1 yoctoNEAR
```

No standard-transfer fallback, purchase entry point, scheduling or settlement
callback is added. Unknown transfers cannot be prepared as retries. The eventual
purchase callback ABI, fee/treasury governance, payer/refund decisions and the
storage-aware financial promise/debit boundary remain prerequisites. A nonzero
placeholder callback commitment in a synthetic test is not an implemented ABI.
Standard NFT hooks/views retain numeric u64 JSON; production JavaScript tooling
must preserve full-width integers, while the wrapper uses strings.

## Compatibility and release blockers

Existing v1 root/key/record/operation bytes and test vectors are unchanged. Foundation
changes add read-only validation accessors and the additive guarded-reservation
transitions; all existing v1 operation vectors remain unchanged. NFT code/ABI and
frontend files are unchanged. Admission sidecars are additive; only the optional
immutable owner is installed at bootstrap;
no legacy reservation receives implicit purpose/consent or funding attribution.

Old runtime deployment domains bind the old code hash. This build cannot simply
replace a deployed prototype: source/domain/key migration, complete raw snapshots,
financial reconciliation and receipt closure need a separately authorized review.
Do not edit manifests or reset state to bypass mismatches.

Before mainnet: approve the actual external owner and its custody; approved
policy/funding installer (owner-only admission is not that policy); fee/treasury
and storage/refund governance; measured gas/storage limits; live NFT
code/binding verification and immutable accounts without transaction-signing keys;
node cross-contract, race, rollback and OOG tests; migration rehearsal; independent
security audit and operational readiness. The private-callback model trusts NEAR
dependency semantics and exclusively reviewed outbound call sites; retained signing
or upgrade keys would undermine those boundaries.

Native SDK tests and Wasm VM tests use synthetic balances/trie/results. No NEAR
sandbox/node is available. They do not establish actual cross-contract execution,
finality, gas safety, refund timing or receipt rollback. No deployment occurred.

Prior admission-lifecycle validation (before controlled dispatch):

| Check | Result |
| --- | --- |
| Foundation `cargo test --locked --offline`, default / testnet / production | 76 tests + 2 doctests passed per mode |
| Runtime native, testnet / production / testnet,test-hooks | 26 passed per mode |
| Runtime VM unit harness, same three feature modes | 38 passed per mode: 26 native + 12 Wasm integration tests |
| Existing `wasm_runtime` harness (including all three artifact export sets) | 12 passed |
| NFT native / Wasm regressions, unchanged NFT source | 42 / 5 passed |
| Independent Python v1 vectors | 6 passed |
| Repository JS regressions with `node --experimental-vm-modules --test tests/*.test.mjs` | 5 test files passed |
| Runtime `build.sh testnet`, `production`, `testnet test-hooks` | All three Wasm builds passed |
| Formatting and Clippy (`-D warnings`), foundation and runtime supported modes, including VM harness | Passed |

Added 4 foundation, 8 native lifecycle and 7 Wasm lifecycle tests. Initial runs
identified two test expectations to update (the bounded guard read and bootstrap's
void return), and the JS suite requires its documented experimental VM flag. No
failing test remains in the final runs.

New tests cover owner authentication and own-credit consent, global generation
competition, same-token exclusion, full-width approval IDs, 1,024-byte token IDs,
exact retries, stale callbacks, managed dispatch cursors, used/unknown/legacy
release rejection, measured charging, storage exhaustion, additive guard corruption
and overflow, and preserved principal/claims/holds/unattributed balances. MemoryStore
fault injection proves only offline batch rollback. A Wasm post-write exhaustion
test deliberately observes the VM harness retaining writes after abort, so it
cannot be mistaken for proof of actual receipt rollback.

Use the existing build script for `testnet`, `production`, and `testnet test-hooks`.
The new Wasm tests are unit-test modules: on this host compile them with
`cargo rustc --locked --offline --features testnet,vm-tests --lib --profile test --
-C link-arg=/home/acknowlege/.local/usr/lib/x86_64-linux-gnu/libc_nonshared.a`
and run the emitted test executable. The existing `--test wasm_runtime` harness
uses the same documented host-only link workaround. Never apply it to Wasm builds.

## Controlled dispatch validation

New native and Wasm tests invoke owner dispatch, including the real generated
Wasm export, and inspect its three-receipt ordered graph: two pinned zero-value
views followed by the private callback. Tests cover authorization, exact locators,
extra caller fields, duplicate sends, stale/terminal admissions, persistence,
result permutations, private callback forgery, expiry, gas preflight, backing
shortfalls, zero remaining credit, and retained principal/claims/holds/reservations.
Host result fixtures remain synthetic; they do not prove live cross-contract
execution or finality. Existing receipt-abort tests still cannot prove node rollback.

Final regression results for controlled dispatch:

| Check | Result |
| --- | --- |
| Foundation default / testnet / production | 76 tests + 2 doctests passed per mode |
| Runtime native testnet / production / testnet,test-hooks | 34 tests passed per mode |
| Runtime VM unit harness in all three modes, loading the corresponding Wasm artifact | 53 passed per mode: 34 native + 19 Wasm tests |
| Existing runtime Wasm regression harness | 12 passed; includes all three export allowlists |
| Unchanged NFT native / Wasm regressions | 42 / 5 passed |
| Independent Python encoding vectors | 6 passed |
| Repository JS regression files with the required experimental VM flag | 5 passed |
| All three runtime Wasm builds; formatting and Clippy with warnings denied | Passed |

Added 8 native and 7 Wasm dispatch tests. The additional export is restricted to
owner-created admissions; artifact allowlists still exclude purchases, settlement,
withdrawals, refunds and activation methods. Neither `near-sandbox` nor `neard`
is installed in this environment. Receipt atomic rollback, OOG behavior and real
cross-contract execution remain node-level release blockers, not verified claims.

Deployment still needs reviewed immutable owner custody, authentic prepaid funding,
commercial/fee/treasury authorization, measured budgets, NFT deployment/key/code
verification, and a separately approved code/domain migration for existing accounts.
There is no safe implemented recovery classification for a missing callback. Keep
that exact attempt and its reservation unresolved rather than adopt a timeout or
operator-supplied result as evidence.

## Security correction: unverified admissions cannot advance approval high-water

The frozen v1 token slot still stores the latest **candidate** approval ID, so its
encoding and exact pending-operation linkage remain unchanged. It is no longer used
as the high-water authority for newly created records. Additive `RecordKey` tag 12
and `Value` tag 9 encode a token-bound `ApprovalWatermark { authenticated: bool,
high_water: u64 }`. All original discriminants, envelopes and operation vectors are
unchanged; tag 11 remains reserved. Old binaries cannot read the new record kind.

Creation preallocates the fixed-width watermark in the same atomic/metered receipt,
initially `false, 0`. Only successful exact ownership/approval verification advances
it. Cancellation, release, malformed views, failed views and expiry do not consume
an unverified ID. In particular, an invalid/released `u64::MAX` candidate does not
prevent a later real approval of 1. Authenticated consumed IDs still reject reuse.
Generation/attempt checks and permanently retained admission tuple records prevent
replay after release or invalidation; exact creation retries only return the old
status and cannot create another reservation. The offline model's generation replay
barriers are distinct from the runtime's additional admission-tuple barrier.

Verification loads the current token slot and watermark in a bounded batch; it
cannot update an older generation. Updating the preallocated watermark is fixed
width and costs no additional callback storage. Creation's byte ceiling must cover
its full record/key cost. Principal/claim/hold/unattributed records are unchanged.

For existing v1 records without a watermark, the historical consumed value is
preserved conservatively; this change does not silently rehabilitate old poisoned
records. A pending old callback cannot allocate missing metadata through the runtime
fixed-size callback boundary. Existing deployed domains still require an authorized
migration, complete receipt history and explicit funding. No live state is migrated.
See [testnet preparation](../../testnet/README.md) for the still-blocked deployment
and mint/approval/admission/verification sequence.


## Testnet owner funding milestone (supersedes earlier missing-funding notes)

Ordinary `testnet` and test-hook builds now export
`fund_testnet_storage({"funding_nonce":"<u64 decimal>"})`. Production has no such
export. It requires the immutable admission-owner **predecessor**, zero promise
results, positive attached NEAR and `.testnet` owner/NFT/marketplace accounts.
There is no allocation recipient, policy, treasury or refund argument. The signer
cannot substitute for predecessor. Account suffix/build separation is not proof
of chain identity; deployment tooling must verify actual testnet network/genesis.

SDK 5.30.0 rejects `#[payable]` receiverless exports, and a state receiver would
conflict with the frozen marketplace root. The dedicated raw testnet export sets
the SDK panic hook, reads at most 128 input bytes through a bounded host register,
strictly decodes the sole U64 field and returns JSON. It never reads/writes SDK
marker state. Generated method isolation is checked against actual Wasm exports.

The host account balance already includes the attached amount. Funding derives
its prior observation by checked subtraction of that exact amount, verifies the
existing backing, then applies `Fund`. In the same receipt it measures actual
storage growth and applies `ChargeFundingStorage` to that credit only, changing
available credit into funded stake. Charged cost may not exceed the fresh deposit,
so old owner credit or another owner's funds cannot cover an undersized deposit.
The second batch changes only fixed-width records; any size/protection mismatch
or post-write error aborts the receipt. Node rollback/OOG remains unverified.

Each credit hash binds domain, owner and nonce. Retained credit tags move from
StorageFunding (0) to StorageFundingCharged (3); old discriminants/encodings stay
unchanged. Repeated funding nonce or charging rejects; a duplicate payable request
cannot silently succeed and retain a second unaccounted deposit. No release,
payment, refund, withdrawal, deficit repair, free-capital allocation or hold-clear
occurs. Gross funding equals available + reserved + staked + in-flight + withdrawn;
queued remains a subset of in-flight. The receipt sends no promises.

Funding result: `{owner_id, funding_nonce, received, storage_charged, available}`;
all numeric values are decimal strings. Admissions consume the same prepaid owner
credit and retain the existing Ready -> Dispatched -> Completed path/cursors 1/2/3.
No public NFT approval intake or commercial listing is enabled. Missing callbacks
retain locks; successful verification is provisional and still requires purchase-
time exact approval authorization before any future NFT movement.

See [the current testnet runbook](../../testnet/README.md) for actual CLI commands,
immutable manifest construction, explicit budget/fixture inputs and finalized
record verification. No accounts, deploys or real cross-contract outcomes are
established by the local test suite. Old documents' synthetic prepaid fixtures
remain tests, not funding evidence or permission to install credits manually.
