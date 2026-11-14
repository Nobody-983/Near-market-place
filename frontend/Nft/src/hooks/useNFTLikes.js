import { useEffect, useRef, useState } from "react";
import { useWallet } from "../context/walletContext";
import { supabase } from "../lib/supabase";

const UUID_REGEX = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const TRUSTED_CONTRACTS = new Set([
  "nft-v2.roundfrog-dev.testnet",
  "market-v2.roundfrog-dev.testnet",
  "editions.roundfrog-dev.testnet",
  "nft.roundfrog-dev.testnet",
  "market.roundfrog-dev.testnet",
]);

export function extractTrustedTokenIdentifier(input) {
  if (!input || typeof input !== "string") return null;
  let decoded = input;
  try {
    decoded = decodeURIComponent(input);
  } catch {
    // preserve original string if decoding fails (handles malformed percent encoding safely)
  }
  const trimmed = decoded.trim();

  // 1. Composite listing ID: contract|token_id|seller|approval
  if (trimmed.includes("|")) {
    const parts = trimmed.split("|");
    if (parts.length >= 2 && TRUSTED_CONTRACTS.has(parts[0].toLowerCase())) {
      return parts[1].trim();
    }
    return null; // Unauthorized contract prefix in composite ID
  }

  // 2. Contract-scoped ID: contract:token_id or contract/token_id
  if (trimmed.includes(":") || trimmed.includes("/")) {
    const delimiter = trimmed.includes(":") ? ":" : "/";
    const parts = trimmed.split(delimiter);
    if (parts.length === 2 && TRUSTED_CONTRACTS.has(parts[0].toLowerCase())) {
      return parts[1].trim();
    }
    return null; // Unauthorized contract prefix in scoped ID
  }

  return trimmed;
}

export class LikedNFTsSet extends Set {
  has(key) {
    if (!key || typeof key !== "string") return false;
    if (super.has(key)) return true;

    if (key.startsWith("catalog-") && super.has(key.slice(8))) {
      return true;
    }

    // Contract-scoped or composite listing ID
    const trustedToken = extractTrustedTokenIdentifier(key);
    if (trustedToken && trustedToken !== key) {
      if (super.has(trustedToken)) return true;
      if (trustedToken.startsWith("catalog-") && super.has(trustedToken.slice(8))) {
        return true;
      }
      const parsedTrusted = parseUUID(trustedToken);
      if (parsedTrusted && super.has(parsedTrusted)) {
        return true;
      }
    }

    const parsed = parseUUID(key);
    if (parsed && super.has(parsed)) {
      return true;
    }

    return false;
  }
}

export function parseUUID(input) {
  if (!input || typeof input !== "string") return null;
  const token = extractTrustedTokenIdentifier(input);
  if (!token) return null;

  if (UUID_REGEX.test(token)) {
    return token.toLowerCase();
  }
  if (token.startsWith("catalog-")) {
    const candidate = token.slice(8);
    if (UUID_REGEX.test(candidate)) {
      return candidate.toLowerCase();
    }
  }

  return null;
}

export async function resolveNFTId(input, supabaseClient = supabase) {
  if (!input || typeof input !== "string") return null;

  const direct = parseUUID(input);
  const token = extractTrustedTokenIdentifier(input);
  if (!token && !direct) return null;

  try {
    const filters = [];
    if (direct) {
      filters.push(`id.eq.${direct}`, `token_id.eq.${direct}`, `token_id.eq.catalog-${direct}`);
    } else {
      // Non-UUID token identifiers (e.g., catalog-roundfrog-genesis-01)
      // Strictly prevent PostgREST filter injection: only permit safe characters [a-zA-Z0-9._-]
      // Disallow commas, parentheses, quotes, spaces, etc.
      if (!/^[a-zA-Z0-9._-]+$/.test(token)) {
        return null;
      }
      const cleanToken = token.startsWith("catalog-") ? token.slice(8) : token;
      filters.push(`token_id.eq.${token}`, `token_id.eq.catalog-${cleanToken}`, `token_id.eq.${cleanToken}`);
    }

    const { data } = await supabaseClient
      .from("nfts")
      .select("id")
      .or(filters.join(","))
      .maybeSingle();

    if (data?.id && UUID_REGEX.test(data.id)) {
      return data.id;
    }

    // Fallback: Check marketplace_listings by contract_sale_id if input is a composite listing ID
    if (input.includes("|") && !direct) {
      const { data: listingData } = await supabaseClient
        .from("marketplace_listings")
        .select("nft_id")
        .eq("contract_sale_id", input.trim())
        .maybeSingle();
      if (listingData?.nft_id && UUID_REGEX.test(listingData.nft_id)) {
        return listingData.nft_id;
      }
    }
  } catch {
    // lookup failed
  }
  return null;
}

