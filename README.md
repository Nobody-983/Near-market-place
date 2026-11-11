# NEAR NFT

### Discover · Create · Trade

A NEAR NFT marketplace prototype with catalog browsing and off-chain collectible creation. **Buying, listing, and cancellation are disabled** pending a verified marketplace and settlement workflow.

This app is configured for **NEAR Testnet** by default.

Built for the NEAR ecosystem with wallet-based identity, NEAR smart contracts, and a modern marketplace experience.

## Overview

**NEAR NFT** is a decentralized NFT marketplace built on NEAR Protocol.

Users authenticate through a server-verified NEAR wallet proof bound to a Supabase user UUID and session. See the [Testnet end-to-end readiness report](TESTNET_E2E_READINESS.md) for the current capabilities, exact commands, test evidence, trading blockers and required live checks, and [production readiness](PRODUCTION_READINESS.md) for the security assessment.

## Features

### Wallet-Based Authentication
- Connect with NEAR wallets (MyNearWallet, Meteor Wallet, HereWallet, Sender)
- NEP-413 wallet proof binds the Supabase user UUID and session
- Secure, non-custodial authentication

### NFT Creation
- Create NFTs with name, description, category, image, and price
- Images stored via Supabase Storage
- Metadata stored in database
- NEP-171 contract source is present; catalog creation is not wired to minting

### Marketplace
- Browse and discover NFTs
- Listing, buying, and cancellation stay temporarily unavailable

### Testnet NEAR Payments
- Testnet is the default network
- Prices use exact decimal strings and yoctoNEAR conversion
- Trusted purchase, withdrawal and refund verification/reconciliation is implemented in `services/`; live Testnet evidence is still pending and the action gates remain disabled

## Tech Stack

### Frontend
- React 19 + Vite
- Tailwind CSS
- Framer Motion
- React Router
- Lucide React

### Blockchain
- NEAR Protocol
- NEAR Wallet Selector
- near-api-js
- NEP-171 NFT Standard
- NEAR Smart Contracts (Rust/AssemblyScript)

### Backend
- Supabase (Authentication, PostgreSQL, Storage, RLS)

## Setup

### Prerequisites
- Node.js 20.19+ or 22.12+ (as required by the installed Vite)
- NEAR testnet account ([create one](https://testnet.mynearwallet.com/))
- NEAR testnet funds from [faucet](https://near-faucet.io/)

### Environment Variables

Create `.env` in `frontend/Nft/`:

```env
VITE_SUPABASE_URL=your_supabase_url
VITE_SUPABASE_PUBLISHABLE_KEY=your_supabase_anon_key
VITE_NEAR_NETWORK_ID=testnet
VITE_NEAR_NODE_URL=https://rpc.testnet.near.org
VITE_NFT_CONTRACT_ID=nft.roundfrog-dev.testnet
VITE_MARKETPLACE_CONTRACT_ID=market.roundfrog-dev.testnet
VITE_PREVIEW_DATA_ENABLED=false
```

Never put a Supabase secret/service-role key in any `VITE_*` variable.

### Installation

```bash
# Install frontend dependencies
cd frontend/Nft
npm install

# Run development server
npm run dev
```

## Smart Contracts & Reproducible Verification

The [NFT contract](contracts/nft/src/lib.rs) implements minting, transfer, approvals, and enumeration. The deployable [marketplace contract](contracts/marketplace-deploy/src/lib.rs) implements approval-based listings, purchases and callback-safe withdrawals. The catalog indexer and settlement worker live in `services/`.

### Reproducible Builds & Tests

```bash
# Verify contract bytecode reproducibility against deployed Testnet code
bash scripts/verify-contracts.sh

# Run the complete test suite (113 JS, 12 Python, 104 NFT Rust, 33 Marketplace Rust, Clippy, ESLint, Vite build, SQL migrations)
bash scripts/test-all.sh
```

See [WASM_REPRODUCIBILITY.md](WASM_REPRODUCIBILITY.md) for full compilation specifications, target flags, and on-chain hashes. Setting contract addresses does not make trading safe; complete the [production requirements](PRODUCTION_READINESS.md#required-before-enabling-trading) before enabling the gates.

## Development Notes

- **Testnet Only**: This app is configured for NEAR Testnet
- **Wallets**: Use NEAR Wallet Selector compatible wallets
- **Contracts**: NFT minting and marketplace operations require deployed contracts on testnet
- **Database**: Uses Supabase for off-chain data (profiles, listings metadata, images)

## Migration Notes

This project has been migrated from Nimiq to NEAR Protocol. All Nimiq dependencies and integrations have been replaced with NEAR-native implementations.

## License

MIT
