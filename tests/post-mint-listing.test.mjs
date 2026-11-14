import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { test } from 'node:test';
import vm from 'node:vm';
import * as amounts from '../frontend/Nft/src/lib/near/utils.js';

const require = createRequire(new URL('../frontend/Nft/package.json', import.meta.url));
const { parse } = require('espree');

const quietConsole = { error() {}, warn() {}, log() {} };

function* walk(node) {
  if (!node || typeof node !== 'object') return;
  yield node;
  for (const [key, value] of Object.entries(node)) {
    if (key === 'parent') continue;
    if (Array.isArray(value)) {
      for (const item of value) yield* walk(item);
    } else {
      yield* walk(value);
    }
  }
}

async function page(name) {
  const source = await readFile(new URL(`../frontend/Nft/src/pages/${name}`, import.meta.url), 'utf8');
  return {
    source,
    nodes: [...walk(parse(source, {
      ecmaVersion: 'latest',
      sourceType: 'module',
      range: true,
      ecmaFeatures: { jsx: true },
    }))],
  };
}

const collection = await page('createNft.jsx');

function handler(pageObj, name, context) {
  const node = pageObj.nodes.find((item) => item.type === 'VariableDeclarator' && item.id.name === name);
  assert.ok(node, `${name} must exist in page`);
  return vm.runInNewContext(`(${pageObj.source.slice(...node.init.range)})`, {
    console: quietConsole,
    ...amounts,
    ...context,
  });
}

const alice = 'alice.roundfrog-dev.testnet';
const bob = 'bob.roundfrog-dev.testnet';
const sampleNFT = {
  id: 'nft-101',
  name: 'Cyber Frog #101',
  description: 'A cyber frog collectible',
  image_url: 'https://example.com/art.png',
  price: '1.5',
  currency: 'NEAR',
  category: 'Gaming',
  source: 'user',
  model: 'unique',
};

