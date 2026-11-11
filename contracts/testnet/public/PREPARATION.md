# Testnet preparation — 2026-11-02, deployment blocked

The three existing accounts have sufficient testnet funding for the measured
candidate below. Each holds **10 NEAR**. No top-up is indicated at the observed
protocol and prices. This is a funding assessment, not deployment authorization.
No contracts were deployed, no transactions signed or broadcast, no accounts or
keys modified, and no private keys, credential files or seed phrases accessed.
The frontend and contract implementation were not changed.

The completed **preparation template** is
[manifest.template.json](manifest.template.json). It includes all manifest fields,
actual account names, approved artifact hashes and observed public keys. It is
intentionally not an executable deployment manifest: `preparation.status` is
`blocked`, all review flags are false, and `expires_at_ns` is `null`.
`readiness.py` and `flow.py` reject it before RPC or command generation.

## Verified public network, artifacts and keys

Evidence: [rpc-snapshot.json](rpc-snapshot.json), collected at
2026-11-01 23:14:42 UTC (2026-11-02 in Lagos). All account observations use finalized
block **275815859**, hash `7cywvhbPHTYHLt6p1uyX3uZVFtLorYY1drzD7gsSQreo`.

- Pinned endpoint: `https://rpc.testnet.near.org`; chain ID `testnet`; synchronized.
- Genesis height **42376888**, obtained from `EXPERIMENTAL_genesis_config` and
  confirmed by the protocol configuration. The endpoint serves this genesis block.
- Genesis hash **`FWJ9kR6KFWoyMoNjpLXXGHeuiy7tEY6GmoFeCA5yuc6b`**, matching both
  the genesis block header and `status.genesis_hash`; its predecessor is zero.
  Genesis time: `2020-07-31T03:39:42.911378Z`.
- This establishes consistency through the pinned endpoint. Independent human
  review of that identity from a separately trusted source is still outstanding.
  A recent block was not substituted for genesis.
- Current protocol: **88**. The local near-vm-runner/parameters **0.38.0** also use
  protocol 88. Their serialized Wasm costs, action fees, storage cost and minimum
  gas purchase price match the snapshot configuration.
- CLI reports **`near-cli-rs 0.23.3`**. Its network profile and key custody have not
  been inspected. No credential/keychain inspection was performed.

| Role / account | Balance | Locked | Code / contract data | Access keys |
| --- | ---: | ---: | --- | --- |
| NFT: `ahmed-nft-contract.testnet` | 10 NEAR | 0 | No code, empty state | One FullAccess key |
| Marketplace: `ahmed-market-contract.testnet` | 10 NEAR | 0 | No code, empty state | One FullAccess key |
| Seller / mint authority / admission owner: `ahmed-market-operator.testnet` | 10 NEAR | 0 | No code, empty state | One FullAccess key |

Every account has 182 bytes of existing protocol account/key storage. Public keys:

| Account | Public key |
| --- | --- |
| NFT | `ed25519:7aZzudyFPeEQsEhyEZjU18obGrjWGaHzZHLHqPykrY5m` |
| Marketplace | `ed25519:4dGe1yqKT4yqfGQtxZMhok4sjrG2bAL71s9MhxYeycj6` |
| Operator | `ed25519:GYG6b4FufEMFQmtPNfaJ8x9jfqeBmZLKS4SSfKKvKceK` |

The template repeats the operator inventory for seller and admission-owner roles.
RPC key nonces are preserved in the evidence, not pinned in the manifest inventory,
because normal transactions advance nonces. Public visibility does not prove custody.

| Approved ordinary testnet artifact | SHA-256 | Bytes |
| --- | --- | ---: |
| NFT | `b8a500297d6e93bc79c3f06834d09f31bf384816301d179d1aabb425a6a4a84d` | 484725 |
| Marketplace | `5f0b522306a87bc01beebe4c7575b8ae705785d38f003feae18eebfe2835708a` | 696681 |

Actual bytes match the supplied approvals and exact export allowlists. The existing
readiness checks also validated default/production export separation. Ordinary
testnet artifacts were used unchanged; no test-hook artifact was selected.

## Storage and deposits

Protocol storage cost is **10000000000000000000 yoctoNEAR/byte** = **0.00001 NEAR/byte**,
matching the pinned SDK's `env::storage_byte_cost()`. Each new storage record adds
40 protocol bytes beyond key and value lengths. The VM measurements independently
reconcile reported storage growth with key/value changes plus that overhead.

