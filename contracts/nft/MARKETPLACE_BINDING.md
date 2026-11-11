# NFT marketplace binding — implemented candidate ABI

This milestone implements the NFT side only. Public minting stays disabled; both
marketplace trading gates remain false. No deployment, purchase orchestration,
recovery, custody-return method, fee policy or wallet integration is included.
This supersedes the earlier specifications' observation that the wrapper is absent,
without changing their marketplace security requirements.

## Initialization and authority

`new_with_marketplace(owner_id: AccountId, metadata: NFTContractMetadata,
marketplace_id: AccountId)` initializes **fresh state** with an immutable binding.
All three initializers require predecessor AND signer equal to the NFT account,
zero deposit and zero promise results; the marketplace must be a different
account. Use an approved atomic deploy/self-initialize bootstrap. NEAR's generated
`#[init]` guard rejects an existing root. There is no binding setter. The existing
`new` and `new_default_meta` JSON signatures remain available and produce unbound
V2 state; the wrapper always rejects an unbound contract. They cannot subsequently
activate a binding. `marketplace_id()` returns its account string or JSON `null`.

Account selection, deployment identity, code review/admission and immutable-code/key
verification remain operator/release decisions. Account names or self-initialization
do not prove the right network or safe marketplace code. These checks must precede
admission, including removing transaction-signing keys that could bypass marketplace
contract methods. No accounts or governance parameters are selected here.

## Transfer ABI

```json
{
  "expected_seller_id": "seller.testnet",
  "token_id": "opaque-token-id",
  "approval_id": "18446744073709551615",
  "receiver_id": "buyer.testnet"
}
```

Call `marketplace_transfer_approved` with exactly **one yoctoNEAR**. Approval uses
SDK `json_types::U64`: a required decimal **string**, never a JSON number or optional
field. Generated callers serialize its canonical decimal form. SDK parsing accepts
its integer string syntax (including leading zeros); this is not a canonical
authorization-payload codec. Marketplace typed commitments must use the decoded
u64 and canonical encoding, and generate the call themselves.

Before any mutation, in the same synchronous receipt, require:

1. Configured marketplace equals NEAR predecessor; signer is not authority.
2. Nonblank token ID of at most 1,024 UTF-8 bytes, without normalization.
3. Expected seller differs from marketplace; receiver differs from seller and
   marketplace; the actual owner exists and equals that expected seller.
4. Approval extension/map contains the marketplace with exactly the supplied ID.
   Approval deserialization has a bounded host-register read, checks map count and
   rejects malformed/noncanonical Borsh. There is no owner-caller exception.

Only then call the pinned SDK's synchronous `internal_transfer`, updating ownership
and enumeration, clearing all approvals and emitting its standard NEP-171 event.
Return exactly the four UTF-8 bytes **`true`**, with no newline, from this receipt.
All validation failures abort; no `false` success result is defined. This method
creates no promises, including storage refunds. It follows the existing standard
`nft_transfer` storage behavior; released approval storage is not credited to any
new marketplace payer/refund scheme. Retained counters/tombstones stay funded.

The NFT cannot infer a sale, price, generation or paying buyer: the marketplace
must derive every argument from its immutable operation records. A different token
with its own matching owner/approval is separately authorized; an arbitrary receiver
other than the excluded accounts cannot be identified as "wrong buyer" by the NFT.
Real callback results must come from NEAR's expected promise dependency. Caller JSON,
mock outcomes, events or `true` supplied by another call are not receipt evidence.

## Approval lifetime and standards compatibility

Re-approval always advances the retained counter, including without a hook message.
Revocation removes authorization; re-approval cannot satisfy an operation committed
to an older ID. Transfer clears approvals without resetting the counter. Issuance
stops when the next counter is `u64::MAX`; it never wraps. The wrapper accepts the
full u64 range when the exact ID is active, including MAX in an audited legacy
fixture; new issuance can reach MAX-1 and retains MAX as exhausted.

New approvals enforce **32 active accounts per token** and a **4,096-byte hook
message**. Replacement at the cap is allowed; additional accounts reject before
counter writes. These are execution bounds, not fee/payer/governance choices.
Account IDs use SDK validation (maximum 64 bytes). Hook ABI/gas allocation and
measured storage charging remain as before. Inputs are initially decoded by SDK
JSON under NEAR protocol/gas limits; method checks are not a pre-decoding input cap.

Standard NFT methods and numeric u64 approval fields remain unchanged. In
particular, `nft_on_approve`, `nft_is_approved`, `nft_transfer`, `nft_token` approval
maps and transfer-call resolver arguments use exact JSON **numbers** as specified
by installed standards. JavaScript consumers need a lossless parser/integer emitter
for those paths: ordinary `JSON.parse`/Number can round IDs above 2^53-1. Do not
silently change standard fields to strings or convert an already-rounded Number.

Ordinary `nft_transfer` still has its owner exception; it is never a marketplace
purchase/retry fallback. Transfer-call scheduling now uses the private
`nft_resolve_transfer_versioned(previous_owner_id, receiver_id, token_id,
approved_account_ids, ownership_version: U64-string, approval_version: U64-string)`.
Approval map values retain numeric u64 encoding. The old resolver export remains
but always aborts: its arguments cannot authenticate a mutation epoch.

