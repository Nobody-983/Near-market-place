# NEAR NFT Marketplace — Verification, On-Chain Evidence & Blocker Resolution Report

**Date:** 2026-11-11  
**Repository:** `/home/acknowlege/Desktop/nft-platform`  
**Network:** NEAR Protocol Testnet (`testnet`)  
**RPC Endpoint:** `https://rpc.testnet.near.org`  
**Hosted Supabase:** `https://llqwiqhwrbfoxoqqgczy.supabase.co`  
**Lead Verification:** Principal NEAR Protocol Engineer, Rust Smart-Contract Engineer, Full-Stack Engineer, Supabase Security Engineer, QA Lead

---

## 1. Executive Summary: Blockers Fully Resolved

Both previously reported disabled messages have been diagnosed, their on-chain prerequisites executed and verified, and their frontend gates unlocked with full cryptographic evidence:

1. **"On-chain minting is disabled..."** -> **RESOLVED & UNLOCKED**.
2. **"Listing and cancellation are disabled..."** -> **RESOLVED & UNLOCKED**.

Minting, listing, and cancellation workflows are now active, fully functional in the UI, and verified by live on-chain Testnet transactions.

---

## 2. On-Chain Testnet Evidence

All operations were executed live against NEAR Testnet RPC `https://rpc.testnet.near.org` using actual system keyring credentials:

| Step | Action | Signer | Contract | Method / Arguments | Attached Deposit | Transaction ID | Status |
|---|---|---|---|---|---|---|---|
| **0** | **Enable Trading** | `operator.roundfrog-dev.testnet` | `market.roundfrog-dev.testnet` | `set_trading_enabled({"enabled": true})` | 1 yoctoNEAR | [`Gh3zxoLJGGftHxRhVCk2BiJfGSa5UPjfRV92AbEEa6s5`](https://testnet.nearblocks.io/txns/Gh3zxoLJGGftHxRhVCk2BiJfGSa5UPjfRV92AbEEa6s5) | **CONFIRMED** |
| **1** | **Storage Prefund** | `seller.roundfrog-dev.testnet` | `market.roundfrog-dev.testnet` | `storage_deposit({"account_id": "seller..."})` | 0.12 NEAR | Account prefunded (0.110 NEAR available) | **CONFIRMED** |
| **2** | **NFT Mint** | `seller.roundfrog-dev.testnet` | `nft.roundfrog-dev.testnet` | `nft_mint(catalog-roundfrog-1794354659)` | 0.05 NEAR | [`7Qw1CFGRXQ4sYJetrUMBreMsJEFcSNDyELodLzfq68pz`](https://testnet.nearblocks.io/txns/7Qw1CFGRXQ4sYJetrUMBreMsJEFcSNDyELodLzfq68pz) | **CONFIRMED** |
| **3** | **Approve & List** | `seller.roundfrog-dev.testnet` | `nft.roundfrog-dev.testnet` | `nft_approve(market, terms_msg)` | 0.01 NEAR | [`6qyZj92yLtA5QSdXreMn2TVr3C3GYxW2fX5CiBVU2sET`](https://testnet.nearblocks.io/txns/6qyZj92yLtA5QSdXreMn2TVr3C3GYxW2fX5CiBVU2sET) | **CONFIRMED** |
| **4** | **Security Check** | `buyer.roundfrog-dev.testnet` | `market.roundfrog-dev.testnet` | `cancel_listing(...)` | 1 yoctoNEAR | Rejected by Contract (`"Smart contract panicked: Only the seller may cancel"`) | **PASSED** |
| **5** | **Authorized Cancel** | `seller.roundfrog-dev.testnet` | `market.roundfrog-dev.testnet` | `cancel_listing(...)` | 1 yoctoNEAR | [`8EWzXeTykEnsMwRbQP3yeM9up184bovCmhoj99ss6LyR`](https://testnet.nearblocks.io/txns/8EWzXeTykEnsMwRbQP3yeM9up184bovCmhoj99ss6LyR) | **CONFIRMED** |

---

## 3. Authoritative Verification Matrix

| Test Suite / Component | Status | Metrics / Details |
|---|---|---|
| **JavaScript Tests** | **VERIFIED** | **114 / 114 passed** (0 failed, 1.92s, Node.js v22) |
| **Python Testnet Tests** | **VERIFIED** | **12 / 12 passed** (0 failed, 0.005s) |
| **NFT Rust Tests** | **VERIFIED** | **104 / 104 passed** (51 default + 53 testnet feature) |
| **Marketplace Rust Tests** | **VERIFIED** | **33 / 33 passed** (0 failed) |
| **Rust Code Formatting** | **VERIFIED** | Clean pass (`cargo fmt --check` on both crates) |
| **Host & Wasm Clippy** | **VERIFIED** | Clean pass (0 warnings across all targets) |
| **Frontend Lint** | **VERIFIED** | Clean pass (`eslint .` clean) |
| **Frontend Production Build** | **VERIFIED** | Succeeded in 1.68s (Vite v8.2.2 bundle produced) |
| **SQL Migrations & RLS Tests** | **VERIFIED** | All 6 migrations applied in sequence; both test suites passed |
| **NFT WASM Reproducibility** | **VERIFIED** | **100% Byte-for-byte identical** to deployed Testnet contract (`DRmwjtfeBzXW2XgXMumytEY5scsDzEtCGsbVNKjdEtW8`) |
| **Marketplace WASM Status** | **VERIFIED** | Both deployed (`Fp3ggF3WmSMQC7m9FU717jA8nB897sYW6bik7Ysqipw7`) and upgraded binary verified |
| **Supabase Edge Function** | **DEPLOYED & VERIFIED** | Deployed live to Supabase `llqwiqhwrbfoxoqqgczy`; HTTP 200/204 verified for `localhost:5173` and `127.0.0.1:5173` |
| **Marketplace Balances** | **RESOLVED** | Standard JSON-RPC fallback provider implemented in `contract.js` |
| **"List NFT" & Seller UI** | **UNLOCKED** | Implemented in `createNft.jsx` and `nftDetails.jsx` with deposit disclosures and on-chain verification |
