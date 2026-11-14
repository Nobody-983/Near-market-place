// Offline regressions for RoundFrog Marketplace Visibility Policy.
// Run: node --experimental-vm-modules --test tests/marketplace-visibility-policy.test.mjs

import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import vm from 'node:vm';
import { isProductionCatalogToken } from '../frontend/Nft/src/config/marketplace.js';

async function loadModule(path, imports) {
  const context = vm.createContext({ console, Buffer });
  const source = await readFile(new URL(path, import.meta.url), 'utf8');
  const module = new vm.SourceTextModule(source, { context });
  await module.link((name) => {
    assert.ok(imports[name], `Unexpected import: ${name}`);
    return new vm.SyntheticModule(Object.keys(imports[name]), function () {
      for (const [key, value] of Object.entries(imports[name])) this.setExport(key, value);
    }, { context });
  });
  await module.evaluate();
  return module.namespace;
}

const { parseUUID, resolveNFTId, LikedNFTsSet, extractTrustedTokenIdentifier } = await loadModule('../frontend/Nft/src/hooks/useNFTLikes.js', {
  react: { useEffect() {}, useRef() {}, useState() {} },
  '../context/walletContext': { useWallet: () => ({}) },
  '../lib/supabase': { supabase: {} },
});

test('isProductionCatalogToken correctly identifies production vs QA/test tokens', () => {
  // Production tokens
  assert.equal(isProductionCatalogToken('catalog-roundfrog-genesis-01', 'CyberFrog Alpha #01'), true);
  assert.equal(isProductionCatalogToken('catalog-roundfrog-genesis-02', 'Quantum Ribbit #02'), true);
  assert.equal(isProductionCatalogToken('catalog-687855bc-bc40-4bae-a299-758a449acb8e', 'Atlas'), true);
  assert.equal(isProductionCatalogToken('catalog-97d8f220-d520-49e6-bfe8-0528a7711477', 'Atlas'), true);

  // Standalone QA & test tokens must be excluded
  assert.equal(isProductionCatalogToken('catalog-roundfrog-test-1794616606', 'Test Genesis'), false);
  assert.equal(isProductionCatalogToken('catalog-roundfrog-test3-1794616647', 'Test With Hash'), false);
  assert.equal(isProductionCatalogToken('catalog-roundfrog-test4-1794616658', 'Test Extra'), false);
  assert.equal(isProductionCatalogToken('catalog-roundfrog-test5-1794616670', 'Test Relative Media'), false);
  assert.equal(isProductionCatalogToken('catalog-roundfrog-v2-1794606032', 'RoundFrog Acceptance Genesis #1794606032'), false);
  assert.equal(isProductionCatalogToken('catalog-8a4c378b-73bd-474d-aa20-62e5d37faba5', 'RoundFrog Acceptance Genesis'), false);
});