Permanent raw sidecars at `nft:mutation:v1:` plus SHA-256(token UTF-8) store two
little-endian u64 values (ownership and approval versions), with bounded 16-byte
reads. They do not change the BindingV2 root. Minting funds their allocation; a
legacy token can allocate one through a funded approval. Transfers never create
unfunded sidecars: legacy synchronous access remains available, while transfer-call
requires an existing funded version. All retained versions survive burn and must
never reset. Transfers (including the wrapper) advance both versions; approvals and
revocations advance the approval version even when a revoke finds no entry.

Resolution requires private predecessor, zero deposit, one bounded host result and
the dispatched ownership version. Intervening ownership changes make it a no-op.
On return, receiver approvals are removed; seller approvals restore only if their
version was not superseded. Completion advances the version to reject duplicate
resolution, including keep-token results. Unknown/malformed receiver results retain
NEP-171's return-request semantics; they are not marketplace payment evidence.

Absent maps remain absent in callback arguments. Empty legacy maps are never
restored or refunded. Approval over a legacy empty map discards the empty entry
before measuring the new full funded allocation. Nonempty restored maps were removed
without a refund at dispatch. Receiver-map refunds measure only those removed map
bytes; permanent counters and mutation records remain funded. The resolver reserves
20 Tgas; this is a technical budget requiring real-node profiling, not gas assurance.

Token IDs and transfer-call messages are bounded to 1,024 and 4,096 bytes, and
captured approval maps to 32 entries. Oversized legacy records retain synchronous
transfer/revocation access but cannot enter the new asynchronous flow. Authentic
receipt ordering, funding depletion, rollback and OOG still need node tests.

## Migration, validation and release limits

See [MIGRATION.md](MIGRATION.md) for frozen V1/V2 layouts, retained historical data,
write-free conversion evidence, root growth and the outstanding authorized bridge.
Do not deploy V2 directly over V1. Migration authority, complete historical evidence,
receipt closure, legacy storage funding and rehearsal remain prerequisites.

Run from `contracts/nft`:

```sh
cargo test --locked --offline
cargo fmt --check
cargo clippy --locked --offline --all-targets -- -D warnings
cargo build --locked --offline --release --target wasm32-unknown-unknown
cargo test --locked --offline --features vm-tests
```

SDK/standards are exactly 5.30.0, host bindings 0.4.0; Wasm configuration mirrors the
existing runtime prototype. The optional native harness executes the actual Wasm
with near-vm-runner 0.38.0/Wasmtime 45.0.3 and protocol configuration from that runner.
Its `MockedExternal` does not roll back writes. Rejection tests check no writes
occurred; they are not node-level rollback, OOG, receipt-finality or migration proof.
On the current host the VM executable needs the same `libc_nonshared.a` link
workaround documented for the marketplace harness; this is not a Wasm build flag.
Use `cargo rustc --locked --offline --features vm-tests --lib --profile test --
-C link-arg=/home/acknowlege/.local/usr/lib/x86_64-linux-gnu/libc_nonshared.a`
and run the resulting test executable under `target/debug/deps`. The reviewed run
used `/tmp/nft-binding-vm-target` as its separate native target directory. Do not
set this host-only flag on the Wasm artifact.

Before release: independent security review, source-snapshot migration rehearsal,
node-level failure/OOG tests, exact dependency gas measurements (including maximum
token/approval sizes), storage-solvency verification, finalized receipt validation,
marketplace operation integration and immutable deployment/key evidence. No success
of a local fixture establishes any of those production requirements.


## Restricted testnet mint adapter

The `testnet` feature adds `new_testnet(mint_authority_id, metadata, marketplace_id)`
and `testnet_mint_authority() -> AccountId|null`. Fresh direct self-initialization
installs a permanent raw authority sidecar without changing the frozen BindingV2
root layout; full root/sidecar storage is measured and must be backed by the NFT
account. No setter or live migration is added. Existing constructors install no
authority, so merely building with `testnet` does not enable old deployments.

Existing `nft_mint(token_id, token_owner_id, token_metadata)` requires that authority
as predecessor, no promise results and recipient=authority. All accounts are bound
to `.testnet` names and the configured marketplace; external network verification
remains required. Mint charges the existing measured storage deposit, including
permanent ID and mutation sidecars, and preserves the existing excess-deposit refund.
This is restricted testnet self-minting; public/third-party curated minting remains
unimplemented. Default/production builds still reject every mint call and omit the
two testnet authority exports. No wrapper, approval or resolver ABI changes.

Build separate artifacts using `bash build.sh testnet` / `production`; never deploy
production or test-hook artifacts through the testnet runbook. Preserve every raw
sidecar, tombstone and approval high-water record during any future independently
reviewed migration. Existing root compatibility does not prove receipt closure or
safe downgrade. See [testnet preparation](../../testnet/README.md).
