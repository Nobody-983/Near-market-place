// Offline handler and JSX-expression checks; no wallet, backend or browser needed.
// Run: node tests/marketplace-safety.test.mjs
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { test } from 'node:test';
import vm from 'node:vm';
import * as safety from '../frontend/Nft/src/config/marketplace.js';
import { PREVIEW_DATA_ENABLED } from '../frontend/Nft/src/config/preview.js';
import { assertPublishableKey } from '../frontend/Nft/src/lib/supabaseKey.js';
import { validateNearAmount } from '../frontend/Nft/src/lib/near/utils.js';

const require = createRequire(new URL('../frontend/Nft/package.json', import.meta.url));
const { parse } = require('espree');

function* nodes(value) {
  if (!value || typeof value !== 'object') return;
  if (value.type) yield value;
  for (const child of Object.values(value)) {
    if (Array.isArray(child)) {
      for (const entry of child) yield* nodes(entry);
    } else if (child && typeof child === 'object') yield* nodes(child);
  }
}

async function page(name) {
  const source = await readFile(new URL(`../frontend/Nft/src/pages/${name}`, import.meta.url), 'utf8');
  const tree = parse(source, {
    ecmaVersion: 'latest', sourceType: 'module', range: true, ecmaFeatures: { jsx: true },
  });
  return { source, nodes: [...nodes(tree)] };
}

const [collection, details] = await Promise.all([page('createNft.jsx'), page('nftDetails.jsx')]);
const nft = { id: '11111111-1111-4111-8111-111111111111', name: 'Artwork', price: '1' };
const user = { id: '22222222-2222-4222-8222-222222222222' };
const listing = { id: '33333333-3333-4333-8333-333333333333', nft_id: nft.id, seller_id: user.id, status: 'active', price: '1' };

function evaluate(page, expression, context) {
  return vm.runInNewContext(`(${page.source.slice(...expression.range)})`, { ...safety, ...context });
}

function handler(page, name, context) {
  const declaration = page.nodes.find((node) => node.type === 'VariableDeclarator' && node.id.name === name);
  assert.ok(declaration, `${name} must exist`);
  return evaluate(page, declaration.init, context);
}

function fixture(overrides = {}) {
  const calls = [];
  const state = { error: '', success: 'Previous success' };
  const forbidden = (name) => () => { calls.push(name); throw new Error(`Unexpected ${name}`); };
  return {
    calls, state,
    context: {
      user, nft, listing, listingNFT: nft, listingForm: { price: '1' }, cancelling: null,
      isConnected: true, walletAddress: 'alice.testnet', balance: 10,
      MARKETPLACE_CONTRACT_ID: 'market.testnet',
      selector: { wallet: forbidden('wallet request') },
      supabase: { auth: { getUser: forbidden('Auth request') }, rpc: forbidden('RPC'), from: forbidden('database request') },
      buyNFT: forbidden('purchase'), createListing: forbidden('listing creation'), cancelListing: forbidden('cancellation'),
      window: { confirm: forbidden('confirmation dialog') },
      setBuying: forbidden('purchase state'), setListing: forbidden('listing state'), setCancelling: forbidden('cancellation state'),
      setListings: forbidden('listing mutation'), setListingNFT: forbidden('listing modal'), setListingForm: forbidden('listing form'),
      refreshBalance: forbidden('balance request'), setTimeout: forbidden('delayed request'),
      getNFTListing: () => listing,
      setError: (value) => { state.error = value; }, setSuccess: (value) => { state.success = value; },
      console: { error: forbidden('caught error') },
      ...overrides,
    },
  };
}

function attribute(element, name) {
  return element.openingElement.attributes.find((item) => item.type === 'JSXAttribute' && item.name.name === name);
}

function attributeValue(page, element, name, context = {}) {
  const attr = attribute(element, name);
  if (!attr) return undefined;
  if (!attr.value) return true;
  return attr.value.type === 'Literal' ? attr.value.value : evaluate(page, attr.value.expression, context);
}

function elementForHandler(page, tag, event, name) {
  const element = page.nodes.find((node) => node.type === 'JSXElement' && node.openingElement.name.name === tag &&
    [...nodes(attribute(node, event))].some((child) => child.type === 'Identifier' && child.name === name));
  assert.ok(element, `${tag} for ${name} must exist`);
  return element;
}