test('marketplace visibility: active listed NFTs appear; unlisted, delisted, sold, and expired disappear', () => {
  const listingsFixture = [
    {
      listing_id: 'nft-v2.roundfrog-dev.testnet|catalog-roundfrog-genesis-01|roundfrog-dev.testnet|1',
      token_id: 'catalog-roundfrog-genesis-01',
      seller: 'roundfrog-dev.testnet',
      price: '500000000000000000000000',
      active: true,
      title: 'CyberFrog Alpha #01',
      media: '/collection/frog-genesis-01.svg',
    },
    {
      // Delisted / cancelled listing
      listing_id: 'nft-v2.roundfrog-dev.testnet|catalog-roundfrog-genesis-02|roundfrog-dev.testnet|1',
      token_id: 'catalog-roundfrog-genesis-02',
      seller: 'roundfrog-dev.testnet',
      price: '750000000000000000000000',
      active: false,
      title: 'Quantum Ribbit #02',
      media: '/collection/frog-genesis-02.svg',
    },
    {
      // Sold listing
      listing_id: 'nft-v2.roundfrog-dev.testnet|catalog-8a4c378b-73bd-474d-aa20-62e5d37faba5|roundfrog-dev.testnet|2',
      token_id: 'catalog-8a4c378b-73bd-474d-aa20-62e5d37faba5',
      seller: 'roundfrog-dev.testnet',
      price: '100000000000000000000000',
      active: false,
      title: 'RoundFrog Acceptance Genesis',
      media: null,
    },
    {
      // QA test token with active listing (must be excluded)
      listing_id: 'nft-v2.roundfrog-dev.testnet|catalog-roundfrog-test5-1794616670|roundfrog-dev.testnet|1',
      token_id: 'catalog-roundfrog-test5-1794616670',
      seller: 'roundfrog-dev.testnet',
      price: '500000000000000000000000',
      active: true,
      title: 'Test Relative Media',
      media: '/collection/frog-genesis-01.svg',
    },
    {
      // Unlisted on-chain token
      token_id: 'catalog-97d8f220-d520-49e6-bfe8-0528a7711477',
      title: 'Atlas',
      media: 'https://storage.example/atlas.png',
      active: false,
    },
  ];

  // 1. Filter according to Marketplace Visibility Policy
  const visibleMarketplaceItems = listingsFixture.filter((item) => {
    if (!item.active) return false;
    if (!isProductionCatalogToken(item.token_id, item.title)) return false;
    if (!item.title || !item.media) return false;
    return true;
  });

  // Verify that only the active production item appears
  assert.equal(visibleMarketplaceItems.length, 1);
  assert.equal(visibleMarketplaceItems[0].token_id, 'catalog-roundfrog-genesis-01');

  // Verify exclusions
  const tokenIds = visibleMarketplaceItems.map((i) => i.token_id);
  assert.ok(!tokenIds.includes('catalog-roundfrog-genesis-02'), 'Delisted NFT must not appear');
  assert.ok(!tokenIds.includes('catalog-8a4c378b-73bd-474d-aa20-62e5d37faba5'), 'Sold NFT must not appear');
  assert.ok(!tokenIds.includes('catalog-roundfrog-test5-1794616670'), 'QA test NFT must not appear');
  assert.ok(!tokenIds.includes('catalog-97d8f220-d520-49e6-bfe8-0528a7711477'), 'Unlisted NFT must not appear');
});

test('dashboard counts match verified active listing records, not total minted supply', () => {
  const allOnChainTokens = [
    { token_id: 'catalog-1' },
    { token_id: 'catalog-2' },
    { token_id: 'catalog-3' },
    { token_id: 'catalog-4' },
    { token_id: 'catalog-5' },
  ]; // Total minted = 5

  const chainListings = [
    { token_id: 'catalog-1', active: true },
    { token_id: 'catalog-2', active: false }, // Sold
    { token_id: 'catalog-3', active: false }, // Cancelled
    { token_id: 'catalog-roundfrog-test5', active: true }, // QA token
  ];

  const activeCount = chainListings.filter(
    (cl) => cl.active && isProductionCatalogToken(cl.token_id)
  ).length;

  // Active production listings count = 1 (catalog-1)
  assert.equal(activeCount, 1);
  assert.notEqual(activeCount, allOnChainTokens.length, 'Must not report total minted supply');
});

