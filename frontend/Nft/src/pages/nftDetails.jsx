import { useCallback, useEffect, useState } from "react";
import { useParams, useNavigate } from "react-router-dom";
import {
  ArrowLeft,
  Heart,
  Share2,
  User,
  Loader2,
  ShoppingCart,
  Tag,
  X,
  Upload,
} from "lucide-react";

import { supabase } from "../lib/supabase";
import { useWallet } from "../context/walletContext";
import {
  MARKETPLACE_CONTRACT_ID,
  NFT_CONTRACT_ID,
  NETWORK_ID,
} from "../config/near";
import {
  PURCHASES_ENABLED,
  PURCHASE_UNAVAILABLE_MESSAGE,
  LISTING_ACTIONS_ENABLED,
  LISTING_UNAVAILABLE_MESSAGE,
  MINTING_ACTIONS_ENABLED,
  MINTING_UNAVAILABLE_MESSAGE,
} from "../config/marketplace";
import {
  buyNftOnChain,
  assertFinalSuccess,
  cancelListingOnChain,
  ensureMarketplaceListingStorage,
  getMintAuthority,
  listingIdFromFinalResult,
  listNftOnChain,
  marketplaceListing,
  marketplaceListingMetadata,
  marketplaceListings,
  mintNftOnChain,
  nftToken,
  requireConnectedAccount,
  transactionHash,
  verifyListingPreconditions,
  waitForFinal,
  waitForSettlement,
} from "../lib/near/contract.js";
import { nearToYocto, validateNearAmount, yoctoToNear } from "../lib/near/utils";
import { LISTING_SELECT_FIELDS, waitForIndexedListing } from "../services/marketService";
import { NFT_SELECT_FIELDS } from "../services/nftService";
import { PREVIEW_NFTS, PREVIEW_LISTINGS } from "../lib/previewData";
import { PREVIEW_DATA_ENABLED } from "../config/preview";
import { useNFTLikes } from "../hooks/useNFTLikes";

