import { execSync } from "child_process";
import fs from "fs";
import path from "path";
import crypto from "crypto";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const collectionDir = path.resolve(__dirname, "../frontend/Nft/public/collection");

const NEAR_CLI = "/home/acknowlege/.cargo/bin/near";
const RPC_URL = "https://rpc.testnet.fastnear.com";
const NFT_CONTRACT = "nft-v2.roundfrog-dev.testnet";
const MARKET_CONTRACT = "market-v2.roundfrog-dev.testnet";
const SIGNER = "roundfrog-dev.testnet";
const OPERATOR = "roundfrog-dev.testnet";

const collection = [
  {
    id: "catalog-roundfrog-genesis-01",
    file: "frog-genesis-01.svg",
    title: "CyberFrog Alpha #01",
    description: "RoundFrog Genesis Collection #01 — Vanguard Operative equipped with neon visor and plasma core.",
    priceYocto: "500000000000000000000000", // 0.5 NEAR
    category: "RoundFrog Genesis",
    rarity: "Epic",
    role: "Vanguard Operative",
  },
  {
    id: "catalog-roundfrog-genesis-02",
    file: "frog-genesis-02.svg",
    title: "Quantum Ribbit #02",
    description: "RoundFrog Genesis Collection #02 — Chrono Navigator warping timelines with quantum goggles.",
    priceYocto: "750000000000000000000000", // 0.75 NEAR
    category: "RoundFrog Genesis",
    rarity: "Rare",
    role: "Chrono Navigator",
  },
  {
    id: "catalog-roundfrog-genesis-03",
    file: "frog-genesis-03.svg",
    title: "Solaris Toad #03",
    description: "RoundFrog Genesis Collection #03 — Solar Flare Sentinel channeling photon shield and golden halo.",
    priceYocto: "1000000000000000000000000", // 1.0 NEAR
    category: "RoundFrog Genesis",
    rarity: "Rare",
    role: "Solar Flare Sentinel",
  },
  {
    id: "catalog-roundfrog-genesis-04",
    file: "frog-genesis-04.svg",
    title: "Vortex Hopper #04",
    description: "RoundFrog Genesis Collection #04 — Void Stalker phasing through dark matter with spiral optics.",
    priceYocto: "1250000000000000000000000", // 1.25 NEAR
    category: "RoundFrog Genesis",
    rarity: "Epic",
    role: "Void Stalker",
  },
  {
    id: "catalog-roundfrog-genesis-05",
    file: "frog-genesis-05.svg",
    title: "Circuit Croaker #05",
    description: "RoundFrog Genesis Collection #05 — Synthetic Architect with bio-digital neural circuit implants.",
    priceYocto: "1500000000000000000000000", // 1.5 NEAR
    category: "RoundFrog Genesis",
    rarity: "Uncommon",
    role: "Synthetic Architect",
  },
  {
    id: "catalog-roundfrog-genesis-06",
    file: "frog-genesis-06.svg",
    title: "Aurora Leap #06",
    description: "RoundFrog Genesis Collection #06 — Celestial Mystic bearing the Borealis Shimmer and crown of stars.",
    priceYocto: "1800000000000000000000000", // 1.8 NEAR
    category: "RoundFrog Genesis",
    rarity: "Legendary",
    role: "Celestial Mystic",
  },
  {
    id: "catalog-roundfrog-genesis-07",
    file: "frog-genesis-07.svg",
    title: "Neon Sentinel #07",
    description: "RoundFrog Genesis Collection #07 — Defense Commander clad in titanium hex armor and cyber visor.",
    priceYocto: "2200000000000000000000000", // 2.2 NEAR
    category: "RoundFrog Genesis",
    rarity: "Epic",
    role: "Defense Commander",
  },
  {
    id: "catalog-roundfrog-genesis-08",
    file: "frog-genesis-08.svg",
    title: "Prism Amphibian #08",
    description: "RoundFrog Genesis Collection #08 — Quantum Alchemist manipulating chromatic refraction fields.",
    priceYocto: "2500000000000000000000000", // 2.5 NEAR
    category: "RoundFrog Genesis",
    rarity: "Legendary",
    role: "Quantum Alchemist",
  },
  {
    id: "catalog-roundfrog-genesis-09",
    file: "frog-genesis-09.svg",
    title: "Zenith Bullfrog #09",
    description: "RoundFrog Genesis Collection #09 — Apex Elder meditating on singularity energy and cosmic runes.",
    priceYocto: "3000000000000000000000000", // 3.0 NEAR
    category: "RoundFrog Genesis",
    rarity: "Mythic",
    role: "Apex Elder",
  },
  {
    id: "catalog-roundfrog-genesis-10",
    file: "frog-genesis-10.svg",
    title: "Phantom Monarch #10",
    description: "RoundFrog Genesis Collection #10 — Genesis Sovereign of RoundFrog realm, crowned in celestial fire.",
    priceYocto: "4500000000000000000000000", // 4.5 NEAR
    category: "RoundFrog Genesis",
    rarity: "Mythic",
    role: "Genesis Sovereign",
  },
];

async function queryToken(tokenId) {
  try {
    const res = await fetch(RPC_URL, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: "check",
        method: "query",
        params: {
          request_type: "call_function",
          account_id: NFT_CONTRACT,
          method_name: "nft_token",
          args_base64: Buffer.from(JSON.stringify({ token_id: tokenId })).toString("base64"),
          finality: "final",
        },
      }),
    });
    const data = await res.json();
    if (!data.result || !data.result.result) return null;
    return JSON.parse(Buffer.from(data.result.result).toString());
  } catch {
    return null;
  }
}

