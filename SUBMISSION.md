# NEAR Protocol Hackathon Submission: RoundFrog NFT Marketplace

**Project Name:** RoundFrog — Permissionless NEAR NFT Marketplace & Gaming Ecosystem  
**Repository:** https://github.com/Nobody-983/nft-platform  
**Network:** NEAR Protocol Testnet  
**Target Contracts:**
- **NFT (NEP-171):** `nft-v2.roundfrog-dev.testnet` *(Release WASM: SHA-256 `61139d32...`, Code Hash: `7Xwtpb4JsXxmcdgCNEJdQVnAUQqYjWYFAvbtK7vXexh7`)*
- **Marketplace:** `market-v2.roundfrog-dev.testnet` *(Release WASM: SHA-256 `1ca5635d...`, Code Hash: `2vpgmXoZSpnPgcAkE6hR4dy9EnjogNLcW2vbkECADM2y`)*
- **Legacy Verified Reference:** `nft.roundfrog-dev.testnet` / `market.roundfrog-dev.testnet`

---

## 1. Short Pitch (30 seconds)
RoundFrog is a fully permissionless, non-custodial NFT marketplace on NEAR Protocol Testnet. Unlike demo platforms that restrict minting to hardcoded admin keys or database badges, RoundFrog lets **any connected NEAR wallet** mint genuine on-chain NEP-171 NFTs directly to their account, list them via NEP-178 approvals without custodial escrow risk, and trade with independent buyers through verified, atomic smart-contract settlement in native NEAR.

---

## 2. Problem & Solution

### The Problem
Most hackathon NFT platforms fall into two failure modes:
1. **Mock Centralization:** NFTs and listings are stored only in off-chain databases; "minting" and "purchasing" are simulated without real blockchain transactions or verifiable ownership.
2. **Restricted Admin Minting:** Deployed NFT contracts hardcode a privileged deployer or seller key as the sole mint authority. Judges and third-party users cannot connect their own wallets, mint an NFT, or test the lifecycle without manual admin intervention.

### The Solution
RoundFrog solves this by delivering an end-to-end, permissionless smart contract and frontend architecture on NEAR Protocol:
- **Permissionless NEP-171 Self-Minting:** Implements a strict `caller == token_owner_id` invariant in Rust. Any valid `.testnet` account can call `nft_mint` to create unique NFTs directly into their own wallet, while arbitrary third-party recipient spoofing is strictly rejected.
- **NEP-178 Approval-Based Trading:** Sellers list NFTs without transferring custody to the marketplace contract. The NFT remains in the seller's wallet until an atomic cross-contract call (`buy_nft` -> `transfer_from_marketplace`) executes simultaneously with payment delivery.
- **Durable Storage & Fee Accounting:** The marketplace contract accounts for NEP-145 storage deposits, refunds excess storage upon cancellation, and automatically distributes 99% of proceeds to the seller and 1% (100 bps) to the protocol fee account.
- **Interactive Game Rewards:** Players tap in an interactive milestone game and claim genuine on-chain NEP-171 tokens directly into their wallet, which can then be listed and traded on the open marketplace.

---

## 3. Technical Architecture

```
                       ┌────────────────────────┐
                       │   NEAR Wallet User     │
                       │ (MyNearWallet, Meteor) │
                       └───────────┬────────────┘
                                   │
                    NEP-413 Auth / Function Calls
                                   ▼
                       ┌────────────────────────┐
                       │   React + Vite DApp    │
                       │ (Tailwind, Lucide, VM) │
                       └─────┬────────────┬─────┘
                             │            │
            RPC / View Calls │            │ State Sync / RLS
                             ▼            ▼
   ┌────────────────────────────────┐  ┌─────────────────────────────────┐
   │    NEAR Testnet Blockchain     │  │        Hosted Supabase          │
   ├────────────────────────────────┤  │ (PostgreSQL, Edge Functions,   │
   │ • nft-v2.roundfrog-dev.testnet │  │  Row-Level Security, Storage)   │
   │   (NEP-171, NEP-177, NEP-178)  │  └─────────────────────────────────┘
   │                                │
   │ • market-v2.roundfrog-dev...   │
   │   (Atomic Fixed-Price Market,  │
   │    NEP-145 Storage Accounting) │
   └────────────────────────────────┘
```

