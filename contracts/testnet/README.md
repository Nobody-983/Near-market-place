# Testnet integration — local implementation; deployment blocked

The testnet artifacts support restricted minting and admission-owner storage
funding. Preparation on 2026-11-02 verified the pinned RPC genesis, public accounts,
keys and protocol costs, and measured the approved Wasm locally. CLI 0.23.3 is
installed. See [the preparation report](public/PREPARATION.md) and the explicitly
blocked [public manifest template](public/manifest.template.json). No credential
files or private keys were accessed. There have been **no deployments,
initialization transactions or live integration results**.
Local tests do not establish testnet readiness, receipt rollback or gas safety.

Both marketplace trading gates remain false. Public `nft_on_approve` is disabled.
There is no purchase, settlement, withdrawal, recovery or mainnet deployment step.

## Implemented testnet boundaries

- NFT `new_testnet(mint_authority_id, metadata, marketplace_id)` is exported only
  by the `testnet` feature. It requires a fresh root, predecessor=signer=current
  NFT account, zero deposit/results, separate authority/NFT/marketplace identities,
  and `.testnet` account names. It persists the immutable authority in the raw
  `nft:testnet:mint-authority:v1` sidecar and retains the BindingV2 root layout.
  Initialization writes/measures the entire root and authority storage; the SDK's
  final root write is identical. The dedicated NFT account must already be funded.
- `nft_mint(token_id, token_owner_id, token_metadata)` retains its existing ABI.
  In this testnet artifact, predecessor must be the configured authority and
  recipient must equal that authority. This self-mint restriction establishes
  recipient consent without selecting a general curated issuance policy. The
  existing mint code charges actual growth including permanent IDs and mutation
  sidecars and returns excess funding to the predecessor. Public minting stays
  unavailable. Default/production artifacts still reject **all** mint calls.
- Marketplace `fund_testnet_storage({"funding_nonce":"<u64>"})` requires the
  immutable admission-owner predecessor, no promise results, a positive attached
  deposit, and `.testnet` NFT/marketplace/owner identities. There is **no** recipient
  argument. Gross funding equals the host's exact attached yoctoNEAR amount.
  All growth of its account/credit metadata is measured and charged to that same
  deposit: `new available = previous available + received - storage_charged`.
  Credits cannot subsidize an existing deficit; funding does not clear holds.
- Funding is absent from production Wasm. SDK 5.30.0 prohibits payable receiverless
  generated methods; a small raw Wasm export bounds input to 128 bytes, strictly
  decodes one U64 string, authenticates the host predecessor, and never lets the
  SDK serialize its marker over `STATE`. After the first write every failure
  panics. Real NEAR receipt rollback remains a node-level acceptance requirement.
- Credit ID is SHA-256 of `nmp:testnet:storage-funding:v1\0`, deployment commitment,
  Borsh(owner), and little-endian nonce. A repeated nonce aborts before writes,
  including when a retry attaches more money. Do not choose a fresh nonce until
  the previous transaction's final outcome is known. No application refund is
  scheduled; failed-receipt refunds are handled by NEAR. Returned JSON fields are
  `owner_id`, `funding_nonce`, `received`, `storage_charged`, `available` (numbers
  are decimal strings). Funding has no fees or native withdrawal path.
- Accounting adds CreditKind tag 3 (`StorageFundingCharged(AccountId)`), with the
  same width as the original funding kind. Existing keys/values/vectors retain
  their encodings. Do not downgrade software over new records or reinterpret
  historical credits as new deposits. No migration of live accounts is supported.

## Human prerequisites — stop here until these exist

1. CLI 0.23.3 is installed and its version was verified during preparation. For a
   new workstation, install the reviewed CLI locally (the commands below are
   provided, **not executed**):

   ```sh
   cargo install near-cli-rs --version 0.23.3 --locked
   near --version
   near account import-account
   ```

   Use its interactive secure keychain or an independently reviewed hardware-key
   workflow **locally**. Never paste keys/seeds here, into shell arguments, logs,
   repository files, or a public manifest. The generated commands use
   `sign-with-keychain`. Installing CLI or comparing public keys does not prove
   custody. Establish that the local keys control the intended accounts.
2. The three accounts in the preparation template already exist, are empty and
   have sufficient funding for the measured candidate at the saved snapshot.
   Review their public keys and custody; repeat empty-state and funding checks
   immediately before deployment. Do not create replacements for this preparation.
   This runbook requires **two dedicated empty `.testnet` contract accounts**, plus
   the testnet mint authority/seller and immutable admission owner (those last two
   may be the same reviewed test account). Never select an existing contract
   account or migrate state.