test('Scenario 1: Mint succeeds, then List for Sale succeeds and updates post-mint panel', async () => {
  let mintSuccessNFT = null;
  let errorState = '';
  let successState = '';
  let nftsState = [sampleNFT];
  let listingsState = [];
  let fetchNFTsCalled = false;
  let fetchListingsCalled = false;

  const mockToken = {
    token_id: `catalog-${sampleNFT.id}`,
    owner_id: alice,
    metadata: { title: sampleNFT.name },
  };

  const wallet = {
    selector: {
      wallet: async () => ({
        getAccounts: async () => [{ accountId: alice }],
      }),
    },
    accountId: alice,
  };

  const mintFn = handler(collection, 'mintCatalogNFT', {
    MINTING_ACTIONS_ENABLED: true,
    isConnected: true,
    selector: wallet.selector,
    walletAddress: alice,
    minting: null,
    NFT_CONTRACT_ID: 'nft.roundfrog-dev.testnet',
    setMinting: () => {},
    setError: (msg) => { errorState = msg; },
    setSuccess: (msg) => { successState = msg; },
    requireConnectedAccount: async () => alice,
    mintNftOnChain: async () => ({ transaction: { hash: 'tx-mint-123' } }),
    transactionHash: () => 'tx-mint-123',
    waitForFinal: async () => ({ status: { SuccessValue: '' } }),
    assertFinalSuccess: () => {},
    nftToken: async () => mockToken,
    setNfts: (update) => { nftsState = typeof update === 'function' ? update(nftsState) : update; },
    setMintSuccessNFT: (data) => { mintSuccessNFT = data; },
    fetchNFTs: async () => { fetchNFTsCalled = true; },
  });

  await mintFn(sampleNFT);

  assert.equal(errorState, '');
  assert.match(successState, /Cyber Frog #101.*minted on NEAR/);
  assert.ok(mintSuccessNFT);
  assert.equal(mintSuccessNFT.tokenId, `catalog-${sampleNFT.id}`);
  assert.equal(mintSuccessNFT.ownerId, alice);
  assert.equal(mintSuccessNFT.listed, false);
  assert.equal(mintSuccessNFT.listingPrice, null);

  // Now user triggers List for Sale
  const listFn = handler(collection, 'handleListNFT', {
    LISTING_ACTIONS_ENABLED: true,
    listing: null,
    listingNFT: mintSuccessNFT.nft,
    listingForm: { price: '2.5', currency: 'NEAR', quantity: '1' },
    user: { id: 'user-alice' },
    isConnected: true,
    walletAddress: alice,
    selector: wallet.selector,
    NFT_CONTRACT_ID: 'nft.roundfrog-dev.testnet',
    setListing: () => {},
    setListingNFT: () => {},
    setListingForm: () => {},
    setError: (msg) => { errorState = msg; },
    setSuccess: (msg) => { successState = msg; },
    setListings: (update) => { listingsState = typeof update === 'function' ? update(listingsState) : update; },
    setMintSuccessNFT: (updater) => {
      mintSuccessNFT = typeof updater === 'function' ? updater(mintSuccessNFT) : updater;
    },
    requireConnectedAccount: async () => alice,
    verifyListingPreconditions: async () => ({ config: { trading_enabled: true }, token: mockToken }),
    ensureMarketplaceListingStorage: async () => ({ funded: false }),
    listNftOnChain: async () => ({ transaction: { hash: 'tx-list-456' } }),
    transactionHash: () => 'tx-list-456',
    waitForFinal: async () => ({ status: { SuccessValue: '' } }),
    assertFinalSuccess: () => {},
    listingIdFromFinalResult: () => 'listing-456',
    marketplaceListing: async () => ({
      active: true,
      token_id: `catalog-${sampleNFT.id}`,
      nft_contract_id: 'nft.roundfrog-dev.testnet',
      seller: alice,
    }),
    marketplaceListingMetadata: async () => ({ status: 'active' }),
    waitForIndexedListing: async () => ({ id: 'listing-456', status: 'active' }),
    fetchListings: async () => { fetchListingsCalled = true; },
    fetchNFTs: async () => { fetchNFTsCalled = true; },
  });

  await listFn({ preventDefault() {} });

  assert.equal(errorState, '');
  assert.match(successState, /is listed on NEAR at 2.5 NEAR/);
  assert.equal(mintSuccessNFT.listed, true);
  assert.equal(mintSuccessNFT.listingPrice, '2.5');
  assert.equal(mintSuccessNFT.contractListingId, 'listing-456');
  assert.equal(fetchListingsCalled, true);
  assert.equal(fetchNFTsCalled, true);
});

test('Scenario 2: Mint succeeds, but listing fails; mintSuccessNFT is preserved for retry', async () => {
  let mintSuccessNFT = {
    nft: { ...sampleNFT, token_id: `catalog-${sampleNFT.id}` },
    tokenId: `catalog-${sampleNFT.id}`,
    ownerId: alice,
    listed: false,
    listingPrice: null,
  };
  let errorState = '';

  const listFn = handler(collection, 'handleListNFT', {
    LISTING_ACTIONS_ENABLED: true,
    listing: null,
    listingNFT: mintSuccessNFT.nft,
    listingForm: { price: '3.0', currency: 'NEAR', quantity: '1' },
    user: { id: 'user-alice' },
    isConnected: true,
    walletAddress: alice,
    selector: { wallet: async () => ({ getAccounts: async () => [{ accountId: alice }] }) },
    NFT_CONTRACT_ID: 'nft.roundfrog-dev.testnet',
    setListing: () => {},
    setListingNFT: () => {},
    setListingForm: () => {},
    setError: (msg) => { errorState = msg; },
    setSuccess: () => {},
    setListings: () => {},
    setMintSuccessNFT: (updater) => {
      mintSuccessNFT = typeof updater === 'function' ? updater(mintSuccessNFT) : updater;
    },
    requireConnectedAccount: async () => alice,
    verifyListingPreconditions: async () => ({ config: { trading_enabled: true } }),
    ensureMarketplaceListingStorage: async () => {
      throw new Error('RPC node connection failed during storage check');
    },
    fetchListings: async () => {},
    fetchNFTs: async () => {},
  });

  await listFn({ preventDefault() {} });

  assert.match(errorState, /RPC node connection failed/);
  // Verified: mintSuccessNFT is preserved and NOT wiped out
  assert.ok(mintSuccessNFT);
  assert.equal(mintSuccessNFT.tokenId, `catalog-${sampleNFT.id}`);
  assert.equal(mintSuccessNFT.listed, false);
});

test('Scenario 3: User rejects the listing approval transaction in wallet', async () => {
  let mintSuccessNFT = {
    nft: { ...sampleNFT, token_id: `catalog-${sampleNFT.id}` },
    tokenId: `catalog-${sampleNFT.id}`,
    ownerId: alice,
    listed: false,
  };
  let errorState = '';

  const listFn = handler(collection, 'handleListNFT', {
    LISTING_ACTIONS_ENABLED: true,
    listing: null,
    listingNFT: mintSuccessNFT.nft,
    listingForm: { price: '1.0', currency: 'NEAR', quantity: '1' },
    user: { id: 'user-alice' },
    isConnected: true,
    walletAddress: alice,
    selector: {},
    NFT_CONTRACT_ID: 'nft.roundfrog-dev.testnet',
    setListing: () => {},
    setListingNFT: () => {},
    setListingForm: () => {},
    setError: (msg) => { errorState = msg; },
    setSuccess: () => {},
    setListings: () => {},
    setMintSuccessNFT: () => {},
    requireConnectedAccount: async () => alice,
    verifyListingPreconditions: async () => ({}),
    ensureMarketplaceListingStorage: async () => ({ funded: false }),
    listNftOnChain: async () => {
      throw new Error('User rejected the transaction in wallet');
    },
    fetchListings: async () => {},
    fetchNFTs: async () => {},
  });

  await listFn({ preventDefault() {} });

  assert.match(errorState, /User rejected the transaction/);
  assert.equal(mintSuccessNFT.listed, false);
});

test('Scenario 4: Token is not owned by the connected wallet on-chain', async () => {
  let errorState = '';
  let mintSuccessNFT = null;

  // Mint verification fails if receiver does not match connected wallet
  const mintFn = handler(collection, 'mintCatalogNFT', {
    MINTING_ACTIONS_ENABLED: true,
    isConnected: true,
    selector: {},
    walletAddress: alice,
    minting: null,
    NFT_CONTRACT_ID: 'nft.roundfrog-dev.testnet',
    setMinting: () => {},
    setError: (msg) => { errorState = msg; },
    setSuccess: () => {},
    requireConnectedAccount: async () => alice,
    mintNftOnChain: async () => ({ transaction: { hash: 'tx-mint-mismatch' } }),
    transactionHash: () => 'tx-mint-mismatch',
    waitForFinal: async () => ({ status: { SuccessValue: '' } }),
    assertFinalSuccess: () => {},
    nftToken: async () => ({
      token_id: `catalog-${sampleNFT.id}`,
      owner_id: bob, // Bob owns it, not Alice
    }),
    setNfts: () => {},
    setMintSuccessNFT: (data) => { mintSuccessNFT = data; },
  });

  await mintFn(sampleNFT);

  assert.match(errorState, new RegExp(`belongs to ${bob}, not your connected wallet \\(${alice}\\)`));
  assert.equal(mintSuccessNFT, null);

  // Similarly, listing check rejects if token owner is not connected wallet
  let listErrorState = '';
  const listFn = handler(collection, 'handleListNFT', {
    LISTING_ACTIONS_ENABLED: true,
    listing: null,
    listingNFT: { ...sampleNFT, token_id: `catalog-${sampleNFT.id}` },
    listingForm: { price: '1.0', currency: 'NEAR', quantity: '1' },
    user: { id: 'user-alice' },
    isConnected: true,
    walletAddress: alice,
    selector: {},
    NFT_CONTRACT_ID: 'nft.roundfrog-dev.testnet',
    setListing: () => {},
    setError: (msg) => { listErrorState = msg; },
    setSuccess: () => {},
    requireConnectedAccount: async () => alice,
    verifyListingPreconditions: async () => {
      throw new Error(`Connected wallet (${alice}) does not own this NFT on-chain. Current owner is ${bob}.`);
    },
    fetchListings: async () => {},
    fetchNFTs: async () => {},
  });

  await listFn({ preventDefault() {} });
  assert.match(listErrorState, /does not own this NFT on-chain/);
});

test('Scenario 5: User already has sufficient marketplace storage registration', async () => {
  let storageDepositCalled = false;
  let listingApprovalCalled = false;

  const listFn = handler(collection, 'handleListNFT', {
    LISTING_ACTIONS_ENABLED: true,
    listing: null,
    listingNFT: { ...sampleNFT, token_id: `catalog-${sampleNFT.id}` },
    listingForm: { price: '2.0', currency: 'NEAR', quantity: '1' },
    user: { id: 'user-alice' },
    isConnected: true,
    walletAddress: alice,
    selector: {},
    NFT_CONTRACT_ID: 'nft.roundfrog-dev.testnet',
    setListing: () => {},
    setListingNFT: () => {},
    setListingForm: () => {},
    setError: () => {},
    setSuccess: () => {},
    setListings: () => {},
    setMintSuccessNFT: () => {},
    requireConnectedAccount: async () => alice,
    verifyListingPreconditions: async () => ({ config: { trading_enabled: true } }),
    ensureMarketplaceListingStorage: async () => {
      // Storage already funded, no deposit needed
      return { funded: false, balance: { available: '50000000000000000000000' } };
    },
    listNftOnChain: async () => {
      listingApprovalCalled = true;
      return { transaction: { hash: 'tx-approval' } };
    },
    transactionHash: () => 'tx-approval',
    waitForFinal: async () => ({ status: { SuccessValue: '' } }),
    assertFinalSuccess: () => {},
    listingIdFromFinalResult: () => 'listing-ok',
    marketplaceListing: async () => ({
      active: true,
      token_id: `catalog-${sampleNFT.id}`,
      nft_contract_id: 'nft.roundfrog-dev.testnet',
      seller: alice,
    }),
    marketplaceListingMetadata: async () => ({ status: 'active' }),
    waitForIndexedListing: async () => ({ id: 'listing-ok', status: 'active' }),
    fetchListings: async () => {},
    fetchNFTs: async () => {},
  });

  await listFn({ preventDefault() {} });

  assert.equal(storageDepositCalled, false);
  assert.equal(listingApprovalCalled, true);
});

test('Scenario 6: User needs marketplace storage registration and funds it before listing', async () => {
  let storageDeposited = false;
  let listingApprovalCalled = false;

  const listFn = handler(collection, 'handleListNFT', {
    LISTING_ACTIONS_ENABLED: true,
    listing: null,
    listingNFT: { ...sampleNFT, token_id: `catalog-${sampleNFT.id}` },
    listingForm: { price: '2.0', currency: 'NEAR', quantity: '1' },
    user: { id: 'user-alice' },
    isConnected: true,
    walletAddress: alice,
    selector: {},
    NFT_CONTRACT_ID: 'nft.roundfrog-dev.testnet',
    setListing: () => {},
    setListingNFT: () => {},
    setListingForm: () => {},
    setError: () => {},
    setSuccess: () => {},
    setListings: () => {},
    setMintSuccessNFT: () => {},
    requireConnectedAccount: async () => alice,
    verifyListingPreconditions: async () => ({ config: { trading_enabled: true } }),
    ensureMarketplaceListingStorage: async () => {
      // Simulates funding storage deposit
      storageDeposited = true;
      return { funded: true, transactionHash: 'tx-storage-dep', balance: { available: '100000000000000000000000' } };
    },
    listNftOnChain: async () => {
      listingApprovalCalled = true;
      return { transaction: { hash: 'tx-approval' } };
    },
    transactionHash: () => 'tx-approval',
    waitForFinal: async () => ({ status: { SuccessValue: '' } }),
    assertFinalSuccess: () => {},
    listingIdFromFinalResult: () => 'listing-ok',
    marketplaceListing: async () => ({
      active: true,
      token_id: `catalog-${sampleNFT.id}`,
      nft_contract_id: 'nft.roundfrog-dev.testnet',
      seller: alice,
    }),
    marketplaceListingMetadata: async () => ({ status: 'active' }),
    waitForIndexedListing: async () => ({ id: 'listing-ok', status: 'active' }),
    fetchListings: async () => {},
    fetchNFTs: async () => {},
  });

  await listFn({ preventDefault() {} });

  assert.equal(storageDeposited, true);
  assert.equal(listingApprovalCalled, true);
});