- **Smart Contracts (Rust):** `near-sdk` v5.7.0, compiled to `wasm32-unknown-unknown`.
- **Frontend:** React 18, Vite, Tailwind CSS, NEAR Wallet Selector v8.10.2 (`@near-wallet-selector/core`, `my-near-wallet`, `meteor-wallet`, `here-wallet`, `sender`).
- **Database & Auth:** Supabase PostgreSQL with 6 migrations, RLS policies, and NEP-413 cryptographic signature authentication via Edge Functions.
- **Testing & Verification:** Comprehensive automated test suite including Rust native unit tests, VM integration tests, Python testnet runners, and disposable PostgreSQL verification.

---

## 4. Why RoundFrog is Useful & Innovative

1. **Zero Registration Barrier for Judges & Users:** Any judge can connect their personal Testnet wallet and immediately experience the complete creator and trader journey.
2. **True Escrow-Free Architecture:** Using NEAR's NEP-178 approval standard, users never surrender token custody to a centralized contract until the exact millisecond payment is settled.
3. **Gaming to DeFi Bridge:** Gamification rewards are not locked inside the application; they are authentic NEP-171 assets that seamlessly integrate into the broader NEAR NFT ecosystem.
4. **Resilient Financial Accounting:** The contracts maintain a formal internal balance sheet (`Ledger`) tracking seller liability, fee liability, pending sales, and storage reserves, guaranteeing that total contract liabilities never exceed actual liquid backing.

---

## 5. Judge Demonstration Script (Step-by-Step)

### Setup
- Open the application: `http://localhost:5173` (or deployed URL).
- Ensure two NEAR Testnet wallets are available: **Judge Wallet A** (Creator/Seller) and **Judge Wallet B** (Buyer).

### Step 1: Wallet A Connects & Mints an NFT
1. In the top right, click **Connect Wallet**.
2. Select **MyNearWallet** or **Meteor Wallet** and authenticate as **Judge Wallet A**.
3. Navigate to **Create NFT** (`/create-nft`).
4. Fill in:
   - Name: `Judge Special Edition #1`
   - Description: `Permissionless NEAR Testnet Hackathon Token`
   - Category: `Collectible`
   - Model: `1 of 1 Unique NFT`
   - Upload an image (PNG, JPG, or WEBP).
5. Click **Create & Mint On-Chain**.
6. Sign the `nft_mint` transaction in your wallet (attaches 0.05 NEAR storage deposit; contract automatically refunds unused storage).
7. Notice the real-time success receipt: the token ID is displayed with immediate link to view on-chain ownership.

### Step 2: Wallet A Lists the NFT for Sale
1. From the Post-Mint modal or your Collection, click **List for Sale**.
2. Enter the listing price: `0.5 NEAR`.
3. Click **Confirm & List on NEAR**.
4. Sign the `nft_approve` transaction in your wallet (attaches 0.01 NEAR for approval storage).
5. The listing is now active on the shared marketplace contract!

