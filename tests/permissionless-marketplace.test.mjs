import assert from 'node:assert/strict';
import { test } from 'node:test';
import vm from 'node:vm';
import * as amounts from '../frontend/Nft/src/lib/near/utils.js';

const quietConsole = { error() {}, warn() {}, log() {} };

async function loadContractModule(envOverrides = {}) {
  const defaultNear = {
    NETWORK_ID: 'testnet',
    NFT_CONTRACT_ID: 'nft-v2.roundfrog-dev.testnet',
    MARKETPLACE_CONTRACT_ID: 'market-v2.roundfrog-dev.testnet',
    REVIEWED_NFT_CONTRACT_ID: 'nft-v2.roundfrog-dev.testnet',
    REVIEWED_MARKETPLACE_CONTRACT_ID: 'market-v2.roundfrog-dev.testnet',
    REVIEWED_NFT_CODE_HASH: '7Xwtpb4JsXxmcdgCNEJdQVnAUQqYjWYFAvbtK7vXexh7',
    REVIEWED_MARKETPLACE_CODE_HASH: '2vpgmXoZSpnPgcAkE6hR4dy9EnjogNLcW2vbkECADM2y',
    GAS: '300000000000000',
    DEPOSIT_STORAGE: '1250000000000000000000',
    ...envOverrides,
  };

  const context = vm.createContext({
    console: quietConsole,
    crypto: globalThis.crypto,
    Buffer: globalThis.Buffer,
    setTimeout,
    clearTimeout,
  });

  const url = new URL('../frontend/Nft/src/lib/near/contract.js', import.meta.url);
  const source = await (await import('node:fs/promises')).readFile(url, 'utf8');

  const module = new vm.SourceTextModule(source, {
    context,
    initializeImportMeta: (meta) => { meta.url = url.href; },
    importModuleDynamically: async (specifier) => {
      throw new Error(`Dynamic import ${specifier} not supported`);
    },
  });

  await module.link(async (specifier) => {
    let exportsObj = {};
    if (specifier.includes('utils')) {
      exportsObj = amounts;
    } else if (specifier.includes('config/near')) {
      exportsObj = defaultNear;
    } else if (specifier.includes('config/marketplace')) {
      exportsObj = {
        LISTING_STORAGE_TARGET_YOCTO: '120000000000000000000000',
      };
    }
    return new vm.SyntheticModule(Object.keys(exportsObj), function () {
      for (const [k, v] of Object.entries(exportsObj)) this.setExport(k, v);
    }, { context });
  });

  await module.evaluate();
  return module.namespace;
}

const alice = 'alice.roundfrog-dev.testnet';
const bob = 'bob.roundfrog-dev.testnet';
const charlie = 'charlie.roundfrog-dev.testnet';

