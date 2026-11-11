// Offline price regressions: real frontend helpers/SDK, mocked HTTP and wallets.
// Run: node --experimental-vm-modules tests/near-price-precision.test.mjs
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { test } from 'node:test';
import vm from 'node:vm';
import * as amounts from '../frontend/Nft/src/lib/near/utils.js';
import * as preview from '../frontend/Nft/src/lib/previewData.js';
import * as safety from '../frontend/Nft/src/config/marketplace.js';

const require = createRequire(new URL('../frontend/Nft/package.json', import.meta.url));
const { createClient } = require('@supabase/supabase-js');
const { parse } = require('espree');
const exactPrices = [
  ['0.00000001', '10000000000000000'],
  ['1.000000000000000000000001', '1000000000000000000000001'],
  ['1', '1000000000000000000000000'],
  ['42', '42000000000000000000000000'],
  ['0.000000000000000000000001', '1'],
  ['340282366920938.463463374607431768211455', '340282366920938463463374607431768211455'],
];
const invalidPrices = [
  '', '-1', '-0', '+1', '.1', '1.', '1..2', '1e-8', '1E24', '0x10', '1,000',
  ' 1', '1 ', '1\n', '1\t', 'NaN', 'Infinity', '1.0000000000000000000000001',
  '340282366920938.463463374607431768211456', 1, 0.00000001, NaN, Infinity, null, undefined, 1n,
];
const quietConsole = { error() {}, warn() {} };
const config = {
  NETWORK_ID: 'testnet',
  NFT_CONTRACT_ID: 'nft.testnet',
  MARKETPLACE_CONTRACT_ID: 'market.testnet',
  REVIEWED_NFT_CONTRACT_ID: 'nft.testnet',
  REVIEWED_MARKETPLACE_CONTRACT_ID: 'market.testnet',
  REVIEWED_NFT_CODE_HASH: 'nft-reviewed-hash',
  REVIEWED_MARKETPLACE_CODE_HASH: 'market-reviewed-hash',
  GAS: '300000000000000',
  DEPOSIT_STORAGE: '1250000000000000000000',
};
const user = { id: '11111111-1111-4111-8111-111111111111' };
const nft = {
  id: '22222222-2222-4222-8222-222222222222', creator_id: user.id, name: 'Artwork',
  token_id: 'token-1', nft_contract_id: 'nft.testnet',
};
const listing = {
  id: '33333333-3333-4333-8333-333333333333',
  nft_id: nft.id,
  seller_id: user.id,
  status: 'active',
  network_id: 'testnet',
  marketplace_contract_id: 'market.testnet',
  contract_sale_id: 'nft.testnet|token-1|seller.testnet|1',
};

