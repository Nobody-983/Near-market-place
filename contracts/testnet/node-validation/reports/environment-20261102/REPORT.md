# Validation tooling check — 2026-11-02

**No node execution results. All 20 planned cases are NOT_RUN.**
See [report.json](report.json) for each case and
[launcher.json](launcher.json) for source, lockfile, binary and public-input hashes.

| Check | Result / evidence |
| --- | --- |
| Standalone Rust helper build | PASS; `build.sh`, locked/offline, Rust 1.99.1 |
| Rust safety unit tests | PASS, 1 test with multiple prohibited-action/account/gas cases |
| Python tooling regressions | PASS, 15 tests; synthetic/archived-VM fixtures and disposable offline signatures |
| Rust formatting and Clippy | PASS, warnings denied |
| Existing readiness/flow/snapshot regressions | PASS, 12 tests |
| Approved ordinary artifact SHA-256 and exports | PASS, offline readiness check |
| Public manifest unchanged | PASS; SHA-256 `f5ebc7d2c95c2711ca6a675e92252143fc5cec830770710d8c3c28837a5a96e5` |
| Bubblewrap isolation capability | PASS outside the outer agent sandbox; disconnected namespaces created |
| Pinned official Sandbox executable | BLOCKED: requires `GLIBC_2.39`; host `/usr` has GLIBC 2.38 |
| S01–S17 real runtime cases | NOT_RUN: loader fails during `--version`, before `init` |
| L01–L03 TestLoop cases | NOT_RUN: pinned TestLoop build and delivery adapter absent; adapter not implemented |

The observed official executable digest is
`f0104897d7bc9c9a9f1d869679d2c0b4dca878a53327a4f1bc6901cfdf55699b`.
Its exact runtime-reported version/commit/protocol could not be observed because
the dynamic loader failed first. The runner contains mandatory checks for them.

The implementation is incomplete with respect to the full requested scope:
S01–S17 recipes have not been integration-tested; L01–L03 still need their real
TestLoop scheduling/delivery backend. A read-only stopped-node receipt exporter
also remains necessary if RPC/chunk history cannot expose initial/data receipt
bodies. Missing evidence cannot result in PASS.

No public RPC was used by the runner. Downloads were official software/source
research only. No node initialized, no node transactions were broadcast, no
contracts were deployed, and no existing private keys/keychains were accessed.
Only disposable offline signing fixtures ran in harness tests. All new repository
files are under `contracts/testnet/node-validation/`; existing contracts, frontend,
public preparation inputs and safety gates were left unchanged.

To proceed, use a GLIBC 2.39+ isolated host or build the exact node source with
Rust 1.95.0, then execute the recipes and resolve failures without weakening
checks. Full build/run/replay commands and the remaining tooling work are in
[the tooling README](../../README.md).
