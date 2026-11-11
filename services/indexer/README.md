# Trusted NEAR catalog indexer

This worker scans finalized NEAR blocks for transactions sent to the pinned NFT
and marketplace contracts. It verifies canonical receipt outcomes, reviewed code
hashes and historical contract state before invoking service-role-only Supabase
RPCs for mint binding, listing publication and cancellation.

## Safe local dry-run (no hosted Supabase writes)

Do **not** start this worker against the hosted Supabase project until the user explicitly approves after reviewing RPC endpoint, contract IDs, pinned code hashes, `CATALOG_INDEXER_START_HEIGHT`, durable checkpoint path, and server-side `SUPABASE_SERVICE_ROLE_KEY` handling.

Safe local verification:
```sh
node --test --experimental-vm-modules tests/catalog-indexer.test.mjs
```
For any manual run, use a non-hosted/test Supabase target only with explicit approval and never put secrets in `VITE_*` or committed files.

Copy [`catalog-indexer.env.example`](catalog-indexer.env.example) outside version
control, fill `SUPABASE_SERVICE_ROLE_KEY` locally, and run it before submitting
demo transactions so the initial checkpoint cannot miss them:

```sh
set -a; . ./services/indexer/catalog-indexer.env; set +a
node services/indexer/index.js
```

It uses the same `NEAR_*`, `SUPABASE_URL` and `SUPABASE_SERVICE_ROLE_KEY`
variables as the settlement worker, plus optional
`CATALOG_INDEXER_PROCESSED_PATH`. For a controlled first backfill, set
`CATALOG_INDEXER_START_HEIGHT` to the first finalized height that must be
scanned (usually the finalized head recorded immediately before the first demo
transaction); without it, an empty checkpoint starts 100 blocks behind the
current final head. A checkpoint, once written, always wins over the start
height. The height must be between 1 and the current final head. The RPC must retain every block after the
saved checkpoint; use an archival provider for recovery beyond the provider's
retention window. Local checkpoints are advisory. Supabase transaction locks and
unique indexes are the authoritative replay boundary.

The worker never accepts a catalog ID, listing ID, owner or price from the
browser. Mint catalog IDs are committed as `catalog-<uuid>` token IDs. Listing
facts come from the seller-signed `nft_approve` message, marketplace event and
final historical listing/NFT approval views.

Every poll verifies the RPC network and sync status, confirms a saved checkpoint
is still canonical, refuses to cross pruned history, and advances the checkpoint
only after every matching transaction in the block has either reconciled or been
cryptographically rejected. RPC and database failures leave the block pending
for retry. The HTTPS RPC is a trust root: this worker does not verify NEAR Merkle
proofs and should use a reputable archival endpoint for recovery.
