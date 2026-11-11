# Validation infrastructure progress — 2026-11-03

**All S01–S17 and L01–L03 remain NOT_RUN. No node started, no local deployment
occurred, and no public transaction was broadcast. Testnet readiness is not yet
established.** See [report.json](report.json) for every case and blocker.

## Implemented and verified

- Exact source archive SHA-256
  `3c4254540a4bac8edf07c49ea07fcaf0bcbb59fe51afd2e37bad6ff036db3f99`;
  genuine Git HEAD `b7b4737cab46f17a6daa2ed694d7aebbd204363a`; 2,482 archive
  files/links checked. The source pins Rust 1.95.0 and release fat LTO. The installed
  compiler reports 1.95.0. This is source/toolchain verification, not a successful build.
- Two fresh overlay extractions reproduced the same source/overlay file hashes.
  Extra or altered build inputs fail source verification. Overlay application
  does not change production runtime files or dependency lockfiles.
- Native Rust signer build, safety unit test, Clippy and formatting passed.
  The host-wide Wasm default no longer breaks the signer build.
- **31 Python tooling tests passed**, including Ed25519 signature corruption,
  hidden missing DataReceipts, payload/identity mismatches, receipt graph failures,
  Borsh/RPC view parity with the Rust encoder, truncated exports, source tampering,
  key/link rejection and batched TestLoop pipe responses. These use synthetic
  fixtures and archived VM records, not node outcomes.
- Existing native testnet regressions: **81 foundation tests plus 2 doctests,
  53 NFT tests, 39 marketplace runtime tests** passed. Logs are retained alongside
  this report. No contract source or approved Wasm was changed.
- Bubblewrap disconnected namespaces successfully ran `/bin/true` outside the
  outer agent sandbox, which otherwise denies the netlink socket.

## Implemented but not verified against a node

The runner now preserves hot and cold stopped stores, explicitly saves outcomes
and receipt indexes, fsyncs journals before submission, validates initializer
failure action indices, and uses the documented four verdicts. The offline
verifier validates actual Ed25519 signatures and derives required data IDs from
receipt bodies; an empty supplied gap list cannot conceal missing dependencies.
It also binds initial action gas, deposits, code hashes and signer nonces.

The old exporter scaffold was unsafe and nonfunctional: it accepted key paths,
contained free-gas genesis changes, did not recover receipt bodies and could
mark a named case PASS without execution. Its replacement accepts only public
case evidence and a pinned native reader, locks stopped databases, mounts them
read-only in a disconnected namespace, checks their genesis identity, preserves
raw Borsh and database hashes, and replays copied evidence independently. GC
metadata is excluded from processing evidence. **The native reader has not been
compiled or run against a real stopped store.** Codec tests are not that proof.

L01/L02 bridge/controller/launcher wiring is present. Source review fixed a
borrowed future that could not meet TestLoop's static lifetime, and the adapter
now counts genuinely withheld endorsement messages. Recipes require observed
pending state and actual block timestamps. **The bridge has not compiled or run;
TestLoop support is not claimed.** L03 individual DataReceipt control is missing.

S10 cannot establish an internal post-write OOG instruction location. S16 now
refuses to treat a pre-stop pending snapshot as proof that a callback remained
queued at shutdown. These coverage limits cannot become PASS through a successful
baseline or process restart. See [all recipes](../../CASE_RECIPES.md).

## Build admission and environment limits

The new build admission uses a conservative **local policy** of 30 GiB free disk
(24 GiB build allowance plus 6 GiB reserve), 6 GiB available memory, and one build
job. These are not upstream minimum requirements or measured peak usage.
The latest checks had about **29.6 GiB free**, below the disk margin. TestLoop and
exporter checks also fell below the available-memory margin. The large builds
were therefore **not started**. Actual build disk/RAM peaks remain unmeasured.

CMake 3.27.4 and small runtime dependencies were downloaded through APT and
extracted into `/tmp`; [package hashes](build-dependency-packages.json) are saved.
No system packages or GLIBC were replaced. Offline nearcore dependency resolution
stops at absent `addr2line 0.24.2`; [the diagnostic](dependency-resolution.log) is
retained. Full build dependencies were not downloaded after resource admission
failed. The original incompatible prebuilt Sandbox was not substituted or used.

[Node](neard-build.json), [TestLoop](testloop-build.json), and
[exporter](exporter-build.json) records contain source/lockfile hashes, exact
commands, compiler output, profile, measured resources and blockers. Their
executable SHA-256 fields are **null**, because no executable was produced.

## Scope and reproducibility

All pre-existing changes on branch `ags/rebuild-node-evidence` were preserved.
The UI, wallet authentication, Supabase/RLS, contract sources and public approval
flags were not edited. Twelve public-input/artifact hashes match the task-start
baseline. No existing wallet or node credentials were read. Tooling signers used
fresh in-memory test keys only. Network access was limited to source and build
package downloads; the validation runner did not contact public RPCs.

The offline verifier checks consistency of locally captured evidence. It does
not cryptographically establish public-chain authenticity or replace an
independent audit. Full case acceptance additionally requires the recipe-specific
evidence; passing transaction bundles alone cannot certify an entire case.

Run commands and retained source paths are in the [tooling README](../../README.md).
With sufficient headroom, complete the exact pinned build, record its actual
executable digest, execute S01/S02 first, then resolve integration failures and run
the remaining recipes. Compile and exercise the exporter and TestLoop separately.
No local result automatically authorizes public deployment or trading.
