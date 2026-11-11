# Runtime storage accounting — Milestone 3

Implemented: internal accounting, additive persistence and a disabled runtime
storage-write helper. Ordinary artifacts still export only bootstrap, storage
status and SDK metadata. No public financial methods, NFT wrapper, recovery,
activation, fee/treasury values or payer/refund policy were added.

## Records and compatibility

Existing v1 root/record encodings and operation vectors are unchanged. New
`NMPSTG01` envelopes contain a deployment commitment, typed key, nonzero revision,
tagged value and domain-separated checksum. The root is `nmp:s:root:v1`; other
keys use `nmp:s:` + LE schema 1 + deployment commitment + Borsh key. Tags 0–4
are root, account, reservation, withdrawal and incoming credit. Values are bounded
at 1,024 bytes; allocation-free preflight checks all account-string lengths,
discriminants and exact input consumption before Borsh. Checksums are not receipt
proof or protection against malicious replacement of all trusted storage.

Each batch compares/advances the v1 root and touched storage records together,
with at most eight keys (current commands touch at most five). No historical
scan or whole-ledger serialization is needed. Credit/withdrawal tombstones and
reservation IDs remain permanent; consumption advances a checked nonce, with no
per-consumption history array. Withdrawal commitments bind the domain, unique ID,
owner, amount and single attempt. They identify intent, not a protocol receipt.

Fresh bootstrap adds an empty storage head while preserving exact v1 empty-root
bytes. Missing accounting records never become zero balances. This is not a
migration: populated older states/code domains and existing v1 operating reserves
need a reviewed bridge/import. Old operating reserves retain their original
meaning and remain protected; this extension cannot silently spend or convert them.

## Accounting rules

Per account and globally, with checked arithmetic:

```text
funded = available + remaining reservations + materialized storage stake
       + in-flight withdrawals + confirmed withdrawn
queued <= in-flight withdrawals
```

| Transition | Rule |
| --- | --- |
| Identified funding | Exact credited A delta increases funded and available once, under a unique credit ID. An approved adapter must authenticate its purpose and beneficiary. It creates an equal liability, no free capital, and cannot repair a deficit. |
| Reserve | Move only that account's available credit into a unique reservation. Reject holds, missing backing and borrowing another account's funds. |
| Consume | Move measured S growth from that reservation's remaining amount into staked; advance its nonce. Zero-growth fixed-size writes cost zero but still consume a nonce. Negative deltas give no refund and are rejected. |
| Queue withdrawal | Move only available credit into in-flight and queued; fix the recipient to its storage-account owner and bind the amount/intent. No transaction is constructed. |
| Materialized debit | Exact A decrease with other partitions unchanged removes queued once, retaining in-flight. A balance match alone is not authority to record this transition. |
| Outcome | Only a matching debited/unknown attempt resolves. Success becomes withdrawn; exact Transfer failure restores available debt, not cash. Missing or panicked callbacks remain unknown. |
| Incoming value | Exact credited A delta enters U under a unique ID. NFT SDK refunds, gas refunds and unrelated transfers have no inferred beneficiary or entitlement. |
| Attribute withdrawal refund | Match one recorded incoming credit to one failed withdrawal/commitment/amount, with no previous attribution. Remove U once; do not add another claim or funding credit. The future adapter must verify the actual receipt lineage. |
| Health | Persist sticky storage and v1 financial holds in the same batch. No clear/resume method. Funded completion and genuine outcomes preserve the hold and debts. Corrupt undecodable records reject; health checks cannot repair them. |

There is no unreserve, storage-stake release, pruning, withdrawal retry or
reassignment API. Unused reservations and unknown withdrawals can remain locked.
Release/refund policy and evidence of operation closure are separate prerequisites;
durable tombstones are never promised as refundable bytes. Existing operating
contributions keep their previous deficit-repair behavior.

## Combined backing and runtime writes

