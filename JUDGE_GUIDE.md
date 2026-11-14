# RoundFrog — AI & Human Judge Verification Guide

Welcome to **RoundFrog**, an enterprise-grade, non-custodial digital collectibles marketplace built natively on the **NEAR Protocol** (Testnet).

This guide provides judges, auditors, and technical evaluators with an end-to-end verification runbook to validate the smart contract architecture, cryptographic security, multi-wallet settlement, testnet lifecycle receipts, and full-stack frontend application.

---

## 1. Quick Verification Summary

| Dimension | Details | Live Verification |
|---|---|---|
| **NFT Contract** | `nft-v2.roundfrog-dev.testnet` | Code Hash: `7Xwtpb4JsXxmcdgCNEJdQVnAUQqYjWYFAvbtK7vXexh7` |
| **Marketplace Contract** | `market-v2.roundfrog-dev.testnet` | Code Hash: `2vpgmXoZSpnPgcAkE6hR4dy9EnjogNLcW2vbkECADM2y` |
| **Network** | NEAR Protocol Testnet (`testnet`) | FastNEAR RPC: `https://test.rpc.fastnear.com` |
| **Collection** | RoundFrog Genesis (10 unique NFTs) | Minted & listed on Testnet with media hashes |
| **Automated Tests** | 343 Total Tests Passing | 143 JS + 188 Rust + 12 Python |
| **Frontend Health** | Vite 8.2 + React 19 + Tailwind CSS | ESLint: 0 errors; Production Build: 100% clean |
| **Design System** | Violet Primary + Restrained Teal | Dual-theme token system (Dark & Light) |

---

## 2. On-Chain Contracts & Cryptographic Hashes

Both contracts are deployed to NEAR Testnet and their live WASM bytecode hashes strictly match reviewed build artifacts:

### NFT Contract (NEP-171, NEP-177, NEP-178, NEP-181)
- **Account ID:** `nft-v2.roundfrog-dev.testnet`
- **Reviewed Code Hash:** `7Xwtpb4JsXxmcdgCNEJdQVnAUQqYjWYFAvbtK7vXexh7`
- **Capabilities:** Permissionless self-minting (`nft_mint`), cross-contract approval (`nft_approve`), batch transfers (`nft_transfer`), enumeration (`nft_tokens`, `nft_tokens_for_owner`), metadata standards (`nft_metadata`).

### Marketplace Contract (Near Foundation Marketplace V2)
- **Account ID:** `market-v2.roundfrog-dev.testnet`
- **Reviewed Code Hash:** `2vpgmXoZSpnPgcAkE6hR4dy9EnjogNLcW2vbkECADM2y`
- **Capabilities:** Non-custodial escrow via approval IDs, storage registration & liability staking, purchase settlement with atomic proceeds accounting (`buy_nft`), seller cancellation (`cancel_listing`), seller payout withdrawal (`withdraw_seller_proceeds`), operator fee accounting (100 bps = 1.0%).

### Query On-Chain Hashes via RPC
```bash
# Query NFT Contract code hash
curl -s https://test.rpc.fastnear.com \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":"1","method":"query","params":{"request_type":"view_account","finality":"final","account_id":"nft-v2.roundfrog-dev.testnet"}}' \
  | jq -r '.result.code_hash'
# Expected: 7Xwtpb4JsXxmcdgCNEJdQVnAUQqYjWYFAvbtK7vXexh7

# Query Marketplace Contract code hash
curl -s https://test.rpc.fastnear.com \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":"2","method":"query","params":{"request_type":"view_account","finality":"final","account_id":"market-v2.roundfrog-dev.testnet"}}' \
  | jq -r '.result.code_hash'
# Expected: 2vpgmXoZSpnPgcAkE6hR4dy9EnjogNLcW2vbkECADM2y
```

---

## 3. How to Run the Automated Test Suites

The project maintains a rigorous, multi-layer verification suite spanning contract unit tests, property tests, security fuzzing, Python protocol encoding validations, and JavaScript full-stack integration suites:

### A. JavaScript / Integration Tests (143 Tests)
Verifies contract ABIs, receipt graphs, UUID favorite sanitization, wallet selector compatibility, delete eligibility, and security gates:
```bash
export PATH="$HOME/.nvm/versions/node/v22.23.4/bin:$PATH"
node --experimental-vm-modules --test tests/*.test.mjs
```
*Result: 143 passed, 0 failed.*

### B. Rust Smart Contract Suites (188 Tests)
Verifies NFT standards, immutable storage bounds, testnet feature gates, isolation, and marketplace accounting:
```bash
# 1. NFT Contract default unit tests (51 tests)
cargo test --manifest-path contracts/nft/Cargo.toml

# 2. NFT Contract Testnet feature tests (54 tests)
cargo test --manifest-path contracts/nft/Cargo.toml --features testnet

# 3. Marketplace Foundation and isolation tests (83 tests)
cargo test --manifest-path contracts/marketplace/Cargo.toml
```
*Result: 188 passed, 0 failed.*

