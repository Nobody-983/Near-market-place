import { useEffect, useRef, useState } from "react";
import { useWallet } from "../context/walletContext";
import { supabase } from "../lib/supabase";

export function useNFTLikes(scope = "") {
  // WalletContext exposes a user only after server verification of that session.
  const { user, walletAddress } = useWallet();
  const userId = user?.id || null;
  const identity = JSON.stringify([userId, walletAddress, scope]);
  const [state, setState] = useState({});
  const requestRef = useRef(null);
  // Hide the previous user's favorites immediately, even before effect cleanup.
  const current = state.identity === identity ? state : {
    liked: new Set(), loading: !!userId, pending: null, error: "",
  };

  useEffect(() => {
    const request = { identity, active: true, busy: false };
    requestRef.current = request;
    const initial = { identity, liked: new Set(), loading: false, pending: null, error: "" };

    const load = async () => {
      try {
        if (userId) {
          const { data, error } = await supabase.from("nft_likes")
            .select("nft_id").eq("user_id", userId);
          if (error) throw error;
          initial.liked = new Set((data || []).map((like) => like.nft_id));
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
    const liked = !current.liked.has(nftId);

    try {
      const query = liked
        ? supabase.from("nft_likes").insert({ nft_id: nftId, user_id: userId })
        : supabase.from("nft_likes").delete().eq("nft_id", nftId).eq("user_id", userId);
      const { data, error } = await query.select("nft_id, user_id").maybeSingle();
      if (error) throw error;
      if (data?.nft_id !== nftId || data?.user_id !== userId) {
        throw new Error("Favorite change was not confirmed. Reload and try again.");
      }
      if (!request.active) return null;

      setState((previous) => {
        const favorites = new Set(previous.liked);
        if (liked) favorites.add(nftId);
        else favorites.delete(nftId);
        return { ...previous, liked: favorites };
      });
      return { liked, change: liked ? 1 : -1 };
    } catch (error) {
      if (request.active) setState((previous) => ({
        ...previous, error: error?.message || "Unable to update this favorite.",
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
