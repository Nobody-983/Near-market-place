#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
if [[ "$(rustc --version | cut -d ' ' -f 2)" != "1.99.1" ]]; then
  echo 'Signer tooling requires Rust 1.99.1; node source build separately requires 1.95.0.' >&2
  exit 2
fi
if [[ -n "${RUSTC:-}" || -n "${RUSTFLAGS:-}" || -n "${CARGO_ENCODED_RUSTFLAGS:-}" || -n "${RUSTC_WRAPPER:-}" || -n "${RUSTC_WORKSPACE_WRAPPER:-}" ]]; then
  echo 'Remove compiler/flag overrides for the standalone signer build.' >&2
  exit 2
fi
# A user-wide Cargo target may be wasm32. This helper is a native executable.
# Keep one explicit host target for build, tests, Clippy and runner discovery.
signer_host="$(rustc -vV | sed -n 's/^host: //p')"
export CARGO_ENCODED_RUSTFLAGS=''
cargo build --locked --offline --target "$signer_host"
cargo test --locked --offline --target "$signer_host"
cargo clippy --locked --offline --target "$signer_host" --all-targets -- -D warnings
cargo fmt -- --check
python3 -B -m unittest discover -s . -p 'test_*.py'
