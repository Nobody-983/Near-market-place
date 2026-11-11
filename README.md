# NEAR NFT Marketplace

### Discover · Create · Play · Trade

A gaming-enabled decentralized NFT marketplace built natively on **NEAR Protocol Testnet**.

Players play an interactive milestone-based game, earn **genuine on-chain NEP-171 NFTs**, manage their digital collections, list NFTs for sale with decentralized approvals, and trade with independent wallets using NEAR smart contracts.

---

## 🌟 Key Features

### 1. Wallet-Based Cryptographic Authentication
- **Multi-Wallet Support:** Connect seamlessly with **MyNearWallet**, **Meteor Wallet**, **Here Wallet**, and **Sender** via NEAR Wallet Selector v8.10.2.
- **NEP-413 Standard:** Server-verified cryptographic challenge-response proof binds the wallet identity directly to a Supabase user session without passwords or seed-phrase exposure.
- **Dual CORS Support:** Fully functional across `http://localhost:5173` and `http://127.0.0.1:5173`.

### 2. Gaming-Enabled On-Chain NFT Rewards
- **Interactive Milestone Gameplay:** Players tap to earn milestone achievements across 4 tiers:
  - 🥉 **Bronze Contender:** 500 taps
  - 🥈 **Silver Champion:** 2,000 taps
  - 🥇 **Gold Grandmaster:** 5,000 taps
  - 💎 **Legendary Titan:** 10,000 taps
- **Genuine On-Chain NEP-171 Minting:** Achievements are not mere off-chain badges; users initiate real on-chain minting on NEAR Testnet (`nft.roundfrog-dev.testnet`), binding the token to their wallet and catalog.
- **Marketplace Tradeability:** Once minted, game reward NFTs are immediately eligible for listing and sale on the marketplace.

### 3. Comprehensive Seller Workflow
- **"List NFT" Action:** Eligible NFTs owned by the connected wallet display a prominent listing action across cards, detail pages, and portfolio views.
- **Transparent Deposit Disclosures:** Clear prompts for storage deposits (0.01 NEAR attached deposit refunded for unspent storage) and 1% marketplace protocol fee.
- **Decentralized Approvals:** Listings use NEP-178 `nft_approve` to the marketplace contract (`market.roundfrog-dev.testnet`), eliminating custodial escrow risks.
- **Seller-Only Cancellation:** Listings can be cancelled at any time by the authorized owner with on-chain verification.

### 4. Marketplace Discovery & Purchasing
- **Real-Time Discovery:** Browse, filter, and search active listings in the decentralized catalog.
- **Autonomous Settlement:** Atomic purchase execution via `buy_nft` with automatic seller proceeds credit (99%) and protocol fee allocation (100 bps).
- **Replay Protection:** Double-purchase attempts and stale approvals are strictly rejected by smart-contract state assertions.

---

## 🏗️ Architecture & Deployed Contracts

| Component | Account / Address | Details |
|---|---|---|
| **NFT Contract** | `nft.roundfrog-dev.testnet` | NEP-171, NEP-177, NEP-178; Code Hash: `DRmwjtfeBzXW2XgXMumytEY5scsDzEtCGsbVNKjdEtW8` |
| **Marketplace Contract** | `market.roundfrog-dev.testnet` | Approval-based listings, storage liability accounting; Code Hash: `Fp3ggF3WmSMQC7m9FU717jA8nB897sYW6bik7Ysqipw7` |
| **Marketplace Operator** | `operator.roundfrog-dev.testnet` | Admin authority, fee recipient (100 bps) |
| **Mint Authority / Seller** | `seller.roundfrog-dev.testnet` | Testnet mint authority, active demo seller |
| **Independent Buyer** | `buyer.roundfrog-dev.testnet` | Verification buyer wallet |
| **Backend & Database** | Hosted Supabase | PostgreSQL with Row-Level Security (RLS) & `near-wallet-auth` Edge Function |

---

## 🔍 On-Chain Testnet Evidence

The complete end-to-end marketplace lifecycle has been executed and verified on NEAR Testnet:

- **Marketplace Trading Activation:**  
  Tx: [`Gh3zxoLJGGftHxRhVCk2BiJfGSa5UPjfRV92AbEEa6s5`](https://testnet.nearblocks.io/txns/Gh3zxoLJGGftHxRhVCk2BiJfGSa5UPjfRV92AbEEa6s5) — `trading_enabled = true`
- **NFT Minting (`nft_mint`):**  
  Tx: [`7Qw1CFGRXQ4sYJetrUMBreMsJEFcSNDyELodLzfq68pz`](https://testnet.nearblocks.io/txns/7Qw1CFGRXQ4sYJetrUMBreMsJEFcSNDyELodLzfq68pz) — Token `catalog-roundfrog-1794354659` minted to seller
- **NFT Listing Approval (`nft_approve`):**  
  Tx: [`3ivrcy1MTpUgpKkmt9E8xzRqh4detzW3gpBBEjtjKPq4`](https://testnet.nearblocks.io/txns/3ivrcy1MTpUgpKkmt9E8xzRqh4detzW3gpBBEjtjKPq4) — Active listing created with composite ID `...|2`
- **Authorized Listing Cancellation (`cancel_listing`):**  
  Tx: [`8EWzXeTykEnsMwRbQP3yeM9up184bovCmhoj99ss6LyR`](https://testnet.nearblocks.io/txns/8EWzXeTykEnsMwRbQP3yeM9up184bovCmhoj99ss6LyR) — Verified on-chain cancellation
- **Multi-Wallet Security Isolation:**  
  Attempt by `buyer.roundfrog-dev.testnet` to cancel rejected by contract panic: *"Only the seller may cancel"*
- **Multi-Wallet NFT Purchase (`buy_nft`):**  
  Tx: [`2mJuAN9Rioqa32mkkMnaScLZUpgdPVMadeSkcyPeVyS3`](https://testnet.nearblocks.io/txns/2mJuAN9Rioqa32mkkMnaScLZUpgdPVMadeSkcyPeVyS3) — `buyer.roundfrog-dev.testnet` bought token for 0.1 NEAR
- **Atomic Settlement & Accounting:**  
  - Ownership transferred: `buyer.roundfrog-dev.testnet`
  - Seller proceeds credited on marketplace: `0.099 NEAR` (`99000000000000000000000` yoctoNEAR)
  - Operator fee credited on marketplace: `0.001 NEAR` (`1000000000000000000000` yoctoNEAR)
  - Listing active status: `false`
- **Replay Protection:**  
  Second purchase attempt rejected with contract panic (`Listing is not active`), confirming duplicate purchase prevention.

---

## 🚀 Quickstart & Demonstration

### Prerequisites
- Node.js 20.19+ or 22.12+ (tested on Node v22.23.4)
- Rust & Cargo (for smart contract tests)
- Python 3.10+ (for testnet verification suite)

### 1. Setup & Launch Frontend

```bash
# Enter frontend directory
cd frontend/Nft

# Install dependencies
npm install

# Start Vite development server
npm run dev
```

Open `http://localhost:5173` or `http://127.0.0.1:5173` in your browser.

### 2. Hackathon Judge Demonstration Flow

1. **Connect NEAR Wallet:** Click "Connect Wallet" at top right. Select MyNearWallet or Meteor. Authenticate via NEP-413.
2. **Play & Mint NFT:** Navigate to **Game** (`/game`). Tap the frog coin to reach 500 taps. Click **Claim & Mint On-Chain**. Observe the verified on-chain badge and token ID.
3. **List on Marketplace:** Click **List on Marketplace** from the game reward card or open your collection. Click **List NFT**, enter price in NEAR, and sign the approval transaction.
4. **Discover & Trade:** Switch to **Marketplace** (`/marketplace`) to see your newly listed NFT available for any independent buyer.

---

## 🧪 Comprehensive Verification & Test Suites

The repository contains automated unit, integration, and contract test suites:

```bash
# 1. Run full JavaScript integration test suite (123 tests)
node --experimental-vm-modules --test tests/*.test.mjs

# 2. Run Python contract testnet tests (12 tests)
python3 -m unittest discover -s contracts/testnet

# 3. Run Rust NFT Smart Contract tests (104 tests)
cargo test --manifest-path contracts/nft/Cargo.toml --all-targets

# 4. Run Rust Marketplace Smart Contract tests (33 tests)
cargo test --manifest-path contracts/marketplace-deploy/Cargo.toml --all-targets

# 5. Frontend lint and production build
npm --prefix frontend/Nft run lint
npm --prefix frontend/Nft run build

# 6. Verify live Testnet state
node scripts/testnet-verify.mjs

# 7. Testnet multi-wallet purchase dry-run (safe inspection)
./scripts/testnet-purchase.sh --dry-run
```

All 272 automated tests pass with 0 warnings and 0 errors.

---

## 📜 License

MIT License. Built for the NEAR Protocol ecosystem.
