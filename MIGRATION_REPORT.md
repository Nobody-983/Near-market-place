# Migration Report: Nimiq → NEAR

This is a historical migration report, not deployment verification. See
[production readiness](PRODUCTION_READINESS.md) for the current state; trading
remains disabled and no compatible marketplace or trusted settlement worker is included.

## Summary
Migrated the NFT marketplace from Nimiq to a fully NEAR-native implementation using NEAR Wallet Selector, near-api-js, and prepared smart contract scaffolding for NEP-171 NFT and marketplace contracts.

## Files Changed

### Configuration & Dependencies
- `frontend/Nft/package.json` - Replaced Nimiq deps with NEAR deps (near-wallet-selector, near-api-js)
- `frontend/Nft/index.html` - Updated title to NEAR NFT, changed favicon
- `frontend/Nft/public/near-logo.svg` - Added NEAR logo
- `frontend/Nft/.env.example` - Created with NEAR testnet config and contract addresses

### NEAR Integration (New)
- `frontend/Nft/src/config/near.js` - NEAR network configuration
- `frontend/Nft/src/lib/near/utils.js` - NEAR utility functions (yocto/NEAR conversion, formatting)
- `frontend/Nft/src/lib/near/contract.js` - Smart contract interaction helpers (mint, list, buy, transfer)

### Wallet & Auth
- `frontend/Nft/src/context/walletContext.jsx` - Migrated from Nimiq Pay to NEAR Wallet Selector
- `frontend/Nft/src/services/auth.js` - Updated for NEAR account IDs

### UI/Pages (Updated for NEAR)
- `frontend/Nft/src/App.jsx` - Cleaned up
- `frontend/Nft/src/pages/auth.jsx` - Rebranded to NEAR, updated connection flow
- `frontend/Nft/src/pages/wallet.jsx` - Migrated to NEAR wallet display
- `frontend/Nft/src/pages/nftDetails.jsx` - Updated for NEAR payments
- `frontend/Nft/src/pages/createNft.jsx` - Currency changed to NEAR
- `frontend/Nft/src/pages/market.jsx`, `dashboard.jsx`, `Account.jsx`, `game/Game.jsx` - Rebranded to NEAR
- `frontend/Nft/src/components/sidebar.jsx`, `dashboardlayout.jsx` - Rebranded styling

### Removed
- `frontend/Nft/src/lib/nimiq.js`, `frontend/Nft/src/lib/nimiq-network.js` - Nimiq integrations removed

### Smart Contracts
- `contracts/README.md` - Contract deployment guide
- `contracts/nft-contract.md` - NEP-171 contract reference

### Documentation
- `README.md` - Completely rewritten for NEAR
- `MIGRATION_REPORT.md` - This report

## NEAR Features Implemented
- **Wallet Connection**: NEAR Wallet Selector with MyNearWallet, Meteor Wallet, HereWallet, Sender
- **Network**: NEAR Testnet by default (configurable)
- **Authentication**: NEAR account ID-based auth via Supabase
- **Balance**: Fetch NEAR balance from RPC
- **NFT Operations**: Prepared contract interaction functions for minting, transferring
- **Marketplace**: Prepared listing/buying via marketplace contract
- **Payments**: NEAR transaction support structure
- **Rebranding**: Complete NEAR branding (colors: black/white/green #00EC97, logo, copy)

## Checks

### Build
- ✅ `npm run build` (frontend/Nft) - **PASSED**

### Lint
- ⚠️ `npm run lint` - Has warnings/errors mostly from dependencies and pre-existing issues; main code issues fixed (wallet.jsx cleaned)

## Manual Steps Required

### 1. Environment Setup
Create `frontend/Nft/.env` from `.env.example` with:
- Supabase credentials (existing)
- NEAR contract addresses (after deployment)

### 2. Deploy Smart Contracts to NEAR Testnet
```bash
# Install NEAR CLI
npm install -g near-cli

# Login to testnet
near login

# Deploy NFT contract (NEP-171) and Marketplace contract
# See contracts/README.md for details
```

### 3. Configure Contract Addresses
Update `.env` with deployed contract IDs:
```env
VITE_NFT_CONTRACT_ID=your-nft.testnet
VITE_MARKETPLACE_CONTRACT_ID=your-market.testnet
```

### 4. Get Testnet NEAR
Get testnet NEAR from [NEAR Faucet](https://near-faucet.io/) for transactions.

### 5. Test the Application
```bash
cd frontend/Nft
npm install
npm run dev
```

### 6. Supabase Configuration
The prepared migrations require a new Supabase project and independent live validation. Supabase user/profile IDs remain UUIDs; NEAR account IDs are separate text values. Keep the existing RLS/grants and session verification restrictions.

**Note**: The NFT contract source does not supply marketplace sale methods. Browser listing/cancellation writes remain blocked, and the fabricated purchase fallback has been removed. Deployment alone is insufficient: trading requires the contract/ABI validation and trusted chain reconciliation described in the readiness document.
