
import { supabase } from "../lib/supabase";
import { PREVIEW_LISTINGS } from "../lib/previewData";
import { PREVIEW_DATA_ENABLED } from "../config/preview";
import { validateNearAmount } from "../lib/near/utils";

// Cast before JSON encoding; converting a decoded numeric price back to text loses precision.
export const LISTING_SELECT_FIELDS = `
  id, nft_id, seller_id, buyer_id, price::text, currency, status, network_id,
  marketplace_contract_id, contract_sale_id, tx_hash, onchain_status, sold_at,
  listing_tx_hash, cancellation_tx_hash, approval_id::text, listing_generation::text,
  created_at, updated_at
`;

function validateListingPrice(listing) {
  return { ...listing, price: validateNearAmount(listing.price, { allowZero: false }) };
}

const MARKETPLACE_TABLE = "marketplace_listings";
const ACTIVE_STATUS = "active";

// Preview/demo listings are display-only and must stay opt-in. Real listings
// are never replaced by demo rows, even when the catalog is empty or failing.
export function selectMarketplaceListings(data, { previewEnabled = PREVIEW_DATA_ENABLED } = {}) {
  if (Array.isArray(data) && data.length > 0) return data.map(validateListingPrice);
  return previewEnabled ? PREVIEW_LISTINGS : [];
}

// ================= GET MARKETPLACE LISTINGS =================

export async function getMarketplaceListings() {
  const { data, error } = await supabase
    .from(MARKETPLACE_TABLE)
    .select(`
      ${LISTING_SELECT_FIELDS},
      nfts (
        id,
        name,
        description,
        image_url,
        category,
        creator_id
      ),
      profiles:seller_id (
        id,
        username,
        display_name,
        avatar_url
      )
    `)
    .eq("status", ACTIVE_STATUS)
    .order("created_at", { ascending: false });

  if (error) {
    console.error("GET MARKETPLACE LISTINGS ERROR:", error.message);
    throw new Error("Unable to load marketplace listings.");
  }

  return selectMarketplaceListings(data);
}

// ================= GET CURRENT USER =================

async function getCurrentUser() {
  const {
    data: { user },
    error,
  } = await supabase.auth.getUser();

  if (error) {
    console.error("GET CURRENT USER ERROR:", error);
    throw error;
  }

  if (!user) {
    throw new Error("You must be logged in.");
  }

  return user;
}

// Listing and cancellation writes are intentionally absent here. Only the
// service-role catalog indexer may call publish_verified_listing or
// cancel_verified_listing after verifying final NEAR receipts and historical
// contract state. Browser code can only wait for those trusted projections.

export async function getIndexedListingByTransaction(transactionHash) {
  if (!transactionHash) throw new Error("Transaction hash is required.");
  const { data, error } = await supabase
    .from(MARKETPLACE_TABLE)
    .select(LISTING_SELECT_FIELDS)
    .eq("listing_tx_hash", transactionHash)
    .maybeSingle();
  if (error) throw new Error("Unable to verify the indexed listing.");
  return data ? validateListingPrice(data) : null;
}

export async function getIndexedListingByContractId(contractListingId) {
  if (!contractListingId) throw new Error("Contract listing ID is required.");
  const { data, error } = await supabase
    .from(MARKETPLACE_TABLE)
    .select(LISTING_SELECT_FIELDS)
    .eq("contract_sale_id", contractListingId)
    .maybeSingle();
  if (error) throw new Error("Unable to verify the indexed listing.");
  return data ? validateListingPrice(data) : null;
}

export async function waitForIndexedListing({
  transactionHash,
  contractListingId,
  status,
  timeoutMs = 45_000,
}) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const listing = transactionHash
      ? await getIndexedListingByTransaction(transactionHash)
      : await getIndexedListingByContractId(contractListingId);
    if (listing && (!status || listing.status === status)) return listing;
    await new Promise((resolve) => setTimeout(resolve, 1500));
  }
  return null;
}

// ================= GET USER LISTINGS =================

export async function getUserListings(userId) {
  if (!userId) {
    throw new Error("User ID is required.");
  }

  const user = await getCurrentUser();

  if (user.id !== userId) {
    throw new Error(
      "You are not authorized to view these listings."
    );
  }

  const { data, error } = await supabase
    .from(MARKETPLACE_TABLE)
    .select(`
      ${LISTING_SELECT_FIELDS},
      nfts (
        id,
        name,
        description,
        image_url,
        category,
        creator_id
      )
    `)
    .eq("seller_id", user.id)
    .order("created_at", { ascending: false });

  if (error) {
    console.error("GET USER LISTINGS ERROR:", error);
    throw error;
  }

  return (data || []).map(validateListingPrice);
}
