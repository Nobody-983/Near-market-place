import { supabase } from "../lib/supabase";
import { validateNearAmount } from "../lib/near/utils";

// Keep every NFT field while returning numeric prices as lossless JSON strings.
export const NFT_SELECT_FIELDS = `
  id, creator_id, name, description, image_url, image_path, category, price::text,
  currency, source, network_id, token_id, nft_contract_id, mint_tx_hash,
  onchain_status, created_at, updated_at
`;

const BUCKET_NAME = "nft-images";
const MAX_IMAGE_SIZE = 5 * 1024 * 1024;

const ALLOWED_IMAGE_TYPES = [
  "image/png",
  "image/jpeg",
  "image/webp",
];

const ALLOWED_EXTENSIONS = [
  "png",
  "jpg",
  "jpeg",
  "webp",
];

const UUID_PATTERN =
  "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}";
const IMAGE_PATH_PATTERN = new RegExp(
  `^(${UUID_PATTERN})/${UUID_PATTERN}\\.(png|jpg|jpeg|webp)$`
);

function getNFTImageReference(filePath, userId) {
  const match = typeof filePath === "string" && filePath.match(IMAGE_PATH_PATTERN);

  if (!match || match[0] !== filePath || match[1] !== userId) {
    throw new Error("IMAGE ERROR: Invalid storage path for the current user.");
  }

  const { data } = supabase.storage.from(BUCKET_NAME).getPublicUrl(filePath);

  if (!data?.publicUrl) {
    throw new Error("STORAGE ERROR: Could not generate public URL.");
  }

  return { filePath, publicUrl: data.publicUrl };
}

function getNFTImageReferenceFromUrl(imageUrl, userId) {
  // The configured client supplies the trusted origin AND bucket prefix.
  // Compare the raw URL so traversal, encoding and query variants are rejected.
  const { data } = supabase.storage.from(BUCKET_NAME).getPublicUrl("");
  const bucketUrl = data?.publicUrl;

  if (!bucketUrl || typeof imageUrl !== "string" || !imageUrl.startsWith(bucketUrl)) {
    throw new Error("IMAGE ERROR: Image URL must belong to the configured nft-images bucket.");
  }

  return getNFTImageReference(imageUrl.slice(bucketUrl.length), userId);
}

// ---------------------------------------------
// Validate image
// ---------------------------------------------
function validateImage(file) {
  if (!file) {
    throw new Error("IMAGE ERROR: No image selected.");
  }

  if (!ALLOWED_IMAGE_TYPES.includes(file.type)) {
    throw new Error(
      "IMAGE ERROR: Only PNG, JPEG, and WebP images are allowed."
    );
  }

  if (file.size > MAX_IMAGE_SIZE) {
    throw new Error(
      "IMAGE ERROR: Image must be smaller than 5MB."
    );
  }

  const extension = file.name
    ?.split(".")
    .pop()
    ?.toLowerCase();

  if (!extension || !ALLOWED_EXTENSIONS.includes(extension)) {
    throw new Error(
      "IMAGE ERROR: Invalid image file extension."
    );
  }
}

// ---------------------------------------------
// Get authenticated user
// ---------------------------------------------
async function getAuthenticatedUser() {
  const { data, error } = await supabase.auth.getSession();

  if (error) {
    throw new Error(
      `SESSION CHECK FAILED: ${error.message}`
    );
  }

  const session = data?.session;

  if (!session?.user) {
    throw new Error(
      "SESSION CHECK FAILED: No authenticated Supabase user."
    );
  }

  if (!session.access_token) {
    throw new Error(
      "SESSION CHECK FAILED: No Supabase access token."
    );
  }

  return session.user;
}

// ---------------------------------------------
// Generate unique filename
// ---------------------------------------------
function generateFileName(file) {
  const extension = file.name
    .split(".")
    .pop()
    .toLowerCase();

  return `${crypto.randomUUID()}.${extension}`;
}