function createMockWallet(activeAccountId, contractState = {}) {
  const sentCalls = [];
  const state = {
    tokens: {}, // tokenId -> { owner_id, token_id, metadata }
    storage: {}, // accountId -> available yocto
    listings: {}, // listingId -> listing
    ever_minted: {}, // permanent ever_minted registry
    ...contractState,
  };

  const provider = {
    query: async (params) => {
      if (params.request_type === 'view_account') {
        const isNft = params.account_id.includes('nft');
        return {
          code_hash: isNft
            ? '7Xwtpb4JsXxmcdgCNEJdQVnAUQqYjWYFAvbtK7vXexh7'
            : '2vpgmXoZSpnPgcAkE6hR4dy9EnjogNLcW2vbkECADM2y',
        };
      }
      if (params.request_type === 'call_function') {
        const methodName = params.method_name;
        const args = params.args_base64
          ? JSON.parse(Buffer.from(params.args_base64, 'base64').toString('utf8'))
          : {};

        if (methodName === 'marketplace_id') {
          return { result: [...Buffer.from(JSON.stringify('market-v2.roundfrog-dev.testnet'))] };
        }
        if (methodName === 'testnet_mint_authority') {
          return { result: [...Buffer.from(JSON.stringify('roundfrog-dev.testnet'))] };
        }
        if (methodName === 'token_id_status') {
          const status = state.ever_minted[args.token_id] || (state.tokens[args.token_id] ? 'Live' : null);
          return { result: [...Buffer.from(JSON.stringify(status))] };
        }
        if (methodName === 'config') {
          return {
            result: [...Buffer.from(JSON.stringify({
              owner: 'operator.roundfrog-dev.testnet',
              fee_bps: 100,
              fee_account: 'operator.roundfrog-dev.testnet',
              primary_nft_contract_id: 'nft-v2.roundfrog-dev.testnet',
              nft_contract_id: 'nft-v2.roundfrog-dev.testnet',
              allowed_nft_contracts: ['nft-v2.roundfrog-dev.testnet'],
              trading_enabled: true,
              state_version: 2,
            }))],
          };
        }
        if (methodName === 'nft_token') {
          const tok = state.tokens[args.token_id] || null;
          return { result: [...Buffer.from(JSON.stringify(tok))] };
        }
        if (methodName === 'storage_balance_of') {
          const avail = state.storage[args.account_id] || '150000000000000000000000';
          return { result: [...Buffer.from(JSON.stringify({ total: avail, available: avail }))] };
        }
        if (methodName === 'storage_balance_bounds') {
          return { result: [...Buffer.from(JSON.stringify({ min: '1250000000000000000000', max: null }))] };
        }
        if (methodName === 'listing') {
          const l = state.listings[args.listing_id] || null;
          return { result: [...Buffer.from(JSON.stringify(l))] };
        }
      }
      return { result: [] };
    },
  };

  const walletInstance = {
    signAndSendTransaction: async ({ receiverId, actions }) => {
      const action = actions[0]?.params;
      sentCalls.push({ receiverId, action });
      const methodName = action?.methodName;
      const args = action?.args || {};

      if (methodName === 'nft_mint') {
        const tokenId = args.token_id;
        if (state.ever_minted[tokenId]) {
          throw new Error('Smart contract panicked: token_id must be unique and never reused');
        }
        state.ever_minted[tokenId] = 'Live';
        const ownerId = args.token_owner_id;
        state.tokens[tokenId] = { token_id: tokenId, owner_id: ownerId, metadata: args.token_metadata };
        return {
          transaction: { hash: `tx-mint-${tokenId}` },
          status: { SuccessValue: Buffer.from(JSON.stringify(state.tokens[tokenId])).toString('base64') },
        };
      }

      if (methodName === 'nft_approve') {
        const tokenId = args.token_id;
        const terms = JSON.parse(args.msg);
        const listingId = `${receiverId}|${tokenId}|${terms.seller_id}|1`;
        state.listings[listingId] = {
          nft_contract_id: receiverId,
          token_id: tokenId,
          seller: terms.seller_id,
          approval_id: '1',
          price: terms.price,
          fee_bps: 100,
          active: true,
        };
        return {
          transaction: { hash: `tx-approve-${tokenId}` },
          status: { SuccessValue: Buffer.from(JSON.stringify(listingId)).toString('base64') },
        };
      }

      if (methodName === 'cancel_listing') {
        const listing = state.listings[args.listing_id];
        if (!listing) throw new Error('Listing not found');
        if (activeAccountId !== listing.seller) {
          throw new Error('Only the seller may cancel');
        }
        listing.active = false;
        return {
          transaction: { hash: `tx-cancel-${args.listing_id}` },
          status: { SuccessValue: '' },
        };
      }

      if (methodName === 'buy_nft') {
        const listing = state.listings[args.listing_id];
        if (!listing || !listing.active) throw new Error('Listing is not active');
        if (activeAccountId === listing.seller) throw new Error('Seller cannot buy own NFT');
        listing.active = false;
        state.tokens[listing.token_id].owner_id = activeAccountId;
        return {
          transaction: { hash: `tx-buy-${args.listing_id}` },
          status: { SuccessValue: '' },
        };
      }

      return { transaction: { hash: 'tx-unknown' }, status: { SuccessValue: '' } };
    },
  };

  return {
    accountId: activeAccountId,
    provider,
    selector: {
      options: { provider },
      store: {
        getState: () => ({
          accounts: [{ accountId: activeAccountId, active: true }],
        }),
      },
      wallet: async () => ({
        getAccounts: async () => [{ accountId: activeAccountId }],
        signAndSendTransaction: walletInstance.signAndSendTransaction,
      }),
    },
    state,
    sentCalls,
  };
}

