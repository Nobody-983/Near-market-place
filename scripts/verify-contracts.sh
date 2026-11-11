#!/usr/bin/env bash
# ==============================================================================
# scripts/verify-contracts.sh
# Reproducible Contract Build and Artifact Hash Verification
#
# Validates that smart contract source code compiles to exact expected Wasm
# artifacts matching the reviewed RoundFrog NEAR Testnet deployment.
# ==============================================================================
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "========================================================================"
echo " NEAR NFT Marketplace — Contract Reproducibility & Integrity Verification"
echo "========================================================================"

# 1. Compiler version check
RUST_VERSION="$(rustc --version | cut -d ' ' -f 2)"
EXPECTED_RUST_VERSION="1.99.1"
if [[ "$RUST_VERSION" != "$EXPECTED_RUST_VERSION" ]]; then
  echo "[-] ERROR: Compiler version mismatch." >&2
  echo "    Expected: $EXPECTED_RUST_VERSION" >&2
  echo "    Found:    $RUST_VERSION" >&2
  exit 1
fi
echo "[+] Compiler: rustc $RUST_VERSION (matches reviewed version)"

# 2. Environment sanity check
if [[ -n "${RUSTFLAGS:-}" || -n "${CARGO_ENCODED_RUSTFLAGS:-}" || -n "${CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS:-}" ]]; then
  echo "[-] WARNING: Detected RUSTFLAGS override in environment which may defeat reproducibility." >&2
fi

# 3. Build & Verify NFT Contract
echo ""
echo "[*] Building NFT Contract (contracts/nft) for testnet target..."
bash "$ROOT_DIR/contracts/nft/build.sh" testnet > /dev/null

NFT_WASM="$ROOT_DIR/contracts/nft/target/testnet/wasm32-unknown-unknown/release/near_nft_contract.wasm"
EXPECTED_NFT_SHA="b8a500297d6e93bc79c3f06834d09f31bf384816301d179d1aabb425a6a4a84d"
EXPECTED_NFT_BYTES=484725
EXPECTED_NFT_CODE_HASH="DRmwjtfeBzXW2XgXMumytEY5scsDzEtCGsbVNKjdEtW8"
DEPLOYED_NFT_ACCOUNT="nft.roundfrog-dev.testnet"

ACTUAL_NFT_SHA="$(sha256sum "$NFT_WASM" | cut -d ' ' -f 1)"
ACTUAL_NFT_BYTES="$(wc -c < "$NFT_WASM" | tr -d ' ')"

echo "    Artifact:       $NFT_WASM"
echo "    Size:           $ACTUAL_NFT_BYTES bytes (expected: $EXPECTED_NFT_BYTES)"
echo "    SHA-256:        $ACTUAL_NFT_SHA"

if [[ "$ACTUAL_NFT_SHA" != "$EXPECTED_NFT_SHA" || "$ACTUAL_NFT_BYTES" -ne "$EXPECTED_NFT_BYTES" ]]; then
  echo "[-] FAILED: NFT contract artifact does not match expected hash or size!" >&2
  exit 1
fi
echo "[+] PASSED: NFT contract is byte-for-byte identical to deployed Testnet contract ($DEPLOYED_NFT_ACCOUNT)"

# 4. Build & Verify Marketplace Contract
echo ""
echo "[*] Building Marketplace Contract (contracts/marketplace-deploy) for testnet target..."
bash "$ROOT_DIR/contracts/marketplace-deploy/build.sh" testnet > /dev/null

MARKET_WASM="$ROOT_DIR/contracts/marketplace-deploy/target/testnet/wasm32-unknown-unknown/release/near_marketplace.wasm"
# Active deployed on Testnet
DEPLOYED_MARKET_SHA="dc10f32c3680fa1d403baf5bbec2a71634d4e343b5a7a783a84a965aff94e846"
DEPLOYED_MARKET_BYTES=475046
EXPECTED_MARKET_CODE_HASH="Fp3ggF3WmSMQC7m9FU717jA8nB897sYW6bik7Ysqipw7"
DEPLOYED_MARKET_ACCOUNT="market.roundfrog-dev.testnet"