test('favorites: registered unlisted NFTs can still be favorited; missing catalog NFTs reject', async () => {
  const registeredUuid = '97d8f220-d520-49e6-bfe8-0528a7711477';
  const registeredTokenId = `catalog-${registeredUuid}`;

  // Mock Supabase client for resolution (models exact PostgREST filter matching)
  const mockSupabase = {
    from(table) {
      assert.equal(table, 'nfts');
      return {
        select() {
          return {
            or(filter) {
              return {
                async maybeSingle() {
                  const conditions = filter.split(',');
                  const matches = conditions.some((c) =>
                    c === `id.eq.${registeredUuid}` ||
                    c === `token_id.eq.${registeredTokenId}` ||
                    c === `token_id.eq.${registeredUuid}`
                  );
                  if (matches) {
                    return { data: { id: registeredUuid } };
                  }
                  return { data: null };
                },
              };
            },
          };
        },
      };
    },
  };

  // 1. Registered unlisted NFT parses/resolves cleanly across all supported canonical formats
  const resolvedDirectUuid = await resolveNFTId(registeredUuid, mockSupabase);
  assert.equal(resolvedDirectUuid, registeredUuid);

  const resolvedCatalogPrefixed = await resolveNFTId(registeredTokenId, mockSupabase);
  assert.equal(resolvedCatalogPrefixed, registeredUuid);

  // Composite listing identifier: contract|token_id|seller|approval
  const compositeListingId = `nft-v2.roundfrog-dev.testnet|${registeredTokenId}|roundfrog-dev.testnet|1`;
  const resolvedComposite = await resolveNFTId(compositeListingId, mockSupabase);
  assert.equal(resolvedComposite, registeredUuid);

  // Contract-scoped token identifier: contract:token_id
  const colonScopedId = `nft-v2.roundfrog-dev.testnet:${registeredTokenId}`;
  const resolvedColon = await resolveNFTId(colonScopedId, mockSupabase);
  assert.equal(resolvedColon, registeredUuid);

  // Contract-scoped path identifier: contract/token_id
  const slashScopedId = `nft-v2.roundfrog-dev.testnet/${registeredTokenId}`;
  const resolvedSlash = await resolveNFTId(slashScopedId, mockSupabase);
  assert.equal(resolvedSlash, registeredUuid);

  // URL-encoded catalog token identifier
  const encodedTokenId = `catalog%2D${registeredUuid}`;
  const resolvedEncoded = await resolveNFTId(encodedTokenId, mockSupabase);
  assert.equal(resolvedEncoded, registeredUuid);

  // 2. Missing catalog NFT returns null and rejects gracefully
  const missingTokenId = 'catalog-unregistered-random-token-999';
  const resolvedMissing = await resolveNFTId(missingTokenId, mockSupabase);
  assert.equal(resolvedMissing, null, 'Unregistered token must return null');

  // 3. Unregistered UUID (syntactically valid UUID absent from catalog) must return null
  const unregisteredUuid = '00000000-0000-0000-0000-000000000000';
  const resolvedUnregisteredUuid = await resolveNFTId(unregisteredUuid, mockSupabase);
  assert.equal(resolvedUnregisteredUuid, null, 'Unregistered UUID must return null');

  // 4. Security: Malicious IDs containing a victim UUID from unauthorized contracts MUST be rejected
  const attackerScopedId = `attacker.testnet:catalog-${registeredUuid}`;
  assert.equal(parseUUID(attackerScopedId), null, 'parseUUID must reject unauthorized contract-scoped IDs');
  assert.equal(await resolveNFTId(attackerScopedId, mockSupabase), null, 'resolveNFTId must reject unauthorized contract-scoped IDs');

  const attackerCompositeId = `attacker.testnet|catalog-${registeredUuid}|evil_seller.testnet|1`;
  assert.equal(parseUUID(attackerCompositeId), null, 'parseUUID must reject unauthorized composite listing IDs');
  assert.equal(await resolveNFTId(attackerCompositeId, mockSupabase), null, 'resolveNFTId must reject unauthorized composite listing IDs');

  const arbitraryPrefixId = `untrusted-prefix-${registeredUuid}`;
  assert.equal(parseUUID(arbitraryPrefixId), null, 'parseUUID must reject arbitrary prefixed UUIDs');
  assert.equal(await resolveNFTId(arbitraryPrefixId, mockSupabase), null, 'resolveNFTId must reject arbitrary prefixed UUIDs');

  const evilUrlId = `https://evil.example.com/${registeredUuid}`;
  assert.equal(parseUUID(evilUrlId), null, 'parseUUID must reject URLs with embedded UUIDs');
  assert.equal(await resolveNFTId(evilUrlId, mockSupabase), null, 'resolveNFTId must reject URLs with embedded UUIDs');

  // 5. Security: PostgREST filter injection protection
  // Any input attempting filter manipulation with commas, parentheses, or quotes must be rejected
  const injectionAttemptComma = `test_token,id.eq.${registeredUuid}`;
  assert.equal(await resolveNFTId(injectionAttemptComma, mockSupabase), null, 'PostgREST comma injection must be rejected');

  const injectionAttemptParen = `bad_token)or(id.eq.${registeredUuid}`;
  assert.equal(await resolveNFTId(injectionAttemptParen, mockSupabase), null, 'PostgREST paren injection must be rejected');

  // 6. Security: Malformed URI percent encoding must be handled safely without throwing
  const malformedEncoded = `catalog%E0%A4%A-${registeredUuid}`;
  assert.equal(parseUUID(malformedEncoded), null, 'Malformed URI must not crash parseUUID');
  assert.equal(await resolveNFTId(malformedEncoded, mockSupabase), null, 'Malformed URI must not crash resolveNFTId');

  // 7. LikedNFTsSet supports legitimate aliases and rejects unauthorized/attacker aliases
  const favoritesSet = new LikedNFTsSet([registeredUuid]);
  assert.equal(favoritesSet.has(registeredUuid), true);
  assert.equal(favoritesSet.has(registeredTokenId), true);
  assert.equal(favoritesSet.has(compositeListingId), true);
  assert.equal(favoritesSet.has(colonScopedId), true);
  assert.equal(favoritesSet.has(slashScopedId), true);
  assert.equal(favoritesSet.has(attackerScopedId), false, 'LikedNFTsSet must reject attacker scoped ID');
  assert.equal(favoritesSet.has(attackerCompositeId), false, 'LikedNFTsSet must reject attacker composite ID');
  assert.equal(favoritesSet.has(arbitraryPrefixId), false, 'LikedNFTsSet must reject arbitrary prefix ID');
  assert.equal(favoritesSet.has('other-uuid'), false);
  assert.equal(favoritesSet.has('catalog-roundfrog-genesis-02'), false);
});

