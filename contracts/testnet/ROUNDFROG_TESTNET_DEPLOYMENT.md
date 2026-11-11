# RoundFrog Testnet deployment package

Status: the contracts are deployed to NEAR Testnet and independently verified by
the operator (`state_version: 2`, 1% fee, `trading_enabled: false`, deployed
hashes matching the reviewed artifacts). Trading, listing, purchase and minting
UI gates remain disabled. See
[TESTNET_E2E_READINESS.md](../../TESTNET_E2E_READINESS.md) for the current
verification and runbook.

## Account and permission architecture

| Responsibility | Account |
|---|---|
| Funding and subaccount creation | `roundfrog-dev.testnet` |
| NFT code account | `nft.roundfrog-dev.testnet` |
| Marketplace code account | `market.roundfrog-dev.testnet` |
| Marketplace owner/admin and fee recipient | `operator.roundfrog-dev.testnet` |
| Testnet mint authority and seller | `seller.roundfrog-dev.testnet` |
| Independent buyer | `buyer.roundfrog-dev.testnet` |

The NFT initializer deliberately requires a mint authority separate from both
contract accounts and permits only authority self-minting. The marketplace owner
controls fee policy, allowlisting and trading activation. The funding account
pays account creation and deployment, but is not granted application authority.

The planned child balances total 19 NEAR: 6 each for the two code accounts,
1 for the operator, and 3 each for seller and buyer. The currently observed
10 NEAR funding balance is insufficient. Top the funding authority up to at
least 22 NEAR before authorization so deployment gas and a parent reserve remain.

## Reviewed artifacts

| Contract | Bytes | SHA-256 | NEAR code hash |
|---|---:|---|---|
| NFT | 484725 | `b8a500297d6e93bc79c3f06834d09f31bf384816301d179d1aabb425a6a4a84d` | `DRmwjtfeBzXW2XgXMumytEY5scsDzEtCGsbVNKjdEtW8` |
| Marketplace | 475046 | `dc10f32c3680fa1d403baf5bbec2a71634d4e343b5a7a783a84a965aff94e846` | `Fp3ggF3WmSMQC7m9FU717jA8nB897sYW6bik7Ysqipw7` |

Paths:

- `contracts/nft/target/testnet/wasm32-unknown-unknown/release/near_nft_contract.wasm`
- `contracts/marketplace-deploy/target/testnet/wasm32-unknown-unknown/release/near_marketplace.wasm`

## Authorization sequence

1. Confirm the deployed accounts and code hashes still match this package, and
   inspect the funding account balance and keychain public key. Do not redeploy.
2. Confirm the hosted Supabase target is disposable development infrastructure;
   apply all six migrations in order and run hosted RLS/Auth smoke tests.
3. Rebuild both Testnet artifacts and confirm the hashes above.
4. Do **not** rerun `deploy_roundfrog_testnet.sh`; the contracts are already
   deployed. The script is retained for reference and would require explicit
   authorization for any future redeployment.

5. At a finalized block, verify both deployed code hashes, `marketplace_id`,
   `testnet_mint_authority`, marketplace owner/fee account, the NFT allowlist and
   `trading_enabled: false`.
6. Configure the frontend from `frontend/Nft/.env.example` and the two workers
   from `roundfrog.env.example` (or the per-service templates); never expose the
   service-role key to Vite.
7. Start the catalog indexer and settlement worker before minting. Record the
   final head as `CATALOG_INDEXER_START_HEIGHT` and `SETTLEMENT_START_HEIGHT` so
   no demo transaction is missed.
8. Fund marketplace storage for the seller, then run mint, approval/listing,
   cancellation, purchase, failure/refund, proceeds, fee, withdrawal, duplicate
   callback and stale-approval scenarios with raw final RPC evidence retained.
9. Keep `MINTING_ACTIONS_ENABLED`, `LISTING_ACTIONS_ENABLED` and
   `PURCHASES_ENABLED` false until that evidence and Supabase reconciliation are
   reviewed. Mainnet is out of scope.
