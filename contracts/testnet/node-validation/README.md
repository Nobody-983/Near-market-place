# Isolated node validation tooling

**2026-11-03: infrastructure improved; real-node readiness is not established.**
All S01–S17 and L01–L03 remain `NOT_RUN`. The pinned node, TestLoop bridge and
stopped-store reader have **not compiled or executed** here. See the current
[report](reports/validation-20261103/REPORT.md), [machine-readable results](reports/validation-20261103/report.json)
and [case recipes](CASE_RECIPES.md). Earlier reports are retained unchanged.

The host's GLIBC remains 2.38. The incompatible prebuilt Sandbox is not used.
The selected route is the exact source commit, Rust 1.95.0 and the unchanged
release profile, including fat LTO. No alternative node version or relaxed
protocol checks are accepted.

## Verified infrastructure and remaining gaps

| Component | Current evidence |
| --- | --- |
| Pinned source | Archive checksum verified; genuine Git HEAD fetched; all 2,482 archive files/links match the checkout |
| Source overlays | Two fresh extractions reproduce identical overlay provenance; unchanged upstream files are checked individually |
| Rust signer | Native build, unit test, Clippy and formatting pass; offline signatures use fresh in-memory keys |
| Python tooling | 31 tests pass, including independent decoding of Rust-produced Borsh receipts and real Ed25519 signature verification |
| Sandbox recipes | S01–S17 implemented, integration unverified; no compatible built executable |
| L01/L02 | Bridge and launcher wiring implemented, uncompiled/unexecuted; support is not claimed |
| L03 | Individual authentic DataReceipt ordering/withholding remains missing; baseline execution cannot pass this case |
| Store exporter | Read-only reader source and Python isolation/replay implementation present; synthetic codec/mutation tests pass; real database recovery unverified |
| Contract regression baseline | 81 foundation tests plus 2 doctests, 53 NFT tests, 39 runtime tests pass; native mocks do not prove node rollback |

The dangerous former exporter scaffold has been replaced. It accepted key paths,
contained free-gas genesis settings and could emit `PASS` from case names without
reading a store. The replacement never starts a node or accepts keys. Original
user files were preserved in a task-start backup before targeted edits.

## Build inputs and resource admission

Exact node pins:

- nearcore `2.14.0-rc.5`, commit `b7b4737cab46f17a6daa2ed694d7aebbd204363a`;
- protocol **88**, Rust **1.95.0**;
- source URL: `https://codeload.github.com/near/nearcore/tar.gz/b7b4737cab46f17a6daa2ed694d7aebbd204363a`;
- archive SHA-256: `3c4254540a4bac8edf07c49ea07fcaf0bcbb59fe51afd2e37bad6ff036db3f99`;
- source Cargo.lock SHA-256: `0da292b89a32e53e4d4cb33e231f0f834ccfe777ea5e69ffddf4f731ff0b4223`.

`build_pinned.py` verifies every source file and known overlay, Rust/version pins,
Git HEAD for neard, native tools, OpenSSL metadata, free disk and available RAM.
Its **local conservative policy**, not an upstream minimum or measured peak,
requires **30 GiB free disk** (24 GiB build allowance + 6 GiB reserve) and **6 GiB
MemAvailable**. It uses one build job and stops compilation at the disk reserve.
It preserves the pinned release profile and rejects compiler/profile overrides.
No cache deletion or system GLIBC change is part of this procedure.

Latest admission measured about 29.6 GiB free, below that policy. Some preflights
also measured below 6 GiB available RAM. Therefore the large builds were not
started. The actual peak disk/RAM demand remains unknown. CMake 3.27.4 and its
small dependencies were downloaded with APT and extracted into `/tmp`, not
installed system-wide. Their hashes are recorded. Offline dependency resolution
also found missing `addr2line 0.24.2`; full nearcore dependencies were not downloaded
for a build that did not pass resource admission.

The verified unmodified checkout retained for this session is:

`/tmp/nft-validation-source-20261103/nearcore-b7b4737cab46f17a6daa2ed694d7aebbd204363a`

For a fresh machine, obtain that exact Git commit and the archive separately in
a network-enabled preparation stage. An archive alone reports `unknown` in
neard's Git version fields: do not forge those fields or weaken the version guard.
For the retained checkout, once resource headroom and dependencies are available:

```sh
python3 -B contracts/testnet/node-validation/build_pinned.py \
  --source /absolute/path/to/verified-nearcore \
  --archive /absolute/path/to/pinned-source.tar.gz \
  --output /absolute/path/to/new-build-report --build
```

The node command remains:

```sh
cargo +1.95.0 build --locked --release -p neard --features sandbox
```

The wrapper sets the native target explicitly to avoid this host's global Wasm
default. It records the actual executable digest and checks its version after a
successful build. **No node executable digest exists for this source build yet.**
A build/version check does not establish Sandbox operation; run S01 next.

## Signer and offline checks

Requires Linux x86_64, Rust **1.99.1**, Python **3.11+**, Python `cryptography`, and
Bubblewrap. `cryptography` is used only for offline Ed25519 verification. The
native signer compiler is separate from the node compiler; contract build pins
and ordinary Wasm artifacts remain unchanged.

```sh
bash contracts/testnet/node-validation/build.sh
python3 -B contracts/testnet/readiness.py
python3 -B -m unittest discover -s contracts/testnet -p 'test_*.py'
```