function NFTDetails() {
  const { id } = useParams();
  const navigate = useNavigate();
  const { walletAddress, isConnected, selector, user, refreshBalance } = useWallet();

  const [nft, setNft] = useState(null);
  const [listing, setListing] = useState(null);
  const [ownership, setOwnership] = useState(null);
  const [loading, setLoading] = useState(true);
  const [buying, setBuying] = useState(false);
  const [listingLoading, setListingLoading] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [minting, setMinting] = useState(false);
  const [showListingModal, setShowListingModal] = useState(false);
  const [listingPrice, setListingPrice] = useState("");
  const [error, setError] = useState("");
  const [success, setSuccess] = useState("");
  const [likes, setLikes] = useState(0);
  const [mintAuthority, setMintAuthority] = useState(null);
  const { likedNFTs, loadingLikes, likingNFT, likeError, toggleLike } = useNFTLikes(id);
  const liked = likedNFTs.has(id);

  useEffect(() => {
    let mounted = true;
    getMintAuthority()
      .then((auth) => {
        if (mounted && auth) setMintAuthority(auth);
      })
      .catch((e) => {
        void e;
      });
    return () => {
      mounted = false;
    };
  }, []);

  const fetchNFTDetails = useCallback(async () => {
    try {
      setLoading(true);
      setError("");

      const previewItem = PREVIEW_DATA_ENABLED
        ? PREVIEW_NFTS.find((p) => String(p.id) === String(id))
        : null;
      if (previewItem) {
        setNft(previewItem);
        const previewListing = PREVIEW_LISTINGS.find((l) => String(l.nft_id) === String(id));
        setListing(previewListing || null);
        setOwnership(null);
        setLikes(3);
        setLoading(false);
        return;
      }

      const isUuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(id || "");
      let nftData = null;

      if (isUuid) {
        const { data, error: nftError } = await supabase
          .from("nfts")
          .select(`
            ${NFT_SELECT_FIELDS},
            profiles:creator_id (
              id,
              username,
              display_name,
              avatar_url,
              wallet_address
            )
          `)
          .eq("id", id)
          .maybeSingle();

        if (nftError) {
          throw nftError;
        }
        nftData = data;
      } else {
        const { data, error: nftError } = await supabase
          .from("nfts")
          .select(`
            ${NFT_SELECT_FIELDS},
            profiles:creator_id (
              id,
              username,
              display_name,
              avatar_url,
              wallet_address
            )
          `)
          .eq("token_id", id)
          .maybeSingle();

        if (nftError) {
          throw nftError;
        }
        nftData = data;
      }

      const tokenId = nftData?.token_id || (!isUuid ? id : null);
      let onChainToken = null;
      if (tokenId) {
        try {
          const wallet = selector ? { selector, accountId: walletAddress } : undefined;
          onChainToken = await nftToken(tokenId, wallet);
        } catch (chainErr) {
          console.warn("Could not query on-chain token:", chainErr);
        }
      }

      if (!nftData && !onChainToken) {
        throw new Error("NFT not found");
      }

      if (nftData) {
        setNft({
          ...nftData,
          price: validateNearAmount(nftData.price),
          on_chain_owner: onChainToken?.owner_id || null,
        });
      } else if (onChainToken) {
        let extra = {};
        try {
          extra = onChainToken.metadata?.extra ? JSON.parse(onChainToken.metadata.extra) : {};
        } catch (e) {
          void e;
        }
        setNft({
          id: onChainToken.token_id,
          token_id: onChainToken.token_id,
          nft_contract_id: NFT_CONTRACT_ID,
          name: onChainToken.metadata?.title || onChainToken.token_id,
          description: onChainToken.metadata?.description || "",
          image_url: onChainToken.metadata?.media || "/placeholder.png",
          category: extra.category || "Art",
          price: "0",
          currency: "NEAR",
          on_chain_owner: onChainToken.owner_id,
        });
      }

      let ownershipData = null;
      if (isUuid) {
        const { data } = await supabase
          .from("nft_ownership")
          .select(`
            owner_id,
            acquired_at,
            profiles:owner_id (
              id,
              username,
              display_name,
              avatar_url,
              wallet_address
            )
          `)
          .eq("nft_id", id)
          .maybeSingle();

        ownershipData = data;
      }
      setOwnership(ownershipData || null);

      let listingData = null;
      if (isUuid) {
        const { data } = await supabase
          .from("marketplace_listings")
          .select(`
            ${LISTING_SELECT_FIELDS},
            profiles:seller_id (
              id,
              username,
              display_name,
              avatar_url,
              wallet_address
            )
          `)
          .eq("nft_id", id)
          .eq("status", "active")
          .maybeSingle();

        listingData = data;
      }

      if (!listingData && tokenId) {
        try {
          const wallet = selector ? { selector, accountId: walletAddress } : undefined;
          const chainListings = await marketplaceListings(wallet);
          const activeChainListing = (chainListings || []).find(
            (l) => l.active && l.token_id === tokenId
          );
          if (activeChainListing) {
            listingData = {
              id: activeChainListing.listing_id,
              contract_sale_id: activeChainListing.listing_id,
              contract_listing_id: activeChainListing.listing_id,
              token_id: activeChainListing.token_id,
              nft_id: nftData?.id || activeChainListing.token_id,
              seller_id: activeChainListing.seller,
              seller: activeChainListing.seller,
              price: yoctoToNear(activeChainListing.price),
              currency: "NEAR",
              status: "active",
              network_id: NETWORK_ID,
              marketplace_contract_id: MARKETPLACE_CONTRACT_ID,
            };
          }
        } catch (chainListingErr) {
          console.warn("Could not query on-chain listing:", chainListingErr);
        }
      }

      setListing(listingData ? {
        ...listingData, price: validateNearAmount(listingData.price, { allowZero: false }),
      } : null);

      // Likes
      if (isUuid) {
        const { count: likeCount, error: likesError } = await supabase
          .from("nft_likes")
          .select("*", { count: "exact", head: true })
          .eq("nft_id", id);
        if (likesError) throw likesError;
        setLikes(likeCount || 0);
      }
    } catch (err) {
      console.error(err);
      setError("Failed to load NFT details");
    } finally {
      setLoading(false);
    }
  }, [id, selector, walletAddress]);

  useEffect(() => {
    void Promise.resolve().then(fetchNFTDetails);
  }, [fetchNFTDetails]);

  const onChainOwner = nft?.on_chain_owner;
  const ownerWalletAddress =
    onChainOwner ||
    ownership?.profiles?.wallet_address ||
    listing?.profiles?.wallet_address ||
    listing?.seller ||
    (nft?.token_id ? null : nft?.profiles?.wallet_address) ||
    null;

  const isOwner = Boolean(
    walletAddress && (
      (onChainOwner && onChainOwner.toLowerCase() === walletAddress.toLowerCase()) ||
      (ownerWalletAddress && ownerWalletAddress.toLowerCase() === walletAddress.toLowerCase()) ||
      (ownership?.owner_id && user?.id && ownership.owner_id === user.id) ||
      (listing?.seller_id && user?.id && listing.seller_id === user.id) ||
      (listing?.seller && listing.seller.toLowerCase() === walletAddress.toLowerCase()) ||
      (nft?.creator_id && user?.id && nft.creator_id === user.id && !ownership?.owner_id)
    )
  );

  const handleBuy = async () => {
    if (!PURCHASES_ENABLED) {
      setSuccess("");
      setError(PURCHASE_UNAVAILABLE_MESSAGE);
      return;
    }

    if (nft?.is_preview) {
      setError("Preview Mode: This is display preview data. Live purchases will be enabled once smart contracts are deployed to NEAR Testnet.");
      return;
    }

    if (!isConnected || !walletAddress) {
      setError("Please connect your NEAR wallet first");
      return;
    }

    if (!listing) {
      setError("This NFT is not listed for sale");
      return;
    }

    setBuying(true);
    setError("");
    setSuccess("");

    try {
      const price = validateNearAmount(listing.price, { allowZero: false });
      if (!MARKETPLACE_CONTRACT_ID) throw new Error(PURCHASE_UNAVAILABLE_MESSAGE);
      if (!listing.contract_sale_id) throw new Error("Listing is missing its verified on-chain sale ID");
      if (listing.network_id !== NETWORK_ID) throw new Error("Listing belongs to a different NEAR network");
      if (listing.marketplace_contract_id !== MARKETPLACE_CONTRACT_ID) {
        throw new Error("Listing belongs to a different marketplace contract");
      }
      if (!selector) throw new Error("Wallet selector not initialized");
      const wallet = { selector, accountId: walletAddress };
      await requireConnectedAccount(walletAddress, wallet);
      const [chainListing, chainMeta] = await Promise.all([
        marketplaceListing(listing.contract_sale_id, wallet),
        marketplaceListingMetadata(listing.contract_sale_id, wallet),
      ]);
      if (!chainListing?.active || chainMeta?.status !== "active") {
        throw new Error("This listing is no longer active on NEAR.");
      }
      if (chainListing.nft_contract_id !== nft.nft_contract_id || chainListing.token_id !== nft.token_id) {
        throw new Error("The catalog listing does not match the on-chain NFT.");
      }
      const priceYocto = nearToYocto(price, { allowZero: false });
      if (chainListing.price !== priceYocto) throw new Error("The listing price changed. Refresh before purchasing.");
      if (chainListing.seller === walletAddress) throw new Error("A seller cannot buy their own NFT.");
      // The wallet/chain checks funds; the UI balance is an approximate display.

      const result = await buyNftOnChain(
        listing.contract_sale_id,
        priceYocto,
        wallet,
      );
      const txHash = transactionHash(result);
      const finalResult = await waitForFinal(txHash, walletAddress, wallet);
      assertFinalSuccess(finalResult, "NFT purchase");
      await requireConnectedAccount(walletAddress, wallet);
      const saleId = `${listing.contract_sale_id}|${walletAddress}|${chainMeta.generation}`;
      const settlement = await waitForSettlement(saleId, wallet);
      await requireConnectedAccount(walletAddress, wallet);
      if (settlement.status === "refund_available" || settlement.status === "refunded") {
        setError("The NFT transfer failed. Your full purchase amount is available as an on-chain refund in Wallet.");
      } else if (settlement.status === "settled") {
        setSuccess("Purchase settled on NEAR. The trusted worker is updating the marketplace record.");
      } else {
        throw new Error("Marketplace settlement returned an unknown state.");
      }

      if (walletAddress) refreshBalance(walletAddress);
      setTimeout(() => fetchNFTDetails(), 3000);
    } catch (err) {
      console.error(err);
      setError(err?.message || "Failed to complete purchase");
    } finally {
      setBuying(false);
    }
  };

  const openListingForm = () => {
    if (!LISTING_ACTIONS_ENABLED) {
      setSuccess("");
      setError(LISTING_UNAVAILABLE_MESSAGE);
      return;
    }
    setError("");
    setSuccess("");
    setListingPrice(nft?.price || "");
    setShowListingModal(true);
  };

  const handleListNFT = async (e) => {
    e.preventDefault();

    if (!LISTING_ACTIONS_ENABLED) {
      setSuccess("");
      setError(LISTING_UNAVAILABLE_MESSAGE);
      return;
    }

    if (listingLoading) return;

    setError("");
    setSuccess("");

    if (!user?.id) {
      setError("You must be logged in to list an NFT.");
      return;
    }

    let price;
    try {
      price = validateNearAmount(listingPrice, { allowZero: false });
    } catch (err) {
      setError(err.message);
      return;
    }

    try {
      setListingLoading(true);

      if (!isConnected || !walletAddress || !selector) {
        throw new Error("Connect the NEAR wallet that owns this NFT before listing.");
      }
      const nftContractId = nft?.nft_contract_id || NFT_CONTRACT_ID;
      const tokenId = nft?.token_id || nft?.id;
      if (!tokenId) {
        throw new Error("This catalog NFT has not been bound to an on-chain NEAR token.");
      }
      const wallet = { selector, accountId: walletAddress };
      await requireConnectedAccount(walletAddress, wallet);
      await verifyListingPreconditions({
        nftContractId,
        tokenId,
        sellerId: walletAddress,
        wallet,
      });
      await ensureMarketplaceListingStorage({ sellerId: walletAddress, wallet });
      await requireConnectedAccount(walletAddress, wallet);
      const result = await listNftOnChain({
        nftContractId,
        tokenId,
        priceNear: price,
        wallet,
      });
      const txHash = transactionHash(result);
      const finalResult = await waitForFinal(txHash, walletAddress, wallet);
      assertFinalSuccess(finalResult, "Listing approval");
      await requireConnectedAccount(walletAddress, wallet);
      const contractListingId = listingIdFromFinalResult(finalResult, {
        nftContractId,
        tokenId,
        sellerId: walletAddress,
      });
      const [confirmed, metadata] = await Promise.all([
        marketplaceListing(contractListingId, wallet),
        marketplaceListingMetadata(contractListingId, wallet),
      ]);
      if (!confirmed?.active || metadata?.status !== "active" ||
          confirmed.token_id !== tokenId ||
          confirmed.nft_contract_id !== nftContractId ||
          confirmed.seller.toLowerCase() !== walletAddress.toLowerCase()) {
        throw new Error("Marketplace listing state could not be confirmed.");
      }

      setShowListingModal(false);
      setListingPrice("");

      setListing({
        id: contractListingId,
        contract_sale_id: contractListingId,
        contract_listing_id: contractListingId,
        token_id: tokenId,
        nft_id: nft.id,
        seller_id: user?.id || walletAddress,
        seller: walletAddress,
        price,
        currency: "NEAR",
        status: "active",
        network_id: NETWORK_ID,
        marketplace_contract_id: MARKETPLACE_CONTRACT_ID,
      });

      const indexed = await waitForIndexedListing({ transactionHash: txHash, status: "active" });

      setSuccess(indexed
        ? `"${nft.name}" is listed on NEAR and verified in the marketplace catalog.`
        : `"${nft.name}" is listed on NEAR. Catalog verification is still pending; the item will appear after the trusted indexer catches up.`);
      await fetchNFTDetails();
    } catch (err) {
      console.error("NFT listing error:", err);
      setError(err?.message || "Unable to list this NFT.");
    } finally {
      setListingLoading(false);
    }
  };

  const handleCancelListing = async () => {
    if (!LISTING_ACTIONS_ENABLED) {
      setSuccess("");
      setError(LISTING_UNAVAILABLE_MESSAGE);
      return;
    }

    if (!listing || cancelling) return;

    const confirmed = window.confirm(`Remove "${nft?.name || "this NFT"}" from the marketplace?`);
    if (!confirmed) return;

    try {
      setCancelling(true);
      setError("");
      setSuccess("");

      const { data: { user: authUser } } = await supabase.auth.getUser().catch(() => ({ data: { user: null } }));
      if (authUser && listing.seller_id && authUser.id !== listing.seller_id && listing.seller?.toLowerCase() !== walletAddress?.toLowerCase()) {
        throw new Error("You are not the owner of this listing.");
      }
      if (!isConnected || !walletAddress || !selector) {
        throw new Error("Connect the seller NEAR wallet before cancelling.");
      }
      const saleId = listing.contract_sale_id || listing.contract_listing_id || listing.id;
      if (!saleId) {
        throw new Error("Listing is missing its verified on-chain listing ID.");
      }

      const wallet = { selector, accountId: walletAddress };
      await requireConnectedAccount(walletAddress, wallet);
      const before = await marketplaceListing(saleId, wallet);
      if (!before?.active || before.seller.toLowerCase() !== walletAddress.toLowerCase()) {
        throw new Error("The on-chain listing is no longer active for this seller.");
      }
      const result = await cancelListingOnChain(saleId, wallet);
      const txHash = transactionHash(result);
      const finalResult = await waitForFinal(txHash, walletAddress, wallet);
      assertFinalSuccess(finalResult, "Listing cancellation");
      await requireConnectedAccount(walletAddress, wallet);
      const [after, metadata] = await Promise.all([
        marketplaceListing(saleId, wallet),
        marketplaceListingMetadata(saleId, wallet),
      ]);
      if (after?.active || after?.seller?.toLowerCase() !== walletAddress.toLowerCase() || metadata?.status !== "cancelled") {
        throw new Error("Final NEAR state did not confirm the listing cancellation.");
      }

      setListing(null);

      const indexed = await waitForIndexedListing({
        contractListingId: saleId,
        status: "cancelled",
      });

      setSuccess(indexed
        ? `"${nft?.name || "NFT"}" was cancelled on NEAR and removed from the verified catalog.`
        : `"${nft?.name || "NFT"}" was cancelled on NEAR. Trusted indexer verification is still pending.`);
      await fetchNFTDetails();
    } catch (err) {
      console.error("Cancel listing error:", err);
      setError(err?.message || "Unable to remove this listing.");
    } finally {
      setCancelling(false);
    }
  };

  const mintCatalogNFT = async () => {
    if (!MINTING_ACTIONS_ENABLED) {
      setSuccess("");
      setError(MINTING_UNAVAILABLE_MESSAGE);
      return;
    }
    if (!isConnected || !selector || !walletAddress) {
      setError("Connect the configured NEAR Testnet mint-authority wallet first.");
      return;
    }
    if (mintAuthority && walletAddress.trim().toLowerCase() !== mintAuthority.trim().toLowerCase()) {
      setError(`Connected wallet (${walletAddress}) is not the configured Testnet mint authority (${mintAuthority}). Only ${mintAuthority} can mint new NFTs on this contract.`);
      return;
    }
    if (!nft?.id || nft.token_id || nft.nft_contract_id || minting) return;

    const tokenId = `catalog-${nft.id}`;
    const wallet = { selector, accountId: walletAddress };
    setMinting(true);
    setError("");
    setSuccess("");
    try {
      await requireConnectedAccount(walletAddress, wallet);
      const result = await mintNftOnChain({
        tokenId,
        ownerId: walletAddress,
        wallet,
        metadata: {
          title: nft.name,
          description: nft.description || null,
          media: nft.image_url,
          media_hash: null,
          copies: "1",
          issued_at: null,
          expires_at: null,
          starts_at: null,
          updated_at: null,
          extra: JSON.stringify({ catalog_id: nft.id, category: nft.category }),
          reference: null,
          reference_hash: null,
        },
      });
      const txHash = transactionHash(result);
      const finalResult = await waitForFinal(txHash, walletAddress, wallet);
      assertFinalSuccess(finalResult, "NFT mint");
      await requireConnectedAccount(walletAddress, wallet);
      const token = await nftToken(tokenId, wallet);
      if (!token || token.owner_id !== walletAddress || token.token_id !== tokenId) {
        throw new Error("The finalized mint could not be verified against NFT contract state.");
      }
      setSuccess(`“${nft.name}” was minted on NEAR. The trusted indexer is binding it to your catalog record.`);
      setTimeout(() => { void fetchNFTDetails(); }, 3000);
    } catch (err) {
      setError(err?.message || "Unable to mint this NFT on NEAR Testnet.");
    } finally {
      setMinting(false);
    }
  };

  const handleLike = async () => {
    setError("");
    setSuccess("");
    const result = await toggleLike(id);
    if (result) {
      setLikes((previous) => Math.max(0, previous + result.change));
    }
  };

  const handleShare = async () => {
    try {
      await navigator.clipboard.writeText(window.location.href);
      setSuccess("Link copied to clipboard!");
      setTimeout(() => setSuccess(""), 2000);
    } catch (err) {
      console.error(err);
    }
  };

  if (loading) {
    return (
      <div className="min-h-screen bg-black flex items-center justify-center">
        <Loader2 className="w-8 h-8 animate-spin text-[#00EC97]" />
      </div>
    );
  }

  if (!nft) {
    return (
      <div className="min-h-screen bg-black flex items-center justify-center text-white">
        <p>NFT not found</p>
      </div>
    );
  }

  const creatorName = nft.profiles?.display_name || nft.profiles?.username || "Unknown";

  return (
    <div className="min-h-screen bg-black text-white px-4 py-6 sm:px-6 lg:px-8">
      <button
        onClick={() => navigate(-1)}
        className="mb-6 flex items-center gap-2 text-gray-400 hover:text-white transition-colors"
      >
        <ArrowLeft className="w-4 h-4" />
        Back
      </button>

      <div className="max-w-6xl mx-auto grid grid-cols-1 lg:grid-cols-2 gap-8">
        <div className="space-y-4">
          <div className="aspect-square rounded-2xl overflow-hidden border border-zinc-800 bg-zinc-900">
            <img src={nft.image_url} alt={nft.name} className="w-full h-full object-cover" />
          </div>
        </div>

        <div className="space-y-6">
          <div>
            <div className="flex items-center gap-2 mb-2">
              <span className="px-2 py-1 bg-[#00EC97]/10 text-[#00EC97] rounded-lg text-xs">
                {nft.category || "NFT"}
              </span>
              {listing && listing.status === "active" && (
                <span className="px-2 py-1 bg-green-500/10 text-green-400 rounded-lg text-xs">
                  For Sale
                </span>
              )}
            </div>
            <h1 className="text-3xl font-bold mb-2">{nft.name}</h1>
            <p className="text-gray-400">{nft.description || "No description"}</p>
          </div>

          <div className="flex items-center gap-4 p-4 rounded-xl bg-zinc-900/50 border border-zinc-800">
            <div className="w-10 h-10 rounded-full bg-gradient-to-br from-[#00EC97] to-[#00C781] flex items-center justify-center">
              <User className="w-5 h-5 text-black" />
            </div>
            <div>
              <p className="text-sm text-gray-400">Creator</p>
              <p className="font-medium">{creatorName}</p>
            </div>
          </div>

          {listing && listing.status === "active" && (
            <div className="p-6 rounded-2xl border border-zinc-800 bg-gradient-to-br from-zinc-900 to-black">
              <p className="text-sm text-gray-400 mb-2">Current Price</p>
              <div className="flex items-baseline gap-2 mb-6">
                <p className="text-3xl font-bold">{listing.price}</p>
                <p className="text-xl text-gray-400">{listing.currency || "NEAR"}</p>
              </div>

              {isOwner ? (
                <div className="space-y-3">
                  <div className="p-3 bg-purple-500/10 border border-purple-500/20 rounded-xl text-sm text-purple-300">
                    You are the seller of this active listing.
                  </div>
                  <button
                    onClick={handleCancelListing}
                    disabled={!LISTING_ACTIONS_ENABLED || cancelling}
                    aria-describedby={!LISTING_ACTIONS_ENABLED ? "listing-unavailable" : undefined}
                    className="w-full border border-red-500/40 text-red-400 font-semibold py-3 px-6 rounded-xl hover:bg-red-500 hover:text-white transition-all disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
                  >
                    {cancelling ? (
                      <>
                        <Loader2 className="w-5 h-5 animate-spin" />
                        Cancelling Listing...
                      </>
                    ) : (
                      <>
                        <X className="w-5 h-5" />
                        Cancel Listing
                      </>
                    )}
                  </button>
                  {!LISTING_ACTIONS_ENABLED && (
                    <p id="listing-unavailable" role="status" className="mt-3 text-sm text-amber-200">
                      {LISTING_UNAVAILABLE_MESSAGE}
                    </p>
                  )}
                </div>
              ) : (
                <>
                  <button
                    onClick={handleBuy}
                    disabled={!PURCHASES_ENABLED || buying || !isConnected}
                    aria-describedby={!PURCHASES_ENABLED ? "purchase-unavailable" : undefined}
                    className="w-full bg-gradient-to-r from-[#00EC97] to-[#00C781] text-black font-semibold py-3 px-6 rounded-xl hover:shadow-lg hover:shadow-[#00EC97]/20 transition-all disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
                  >
                    {PURCHASES_ENABLED && buying ? (
                      <>
                        <Loader2 className="w-5 h-5 animate-spin" />
                        Processing...
                      </>
                    ) : (
                      <>
                        <ShoppingCart className="w-5 h-5" />
                        {!PURCHASES_ENABLED
                          ? "Purchases unavailable"
                          : isConnected ? "Buy Now" : "Connect Wallet to Buy"}
                      </>
                    )}
                  </button>
                  {!PURCHASES_ENABLED && (
                    <p id="purchase-unavailable" role="status" className="mt-3 text-sm text-amber-200">
                      {PURCHASE_UNAVAILABLE_MESSAGE}
                    </p>
                  )}
                </>
              )}
            </div>
          )}

          {(!listing || listing.status !== "active") && (
            <div className="p-6 rounded-2xl border border-zinc-800 bg-gradient-to-br from-zinc-900 to-black">
              {isOwner ? (
                <div className="space-y-4">
                  <div className="flex items-center justify-between">
                    <div>
                      <p className="text-sm text-gray-400">Ownership Status</p>
                      <p className="text-base font-semibold text-white mt-1">Owned by You</p>
                    </div>
                    <span className="px-2.5 py-1 bg-zinc-800 text-gray-300 rounded-lg text-xs font-medium">
                      Not Listed
                    </span>
                  </div>

                  {!nft.token_id ? (
                    <div className="space-y-3 pt-2">
                      <p className="text-sm text-gray-400">
                        This NFT has been created off-chain. Mint it to NEAR Testnet to enable marketplace listing.
                      </p>
                      <button
                        onClick={mintCatalogNFT}
                        disabled={!MINTING_ACTIONS_ENABLED || minting}
                        aria-describedby={!MINTING_ACTIONS_ENABLED ? "minting-unavailable" : undefined}
                        className="w-full bg-[#00EC97] text-black font-semibold py-3 px-6 rounded-xl hover:bg-[#00C781] transition-all disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
                      >
                        {minting ? (
                          <>
                            <Loader2 className="w-5 h-5 animate-spin" />
                            Minting on NEAR...
                          </>
                        ) : (
                          <>
                            <Upload className="w-5 h-5" />
                            Mint on NEAR
                          </>
                        )}
                      </button>
                      {!MINTING_ACTIONS_ENABLED && (
                        <p id="minting-unavailable" role="status" className="text-sm text-amber-200">
                          {MINTING_UNAVAILABLE_MESSAGE}
                        </p>
                      )}
                    </div>
                  ) : (
                    <div className="space-y-3 pt-2">
                      <div className="rounded-xl border border-zinc-800 bg-zinc-900/50 p-3 text-xs text-gray-400 space-y-1">
                        <div className="flex justify-between">
                          <span>Token ID:</span>
                          <span className="font-mono text-gray-300">{nft.token_id}</span>
                        </div>
                        <div className="flex justify-between">
                          <span>NFT Contract:</span>
                          <span className="font-mono text-gray-300 truncate max-w-[200px]">{nft.nft_contract_id}</span>
                        </div>
                      </div>

                      <button
                        onClick={openListingForm}
                        disabled={!LISTING_ACTIONS_ENABLED || listingLoading}
                        aria-describedby={!LISTING_ACTIONS_ENABLED ? "listing-unavailable" : undefined}
                        className="w-full bg-purple-600 text-white font-semibold py-3 px-6 rounded-xl hover:bg-purple-700 transition-all disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
                      >
                        <Tag className="w-5 h-5" />
                        List NFT
                      </button>
                      {!LISTING_ACTIONS_ENABLED && (
                        <p id="listing-unavailable" role="status" className="text-sm text-amber-200">
                          {LISTING_UNAVAILABLE_MESSAGE}
                        </p>
                      )}
                    </div>
                  )}
                </div>
              ) : (
                <div>
                  <p className="text-sm text-gray-400 mb-1">Status</p>
                  <p className="text-lg font-semibold text-gray-300">Not listed for sale</p>
                  <p className="text-xs text-gray-500 mt-2">
                    This item is not currently available for purchase on the marketplace.
                  </p>
                </div>
              )}
            </div>
          )}

          <div className="flex items-center gap-4">
            <button
              onClick={handleLike}
              disabled={loadingLikes || !!likingNFT}
              aria-label={liked ? "Unlike NFT" : "Like NFT"}
              aria-pressed={liked}
              className={`flex items-center gap-2 px-4 py-2 rounded-lg border transition-colors ${
                liked
                  ? "bg-red-500/10 border-red-500/20 text-red-400"
                  : "border-zinc-700 hover:bg-zinc-800"
              }`}
            >
              <Heart className={`w-4 h-4 ${liked ? "fill-current" : ""}`} />
              <span>{likes}</span>
            </button>
            <button
              onClick={handleShare}
              className="flex items-center gap-2 px-4 py-2 rounded-lg border border-zinc-700 hover:bg-zinc-800"
            >
              <Share2 className="w-4 h-4" />
              Share
            </button>
          </div>

          {(error || likeError) && (
            <div role="alert" className="p-3 bg-red-500/10 border border-red-500/20 rounded-lg text-red-400 text-sm">
              {error || likeError}
            </div>
          )}
          {success && (
            <div className="p-3 bg-[#00EC97]/10 border border-[#00EC97]/20 rounded-lg text-[#00EC97] text-sm">
              {success}
            </div>
          )}
        </div>
      </div>

      {/* LISTING MODAL */}
      {LISTING_ACTIONS_ENABLED && showListingModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4">
          <div className="relative w-full max-w-md rounded-2xl border border-zinc-800 bg-zinc-950 p-6 shadow-2xl">
            <div className="mb-6 flex items-center justify-between">
              <div>
                <h3 className="text-xl font-bold text-white">List NFT for Sale</h3>
                <p className="mt-1 text-sm text-gray-400">{nft.name}</p>
              </div>
              <button
                type="button"
                onClick={() => { if (!listingLoading) setShowListingModal(false); }}
                disabled={listingLoading}
                className="flex h-9 w-9 items-center justify-center rounded-full bg-white/5 text-gray-400 transition hover:bg-white/10 hover:text-white disabled:opacity-50"
              >
                <X className="w-5 h-5" />
              </button>
            </div>

            <form onSubmit={handleListNFT} className="space-y-5">
              <div>
                <label className="mb-2 block text-sm font-medium text-gray-200">
                  Sale Price (NEAR)
                </label>
                <input
                  type="text"
                  inputMode="decimal"
                  value={listingPrice}
                  onChange={(e) => setListingPrice(e.target.value)}
                  disabled={listingLoading}
                  placeholder="e.g. 1.5"
                  className="w-full rounded-xl border border-white/10 bg-white/[0.04] px-4 py-3 text-sm text-white outline-none transition placeholder:text-gray-600 focus:border-purple-500 disabled:opacity-60"
                />
              </div>

              <div className="rounded-xl border border-white/10 bg-white/[0.02] p-3 text-xs text-gray-400 space-y-1">
                <p className="font-medium text-gray-300">Transaction & Deposit Requirements:</p>
                <p>• Approval deposit: ~0.01 NEAR attached for NFT contract approval.</p>
                <p>• Storage deposit: up to 0.12 NEAR for contract storage if not already registered.</p>
              </div>

              <div className="flex gap-3 pt-2">
                <button
                  type="button"
                  onClick={() => setShowListingModal(false)}
                  disabled={listingLoading}
                  className="flex-1 rounded-xl border border-white/10 bg-white/[0.04] px-4 py-3 text-sm font-medium text-gray-300 transition hover:bg-white/[0.08] hover:text-white disabled:opacity-50"
                >
                  Cancel
                </button>

                <button
                  type="submit"
                  disabled={!LISTING_ACTIONS_ENABLED || listingLoading}
                  className="flex flex-1 items-center justify-center gap-2 rounded-xl bg-purple-600 px-4 py-3 text-sm font-semibold text-white transition hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-60"
                >
                  {listingLoading ? (
                    <>
                      <Loader2 className="w-4 h-4 animate-spin" />
                      Listing...
                    </>
                  ) : (
                    <>
                      <Tag className="w-4 h-4" />
                      List NFT
                    </>
                  )}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
}

export default NFTDetails;
