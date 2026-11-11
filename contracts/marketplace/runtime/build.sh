#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
runtime_mode="${1:-}"
case "$runtime_mode" in
  testnet|production) ;;
  *) echo "Usage: bash build.sh testnet|production [test-hooks]" >&2; exit 2 ;;
esac
runtime_features="$runtime_mode"
if [[ "${2:-}" == "test-hooks" && "$runtime_mode" == "testnet" ]]; then
  runtime_features="testnet,test-hooks"
elif [[ -n "${2:-}" ]]; then
  echo "Fault hooks are allowed only in a separate testnet test artifact" >&2; exit 2
fi
if [[ $# -gt 2 ]]; then exit 2; fi
if [[ "$(rustc --version | cut -d ' ' -f 2)" != "1.99.1" ]]; then
  echo "This prototype requires the reviewed Rust 1.99.1 compiler" >&2; exit 2
fi
# Cargo's environment overrides can defeat a checked-in target configuration.
if [[ -n "${RUSTC:-}" || -n "${RUSTFLAGS:-}" || -n "${CARGO_ENCODED_RUSTFLAGS:-}" || -n "${RUSTC_WRAPPER:-}" || -n "${RUSTC_WORKSPACE_WRAPPER:-}" || -n "${CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS:-}" ]]; then
  echo "Remove compiler/flag overrides before a reproducible prototype build" >&2; exit 2
fi
cargo build --locked --offline --release --target wasm32-unknown-unknown --no-default-features --features "$runtime_features" --target-dir "target/$runtime_features"
