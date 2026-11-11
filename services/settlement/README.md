# NEAR Marketplace Settlement Service

This trusted worker indexes final NEAR purchases into Supabase. It accepts no
transaction, receipt, sale, buyer, seller, token, or price evidence from the
browser. Supabase is an index; the chain and reviewed contract code are the
evidence source.

## Contract evidence

`contracts/marketplace-deploy` emits NEP-297-style `sale_pending`,
`sale_settled`, and `sale_failed` events. `buy_nft` creates a durable sale and
settlement record, calls the configured NFT contract's
`marketplace_transfer_approved`, and schedules `on_transfer_result({ sale_id })`.
A successful callback sets `Sale.settled = true`, records NFT delivery, and
credits the snapshotted seller and fee liabilities.

The NFT contract's pinned `near-contract-standards` 5.30.0 implementation emits:

```text
EVENT_JSON:{"standard":"nep171","version":"1.0.0","event":"nft_transfer","data":[{"old_owner_id":"...","new_owner_id":"...","token_ids":["..."]}]}
```

The same worker also reconciles seller/fee/refund/storage withdrawals and
registration-excess refunds. For `withdraw_proceeds`, `withdraw_fees`,
`withdraw_refund`, `storage_withdraw` and registration-only `storage_deposit` it
requires:

- the exact signed call, one-yoctoNEAR (or registration deposit) value and
  callback arguments;
- the marketplace `transfer` receipt and its private `on_withdrawal_result`
  callback linked by the promise data dependency;
- exactly one `withdrawal_succeeded` or `withdrawal_failed` event matching the
  account, recipient, amount, nonce and kind, plus the boolean callback result;
- historical `pending_withdrawal`, `ledger`, proceeds/refund/storage balances and
  (for refunds) `settlement` views at the block before the request and at the
  callback block;
- no pending record after the callback, exact debit/restore deltas and, for
  buyer refunds, the `refund_available` to `refunded` transition only on success.

A failed transfer is recorded as `failed` with callback evidence; it is never
promoted to `succeeded`. Replaying the same final receipt graph is idempotent.

## Safe local dry-run (no hosted Supabase writes)

Do **not** start this worker against the hosted Supabase project until the user explicitly approves after reviewing RPC endpoint, contract IDs, pinned code hashes, `SETTLEMENT_START_HEIGHT`, durable checkpoint path, and server-side `SUPABASE_SERVICE_ROLE_KEY` handling.

Safe local verification:
```sh
# From repo root (mocked/local tests only; no hosted DB, no live RPC side effects beyond test fixtures)
node --test --experimental-vm-modules tests/settlement.test.mjs tests/withdrawals.test.mjs
```
For any manual run, use a non-hosted/test Supabase target only with explicit approval and never put secrets in `VITE_*` or committed files.

The worker therefore requires all of the following evidence from the same final
receipt graph:

- a signed `buy_nft({ listing_id })` transaction sent directly to the configured
  marketplace with the exact price attached;
- a marketplace receipt calling the configured NFT contract's
  `marketplace_transfer_approved` with the seller, token, approval ID and buyer;
- the NFT receipt returning base64 `dHJ1ZQ==` (JSON `true`) and emitting the exact
  NEP-171 transfer above;
- the promise data dependency linking that NFT receipt to the marketplace's
  private `on_transfer_result` callback with the matching immutable sale ID;
- matching `sale_pending` and `sale_settled` marketplace events in the purchase
  and callback outcomes;
- no failed, missing, cyclic, unrelated or unexpected action outcomes;
- canonical finalized execution blocks and pinned marketplace/NFT code hashes;
- historical `config`, `marketplace_id`, `sale`, `settlement`, `listing` and
  `nft_token` views at the callback block confirming the contract binding,
  settled sale, authenticated approval, exact accounting and buyer ownership.

The configured HTTPS NEAR RPC is a trust root. The worker cross-checks RPC
structures, canonical blocks and pinned code, but it does not run a NEAR light
client or independently verify outcome Merkle proofs.

## RPC structures