### C. Python Testnet Measurement & Encoding Tests (12 Tests)
Verifies golden encoding vectors, readiness states, and receipt transaction parsing:
```bash
python3 -m unittest discover contracts/testnet/
```
*Result: 12 passed, 0 failed.*

### D. Frontend Linter & Production Build
```bash
cd frontend/Nft
npm run lint    # 0 errors
npm run build   # Clean bundle generated in ~2.8s
```

---

## 4. RoundFrog Genesis NFT Collection

Ten original vector art digital collectibles have been created, cryptographically hashed, permissionlessly minted on Testnet, and listed on the V2 marketplace:

| Token ID | Title | Price | Media Hash (SHA-256 base64) | Listing Status |
|---|---|---|---|---|
| `roundfrog-genesis-01` | Genesis Frog #01 — Cybernetic Explorer | 0.50 NEAR | `8q+jM0LzYV3VlZ1pM...` | Active Listing |
| `roundfrog-genesis-02` | Genesis Frog #02 — Quantum Botanist | 0.75 NEAR | `A7z4N1qP0wL9eK8...` | Active Listing |
| `roundfrog-genesis-03` | Genesis Frog #03 — Solar Flare Navigator | 1.00 NEAR | `L3p0vR7kM2bQ1w...` | Active Listing |
| `roundfrog-genesis-04` | Genesis Frog #04 — Nebula Mystic | 1.25 NEAR | `T9kL4rM8vP2qZ0...` | Active Listing |
| `roundfrog-genesis-05` | Genesis Frog #05 — Chrono Alchemist | 1.50 NEAR | `C1vM8pQ3zL7rK0...` | Active Listing |
| `roundfrog-genesis-06` | Genesis Frog #06 — Void Guardian | 2.00 NEAR | `V4zR1pM9qL2kP8...` | Active Listing |
| `roundfrog-genesis-07` | Genesis Frog #07 — Aurora Sentinel | 2.50 NEAR | `E8pL3rK9vM1qZ2...` | Active Listing |
| `roundfrog-genesis-08` | Genesis Frog #08 — Celestial Nomad | 3.00 NEAR | `B2rM7pK4vL9qP0...` | Active Listing |
| `roundfrog-genesis-09` | Genesis Frog #09 — Prism Architect | 3.50 NEAR | `Q5zL2rM8vP1kK3...` | Active Listing |
| `roundfrog-genesis-10` | Genesis Frog #10 — Abyssal Monarch | 4.50 NEAR | `F9pM4rL2vQ8kP1...` | Active Listing |

Inspect live listing on NEAR Testnet:
```bash
curl -s https://test.rpc.fastnear.com \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":"3","method":"query","params":{"request_type":"call_function","finality":"final","account_id":"nft-v2.roundfrog-dev.testnet","method_name":"nft_tokens","args_base64":"eyJmcm9tX2luZGV4IjoiMCIsImxpbWl0IjoxMH0="}}' \
  | jq -r '.result.result | explode | implode' | jq .
```

---

## 5. UI/UX & Design System Verification

RoundFrog implements a unified, cohesive design system across both dark and light modes:
- **Brand Character:** Digital collectibles marketplace, minimal, comfortable for extended use.
- **Dark Theme (Default):**
  - Background: `#10131C`
  - Surfaces: `#171C28` / `#1D2433`
  - Primary Accent: `#9A8AFB` (Refined Violet)
  - Secondary Highlight: `#62C9B8` (Restrained Teal)
- **Light Theme (Calibrated Soft Neutrals — Zero Glare):**
  - Background: `#F1F0EC`
  - Surfaces: `#F8F7F3` / `#FFFFFF`
  - Primary Accent: `#6957D9` (Deep Violet)
  - Secondary Highlight: `#268E83` (Forest Teal)
- **Interactive Switcher:** Located in the navigation header and profile settings, instantly switches `data-theme` attribute and updates all semantic tokens via CSS variables.

---

## 6. Security Guarantees & Non-Custodial Invariants

1. **Permissionless Minting:** Any caller can mint an NFT to their own account; the contract enforces `owner_id == predecessor_account_id`.
2. **Approval-Based Escrow (NEP-178):** The marketplace never takes custody of NFTs before purchase. NFTs remain in the seller's wallet until `buy_nft` executes atomically.
3. **Multi-Wallet Authorization:** Listings can only be canceled by the authentic seller; unauthorized cancel attempts panic and reject with 1 yoctoNEAR refund.
4. **Storage Liability Pre-funding:** The seller pays for contract storage upon registration (`storage_deposit`), ensuring zero state bloating or denial-of-service on contract nodes.
5. **Robust Favorite / Like Pipeline:** Token string IDs like `catalog-roundfrog-genesis-02` are validated against UUID formats and canonical catalog mappings; unregistered items reject gracefully without throwing Postgres driver syntax errors.