### Step 3: Wallet B Discovers and Purchases the NFT
1. Disconnect Wallet A and connect **Judge Wallet B** (or open in a second browser profile).
2. Navigate to **Marketplace** (`/marketplace`).
3. Locate `Judge Special Edition #1` listed by Wallet A.
4. Click on the NFT card to open **NFT Details** (`/nft/:id`).
5. Notice that Wallet B sees the **Buy Now (0.5 NEAR)** button (Wallet A would see the seller's "Cancel Listing" option instead).
6. Click **Buy Now** and approve the transaction in Wallet B.
7. The marketplace executes atomic settlement:
   - NFT ownership transfers immediately to **Wallet B**.
   - 99% of funds (`0.495 NEAR`) are credited to **Wallet A**'s seller proceeds balance.
   - 1% of funds (`0.005 NEAR`) are allocated to the marketplace protocol fee account.

### Step 4: Verification of Ownership & Cancellation Safety
1. Wallet B navigates to **Create NFT** / **My Collection** and confirms the NFT is now listed in Wallet B's inventory.
2. In a separate test, any seller can list an NFT and click **Cancel Listing** to immediately revoke the marketplace approval and release reserved storage.

---

## 6. Live Testnet Deployment Receipts & Evidence

### Verified V2 Deployment Receipts (Live on Testnet)
- **NFT Subaccount (`nft-v2.roundfrog-dev.testnet`):**  
  Tx: [`sFNBPGuXNrudrSaBWC2G1LMEypXCH49vybZWWmwJDFG`](https://testnet.nearblocks.io/txns/sFNBPGuXNrudrSaBWC2G1LMEypXCH49vybZWWmwJDFG) — Funded with 6.0 NEAR
- **Marketplace Subaccount (`market-v2.roundfrog-dev.testnet`):**  
  Tx: [`AutB5G4upHvG18usQYCEq1cXLfk6oNCZ1ndsEoew1Lqd`](https://testnet.nearblocks.io/txns/AutB5G4upHvG18usQYCEq1cXLfk6oNCZ1ndsEoew1Lqd) — Funded with 6.0 NEAR
- **V2 NFT Deployment & Initialization (`new_testnet`):**  
  Tx: [`EatAD9i9NfguAZujqafEhM4tAoXHda4KJ5uDzUPUvH7t`](https://testnet.nearblocks.io/txns/EatAD9i9NfguAZujqafEhM4tAoXHda4KJ5uDzUPUvH7t) — Verified Code Hash: `7Xwtpb4JsXxmcdgCNEJdQVnAUQqYjWYFAvbtK7vXexh7`
- **V2 Marketplace Deployment & Initialization (`new`):**  
  Tx: [`Ejuv8dDu9C1D14mo8cQitW6aAuDL3XaoWodyipA2Sicq`](https://testnet.nearblocks.io/txns/Ejuv8dDu9C1D14mo8cQitW6aAuDL3XaoWodyipA2Sicq) — Verified Code Hash: `2vpgmXoZSpnPgcAkE6hR4dy9EnjogNLcW2vbkECADM2y`
- **V2 Marketplace Trading Activation (`set_trading_enabled`):**  
  Tx: [`DQShEKYq1ozBn1Yb6R4p3SbppNu3jdhFMWLL3yPBFKY4`](https://testnet.nearblocks.io/txns/DQShEKYq1ozBn1Yb6R4p3SbppNu3jdhFMWLL3yPBFKY4) — `trading_enabled = true`

### V1 Reference Multi-Wallet Lifecycle Receipts
- **Automated Tests:**
  - 138/138 JavaScript integration tests passing (`tests/*.test.mjs`).
  - 12/12 Python contract testnet tests passing (`contracts/testnet/test_*.py`).
  - 105/105 Rust smart-contract tests passing (NFT contract: 51 default + 54 testnet; Marketplace contract: 34 tests).
  - 100% Rust code formatting and Clippy checks passing with zero warnings.
  - Production Vite build compiling cleanly with zero errors.

---

## 7. Draft Announcement for X (Twitter)

> 🐸 Introducing RoundFrog on @NEARProtocol!
> 
> A 100% permissionless, non-custodial NFT marketplace built natively on #NEAR Testnet:
> 
> 🔹 Permissionless NEP-171 self-minting for any wallet
> 🔹 NEP-178 approval listings (zero custodial escrow)
> 🔹 Atomic fixed-price settlement & seller payouts
> 🔹 Interactive gaming rewards minted on-chain
> 
> Connect your wallet, mint, list & trade: https://github.com/Nobody-983/nft-platform
> 
> #NEARHackathon #BOS #Web3 #Gaming

