# RoundFrog — Live Demo & Walkthrough Script

This demo script guides judges, evaluators, and users through a step-by-step interactive demonstration of the RoundFrog digital collectibles marketplace on NEAR Protocol Testnet.

---

## Prerequisites for Live Evaluation
1. **Local Dev Server:** Running at `http://localhost:5173/`
   ```bash
   cd frontend/Nft
   npm run dev
   ```
2. **Web Browser:** Google Chrome, Firefox, or Brave.
3. **NEAR Testnet Wallet:** A funded testnet account (e.g. via [testnet.mynearwallet.com](https://testnet.mynearwallet.com/) or Meteor Wallet).
   - Alternatively, use the pre-configured testnet accounts: `roundfrog-dev.testnet` or `buyer.roundfrog-dev.testnet`.

---

## Step 1: Visual Design System & Dual-Theme Switch
- **Action:**
  1. Navigate to `http://localhost:5173/`.
  2. Locate the **Theme Toggle** button in the navigation header (or top right).
  3. Click the toggle to switch between **Dark Theme** and **Light Theme**.
- **Expected Outcome:**
  - **Dark Mode:** Deep slate `#10131C` background, surface cards at `#171C28`, luminous violet `#9A8AFB` accent, and restrained teal highlights `#62C9B8`.
  - **Light Mode:** Calibrated soft warm neutral `#F1F0EC` background, elevated `#FFFFFF` surface cards, rich violet `#6957D9` accent, and forest teal `#268E83` highlights. Zero high-contrast glare.
  - The entire page (hero banner, cards, buttons, badges) seamlessly adapts using CSS tokens.

---

## Step 2: Dashboard & Genesis Collection Preview
- **Action:**
  1. On the Dashboard (`/`), observe the Hero Carousel and **Trending Now** section.
  2. Notice the **Market Overview** statistics card at the bottom of the page.
- **Expected Outcome:**
  - The hero carousel rotates through the newly minted **RoundFrog Genesis** collection items.
  - Market Overview displays live on-chain stats: **18 NFTs Created**, **11 Active Listings**.
  - All cards display real prices in NEAR, token creator badges, and like buttons.

---

## Step 3: Marketplace Exploration & Category Filter
- **Action:**
  1. Click **Marketplace** in the navigation bar (or navigate to `/marketplace`).
  2. Click the category filter pill labeled **"RoundFrog Genesis"**.
  3. Sort by "Price: Low to High" or search for "Cybernetic".
- **Expected Outcome:**
  - Displays all 10 verified on-chain Genesis NFTs (`Genesis Frog #01` through `#10`) with prices ranging from 0.5 NEAR to 4.5 NEAR.
  - High-resolution SVG vector art displays crisply on both mobile and desktop screens.
  - Cards feature responsive hover animations with violet accent borders.

---

## Step 4: NFT Details & Metadata Verification
- **Action:**
  1. Click on **Genesis Frog #01 — Cybernetic Explorer**.
  2. You will be routed to `/nft/roundfrog-genesis-01`.
  3. Inspect the details panel: Token ID, Contract Address (`nft-v2.roundfrog-dev.testnet`), Owner, Media Hash, and Price (0.5 NEAR).
- **Expected Outcome:**
  - Token details are retrieved directly from the live Testnet RPC via `nft_token`.
  - Media hash displays the verified SHA-256 base64 digest.
  - If the connected wallet is the owner, a **"Cancel Listing"** action is available.
  - If the connected wallet is a different user, a **"Buy Now"** action is available.

---

## Step 5: Liking / Favoriting without Database Syntax Errors
- **Action:**
  1. Click the **Heart (Like)** button on an on-chain NFT card (e.g., `roundfrog-genesis-01`).
- **Expected Outcome:**
  - If wallet is not connected/verified, the UI shows a helpful prompt: `"Connect and verify your NEAR wallet to save favorites."`
  - If connected, `useNFTLikes` safely resolves the token ID without crashing the Postgres driver with an `invalid input syntax for type uuid` error.
  - Unregistered catalog entries gracefully report: `"This NFT is not registered in the catalog and cannot be favorited."` without sending invalid SQL.

---

## Step 6: Permissionless On-Chain Minting
- **Action:**
  1. Click **Create NFT** in the top navigation (`/create-nft`).
  2. Enter a title (e.g. "Judge Verification Token"), description, and category.
  3. Upload an artwork image.
  4. Click **Mint on NEAR Testnet**.
- **Expected Outcome:**
  - Wallet Selector modal prompts user to confirm the transaction.
  - Calls `nft_mint` on `nft-v2.roundfrog-dev.testnet` attaching 0.05 NEAR storage deposit.
  - Contract strictly enforces `owner_id == predecessor_account_id`.
  - Transaction succeeds with a finalized NEAR block receipt and updates the collection.

---

## Step 7: Cross-Contract Listing & Multi-Wallet Purchase
- **Action:**
  1. On a newly minted NFT, click **List for Sale**.
  2. Enter price (e.g. `1.0` NEAR).
  3. Sign the `nft_approve` transaction calling `market-v2.roundfrog-dev.testnet`.
  4. Connect a second wallet (e.g. `buyer.roundfrog-dev.testnet`).
  5. Click **Buy Now** and confirm payment of 1.0 NEAR.
- **Expected Outcome:**
  - Non-custodial cross-contract settlement executes atomically.
  - Token ownership transfers to the buyer wallet.
  - 0.99 NEAR credited to seller proceeds; 0.01 NEAR credited to protocol fee escrow.
  - The listing closes and transitions to sold/inactive status.

