
import { useEffect, useState, useMemo } from "react";
import {
  Heart,
  Search,
  Plus,
  ArrowUpRight,
  Loader2,
} from "lucide-react";
import { useNavigate } from "react-router-dom";

import { getMarketplaceListings } from "../services/marketService";
import { supabase } from "../lib/supabase";
import { useNFTLikes, parseUUID } from "../hooks/useNFTLikes";
import { useWallet } from "../context/walletContext";
import { editionListings, marketplaceListings, mtToken, nftToken } from "../lib/near/contract";
import { yoctoToNear } from "../lib/near/utils";
import { NFT_CONTRACT_ID } from "../config/near";

function resolveImageUrl(url) {
  if (!url || typeof url !== "string") return "/near-logo.svg";
  if (url.startsWith("ipfs://")) {
    return `https://ipfs.io/ipfs/${url.slice(7)}`;
  }
  if (url.startsWith("ipfs/")) {
    return `https://ipfs.io/ipfs/${url.slice(5)}`;
  }
  return url;
}

const categories = [
  "All",
  "RoundFrog Genesis",
  "Unique (1/1)",
  "Limited Editions",
  "Art",
  "Music",
  "Collectible",
  "Gaming",
];

function Marketplace() {
  const navigate = useNavigate();
  const { selector, accountId } = useWallet();
  const wallet = useMemo(
    () => (selector ? { selector, accountId } : undefined),
    [selector, accountId]
  );

  const [items, setItems] = useState([]);
  const [search, setSearch] = useState("");
  const [selectedCategory, setSelectedCategory] = useState("All");

  const { likedNFTs, loadingLikes, likingNFT, likeError, toggleLike: updateLike } = useNFTLikes();
  const [likeCounts, setLikeCounts] = useState({});
  const [likesReadError, setLikesReadError] = useState("");

  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  // =====================================================
  // LOAD LIKES
  // =====================================================

  const loadLikes = async () => {
    try {
      const { data: likes, error: likesError } = await supabase
        .from("nft_likes")
        .select("nft_id");

      if (likesError) throw likesError;

      const counts = {};

      likes?.forEach((like) => {
        if (!like?.nft_id) return;
        const id = String(like.nft_id).toLowerCase();
        counts[id] = (counts[id] || 0) + 1;
      });

      setLikeCounts(counts);
      setLikesReadError("");
    } catch (err) {
      console.error("LOAD LIKES ERROR:", err);
      setLikesReadError("Unable to load like counts.");
    }
  };

  // =====================================================
  // LOAD MARKETPLACE
  // =====================================================

  useEffect(() => {
    let isCancelled = false;

    const fetchMarketplace = async () => {
      try {
        setLoading(true);
        setError("");

        // 1. Fetch on-chain marketplace listings and database listings in parallel
        const [chainListingsRes, dbListingsRes, edListingsRes] = await Promise.allSettled([
          marketplaceListings(wallet),
          getMarketplaceListings(),
          editionListings(wallet),
        ]);

        if (isCancelled) return;

        // Propagate RPC errors if on-chain listing query fails
        if (chainListingsRes.status === "rejected") {
          console.error("ON-CHAIN MARKETPLACE LISTINGS ERROR:", chainListingsRes.reason);
          throw new Error(
            chainListingsRes.reason?.message || "Failed to query on-chain marketplace listings from NEAR RPC."
          );
        }

        const chainListings = chainListingsRes.value || [];
        const dbListings = dbListingsRes.status === "fulfilled" ? dbListingsRes.value || [] : [];
        const edListings = edListingsRes.status === "fulfilled" ? edListingsRes.value || [] : [];

        // 2. Genuine active on-chain listings from market-v2
        const activeChainListings = (chainListings || []).filter((cl) => {
          if (!cl || !cl.active) return false;
          const tid = String(cl.token_id || "").toLowerCase();
          if (
            tid.includes("-test-") ||
            tid.includes("-test3-") ||
            tid.includes("-test4-") ||
            tid.includes("-test5-") ||
            tid.includes("acceptance")
          ) {
            return false;
          }
          return true;
        });

        // 3. Map on-chain active listings with real token metadata
        const mappedChainListings = await Promise.all(
          activeChainListings.map(async (cl) => {
            const rawTokenId = cl.token_id;
            const strippedUuid = rawTokenId?.startsWith("catalog-") ? rawTokenId.slice(8) : null;
            const existingDb = dbListings.find(
              (dbl) =>
                dbl.contract_sale_id === cl.listing_id ||
                dbl.contract_listing_id === cl.listing_id ||
                dbl.token_id === rawTokenId ||
                dbl.nfts?.token_id === rawTokenId ||
                dbl.nfts?.id === rawTokenId ||
                (strippedUuid && (dbl.nfts?.id === strippedUuid || dbl.nft_id === strippedUuid || dbl.nfts?.token_id === strippedUuid))
            );

            let meta = null;
            try {
              meta = await nftToken(cl.token_id, wallet, cl.nft_contract_id || NFT_CONTRACT_ID);
            } catch (metaErr) {
              console.warn(`Could not load token metadata for ${cl.token_id}:`, metaErr);
            }

            let extra = {};
            try {
              if (meta?.metadata?.extra) {
                extra = typeof meta.metadata.extra === "string"
                  ? JSON.parse(meta.metadata.extra)
                  : meta.metadata.extra;
              }
            } catch {
              extra = {};
            }

            const title = meta?.metadata?.title || existingDb?.nfts?.name;
            const imageUrl = meta?.metadata?.media || extra?.media_url || existingDb?.nfts?.image_url;

            // Exclude items with invalid or missing catalog records
            if (!title || !imageUrl) return null;
            const tLower = title.toLowerCase();
            if (
              tLower.includes("test genesis") ||
              tLower.includes("test with hash") ||
              tLower.includes("test extra") ||
              tLower.includes("test relative media") ||
              tLower.includes("acceptance genesis")
            ) {
              return null;
            }

            const description = meta?.metadata?.description || existingDb?.nfts?.description || "";
            const category = extra?.category || existingDb?.nfts?.category || "Collectible";

            return {
              id: cl.listing_id,
              contract_sale_id: cl.listing_id,
              contract_listing_id: cl.listing_id,
              token_id: cl.token_id,
              price: yoctoToNear(cl.price),
              currency: "NEAR",
              status: "active",
              isEdition: false,
              created_at: cl.created_at,
              nfts: {
                id: existingDb?.nfts?.id || cl.token_id,
                token_id: cl.token_id,
                name: title,
                description,
                image_url: imageUrl,
                category,
                isEdition: false,
              },
              profiles: {
                display_name: existingDb?.profiles?.display_name || cl.seller,
                username: existingDb?.profiles?.username || cl.seller,
                avatar_url: existingDb?.profiles?.avatar_url || null,
              },
            };
          })
        );

        // 4. Map active edition listings
        const mappedEditions = [];
        for (const ed of edListings || []) {
          if (!ed.active || ed.available_quantity === "0") continue;
          let meta = null;
          try {
            meta = await mtToken(ed.token_id, wallet);
          } catch {
            // view fallback
          }
          mappedEditions.push({
            id: ed.listing_id,
            price: yoctoToNear(ed.price_per_copy),
            currency: "NEAR",
            status: "active",
            isEdition: true,
            availableQuantity: Number(ed.available_quantity),
            nfts: {
              id: ed.token_id,
              name: meta?.metadata?.title || ed.token_id,
              description: meta?.metadata?.description || "",
              image_url: meta?.metadata?.media || "/near-logo.svg",
              category: "Collectible",
              isEdition: true,
              supply: meta?.supply ? String(meta.supply) : ed.initial_quantity,
            },
            profiles: {
              display_name: ed.seller,
              username: ed.seller,
            },
          });
        }

        // 5. Deduplicate and filter verified active listings
        const validChainListings = mappedChainListings.filter(Boolean);
        const seenTokens = new Set();
        const uniqueListings = [];
        for (const item of [...validChainListings, ...mappedEditions]) {
          const key = `${item.token_id}`;
          if (!seenTokens.has(key)) {
            seenTokens.add(key);
            uniqueListings.push(item);
          }
        }

        if (!isCancelled) {
          setItems(uniqueListings);
          await loadLikes();
        }
      } catch (err) {
        console.error("MARKETPLACE ERROR:", err);
        if (!isCancelled) {
          setError(err?.message || "Unable to load marketplace.");
        }
      } finally {
        if (!isCancelled) {
          setLoading(false);
        }
      }
    };

    fetchMarketplace();

    return () => {
      isCancelled = true;
    };
  }, [wallet]);

  // =====================================================
  // LIKE / UNLIKE
  // =====================================================

  const toggleLike = async (nftId) => {
    const result = await updateLike(nftId);
    if (result) {
      setLikeCounts((previous) => ({
        ...previous,
        [nftId]: Math.max(0, (previous[nftId] || 0) + result.change),
        ...(result.resolvedId ? { [result.resolvedId]: Math.max(0, (previous[result.resolvedId] || 0) + result.change) } : {}),
      }));
    }
  };

  // =====================================================
  // FILTER
  // =====================================================

  const filteredItems = items.filter((item) => {
    const nft = item.nfts;
    const seller = item.profiles;

    if (!nft) {
      return false;
    }

    const searchValue = search.toLowerCase();

    const title = nft.name?.toLowerCase() || "";

    const creator =
      seller?.display_name?.toLowerCase() ||
      seller?.username?.toLowerCase() ||
      "";

    const matchesSearch =
      title.includes(searchValue) ||
      creator.includes(searchValue);

    const matchesCategory =
      selectedCategory === "All" ||
      (selectedCategory === "Unique (1/1)" && !item.isEdition && !nft.isEdition) ||
      (selectedCategory === "Limited Editions" && (item.isEdition || nft.isEdition)) ||
      nft.category === selectedCategory;

    return matchesSearch && matchesCategory;
  });

  return (
    <div className="min-h-screen bg-black px-4 py-6 text-white sm:px-6">

      {/* HEADER */}

      <div className="mb-8 flex flex-col gap-5 md:flex-row md:items-center md:justify-between">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">
            Marketplace
          </h1>

          <p className="mt-3 text-sm text-gray-400">
            Discover, collect and trade unique digital assets.
          </p>
        </div>

        <button
          type="button"
          onClick={() => navigate("/create-nft")}
          className="flex w-fit items-center gap-2 rounded-xl bg-purple-600 px-5 py-3 text-sm font-semibold transition hover:bg-purple-700"
        >
          <Plus size={18} />
          Create NFT
        </button>
      </div>

      {/* SEARCH + CATEGORIES */}

      <div className="mb-8 flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
        <div className="relative w-full lg:max-w-md">
          <Search
            size={18}
            className="absolute left-4 top-1/2 -translate-y-1/2 text-gray-500"
          />

          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search NFTs, creators..."
            className="w-full rounded-xl border border-white/10 bg-white/[0.04] py-3 pl-11 pr-5 text-sm text-white outline-none transition placeholder:text-gray-500 focus:border-purple-500"
          />
        </div>

        <div className="flex gap-2 overflow-x-auto pb-1">
          {categories.map((category) => (
            <button
              type="button"
              key={category}
              onClick={() => setSelectedCategory(category)}
              className={`whitespace-nowrap rounded-xl px-5 py-2.5 text-sm font-medium transition ${
                selectedCategory === category
                  ? "bg-purple-600 text-white"
                  : "bg-white/[0.04] text-gray-400 hover:bg-white/[0.08] hover:text-white"
              }`}
            >
              {category}
            </button>
          ))}
        </div>
      </div>

      {/* LOADING */}

      {loading && (
        <div className="flex min-h-[300px] items-center justify-center">
          <Loader2
            size={30}
            className="animate-spin text-purple-500"
          />
        </div>
      )}

      {/* ERROR */}

      {!loading && error && (
        <div className="rounded-2xl border border-red-500/20 bg-red-500/10 py-12 text-center">
          <p className="text-lg font-semibold text-red-400">
            {error}
          </p>

          <p className="mt-2 text-sm text-gray-500">
            Please try again later.
          </p>
        </div>
      )}

      {/* NFT GRID */}

      {(likeError || likesReadError) && (
        <p role="alert" className="mb-4 text-sm text-red-400">
          {likeError || likesReadError}
        </p>
      )}

      {!loading &&
        !error &&
        filteredItems.length > 0 && (
          <div className="grid grid-cols-1 gap-5 sm:grid-cols-2 xl:grid-cols-3">
            {filteredItems.map((item) => {
              const nft = item.nfts;
              const seller = item.profiles;

              const creator =
                seller?.display_name ||
                seller?.username ||
                "Unknown creator";

              const nftId = nft.id;
              const id = String(nftId);
              const canonicalUuid = parseUUID(id) || (nft.token_id && parseUUID(nft.token_id));

              const isLiked = likedNFTs.has(id) || (canonicalUuid && likedNFTs.has(canonicalUuid)) || (nft.token_id && likedNFTs.has(nft.token_id));
              const totalLikes = (canonicalUuid && likeCounts[canonicalUuid]) || likeCounts[id] || (nft.token_id && likeCounts[nft.token_id]) || 0;

              return (
                <div
                  key={item.id}
                  className="group overflow-hidden rounded-2xl border border-white/10 bg-white/[0.03] transition-colors duration-300 hover:-translate-y-1 hover:border-purple-500/40"
                >

                  {/* IMAGE */}

                  <div className="relative aspect-[4/3] overflow-hidden bg-white/5">
                    <img
                      src={resolveImageUrl(nft.image_url)}
                      alt={nft.name}
                      loading="lazy"
                      onError={(e) => {
                        e.currentTarget.onerror = null;
                        e.currentTarget.src = "/near-logo.svg";
                      }}
                      className="h-full w-full object-cover transition duration-500 group-hover:scale-105"
                    />

                    <div className="absolute inset-0 bg-gradient-to-t from-black/50 via-transparent to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100" />

                    {/* CATEGORY & EDITION BADGE */}

                    <div className="absolute left-3 top-3 flex items-center gap-1.5 flex-wrap">
                      <div className="rounded-lg bg-black/60 px-3 py-1.5 text-xs font-medium backdrop-blur-md">
                        {nft.category}
                      </div>
                      {(item.isEdition || nft.isEdition) && (
                        <div className="rounded-lg bg-purple-600/80 px-2.5 py-1.5 text-xs font-semibold backdrop-blur-md text-white">
                          Edition ({item.availableQuantity ?? nft.supply} left)
                        </div>
                      )}
                    </div>

                    {/* LIKE BUTTON */}

                    <button
                      type="button"
                      onClick={() => toggleLike(nftId)}
                      disabled={loadingLikes || !!likingNFT}
                      aria-pressed={isLiked}
                      aria-label={
                        isLiked
                          ? "Unlike NFT"
                          : "Like NFT"
                      }
                      className={`absolute right-3 top-3 flex h-10 w-10 items-center justify-center rounded-full backdrop-blur-md transition-all duration-200 ${
                        isLiked
                          ? "scale-105 bg-purple-600 text-white"
                          : "bg-black/60 text-white hover:scale-105 hover:bg-purple-600"
                      }`}
                    >
                      {likingNFT === id ? (
                        <Loader2
                          size={18}
                          className="animate-spin"
                        />
                      ) : (
                        <Heart
                          size={18}
                          strokeWidth={2}
                          fill={
                            isLiked
                              ? "currentColor"
                              : "none"
                          }
                        />
                      )}
                    </button>

                    {/* LIKE COUNT */}

                    <div className="absolute bottom-3 right-3 flex items-center gap-1.5 rounded-full bg-black/60 px-2.5 py-1.5 text-xs backdrop-blur-md">
                      <Heart
                        size={13}
                        fill={
                          isLiked
                            ? "currentColor"
                            : "none"
                        }
                        className={
                          isLiked
                            ? "text-purple-400"
                            : "text-white"
                        }
                      />

                      <span>{totalLikes}</span>
                    </div>
                  </div>

                  {/* DETAILS */}

                  <div className="p-5">
                    <div className="mb-4">
                      <h2 className="text-lg font-semibold">
                        {nft.name}
                      </h2>

                      <p className="mt-1 text-sm text-gray-500">
                        by {creator}
                      </p>
                    </div>

                    <div className="flex items-end justify-between">
                      <div>
                        <p className="text-xs text-gray-500">
                          Current price
                        </p>

                        <p className="mt-1 text-base font-semibold">
                          {item.price} {item.currency || "NEAR"}
                        </p>
                      </div>

                      <button
                        type="button"
                        onClick={() =>
                          navigate(`/nft/${nft.id}`)
                        }
                        className="flex items-center gap-1.5 rounded-xl border border-purple-500/40 px-4 py-2 text-sm font-medium text-purple-400 transition hover:bg-purple-600 hover:text-white"
                      >
                        View NFT
                        <ArrowUpRight size={15} />
                      </button>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        )}

      {/* EMPTY */}

      {!loading &&
        !error &&
        filteredItems.length === 0 && (
          <div className="py-16 text-center">
            <p className="text-lg font-semibold">
              No NFTs found
            </p>

            <p className="mt-2 text-sm text-gray-500">
              {items.length === 0
                ? "No NFTs have been listed for sale yet."
                : "Try another search or category."}
            </p>
          </div>
        )}
    </div>
  );
}

export default Marketplace;