`build.sh` explicitly chooses the host target and builds offline with its own
lockfile. Its executable is under `target/x86_64-unknown-linux-gnu/debug/`.
It also runs the node-validation Python suite (root unittest discovery does not
recurse into this hyphenated directory). Fixtures are clearly synthetic or prior
VM evidence; no harness test is a real-node result.

## Isolated execution

```sh
python3 -B contracts/testnet/node-validation/run.py \
  --sandbox-bin /absolute/path/to/built/neard \
  --sandbox-sha256 ACTUAL_BUILT_BINARY_SHA256 \
  --output /absolute/path/to/new-evidence-directory \
  --window-seconds 3600 --cases S01 S02
```

Use fresh output directories. The finite lifetime is a local fixture choice.
Every independent variant uses a fresh genesis, unique
`nft-validation-local-<random-id>` chain ID and three fresh in-memory signers.
Local `.testnet` account aliases preserve contract validation and never refer to
public accounts. Deployment and initialization are one self-signed action batch;
only the two approved ordinary Wasm hashes are accepted. No public transaction,
wallet profile, keychain, Supabase connection or frontend change is needed.

The launcher stages allowlisted public inputs and explicit binaries. Bubblewrap
creates fresh network/PID/mount/user namespaces with only loopback communication,
no real home/repository mounts, and a cleared environment. The runner has no RPC
URL option or public fallback. Every submission checks process/binary identity,
chain/genesis, protocol 88 and the full saved runtime configuration. HTTP redirects
are rejected. The original `prepare_runtime.py` remains historical tooling; the
GLIBC overlay route is not used for this validation effort.

`--cases` selects IDs; other IDs stay `NOT_RUN`. Reports use only `PASS`, `FAIL`,
`INCONCLUSIVE`, `NOT_RUN`. Exit 0 means all selected cases passed, 1 means a case
failed or is incomplete, 2 means launcher/build admission failed. The outer agent
sandbox may deny Bubblewrap's netlink socket; disconnected Bubblewrap itself was
successfully checked outside that outer sandbox. There is no unisolated fallback.

## TestLoop preparation and execution

```sh
python3 -B contracts/testnet/node-validation/prepare_testloop.py \
  /absolute/path/to/pinned-source.tar.gz /absolute/path/to/new-overlay-directory
```

This creates a verified source tree plus a narrow test-crate entry point, bridge,
and `near-store` exporter binary. It changes no production runtime implementation
or dependencies. `nft-validation-source.json` records every overlay digest.
`build_pinned.py --component testloop` and `--component exporter` use this tree and
the same resource admission. The TestLoop build uses upstream's `test_features`
test-crate feature; it does not rebuild or replace the approved NFT/market Wasm.

After a successful TestLoop build, the same launcher accepts
`--testloop-bin /absolute/path/to/test-executable --testloop-sha256 ACTUAL_SHA256
--cases L01 L02 L03`. For an L-only run no Sandbox binary is required. No default
TestLoop executable is downloaded or selected.

L01/L02 withhold actual NFT-shard chunk endorsements through the pinned TestLoop
handler. The bridge records observed withheld messages; recipes require actual
pending state and, for L02, block timestamps crossing expiry. Those observations
have **not yet occurred here**. L03 explicitly reports its missing individual-data
control rather than inserting fabricated results. TestLoop remains a separate
simulated-network/clock evidence class, never standalone-node evidence.

## Evidence replay and stopped stores

Each variant retains public genesis/config/keys, a local manifest, raw RPC and
chunk data, finalized snapshots and a journal committed with file/directory fsync
before submission. Unknown outcomes never trigger automatic resubmission. Private
signer material is never emitted; node-owned ephemeral keys are deleted with the
fresh node home. If receipt evidence is incomplete, only stopped hot/cold databases
are retained, without sibling node/validator key files.

```sh
python3 -B contracts/testnet/node-validation/verify.py /path/to/variant/003-mint.json
```

The offline verifier checks signed transaction bytes and Ed25519 signatures,
initial action identity/gas/deposit/code hash, expected success/failure action
index, nonce consumption, graph reachability, snapshot consistency, raw storage,
owner accounting and finalized aggregate balance conservation. It derives data
requirements from action bodies and checks dependency IDs, producer/consumer
accounts and success/failure payloads. Clearing an input's claimed gap list cannot
hide missing DataReceipts. Nearcore's `DeployContract.code` RPC view contains a
base64 code **hash**; the signed transaction and stored Borsh contain actual code.

For an incomplete actual run, after the exporter is compiled:

```sh
python3 -B contracts/testnet/node-validation/rebuild_node_store_exporter.py \
  --exporter-bin /absolute/path/to/nft-receipt-exporter \
  --exporter-sha256 ACTUAL_SHA256 \
  --case-dir /path/to/variant --output /path/to/fresh-export
```

The launcher holds database writer locks to reject live stores, mounts databases
read-only in a disconnected namespace, and hashes their contents before/after.
The native reader checks the stored genesis identity, exports original Borsh
`Receipts` and `ProcessedReceiptIds`, and never loads node configuration or keys.
Python decodes the bytes independently, rejects RPC/store conflicts and replays
copied evidence. Original evidence and case verdicts remain unchanged. GC-only
metadata is not a processing/delivery observation; stored bodies alone do not
prove a delivery timestamp. Missing bodies remain `INCONCLUSIVE`.

This verifier establishes consistency within a captured local evidence bundle;
it is not a light client or an independent public-chain authenticity attestation.
Gas instruction-phase proof, S16 stopped-queue proof, L03 delivery control and
real-node execution remain open. Local success would not approve public policies,
enable marketplace trading or change public manifest review flags.