3. Verify the CLI's `testnet` network profile points to exactly
   `https://rpc.testnet.near.org` and network name `testnet`. Do not rely on the
   profile's label alone. The Python tools query that fixed endpoint and verify
   synchronized chain ID + pinned genesis **before every RPC operation**, but
   they do not execute or inspect the CLI's signing/network configuration.
   Repeat the network/profile check before each manually executed command.
4. Review the preparation evidence for genesis height/hash, public access-key
   inventories, approved artifact hashes, protocol/SDK storage cost, and measured
   code/root stake. Explicit candidate deposits and gas budgets are now in the
   preparation template; their margins, test policy and independent node
   measurements still require review. They are not approved deployment defaults.
   Actual-growth mint funding does not provision future ownership/enumeration
   growth on transfers. This flow performs no transfer; purchases/transfer reserve
   policy still need separate approval and implementation.

## Build and verify actual exports

Run from the repository root:

```sh
bash contracts/nft/build.sh testnet
bash contracts/nft/build.sh production
(cd contracts/nft && cargo build --locked --offline --release --target wasm32-unknown-unknown)
bash contracts/marketplace/runtime/build.sh testnet
bash contracts/marketplace/runtime/build.sh production
bash contracts/marketplace/runtime/build.sh testnet test-hooks
python3 -B contracts/testnet/readiness.py
python3 -B -m unittest discover -s contracts/testnet -p 'test_*.py'
```

Rust 1.99.1, NEAR SDK/standards 5.30.0 and near-sys 0.4.0 stay pinned. Default,
production, testnet, and fault-hook artifacts are distinct; deploy only the ordinary
testnet artifact paths printed by the tool. The tool checks exact export allowlists
and SHA-256 digests. Never deploy the test-hook build. Contract code hashes must
match the approved bytes after deployment. Wasm presence/hash is not execution proof.

## Public manifest and explicit operator inputs

Create a new **public** JSON manifest outside credential directories:

- `network`: `testnet`; `rpc_url`: `https://rpc.testnet.near.org`.
- `genesis_height`, `genesis_block_hash`: independently reviewed genesis identity.
  The endpoint must serve that block; otherwise fail closed and review an archival
  strategy. An arbitrary recent block is not a genesis substitute.
- `accounts`: `nft`, `marketplace`, `admission_owner`, `seller`, all `.testnet`.
- `artifact_sha256`: approved 64-character lowercase hex `nft`/`marketplace` hashes.
- `expected_access_keys`: per role, an array of public `public_key` and `permission`
  entries exactly matching the finalized RPC access-key inventory.
- `flow`: supply every field in the following table. Monetary/u64 values should be
  decimal strings; no secrets or credentials belong in this object.

| Fields | Required meaning |
| --- | --- |
| `nft_metadata`, `token_metadata` | Standard existing NFT metadata JSON; bounded test content |
| `token_id`, `generation` | Unique permanent ID; this fresh-account flow requires generation `"1"` |
| `funding_nonce`, `funding_yocto` | Unique owner funding nonce and exact positive testnet deposit |
| `mint_deposit_yocto`, `approval_deposit_yocto` | Positive measured NFT storage deposits |
| `reserve_yocto`, `creation_bytes` | Admission allocation and maximum authorized creation growth |
| `first_call_gas`, `second_call_gas`, `callback_gas`, `local_gas`, `max_result_bytes` | Explicit measured verification budgets; gas in raw units |
| `transaction_gas` | Object containing raw gas per transaction stage: `nft-deploy`, `marketplace-deploy`, `fund`, `mint`, `approve`, `create`, `dispatch` |
| `price_yocto`, `expires_at_ns`, `fee_bps`, `fee_cap_bps`, `fee_account`, `policy_version`, `policy_hash`, `correlation` | Explicit **test-only** v1 admission fixture terms; hashes are nonzero 32-byte hex |

The frozen admission codec requires prospective price/fee fields even though no
commercial listing can be opened. Supply reviewed fixture values explicitly; this
does **not** configure real fees, treasury or financial governance. Missing fields
fail closed. Do not copy synthetic unit-test values as deployment defaults.

Admission usable funding must cover `reserve_yocto` **after** funding metadata is
charged. `reserve_yocto >= creation_bytes * storage_byte_cost`; `creation_bytes`
must cover the admission, lifecycle, reservation/guard, listing, operation, slot
and watermark. Test with measured margin. Dispatch/callback currently update
fixed-width records, consuming cursor 2/3 without adding stake. Reserved money is
not capital. There is no withdrawal or release path for completed/dispatched
reservations. Ready-only release retains replay records and charged stake.