test('trading safety gates remain closed independently of deployment configuration', () => {
  assert.equal(safety.MINTING_ACTIONS_ENABLED, false);
  assert.equal(safety.LISTING_ACTIONS_ENABLED, false);
  assert.equal(safety.PURCHASES_ENABLED, false);
  assert.equal(PREVIEW_DATA_ENABLED, false, 'demo data requires an explicit opt-in');
});

test('Testnet release configuration accepts only valid distinct reviewed contract accounts', () => {
  assert.equal(safety.isValidTestnetContractId('nft.roundfrog-dev.testnet'), true);
  assert.equal(safety.isValidTestnetContractId('market.roundfrog-dev.testnet'), true);
  for (const invalid of ['', 'nft.testnet.evil', 'NFT.roundfrog-dev.testnet', 'mainnet.near', 'a'.repeat(65)]) {
    assert.equal(safety.isValidTestnetContractId(invalid), false, invalid);
  }
  assert.match(safety.MINTING_UNAVAILABLE_MESSAGE, /mint authority.*complete mint-to-catalog lifecycle/i);
  assert.match(safety.LISTING_UNAVAILABLE_MESSAGE, /finalized Testnet receipts.*trusted catalog reconciliation/i);
});

test('browser Supabase keys must be publishable, never service-role secrets', async () => {
  assert.equal(assertPublishableKey('sb_publishable_example'), 'sb_publishable_example');
  assert.throws(() => assertPublishableKey('sb_secret_example'), /never be exposed/);
  assert.throws(() => assertPublishableKey(''), /publishable/);
  const claims = Buffer.from(JSON.stringify({ role: 'service_role' })).toString('base64url');
  assert.throws(() => assertPublishableKey(`header.${claims}.signature`), /never be exposed/);
  const anonClaims = Buffer.from(JSON.stringify({ role: 'anon' })).toString('base64url');
  assert.equal(assertPublishableKey(`header.${anonClaims}.signature`), `header.${anonClaims}.signature`);
});

test('the Vite env template pins deployed Testnet contracts and no secrets', async () => {
  const env = await readFile(new URL('../frontend/Nft/.env.example', import.meta.url), 'utf8');
  assert.match(env, /^VITE_NFT_CONTRACT_ID=nft\.roundfrog-dev\.testnet$/m);
  assert.match(env, /^VITE_MARKETPLACE_CONTRACT_ID=market\.roundfrog-dev\.testnet$/m);
  assert.match(env, /^VITE_NEAR_NETWORK_ID=testnet$/m);
  assert.match(env, /^VITE_SUPABASE_PUBLISHABLE_KEY=/m);
  assert.match(env, /^VITE_PREVIEW_DATA_ENABLED=false$/m);
  assert.match(env, /^VITE_TESTNET_LISTING_FLOW_VERIFIED=false$/m);
  assert.match(env, /^VITE_TESTNET_MINT_FLOW_VERIFIED=false$/m);
  assert.doesNotMatch(env, /SERVICE_ROLE|SECRET_KEY|PRIVATE_KEY/i);
});

test('forced purchase callbacks cannot request a wallet or use the database fallback', async () => {
  for (const contractId of ['', 'market.testnet']) {
    for (const connected of [false, true]) {
      for (const preview of [false, true]) {
        const f = fixture({ MARKETPLACE_CONTRACT_ID: contractId, isConnected: connected, nft: { ...nft, is_preview: preview } });
        await handler(details, 'handleBuy', f.context)();
        assert.deepEqual(f.calls, []);
        assert.equal(f.state.error, safety.PURCHASE_UNAVAILABLE_MESSAGE);
        assert.equal(f.state.success, '');
      }
    }
  }
});

test('missing marketplace configuration cannot fabricate settlement even if the UI gate is bypassed', async () => {
  let failure = '';
  const forbidden = () => assert.fail('No wallet or settlement request is allowed');
  await handler(details, 'handleBuy', {
    PURCHASES_ENABLED: true, MARKETPLACE_CONTRACT_ID: '',
    nft, listing, isConnected: true, walletAddress: 'alice.testnet', validateNearAmount,
    selector: { wallet: forbidden }, supabase: { rpc: forbidden }, buyNFT: forbidden,
    setBuying() {}, setSuccess: (message) => assert.equal(message, ''),
    setError: (message) => { failure = message; }, console: { error() {} },
  })();
  assert.equal(failure, safety.PURCHASE_UNAVAILABLE_MESSAGE);
  assert.doesNotMatch(details.source, /complete_nft_purchase|p_transaction_hash|Purchase completed successfully/);
});

