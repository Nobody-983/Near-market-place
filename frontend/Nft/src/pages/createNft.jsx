
import { useCallback, useEffect, useRef, useState } from "react";
import {
  ImagePlus,
  Upload,
  X,
  Loader2,
  Plus,
  Trash2,
  ArrowUpRight,
  Tag,
  ShoppingBag,
  CheckCircle2,
} from "lucide-react";

import { useWallet } from "../context/walletContext";
import { supabase } from "../lib/supabase";
import { validateNearAmount, yoctoToNear } from "../lib/near/utils";
import { EDITIONS_CONTRACT_ID, LEGACY_NFT_CONTRACT_ID, NFT_CONTRACT_ID } from "../config/near";
import {
  LISTING_ACTIONS_ENABLED,
  LISTING_UNAVAILABLE_MESSAGE,
  MINTING_ACTIONS_ENABLED,
  MINTING_UNAVAILABLE_MESSAGE,
} from "../config/marketplace";

import {
  uploadNFTImage,
  createNFT,
  deleteNFT,
  getNFTDeletionError,
} from "../services/nftService";

import {
  getUserListings,
  waitForIndexedListing,
} from "../services/marketService";
import {
  assertFinalSuccess,
  cancelListingOnChain,
  cancelEditionListingOnChain,
  ensureMarketplaceListingStorage,
  generateUniqueTokenId,
  listingIdFromFinalResult,
  listNftOnChain,
  listEditionOnChain,
  marketplaceListing,
  marketplaceListingMetadata,
  marketplaceListings,
  mintNftOnChain,
  mintEditionOnChain,
  mtToken,
  nftToken,
  nftTokensForOwner,
  requireConnectedAccount,
  transactionHash,
  verifyListingPreconditions,
  waitForFinal,
} from "../lib/near/contract.js";

// =========================================================
// CONSTANTS
// =========================================================

const MAX_IMAGE_SIZE = 5 * 1024 * 1024;

const ALLOWED_IMAGE_TYPES = [
  "image/png",
  "image/jpeg",
  "image/webp",
];

const ALLOWED_IMAGE_EXTENSIONS = [
  "png",
  "jpg",
  "jpeg",
  "webp",
];

const CATEGORIES = [
  "Art",
  "Music",
  "Collectible",
  "Gaming",
];

// =========================================================
// COMPONENT
// =========================================================