// ---------------------------------------------
// Upload NFT image
// ---------------------------------------------
export async function uploadNFTImage(file, userId) {
  validateImage(file);

  if (!userId) {
    throw new Error(
      "IMAGE UPLOAD FAILED: Missing user ID."
    );
  }

  const user = await getAuthenticatedUser();

  if (user.id !== userId) {
    throw new Error(
      "IMAGE UPLOAD FAILED: Authenticated user does not match user ID."
    );
  }

  const fileName = generateFileName(file);
  const filePath = `${userId}/${fileName}`;

  const imageReference = getNFTImageReference(filePath, user.id);

  // ---------------------------------------------
  // Actual Storage upload
  // ---------------------------------------------
  try {
    const {
      data,
      error,
    } = await supabase.storage
      .from(BUCKET_NAME)
      .upload(filePath, file, {
        cacheControl: "31536000",
        contentType: file.type,
        upsert: false,
      });

    if (error) {
      throw new Error(
        `SUPABASE STORAGE ERROR: ${error.message}`
      );
    }

    if (data?.path !== imageReference.filePath) {
      throw new Error(
        "STORAGE ERROR: Upload did not return the expected file path."
      );
    }

    return imageReference;
  } catch (error) {
    throw new Error(
      error?.message ||
        `IMAGE UPLOAD FAILED: ${String(error)}`,
      { cause: error }
    );
  }
}

// ---------------------------------------------
// Delete NFT image
// ---------------------------------------------
export async function deleteNFTImage(filePath) {
  if (!filePath) {
    return;
  }

  try {
    const { error } = await supabase.storage
      .from(BUCKET_NAME)
      .remove([filePath]);

    if (error) {
      throw new Error(
        `IMAGE DELETE FAILED: ${error.message}`
      );
    }
  } catch (error) {
    throw new Error(
      error?.message ||
        `IMAGE DELETE FAILED: ${String(error)}`,
      { cause: error }
    );
  }
}

// ---------------------------------------------
// Create NFT
// ---------------------------------------------
export async function createNFT({
  name,
  description,
  image_url,
  category,
  price = "0",
}) {
  try {
    const validatedPrice = validateNearAmount(price);
    const user = await getAuthenticatedUser();
    const imageReference = getNFTImageReferenceFromUrl(image_url, user.id);

    const {
      data,
      error,
    } = await supabase
      .from("nfts")
      .insert({
        creator_id: user.id,
        name,
        description,
        image_url: imageReference.publicUrl,
        image_path: imageReference.filePath,
        category,
        price: validatedPrice,
        currency: "NEAR",
      })
      .select(NFT_SELECT_FIELDS)
      .single();

    if (error) {
      throw new Error(
        `NFT DATABASE INSERT FAILED: ${error.message}`
      );
    }

    if (!data) {
      throw new Error(
        "NFT DATABASE INSERT FAILED: No NFT was returned."
      );
    }

    return { ...data, price: validateNearAmount(data.price) };
  } catch (error) {
    throw new Error(
      error?.message ||
        `NFT DATABASE INSERT FAILED: ${String(error)}`,
      { cause: error }
    );
  }
}

// ---------------------------------------------
// Delete NFT
// ---------------------------------------------
export function getNFTDeletionError(nft, userId) {
  if (!userId) return "Connect and verify your wallet before deleting an NFT.";
  if (nft?.creator_id !== userId || nft?.owner_id !== userId) {
    return "Only NFTs you created and still own can be deleted.";
  }
  if (nft.source !== "user") return "Only user-created NFT entries can be deleted.";
  if (nft.token_id !== null || nft.nft_contract_id !== null) {
    return "Only unminted NFT entries can be deleted.";
  }
  if (!Array.isArray(nft.listing_history)) return "Reload the NFT's listing history before deleting it.";
  if (nft.listing_history.length > 0) return "NFTs with listing history cannot be deleted, even after cancellation.";
  return "";
}

export async function deleteNFT(nft) {
  if (!nft?.id) {
    throw new Error(
      "DELETE NFT FAILED: Missing NFT ID."
    );
  }

  try {
    const user = await getAuthenticatedUser();

    if (nft.creator_id !== user.id) {
      throw new Error(
        "DELETE NFT FAILED: You are not the owner of this NFT."
      );
    }

    const {
      data: deleted,
      error: dbError,
    } = await supabase
      .from("nfts")
      .delete()
      .eq("id", nft.id)
      .eq("creator_id", user.id)
      .select("id")
      .maybeSingle();

    if (dbError) {
      throw new Error(
        `NFT DATABASE DELETE FAILED: ${dbError.message}`
      );
    }

    if (deleted?.id !== nft.id) {
      throw new Error("NFT was not deleted. It may no longer exist or be ineligible for deletion. Reload and try again.");
    }

    // Storage is append-only for browsers. Retained artwork needs trusted cleanup;
    // a forbidden cleanup request must not turn a confirmed deletion into a failure.
    return true;
  } catch (error) {
    throw new Error(
      error?.message ||
        `DELETE NFT FAILED: ${String(error)}`,
      { cause: error }
    );
  }
}