test('listing entry, form submission and cancellation stop before any side effect', async () => {
  for (const currentUser of [null, user]) {
    for (const name of ['openListingForm', 'handleListNFT', 'handleCancelListing']) {
      const f = fixture({ user: currentUser });
      let prevented = false;
      const argument = name === 'handleListNFT' ? { preventDefault: () => { prevented = true; } } : nft;
      await handler(collection, name, f.context)(argument);
      assert.equal(prevented, name === 'handleListNFT');
      assert.deepEqual(f.calls, [], name);
      assert.equal(f.state.error, safety.LISTING_UNAVAILABLE_MESSAGE);
      assert.equal(f.state.success, '');
    }
  }
});

test('trade buttons are disabled and associated with visible explanations', () => {
  for (const [page, name, message] of [
    [collection, 'openListingForm', safety.LISTING_UNAVAILABLE_MESSAGE],
    [collection, 'handleCancelListing', safety.LISTING_UNAVAILABLE_MESSAGE],
    [details, 'handleBuy', safety.PURCHASE_UNAVAILABLE_MESSAGE],
  ]) {
    const button = elementForHandler(page, 'button', 'onClick', name);
    const context = { nft, buying: false, cancelling: null, isConnected: true };
    assert.equal(attributeValue(page, button, 'disabled', context), true, name);
    const descriptionId = attributeValue(page, button, 'aria-describedby', context);
    assert.ok(descriptionId);
    const notice = page.nodes.find((node) => node.type === 'JSXElement' && attributeValue(page, node, 'id') === descriptionId);
    assert.ok(notice, 'disabled controls must explain why they are unavailable');
    const condition = page.nodes.find((node) => node.type === 'LogicalExpression' && node.right === notice);
    assert.ok(condition, 'the availability notice must be rendered while paused');
    assert.equal(evaluate(page, condition.left), true);
    const text = notice.children.filter((node) => node.type === 'JSXExpressionContainer')
      .map((node) => evaluate(page, node.expression)).join('');
    assert.equal(text, message);
    assert.match(text, /disabled|temporarily unavailable/);
  }
});

test('an existing listing modal stays hidden and its submit button stays disabled', () => {
  const form = elementForHandler(collection, 'form', 'onSubmit', 'handleListNFT');
  const condition = collection.nodes.find((node) => node.type === 'LogicalExpression' &&
    node.right.type === 'JSXElement' && [...nodes(node.right)].includes(form));
  assert.ok(condition);
  assert.equal(Boolean(evaluate(collection, condition.left, { listingNFT: nft })), false);
  const submit = [...nodes(form)].find((node) => node.type === 'JSXElement' &&
    node.openingElement.name.name === 'button' && attributeValue(collection, node, 'type') === 'submit');
  assert.ok(submit);
  assert.equal(attributeValue(collection, submit, 'disabled', { listing: null, listingNFT: nft }), true);
});

test('viewing NFTs, sharing, likes and catalog creation controls remain available', () => {
  for (const [page, name] of [[collection, 'handleViewNFT'], [details, 'handleShare'], [details, 'handleLike']]) {
    const button = elementForHandler(page, 'button', 'onClick', name);
    assert.equal(Boolean(attributeValue(page, button, 'disabled', { loadingLikes: false, likingNFT: null })), false, name);
  }
  const location = { href: '' };
  handler(collection, 'handleViewNFT', { window: { location } })(nft);
  assert.equal(location.href, `/nft/${nft.id}`);
  const form = elementForHandler(collection, 'form', 'onSubmit', 'handleSubmit');
  const submit = [...nodes(form)].find((node) => node.type === 'JSXElement' &&
    node.openingElement.name.name === 'button' && attributeValue(collection, node, 'type') === 'submit');
  assert.ok(submit);
  assert.equal(Boolean(attributeValue(collection, submit, 'disabled', { loading: false, uploading: false })), false);
});
