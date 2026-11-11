# Approval-based NEAR marketplace

This package is the deployable native-NEAR fixed-price marketplace used by the
frontend and trusted settlement verifier. It is separate from the deliberately
disabled architecture prototype in `contracts/marketplace/runtime`.

No deployment is performed by this repository workflow. Fresh deployments
start with on-chain trading disabled, and the frontend gates in
`frontend/Nft/src/config/marketplace.js` also remain disabled.

## Architecture

- Sellers pre-fund NEP-145 marketplace storage, then sign an `nft_approve` call
  containing versioned terms that bind marketplace, NFT contract, token,
  seller, exact yoctoNEAR price, fee snapshot, fee recipient and expiration.
- `nft_on_approve` accepts only allowlisted NFT predecessors and the seller as
  original signer. Per-token approval IDs must increase; independent sale
  generations are immutable and retained as replay barriers.
- `buy_nft` requires the exact price, locks the listing before dispatch, records
  a pending sale and calls the bound NFT's `marketplace_transfer_approved`
  wrapper with the exact seller, token and approval ID.
- The private callback derives all accounting from stored state. Successful NFT
  delivery credits seller and fee balances. Failure creates a durable buyer
  refund; it never trusts callback-supplied amounts or browser confirmations.
- Seller, fee and buyer-refund withdrawals are debited into a single in-flight
  record. A failed transfer callback restores the exact source balance;
  duplicate or stale callbacks cannot pay twice. NEP-145 storage excess and
  storage withdrawals use the standard direct-refund pattern so an exact
  withdraw-all operation does not consume its own refundable balance.
- Listings, settlement records and NEP-297-style marketplace events retain an
  auditable history. Paginated views are capped at 100 records per call.
- The liability ledger includes pending sale principal, seller proceeds, fees,
  buyer refunds, refundable storage and in-flight withdrawals. Storage growth is
  charged before accepting listings, allowlist entries and withdrawals.

The bundled NFT implements NEP-171 core/enumeration/metadata and NEP-178
approvals plus a marketplace-bound exact-approval transfer wrapper. It does not
implement NEP-199 payout/royalty methods; this marketplace therefore settles the
configured fee and seller remainder only. Adding royalties requires a separate
reviewed interface and accounting change.

## Build and test

Rust 1.99.1 and `wasm32-unknown-unknown` are required. This workstation's global
Cargo configuration targets Wasm, so native tests must clear inherited rustflags:

```sh
CARGO_ENCODED_RUSTFLAGS='' cargo test \
  --manifest-path contracts/marketplace-deploy/Cargo.toml \
  --locked --target x86_64-unknown-linux-gnu

bash contracts/marketplace-deploy/build.sh testnet
bash contracts/nft/build.sh testnet
```

Rebuild and record the exact byte size, SHA-256 and NEAR base58 code hash before
any authorized deployment. Generated artifacts are under each package's
`target/testnet/wasm32-unknown-unknown/release` directory and are not deployment
evidence.

## Initialization and activation sequence

For a fresh account, initialize the marketplace with:

```json
{
  "nft_contract_id": "<NFT_CONTRACT_ID>",
  "owner": "<OWNER_ACCOUNT_ID>",
  "fee_bps": 100,
  "fee_account": null
}
```

After an explicitly authorized deployment:

1. Verify final account/code hashes and the NFT's immutable `marketplace_id`.
2. Register the owner and test sellers with `storage_deposit`.
3. Confirm `config().trading_enabled` is `false`.
4. Run listing, cancellation, expiry, purchase, failed-transfer, refund,
   proceeds, fee and failed-withdrawal flows on isolated Testnet accounts.
5. Pin the reviewed hashes in the trusted settlement worker and verify hosted
   Supabase reconciliation against final receipt graphs.
6. Only then authorize the owner to call `set_trading_enabled(true)` and enable
   the two frontend feature gates in a separate reviewed change.

For upgrades, deploy code only after draining outstanding callbacks, then invoke
the direct self-call `migrate`. Migration preserves legacy proceeds/fees but
always leaves trading disabled. Existing legacy listings must be re-approved
under versioned terms before activation.
