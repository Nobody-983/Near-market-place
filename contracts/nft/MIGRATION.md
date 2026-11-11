# NFT state migration design — not an executable migration

Resolver-hardening addendum: the `BindingV2` root is unchanged and independently
frozen as `BindingV2State`. New per-token ownership/approval versions are permanent
16-byte raw sidecars, funded at mint or first paid approval. Do not reconstruct or
reset existing sidecars, even for burned tokens. No migration is executed here.
Legacy tokens without sidecars cannot start a new transfer-call; synchronous
transfer/revocation remain available. The old unversioned resolver now aborts, so
all old transfer-call/resolver/refund receipts MUST be conclusively drained before
any future authorized upgrade, despite unchanged root serialization. Retaining the
old ABI does not make pending old receipts safe. Historical nonempty approval-map
funding still needs reconciliation; the new code cannot prove legacy funding.

Do not deploy the current NFT code over an existing account without its reviewed
migration. There are three incompatible exact roots:

| Layout | Borsh fields in order |
| --- | --- |
| `LegacyWithoutRegistry` | `tokens, metadata` |
| `RegistryV1` (previous contract) | `tokens, metadata, ever_minted` |
| `BindingV2` (current contract) | `tokens, metadata, ever_minted, marketplace_id: Option<AccountId>` |

Initializers reject existing `STATE`. No migration/bridge transaction entry point
is implemented or authorized. `src/migration.rs` checks exact layouts, prepares an
offline identity inventory, and offers **write-free** `prepare_binding_v2` for the
exact frozen `RegistryV1` root. This function returns a candidate object; it neither
authenticates a source/manifest/authority nor writes `STATE` or executes migration.

## Evidence and authorization prerequisites

Approve an exact manifest binding network/genesis, NFT account, source and target
code hashes, source layout, final source block/state root, complete raw storage
snapshot, evidence digest, import digest/count, byte-cost observations and funding.
Identify the actual upgrade authority and independently review its custody. No
account, authority, economic allocation or deployment value is selected here.
Use source code and access-key history to account for every previous upgrade and
every method that could mint, burn, change ownership or reset an approval counter.

Live enumeration is not mint history. Obtain every successfully minted ID, including
subsequent burns, from complete verified archival receipts/state history for the
same NFT account. Failed/reverted mint logs are not successful mints. Include every
historical approval counter, including counters of burned tokens. Deduplicate
receipt replays by their authentic receipt/event identity before constructing the
inventory; two distinct successful mints using one ID are an identity conflict
requiring separate review, not a record to silently merge.

For a source with a permanent registry, export its complete raw keyspace, not only
live tokens: `LookupMap` cannot enumerate itself. Preserve all existing registry
entries and reconcile them with history. A registry introduced by an earlier
migration cannot prove completeness before its introduction. Missing archives,
unexplained keys, incomplete burn history or conflicting evidence block migration
and marketplace admission. Do not reset state or change the NFT account to bypass
this blocker. A human declaration/hash alone does not prove historical completeness.

## Future migration procedure

1. Independently reproduce the exact source binary/layout and validate the snapshot
   against the finalized source state root. Rehearse using that snapshot in a
   sandbox. `check_source_layout` rejects the wrong layout/trailing bytes but does
   not verify a state root, code hash, or authority.
2. Prevent new mutations before taking the authoritative import snapshot. The
   legacy contract has no general freeze method, so a separately reviewed bridge
   release compatible with its exact layout is required. Stop new mint/approval/
   transfer work, preserve genuine callbacks while draining already-issued work,
   and prove all outstanding transfer-call/resolver/refund receipts resolved.
   Freeze the final source state only after that closure. An owner view or elapsed
   delay is insufficient; do not migrate an unsettled resolver into incompatible
   code. This bridge and its entry points require a later authorized implementation.
3. Replay and reconcile the complete successful-mint/approval history against that
   frozen snapshot. Import every exact ID into the independent registry, including
   burned IDs absent from ownership/enumeration. Retain existing storage prefixes,
   live owners, metadata and approval maps. Preserve each counter at least at the
   maximum of retained state and verified historical next-counter value. Never
   lower/reset a counter; preserve exhausted counters as exhausted. Burned IDs stay
   non-live and permanently consumed. Older IDs exceeding today's mint length
   limit must be retained exactly, not truncated, normalized or dropped.