test('1 & 2. Permissionless minting: Wallets A, B, and C can independently mint', async () => {
  const contract = await loadContractModule();
  const sharedState = { tokens: {}, storage: {}, listings: {} };

  // Wallet A (Alice) mints
  const walletA = createMockWallet(alice, sharedState);
  const mintResultA = await contract.mintNftOnChain({
    tokenId: 'token-a-1',
    metadata: { title: 'Alice Token' },
    wallet: walletA,
  });
  assert.ok(mintResultA.transaction.hash.includes('token-a-1'));
  assert.equal(sharedState.tokens['token-a-1'].owner_id, alice);

  // Wallet B (Bob) mints
  const walletB = createMockWallet(bob, sharedState);
  const mintResultB = await contract.mintNftOnChain({
    tokenId: 'token-b-1',
    metadata: { title: 'Bob Token' },
    wallet: walletB,
  });
  assert.ok(mintResultB.transaction.hash.includes('token-b-1'));
  assert.equal(sharedState.tokens['token-b-1'].owner_id, bob);

  // Wallet C (Charlie) mints
  const walletC = createMockWallet(charlie, sharedState);
  const mintResultC = await contract.mintNftOnChain({
    tokenId: 'token-c-1',
    metadata: { title: 'Charlie Token' },
    wallet: walletC,
  });
  assert.ok(mintResultC.transaction.hash.includes('token-c-1'));
  assert.equal(sharedState.tokens['token-c-1'].owner_id, charlie);
});

test('3. Each wallet independently owns and views its own distinct NFT', async () => {
  const contract = await loadContractModule();
  const sharedState = {
    tokens: {
      'token-a-1': { token_id: 'token-a-1', owner_id: alice },
      'token-b-1': { token_id: 'token-b-1', owner_id: bob },
      'token-c-1': { token_id: 'token-c-1', owner_id: charlie },
    },
    storage: {},
    listings: {},
  };

  const walletA = createMockWallet(alice, sharedState);
  const tokenA = await contract.nftToken('token-a-1', walletA, 'nft-v2.roundfrog-dev.testnet');
  assert.equal(tokenA.owner_id, alice);

  const walletB = createMockWallet(bob, sharedState);
  const tokenB = await contract.nftToken('token-b-1', walletB, 'nft-v2.roundfrog-dev.testnet');
  assert.equal(tokenB.owner_id, bob);

  const walletC = createMockWallet(charlie, sharedState);
  const tokenC = await contract.nftToken('token-c-1', walletC, 'nft-v2.roundfrog-dev.testnet');
  assert.equal(tokenC.owner_id, charlie);

  assert.notEqual(tokenA.owner_id, tokenB.owner_id);
  assert.notEqual(tokenB.owner_id, tokenC.owner_id);
});

test("4. Wallet A cannot list Wallet B's NFT without authorization", async () => {
  const contract = await loadContractModule();
  const sharedState = {
    tokens: {
      'token-b-1': { token_id: 'token-b-1', owner_id: bob },
    },
    storage: { [alice]: '150000000000000000000000' },
    listings: {},
  };

  const walletA = createMockWallet(alice, sharedState);
  await assert.rejects(
    contract.verifyListingPreconditions({
      nftContractId: 'nft-v2.roundfrog-dev.testnet',
      tokenId: 'token-b-1',
      sellerId: alice,
      wallet: walletA,
    }),
    /Connected wallet is not the current on-chain NFT owner/
  );
});

test("5. Wallet A can list its own NFT", async () => {
  const contract = await loadContractModule();
  const sharedState = {
    tokens: {
      'token-a-1': { token_id: 'token-a-1', owner_id: alice },
    },
    storage: { [alice]: '150000000000000000000000' },
    listings: {},
  };

  const walletA = createMockWallet(alice, sharedState);
  const approveRes = await contract.listNftOnChain({
    nftContractId: 'nft-v2.roundfrog-dev.testnet',
    tokenId: 'token-a-1',
    priceNear: '0.5',
    wallet: walletA,
  });
  assert.ok(approveRes.transaction.hash.includes('token-a-1'));

  // The listing is now active in state
  const listingId = Object.keys(sharedState.listings)[0];
  assert.ok(listingId);
  assert.equal(sharedState.listings[listingId].seller, alice);
  assert.equal(sharedState.listings[listingId].active, true);
});

test("6. Wallet B can buy Wallet A's NFT and ownership transfers", async () => {
  const contract = await loadContractModule();
  const listingId = 'nft-v2.roundfrog-dev.testnet|token-a-1|alice.roundfrog-dev.testnet|1';
  const sharedState = {
    tokens: {
      'token-a-1': { token_id: 'token-a-1', owner_id: alice },
    },
    storage: {},
    listings: {
      [listingId]: {
        nft_contract_id: 'nft-v2.roundfrog-dev.testnet',
        token_id: 'token-a-1',
        seller: alice,
        price: '500000000000000000000000',
        active: true,
      },
    },
  };

  const walletB = createMockWallet(bob, sharedState);
  const buyRes = await contract.buyNftOnChain(listingId, '500000000000000000000000', walletB);
  assert.ok(buyRes.transaction.hash.includes('buy'));
  assert.equal(sharedState.tokens['token-a-1'].owner_id, bob);
  assert.equal(sharedState.listings[listingId].active, false);
});