| Item | Bytes | Storage NEAR |
| --- | ---: | ---: |
| NFT Wasm code | 484725 | 4.84725 |
| NFT initializer/root/metadata/authority growth | 380 | 0.00380 |
| NFT account + code + initialized state | 485287 | 4.85287 |
| Marketplace Wasm code | 696681 | 6.96681 |
| Marketplace bootstrap/root/owner growth | 1019 | 0.01019 |
| Marketplace account + code + initialized state | 697882 | 6.97882 |
| Exact fixture mint growth | 929 | 0.00929 |
| First marketplace approval growth | 188 | 0.00188 |
| First storage-funding metadata growth | 618 | 0.00618 |
| Admission creation growth | 3280 | 0.03280 |
| Dispatch / successful callback growth | 0 / 0 | 0 / 0 |

Minting measures all growth in `nft/src/lib.rs::mint_once`, including permanent
token-ID and mutation records, owner enumeration, ownership and metadata. Its
root update keeps the same width. The measured **9290000000000000000000** yoctoNEAR
deposit is sufficient for this exact first token/owner/metadata fixture.

Approval measures the complete first map and permanent counter: 112 + 76 = 188
bytes. The mutation sidecar already exists from minting. The measured deposit is
**1880000000000000000000** yoctoNEAR. This is greater than the method's additional
one-yocto minimum. Approval uses `msg:null`; it does not call `nft_on_approve`.
Both exact deposits succeeded in the second VM run without a refund promise.
These are fixture-specific amounts, not a universal mint/approval storage price.

Funding metadata is 341 bytes for the owner record plus 277 for the retained
credit record. Admission creation consists of slot 294, listing 461, verification
operation 715, watermark 286, lifecycle 191, admission 757, reservation 316 and
guard 260 bytes: **3280 total**. Existing roots/account totals update at fixed width.

The exact zero-margin admission minimum is 3280 authorized bytes and
**32800000000000000000000** yoctoNEAR. Funding that minimum after metadata requires
**38980000000000000000000** yoctoNEAR. The template instead proposes an explicit
**2× measured creation margin**, subject to approval:

- `creation_bytes = "6560"`.
- `reserve_yocto = "65600000000000000000000"` (0.06560 NEAR).
- `funding_yocto = "71780000000000000000000"` (0.07178 NEAR).
- Gross funding 0.07178 minus metadata 0.00618 leaves exactly 0.06560 available.
- Creation charges 0.03280 and leaves 0.03280 reserved. Dispatch/callback add no
  storage stake. The independent Python verifier read the actual VM-produced
  records and confirmed completed lifecycle, internal Active verification,
  cursor 3 and `staked + remaining = allocated`.

That verification occurred **only in the local VM**. Commercial trading remains
false. Neither completed allocation nor its replay records have a withdrawal or
release path in this flow. Approve that retained testnet allocation explicitly.
Transfer storage is outside this flow and is not provisioned by these deposits.

## Measured gas and proposed budgets

Gas below is Tgas (10^12 raw gas). Template values are exact raw integer strings.
Candidates use **2× probe gas burn, rounded up to whole Tgas**. Probe deposits were
larger to discover actual storage; mint/approval probe gas therefore also included
refund scheduling. The final exact-deposit run succeeded within these budgets.

| Call | Final local VM gas burnt, Tgas | Proposed prepaid Tgas |
| --- | ---: | ---: |
| NFT `new_testnet` | 2.793596 | 6 |
| Marketplace `bootstrap_with_admission_owner` | 2.950136 | 6 |
| Funding | 8.141446 | 17 |
| Mint | 2.060065 | 5 |
| Approval, null message | 1.325223 | 4 |
| Create admission | 16.214577 | 33 |
| Dispatch | 17.160702 | 106 |
| First dependency: `nft_token` | 0.871503 | 2 |
| Second dependency: `nft_is_approved` | 0.782825 | 2 |
| Callback | 15.583830 | 32 |

Dispatch uses 55.825375 Tgas including promised gas and host action charges.
`local_gas = 35 Tgas`; dispatch prepaid is child budgets 2 + 2 + 32 plus
2 × local 35 = **106 Tgas**. This covers the contract's
`children + local_gas + already_used_gas <= prepaid_gas` guard in the local run.
The returned NFT JSON was 418 bytes; `max_result_bytes = "836"` is the proposed
2× bound. It remains within the runtime's 5..16384 range.

The two **6 Tgas deployment values are initialization FunctionCall budgets**.
DeployContract has additional protocol action fees, derived from the actual Wasm
lengths and the self-signed send/execution rates:

- NFT deploy action: **34972082720675 gas** (34.972083 Tgas).
- Marketplace deploy action: **50102761655183 gas** (50.102762 Tgas).
- Initializer/receipt base and byte fees add about 1.197 Tgas to each batch.

The protocol currently allows up to 1000 Tgas total prepaid gas; the tooling's
stricter **300 Tgas** cap is preserved. The observed block burn price is
100000000 yoctoNEAR/gas, but protocol `min_gas_purchase_price` is **1000000000**.
Funding calculations conservatively use the higher purchase price for prepaid gas
and all action fees; they do not assume gas refunds or a future gas price.

| Account | Current NEAR | Storage + candidate gas/deposits, NEAR | Remaining headroom, NEAR |
| --- | ---: | ---: | ---: |
| NFT | 10 | 4.895040 | 5.104960 |
| Marketplace | 10 | 7.036121 | 2.963879 |
| Operator, all five later transactions | 10 | 0.255821 | 9.744179 |

Required amounts are rounded up for presentation. Exact integers, per-stage action
fee formulas and 300-Tgas sensitivity amounts are in
[funding-calculations.json](funding-calculations.json). New storage from deposits
is paid by those deposits, so it is not double-counted against contract prefunding.
Actual fees, trie access costs, receipt delivery, rollback and OOG behavior require
independent node measurement. Recheck funding and prices immediately before any
later, separately authorized deployment.

## Every manifest requirement and remaining decision

| Manifest field(s) | Prepared value / remaining decision |
| --- | --- |
| `network`, `rpc_url` | Verified fixed `testnet` and pinned HTTPS endpoint |
| `genesis_height`, `genesis_block_hash` | Verified above through RPC; independent identity review pending |
| `accounts.nft`, `.marketplace`, `.seller`, `.admission_owner` | Actual supplied accounts; distinct contract/authority roles validated; continued dedicated use and local custody review pending |
| `artifact_sha256.nft`, `.marketplace` | Supplied approved hashes verified against actual local files |
| `expected_access_keys` for all four roles | Exact finalized public keys/permissions above; operator must recognize them and independently establish custody locally |
| `nft_metadata`, `token_metadata` | Bounded text-only fixture included; approve exact content or replace and remeasure |
| `token_id` | Proposed permanent ID `prep-ahmed-20261102-01`; approve uniqueness/no reuse |
| `generation` | `"1"`, required for this fresh-account flow |
| `funding_nonce` | Proposed `"0"`, the lowest accepted U64; approve as first nonce only; never use a fresh nonce to retry an unknown outcome |
| `funding_yocto` | Measured overhead + proposed reservation = 0.07178 NEAR; allocation approval pending |
| `mint_deposit_yocto`, `approval_deposit_yocto` | Measured exact fixture values 0.00929 / 0.00188 NEAR; review and remeasurement after changes pending |
| `reserve_yocto`, `creation_bytes` | Proposed 2× measured creation storage, 0.06560 NEAR / 6560 bytes; retained-funding and margin approval pending |
| `first_call_gas`, `second_call_gas`, `callback_gas`, `local_gas` | Proposed measured-margin 2 / 2 / 32 / 35 Tgas; independent node measurement and approval pending |
| `max_result_bytes` | Proposed 836 = 2 × actual 418; approve bound for final metadata |
| `transaction_gas.nft-deploy`, `.marketplace-deploy`, `.fund`, `.mint`, `.approve`, `.create`, `.dispatch` | Proposed 6 / 6 / 17 / 5 / 4 / 33 / 106 Tgas; extra deployment action fees accounted separately; approval pending |
| `price_yocto` | Candidate `"1"`: minimum positive codec value, not an offer or valuation; approve test sentinel |
| `expires_at_ns` | **Unknown, null. Human must choose an absolute future U64 nanosecond expiry with enough integration time.** |
| `fee_bps`, `fee_cap_bps` | Candidates `"0"` / `"0"`: explicit no-fee fixture; contract requires `0 <= rate <= cap < 10000`; no real fee policy selected |
| `fee_account` | Candidate operator account; required codec identity, not a real treasury decision |
| `policy_version` | Candidate `"1"`, minimum accepted nonzero version; approve fixture version |
| `policy_hash` | SHA-256 of sorted compact UTF-8 JSON of `test-policy-candidate.json`; review the candidate document and regenerate after edits |
| `correlation` | SHA-256 of sorted compact UTF-8 JSON containing purpose `testnet preparation correlation v1`, accounts, token ID and policy hash; approve run binding |
| `preparation` | Additional public review annotations; keep blocked during preparation; final approved manifest must reflect resolved reviews before its commitment is frozen |