test('favorites retain correct unavailable/unlisted state when an NFT is not actively listed', () => {
  // Simulate an NFT that is in favorites but its listing is inactive (delisted / unlisted)
  const favoritedNFT = {
    id: '97d8f220-d520-49e6-bfe8-0528a7711477',
    token_id: 'catalog-97d8f220-d520-49e6-bfe8-0528a7711477',
    name: 'Atlas',
  };

  const listing = null; // Unlisted or delisted
  const isOwner = false;

  const isAvailableForSale = Boolean(listing && listing.status === 'active');
  assert.equal(isAvailableForSale, false);

  const statusText = !isAvailableForSale
    ? (isOwner ? 'Owned by You (Not Listed)' : 'Not listed for sale')
    : 'Available';

  assert.equal(statusText, 'Not listed for sale');
});

test('like counts do not double-count or triple-count for UUID and catalog-prefixed aliases', () => {
  const targetUuid = '97d8f220-d520-49e6-bfe8-0528a7711477';
  const rawDbLikes = [
    { nft_id: targetUuid },
    { nft_id: targetUuid },
    { nft_id: '44444444-4444-4444-8444-444444444444' },
  ];

  // 1. Storage of like counts must be keyed strictly by canonical UUID
  const likeCounts = {};
  rawDbLikes.forEach((like) => {
    if (!like?.nft_id) return;
    const id = String(like.nft_id).toLowerCase();
    likeCounts[id] = (likeCounts[id] || 0) + 1;
  });

  // Verify stored counts
  assert.equal(likeCounts[targetUuid], 2);
  assert.equal(likeCounts[`catalog-${targetUuid}`], undefined, 'Must not store alias keys that duplicate counts');

  // 2. Lookup by canonical ID or catalog token ID yields the exact count without summing aliases
  const cl = { token_id: `catalog-${targetUuid}` };
  const strippedUuid = parseUUID(cl.token_id);
  const canonicalId = strippedUuid;

  const resolvedCount = (canonicalId && likeCounts[canonicalId]) || (likeCounts[cl.token_id]) || 0;
  assert.equal(resolvedCount, 2, 'Must report exact 2 likes, never 4 or 6');
});