test("7. Unauthorized cancellation is rejected", async () => {
  const contract = await loadContractModule();
  const listingId = 'nft-v2.roundfrog-dev.testnet|token-a-1|alice.roundfrog-dev.testnet|1';
  const sharedState = {
    tokens: { 'token-a-1': { token_id: 'token-a-1', owner_id: alice } },
    storage: {},
    listings: {
      [listingId]: {
        nft_contract_id: 'nft-v2.roundfrog-dev.testnet',
        token_id: 'token-a-1',
        seller: alice,
        price: '500000000000000000000000',
        active: true,
      },
    },
  };

  const walletB = createMockWallet(bob, sharedState);
  await assert.rejects(
    contract.cancelListingOnChain(listingId, walletB),
    /Only the seller may cancel/
  );
  assert.equal(sharedState.listings[listingId].active, true);
});

test("8. Seller cancellation works", async () => {
  const contract = await loadContractModule();
  const listingId = 'nft-v2.roundfrog-dev.testnet|token-a-1|alice.roundfrog-dev.testnet|1';
  const sharedState = {
    tokens: { 'token-a-1': { token_id: 'token-a-1', owner_id: alice } },
    storage: {},
    listings: {
      [listingId]: {
        nft_contract_id: 'nft-v2.roundfrog-dev.testnet',
        token_id: 'token-a-1',
        seller: alice,
        price: '500000000000000000000000',
        active: true,
      },
    },
  };

  const walletA = createMockWallet(alice, sharedState);
  const cancelRes = await contract.cancelListingOnChain(listingId, walletA);
  assert.ok(cancelRes.transaction.hash.includes('cancel'));
  assert.equal(sharedState.listings[listingId].active, false);
});

test("9. Failed transactions do not create false frontend success", async () => {
  const contract = await loadContractModule();
  const rejectingWallet = {
    accountId: alice,
    selector: {
      options: {
        provider: {
          query: async (p) => {
            if (p.request_type === 'view_account') {
              const isNft = p.account_id.includes('nft');
              return {
                code_hash: isNft
                  ? '7Xwtpb4JsXxmcdgCNEJdQVnAUQqYjWYFAvbtK7vXexh7'
                  : '2vpgmXoZSpnPgcAkE6hR4dy9EnjogNLcW2vbkECADM2y',
              };
            }
            if (p.method_name === 'marketplace_id') {
              return { result: [...Buffer.from(JSON.stringify('market-v2.roundfrog-dev.testnet'))] };
            }
            if (p.method_name === 'testnet_mint_authority') {
              return { result: [...Buffer.from(JSON.stringify('roundfrog-dev.testnet'))] };
            }
            return { result: [] };
          },
        },
      },
      store: { getState: () => ({ accounts: [{ accountId: alice, active: true }] }) },
      wallet: async () => ({
        getAccounts: async () => [{ accountId: alice }],
        signAndSendTransaction: async () => {
          throw new Error('User rejected the transaction in NEAR Wallet');
        },
      }),
    },
  };

  await assert.rejects(
    contract.mintNftOnChain({
      tokenId: 'token-fail-1',
      metadata: { title: 'Failed Token' },
      wallet: rejectingWallet,
    }),
    /User rejected the transaction in NEAR Wallet/
  );

  // Asserting final success throws on failure receipt
  assert.throws(
    () => contract.assertFinalSuccess({ status: { Failure: { error_message: 'Execution failed' } } }, 'Minting'),
    /Minting failed/
  );
});