4. Measure and explicitly fund all new registry bytes, restored permanent counters,
   bridge/cursor records and required temporary storage at the actual byte cost.
   Audit funding of legacy approval-map overhead before the new measured revocation
   refunds are admitted. Record who supplies any legacy funding shortfall; do not
   silently debit marketplace principal or promise refunds of retained bytes.
5. Use bounded, manifest-bound import batches with monotonic cursors and content
   commitments. Replay of a completed batch is a no-op; changed/reordered batches
   reject. Keep ordinary operations disabled until the complete inventory and
   source-preservation checks pass. Partial imports cannot expose minting. Define
   bridge interruption/resume and rollback before the first write; never revert to
   code that can mint without consulting the completed permanent registry.
6. Atomically finalize the reviewed target root only after full count/digest,
   ownership/metadata/approval/counter preservation and funding checks. Verify
   finalized target code/state/keys and archive evidence. Migration completion must
   not enable minting or marketplace trading. Require a separate release decision.

The new candidate target is `BindingV2`; a previously approved `RegistryV1` manifest
does not authorize this layout or its marketplace account. Pin the immutable
marketplace binding in a separately reviewed target manifest. The new root needs
`1 + 4 + marketplace_account_utf8_bytes` more bytes than `RegistryV1` when bound;
an unbound root adds one byte. Explicitly fund the actual resulting storage stake.
An old constructor creates an unbound V2 root and cannot later set its binding.

`RegistryV1State` freezes the exact previous field order independently of `Contract`.
SDK and standards are pinned to **=5.30.0**; their nested collection layouts and the
committed lockfile are also part of compatibility. The golden fixture
`tests/fixtures/registry_v1_root.hex` was captured from the previous `Contract`,
and round-tripped through the frozen type **before** adding the binding field.
Do not update it simply to make a future incompatible dependency change pass.

The V1-to-V2 candidate conversion moves all existing collection handles unchanged.
All non-root trie entries must remain byte-for-byte unchanged, including unknown
historical tombstones and exhausted counters. No live enumeration, registry
reconstruction, truncation or counter reset occurs. Legacy two-field roots are
rejected by this converter; they still require the history/import bridge above.
Wrong-layout, truncated, trailing-byte and repeated V2 conversion attempts reject.
Exact decoding cannot establish that stored data are semantically complete or
authentic. Future root changes must freeze V2 before modifying `Contract` again.

Inventory oversized historical IDs/maps before admission. The wrapper and new
approvals accept only IDs up to 1,024 UTF-8 bytes and at most 32 active approvals.
Existing IDs, tombstones and maps are preserved, including larger historical ones.
Standard revocation/transfer remain available without these new admission bounds;
an oversized approval map needs owner-directed revocation before new approvals or
wrapper use. An oversized token ID remains ineligible for the wrapper. Do not
silently delete approvals or IDs in migration to satisfy these limits. Account for
the gas/funding needed to revoke exceptionally large legacy maps during rehearsal.

## What the regression tests establish

Tests demonstrate incompatible legacy/current decoding, rejection of trailing or
truncated state, refusal of live-only/incomplete inventories, retained burned IDs,
preserved/exhausted counters, registry conflicts and duplicate successful mints.
Frozen V1 fixtures also verify live owner/metadata/enumeration/approval preservation,
long historical tombstones, full-width approval counters and successful wrapper use
after write-free conversion. The Wasm harness rejects initialization over V1 state
and reads the prepared V2 fixture; it does not execute an on-chain migration.
`ExternallyVerifiedLifetime` in the planner explicitly represents a trusted external
verification result; it is not an on-chain proof or permission to execute migration.
No test establishes real archive completeness, authentic deployment identity,
bounded migration gas, receipt closure or a production-safe bridge. Those require
operator evidence, sandbox execution and independent review before any deployment.
