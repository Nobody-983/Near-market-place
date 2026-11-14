import {
  MARKETPLACE_CONTRACT_ID,
  NETWORK_ID,
  NFT_CONTRACT_ID,
  REVIEWED_MARKETPLACE_CONTRACT_ID,
  REVIEWED_NFT_CONTRACT_ID,
} from "./near.js";

const runtimeEnv = import.meta.env || {};

export function isValidTestnetContractId(accountId) {
  return typeof accountId === "string" &&
    accountId.length <= 64 &&
    /^[a-z0-9]+(?:[._-][a-z0-9]+)*\.testnet$/.test(accountId);
}

const reviewedContractsConfigured =
  NETWORK_ID === "testnet" &&
  NFT_CONTRACT_ID === REVIEWED_NFT_CONTRACT_ID &&
  MARKETPLACE_CONTRACT_ID === REVIEWED_MARKETPLACE_CONTRACT_ID &&
  isValidTestnetContractId(NFT_CONTRACT_ID) &&
  isValidTestnetContractId(MARKETPLACE_CONTRACT_ID) &&
  NFT_CONTRACT_ID !== MARKETPLACE_CONTRACT_ID;

// Listing/cancellation is a reviewed release decision, not a deployment side
// effect. The build-time switch may only be enabled after real Testnet receipts,
// the catalog indexer and hosted reconciliation have been reviewed. Every action
// also checks the pinned contract code and live state before opening the wallet.
export const LISTING_ACTIONS_ENABLED =
  runtimeEnv.VITE_TESTNET_LISTING_FLOW_VERIFIED === "true" &&
  reviewedContractsConfigured;
export const PURCHASES_ENABLED =
  runtimeEnv.VITE_TESTNET_PURCHASE_FLOW_VERIFIED === "true" &&
  reviewedContractsConfigured;
export const MINTING_ACTIONS_ENABLED =
  runtimeEnv.VITE_TESTNET_MINT_FLOW_VERIFIED === "true" &&
  reviewedContractsConfigured;

export const LISTING_UNAVAILABLE_MESSAGE =
  "Listing and cancellation are disabled until finalized Testnet receipts, reviewed contract code, and trusted catalog reconciliation are verified. Browser-side listing writes remain blocked.";

export const PURCHASE_UNAVAILABLE_MESSAGE =
  "Purchases are temporarily unavailable while we complete sale and payment verification. You can still browse NFTs.";

export const MINTING_UNAVAILABLE_MESSAGE =
  "On-chain minting is disabled until the Testnet mint authority, reviewed contract code, and complete mint-to-catalog lifecycle are verified.";

// The reviewed deploy contract reserves 8 KiB for each possible sale in
// addition to listing metadata. This conservative target is checked against the
// seller's live NEP-145 available balance and any shortfall is funded before an
// approval is submitted. It is deliberately configurable for a future reviewed
// contract revision, but cannot itself enable actions.
export const LISTING_STORAGE_TARGET_YOCTO =
  runtimeEnv.VITE_MARKETPLACE_LISTING_STORAGE_YOCTO ||
  "120000000000000000000000"; // 0.12 NEAR

// Visibility Policy: Exclude standalone QA and acceptance test tokens from public inventory
export function isProductionCatalogToken(tokenId, title = "") {
  if (!tokenId || typeof tokenId !== "string") return false;
  const tid = tokenId.toLowerCase();
  const t = (title || "").toLowerCase();
  if (
    tid.includes("-test") ||
    tid.includes("acceptance") ||
    t.includes("test genesis") ||
    t.includes("test with hash") ||
    t.includes("test extra") ||
    t.includes("test relative media") ||
    t.includes("acceptance genesis")
  ) {
    return false;
  }
  return true;
}