test("10. Storage deposits and settlement accounting remain correct", async () => {
  const contract = await loadContractModule();
  const validBase58TxHash = '2mJuAN9Rioqa32mkkMnaScLZUpgdPVMadeSkcyPeVyS3';
  const sharedState = {
    storage: { [alice]: '0' },
  };

  const provider = {
    txStatus: async () => ({
      status: { SuccessValue: '' },
      transaction_outcome: { id: validBase58TxHash },
    }),
    query: async (p) => {
      if (p.method_name === 'storage_balance_of') {
        const bal = sharedState.storage[alice] || '120000000000000000000000';
        return { result: [...Buffer.from(JSON.stringify({ total: bal, available: bal }))] };
      }
      if (p.method_name === 'storage_balance_bounds') {
        return { result: [...Buffer.from(JSON.stringify({ min: '1250000000000000000000' }))] };
      }
      return { result: [] };
    },
  };

  const walletA = {
    accountId: alice,
    provider,
    selector: {
      options: { provider },
      store: { getState: () => ({ accounts: [{ accountId: alice, active: true }] }) },
      wallet: async () => ({
        getAccounts: async () => [{ accountId: alice }],
        signAndSendTransaction: async ({ actions }) => {
          const deposit = actions[0].params.deposit;
          sharedState.storage[alice] = deposit;
          return {
            transaction: { hash: validBase58TxHash },
            status: { SuccessValue: '' },
          };
        },
      }),
    },
  };

  const storageRes = await contract.ensureMarketplaceListingStorage({
    sellerId: alice,
    wallet: walletA,
  });

  assert.equal(storageRes.funded, true);
  assert.equal(storageRes.transactionHash, validBase58TxHash);
  assert.ok(BigInt(storageRes.balance.available) >= 120000000000000000000000n);
});

test("11. Duplicate token ID submission fails with uniqueness contract error", async () => {
  const contract = await loadContractModule();
  const sharedState = {
    tokens: { 'token-existing-1': { token_id: 'token-existing-1', owner_id: alice } },
    ever_minted: { 'token-existing-1': 'Live' },
    storage: {},
    listings: {},
  };

  const walletB = createMockWallet(bob, sharedState);
  await assert.rejects(
    contract.mintNftOnChain({
      tokenId: 'token-existing-1',
      metadata: { title: 'Duplicate Attempt' },
      wallet: walletB,
    }),
    /token_id must be unique and never reused/
  );
});

test("12. generateUniqueTokenId detects on-chain collisions and generates guaranteed unique IDs", async () => {
  const contract = await loadContractModule();
  const sharedState = {
    tokens: { 'catalog-collision-1': { token_id: 'catalog-collision-1', owner_id: alice } },
    ever_minted: { 'catalog-collision-1': 'Live' },
    storage: {},
    listings: {},
  };

  const walletC = createMockWallet(charlie, sharedState);

  // When candidate is collision-free:
  const freshCandidate = await contract.generateUniqueTokenId('fresh-new-id', walletC, 'nft-v2.roundfrog-dev.testnet');
  assert.equal(freshCandidate, 'catalog-fresh-new-id');

  // When candidate collides with existing on-chain token:
  const resolvedCandidate = await contract.generateUniqueTokenId('collision-1', walletC, 'nft-v2.roundfrog-dev.testnet');
  assert.notEqual(resolvedCandidate, 'catalog-collision-1');
  assert.match(resolvedCandidate, /^catalog-[0-9a-f-]{10,}/);

  // Minting with resolved candidate succeeds without uniqueness collision
  const mintRes = await contract.mintNftOnChain({
    tokenId: resolvedCandidate,
    metadata: { title: 'Charlie Resolved Token' },
    wallet: walletC,
  });
  assert.ok(mintRes.transaction.hash.includes(resolvedCandidate));
  assert.equal(sharedState.tokens[resolvedCandidate].owner_id, charlie);
});

test("13. Wallet C can list its own NFT and cross-listing by Wallet A is rejected", async () => {
  const contract = await loadContractModule();
  const sharedState = {
    tokens: {
      'token-c-1': { token_id: 'token-c-1', owner_id: charlie },
    },
    storage: { [charlie]: '150000000000000000000000', [alice]: '150000000000000000000000' },
    listings: {},
  };

  // Alice cannot list Charlie's NFT
  const walletA = createMockWallet(alice, sharedState);
  await assert.rejects(
    contract.verifyListingPreconditions({
      nftContractId: 'nft-v2.roundfrog-dev.testnet',
      tokenId: 'token-c-1',
      sellerId: alice,
      wallet: walletA,
    }),
    /Connected wallet is not the current on-chain NFT owner/
  );

  // Charlie can list his own NFT
  const walletC = createMockWallet(charlie, sharedState);
  const approveRes = await contract.listNftOnChain({
    nftContractId: 'nft-v2.roundfrog-dev.testnet',
    tokenId: 'token-c-1',
    priceNear: '3.5',
    wallet: walletC,
  });
  assert.ok(approveRes.transaction.hash.includes('token-c-1'));
  const listingId = Object.keys(sharedState.listings)[0];
  assert.ok(listingId);
  assert.equal(sharedState.listings[listingId].seller, charlie);
  assert.equal(sharedState.listings[listingId].active, true);
});