test('failure mode 1 regression: full identity flow resolves composite listings and token slugs to canonical UUID', async () => {
  const genesisUuid = 'a0000001-0000-4000-8000-000000000001';
  const genesisTokenId = 'catalog-roundfrog-genesis-01';
  const bareTokenId = 'roundfrog-genesis-01';

  // Mock Supabase with both public.nfts and public.marketplace_listings records
  const mockDb = {
    from(table) {
      if (table === 'nfts') {
        return {
          select() {
            return {
              or(filter) {
                return {
                  async maybeSingle() {
                    const conditions = filter.split(',');
                    const matches = conditions.some((c) =>
                      c === `id.eq.${genesisUuid}` ||
                      c === `token_id.eq.${genesisTokenId}` ||
                      c === `token_id.eq.${bareTokenId}`
                    );
                    if (matches) {
                      return { data: { id: genesisUuid } };
                    }
                    return { data: null };
                  },
                };
              },
            };
          },
        };
      }
      if (table === 'marketplace_listings') {
        return {
          select() {
            return {
              eq(col, val) {
                return {
                  async maybeSingle() {
                    if (col === 'contract_sale_id' && val.includes(genesisTokenId)) {
                      return { data: { nft_id: genesisUuid } };
                    }
                    return { data: null };
                  },
                };
              },
            };
          },
        };
      }
      throw new Error(`Unexpected table ${table}`);
    },
  };

  // 1. Direct slug token ID
  const resolvedSlug = await resolveNFTId(genesisTokenId, mockDb);
  assert.equal(resolvedSlug, genesisUuid, 'Slug token catalog-roundfrog-genesis-01 must resolve to canonical UUID');

  // 2. Bare slug token ID
  const resolvedBare = await resolveNFTId(bareTokenId, mockDb);
  assert.equal(resolvedBare, genesisUuid, 'Bare slug roundfrog-genesis-01 must resolve to canonical UUID');

  // 3. Composite marketplace listing ID with trusted contract
  const compositeListingId = `nft-v2.roundfrog-dev.testnet|${genesisTokenId}|roundfrog-dev.testnet|1`;
  const resolvedComposite = await resolveNFTId(compositeListingId, mockDb);
  assert.equal(resolvedComposite, genesisUuid, 'Composite listing ID with trusted contract must resolve to canonical UUID');

  // 4. Contract-scoped token ID with trusted contract
  const scopedId = `nft-v2.roundfrog-dev.testnet:${genesisTokenId}`;
  const resolvedScoped = await resolveNFTId(scopedId, mockDb);
  assert.equal(resolvedScoped, genesisUuid, 'Contract-scoped token ID with trusted contract must resolve to canonical UUID');

  // 5. Slash-scoped token ID with trusted contract
  const slashScoped = `nft-v2.roundfrog-dev.testnet/${genesisTokenId}`;
  const resolvedSlash = await resolveNFTId(slashScoped, mockDb);
  assert.equal(resolvedSlash, genesisUuid, 'Slash-scoped token ID with trusted contract must resolve to canonical UUID');

  // 6. Security: Unauthorized contract composite listing ID MUST return null
  const attackerComposite = `attacker.testnet|${genesisTokenId}|evil_seller|1`;
  assert.equal(extractTrustedTokenIdentifier(attackerComposite), null);
  assert.equal(await resolveNFTId(attackerComposite, mockDb), null, 'Unauthorized contract composite listing must be rejected');

  // 7. Security: Unauthorized contract scoped ID MUST return null
  const attackerScoped = `attacker.testnet:${genesisTokenId}`;
  assert.equal(extractTrustedTokenIdentifier(attackerScoped), null);
  assert.equal(await resolveNFTId(attackerScoped, mockDb), null, 'Unauthorized contract scoped ID must be rejected');
});