The immutable deployment manifest commitment is SHA-256 of sorted compact UTF-8
JSON of the **entire** public manifest. The Borsh domain includes testnet byte 0,
genesis hash bytes, that commitment, both code hashes, and both contract accounts.
Do not edit any manifest field after marketplace initialization. If test terms
expire, this bounded runbook requires fresh accounts and a newly reviewed manifest.

## Exact deployment and integration commands

`flow.py` is a **read-only command generator and state verifier**. It never signs,
broadcasts or accesses credentials. Each invocation verifies artifacts, network,
finalized account/key state, deployed identity and initialization bindings before
printing one shell-quoted CLI command. Review and execute that printed command
locally; do not pipe it blindly to a shell. CLI syntax/action ordering was verified
against [near-cli-rs v0.23.3 deploy/init source](https://github.com/near/near-cli-rs/blob/v0.23.3/src/commands/contract/deploy/initialize_mode/call_function_type/mod.rs)
and its [signing options](https://github.com/near/near-cli-rs/blob/v0.23.3/src/transaction_signature_options/mod.rs).
Reconfirm installed CLI help/profile before use. `with-init-call` builds
DeployContract followed by FunctionCall in **one self-signed action batch**.

Run these individually, waiting for finalized action and receipt outcomes after
**each printed transaction**. Replace the manifest path with your public file:

```sh
python3 -B contracts/testnet/readiness.py --manifest /path/to/public-testnet.json --check-accounts
python3 -B contracts/testnet/flow.py --manifest /path/to/public-testnet.json --stage nft-deploy
python3 -B contracts/testnet/flow.py --manifest /path/to/public-testnet.json --stage marketplace-deploy
python3 -B contracts/testnet/flow.py --manifest /path/to/public-testnet.json --stage fund
python3 -B contracts/testnet/flow.py --manifest /path/to/public-testnet.json --stage mint
python3 -B contracts/testnet/flow.py --manifest /path/to/public-testnet.json --stage approve
python3 -B contracts/testnet/flow.py --manifest /path/to/public-testnet.json --stage inspect
python3 -B contracts/testnet/flow.py --manifest /path/to/public-testnet.json --stage create
python3 -B contracts/testnet/flow.py --manifest /path/to/public-testnet.json --stage dispatch
python3 -B contracts/testnet/flow.py --manifest /path/to/public-testnet.json --stage verify
```

- NFT deployment calls `new_testnet`; marketplace deployment calls
  `bootstrap_with_admission_owner`. Both initialization deposits are zero, accounts
  already funded, and neither step will generate an overwrite command.
- Funding signs as immutable admission owner. Mint and approval sign as authority/
  seller. Approval uses `msg:null` to avoid the disabled hook. JSON approval IDs
  from finalized NFT state are parsed with Python's full-width integers.
- Create encodes the exact v1 operation. Dispatch accepts its 32-byte admission
  locator only, zero deposit; owner identity is authenticated again on chain.
- The promise graph is `marketplace -> NFT.nft_token` AND `NFT.nft_is_approved`,
  joined in that order, then private `marketplace.on_nft_verified`. Never manually
  call the callback or inject result bytes/hashes.
- Verify reads one finalized block, validates exact operation/lifecycle/listing
  envelopes and checks `Completed`, internally `Active`, terminal verification,
  cursor 3 and reservation conservation. It rejects unknown/pending/invalid state.
  This is provisional ownership/approval verification **not an active sale**.
  Source-matched codecs and checksums detect corruption; trusted RPC responses are
  not cryptographic receipts. Preserve the full finalized receipt graph as well.

Archive the actual deployment/initialization transaction IDs (one batch each),
all later transaction IDs, success/failure outcomes and receipts, code hashes,
network identity, funding result and final state report. A top-level submitted or
successful transaction alone does not prove successful asynchronous callbacks.
No transaction hashes are supplied here because no transactions have been sent.

## Failure and recovery

- Failed initialization: inspect finalized action outcomes, code and state before
  retrying. Never reset storage, overwrite initialized accounts or migrate live state.
- Unknown funding: wait for finalized outcome; do not use a fresh nonce to retry
  blindly. Reusing a successful nonce aborts without allocating another credit.
- Ready admission may use the existing owner-only release; future credit remains
  attributed and retained records/stake are not refunded.
- Dispatched/missing/aborted callback stays pending and locked. Duplicate dispatch
  never resends the graph. No timeout, operator assertion or synthetic receipt can
  close it. Completed reservations are also retained. Recovery is not implemented.
- NFT legacy unversioned resolvers remain disabled. Any later upgrade needs
  independently proven receipt drain, complete history and a separately reviewed
  migration. Never assume live NFTs reveal burned IDs.
- Before listings or purchases: approve and implement real commercial policy,
  purchase-time exact approval authorization, reserves and payouts, then independently
  audit and test receipt rollback, ordering, OOG and unknown outcomes on a real node.
  Preserve frontend design/NEP-413, projection-only Supabase and no-royalty v1 scope.

## Current local validation — funding/mint milestone (2026-11-01)

| Suite | Result |
| --- | --- |
| Foundation default / testnet / production | 81 tests + 2 doctests passed per mode |
| NFT default / production | 51 native; 60 including Wasm, per mode |
| NFT testnet | 53 native; 62 including Wasm |
| Runtime testnet / testnet,test-hooks | 39 native; 61 including Wasm, per mode |
| Runtime production | 35 native; 56 including Wasm |
| Separate marketplace Wasm harness | 12 passed, including all three exact export allowlists |
| Independent frozen Python codec vectors | 6 passed |
| Readiness/flow Python tests | 8 passed |
| Repository JavaScript regressions | 5 test files passed |
| Rust formatting and Clippy with `-D warnings` | Passed in every supported native/VM feature mode |
| NFT default/production/testnet + marketplace production/testnet/test-hooks Wasm | All six builds passed |

Native command: `cargo test --locked --offline` plus each mode's feature selection.
Clippy: `cargo clippy --locked --offline --all-targets --features <mode>`
`-- -D warnings`; formatting: `cargo fmt -- --check`. Foundation/NFT default omit
`--features`; runtime has no default artifact mode. Build commands appear above.

Wasm harness command, from each contract crate:

```sh
cargo rustc --locked --offline --features testnet,vm-tests --lib --profile test -- -C link-arg=/home/acknowlege/.local/usr/lib/x86_64-linux-gnu/libc_nonshared.a
```

Then run the emitted executable under `target/debug/deps`. Repeat for production
and runtime testnet,test-hooks; NFT default uses `vm-tests` only. The separate
runtime harness substitutes `--test wasm_runtime` for `--lib`. The local link
workaround is native-only and never enters the Wasm flags.

Additional checks:

```sh
python3 -B contracts/marketplace/tests/encoding_vectors.py
python3 -B -m unittest discover -s contracts/testnet -p 'test_*.py'
node --experimental-vm-modules --test tests/*.test.mjs
```

New regressions exercise funding authorization, exact gross/net/staked attribution,
duplicate nonce/charge prevention, bounded JSON and full-width u64, undersized
funding, preserved holds/unattributed funds, restart, frozen Rust/Python operation
interoperability, and funding followed by admission/dispatch with synthetic callback
results. NFT cases exercise authority-only self-mint, network exclusions, funded
long IDs, duplicate IDs, immutable authority, frozen root layout and omitted
production exports. Initial test failures exposed a stale disabled-mint error
expectation, an incorrect test assertion that SDK enumeration never updates root
bytes, and an unsupported receiverless payable macro; these were corrected without
removing storage or authorization checks. Final supported suites have no failures.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| NFT testnet | 484725 | `b8a500297d6e93bc79c3f06834d09f31bf384816301d179d1aabb425a6a4a84d` |
| NFT default | 466789 | `19117a23c5f1b86898a71c77ce57fa33c9d7971848ce8df76fc6568cd903c70d` |
| NFT production | 466789 | `402496c0c120eaea85d3b38816d550a110878ada832a8b9509f2be46302bfa29` |
| Marketplace testnet | 696681 | `5f0b522306a87bc01beebe4c7575b8ae705785d38f003feae18eebfe2835708a` |
| Marketplace production | 689034 | `a52e614eb11678d41c943a429b4f41179dcbac2386a83cfbb8881644ffb0b0eb` |
| Marketplace test hooks | 718035 | `6bb7800aa6967527f6032cfa2afb9d96d99397627e04d7818de1db96275d47b9` |

Deployment/account/key-control status: **not verified on testnet**. Deployment and
initialization transaction IDs: **none**. Real mint/approval/admission/verification
flow: **not executed**. Node receipt rollback, protocol refunds, gas/OOG and live
cross-contract ordering remain unverified. All existing frontend/authentication
files, unrelated changes and the Git index were preserved against the inspection
baseline. No dependency version or lockfile changed.

## Exact files changed in this milestone

- `contracts/marketplace/runtime/NFT_RUNTIME_INTEGRATION.md`
- `contracts/marketplace/runtime/src/lib.rs`
- `contracts/marketplace/runtime/src/nft_integration/authority.rs`
- `contracts/marketplace/runtime/src/nft_integration/funding_tests.rs`
- `contracts/marketplace/runtime/src/nft_integration/tests.rs`
- `contracts/marketplace/runtime/src/nft_integration/wasm_tests.rs`
- `contracts/marketplace/runtime/src/testnet_funding.rs`
- `contracts/marketplace/runtime/tests/wasm_runtime.rs`
- `contracts/marketplace/src/persistence/storage.rs`
- `contracts/marketplace/src/persistence/storage/tests.rs`
- `contracts/marketplace/src/storage_accounting.rs`
- `contracts/nft/Cargo.toml`
- `contracts/nft/MARKETPLACE_BINDING.md`
- `contracts/nft/build.sh`
- `contracts/nft/src/lib.rs`
- `contracts/nft/src/testnet_mint.rs`
- `contracts/nft/src/testnet_mint_tests.rs`
- `contracts/nft/src/tests.rs`
- `contracts/nft/src/wasm_binding_tests.rs`
- `contracts/testnet/README.md`
- `contracts/testnet/flow.py`
- `contracts/testnet/readiness.py`
- `contracts/testnet/test_flow.py`
- `contracts/testnet/test_readiness.py`

## Historical validation evidence — before testnet funding/mint changes

Rust 1.99.1; existing pinned NEAR SDK/standards 5.30.0 and near-sys 0.4.0.
Dependencies, NFT root serialization and existing v1 operation vectors are unchanged.

| Check | Result |
| --- | --- |
| Foundation default / testnet / production | 79 tests + 2 doctests passed per mode |
| Runtime testnet / production / testnet,test-hooks | 55 per mode: 35 native + 20 Wasm tests |
| Separate marketplace Wasm regression harness | 12 passed; all three export allowlists checked |
| NFT native | 51 passed |
| NFT with vm-tests | 59 passed: 51 native + 8 Wasm tests |
| Independent Python operation vectors | 6 passed |
| Readiness tool unit tests | 4 passed |
| Repository JS regressions | 5 test files passed |
| Formatting, Clippy with warnings denied, whitespace check | Passed |
| NFT Wasm; runtime testnet / production / test-hook Wasm | All built successfully |

Native tests use `cargo test --locked --offline` with the relevant features. Wasm
harnesses use the repository's existing native-only link workaround:
`cargo rustc --locked --offline --features <mode>,vm-tests --lib --profile test --
-C link-arg=/home/acknowlege/.local/usr/lib/x86_64-linux-gnu/libc_nonshared.a`, followed
by the emitted test executable. NFT uses `vm-tests` alone. The separate runtime
harness uses `--test wasm_runtime`. The workaround is not a Wasm build option.

Initial failures were existing test expectations tied to public initialization,
unversioned restoration and consumption of unverified approvals. Those expectations
were updated to assert the stronger/new required behavior; no final failure remains.
New cases cover repeated long-ID refund cycles, empty-map conversion funding,
delayed/duplicate/forged resolvers, ownership ABA, revoke/reapproval, overflow,
corrupt versions, funded legacy version allocation, frozen-root/restart compatibility,
admission poisoning after release/invalidation and retained replay barriers.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| NFT `target/wasm32-unknown-unknown/release/near_nft_contract.wasm` | 466789 | `840d9f3f6ace07a27a3de3c5efd03f341bfc8634ceedbd72e6e0671bddc6d863` |
| Marketplace `target/testnet/wasm32-unknown-unknown/release/near_marketplace_runtime_prototype.wasm` | 681197 | `03ab64e7079966a8527596577f7c616503b372ddd0eae982f211126fff270dee` |
| Marketplace `target/production/wasm32-unknown-unknown/release/near_marketplace_runtime_prototype.wasm` (regression only) | 686801 | `7034c30ad6ba76f8ce40e3ee438937761c91b817ecee8f799dc43bbf018c56a2` |
| Marketplace `target/testnet,test-hooks/wasm32-unknown-unknown/release/near_marketplace_runtime_prototype.wasm` (fault testing only) | 704730 | `7bff73aba04d10d4656c1f1e8e90e569d0af5661217590354c00adc0c15ca54c` |

Hashes identify local artifacts only, not deployed code. No online account/key/
binding verification or mint/approval/admission transaction flow was run. Node-level
receipt rollback, actual callback gas sufficiency and live cross-contract behavior
remain unverified. No deployment/initialization transaction hashes exist.

The table and hashes above are archived evidence for the preceding security-fix milestone,
not hashes or test counts for the current testnet funding/mint artifacts. Current
local evidence is reported by the regression logs and readiness tool.