[test-policy-candidate.json](test-policy-candidate.json) explains each proposed
test value's basis in the actual codec. These are **candidates requiring human
approval**, not adopted policy or copied synthetic unit-test financial defaults.
No commercial fees, treasury, sale price, payment policy or governance is established.

Amounts are canonical decimal U128 strings; nonces/timestamps/gas/creation bytes
are U64, result bound U32, and fee fields U16. Token ID is nonblank and at most
1024 UTF-8 bytes. Hashes are nonzero 32-byte hexadecimal values. Metadata must pass
the pinned NFT standard checks. The actual numeric approval ID is deliberately
absent from this predeployment manifest: the later flow reads it from finalized
NFT state, rather than inventing live approval evidence.

The existing tools validate some fields only at their relevant stage. The new
preparation marker prevents this template from reaching even an early deployment
command with later decisions unfinished. Changing only its status cannot bypass
the false review flags or missing expiry. Removing that marker is not a substitute
for resolving the table. The entire final manifest, including annotations, is
committed during marketplace initialization and must never be edited afterwards.

## Exact remaining human actions before deployment

1. Independently review the genesis height/hash and recognize the three public
   key inventories. Establish custody locally using the approved secure workflow;
   provide no private key or credential contents here.
2. Verify installed CLI 0.23.3 help and its `testnet` profile's exact network name
   and RPC URL. The version is verified; profile/signing configuration is not.
3. Approve exact metadata, permanent token ID, first funding nonce, and the
   noncommercial policy candidate. **Choose the missing absolute expiry.**
   Recompute policy/correlation hashes when their source document changes.
4. Independently measure the final fixture on a reviewed node/receipt environment,
   including real gas, storage, atomic initialization/rollback, dependency ordering,
   failure/OOG and unknown outcomes. The local VM cannot establish these properties.
   This remains outstanding and is not authorized to be performed on these accounts
   during the current preparation-only task.
5. Approve exact deposits, gas budgets, result bound, storage margin and retained
   reservation with no withdrawal/recovery path. Recheck account balances, empty
   state, public keys, artifact hashes, network and protocol immediately before use.
6. Complete and review one final manifest, resolve the preparation marker, calculate
   its final commitment, and freeze it. Obtain separate deployment authorization.
   This task stops at preparation; no deployment or integration command was run.

## Reproduction and validation

Public read-only collector: `python3 -B contracts/testnet/snapshot.py --output <new-public-file>`.
It refuses to overwrite existing evidence and rechecks network/genesis before
each finalized query. It has no key, signing, transaction or account-management API.

Reproduce offline measurements against the saved snapshot from the repository root:

```sh
cargo rustc --manifest-path contracts/marketplace/runtime/Cargo.toml --locked --offline --features testnet,vm-tests --test preparation_measurements --profile test -- -C link-arg=/home/acknowlege/.local/usr/lib/x86_64-linux-gnu/libc_nonshared.a
python3 -B contracts/testnet/measure_local.py
python3 -B contracts/testnet/summarize.py
python3 -B -m unittest discover -s contracts/testnet -p 'test_*.py'
```

The driver regenerates candidate files; do not run it over a final approved/frozen
manifest. `probe-input.json` and `measurement-input.json` are explicitly **offline
inputs**. Their U64-max expiry is a fixed-width measurement sentinel, never a
proposed deployment expiry. Different final manifest hashes change commitments;
the measured envelope widths remain fixed, but final inputs still require review
and remeasurement. The candidate document remains blocked even after successful VM tests.

Validation: 12 Python tests and 6 independent codec vectors passed; both offline Wasm passes succeeded; the
independent Python finalized-record verifier accepted the VM-produced records;
Rust formatting and Clippy with warnings denied passed for the new harness;
readiness/export/hash checks passed. Added regressions cover preparation refusal,
genesis disagreement, wrong finalized query blocks, and the strict fee cap.
The harness checks the actual approved bytes and protocol cost compatibility and
retains per-record storage/gas evidence in [measurements.json](measurements.json)
and [probe-measurements.json](probe-measurements.json).

These results do not prove live execution, key custody, real receipt rollback,
callback delivery, OOG recovery, safe transfers or production readiness. No listing,
purchase, withdrawal, transfer or public NFT approval intake was enabled.