export function useNFTLikes(scope = "") {
  // WalletContext exposes a user only after server verification of that session.
  const { user, walletAddress } = useWallet();
  const userId = user?.id || null;
  const identity = JSON.stringify([userId, walletAddress, scope]);
  const [state, setState] = useState({});
  const requestRef = useRef(null);
  // Hide the previous user's favorites immediately, even before effect cleanup.
  const current = state.identity === identity ? state : {
    liked: new LikedNFTsSet(), loading: !!userId, pending: null, error: "",
  };

  useEffect(() => {
    const request = { identity, active: true, busy: false };
    requestRef.current = request;
    const initial = { identity, liked: new LikedNFTsSet(), loading: false, pending: null, error: "" };

    const load = async () => {
      try {
        if (userId) {
          const { data, error } = await supabase.from("nft_likes")
            .select("nft_id").eq("user_id", userId);
          if (error) throw error;
          initial.liked = new LikedNFTsSet((data || []).map((like) => like.nft_id));
        }
        if (request.active) setState(initial);
      } catch (error) {
        if (request.active) setState({ ...initial, error: error?.message || "Unable to load favorites." });
      }
    };
    void load();
    return () => { request.active = false; };
  }, [identity, userId]);

  const toggleLike = async (nftId) => {
    if (!userId) {
      setState({ identity, ...current, error: "Connect and verify your NEAR wallet to save favorites." });
      return null;
    }
    const request = requestRef.current;
    if (!request?.active || request.identity !== identity || current.loading || request.busy) return null;
    request.busy = true;
    setState((previous) => ({ ...previous, pending: nftId, error: "" }));

    let resolvedId = parseUUID(nftId);
    if (!resolvedId) {
      resolvedId = await resolveNFTId(nftId);
    }

    if (!resolvedId) {
      request.busy = false;
      if (request.active) {
        setState((previous) => ({
          ...previous,
          pending: null,
          error: "This NFT is not registered in the catalog and cannot be favorited.",
        }));
      }
      return null;
    }

    const liked = !current.liked.has(resolvedId) && !current.liked.has(nftId);

    try {
      const query = liked
        ? supabase.from("nft_likes").insert({ nft_id: resolvedId, user_id: userId })
        : supabase.from("nft_likes").delete().eq("nft_id", resolvedId).eq("user_id", userId);
      const { data, error } = await query.select("nft_id, user_id").maybeSingle();

      if (error) throw error;
      if (data?.nft_id !== resolvedId || data?.user_id !== userId) {
        throw new Error("Favorite change was not confirmed. Reload and try again.");
      }
      if (!request.active) return null;

      setState((previous) => {
        const favorites = new LikedNFTsSet(previous.liked);
        if (liked) {
          favorites.add(resolvedId);
          if (nftId) favorites.add(nftId);
        } else {
          favorites.delete(resolvedId);
          if (nftId) favorites.delete(nftId);
        }
        return { ...previous, liked: favorites };
      });
      return { liked, change: liked ? 1 : -1, resolvedId };
    } catch (error) {
      const isCatalogError =
        error?.code === "23503" ||
        /foreign key/i.test(error?.message || "") ||
        /not registered in the catalog/i.test(error?.message || "");

      if (request.active) setState((previous) => ({
        ...previous,
        error: isCatalogError
          ? "This NFT is not registered in the catalog and cannot be favorited."
          : error?.message || "Unable to update this favorite.",
      }));
      return null;
    } finally {
      request.busy = false;
      if (request.active) setState((previous) => ({ ...previous, pending: null }));
    }
  };

  return {
    likedNFTs: current.liked, loadingLikes: current.loading,
    likingNFT: current.pending, likeError: current.error, toggleLike,
  };
}
