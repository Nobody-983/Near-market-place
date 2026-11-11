#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

runtime_mode="${1:-}"
case "$runtime_mode" in
  testnet|production) ;;
  *) echo "Usage: bash build.sh testnet|production" >&2; exit 2 ;;
esac
if [[ $# -ne 1 ]]; then exit 2; fi

if [[ "$(rustc --version | cut -d ' ' -f 2)" != "1.99.1" ]]; then
  echo "This contract requires the reviewed Rust 1.99.1 compiler" >&2
  exit 2
fi
if [[ -n "${RUSTC:-}" || -n "${RUSTFLAGS:-}" || -n "${CARGO_ENCODED_RUSTFLAGS:-}" || -n "${RUSTC_WRAPPER:-}" || -n "${RUSTC_WORKSPACE_WRAPPER:-}" || -n "${CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS:-}" ]]; then
  echo "Remove compiler/flag overrides before a reproducible contract build" >&2
  exit 2
fi

cargo build --locked --offline --release --target wasm32-unknown-unknown \
  --no-default-features --features "$runtime_mode" --target-dir "target/$runtime_mode"