async function loadModule(path, imports) {
  const context = vm.createContext({ console: quietConsole, crypto, Buffer, setTimeout, clearTimeout });
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

const contract = await loadModule('../frontend/Nft/src/lib/near/contract.js', {
  './utils': amounts, '../../config/near': config,
  '../../config/marketplace': { LISTING_STORAGE_TARGET_YOCTO: '120000000000000000000000' },
});

test('contract views use the supported provider query interface', async () => {
  const calls = [];
  const provider = {
    query: async (request) => {
      calls.push(request);
      return { result: [...Buffer.from(JSON.stringify({ trading_enabled: false }))] };
    },
  };
  const result = await contract.marketplaceConfig({ provider });
  assert.equal(result.trading_enabled, false);
  assert.equal(calls[0].request_type, 'call_function');
  assert.equal(calls[0].account_id, 'market.testnet');
  assert.equal(calls[0].method_name, 'config');
  assert.deepEqual(JSON.parse(Buffer.from(calls[0].args_base64, 'base64')), {});
});

test('wallet switching is detected before transaction success is reported', async () => {
  const state = { accounts: [{ accountId: 'seller.testnet', active: true }] };
  const wallet = { selector: { store: { getState: () => state } } };
  assert.equal(await contract.requireConnectedAccount('seller.testnet', wallet), 'seller.testnet');
  state.accounts = [{ accountId: 'buyer.testnet', active: true }];
  await assert.rejects(contract.requireConnectedAccount('seller.testnet', wallet), /wallet changed/);
});

test('wallet responses never receive fabricated transaction hashes', async () => {
  const wallet = { signAndSendTransaction: async () => ({ hash: 'not-a-transaction-result' }) };
  const result = await contract.call('market.testnet', 'cancel_listing', { listing_id: 'listing' }, '1', wallet);
  assert.equal(result.transaction, undefined);
  assert.throws(() => contract.transactionHash(result), /valid NEAR transaction hash/);
});

test('listing storage is funded and confirmed before approval can continue', async () => {
  const txHash = '6RWmTYhXCzjMjoY3Mz1rfFcnBm8E6XeDDbFEPUA4sv1w';
  let balanceReads = 0;
  const sent = [];
  const provider = {
    query: async (request) => {
      const args = JSON.parse(Buffer.from(request.args_base64, 'base64'));
      assert.equal(request.account_id, 'market.testnet');
      if (request.method_name === 'storage_balance_bounds') {
        return { result: [...Buffer.from(JSON.stringify({ min: '1000', max: null }))] };
      }
      assert.equal(request.method_name, 'storage_balance_of');
      assert.deepEqual(args, { account_id: 'seller.testnet' });
      balanceReads++;
      const value = balanceReads === 1 ? null : {
        total: '120000000000000000001000', available: '120000000000000000000000',
      };
      return { result: [...Buffer.from(JSON.stringify(value))] };
    },
    txStatus: async () => ({ status: { SuccessValue: '' }, receipts_outcome: [] }),
  };
  const wallet = {
    provider, accountId: 'seller.testnet',
    signAndSendTransaction: async (transaction) => { sent.push(transaction); return { transaction: { hash: txHash } }; },
  };
  const result = await contract.ensureMarketplaceListingStorage({ sellerId: 'seller.testnet', wallet });
  assert.equal(result.funded, true);
  assert.equal(sent.length, 1);
  assert.equal(sent[0].actions[0].params.methodName, 'storage_deposit');
  assert.equal(sent[0].actions[0].params.deposit, '120000000000000000001000');
  assert.equal(balanceReads, 2);
});

test('decimal strings convert to exact integer yoctoNEAR and round-trip without rounding', () => {
  for (const [price, yocto] of exactPrices) {
    assert.equal(amounts.validateNearAmount(price), price);
    assert.equal(amounts.nearToYocto(price), yocto);
    assert.equal(amounts.yoctoToNear(yocto), price);
  }
  assert.equal(amounts.validateNearAmount('0002.500'), '0002.500');
  assert.equal(amounts.nearToYocto('0002.500'), '2500000000000000000000000');
  assert.equal(amounts.nearToYocto('0.000'), '0');
  assert.equal(amounts.yoctoToNear('0'), '0');
  assert.equal(amounts.formatNearAmount('1234000000000000000000000001'), '1,234.000000000000000000000001');
});

test('malformed, negative, over-precision, overflowing and already numeric prices are rejected', () => {
  for (const price of invalidPrices) {
    assert.throws(() => amounts.validateNearAmount(price), /NEAR amount/);
    assert.throws(() => amounts.nearToYocto(price), /NEAR amount/);
  }
  assert.throws(() => amounts.nearToYocto('0', { allowZero: false }), /greater than zero/);
  assert.throws(() => amounts.validateNearAmount('0.000', { allowZero: false }), /greater than zero/);
  for (const yocto of ['-1', '1.5', '1e24', '1\n', 1]) {
    assert.throws(() => amounts.yoctoToNear(yocto), /Invalid yoctoNEAR/);
  }
});

test('listing and purchase transaction payloads use exact string amounts', async () => {
  for (const [price, expected] of exactPrices) {
    const calls = [];
    const wallet = { signAndSendTransaction: async (transaction) => { calls.push(transaction); return { transaction: { hash: 'test-hash' } }; } };
    await contract.listNFTForSale(wallet, { tokenId: 'token-1', price });
    await contract.buyNFT(wallet, { saleId: 'sale-42', price });
    assert.deepEqual(JSON.parse(calls[0].actions[0].params.args.msg), { price: expected });
    assert.equal(calls[1].actions[0].params.deposit, expected);
    assert.equal(calls[1].actions[0].params.args.listing_id, 'sale-42');
  }
});

test('invalid transaction prices fail before a wallet can be prompted', async () => {
  let prompts = 0;
  const wallet = { signAndSendTransaction: async () => { prompts++; } };
  for (const price of [...invalidPrices, '0', '0.000']) {
    await assert.rejects(contract.listNFTForSale(wallet, { tokenId: 'token-1', price }));
    await assert.rejects(contract.buyNFT(wallet, { saleId: 'sale-42', price }));
  }
  assert.equal(prompts, 0);
});

async function services(price = exactPrices[1][0]) {
  const calls = [];
  const project = 'https://precision.example';
  const state = { responsePrice: price };
  const client = createClient(project, 'offline-test-key', {
    auth: { persistSession: false, autoRefreshToken: false, detectSessionInUrl: false },
    global: { fetch: async (input, options = {}) => {
      const url = new URL(input instanceof Request ? input.url : input);
      assert.equal(url.origin, project, 'all HTTP must stay in this offline mock');
      const method = options.method || 'GET';
      const body = options.body ? JSON.parse(options.body) : null;
      const select = url.searchParams.get('select');
      calls.push({ table: url.pathname.split('/').pop(), method, body, select });
      const table = url.pathname.split('/').pop();
      const single = new Headers(options.headers).get('accept')?.includes('pgrst.object');
      let result;
      if (table === 'nft_ownership') {
        result = { nft_id: nft.id, owner_id: user.id };
      } else if (table === 'marketplace_listings' && select === 'id') {
        return new Response('[]', { headers: { 'Content-Type': 'application/json' } });
      } else {
        assert.match(select, /(?:^|,)price::text(?:,|$)/, 'price must be cast before JSON decoding');
        assert.doesNotMatch(select, /\*/, 'a duplicate numeric price must not enter the response');
        result = { ...(table === 'nfts' ? nft : listing), ...body, price: state.responsePrice };
      }
      return new Response(JSON.stringify(single ? result : [result]), { headers: { 'Content-Type': 'application/json' } });
    } },
  });
  client.auth.getUser = async () => ({ data: { user }, error: null });
  client.auth.getSession = async () => ({ data: { session: { user, access_token: 'offline-access-token' } }, error: null });
  const imports = {
    '../lib/supabase': { supabase: client },
    '../lib/near/utils': amounts,
    '../lib/previewData': preview,
    '../config/preview': { PREVIEW_DATA_ENABLED: false },
  };
  const [market, nfts] = await Promise.all([
    loadModule('../frontend/Nft/src/services/marketService.js', imports),
    loadModule('../frontend/Nft/src/services/nftService.js', imports),
  ]);
  const imagePath = `${user.id}/44444444-4444-4444-8444-444444444444.png`;
  return { market, nfts, calls, state,
    imageUrl: client.storage.from('nft-images').getPublicUrl(imagePath).data.publicUrl };
}

test('service reads and trusted indexed responses preserve API prices as validated strings', async () => {
  for (const [price] of exactPrices.slice(0, 3)) {
    const s = await services(price);
    const catalog = await s.market.getMarketplaceListings();
    const mine = await s.market.getUserListings(user.id);
    const indexedByTx = await s.market.getIndexedListingByTransaction('6RWmTYhXCzjMjoY3Mz1rfFcnBm8E6XeDDbFEPUA4sv1w');
    const indexedByContract = await s.market.getIndexedListingByContractId(listing.contract_sale_id);
    const createdNft = await s.nfts.createNFT({ name: nft.name, category: 'Art', image_url: s.imageUrl, price });
    for (const row of [catalog[0], mine[0], indexedByTx, indexedByContract, createdNft]) {
      assert.equal(row.price, price);
      assert.equal(typeof row.price, 'string');
    }
    for (const call of s.calls.filter((call) => call.method === 'POST')) {
      assert.equal(call.body.price, price, 'outgoing JSON must contain the original decimal string');
    }
  }
});

test('services reject invalid input before making HTTP calls', async () => {
  const s = await services();
  await assert.rejects(s.market.getIndexedListingByTransaction(''));
  await assert.rejects(s.market.getIndexedListingByContractId(''));
  for (const price of invalidPrices) {
    // Omitted catalog prices retain their existing zero-price default.
    if (price !== undefined) {
      await assert.rejects(s.nfts.createNFT({ name: nft.name, category: 'Art', image_url: s.imageUrl, price }));
    }
  }
  assert.deepEqual(s.calls, []);
  s.state.responsePrice = '0';
  assert.equal((await s.nfts.createNFT({ name: nft.name, category: 'Art', image_url: s.imageUrl })).price, '0');
});

test('numeric or invalid API responses are not repaired using lossy string coercion', async () => {
  const s = await services();
  for (const invalid of [1, '1e-8', '-1', '1.0000000000000000000000001']) {
    s.state.responsePrice = invalid;
    await assert.rejects(s.market.getUserListings(user.id));
    await assert.rejects(s.market.getIndexedListingByContractId(listing.contract_sale_id));
    await assert.rejects(s.nfts.createNFT({ name: nft.name, category: 'Art', image_url: s.imageUrl, price: '1' }));
    // Invalid API prices fail closed; demo rows never replace a broken response.
    await assert.rejects(s.market.getMarketplaceListings(), /Cannot read properties|NEAR amount/);
  }
});

test('preview listings replace data only when the explicit demo flag is enabled', async () => {
  const marketSource = await readFile(
    new URL('../frontend/Nft/src/services/marketService.js', import.meta.url), 'utf8',
  );
  assert.match(marketSource, /previewEnabled = PREVIEW_DATA_ENABLED/);
  const market = await loadModule('../frontend/Nft/src/services/marketService.js', {
    '../lib/supabase': { supabase: { from() { throw new Error('no request expected'); } } },
    '../lib/near/utils': amounts,
    '../lib/previewData': preview,
    '../config/preview': { PREVIEW_DATA_ENABLED: false },
  });
  assert.equal(market.selectMarketplaceListings([], { previewEnabled: false }).length, 0);
  const demo = market.selectMarketplaceListings([], { previewEnabled: true });
  assert.deepEqual(
    JSON.parse(JSON.stringify(demo.map((row) => row.id))),
    preview.PREVIEW_LISTINGS.map((row) => row.id),
  );
});

function* walk(value) {
  if (!value || typeof value !== 'object') return;
  if (value.type) yield value;
  for (const child of Object.values(value)) {
    if (Array.isArray(child)) { for (const item of child) yield* walk(item); }
    else if (child && typeof child === 'object') yield* walk(child);
  }
}

async function page(name) {
  const source = await readFile(new URL(`../frontend/Nft/src/pages/${name}`, import.meta.url), 'utf8');
  const nodes = [...walk(parse(source, { ecmaVersion: 'latest', sourceType: 'module', range: true, ecmaFeatures: { jsx: true } }))];
  return { source, nodes };
}

const [collection, details] = await Promise.all([page('createNft.jsx'), page('nftDetails.jsx')]);
function handler(page, name, context) {
  const node = page.nodes.find((node) => node.type === 'VariableDeclarator' && node.id.name === name);
  assert.ok(node, `${name} must exist`);
  return vm.runInNewContext(`(${page.source.slice(...node.init.range)})`, { ...amounts, ...context });
}

test('form and purchase handlers pass full-precision prices through unchanged', async () => {
  // Only these isolated callbacks bypass the gates to test unfinished paths with
  // fake services. The production gate values are asserted below and never edited.
  for (const [price, yocto] of exactPrices.slice(0, 3)) {
    const sent = [];
    const context = {
      user, nft, listing: { ...listing, price }, listingNFT: nft, listingForm: { price },
      form: { name: nft.name, description: '', category: 'Art', price },
      image: {}, loading: false, validateImage: () => null, removeImage() {},
      isConnected: true, walletAddress: 'alice.testnet', selector: { wallet: async () => ({}) },
      ...config, LISTING_ACTIONS_ENABLED: true, PURCHASES_ENABLED: true,
      uploadNFTImage: async () => ({ publicUrl: 'https://image.example/art.png' }),
      createNFT: async (body) => { sent.push(body.price); return nft; },
      listNftOnChain: async (body) => {
        sent.push(body.priceNear);
        return { transaction: { hash: 'listing-hash' } };
      },
      verifyListingPreconditions: async () => ({}),
      ensureMarketplaceListingStorage: async () => ({ funded: false }),
      transactionHash: (result) => result.transaction.hash,
      assertFinalSuccess: (result) => result,
      listingIdFromFinalResult: () => `nft.testnet|token-1|alice.testnet|1`,
      marketplaceListing: async (id) => ({ active: true, token_id: nft.token_id,
        nft_contract_id: nft.nft_contract_id,
        seller: id.includes('|alice.testnet|') ? 'alice.testnet' : 'seller.testnet', price: yocto }),
      marketplaceListingMetadata: async () => ({ status: 'active', generation: '1' }),
      buyNftOnChain: async (_saleId, amount) => {
        sent.push(amount);
        return { transaction: { hash: 'test-hash' } };
      },
      waitForFinal: async () => ({ status: { SuccessValue: '' } }),
      waitForSettlement: async () => ({ status: 'settled' }),
      waitForIndexedListing: async () => null,
      requireConnectedAccount: async (accountId) => accountId,
      setError: (message) => { assert.equal(message, ''); }, setSuccess() {}, setLoading() {}, setUploading() {},
      setNfts() {}, setForm() {}, setListing() {}, setListings() {}, setListingNFT() {}, setListingForm() {},
      setBuying() {}, refreshBalance() {}, setTimeout() {}, fetchListings: async () => {}, console: quietConsole,
    };
    await handler(collection, 'handleSubmit', context)({ preventDefault() {} });
    await handler(collection, 'handleListNFT', { ...context, listing: null })({ preventDefault() {} });
    await handler(details, 'handleBuy', context)();
    assert.deepEqual(sent, [price, price, yocto]);
  }
});

test('invalid form prices stop before upload or listing writes', async () => {
  for (const price of ['-1', '1e-8', '1.0000000000000000000000001', '0']) {
    for (const name of ['handleSubmit', 'handleListNFT']) {
      let error = '';
      const context = {
        user, listing: null, listingNFT: nft, listingForm: { price },
        form: { name: nft.name, description: '', price }, image: {}, loading: false,
        LISTING_ACTIONS_ENABLED: true, validateImage: () => null,
        setError: (value) => { error = value; }, setSuccess() {},
      };
      await handler(collection, name, context)({ preventDefault() {} });
      assert.match(error, /NEAR amount/);
    }
  }
});

test('price inputs preserve decimal text and production trading gates stay closed', () => {
  const inputs = collection.nodes.filter((node) => node.type === 'JSXOpeningElement' && node.name.name === 'input' &&
    node.attributes.some((attr) => attr.name?.name === 'value' && attr.value?.expression?.property?.name === 'price'));
  assert.equal(inputs.length, 2);
  for (const input of inputs) {
    const attr = (name) => input.attributes.find((attr) => attr.name?.name === name)?.value?.value;
    assert.equal(attr('type'), 'text');
    assert.equal(attr('inputMode'), 'decimal');
    assert.equal(attr('step'), undefined);
  }
  assert.equal(safety.LISTING_ACTIONS_ENABLED, false);
  assert.equal(safety.PURCHASES_ENABLED, false);
  for (const row of [...preview.PREVIEW_NFTS, ...preview.PREVIEW_LISTINGS]) {
    assert.equal(amounts.validateNearAmount(row.price), row.price);
  }
});

test('no marketplace source file contains fiat or non-NEAR currency symbols and preview data uses NEAR', async () => {
  const fg = await import('node:fs/promises');
  const path = await import('node:path');

  async function* getFiles(dir) {
    const dirents = await fg.readdir(dir, { withFileTypes: true });
    for (const dirent of dirents) {
      const res = path.resolve(dir, dirent.name);
      if (dirent.isDirectory()) {
        yield* getFiles(res);
      } else {
        yield res;
      }
    }
  }

  const forbidden = [/₦/, /\bNGN\b/, /\bNaira\b/, /\bNIM\b/];
  const srcDir = path.resolve(new URL('../frontend/Nft/src', import.meta.url).pathname);

  for await (const file of getFiles(srcDir)) {
    if (file.endsWith('.jsx') || file.endsWith('.js')) {
      const content = await fg.readFile(file, 'utf8');
      for (const pattern of forbidden) {
        assert.equal(
          pattern.test(content),
          false,
          `File ${file} contains forbidden currency pattern ${pattern}`
        );
      }
    }
  }

  for (const row of [...preview.PREVIEW_NFTS, ...preview.PREVIEW_LISTINGS]) {
    assert.equal(row.currency, 'NEAR');
  }
});