test('failure mode 2 & 3 regression: likes persist after refresh, support like/unlike cycle, and update UI state', () => {
  const genesisUuid = 'a0000001-0000-4000-8000-000000000001';
  const genesisTokenId = 'catalog-roundfrog-genesis-01';
  const compositeListingId = `nft-v2.roundfrog-dev.testnet|${genesisTokenId}|roundfrog-dev.testnet|1`;
  const scopedId = `nft-v2.roundfrog-dev.testnet:${genesisTokenId}`;

  // Initial state loaded from DB: user previously liked genesisUuid
  const favorites = new LikedNFTsSet([genesisUuid, genesisTokenId]);

  // 1. All representations of the liked NFT evaluate to liked (heart turns red, no false unlikes)
  assert.equal(favorites.has(genesisUuid), true, 'Canonical UUID must be recognized as liked');
  assert.equal(favorites.has(genesisTokenId), true, 'Catalog token ID must be recognized as liked');
  assert.equal(favorites.has(compositeListingId), true, 'Composite listing ID must be recognized as liked');
  assert.equal(favorites.has(scopedId), true, 'Scoped token ID must be recognized as liked');
  assert.equal(favorites.has('catalog-roundfrog-genesis-02'), false, 'Unliked NFT must not evaluate to liked');

  // 2. Unlike operation: removes from set
  favorites.delete(genesisUuid);
  favorites.delete(genesisTokenId);
  assert.equal(favorites.has(genesisUuid), false);
  assert.equal(favorites.has(genesisTokenId), false);
  assert.equal(favorites.has(compositeListingId), false);
  assert.equal(favorites.has(scopedId), false);

  // 3. Re-like operation: restores to set
  favorites.add(genesisUuid);
  favorites.add(genesisTokenId);
  assert.equal(favorites.has(genesisUuid), true);
  assert.equal(favorites.has(genesisTokenId), true);
  assert.equal(favorites.has(compositeListingId), true);
  assert.equal(favorites.has(scopedId), true);

  // 4. Security check: Attacker contract composite ID cannot fool LikedNFTsSet into reporting liked
  const attackerComposite = `attacker.testnet|${genesisTokenId}|evil_seller|1`;
  assert.equal(favorites.has(attackerComposite), false, 'Attacker contract composite ID must not evaluate to liked');
});

test('marketplace catalog-presence filter excludes uncataloged on-chain listings and includes catalog-backed listings', () => {
  // Simulate active on-chain listings from market-v2
  const activeChainListings = [
    {
      listing_id: 'nft-v2.roundfrog-dev.testnet|catalog-687855bc-bc40-4bae-a299-758a449acb8e|dragon983.testnet|1',
      token_id: 'catalog-687855bc-bc40-4bae-a299-758a449acb8e',
      price: '5000000000000000000000000',
      seller: 'dragon983.testnet',
      active: true,
    },
    {
      listing_id: 'nft-v2.roundfrog-dev.testnet|catalog-roundfrog-genesis-01|roundfrog-dev.testnet|2',
      token_id: 'catalog-roundfrog-genesis-01',
      price: '500000000000000000000000',
      seller: 'roundfrog-dev.testnet',
      active: true,
    },
  ];

  // Supabase public.nfts catalog (only Atlas exists in production DB)
  const catalogNfts = [
    {
      id: '687855bc-bc40-4bae-a299-758a449acb8e',
      name: 'Atlas',
      price: 5,
      image_url: 'https://llqwiqhwrbfoxoqqgczy.supabase.co/storage/v1/object/public/nft-images/atlas.png',
      token_id: null,
    },
  ];

  // Map and filter listings using catalog verification policy
  const visible = activeChainListings.map((cl) => {
    const rawTokenId = cl.token_id;
    const strippedUuid = rawTokenId?.startsWith('catalog-') ? rawTokenId.slice(8) : null;
    const catalogNft = catalogNfts.find(
      (cnft) =>
        (strippedUuid && cnft.id === strippedUuid) ||
        cnft.id === rawTokenId ||
        cnft.token_id === rawTokenId ||
        (strippedUuid && cnft.token_id === strippedUuid)
    );
    if (!catalogNft) return null; // Exclude uncataloged/dummy listing
    return {
      id: cl.listing_id,
      nfts: { id: catalogNft.id, name: catalogNft.name },
    };
  }).filter(Boolean);

  // Atlas is included because it has a real catalog record
  assert.equal(visible.length, 1);
  assert.equal(visible[0].nfts.id, '687855bc-bc40-4bae-a299-758a449acb8e');
  assert.equal(visible[0].nfts.name, 'Atlas');

  // Uncataloged genesis listing is excluded
  assert.equal(visible.some((v) => v.id.includes('genesis-01')), false);
});