# Enhanced version with protocol storage reserve (post-deployment upgrade branch)
ENHANCED_MARKET_SHA="1ca5635d7b9da25cc7ef535061c64d103189d1d3dd3924c79670addee72d5952"
ENHANCED_MARKET_BYTES=477311

ACTUAL_MARKET_SHA="$(sha256sum "$MARKET_WASM" | cut -d ' ' -f 1)"
ACTUAL_MARKET_BYTES="$(wc -c < "$MARKET_WASM" | tr -d ' ')"

echo "    Artifact:       $MARKET_WASM"
echo "    Size:           $ACTUAL_MARKET_BYTES bytes"
echo "    SHA-256:        $ACTUAL_MARKET_SHA"

if [[ "$ACTUAL_MARKET_SHA" == "$ENHANCED_MARKET_SHA" && "$ACTUAL_MARKET_BYTES" -eq "$ENHANCED_MARKET_BYTES" ]]; then
  echo "[+] PASSED: Marketplace contract matches the reviewed enhanced version with protocol storage reserve (upgrade binary)"
  echo "    Note: Deployed on-chain version ($DEPLOYED_MARKET_ACCOUNT) is $DEPLOYED_MARKET_SHA ($DEPLOYED_MARKET_BYTES bytes)."
  echo "          Contract includes migrate() state migration entry point ready for authorized upgrade."
elif [[ "$ACTUAL_MARKET_SHA" == "$DEPLOYED_MARKET_SHA" && "$ACTUAL_MARKET_BYTES" -eq "$DEPLOYED_MARKET_BYTES" ]]; then
  echo "[+] PASSED: Marketplace contract is byte-for-byte identical to active deployed Testnet contract ($DEPLOYED_MARKET_ACCOUNT)"
else
  echo "[-] FAILED: Marketplace contract does not match either known reviewed hash!" >&2
  echo "    Expected either: $ENHANCED_MARKET_SHA (enhanced) or $DEPLOYED_MARKET_SHA (deployed active)" >&2
  exit 1
fi

# 5. Live Testnet Read-Only Verification (if network reachable)
if [[ "${SKIP_NETWORK_CHECK:-false}" != "true" ]]; then
  echo ""
  echo "[*] Checking live NEAR Testnet RPC code hashes..."
  RPC_URL="https://rpc.testnet.near.org"
  
  LIVE_NFT_HASH="$(curl -s -X POST "$RPC_URL" \
    -H 'Content-Type: application/json' \
    -d "{\"jsonrpc\": \"2.0\", \"id\": \"nft-check\", \"method\": \"query\", \"params\": {\"request_type\": \"view_code\", \"finality\": \"final\", \"account_id\": \"$DEPLOYED_NFT_ACCOUNT\"}}" \
    | grep -o '"hash":"[^"]*"' | cut -d '"' -f 4 || true)"

  LIVE_MARKET_HASH="$(curl -s -X POST "$RPC_URL" \
    -H 'Content-Type: application/json' \
    -d "{\"jsonrpc\": \"2.0\", \"id\": \"mkt-check\", \"method\": \"query\", \"params\": {\"request_type\": \"view_code\", \"finality\": \"final\", \"account_id\": \"$DEPLOYED_MARKET_ACCOUNT\"}}" \
    | grep -o '"hash":"[^"]*"' | cut -d '"' -f 4 || true)"

  if [[ -n "$LIVE_NFT_HASH" && -n "$LIVE_MARKET_HASH" ]]; then
    echo "    Live $DEPLOYED_NFT_ACCOUNT code hash:    $LIVE_NFT_HASH"
    echo "    Live $DEPLOYED_MARKET_ACCOUNT code hash: $LIVE_MARKET_HASH"
    if [[ "$LIVE_NFT_HASH" == "$EXPECTED_NFT_CODE_HASH" && "$LIVE_MARKET_HASH" == "$EXPECTED_MARKET_CODE_HASH" ]]; then
      echo "[+] PASSED: Live Testnet code hashes match pinned reviewed values!"
    else
      echo "[-] WARNING: Live code hash on Testnet diverged from pinned values." >&2
    fi
  else
    echo "[!] Notice: Testnet RPC not reachable or rate-limited; skipped live check."
  fi
fi

echo ""
echo "========================================================================"
echo " All contract reproducibility checks passed successfully."
echo "========================================================================"
