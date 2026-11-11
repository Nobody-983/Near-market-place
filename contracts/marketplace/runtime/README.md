# Disabled NEAR host-storage prototype

Milestone 3 adds [storage accounting](../STORAGE_ACCOUNTING.md): separate bounded
records, measured reservation consumption and combined backing checks. Financial
entry points remain absent; the ordinary ABI and all v1 encodings are preserved.

This separate runtime crate connects the existing offline byte format to NEAR
host storage. It implements bootstrap and inspection only. It is **not approved
for deployment or trading**. The parent foundation remains an offline `rlib`;
`OfflineCommand`, supplied results and supplied hashes are never transaction APIs
or receipt evidence. No NFT calls, purchases, payments, funding, recovery,
governance, migration or activation have been added.

## Compatibility and build

The existing v1 `STATE` value is a checksummed record envelope, not an SDK Borsh
contract struct. An ordinary `#[init]`/`&self`/`&mut self` SDK wrapper would conflict
with that root. All exports here are **receiverless** and bootstrap deliberately
does not use `#[init]`. The marker struct is never serialized into storage.
The SDK also generates a receiverless, read-only `contract_source_metadata` export.
The ordinary artifact's complete function ABI is therefore those three methods.

The older LookupMap suggestion in the runtime design is not used: the implemented
v1 byte format and this milestone's raw-storage constraint take precedence. Root
key `STATE`, record-family keys, envelopes, checksums and operation commitments
remain unchanged. The original additions were a pure empty-root batch factory
and public batch validation. Storage accounting now occupies its own namespace
without reinterpreting the existing v1 operating reserves.

Direct pins are near-sdk **5.30.0**, near-sys **0.4.0**, Borsh **1.8.1** and the
local foundation **0.1.0**. The independent runtime `Cargo.lock` pins transitive
dependencies without updating either existing contract's lockfile. In particular,
it resolves near-sdk-env 0.1.8, near-sdk-core 4.4.1 and near-account-id 3.0.0.
Native VM tests pin near-vm-runner/near-parameters/near-primitives-core **0.38.0**
and wasmparser **0.254.0**; the lockfile selects Wasmtime **45.0.3**. VM dependencies
are development-only and the VM feature rejects Wasm compilation.

`bash build.sh` enforces Rust **1.99.1**, locked/offline resolution, an explicit
testnet or production feature, and separate artifact directories. It changes to
this directory so Cargo loads `.cargo/config.toml`. Wasm flags include **`--cfg
near`**, required by the installed SDK environment crate to select real host
panic/hashing functions, and disable reference-types, multivalue, SIMD and relaxed
SIMD. A compile guard rejects Wasm without `cfg(near)`; common compiler/flag
overrides are rejected by the build script. Release builds use checked overflow,
panic abort, LTO and one codegen unit. This is a pinned local build procedure, not
a signed/hermetic release pipeline or deployment configuration.

No mode, both modes together, and production plus fault hooks fail compilation.
Test hooks exist only in the separately requested `testnet,test-hooks` artifact;
never deploy that artifact. They inject post-write aborts and a stale write set;
they accept no arbitrary keys, write sets or external results.

## Storage and bootstrap boundary

`HostStore` implements the existing `AtomicStore` trait with raw host operations.
It validates the opaque batch, checks every full-byte precondition **before**
writing, then eagerly writes exactly the provided bytes. The foundation retains
its maximum 16 touched keys and operation-specific bounds. No prefix translation,
whole-history load, `LookupMap`, cached flush or record reserialization is added.

Reads limit keys to 1,200 bytes and values to 16 KiB. The SDK's ordinary
`storage_read` allocates the complete value. A narrowly scoped unsafe
`near_sys::storage_read` call instead loads a dedicated host register, followed by
`env::read_register_bounded` before any guest value allocation. Host register
loading itself still consumes runtime resources; this is not a host-memory cap.
The rest of the crate denies unsafe code. All backend errors panic. There is no
ordinary `Err` return after a partial write. Receipt transaction rollback remains
the node runtime's responsibility, not an in-contract undo log.

`bootstrap({"manifest": "<base64 canonical Borsh DeploymentDomain>"})` requires:

- Host predecessor **and signer** equal the current contract account, no promise
  results, and zero attached deposit. This is temporary self-bootstrap authority,
  not an administrator or approved governance role.
- `STATE`, `nmp:runtime:bootstrap:v1` and `nmp:s:root:v1` absent. Empty, corrupt, oversized or
  legacy state is still existing state and must never be overwritten.
- Bounded, exact Borsh manifest consumption, valid distinct marketplace/NFT IDs
  and nonzero commitments; matching immutable artifact mode/current account; and
  the manifest's marketplace code hash equal to the host's executing **local**
  code hash. Global-contract deployments are intentionally rejected.
- Sufficient observed account balance for the resulting storage usage multiplied
  by the SDK byte-price constant. Bootstrap accepts no customer deposit and
  creates no liabilities. The account must already be funded; surplus is not
  recorded as free capital. This check does not implement reserve funding policy.

Bootstrap writes the canonical paused empty v1 root (revision 1, zero totals and
counters, payment policy `None`), plus a separate sidecar containing `NMPBOOT1`
and the exact domain bytes, followed by an empty `nmp:s:root:v1` accounting head.
Neither new key changes the v1 root codec. Status
validates the sidecar/domain/root and returns a checksum and `trading_enabled:
false`; it performs no storage writes and works in VM view mode.

