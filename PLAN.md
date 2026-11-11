# Migration Plan: Nimiq → NEAR

## Current State
- Frontend: React + Vite, Nimiq Mini App SDK + @nimiq/core, Nimiq Pay for transactions
- Backend: Supabase (auth, DB, storage) - profiles, nfts, marketplace_listings, nft_likes, nft_ownership, game rewards
- Currency: NIM (testnet), payments via sendBasicTransaction through Nimiq Pay
- Auth: wallet address-based via Supabase auth with derived credentials
- No on-chain NFT contracts in repo - logic is mostly off-chain via Supabase

## Target State (NEAR-native)
- Wallet: NEAR Wallet Selector (MyNearWallet, HereWallet, Meteor Wallet, etc.) connecting to NEAR testnet
- Smart Contracts: Deploy NEP-171 NFT contract and/or marketplace contract on NEAR testnet
- Currency: NEAR (testnet), payments via NEAR transactions
- Auth: Use NEAR account as identity
- Rebrand to NEAR (logo, colors, copy)
- Remove Nimiq dependencies

## Steps
1. **Research & Dependencies**
- Add NEAR packages: near-api-js, @near-wallet-selector/core, @near-wallet-selector/modal-ui, @near-wallet-selector/my-near-wallet, @near-wallet-selector/meteor-wallet, etc.
- Consider using near-sdk-js/near-sdk-rs for contracts? May need to create contracts in contracts/ dir

2. **Smart Contracts**
- Create NEAR NFT contract (NEP-171) - e.g. using near-sdk-js or write simple contract
- Create marketplace contract for listing/buying or use NFT transfer with attached deposit
- Put contracts in /contracts directory

3. **Frontend Migration**
- Replace Nimiq wallet context with NEAR wallet selector
- Replace nimiq lib with near lib
- Update auth to work with NEAR accounts
- Update all transaction flows to use NEAR
- Update UI/branding to NEAR (colors, logo, copy)

4. **Backend/DB**
- May need to adapt schema - store NEAR account IDs, token IDs, contract addresses
- Keep Supabase if it still provides value or adapt

5. **Rebrand**
- Update all text from Nimiq to NEAR
- Update colors to NEAR palette (black/white/green)
- Update logos/icons

6. **Testing & Cleanup**
- Remove Nimiq deps
- Update README
- Run builds/tests