async function queryListing(tokenId) {
  try {
    const res = await fetch(RPC_URL, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: "check_lst",
        method: "query",
        params: {
          request_type: "call_function",
          account_id: MARKET_CONTRACT,
          method_name: "listings",
          args_base64: Buffer.from(JSON.stringify({ limit: 100 })).toString("base64"),
          finality: "final",
        },
      }),
    });
    const data = await res.json();
    if (!data.result || !data.result.result) return null;
    const listings = JSON.parse(Buffer.from(data.result.result).toString());
    return (listings || []).find((l) => l.token_id === tokenId && l.active);
  } catch {
    return null;
  }
}

async function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

async function main() {
  console.log("========================================================================");
  console.log(" ROUNDFROG GENESIS COLLECTION — TESTNET MINTING & LISTING");
  console.log("========================================================================");
  console.log(`NFT Contract:         ${NFT_CONTRACT}`);
  console.log(`Marketplace Contract: ${MARKET_CONTRACT}`);
  console.log(`Signer & Seller:      ${SIGNER}`);
  console.log(`Items in Collection:  ${collection.length}`);
  console.log("------------------------------------------------------------------------");

  for (let i = 0; i < collection.length; i++) {
    const item = collection[i];
    console.log(`\n[${i + 1}/${collection.length}] Processing ${item.title} (${item.id})...`);

    const svgPath = path.join(collectionDir, item.file);
    const svgBytes = fs.readFileSync(svgPath);
    const mediaHash = crypto.createHash("sha256").update(svgBytes).digest("base64");
    const mediaUrl = `/collection/${item.file}`;

    const extraObj = {
      category: item.category,
      rarity: item.rarity,
      role: item.role,
      collection: "RoundFrog Genesis Collection",
      media_url: mediaUrl,
      attributes: [
        { trait_type: "Collection", value: "RoundFrog Genesis" },
        { trait_type: "Role", value: item.role },
        { trait_type: "Rarity", value: item.rarity },
      ],
    };

    // 1. Check if token already exists on-chain
    let existingToken = await queryToken(item.id);
    if (!existingToken) {
      console.log(`  -> Minting on-chain to ${SIGNER}...`);
      const mintArgs = JSON.stringify({
        token_id: item.id,
        token_owner_id: SIGNER,
        token_metadata: {
          title: item.title,
          description: item.description,
          media: mediaUrl,
          media_hash: mediaHash,
          copies: 1,
          extra: JSON.stringify(extraObj),
        },
      });

      const mintCmd = `${NEAR_CLI} contract call-function as-transaction "${NFT_CONTRACT}" nft_mint json-args '${mintArgs}' prepaid-gas '150.0 Tgas' attached-deposit '0.05 NEAR' sign-as "${SIGNER}" network-config testnet sign-with-keychain send`;
      execSync(mintCmd, { stdio: "inherit" });
      console.log(`  [+] Mint succeeded!`);
      await sleep(1500);

      // Verify
      existingToken = await queryToken(item.id);
      if (!existingToken) {
        throw new Error(`Minted token ${item.id} not found after transaction!`);
      }
    } else {
      console.log(`  [*] Token already exists on-chain (owner: ${existingToken.owner_id}).`);
    }

    // 2. Check if active listing already exists
    let existingListing = await queryListing(item.id);
    if (!existingListing) {
      console.log(`  -> Approving and listing on marketplace for ${Number(BigInt(item.priceYocto) / 10n ** 20n) / 10000} NEAR...`);
      const termsMsg = JSON.stringify({
        standard: "near_marketplace",
        version: "1.0.0",
        marketplace_id: MARKET_CONTRACT,
        nft_contract_id: NFT_CONTRACT,
        token_id: item.id,
        seller_id: SIGNER,
        price: item.priceYocto,
        fee_bps: 100,
        fee_account: OPERATOR,
        expires_at: null,
      });

      const approveArgs = JSON.stringify({
        token_id: item.id,
        account_id: MARKET_CONTRACT,
        msg: termsMsg,
      });

      const listCmd = `${NEAR_CLI} contract call-function as-transaction "${NFT_CONTRACT}" nft_approve json-args '${approveArgs}' prepaid-gas '150.0 Tgas' attached-deposit '0.01 NEAR' sign-as "${SIGNER}" network-config testnet sign-with-keychain send`;
      execSync(listCmd, { stdio: "inherit" });
      console.log(`  [+] Listing approved and activated on marketplace!`);
      await sleep(1500);

      existingListing = await queryListing(item.id);
      if (existingListing) {
        console.log(`  [✓] Verified active listing for ${item.id} at ${Number(BigInt(existingListing.price) / 10n ** 20n) / 10000} NEAR.`);
      }
    } else {
      console.log(`  [*] Listing already active on marketplace at ${Number(BigInt(existingListing.price) / 10n ** 20n) / 10000} NEAR.`);
    }
  }

  console.log("\n========================================================================");
  console.log(" [✓] ROUNDFROG GENESIS COLLECTION FULLY MINTED & LISTED ON TESTNET!");
  console.log("========================================================================");
}

main().catch((err) => {
  console.error("[-] ERROR:", err);
  process.exit(1);
});