function CreateNFT() {
  const { user, selector, walletAddress, isConnected } = useWallet();
  const userId = user?.id;
  const fileInputRef = useRef(null);

  const [form, setForm] = useState({
    name: "",
    description: "",
    category: "Art",
    price: "",
    currency: "NEAR",
    model: "unique",
    editionSupply: "100",
  });

  const [image, setImage] = useState(null);
  const [preview, setPreview] = useState("");

  // NFTs currently owned by the user
  const [nfts, setNfts] = useState([]);

  const [listings, setListings] = useState([]);

  const [loading, setLoading] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [loadingNFTs, setLoadingNFTs] = useState(true);

  const [deleting, setDeleting] = useState(null);
  const [minting, setMinting] = useState(null);
  const [listing, setListing] = useState(null);
  const [listingStep, setListingStep] = useState("");
  const [cancelling, setCancelling] = useState(null);

  const [listingNFT, setListingNFT] = useState(null);

  const [listingForm, setListingForm] = useState({
    price: "",
    currency: "NEAR",
    quantity: "1",
  });

  const [mintSuccessNFT, setMintSuccessNFT] = useState(null);

  const [error, setError] = useState("");
  const [success, setSuccess] = useState("");

  const mintCatalogNFT = async (nft) => {
    if (!MINTING_ACTIONS_ENABLED) {
      setSuccess("");
      setError(MINTING_UNAVAILABLE_MESSAGE);
      return;
    }
    if (!isConnected || !selector || !walletAddress) {
      setError("Connect a NEAR Testnet wallet first.");
      return;
    }
    if (!nft?.id || nft.token_id || nft.nft_contract_id || minting) return;

    const wallet = { selector, accountId: walletAddress };
    const isEdition = nft.model === "edition" || (nft.edition_supply && Number(nft.edition_supply) > 1);
    const supply = isEdition ? Number(nft.edition_supply || 100) : 1;
    let tokenId = nft.token_id || `catalog-${nft.id}`;

    setMinting(nft.id);
    setError("");
    setSuccess("");
    try {
      await requireConnectedAccount(walletAddress, wallet);

      const candidateTokenId = tokenId;

      // On permissionless contracts, ensure token_id is verified unused on-chain
      // to permanently prevent "token_id must be unique and never reused" panics.
      if (!isEdition && typeof generateUniqueTokenId === "function") {
        try {
          tokenId = await generateUniqueTokenId(candidateTokenId, wallet, NFT_CONTRACT_ID);
        } catch {
          tokenId = candidateTokenId;
        }
      }
      if (isEdition) {
        const result = await mintEditionOnChain({
          tokenId,
          supply,
          wallet,
          metadata: {
            title: nft.name,
            description: nft.description || null,
            media: nft.image_url,
            media_hash: null,
            copies: String(supply),
            issued_at: null,
            expires_at: null,
            starts_at: null,
            updated_at: null,
            extra: JSON.stringify({ catalog_id: nft.id, category: nft.category, model: "edition", supply }),
            reference: null,
            reference_hash: null,
          },
        });
        const txHash = transactionHash(result);
        const finalResult = await waitForFinal(result || txHash, walletAddress, wallet);
        assertFinalSuccess(finalResult, "Edition mint");
        await requireConnectedAccount(walletAddress, wallet);
        const token = await mtToken(tokenId, wallet);
        if (!token) {
          throw new Error("The finalized edition mint could not be verified against contract state.");
        }
        const contractId = typeof EDITIONS_CONTRACT_ID !== "undefined" ? EDITIONS_CONTRACT_ID : "editions.roundfrog-dev.testnet";
        const verifiedNft = {
          ...nft,
          token_id: tokenId,
          nft_contract_id: contractId,
          owner_id: walletAddress,
          onChainOwner: walletAddress,
          model: "edition",
          edition_supply: supply,
        };
        setNfts((currentNFTs) =>
          currentNFTs.map((item) => (item.id === nft.id ? verifiedNft : item))
        );
        if (typeof setMintSuccessNFT === "function") {
          setMintSuccessNFT({
            nft: verifiedNft,
            tokenId,
            contractId,
            ownerId: walletAddress,
            txHash,
            name: nft.name,
            imageUrl: nft.image_url,
            description: nft.description,
            price: nft.price,
            model: "edition",
            supply,
            listed: false,
            listingPrice: null,
            contractListingId: null,
          });
        }
        setSuccess(`Limited Edition “${nft.name}” (${supply} copies) was minted on NEAR! You can now list it for sale.`);
      } else {
        let mediaHash = null;
        if (nft.image_url && typeof globalThis.crypto?.subtle?.digest === "function") {
          try {
            const buf = await globalThis.crypto.subtle.digest("SHA-256", new TextEncoder().encode(nft.image_url));
            mediaHash = btoa(String.fromCharCode(...new Uint8Array(buf)));
          } catch {
            mediaHash = null;
          }
        }
        const validMedia = mediaHash ? nft.image_url : null;
        const validMediaHash = mediaHash || null;
        const result = await mintNftOnChain({
          tokenId,
          ownerId: walletAddress,
          wallet,
          metadata: {
            title: nft.name,
            description: nft.description || null,
            media: validMedia,
            media_hash: validMediaHash,
            copies: 1,
            issued_at: null,
            expires_at: null,
            starts_at: null,
            updated_at: null,
            extra: JSON.stringify({
              catalog_id: nft.id,
              category: nft.category,
              model: "unique",
              media_url: nft.image_url,
            }),
            reference: null,
            reference_hash: null,
          },
        });
        const txHash = transactionHash(result);
        const finalResult = await waitForFinal(result || txHash, walletAddress, wallet);
        assertFinalSuccess(finalResult, "NFT mint");
        await requireConnectedAccount(walletAddress, wallet);
        const token = await nftToken(tokenId, wallet);
        if (!token || token.token_id !== tokenId) {
          throw new Error("The finalized mint could not be verified against NFT contract state.");
        }
        if (token.owner_id.toLowerCase() !== walletAddress.toLowerCase()) {
          throw new Error(
            `The NFT was minted on-chain but belongs to ${token.owner_id}, not your connected wallet (${walletAddress}). You must transfer it to ${walletAddress} before listing.`
          );
        }
        const verifiedNft = {
          ...nft,
          token_id: tokenId,
          nft_contract_id: NFT_CONTRACT_ID,
          owner_id: token.owner_id,
          onChainOwner: token.owner_id,
          model: "unique",
        };
        setNfts((currentNFTs) =>
          currentNFTs.map((item) => (item.id === nft.id ? verifiedNft : item))
        );
        if (typeof setMintSuccessNFT === "function") {
          setMintSuccessNFT({
            nft: verifiedNft,
            tokenId: token.token_id,
            contractId: NFT_CONTRACT_ID,
            ownerId: token.owner_id,
            txHash,
            name: nft.name,
            imageUrl: nft.image_url,
            description: nft.description,
            price: nft.price,
            model: "unique",
            supply: 1,
            listed: false,
            listingPrice: null,
            contractListingId: null,
          });
        }
        setSuccess(`“${nft.name}” was minted on NEAR! You can now list it for sale on the marketplace.`);
      }
      if (typeof window !== "undefined" && typeof window.setTimeout === "function") {
        window.setTimeout(() => {
          if (typeof fetchNFTs === "function") void fetchNFTs();
        }, 3000);
      }
    } catch (err) {
      const errMsg = err?.message || "";
      if (/already exists|duplicate token|unique and never reused/i.test(errMsg)) {
        try {
          const recovered = isEdition
            ? await mtToken(tokenId, wallet)
            : await nftToken(tokenId, wallet);
          if (recovered && (recovered.token_id === tokenId || recovered.id === tokenId)) {
            const onChainOwner = recovered.owner_id || walletAddress;
            const contractId = isEdition
              ? (typeof EDITIONS_CONTRACT_ID !== "undefined" ? EDITIONS_CONTRACT_ID : "editions.roundfrog-dev.testnet")
              : NFT_CONTRACT_ID;
            const verifiedNft = {
              ...nft,
              token_id: tokenId,
              nft_contract_id: contractId,
              owner_id: onChainOwner,
              onChainOwner,
              model: isEdition ? "edition" : "unique",
              ...(isEdition ? { edition_supply: supply } : {}),
            };
            setNfts((currentNFTs) =>
              currentNFTs.map((item) => (item.id === nft.id ? verifiedNft : item))
            );
            if (typeof setMintSuccessNFT === "function") {
              setMintSuccessNFT({
                nft: verifiedNft,
                tokenId,
                contractId,
                ownerId: onChainOwner,
                txHash: null,
                name: nft.name,
                imageUrl: nft.image_url,
                description: nft.description,
                price: nft.price,
                model: verifiedNft.model,
                supply: isEdition ? supply : 1,
                listed: false,
                listingPrice: null,
                contractListingId: null,
              });
            }
            setSuccess(`“${nft.name}” was successfully confirmed on NEAR! You can now list it for sale.`);
            return;
          }
        } catch {
          // Fall through to standard error report
        }
      }
      setError(err?.message || "Unable to mint this NFT on NEAR Testnet.");
    } finally {
      setMinting(null);
    }
  };

  // =========================================================
  // FETCH OWNED NFTS
  // =========================================================

  const fetchNFTs = useCallback(async () => {
    const currentWallet = typeof walletAddress !== "undefined" ? walletAddress : null;
    const currentSelector = typeof selector !== "undefined" ? selector : null;

    if (!userId && !currentWallet) {
      setNfts([]);
      setLoadingNFTs(false);
      return;
    }

    setLoadingNFTs(true);

    try {
      let ownedNFTs = [];
      if (userId) {
        const { data, error: fetchError } = await supabase
          .from("nft_ownership")
          .select(`
            nft_id,
            owner_id,
            acquired_at,
            updated_at,
            nfts (
              id,
              creator_id,
              name,
              description,
              image_url,
              category,
              source,
              token_id,
              nft_contract_id,
              price::text,
              currency,
              created_at,
              updated_at,
              listing_history:marketplace_listings (id)
            )
          `)
          .eq("owner_id", userId)
          .order("acquired_at", {
            ascending: false,
          });

        if (fetchError) {
          throw fetchError;
        }

        // Supabase returns the NFT inside the `nfts` relation.
        // Flatten it so the rest of the component can continue
        // using nft.id, nft.name, nft.image_url, etc.
        ownedNFTs = (data || [])
          .filter((ownership) => ownership.nfts)
          .map(({ nfts: nft, owner_id }) => ({
            ...nft, owner_id, price: validateNearAmount(nft.price),
          }));
      }

      if (currentWallet) {
        try {
          const wallet = currentSelector ? { selector: currentSelector, accountId: currentWallet } : undefined;
          const chainTokens = await nftTokensForOwner(currentWallet, wallet);
          if (Array.isArray(chainTokens)) {
            for (const token of chainTokens) {
              const existingIdx = ownedNFTs.findIndex(
                (n) => n.token_id === token.token_id || n.id === token.token_id
              );
              if (existingIdx >= 0) {
                ownedNFTs[existingIdx] = {
                  ...ownedNFTs[existingIdx],
                  token_id: token.token_id,
                  nft_contract_id: NFT_CONTRACT_ID,
                  onChainOwner: token.owner_id,
                };
              } else {
                let extra = {};
                try {
                  extra = token.metadata?.extra ? JSON.parse(token.metadata.extra) : {};
                } catch (e) {
                  void e;
                }
                ownedNFTs.push({
                  id: token.token_id,
                  token_id: token.token_id,
                  nft_contract_id: NFT_CONTRACT_ID,
                  name: token.metadata?.title || token.token_id,
                  description: token.metadata?.description || "",
                  image_url: token.metadata?.media || "/placeholder.png",
                  category: extra.category || "Art",
                  source: "near_chain",
                  price: null,
                  currency: "NEAR",
                  owner_id: userId || currentWallet,
                  onChainOwner: token.owner_id,
                  listing_history: [],
                });
              }
            }
          }
        } catch (chainErr) {
          console.warn("Could not query on-chain tokens for owner:", chainErr);
        }
      }

      setNfts(ownedNFTs);
    } catch (err) {
      console.error("Error fetching owned NFTs:", err);
      setError("Unable to load your NFTs.");
    } finally {
      setLoadingNFTs(false);
    }
  }, [userId]);

  // =========================================================
  // FETCH USER LISTINGS
  // =========================================================

  const fetchListings = useCallback(async () => {
    const currentWallet = typeof walletAddress !== "undefined" ? walletAddress : null;
    const currentSelector = typeof selector !== "undefined" ? selector : null;

    let combinedListings = [];
    if (userId) {
      try {
        const data = await getUserListings(userId);
        if (Array.isArray(data)) {
          combinedListings.push(...data);
        }
      } catch (err) {
        console.error("Error fetching listings:", err);
      }
    }

    if (currentWallet) {
      try {
        const wallet = currentSelector ? { selector: currentSelector, accountId: currentWallet } : undefined;
        const chainListings = await marketplaceListings(wallet);
        if (Array.isArray(chainListings)) {
          const userChainListings = chainListings.filter(
            (l) => l.active && l.seller?.toLowerCase() === currentWallet.toLowerCase()
          );
          for (const cl of userChainListings) {
            const exists = combinedListings.some(
              (l) =>
                (l.contract_sale_id && l.contract_sale_id === cl.listing_id) ||
                (l.contract_listing_id && l.contract_listing_id === cl.listing_id) ||
                (l.token_id === cl.token_id && l.status === "active")
            );
            if (!exists) {
              combinedListings.push({
                id: cl.listing_id,
                contract_sale_id: cl.listing_id,
                contract_listing_id: cl.listing_id,
                token_id: cl.token_id,
                nft_id: cl.token_id,
                seller_id: userId || cl.seller,
                seller: cl.seller,
                price: yoctoToNear(cl.price),
                status: "active",
                created_at: new Date(Number(cl.created_at) / 1000000).toISOString(),
              });
            }
          }
        }
      } catch (chainErr) {
        console.warn("Could not query on-chain listings:", chainErr);
      }
    }

    setListings(combinedListings);
  }, [userId]);

  // =========================================================
  // INITIAL LOAD
  // =========================================================

  useEffect(() => {
    void Promise.resolve().then(() => Promise.all([fetchNFTs(), fetchListings()]));
  }, [fetchListings, fetchNFTs, walletAddress, selector]);

  // =========================================================
  // FIND ACTIVE LISTING
  // =========================================================

  const getNFTListing = (nftId) => {
    const nftObj = nfts.find((item) => item.id === nftId);
    const tokenId = nftObj?.token_id;
    return listings.find(
      (item) =>
        item.status === "active" &&
        (item.nft_id === nftId || (tokenId && (item.token_id === tokenId || item.nft_id === tokenId)))
    );
  };

  // =========================================================
  // INPUT HANDLING
  // =========================================================

  const handleChange = (e) => {
    const { name, value } = e.target;

    setForm((current) => ({
      ...current,
      [name]: value,
    }));
  };

  // =========================================================
  // VALIDATE IMAGE
  // =========================================================

  const validateImage = (file) => {
    if (!file) {
      return "Please select an image.";
    }

    if (!file.type) {
      return "Unable to determine the image type.";
    }

    if (!ALLOWED_IMAGE_TYPES.includes(file.type)) {
      return "Only PNG, JPG, JPEG or WEBP images are allowed.";
    }

    if (file.size > MAX_IMAGE_SIZE) {
      return "Image must be less than 5MB.";
    }

    const extension = file.name
      ?.split(".")
      .pop()
      ?.toLowerCase();

    if (
      !extension ||
      !ALLOWED_IMAGE_EXTENSIONS.includes(extension)
    ) {
      return "Invalid image file extension.";
    }

    return null;
  };

  // =========================================================
  // IMAGE SELECTION
  // =========================================================

  const handleImageChange = (e) => {
    const file = e.target.files?.[0];

    if (!file) {
      return;
    }

    setError("");
    setSuccess("");

    const validationError = validateImage(file);

    if (validationError) {
      setImage(null);
      setPreview("");

      if (fileInputRef.current) {
        fileInputRef.current.value = "";
      }

      setError(validationError);
      return;
    }

    if (preview) {
      URL.revokeObjectURL(preview);
    }

    const newPreview = URL.createObjectURL(file);

    setImage(file);
    setPreview(newPreview);
  };

  // =========================================================
  // REMOVE IMAGE
  // =========================================================

  const removeImage = () => {
    if (preview) {
      URL.revokeObjectURL(preview);
    }

    setPreview("");
    setImage(null);

    if (fileInputRef.current) {
      fileInputRef.current.value = "";
    }
  };

  // =========================================================
  // CREATE NFT
  // =========================================================

  const handleSubmit = async (e) => {
    e.preventDefault();

    if (loading) {
      return;
    }

    setError("");
    setSuccess("");

    if (!user?.id) {
      setError("You must be logged in to create an NFT.");
      return;
    }

    const trimmedName = form.name.trim();

    if (!trimmedName) {
      setError("NFT name is required.");
      return;
    }

    if (trimmedName.length > 100) {
      setError("NFT name must be 100 characters or less.");
      return;
    }

    const trimmedDescription = form.description.trim();

    if (trimmedDescription.length > 1000) {
      setError("Description must be 1000 characters or less.");
      return;
    }

    if (!image) {
      setError("Please select an image.");
      return;
    }

    const imageValidationError = validateImage(image);

    if (imageValidationError) {
      setError(imageValidationError);
      return;
    }

    let price;
    try {
      price = validateNearAmount(form.price, { allowZero: false });
    } catch (err) {
      setError(err.message);
      return;
    }

    let uploadedImage = null;

    try {
      setLoading(true);
      setUploading(true);

      uploadedImage = await uploadNFTImage(
        image,
        user.id
      );

      if (!uploadedImage?.publicUrl) {
        throw new Error(
          "Image upload completed but no image URL was returned."
        );
      }

      setUploading(false);

      const newNFT = await createNFT({
        creator_id: user.id,
        name: trimmedName,
        description: trimmedDescription,
        image_url: uploadedImage.publicUrl,
        category: form.category,
        price,
        currency: "NEAR",
      });

      if (!newNFT) {
        throw new Error("NFT could not be created.");
      }

      setNfts((currentNFTs) => [
        // The creation trigger establishes ownership; new entries have no listings.
        { ...newNFT, owner_id: user.id, listing_history: [] },
        ...currentNFTs,
      ]);

      setForm({
        name: "",
        description: "",
        category: "Art",
        price: "",
        currency: "NEAR",
      });

      removeImage();

      setSuccess(
        "NFT saved to your collection. It will not appear in the Marketplace until it is minted on NEAR and listed for sale."
      );
    } catch (err) {
      console.error("NFT creation error:", err);

      // Browser Storage access is append-only. A trusted cleanup process must
      // recheck references, including inserts whose response was lost in transit.
      const message = err?.message || "Something went wrong while creating your NFT.";
      setError(uploadedImage?.filePath ? `${message} The uploaded image is retained.` : message);
    } finally {
      setUploading(false);
      setLoading(false);
    }
  };

  // =========================================================
  // OPEN LISTING FORM
  // =========================================================

  const openListingForm = (nft) => {
    if (!LISTING_ACTIONS_ENABLED) {
      setSuccess("");
      setError(LISTING_UNAVAILABLE_MESSAGE);
      return;
    }

    setError("");
    setSuccess("");

    setListingNFT(nft);

    setListingForm({
      price: nft.price || "",
      currency: nft.currency || "NEAR",
    });
  };

  // =========================================================
  // CLOSE LISTING FORM
  // =========================================================

  const closeListingForm = () => {
    if (listing) {
      return;
    }

    setListingNFT(null);

    setListingForm({
      price: "",
      currency: "NEAR",
    });
  };

  // =========================================================
  // LIST NFT
  // =========================================================

  const handleListNFT = async (e) => {
    e.preventDefault();

    if (!LISTING_ACTIONS_ENABLED) {
      setSuccess("");
      setError(LISTING_UNAVAILABLE_MESSAGE);
      return;
    }

    if (!listingNFT || listing) {
      return;
    }

    setError("");
    setSuccess("");

    if (!user?.id) {
      setError("You must be logged in to list an NFT.");
      return;
    }

    let price;
    try {
      price = validateNearAmount(listingForm.price, { allowZero: false });
    } catch (err) {
      setError(err.message);
      return;
    }

    try {
      setListing(listingNFT.id);

      if (!isConnected || !walletAddress || !selector) {
        throw new Error("Connect the NEAR wallet that owns this NFT before listing.");
      }
      const nftContractId = listingNFT.nft_contract_id || NFT_CONTRACT_ID;
      const tokenId = listingNFT.token_id || listingNFT.id;
      if (!tokenId) {
        throw new Error("This catalog NFT has not been bound to an on-chain NEAR token.");
      }
      const wallet = { selector, accountId: walletAddress };
      await requireConnectedAccount(walletAddress, wallet);
      const isEdition = listingNFT.model === "edition" || (typeof EDITIONS_CONTRACT_ID !== "undefined" && listingNFT.nft_contract_id === EDITIONS_CONTRACT_ID);
      if (isEdition) {
        const qty = Number(listingForm.quantity || 1);
        if (typeof setListingStep === "function") setListingStep("Submitting transaction to wallet...");
        const result = await listEditionOnChain({
          tokenId,
          quantity: qty,
          pricePerCopyNear: price,
          wallet,
        });
        const txHash = transactionHash(result);
        if (typeof setListingStep === "function") setListingStep("Transaction submitted. Processing on NEAR...");
        const onProgress = typeof setListingStep === "function" ? setListingStep : undefined;
        const finalResult = await waitForFinal(result || txHash, walletAddress, wallet, 60000, onProgress);
        assertFinalSuccess(finalResult, "Edition listing");
        await requireConnectedAccount(walletAddress, wallet);
        if (typeof setListingStep === "function") setListingStep("Verifying on-chain listing state...");
        const contractListingId = `edition|${tokenId}|${walletAddress}`;
        setListings((prev) => [
          {
            id: contractListingId,
            contract_sale_id: contractListingId,
            contract_listing_id: contractListingId,
            token_id: tokenId,
            nft_id: listingNFT.id,
            seller_id: user?.id || walletAddress,
            seller: walletAddress,
            price,
            status: "active",
            is_edition: true,
            available_quantity: qty,
            created_at: new Date().toISOString(),
          },
          ...prev,
        ]);
        setSuccess(`"${listingNFT.name}" (${qty} copies) is listed for sale on NEAR at ${price} NEAR each.`);
        if (typeof setMintSuccessNFT === "function") {
          setMintSuccessNFT((prev) => (prev && (prev.tokenId === tokenId || prev.nft?.id === listingNFT.id) ? {
            ...prev,
            listed: true,
            listingPrice: price,
            contractListingId,
          } : prev));
        }
        setListingNFT(null);
        setListingForm({ price: "", currency: "NEAR", quantity: "1" });
        if (typeof setListingStep === "function") setListingStep("");
        await fetchListings();
        if (typeof fetchNFTs === "function") await fetchNFTs();
        return;
      }

      await verifyListingPreconditions({
        nftContractId,
        tokenId,
        sellerId: walletAddress,
        wallet,
      });
      await ensureMarketplaceListingStorage({ sellerId: walletAddress, wallet });
      await requireConnectedAccount(walletAddress, wallet);
      if (typeof setListingStep === "function") setListingStep("Submitting transaction to wallet...");
      const result = await listNftOnChain({
        nftContractId,
        tokenId,
        priceNear: price,
        wallet,
      });
      const txHash = transactionHash(result);
      if (typeof setListingStep === "function") setListingStep("Transaction submitted. Processing on NEAR...");
      const onProgress = typeof setListingStep === "function" ? setListingStep : undefined;
      const finalResult = await waitForFinal(result || txHash, walletAddress, wallet, 60000, onProgress);
      assertFinalSuccess(finalResult, "Listing approval");
      await requireConnectedAccount(walletAddress, wallet);
      if (typeof setListingStep === "function") setListingStep("Verifying on-chain listing state...");
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

      setListings((prev) => [
        {
          id: contractListingId,
          contract_sale_id: contractListingId,
          contract_listing_id: contractListingId,
          token_id: tokenId,
          nft_id: listingNFT.id,
          seller_id: user?.id || walletAddress,
          seller: walletAddress,
          price,
          status: "active",
          created_at: new Date().toISOString(),
        },
        ...prev,
      ]);

      if (typeof setMintSuccessNFT === "function") {
        setMintSuccessNFT((prev) => (prev && (prev.tokenId === tokenId || prev.nft?.id === listingNFT.id) ? {
          ...prev,
          listed: true,
          listingPrice: price,
          contractListingId,
        } : prev));
      }

      const nftName = listingNFT.name;

      setListingNFT(null);

      setListingForm({
        price: "",
        currency: "NEAR",
      });
      if (typeof setListingStep === "function") setListingStep("");

      // On-chain listing is fully confirmed. Check indexer with short timeout (2.5s) to avoid UI blocking.
      const indexed = await waitForIndexedListing({ transactionHash: txHash, status: "active", timeoutMs: 2500 }).catch(() => null);

      setSuccess(indexed
        ? `"${nftName}" is listed on NEAR at ${price} NEAR and verified in the marketplace catalog.`
        : `"${nftName}" is listed on NEAR at ${price} NEAR. Catalog verification is still pending; the item will appear after the trusted indexer catches up.`);
      await fetchListings();
      if (typeof fetchNFTs === "function") await fetchNFTs();

      // If catalog indexing is still catching up, background poll updates catalog without blocking the user
      if (!indexed) {
        waitForIndexedListing({ transactionHash: txHash, status: "active", timeoutMs: 30000 })
          .then(async (bg) => {
            if (bg) {
              await fetchListings();
              if (typeof fetchNFTs === "function") await fetchNFTs();
            }
          })
          .catch(() => {});
      }
    } catch (err) {
      console.error("NFT listing error:", err);

      setError(
        err?.message ||
          "Unable to list this NFT."
      );
    } finally {
      setListing(null);
      if (typeof setListingStep === "function") setListingStep("");
    }
  };

  // =========================================================
  // CANCEL LISTING
  // =========================================================

  const handleCancelListing = async (nft) => {
    if (!LISTING_ACTIONS_ENABLED) {
      setSuccess("");
      setError(LISTING_UNAVAILABLE_MESSAGE);
      return;
    }

    const currentListing = getNFTListing(nft.id);

    if (!currentListing || cancelling) {
      return;
    }

    const confirmed = window.confirm(
      `Remove "${nft.name}" from the marketplace?`
    );

    if (!confirmed) {
      return;
    }

    try {
      setCancelling(nft.id);
      setError("");
      setSuccess("");

      const {
        data: { user: authUser },
      } = await supabase.auth.getUser().catch(() => ({ data: { user: null } }));

      if (authUser && currentListing.seller_id && authUser.id !== currentListing.seller_id && currentListing.seller?.toLowerCase() !== walletAddress?.toLowerCase()) {
        throw new Error(
          "You are not the owner of this listing."
        );
      }

      if (!isConnected || !walletAddress || !selector) {
        throw new Error("Connect the seller NEAR wallet before cancelling.");
      }
      const saleId = currentListing.contract_sale_id || currentListing.contract_listing_id || currentListing.id;
      if (!saleId) {
        throw new Error("Listing is missing its verified on-chain listing ID.");
      }
      const wallet = { selector, accountId: walletAddress };
      await requireConnectedAccount(walletAddress, wallet);

      if (currentListing.is_edition || currentListing.id?.startsWith("edition|") || saleId?.startsWith("edition|")) {
        const result = await cancelEditionListingOnChain({
          listingId: saleId,
          wallet,
        });
        const txHash = transactionHash(result);
        const finalResult = await waitForFinal(result || txHash, walletAddress, wallet);
        assertFinalSuccess(finalResult, "Edition listing cancellation");
        setSuccess(`"${nft.name}" listing was cancelled on NEAR and escrowed copies returned to your wallet.`);
        await fetchListings();
        return;
      }

      const before = await marketplaceListing(saleId, wallet);
      if (!before?.active || before.seller.toLowerCase() !== walletAddress.toLowerCase()) {
        throw new Error("The on-chain listing is no longer active for this seller.");
      }
      const result = await cancelListingOnChain(saleId, wallet);
      const txHash = transactionHash(result);
      const finalResult = await waitForFinal(result || txHash, walletAddress, wallet);
      assertFinalSuccess(finalResult, "Listing cancellation");
      await requireConnectedAccount(walletAddress, wallet);
      const [after, metadata] = await Promise.all([
        marketplaceListing(saleId, wallet),
        marketplaceListingMetadata(saleId, wallet),
      ]);
      if (after?.active || after?.seller?.toLowerCase() !== walletAddress.toLowerCase() || metadata?.status !== "cancelled") {
        throw new Error("Final NEAR state did not confirm the listing cancellation.");
      }

      const indexed = await waitForIndexedListing({
        contractListingId: saleId,
        status: "cancelled",
        timeoutMs: 2500,
      }).catch(() => null);

      setSuccess(indexed
        ? `"${nft.name}" was cancelled on NEAR and removed from the verified catalog.`
        : `"${nft.name}" was cancelled on NEAR. Trusted indexer verification is still pending.`);
      await fetchListings();

      if (!indexed) {
        waitForIndexedListing({
          contractListingId: saleId,
          status: "cancelled",
          timeoutMs: 30000,
        }).then(async (bg) => {
          if (bg) await fetchListings();
        }).catch(() => {});
      }
    } catch (err) {
      console.error(
        "Cancel listing error:",
        err
      );

      setError(
        err?.message ||
          "Unable to remove this listing."
      );
    } finally {
      setCancelling(null);
    }
  };

  // =========================================================
  // DELETE NFT
  // =========================================================

  const handleDelete = async (nft) => {
    setSuccess("");
    const deletionError = getNFTDeletionError(nft, user?.id);
    if (deletionError) {
      setError(deletionError);
      return;
    }

    if (deleting) {
      return;
    }

    const confirmed = window.confirm(
      `Are you sure you want to delete "${nft.name}"?`
    );

    if (!confirmed) {
      return;
    }

    try {
      setDeleting(nft.id);
      setError("");
      setSuccess("");

      await deleteNFT(nft);

      setNfts((currentNFTs) =>
        currentNFTs.filter(
          (item) => item.id !== nft.id
        )
      );

      setSuccess("NFT entry deleted. Its uploaded image is retained.");
    } catch (err) {
      console.error("NFT delete error:", err);

      setError(
        err?.message ||
          "Unable to delete this NFT."
      );
    } finally {
      setDeleting(null);
    }
  };

  // =========================================================
  // VIEW NFT
  // =========================================================

  const handleViewNFT = (nft) => {
    window.location.href = `/nft/${nft.id}`;
  };

  // =========================================================
  // CLEAN PREVIEW ON UNMOUNT
  // =========================================================

  useEffect(() => {
    return () => {
      if (preview) {
        URL.revokeObjectURL(preview);
      }
    };
  }, [preview]);

  // =========================================================
  // RENDER
  // =========================================================

  return (
    <div className="min-h-screen bg-black px-4 py-6 text-white sm:px-6">

      {/* HEADER */}

      <div className="mb-8">
        <h1 className="text-3xl font-bold tracking-tight">
          My NFTs
        </h1>

        <p className="mt-3 text-sm text-gray-400">
          Create, manage and showcase your digital assets.
        </p>
      </div>

      {/* CONNECTED WALLET / MINT AUTHORITY NOTICE */}
      {isConnected && walletAddress && (
        NFT_CONTRACT_ID === LEGACY_NFT_CONTRACT_ID && walletAddress.toLowerCase() !== "seller.roundfrog-dev.testnet" ? (
          <div className="mb-6 rounded-xl border border-amber-500/30 bg-amber-500/10 p-4 text-sm text-amber-200">
            <p className="font-semibold text-amber-100 mb-1">
              Connected Wallet: <span className="font-mono text-white">{walletAddress}</span>
            </p>
            <p>
              On-chain NFT minting on legacy contract <span className="font-mono text-amber-300">{LEGACY_NFT_CONTRACT_ID}</span> requires the configured Testnet mint authority <span className="font-mono font-bold text-white">seller.roundfrog-dev.testnet</span>. Please configure <span className="font-mono font-bold text-white">VITE_NFT_CONTRACT_ID</span> to the reviewed permissionless contract to mint from any wallet.
            </p>
          </div>
        ) : MINTING_ACTIONS_ENABLED ? (
          <div className="mb-6 rounded-xl border border-emerald-500/30 bg-emerald-500/10 p-4 text-sm text-emerald-200">
            <p className="font-semibold text-emerald-100 mb-1">
              Connected Wallet: <span className="font-mono text-white">{walletAddress}</span>
            </p>
            <p>
              Permissionless minting enabled on <span className="font-mono text-emerald-300">{NFT_CONTRACT_ID}</span>. Your connected wallet will become the verified initial token owner on NEAR Testnet.
            </p>
          </div>
        ) : (
          <div className="mb-6 rounded-xl border border-blue-500/30 bg-blue-500/10 p-4 text-sm text-blue-200">
            <p className="font-semibold text-blue-100 mb-1">
              Connected Wallet: <span className="font-mono text-white">{walletAddress}</span>
            </p>
            <p>
              Target Contract: <span className="font-mono text-blue-300">{NFT_CONTRACT_ID}</span> (Permissionless). Awaiting final Testnet lifecycle acceptance verification before live minting actions are unpaused.
            </p>
          </div>
        )
      )}

      {!MINTING_ACTIONS_ENABLED && (
        <p
          id="minting-unavailable"
          role="status"
          className="mb-4 rounded-xl border border-amber-500/20 bg-amber-500/10 p-4 text-sm text-amber-200"
        >
          {MINTING_UNAVAILABLE_MESSAGE}
        </p>
      )}

      {!LISTING_ACTIONS_ENABLED && (
        <p
          id="listing-unavailable"
          role="status"
          className="mb-6 rounded-xl border border-amber-500/20 bg-amber-500/10 p-4 text-sm text-amber-200"
        >
          {LISTING_UNAVAILABLE_MESSAGE}
        </p>
      )}

      {/* ERROR */}

      {error && (
        <div className="mb-6 flex items-center justify-between rounded-xl border border-red-500/20 bg-red-500/10 px-4 py-3 text-sm text-red-400">
          <span>{error}</span>

          <button
            type="button"
            onClick={() => setError("")}
            className="ml-4 transition hover:text-white"
          >
            <X size={17} />
          </button>
        </div>
      )}

      {/* SUCCESS */}

      {success && (
        <div className="mb-6 flex items-center justify-between rounded-xl border border-green-500/20 bg-green-500/10 px-4 py-3 text-sm text-green-400">
          <span>{success}</span>

          <button
            type="button"
            onClick={() => setSuccess("")}
            className="ml-4 transition hover:text-white"
          >
            <X size={17} />
          </button>
        </div>
      )}

      {/* MINTED NFT SUCCESS PANEL */}

      {mintSuccessNFT && (
        <div
          data-testid="post-mint-success-panel"
          className="mb-8 overflow-hidden rounded-2xl border border-purple-500/30 bg-gradient-to-br from-purple-900/20 via-[#131127] to-black/40 p-5 sm:p-6 backdrop-blur-md shadow-xl"
        >
          <div className="mb-4 flex items-center justify-between">
            <div className="flex items-center gap-2.5">
              <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-emerald-500/20 text-emerald-400">
                <CheckCircle2 size={18} />
              </div>
              <div>
                <h3 className="text-base font-semibold text-white">
                  {mintSuccessNFT.listed ? "NFT Listed for Sale!" : "NFT Minted on NEAR!"}
                </h3>
                <p className="text-xs text-gray-400">
                  {mintSuccessNFT.listed
                    ? "Your NFT is active on the Testnet marketplace."
                    : "Verified on-chain. Ready to be listed for sale."}
                </p>
              </div>
            </div>
            <button
              type="button"
              onClick={() => setMintSuccessNFT(null)}
              className="flex h-8 w-8 items-center justify-center rounded-lg bg-white/5 text-gray-400 transition hover:bg-white/10 hover:text-white"
              title="Dismiss panel"
            >
              <X size={16} />
            </button>
          </div>

          <div className="flex flex-col gap-5 sm:flex-row sm:items-center sm:justify-between rounded-xl border border-white/5 bg-white/[0.02] p-4">
            <div className="flex items-center gap-4">
              {mintSuccessNFT.imageUrl && (
                <img
                  src={mintSuccessNFT.imageUrl}
                  alt={mintSuccessNFT.name}
                  className="h-20 w-20 flex-shrink-0 rounded-xl border border-white/10 object-cover"
                />
              )}
              <div className="space-y-1">
                <div className="flex items-center gap-2">
                  <h4 className="font-semibold text-white text-base">
                    {mintSuccessNFT.name}
                  </h4>
                  <span className="rounded-full bg-purple-500/10 px-2 py-0.5 text-[11px] font-medium text-purple-300 border border-purple-500/20 font-mono">
                    {mintSuccessNFT.tokenId}
                  </span>
                </div>
                <p className="text-xs text-gray-400">
                  Owner: <span className="font-mono text-gray-300">{mintSuccessNFT.ownerId}</span>
                </p>
                {mintSuccessNFT.listed ? (
                  <div className="flex items-center gap-2 pt-1">
                    <span className="inline-flex items-center gap-1 text-xs font-semibold text-emerald-400">
                      <Tag size={13} />
                      Listed at {mintSuccessNFT.listingPrice} NEAR
                    </span>
                  </div>
                ) : (
                  <p className="text-xs text-amber-300/90 pt-0.5">
                    Not listed on the marketplace yet
                  </p>
                )}
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-2 sm:self-center">
              <button
                type="button"
                onClick={() => handleViewNFT(mintSuccessNFT.nft)}
                className="flex items-center gap-1.5 rounded-xl border border-white/10 bg-white/5 px-4 py-2.5 text-xs font-semibold text-gray-200 transition hover:bg-white/10 hover:text-white"
              >
                View NFT
                <ArrowUpRight size={14} />
              </button>

              {mintSuccessNFT.listed ? (
                <button
                  type="button"
                  onClick={() => { window.location.href = "/marketplace"; }}
                  className="flex items-center gap-1.5 rounded-xl bg-purple-600 px-4 py-2.5 text-xs font-semibold text-white transition hover:bg-purple-700 shadow-md shadow-purple-600/20"
                >
                  <ShoppingBag size={14} />
                  View in Marketplace
                </button>
              ) : (
                <button
                  type="button"
                  onClick={() => openListingForm(mintSuccessNFT.nft)}
                  disabled={!LISTING_ACTIONS_ENABLED || (typeof listing !== "undefined" && Boolean(mintSuccessNFT?.nft) && listing === mintSuccessNFT.nft?.id)}
                  aria-describedby={!LISTING_ACTIONS_ENABLED ? "listing-unavailable" : undefined}
                  className="flex items-center gap-1.5 rounded-xl bg-purple-600 px-4 py-2.5 text-xs font-semibold text-white transition hover:bg-purple-700 disabled:opacity-50 disabled:cursor-not-allowed shadow-md shadow-purple-600/20"
                >
                  <Tag size={14} />
                  List for Sale
                </button>
              )}
            </div>
          </div>
        </div>
      )}

      {/* CREATE NFT */}

      <div className="mb-12 rounded-2xl border border-white/10 bg-white/[0.03] p-5 sm:p-6 lg:p-8">
        <div className="mb-6">
          <h2 className="text-xl font-semibold">
            Create New NFT
          </h2>

          <p className="mt-1 text-sm text-gray-500">
            Upload your digital asset and save it to your collection. Marketplace
            visibility requires a separate on-chain listing after minting.
          </p>
        </div>

        <form
          onSubmit={handleSubmit}
          className="grid gap-8 lg:grid-cols-[320px_1fr]"
        >
          {/* IMAGE UPLOAD */}

          <div>
            <label className="mb-3 block text-sm font-medium">
              NFT Image
            </label>

            {preview ? (
              <div className="relative overflow-hidden rounded-2xl border border-white/10 bg-black/20">
                <img
                  src={preview}
                  alt="NFT preview"
                  className="aspect-square w-full object-cover"
                />

                <button
                  type="button"
                  onClick={removeImage}
                  disabled={loading}
                  className="absolute right-3 top-3 flex h-9 w-9 items-center justify-center rounded-full bg-black/70 text-white backdrop-blur-md transition hover:bg-red-500 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <X size={17} />
                </button>
              </div>
            ) : (
              <label className="group flex aspect-square cursor-pointer flex-col items-center justify-center rounded-2xl border border-dashed border-white/15 bg-white/[0.02] transition hover:border-purple-500/50 hover:bg-purple-500/[0.03]">
                <div className="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-purple-600/10 text-purple-400 transition group-hover:bg-purple-600 group-hover:text-white">
                  <ImagePlus size={26} />
                </div>

                <p className="text-sm font-medium">
                  Upload image
                </p>

                <p className="mt-2 px-6 text-center text-xs text-gray-500">
                  PNG, JPG or WEBP
                  <br />
                  Maximum 5MB
                </p>

                <input
                  ref={fileInputRef}
                  type="file"
                  accept="image/png,image/jpeg,image/webp"
                  onChange={handleImageChange}
                  disabled={loading}
                  className="hidden"
                />
              </label>
            )}
          </div>

          {/* FORM */}

          <div className="space-y-5">

            {/* NAME */}

            <div>
              <label className="mb-2 block text-sm font-medium">
                NFT Name
              </label>

              <input
                type="text"
                name="name"
                value={form.name}
                onChange={handleChange}
                maxLength={100}
                placeholder="Enter NFT name"
                className="w-full rounded-xl border border-white/10 bg-white/[0.04] px-4 py-3 text-sm text-white outline-none transition placeholder:text-gray-600 focus:border-purple-500"
              />
            </div>

            {/* DESCRIPTION */}

            <div>
              <label className="mb-2 block text-sm font-medium">
                Description
              </label>

              <textarea
                name="description"
                value={form.description}
                onChange={handleChange}
                maxLength={1000}
                rows={5}
                placeholder="Describe your NFT..."
                className="w-full resize-none rounded-xl border border-white/10 bg-white/[0.04] px-4 py-3 text-sm text-white outline-none transition placeholder:text-gray-600 focus:border-purple-500"
              />
            </div>

            {/* CATEGORY */}

            <div>
              <label className="mb-2 block text-sm font-medium">
                Category
              </label>

              <select
                name="category"
                value={form.category}
                onChange={handleChange}
                className="w-full rounded-xl border border-white/10 bg-[#11111a] px-4 py-3 text-sm text-white outline-none transition focus:border-purple-500"
              >
                {CATEGORIES.map((category) => (
                  <option
                    key={category}
                    value={category}
                  >
                    {category}
                  </option>
                ))}
              </select>
            </div>

            {/* CREATION MODEL: UNIQUE VS LIMITED EDITION */}

            <div>
              <label className="mb-2 block text-sm font-medium">
                Creation Model
              </label>
              <div className="grid grid-cols-2 gap-3">
                <button
                  type="button"
                  onClick={() => setForm((c) => ({ ...c, model: "unique" }))}
                  className={`flex flex-col rounded-xl border p-3.5 text-left transition ${
                    form.model === "unique"
                      ? "border-purple-500 bg-purple-500/10 text-white"
                      : "border-white/10 bg-white/[0.02] text-gray-400 hover:border-white/20"
                  }`}
                >
                  <span className="text-sm font-semibold text-white">Unique (1 of 1)</span>
                  <span className="mt-1 text-xs text-gray-400">NEP-171 single collectible</span>
                </button>
                <button
                  type="button"
                  onClick={() => setForm((c) => ({ ...c, model: "edition" }))}
                  className={`flex flex-col rounded-xl border p-3.5 text-left transition ${
                    form.model === "edition"
                      ? "border-purple-500 bg-purple-500/10 text-white"
                      : "border-white/10 bg-white/[0.02] text-gray-400 hover:border-white/20"
                  }`}
                >
                  <span className="text-sm font-semibold text-white">Limited Edition</span>
                  <span className="mt-1 text-xs text-gray-400">NEP-245 fixed supply batch</span>
                </button>
              </div>
            </div>

            {form.model === "edition" && (
              <div>
                <label className="mb-2 block text-sm font-medium">
                  Total Edition Supply
                </label>
                <input
                  type="number"
                  name="editionSupply"
                  value={form.editionSupply}
                  onChange={handleChange}
                  min="2"
                  max="1000000"
                  placeholder="e.g. 100"
                  className="w-full rounded-xl border border-white/10 bg-white/[0.04] px-4 py-3 text-sm text-white outline-none transition placeholder:text-gray-600 focus:border-purple-500"
                />
                <p className="mt-1 text-xs text-gray-500">
                  Fixed supply cap minted in your wallet. Buyers can purchase individual or multiple copies.
                </p>
              </div>
            )}

            {/* PRICE */}

            <div className="grid grid-cols-1 gap-4 sm:grid-cols-[1fr_140px]">
              <div>
                <label className="mb-2 block text-sm font-medium">
                  Price
                </label>

                <input
                  type="text"
                  inputMode="decimal"
                  name="price"
                  value={form.price}
                  onChange={handleChange}
                  placeholder="0.00"
                  className="w-full rounded-xl border border-white/10 bg-white/[0.04] px-4 py-3 text-sm text-white outline-none transition placeholder:text-gray-600 focus:border-purple-500"
                />
              </div>

              {/* CURRENCY */}

              <div>
                <label className="mb-2 block text-sm font-medium">
                  Currency
                </label>

                <select
                  name="currency"
                  value={form.currency}
                  onChange={handleChange}
                  className="w-full rounded-xl border border-white/10 bg-[#11111a] px-4 py-3 text-sm text-white outline-none transition focus:border-purple-500"
                >
                  <option value="NEAR">
                    Testnet NEAR
                  </option>
                </select>
              </div>
            </div>

            {/* SUBMIT */}

            <div className="pt-2">
              <button
                type="submit"
                disabled={loading}
                className="flex w-full items-center justify-center gap-2 rounded-xl bg-purple-600 px-5 py-3 text-sm font-semibold transition hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-60 sm:w-fit"
              >
                {loading ? (
                  <>
                    <Loader2
                      size={18}
                      className="animate-spin"
                    />

                    {uploading
                      ? "Uploading image..."
                      : "Creating..."}
                  </>
                ) : (
                  <>
                    <Upload size={18} />
                    Create NFT
                  </>
                )}
              </button>
            </div>
          </div>
        </form>
      </div>

      {/* YOUR NFTS */}

      <div>
        <div className="mb-6 flex items-center justify-between">
          <div>
            <h2 className="text-2xl font-bold">
              Your NFTs
            </h2>

            <p className="mt-1 text-sm text-gray-500">
              {nfts.length}{" "}
              {nfts.length === 1 ? "NFT" : "NFTs"}{" "}
              owned
            </p>
          </div>

          <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-purple-600/10 text-purple-400">
            <Plus size={20} />
          </div>
        </div>

        {loadingNFTs ? (
          <div className="flex min-h-[220px] items-center justify-center rounded-2xl border border-white/10 bg-white/[0.03]">
            <Loader2
              size={28}
              className="animate-spin text-purple-500"
            />
          </div>
        ) : nfts.length > 0 ? (
          <div className="grid grid-cols-1 gap-5 sm:grid-cols-2 xl:grid-cols-3">
            {nfts.map((nft) => {
              const currentListing =
                getNFTListing(nft.id);
              const deletionError = getNFTDeletionError(nft, user?.id);

              return (
                <div
                  key={nft.id}
                  className="group overflow-hidden rounded-2xl border border-white/10 bg-white/[0.03] transition-colors duration-300 hover:-translate-y-1 hover:border-purple-500/40"
                >
                  {/* NFT IMAGE */}

                  <div className="relative aspect-[4/3] overflow-hidden">
                    <img
                      src={nft.image_url}
                      alt={nft.name}
                      loading="lazy"
                      className="h-full w-full object-cover transition duration-500 group-hover:scale-105"
                    />

                    <div className="absolute left-3 top-3 rounded-lg bg-black/60 px-3 py-1.5 text-xs font-medium backdrop-blur-md">
                      {nft.category}
                    </div>

                    {currentListing && (
                      <div className="absolute bottom-3 left-3 flex items-center gap-1.5 rounded-lg bg-green-500/90 px-3 py-1.5 text-xs font-semibold text-white backdrop-blur-md">
                        <ShoppingBag size={13} />
                        Listed
                      </div>
                    )}

                    {/* DELETE */}

                    <button
                      type="button"
                      onClick={() => handleDelete(nft)}
                      aria-label="Delete NFT"
                      title={deletionError || "Delete NFT"}
                      disabled={
                        !!deleting ||
                        !!deletionError
                      }
                      className="absolute right-3 top-3 flex h-9 w-9 items-center justify-center rounded-full bg-black/60 text-white backdrop-blur-md transition hover:bg-red-500 disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      {deleting === nft.id ? (
                        <Loader2
                          size={16}
                          className="animate-spin"
                        />
                      ) : (
                        <Trash2 size={16} />
                      )}
                    </button>
                  </div>

                  {/* DETAILS */}

                  <div className="p-5">
                    <div className="mb-4">
                      <h3 className="text-lg font-semibold">
                        {nft.name}
                      </h3>

                      <p className="mt-1 line-clamp-2 text-sm text-gray-500">
                        {nft.description ||
                          "No description provided."}
                      </p>
                    </div>

                    {/* PRICE */}

                    <div className="mb-4">
                      <p className="text-xs text-gray-500">
                        {currentListing
                          ? "Listing price"
                          : "Price"}
                      </p>

                      <p className="mt-1 text-base font-semibold">
                        {currentListing
                          ? `${currentListing.price} ${currentListing.currency || "NEAR"}`
                          : `${nft.price} ${nft.currency || "NEAR"}`}
                      </p>
                    </div>

                    {/* ACTIONS */}

                    <div className="flex gap-2">
                      {!nft.token_id ? (
                        <button
                          type="button"
                          onClick={() => mintCatalogNFT(nft)}
                          disabled={!MINTING_ACTIONS_ENABLED || minting === nft.id}
                          aria-describedby={!MINTING_ACTIONS_ENABLED ? "minting-unavailable" : undefined}
                          title={!MINTING_ACTIONS_ENABLED ? MINTING_UNAVAILABLE_MESSAGE : undefined}
                          className="flex flex-1 items-center justify-center gap-1.5 rounded-xl bg-[#00EC97] px-3 py-2 text-sm font-medium text-black transition hover:bg-[#00C781] disabled:cursor-not-allowed disabled:opacity-50"
                        >
                          {minting === nft.id ? (
                            <><Loader2 size={15} className="animate-spin" /> Minting...</>
                          ) : (
                            <><Upload size={15} /> Mint on NEAR</>
                          )}
                        </button>
                      ) : currentListing ? (
                        <button
                          type="button"
                          onClick={() =>
                            handleCancelListing(nft)
                          }
                          disabled={
                            !LISTING_ACTIONS_ENABLED || cancelling === nft.id
                          }
                          aria-describedby={!LISTING_ACTIONS_ENABLED ? "listing-unavailable" : undefined}
                          className="flex flex-1 items-center justify-center gap-1.5 rounded-xl border border-red-500/40 px-3 py-2 text-sm font-medium text-red-400 transition hover:bg-red-500 hover:text-white disabled:cursor-not-allowed disabled:opacity-50"
                        >
                          {cancelling === nft.id ? (
                            <>
                              <Loader2
                                size={15}
                                className="animate-spin"
                              />
                              Removing...
                            </>
                          ) : (
                            <>
                              <X size={15} />
                              Cancel Listing
                            </>
                          )}
                        </button>
                      ) : (
                        <button
                          type="button"
                          onClick={() =>
                            openListingForm(nft)
                          }
                          disabled={!LISTING_ACTIONS_ENABLED}
                          aria-describedby={!LISTING_ACTIONS_ENABLED ? "listing-unavailable" : undefined}
                          title={!LISTING_ACTIONS_ENABLED ? LISTING_UNAVAILABLE_MESSAGE : undefined}
                          className="flex flex-1 items-center justify-center gap-1.5 rounded-xl bg-purple-600 px-3 py-2 text-sm font-medium text-white transition hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-50"
                        >
                          <Tag size={15} />
                          List NFT
                        </button>
                      )}

                      <button
                        type="button"
                        onClick={() =>
                          handleViewNFT(nft)
                        }
                        className="flex items-center gap-1.5 rounded-xl border border-purple-500/40 px-3 py-2 text-sm font-medium text-purple-400 transition hover:bg-purple-600 hover:text-white"
                      >
                        View
                        <ArrowUpRight size={15} />
                      </button>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="rounded-2xl border border-white/10 bg-white/[0.03] py-16 text-center">
            <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-purple-600/10 text-purple-400">
              <ImagePlus size={25} />
            </div>

            <p className="text-lg font-semibold">
              No NFTs yet
            </p>

            <p className="mt-2 text-sm text-gray-500">
              Create or acquire your first NFT.
            </p>
          </div>
        )}
      </div>

      {/* LISTING MODAL */}

      {LISTING_ACTIONS_ENABLED && listingNFT && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 px-4 backdrop-blur-sm">
          <div className="w-full max-w-md rounded-2xl border border-white/10 bg-[#11111a] p-6 shadow-2xl">

            <div className="mb-6 flex items-start justify-between">
              <div>
                <h2 className="text-xl font-semibold">
                  List NFT for Sale
                </h2>

                <p className="mt-1 text-sm text-gray-500">
                  {listingNFT.name}
                </p>
              </div>

              <button
                type="button"
                onClick={closeListingForm}
                disabled={
                  listing === listingNFT.id
                }
                className="flex h-9 w-9 items-center justify-center rounded-full bg-white/5 text-gray-400 transition hover:bg-white/10 hover:text-white disabled:cursor-not-allowed disabled:opacity-50"
              >
                <X size={18} />
              </button>
            </div>

            <form
              onSubmit={handleListNFT}
              className="space-y-5"
            >
              {(listingNFT.model === "edition" || listingNFT.nft_contract_id === EDITIONS_CONTRACT_ID) && (
                <div>
                  <label className="mb-2 block text-sm font-medium">
                    Quantity to List (Copies)
                  </label>
                  <input
                    type="number"
                    min="1"
                    value={listingForm.quantity}
                    onChange={(e) =>
                      setListingForm((current) => ({
                        ...current,
                        quantity: e.target.value,
                      }))
                    }
                    disabled={listing === listingNFT.id}
                    placeholder="Enter quantity to list"
                    className="w-full rounded-xl border border-white/10 bg-white/[0.04] px-4 py-3 text-sm text-white outline-none transition placeholder:text-gray-600 focus:border-purple-500 disabled:opacity-60"
                  />
                  <p className="mt-1 text-xs text-gray-500">
                    Price below is per individual copy.
                  </p>
                </div>
              )}

              <div>
                <label className="mb-2 block text-sm font-medium">
                  Sale Price {listingNFT.model === "edition" || listingNFT.nft_contract_id === EDITIONS_CONTRACT_ID ? "(Per Copy)" : ""}
                </label>

                <input
                  type="text"
                  inputMode="decimal"
                  value={listingForm.price}
                  onChange={(e) =>
                    setListingForm((current) => ({
                      ...current,
                      price: e.target.value,
                    }))
                  }
                  disabled={
                    listing === listingNFT.id
                  }
                  placeholder="Enter price"
                  className="w-full rounded-xl border border-white/10 bg-white/[0.04] px-4 py-3 text-sm text-white outline-none transition placeholder:text-gray-600 focus:border-purple-500 disabled:opacity-60"
                />
              </div>

              <div>
                <label className="mb-2 block text-sm font-medium">
                  Currency
                </label>

                <select
                  value={listingForm.currency}
                  onChange={(e) =>
                    setListingForm((current) => ({
                      ...current,
                      currency: e.target.value,
                    }))
                  }
                  disabled={
                    listing === listingNFT.id
                  }
                  className="w-full rounded-xl border border-white/10 bg-black px-4 py-3 text-sm text-white outline-none transition focus:border-purple-500 disabled:opacity-60"
                >
                  <option value="NEAR">
                    Testnet NEAR
                  </option>
                </select>
              </div>

              <div className="rounded-xl border border-white/10 bg-white/[0.02] p-3 text-xs text-gray-400 space-y-1">
                <p className="font-medium text-gray-300">Transaction & Deposit Requirements:</p>
                <p>• Approval deposit: ~0.01 NEAR attached for NFT contract approval.</p>
                <p>• Storage deposit: up to 0.12 NEAR for contract storage if not already registered.</p>
              </div>

              <div className="flex gap-3 pt-2">
                <button
                  type="button"
                  onClick={closeListingForm}
                  disabled={
                    listing === listingNFT.id
                  }
                  className="flex-1 rounded-xl border border-white/10 bg-white/[0.04] px-4 py-3 text-sm font-medium text-gray-300 transition hover:bg-white/[0.08] hover:text-white disabled:opacity-50"
                >
                  Cancel
                </button>

                <button
                  type="submit"
                  disabled={
                    !LISTING_ACTIONS_ENABLED || listing === listingNFT.id
                  }
                  className="flex flex-1 items-center justify-center gap-2 rounded-xl bg-purple-600 px-4 py-3 text-sm font-semibold text-white transition hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-60"
                >
                  {listing === listingNFT.id ? (
                    <>
                      <Loader2
                        size={17}
                        className="animate-spin"
                      />
                      {listingStep || "Processing..."}
                    </>
                  ) : (
                    <>
                      <Tag size={17} />
                      List NFT
                    </>
                  )}
                </button>
              </div>
              {listing === listingNFT.id && listingStep && (
                <div className="mt-3 flex items-center justify-center gap-2 text-xs text-purple-300">
                  <span className="inline-block h-2 w-2 rounded-full bg-purple-400 animate-pulse" />
                  <span>{listingStep}</span>
                </div>
              )}
            </form>
          </div>
        </div>
      )}
    </div>
  );
}

export default CreateNFT;