The scanner reads `block.header` and `block.chunks`. It retrieves each newly
included chunk with `chunk({ chunk_id })` and reads transactions from
`chunk.transactions` using `hash`, `signer_id`, `receiver_id` and `actions`.
Blocks do not have a supported top-level `transactions` field.

For candidate purchases, the worker calls `EXPERIMENTAL_tx_status` with
`wait_until: "FINAL"`. It validates `final_execution_status`, `status`,
`transaction`, `transaction_outcome`, `receipts_outcome`, and action/data
`receipts`. Missing receipt bodies are fetched with `EXPERIMENTAL_receipt`.
Historical account and contract views use `query` with the exact callback block.

## Reconciliation and restart behavior

Only opaque facts produced by `ChainVerifier` can enter `Reconciler`. The worker
resolves the Supabase listing UUID from the verified network, marketplace and
contract listing ID, then invokes the existing service-role-only
`reconcile_nft_purchase` RPC. The database locks the listing/NFT, verifies all
identities and price, changes listing and ownership atomically, and rejects a
different purchase after settlement.

`store.js` writes an atomic block checkpoint and an advisory transaction audit
ledger. Replays always reach the database RPC, so local file loss, restarts and
duplicate blocks cannot replace the database's idempotency checks. RPC or
database errors leave the current block uncheckpointed for retry.
Finalized missing block heights are skipped only when RPC returns `UNKNOWN_BLOCK`;
if the node has pruned history after the saved checkpoint, the worker stops and
requires an archival RPC instead of silently losing transactions.

Withdrawals use two service-role-only RPCs: `record_withdrawal_intent` inserts or
replays the request as `pending`, then `confirm_withdrawal` or `fail_withdrawal`
adds the callback block evidence and terminal outcome. The table constraint
forbids `succeeded`/`failed` without callback block hash, height and time, so a
crash between the two calls can only leave a retryable `pending` row, never a
false completion. The unique `(network, marketplace, account, nonce)` and
request-transaction indexes make duplicate and stale callbacks safe.

## Running

```sh
node services/settlement/index.js
```

Use [`settlement.env.example`](settlement.env.example) as the variable template.
Required environment:

- `NEAR_NETWORK_ID` (`testnet` or `mainnet`)
- `NEAR_RPC_URL`
- `NEAR_MARKETPLACE_CONTRACT_ID`
- `NEAR_NFT_CONTRACT_ID`
- `NEAR_MARKETPLACE_CODE_HASH` (deployed 32-byte base58 code hash)
- `NEAR_NFT_CODE_HASH` (deployed 32-byte base58 code hash)
- `SUPABASE_URL` (HTTPS)
- `SUPABASE_SERVICE_ROLE_KEY` (server runtime only)
- `SETTLEMENT_PROCESSED_PATH` (optional; defaults to `./processed.jsonl`)
- `SETTLEMENT_START_HEIGHT` (optional; first finalized height to scan when no
  checkpoint exists; otherwise start 100 blocks behind the final head)

```sh
set -a; . ./services/settlement/settlement.env; set +a
node services/settlement/index.js
```

The service logs each poll error and keeps retrying. Start it before submitting
the first demo transaction, and keep its checkpoint file on persistent storage.

Keep `MINTING_ACTIONS_ENABLED=false`, `LISTING_ACTIONS_ENABLED=false` and
`PURCHASES_ENABLED=false`. Local fixture tests do not prove a deployed Testnet
purchase or withdrawal. Before enabling trading, deploy
or identify reviewed contracts, pin their real code hashes, execute a successful
and failed Testnet purchase, retain the raw final RPC responses, and demonstrate
the worker's historical state checks and Supabase reconciliation against them.
The marketplace's former global seller-withdrawal accounting and missing failed
transfer refund were fixed on 2026-11-04 and are covered by unit tests, but no
deployed Testnet receipt has exercised the revised contract. Withdrawal/refund
promise-failure recovery is now verified and reconciled off-chain by this
worker, including failed storage withdrawals and registration-excess refunds.
Trusted listing publication and hosted Supabase validation remain release
blockers.