Backing protects actual host S, legacy held/available principal, queued payments,
all legacy operating reserves/free capital and explicit legacy external Q/L/U,
plus new available storage credits, remaining reservations, queued withdrawals
and U. Debited sends are not subtracted again. Recorded staked amounts are a
subset of host S, not another liquid deduction. A larger A never increases credits.

`runtime::storage_accounting::apply_reserved_storage`:

1. Loads the domain-bound records and existing reservation; checks backing against
   host account balance and storage usage multiplied by the SDK byte-price constant.
2. Applies an internally prepared v1 payload. Rejects payloads rewriting the new
   accounting namespace, attached value or changes to legacy financial state.
3. Measures growth and prepares one reservation consumption.
4. Requires every charge record already exist and retain its encoded size; commits
   the charge and rechecks usage/backing. Thus charge bookkeeping adds no unfunded
   bytes. Every failure panics, including failures after writes.

The helper uses zero legacy external Q/L/U only at this disabled, storage-only
boundary. It must not be reused unchanged for trading. The future operation must
select its bound reservation and nonce internally. Raw HostStore and offline
commands are not authorization layers. Old offline financial commands must never
be exposed as runtime methods that ignore the new storage liabilities.

Funding/account/reservation/withdrawal/credit-record creation also needs prepaid
metadata storage. Public finance paths remain absent until approved payer rules
and measured budgets cover these writes. Test fixtures use their prefunded test
account for setup metadata; this is not a production funding rule. Bootstrap's
own bytes are covered by its prefunded balance check.

## Promise and refund boundaries still required

Before an approved send: reserve dispatch, callback and tombstone storage; derive
recipient/amount from the stored claim; record the queued attempt; explicitly
construct the complete promise/callback chain in the same receipt; inspect the
host debit; record it once; and perform the post-send backing check. Do not rely
on deferred Promise destruction. Any failure must abort the receipt. The current
storage-only helper rejects financial mutations and does not implement this send.

Private callbacks must authenticate their own persisted dependencies/attempts.
Transfer failure and callback failure are different. Restored debt can precede
the protocol refund; cash can arrive before classification. Neither sequence
grants a resend or free credit. The internal attribution command has **no runtime
caller**: protocol refund lineage/finality verification and attribution policy
remain missing. Caller-supplied outcomes, balances, IDs or hashes are never receipt
evidence. No operator/recovery override exists here.

## Tests, limitations and approval inputs

Added tests cover funding/withdrawal bounds, account isolation, reservation
creation and partial/exact/zero consumption, replay, deficit rejection, early/late/
duplicate/wrong-attempt refunds, unknown locks, sticky holds, legacy principal and
claim protection, bounded history access, reopening, corruption/length bombs,
overflow and injected MemoryStore failures at every write.

The real Wasm VM test measures a 1,024-byte token-ID fixture: **5,324 additional
bytes**, charged as **53,240,000,000,000,000,000,000 yoctoNEAR**, with the same
reduction in its reservation. This is one synthetic fixture, not a production
budget or reserve parameter. Insufficient funding aborts after the payload write.
VM-runner 0.38.0/protocol 88 uses MockedExternal, which retains aborted writes.
**Node receipt rollback, OOG, promise debit/refund timing and mainnet compatibility
remain unverified.** No sandbox/node is available and no deployment was performed.

Current validation: 72 foundation tests plus 2 doctests in each of default,
testnet and production; 6 native runtime tests in each of testnet, production and
testnet-with-hooks; 12 compiled Wasm tests; and all 6 unchanged independent Python
vectors passed. Build pins are unchanged. Native Wasm-test linking uses the
existing documented libc_nonshared workaround on this machine.

Before financial entry points/deployment, approve payer/beneficiary and sponsor
terms, metadata budgets, unused-reservation release/refundable-byte rules,
protocol refund attribution/evidence, withdrawal/attempt policy and hold-clearance
authority. Finish the storage-aware financial adapter, migration review, node
accounting/rollback/OOG tests and independent audit. Fee/treasury, NFT wrapper,
recovery and trading activation remain separately blocked by the architecture.
