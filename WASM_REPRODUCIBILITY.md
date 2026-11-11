# NEAR NFT Marketplace — WASM Artifact Reproducibility & Specification

**Date:** 2026-11-09  
**Repository:** `~/Desktop/nft-platform`  
**Network:** NEAR Protocol Testnet (`testnet`)  
**RPC Endpoint:** `https://rpc.testnet.near.org`

---

## 1. Executive Summary

This document establishes the exact, verifiable build environment, compiler toolchain, target flags, and SHA-256 hashes for all smart contract WebAssembly (WASM) artifacts across the RoundFrog NEAR NFT Marketplace.

Drift detection and build verification are automated via [`scripts/verify-contracts.sh`](scripts/verify-contracts.sh).

---

## 2. Compiler Toolchain & Target Configuration

All contracts are compiled using the reviewed Rust compiler release:

```
rustc 1.99.1 (e3c78864a 2026-02-12)
cargo 1.99.1 (f3f2716a5 2026-02-12)
Target: wasm32-unknown-unknown
Profile: release (opt-level = "s", lto = true, codegen-units = 1, panic = "abort", overflow-checks = true)
```

### Critical Target-Feature Isolation (`.cargo/config.toml`)

To ensure byte-for-byte reproducibility across developer workstations with custom global Cargo configs (e.g. `~/.cargo/config.toml` injecting `+atomics`), each contract crate specifies its own local `.cargo/config.toml`:

```toml
[target.wasm32-unknown-unknown]
rustflags = [
  "--cfg", "near",
  "-C", "target-feature=-reference-types,-multivalue,-simd128,-relaxed-simd,-atomics"
]
```

---

## 3. Contract Artifacts & On-Chain Pinned Records

### A. NFT Contract (`near-nft-contract`)

* **Crate Path:** `contracts/nft`
* **Target Build Script:** `bash contracts/nft/build.sh testnet`
* **Underlying Cargo Command:**
  ```bash
  cargo build --locked --offline --release --target wasm32-unknown-unknown \
    --no-default-features --features testnet --target-dir target/testnet
  ```
* **Output Artifact:** `contracts/nft/target/testnet/wasm32-unknown-unknown/release/near_nft_contract.wasm`
* **Byte Size:** `484,725` bytes
* **Artifact SHA-256:** `b8a500297d6e93bc79c3f06834d09f31bf384816301d179d1aabb425a6a4a84d`
* **Deployed Account ID:** `nft.roundfrog-dev.testnet`
* **On-Chain Code Hash (Base58):** `DRmwjtfeBzXW2XgXMumytEY5scsDzEtCGsbVNKjdEtW8`
* **Verification Status:** **100% Byte-for-byte identical to live on-chain bytecode.**

### B. Marketplace Contract (`near-marketplace`)

* **Crate Path:** `contracts/marketplace-deploy`
* **Target Build Script:** `bash contracts/marketplace-deploy/build.sh testnet`
* **Underlying Cargo Command:**
  ```bash
  cargo build --locked --offline --release --target wasm32-unknown-unknown \
    --no-default-features --features testnet --target-dir target/testnet
  ```
* **Output Artifact:** `contracts/marketplace-deploy/target/testnet/wasm32-unknown-unknown/release/near_marketplace.wasm`
* **Deployed Account ID:** `market.roundfrog-dev.testnet`
* **Active On-Chain Version:**
  - **Code Hash (Base58):** `Fp3ggF3WmSMQC7m9FU717jA8nB897sYW6bik7Ysqipw7`
  - **Artifact SHA-256:** `dc10f32c3680fa1d403baf5bbec2a71634d4e343b5a7a783a84a965aff94e846`
  - **Byte Size:** `475,046` bytes
  - **State Version:** `2`
* **Enhanced Upgrade Binary (Working Tree):**
  - **Artifact SHA-256:** `1ca5635d7b9da25cc7ef535061c64d103189d1d3dd3924c79670addee72d5952`
  - **Byte Size:** `477,311` bytes
  - **Enhancements:** Protocol storage reserve (`control_storage_reserve`), registration excess withdrawal handling (`RegistrationExcess`), zero-balance storage withdrawal protection, and `migrate()` state upgrade method.
* **Verification Status:** **Both deployed bytecode and local upgrade binary are fully accounted for, reproducible, and guarded.**

---

## 4. Verification Instructions

To reproduce and verify both contract artifacts independently:

```bash
# 1. Run the authoritative verification script
bash scripts/verify-contracts.sh

# 2. Check byte equality against on-chain code directly:
curl -s -X POST https://rpc.testnet.near.org \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc": "2.0", "id": "check", "method": "query", "params": {"request_type": "view_code", "finality": "final", "account_id": "nft.roundfrog-dev.testnet"}}' \
  | jq -r '.result.code_base64' | base64 -d > /tmp/live_nft.wasm

cmp contracts/nft/target/testnet/wasm32-unknown-unknown/release/near_nft_contract.wasm /tmp/live_nft.wasm
# Returns 0 / identical output
```
