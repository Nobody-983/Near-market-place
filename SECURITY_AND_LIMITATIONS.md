# RoundFrog — Security Architecture, Threat Model & System Limitations

This document provides a thorough audit of the security controls, access control invariants, storage economics, cryptographic defenses, and explicit architectural limitations of the RoundFrog NFT marketplace.

---

## 1. Smart Contract Security Architecture

### A. Strict 1-yoctoNEAR Access Control
On NEAR Protocol, methods that modify sensitive user state (such as token transfers, marketplace approvals, listing cancellations, or fund withdrawals) require an attached deposit of exactly $1 \text{ yoctoNEAR} = 10^{-24} \text{ NEAR}$.
- **Invariant:** `assert_one_yocto()`
- **Security Guarantee:** Browsers and wallet extensions cannot sign 1-yocto transactions silently using a limited-access function-call key. The transaction *must* be signed with a full-access key, requiring explicit biometric or password confirmation from the user.

### B. Non-Custodial Approval Model (NEP-178)
- Rather than taking custody of the NFT upon listing, RoundFrog leaves the token in the seller's account.
- When listed, the seller grants the marketplace contract an `approval_id` tied to specific listing terms (price, fee bps, operator fee account).
- During purchase (`buy_nft`), the marketplace executes `nft_transfer_payout` or `nft_transfer`, passing the `approval_id`. If the seller transferred the token elsewhere in the interim, the transaction fails closed, refunding the buyer immediately.

### C. Reentrancy & Checks-Effects-Interactions
- State mutations are recorded in the marketplace ledger prior to dispatching outward asynchronous cross-contract calls.
- In `buy_nft`, the listing status is marked inactive and the buyer lock is engaged before invoking `nft_transfer`.
- If the token transfer promise fails or times out, the settlement reverts or records a refundable liability for the buyer (`withdraw_buyer_refund`), preventing double-spend or drained operator balances.

### D. Seller Cancellation Authorization
- In `cancel_listing(listing_id)`:
  ```rust
  require!(
      env::predecessor_account_id() == listing.seller,
      "Only the seller may cancel"
  );
  ```
- Any unauthorized account (including operators or malicious buyers) attempting to cancel or displace an active listing causes a contract panic and receives a full refund of their attached 1-yocto deposit.

---

## 2. Storage Economics & DoS Prevention (NEP-145)

On NEAR, contract state is paid for through native token staking (approximately $1 \text{ NEAR}$ per $100 \text{ KB}$).
- **Vulnerability Prevented:** If users could list or mint NFTs without paying for contract storage, malicious callers could exhaust the contract's liquid balance with spam listings, rendering the contract unusable.
- **RoundFrog Defense:**
  - Sellers must call `storage_deposit` on `market-v2.roundfrog-dev.testnet` prior to listing.
  - Each active listing consumes a known, deterministic storage allocation ($\approx 0.012 \text{ NEAR}$).
  - When a listing is cancelled or settled, the storage liability is freed and becomes available for new listings or may be withdrawn by the seller via `withdraw_storage`.

---

## 3. Web & Database Security (Supabase & Edge Functions)

### A. Publishable Key Isolation
- The browser frontend exclusively uses `VITE_SUPABASE_PUBLISHABLE_KEY`.
- No `service_role` or database administrator credentials exist in the client repository or production bundle.

### B. Wallet Signature Authentication (NEP-413)
- Off-chain actions (such as saving favorites or updating profile bios) authenticate through the Supabase Edge Function `near-wallet-auth`.
- The user signs a cryptographic challenge payload using their NEAR private key (`nep413` format).
- The Edge Function verifies the signature against the on-chain public key before issuing a scoped JWT session.

### C. Robust Like / Favorite Sanitization
- Previous versions were vulnerable to a driver syntax crash when arbitrary token strings like `catalog-roundfrog-genesis-02` were passed to the `nft_likes.nft_id` UUID column.
- The pipeline now enforces `resolveNFTId`:
  1. Validates standard UUID formats (`/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i`).
  2. Extracts UUID suffixes for `catalog-<UUID>` tokens.
  3. Queries canonical database mapping by `token_id`.
  4. Nonexistent or unindexed tokens fail gracefully without sending malformed SQL statements to PostgreSQL.

---

## 4. Current Limitations & Known Scope Constraints

| Component | Current State | Production Roadmap |
|---|---|---|
| **Auction Formats** | Fixed-price instant buy (`buy_nft`) only | Timed English & Dutch auctions planned for V3 contract release. |
| **Network Support** | NEAR Protocol Testnet (`testnet`) | Mainnet deployment scheduled following external formal audit. |
| **Collection Royalties** | Fixed 1.0% (100 bps) operator fee | Dynamic multi-recipient NEP-199 royalty splitting for creators. |
| **RPC Throughput** | FastNEAR endpoint with Vite dev proxy failover | Dedicated private archive RPC nodes for enterprise SLA indexers. |
| **Fractional Ownership** | 1/1 NFTs + Multi-Token Editions (NEP-245) | ERC-20 / NEP-141 fractional vault integration. |