These guards do **not** prove network/genesis, NFT code/ABI, manifest provenance,
account freshness, access-key removal, source history or receipt finality. Supplied
hashes are commitments only. No bounded prefix scan can prove the absence of
orphan records when all root/sentinel keys are missing; independent fresh-account/state
verification is required before any bootstrap. A corrupt/missing sentinel cannot
be repaired through this ABI. Existing offline fixtures are not imported. An
existing root without a sidecar, or bound to another code hash/domain, rejects
adoption. Code upgrades/domain changes require a separately reviewed migration
covering every retained record and invariant, not rerunning bootstrap. NFT state
and its separate historical burn/ID migration requirements are unchanged.

## Reproducing checks

From this directory, after dependencies and the Wasm target are installed:

```sh
cargo test --locked --offline --features testnet --lib
cargo test --locked --offline --features production --lib
cargo test --locked --offline --features testnet,test-hooks --lib
bash build.sh testnet
bash build.sh production
bash build.sh testnet test-hooks
cargo test --locked --offline --features testnet,vm-tests --test wasm_runtime -- --nocapture
cargo fmt -- --check
cargo clippy --locked --offline --all-targets --features testnet,vm-tests,test-hooks -- -D warnings
```

Native tests verify exact bootstrap parity with the offline root, raw storage,
bounded multi-record writes/reopen with a 1,024-byte token ID, maximum-size values
and bounded manifests. The existing foundation suites and all six independent
Python operation vectors must continue to pass unchanged.

Wasm tests execute the real compiled artifact using NEAR VM-runner with the
installed stable **protocol 88** configuration. They verify exports and required
host imports, v1 root preservation (271-byte empty root), view-mode inspection,
reinitialization/authentication/domain rejection, corrupt/partial roots, malformed
manifests, oversized stored values, write-set conflicts, post-write storage
underfunding and injected aborts after each bootstrap write. Fixtures provide VM
context; they do not prove a live deployment or authenticate historical receipts.

On this development machine, the custom `cc` wrapper omits `libc_nonshared.a`,
causing Wasmtime's `pthread_atfork` link failure. The installed archive can be
passed to the final **native test link only**:

```sh
cargo rustc --locked --offline --features testnet,vm-tests --test wasm_runtime -- -C link-arg=/home/acknowlege/.local/usr/lib/x86_64-linux-gnu/libc_nonshared.a
```

Run the resulting `target/debug/deps/wasm_runtime-<hash>` executable. This local
workaround changes neither Wasm flags nor library behavior. A normal development
installation with its complete libc development linker configuration should use
the standard `cargo test` command above.

**Rollback evidence limit:** NEAR sandbox/node is unavailable here. VM-runner's
`MockedExternal` retains writes even after `VMOutcome.aborted`; the tests expressly
assert this limitation rather than simulating transaction rollback. Real Wasm
abort behavior is tested; **receipt-level rollback is not verified**. Before
deployment, a real sandbox/node test must verify that failed bootstrap at both
write boundaries, existing-root writes, OOG and underfunding leave committed trie
state unchanged. Add multi-record failure/OOG cases at every write boundary and
promise-plus-state rollback when promises are implemented. No ignored/stub test
is counted as satisfying this acceptance gate.

Historical Milestone 1 verification, before storage accounting:
5 native prototype tests passed in each of
testnet, production and testnet-with-hooks; all 11 Wasm VM tests passed. The
foundation passed 58 tests plus 2 doctests in each of default/testnet/production;
6 independent encoding vectors, 21 existing NFT tests and all 5 JavaScript test
files passed (JavaScript requires `--experimental-vm-modules`). Formatting and
Clippy with warnings denied passed for the modified crates. Invalid artifact
combinations and Wasm missing `cfg(near)` rejected as expected. No sandbox/node
test, transaction or deployment was run.

Historical Milestone 1 hashes below are not the current accounting-extension
artifacts and are not deployment attestations:

| Artifact | SHA-256 |
| --- | --- |
| testnet | `caa82a5d4f4c165ea52217424ec08aee95f552d09efd410f20e291b550e05fb6` |
| production | `4f2999a19230ed63fd6ea400ee9e8269663285975852b489a611ec5c5fb727fb` |
| testnet,test-hooks — never deploy | `109ff72baa21ae70af34a7ac4cf375197b4c654797a4150f6d49058f32276f3c` |

## Remaining review and release gates

Review the receiverless ABI/root ownership, raw-register unsafe boundary,
bootstrap sidecar/schema, compiler/dependency pins and target protocol support.
Approve exact deployment identities/manifests and the atomic deploy/self-bootstrap/
verification/key-sealing procedure separately. There is no deployment script or
transaction execution in this milestone, and no migration/repair authority.

Storage staking, reserve attribution/headroom, gas/debit/refund timing and full
node atomicity need measurement before accepting customer funds. The actual NFT
approval-only wrapper/binding, authenticated cross-contract callbacks, finance
configuration, recovery/deposit-return policy, finalized-receipt verifier,
isolation of operator resources, audit and explicit release approvals remain
blocked/out of scope as documented in the architecture. No financial or governance
values have been selected. Both existing frontend trading gates remain false.
