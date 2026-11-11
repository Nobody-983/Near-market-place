
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
} from "lucide-react";

import { useWallet } from "../context/walletContext";
import { supabase } from "../lib/supabase";
import { validateNearAmount } from "../lib/near/utils";
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
  ensureMarketplaceListingStorage,
  listingIdFromFinalResult,
  listNftOnChain,
  marketplaceListing,
  marketplaceListingMetadata,
  mintNftOnChain,
  nftToken,
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
  const [cancelling, setCancelling] = useState(null);

  const [listingNFT, setListingNFT] = useState(null);

  const [listingForm, setListingForm] = useState({
    price: "",
    currency: "NEAR",
  });

  const [error, setError] = useState("");
  const [success, setSuccess] = useState("");

  const mintCatalogNFT = async (nft) => {
    if (!MINTING_ACTIONS_ENABLED) {
      setSuccess("");
      setError(MINTING_UNAVAILABLE_MESSAGE);
      return;
    }
    if (!isConnected || !selector || !walletAddress) {
      setError("Connect the configured NEAR Testnet mint-authority wallet first.");
      return;
    }
    if (!nft?.id || nft.token_id || nft.nft_contract_id || minting) return;

    const tokenId = `catalog-${nft.id}`;
    const wallet = { selector, accountId: walletAddress };
    setMinting(nft.id);
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
      window.setTimeout(() => { void fetchNFTs(); }, 3000);
    } catch (err) {
      setError(err?.message || "Unable to mint this NFT on NEAR Testnet.");
    } finally {
      setMinting(null);
    }
  };

  // =========================================================
  // FETCH OWNED NFTS
  // =========================================================

  const fetchNFTs = useCallback(async () => {
    if (!userId) {
      setNfts([]);
      setLoadingNFTs(false);
      return;
    }

    setLoadingNFTs(true);

    try {
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
      const ownedNFTs = (data || [])
        .filter((ownership) => ownership.nfts)
        .map(({ nfts: nft, owner_id }) => ({
          ...nft, owner_id, price: validateNearAmount(nft.price),
        }));

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
    if (!userId) {
      setListings([]);
      return;
    }

    try {
      const data = await getUserListings(userId);
      setListings(data || []);
    } catch (err) {
      console.error("Error fetching listings:", err);
    }
  }, [userId]);

  // =========================================================
  // INITIAL LOAD
  // =========================================================

  useEffect(() => {
    void Promise.resolve().then(() => Promise.all([fetchNFTs(), fetchListings()]));
  }, [fetchListings, fetchNFTs]);

  // =========================================================
  // FIND ACTIVE LISTING
  // =========================================================

  const getNFTListing = (nftId) => {
    return listings.find(
      (item) =>
        item.nft_id === nftId &&
        item.status === "active"
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
      if (!listingNFT.token_id || !listingNFT.nft_contract_id) {
        throw new Error("This catalog NFT has not been bound to an on-chain NEAR token.");
      }
      const wallet = { selector, accountId: walletAddress };
      await requireConnectedAccount(walletAddress, wallet);
      await verifyListingPreconditions({
        nftContractId: listingNFT.nft_contract_id,
        tokenId: listingNFT.token_id,
        sellerId: walletAddress,
        wallet,
      });
      await ensureMarketplaceListingStorage({ sellerId: walletAddress, wallet });
      await requireConnectedAccount(walletAddress, wallet);
      const result = await listNftOnChain({
        nftContractId: listingNFT.nft_contract_id,
        tokenId: listingNFT.token_id,
        priceNear: price,
        wallet,
      });
      const txHash = transactionHash(result);
      const finalResult = await waitForFinal(txHash, walletAddress, wallet);
      assertFinalSuccess(finalResult, "Listing approval");
      await requireConnectedAccount(walletAddress, wallet);
      const contractListingId = listingIdFromFinalResult(finalResult, {
        nftContractId: listingNFT.nft_contract_id,
        tokenId: listingNFT.token_id,
        sellerId: walletAddress,
      });
      const [confirmed, metadata] = await Promise.all([
        marketplaceListing(contractListingId, wallet),
        marketplaceListingMetadata(contractListingId, wallet),
      ]);
      if (!confirmed?.active || metadata?.status !== "active" ||
          confirmed.token_id !== listingNFT.token_id ||
          confirmed.nft_contract_id !== listingNFT.nft_contract_id ||
          confirmed.seller !== walletAddress) {
        throw new Error("Marketplace listing state could not be confirmed.");
      }

      const indexed = await waitForIndexedListing({ transactionHash: txHash, status: "active" });

      const nftName = listingNFT.name;

      setListingNFT(null);

      setListingForm({
        price: "",
        currency: "NEAR",
      });

      setSuccess(indexed
        ? `"${nftName}" is listed on NEAR and verified in the marketplace catalog.`
        : `"${nftName}" is listed on NEAR. Catalog verification is still pending; the item will appear after the trusted indexer catches up.`);
      await fetchListings();
    } catch (err) {
      console.error("NFT listing error:", err);

      setError(
        err?.message ||
          "Unable to list this NFT."
      );
    } finally {
      setListing(null);
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
        error: authError,
      } = await supabase.auth.getUser();

      if (authError) {
        throw authError;
      }

      if (!authUser) {
        throw new Error(
          "Your session has expired. Please log in again."
        );
      }

      if (authUser.id !== currentListing.seller_id) {
        throw new Error(
          "You are not the owner of this listing."
        );
      }

      if (!isConnected || !walletAddress || !selector) {
        throw new Error("Connect the seller NEAR wallet before cancelling.");
      }
      if (!currentListing.contract_sale_id) {
        throw new Error("Listing is missing its verified on-chain listing ID.");
      }
      const wallet = { selector, accountId: walletAddress };
      await requireConnectedAccount(walletAddress, wallet);
      const before = await marketplaceListing(currentListing.contract_sale_id, wallet);
      if (!before?.active || before.seller !== walletAddress) {
        throw new Error("The on-chain listing is no longer active for this seller.");
      }
      const result = await cancelListingOnChain(currentListing.contract_sale_id, wallet);
      const txHash = transactionHash(result);
      const finalResult = await waitForFinal(txHash, walletAddress, wallet);
      assertFinalSuccess(finalResult, "Listing cancellation");
      await requireConnectedAccount(walletAddress, wallet);
      const [after, metadata] = await Promise.all([
        marketplaceListing(currentListing.contract_sale_id, wallet),
        marketplaceListingMetadata(currentListing.contract_sale_id, wallet),
      ]);
      if (after?.active || after?.seller !== walletAddress || metadata?.status !== "cancelled") {
        throw new Error("Final NEAR state did not confirm the listing cancellation.");
      }

      const indexed = await waitForIndexedListing({
        contractListingId: currentListing.contract_sale_id,
        status: "cancelled",
      });

      setSuccess(indexed
        ? `"${nft.name}" was cancelled on NEAR and removed from the verified catalog.`
        : `"${nft.name}" was cancelled on NEAR. Trusted indexer verification is still pending.`);
      await fetchListings();
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
              <div>
                <label className="mb-2 block text-sm font-medium">
                  Sale Price
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
                      Listing...
                    </>
                  ) : (
                    <>
                      <Tag size={17} />
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

export default CreateNFT;
