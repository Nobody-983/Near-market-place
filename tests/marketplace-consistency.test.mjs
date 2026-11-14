// Offline regressions: real frontend modules and Supabase SDK, mocked HTTP.
// Run: node --experimental-vm-modules tests/marketplace-consistency.test.mjs
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { test } from 'node:test';
import vm from 'node:vm';
import * as amounts from '../frontend/Nft/src/lib/near/utils.js';

const require = createRequire(new URL('../frontend/Nft/package.json', import.meta.url));
const { createClient } = require('@supabase/supabase-js');
const { parse } = require('espree');
const alice = { id: '11111111-1111-4111-8111-111111111111' };
const bob = { id: '22222222-2222-4222-8222-222222222222' };
const nft = { id: '33333333-3333-4333-8333-333333333333', name: 'Artwork', price: '1',
  creator_id: alice.id, owner_id: alice.id, source: 'user', token_id: null, nft_contract_id: null,
  listing_history: [], image_path: `${alice.id}/art.png` };
const quietConsole = { error() {}, warn() {} };
const turn = () => new Promise((resolve) => setImmediate(resolve));
const deferred = () => {
  let resolve;
  const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve };
};
const reply = (data, status = 200) => new Response(JSON.stringify(data), {
  status, headers: { 'Content-Type': 'application/json' },
});

async function loadModule(path, imports) {
  const context = vm.createContext({ console: quietConsole, Buffer });
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

const reviewedContract = await loadModule('../frontend/Nft/src/lib/near/contract.js', {
  './utils': amounts,
  '../../config/near': {
    NETWORK_ID: 'testnet',
    NFT_CONTRACT_ID: 'nft.roundfrog-dev.testnet',
    MARKETPLACE_CONTRACT_ID: 'market.roundfrog-dev.testnet',
    REVIEWED_NFT_CONTRACT_ID: 'nft.roundfrog-dev.testnet',
    REVIEWED_MARKETPLACE_CONTRACT_ID: 'market.roundfrog-dev.testnet',
    REVIEWED_NFT_CODE_HASH: 'nft-reviewed-hash',
    REVIEWED_MARKETPLACE_CODE_HASH: 'market-reviewed-hash',
  },
  '../../config/marketplace': { LISTING_STORAGE_TARGET_YOCTO: '120000000000000000000000' },
});

function backend() {
  const calls = [];
  const state = { respond: () => reply([]) };
  const supabase = createClient('https://consistency.example', 'offline-test-key', {
    auth: { persistSession: false, autoRefreshToken: false, detectSessionInUrl: false },
    global: { fetch: async (input, options = {}) => {
      const url = new URL(input instanceof Request ? input.url : input);
      assert.equal(url.origin, 'https://consistency.example');
      const call = { url, method: options.method || 'GET', body: options.body ? JSON.parse(options.body) : null };
      calls.push(call);
      return state.respond(call);
    } },
  });
  // An Auth session alone must never be used as the verified favorites identity.
  supabase.auth.getSession = async () => ({ data: { session: { user: alice, access_token: 'offline-token' } }, error: null });
  supabase.auth.getUser = () => { throw new Error('Use the verified WalletContext identity'); };
  return { supabase, calls, state };
}

// Small deterministic hook host: preserves state/ref slots, effect cleanup and
// render/commit order so delayed responses and identity changes can be exercised.
async function favorites(b, user = alice, walletAddress = 'alice.testnet') {
  let wallet = { user, walletAddress };
  let scope = '';
  const slots = [];
  let cursor = 0;
  let effects = [];
  const react = {
    useState(initial) {
      const index = cursor++;
      if (!slots[index]) slots[index] = { value: typeof initial === 'function' ? initial() : initial };
      return [slots[index].value, (next) => {
        slots[index].value = typeof next === 'function' ? next(slots[index].value) : next;
      }];
    },
    useRef(initial) {
      const index = cursor++;
      if (!slots[index]) slots[index] = { current: initial };
      return slots[index];
    },
    useEffect(callback, deps) {
      const index = cursor++;
      if (!slots[index] || deps.some((value, i) => value !== slots[index].deps[i])) {
        effects.push(() => {
          slots[index]?.cleanup?.();
          slots[index] = { deps, cleanup: callback() };
        });
      }
    },
  };
  const { useNFTLikes } = await loadModule('../frontend/Nft/src/hooks/useNFTLikes.js', {
    react, '../context/walletContext': { useWallet: () => wallet }, '../lib/supabase': { supabase: b.supabase },
  });
  const commit = () => { const pending = effects; effects = []; pending.forEach((effect) => effect()); };
  const render = (commitEffects = true) => {
    cursor = 0;
    const output = useNFTLikes(scope);
    if (commitEffects) commit();
    return output;
  };
  return {
    render, commit,
    switchUser: (user, walletAddress) => { wallet = { user, walletAddress }; },
    setScope: (value) => { scope = value; },
    unmount: () => slots.forEach((slot) => slot.cleanup?.()),
    settle: async () => { await turn(); return render(); },
  };
}

test('favorites use verified UUIDs and change only after a matching write is confirmed', async () => {
  const b = backend();
  const f = await favorites(b);
  assert.equal(f.render().loadingLikes, true);
  let view = await f.settle();
  assert.equal(b.calls[0].url.searchParams.get('user_id'), `eq.${alice.id}`);
  const pending = deferred();
  b.state.respond = () => pending.promise;
  const saving = view.toggleLike(nft.id);
  await turn();
  view = f.render();
  assert.equal(view.likedNFTs.has(nft.id), false);
  assert.equal(view.likingNFT, nft.id);
  assert.equal(await view.toggleLike(nft.id), null, 'rapid repeat clicks must not write twice');
  assert.deepEqual(b.calls[1].body, { nft_id: nft.id, user_id: alice.id });
  assert.equal(b.calls.length, 2);
  pending.resolve(reply({ nft_id: nft.id, user_id: alice.id }));
  assert.equal((await saving).change, 1);
  view = f.render();
  assert.equal(view.likedNFTs.has(nft.id), true);
  b.state.respond = () => reply({ nft_id: nft.id, user_id: alice.id });
  assert.equal((await view.toggleLike(nft.id)).change, -1);
  assert.equal(f.render().likedNFTs.has(nft.id), false);
  const deletion = b.calls.at(-1);
  assert.equal(deletion.method, 'DELETE');
  assert.equal(deletion.url.searchParams.get('user_id'), `eq.${alice.id}`);
  assert.equal(deletion.url.searchParams.get('nft_id'), `eq.${nft.id}`);
  f.unmount();
});

test('returned errors, thrown failures and zero-row writes never report a successful favorite change', async () => {
  for (const alreadyLiked of [false, true]) {
    const b = backend();
    b.state.respond = () => reply(alreadyLiked ? [{ nft_id: nft.id }] : []);
    const f = await favorites(b);
    f.render();
    await f.settle();
    for (const respond of [
      () => reply({ message: 'permission denied', code: '42501' }, 403),
      () => reply({ message: 'duplicate favorite', code: '23505' }, 409),
      () => reply({ code: 'PGRST116', message: 'Cannot coerce to a single JSON object', details: 'The result contains 0 rows' }, 406),
      () => reply(null), () => reply([]),
      () => reply({ nft_id: nft.id, user_id: bob.id }),
      () => { throw new Error('offline'); },
    ]) {
      b.state.respond = respond;
      assert.equal(await f.render().toggleLike(nft.id), null);
      const view = f.render();
      assert.equal(view.likedNFTs.has(nft.id), alreadyLiked);
      assert.ok(view.likeError);
      assert.equal(view.likingNFT, null);
    }
    f.unmount();
  }
});

test('nonexistent NFTs or unresolvable catalog strings cannot be liked and reject gracefully without invalid UUID writes', async () => {
  const b = backend();
  const f = await favorites(b);
  f.render();
  const view = await f.settle();

  // Test 1: Unregistered string ID like catalog-roundfrog-genesis-02
  b.state.respond = () => reply(null); // lookup in public.nfts yields nothing
  const unresolvable = 'catalog-roundfrog-genesis-02';
  const result = await view.toggleLike(unresolvable);
  assert.equal(result, null);
  const currentView = f.render();
  assert.match(currentView.likeError, /not registered in the catalog/);
  // Verify that NO insert to nft_likes was executed
  const likeInserts = b.calls.filter((c) => c.method === 'POST' && c.url.pathname.includes('nft_likes'));
  assert.equal(likeInserts.length, 0);

  // Test 2: catalog-<UUID> correctly parses and inserts using canonical UUID
  const validUuid = '44444444-4444-4444-8444-444444444444';
  const catalogWithUuid = `catalog-${validUuid}`;
  b.state.respond = () => reply({ nft_id: validUuid, user_id: alice.id });
  const successResult = await view.toggleLike(catalogWithUuid);
  assert.equal(successResult.liked, true);
  assert.equal(successResult.resolvedId, validUuid);
  assert.equal(f.render().likedNFTs.has(validUuid), true);
  assert.equal(f.render().likedNFTs.has(catalogWithUuid), true);
  f.unmount();
});

test('a connected but unverified wallet cannot load or write personal favorites', async () => {
  const b = backend();
  const f = await favorites(b, null, 'alice.testnet');
  f.render();
  assert.equal(await f.render().toggleLike(nft.id), null);
  assert.match(f.render().likeError, /verify/);
  assert.deepEqual(b.calls, []);
  f.unmount();
});

test('listing readiness checks reviewed code, binding, owner, and live marketplace ABI', async () => {
  const requests = [];
  const provider = {
    async query(request) {
      requests.push(request);
      if (request.request_type === 'view_account') {
        return { code_hash: request.account_id === 'nft.roundfrog-dev.testnet'
          ? 'nft-reviewed-hash' : 'market-reviewed-hash' };
      }
      const result = {
        config: { state_version: 2, trading_enabled: true, allowed_nft_contracts: ['nft.roundfrog-dev.testnet'] },
        marketplace_id: 'market.roundfrog-dev.testnet',
        nft_token: { token_id: 'token-1', owner_id: 'seller.testnet' },
      }[request.method_name];
      return { result: [...Buffer.from(JSON.stringify(result))] };
    },
  };
  const wallet = { selector: {
    options: { provider },
    store: { getState: () => ({ accounts: [{ accountId: 'seller.testnet', active: true }] }) },
  } };
  const readiness = await reviewedContract.verifyListingPreconditions({
    nftContractId: 'nft.roundfrog-dev.testnet',
    tokenId: 'token-1',
    sellerId: 'seller.testnet',
    wallet,
  });
  assert.equal(readiness.config.trading_enabled, true);
  assert.equal(readiness.token.owner_id, 'seller.testnet');
  assert.deepEqual(requests.filter(({ request_type }) => request_type === 'view_account')
    .map(({ account_id, finality }) => [account_id, finality]).sort(), [
    ['market.roundfrog-dev.testnet', 'final'],
    ['nft.roundfrog-dev.testnet', 'final'],
  ]);
  assert.deepEqual(requests.filter(({ request_type }) => request_type === 'call_function')
    .map(({ method_name }) => method_name).sort(), ['config', 'marketplace_id', 'nft_token']);
});

test('listing readiness rejects unreviewed code and mint readiness requires the configured authority', async () => {
  const createWallet = (wrongCodeHash = false) => {
    const provider = {
      async query(request) {
        if (request.request_type === 'view_account') {
          return { code_hash: wrongCodeHash ? 'changed-hash' :
            request.account_id === 'nft.roundfrog-dev.testnet' ? 'nft-reviewed-hash' : 'market-reviewed-hash' };
        }
        const value = request.method_name === 'marketplace_id'
          ? 'market.roundfrog-dev.testnet' : 'seller.roundfrog-dev.testnet';
        return { result: [...Buffer.from(JSON.stringify(value))] };
      },
    };
    return { selector: {
      options: { provider },
      store: { getState: () => ({ accounts: [{ accountId: 'seller.roundfrog-dev.testnet', active: true }] }) },
    } };
  };
  await assert.rejects(reviewedContract.verifyListingPreconditions({
    nftContractId: 'nft.roundfrog-dev.testnet', tokenId: 'token-1',
    sellerId: 'seller.roundfrog-dev.testnet', wallet: createWallet(true),
  }), /does not match the reviewed Testnet contract/);
  const result = await reviewedContract.verifyMintPreconditions({
    signerId: 'seller.roundfrog-dev.testnet', wallet: createWallet(),
  });
  assert.equal(result.mintAuthority, 'seller.roundfrog-dev.testnet');
  await assert.rejects(reviewedContract.verifyMintPreconditions({
    signerId: 'another.testnet', wallet: createWallet(),
  }), /wallet changed during this transaction/);
});

test('favorite reloads reset on user/wallet changes and discard out-of-order reads', async () => {
  const b = backend();
  const oldRead = deferred();
  b.state.respond = () => oldRead.promise;
  const f = await favorites(b);
  f.render();
  await turn();
  f.switchUser(bob, 'bob.testnet');
  b.state.respond = () => reply([]);
  f.render();
  await f.settle();
  oldRead.resolve(reply([{ nft_id: nft.id }]));
  assert.equal((await f.settle()).likedNFTs.size, 0);
  assert.equal(b.calls.at(-1).url.searchParams.get('user_id'), `eq.${bob.id}`);

  b.state.respond = () => reply([{ nft_id: nft.id }]);
  f.switchUser(bob, 'another-wallet.testnet');
  f.render();
  assert.equal((await f.settle()).likedNFTs.has(nft.id), true);
  f.switchUser(null, null);
  assert.equal(f.render(false).likedNFTs.size, 0, 'hide old favorites before effects commit');
  f.commit();
  assert.equal(f.render().likedNFTs.size, 0);
  f.unmount();
});

test('a delayed favorite write cannot overwrite a new user or a different detail page', async () => {
  for (const failed of [false, true]) {
    for (const changeIdentity of [
      (f) => f.switchUser(bob, 'bob.testnet'),
      (f) => f.setScope('another-nft'),
      (f) => f.unmount(),
    ]) {
      const b = backend();
      const f = await favorites(b);
      f.render();
      const view = await f.settle();
      const pending = deferred();
      b.state.respond = () => pending.promise;
      const saving = view.toggleLike(nft.id);
      await turn();
      changeIdentity(f);
      b.state.respond = () => reply([]);
      f.render();
      await f.settle();
      pending.resolve(failed ? reply({ message: 'previous user write denied', code: '42501' }, 403)
        : reply({ nft_id: nft.id, user_id: alice.id }));
      assert.equal(await saving, null);
      assert.equal(f.render().likedNFTs.size, 0);
      assert.equal(f.render().likeError, '', 'late errors must not leak into the next identity');
      f.unmount();
    }
  }
});

test('favorite read errors are exposed without leaving another user selected', async () => {
  const b = backend();
  b.state.respond = () => reply([{ nft_id: nft.id }]);
  const f = await favorites(b);
  f.render();
  assert.equal((await f.settle()).likedNFTs.has(nft.id), true);
  f.switchUser(bob, 'bob.testnet');
  b.state.respond = () => reply({ message: 'read failed' }, 500);
  f.render();
  const view = await f.settle();
  assert.equal(view.likedNFTs.size, 0);
  assert.match(view.likeError, /read failed/);
  f.unmount();
});

async function nftService(b) {
  return loadModule('../frontend/Nft/src/services/nftService.js', {
    '../lib/supabase': { supabase: b.supabase }, '../lib/near/utils': amounts,
  });
}

test('NFT deletion requires the returned ID and leaves append-only artwork alone', async () => {
  const b = backend();
  const service = await nftService(b);
  b.state.respond = () => reply({ id: nft.id });
  assert.equal(await service.deleteNFT(nft), true);
  assert.equal(b.calls.length, 1, 'no browser Storage cleanup is permitted');
  assert.equal(b.calls[0].method, 'DELETE');
  assert.equal(b.calls[0].url.searchParams.get('id'), `eq.${nft.id}`);
  assert.equal(b.calls[0].url.searchParams.get('creator_id'), `eq.${alice.id}`);
  assert.equal(b.calls[0].url.searchParams.get('select'), 'id');
  for (const respond of [
    () => reply(null), () => reply([]), () => reply({ id: bob.id }),
    () => reply({ code: 'PGRST116', message: 'Cannot coerce to a single JSON object', details: 'The result contains 0 rows' }, 406),
    () => reply({ message: 'permission denied', code: '42501' }, 403),
    () => reply({ message: 'listing history exists', code: '23503' }, 409),
  ]) {
    b.state.respond = respond;
    await assert.rejects(service.deleteNFT(nft));
    assert.ok(b.calls.every((call) => call.url.pathname === '/rest/v1/nfts'));
  }
});

test('delete eligibility matches creator/current-owner, unminted source and all listing history rules', async () => {
  const { getNFTDeletionError } = await nftService(backend());
  assert.equal(getNFTDeletionError(nft, alice.id), '');
  for (const [candidate, userId] of [
    [nft, null], [nft, bob.id], [{ ...nft, owner_id: bob.id }, alice.id],
    [{ ...nft, creator_id: bob.id }, alice.id], [{ ...nft, source: 'game_reward' }, alice.id],
    [{ ...nft, token_id: 'token-1', nft_contract_id: 'nft.testnet' }, alice.id],
    [{ ...nft, token_id: undefined }, alice.id], [{ ...nft, listing_history: undefined }, alice.id],
    [{ ...nft, source: undefined }, alice.id], [{ ...nft, nft_contract_id: undefined }, alice.id],
    ...['pending', 'active', 'cancelled', 'sold'].map((status) => [
      { ...nft, listing_history: [{ id: 'listing-id', status, seller_id: bob.id }] }, alice.id,
    ]),
  ]) assert.ok(getNFTDeletionError(candidate, userId));
});

test('NFT deletion refuses changed, missing or failed Auth sessions before a mutation', async () => {
  const b = backend();
  const service = await nftService(b);
  for (const response of [
    { data: { session: null }, error: null },
    { data: { session: { user: alice } }, error: null },
    { data: { session: { user: bob, access_token: 'offline-token' } }, error: null },
    { data: { session: null }, error: { message: 'session unavailable' } },
  ]) {
    b.supabase.auth.getSession = async () => response;
    await assert.rejects(service.deleteNFT(nft));
  }
  assert.deepEqual(b.calls, []);
});

function* walk(value) {
  if (!value || typeof value !== 'object') return;
  if (value.type) yield value;
  for (const child of Object.values(value)) {
    if (Array.isArray(child)) { for (const entry of child) yield* walk(entry); }
    else if (child && typeof child === 'object') yield* walk(child);
  }
}
async function page(name) {
  const source = await readFile(new URL(`../frontend/Nft/src/pages/${name}`, import.meta.url), 'utf8');
  return { source, nodes: [...walk(parse(source, { ecmaVersion: 'latest', sourceType: 'module', range: true, ecmaFeatures: { jsx: true } }))] };
}
const [collection, detail, market, dashboard] = await Promise.all(
  ['createNft.jsx', 'nftDetails.jsx', 'market.jsx', 'dashboard.jsx'].map(page),
);
function handler(page, name, context) {
  const node = page.nodes.find((node) => node.type === 'VariableDeclarator' && node.id.name === name);
  assert.ok(node, `${name} must exist`);
  return vm.runInNewContext(`(${page.source.slice(...node.init.range)})`, { console: quietConsole, ...context });
}

test('a zero-row NFT delete keeps the collection and shows an error; a confirmed delete removes it', async () => {
  for (const deleted of [false, true]) {
    const b = backend();
    b.state.respond = () => reply(deleted ? { id: nft.id } : null);
    const service = await nftService(b);
    const state = { nfts: [nft], success: 'Old success', error: '' };
    const remove = handler(collection, 'handleDelete', {
      ...service, user: alice, deleting: null, window: { confirm: () => true },
      setDeleting() {}, setError: (value) => { state.error = value; },
      setSuccess: (value) => { state.success = value; },
      setNfts: (update) => { state.nfts = update(state.nfts); },
    });
    await remove(nft);
    assert.equal(state.nfts.length, deleted ? 0 : 1);
    assert.equal(Boolean(state.success), deleted);
    assert.equal(Boolean(state.error), !deleted);
  }
});

test('delete buttons and forced callbacks reject minted NFTs and any listing history', async () => {
  const { getNFTDeletionError } = await nftService(backend());
  const button = collection.nodes.find((node) => node.type === 'JSXOpeningElement' && node.name.name === 'button' &&
    node.attributes.some((attr) => attr.name?.name === 'onClick' &&
      [...walk(attr)].some((child) => child.type === 'Identifier' && child.name === 'handleDelete')));
  const disabled = button.attributes.find((attr) => attr.name?.name === 'disabled').value.expression;
  for (const candidate of [
    nft, { ...nft, owner_id: bob.id }, { ...nft, source: 'game_reward' },
    { ...nft, token_id: 'token-1', nft_contract_id: 'nft.testnet' },
    ...['pending', 'active', 'cancelled', 'sold'].map((status) => ({ ...nft, listing_history: [{ id: 'listing', status }] })),
  ]) {
    const deletionError = getNFTDeletionError(candidate, alice.id);
    assert.equal(vm.runInNewContext(`(${collection.source.slice(...disabled.range)})`, {
      deleting: null, deletionError,
    }), candidate !== nft);
    if (!deletionError) continue;
    let error = '';
    let success = 'Old success';
    await handler(collection, 'handleDelete', {
      getNFTDeletionError, user: alice, deleting: null,
      window: { confirm: assert.fail }, deleteNFT: assert.fail,
      setError: (value) => { error = value; }, setSuccess: (value) => { success = value; },
    })(candidate);
    assert.equal(error, deletionError);
    assert.equal(success, '');
  }
});

test('owned NFT reads include deletion evidence and all three pages use the shared favorites path', async () => {
  const b = backend();
  b.state.respond = () => reply([{ owner_id: alice.id, nfts: nft }]);
  let owned;
  await handler(collection, 'fetchNFTs', {
    ...amounts, userId: alice.id, useCallback: (callback) => callback,
    supabase: b.supabase, setLoadingNFTs() {},
    setNfts: (value) => { owned = value; }, setError: assert.fail,
  })();
  const select = b.calls[0].url.searchParams.get('select');
  assert.match(select, /owner_id/);
  assert.match(select, /source,token_id,nft_contract_id/);
  assert.match(select, /listing_history:marketplace_listings\(id\)/);
  assert.equal(b.calls[0].url.searchParams.has('status'), false);
  assert.equal(owned[0].owner_id, alice.id);
  for (const view of [detail, market, dashboard]) {
    assert.match(view.source, /useNFTLikes\(/);
    assert.doesNotMatch(view.source, /user_id:\s*walletAddress|eq\("user_id",\s*walletAddress\)/);
    assert.ok(view.nodes.some((node) => node.type === 'JSXExpressionContainer' && node.expression.name === 'likeError') ||
      view.source.includes('likeError || likesReadError') || view.source.includes('error || likeError'));
  }
});

test('page like counts and refreshes change only when the shared write succeeds', async () => {
  for (const result of [null, { liked: true, change: 1 }]) {
    let count = 4;
    let refreshes = 0;
    await handler(detail, 'handleLike', {
      id: nft.id, toggleLike: async () => result, setError() {}, setSuccess() {},
      setLikes: (update) => { count = update(count); },
    })();
    assert.equal(count, result ? 5 : 4);
    let counts = { [nft.id]: 4 };
    await handler(market, 'toggleLike', {
      updateLike: async () => result, setLikeCounts: (update) => { counts = update(counts); },
    })(nft.id);
    assert.equal(counts[nft.id], result ? 5 : 4);
    await handler(dashboard, 'handleLike', {
      user: alice, toggleLike: async () => result,
      fetchTrendingNFTs: async () => { refreshes++; }, navigate: assert.fail,
    })(nft.id);
    assert.equal(refreshes, result ? 1 : 0);
  }
});

test('failed catalog creation retains the uploaded image without success or forbidden cleanup', async () => {
  const state = { nfts: [nft], success: 'Old success', error: '' };
  await handler(collection, 'handleSubmit', {
    ...amounts, user: alice, loading: false, image: {}, validateImage: () => null,
    form: { name: 'Artwork', description: '', category: 'Art', price: '1' },
    uploadNFTImage: async () => ({ publicUrl: 'https://consistency.example/art.png', filePath: nft.image_path }),
    createNFT: async () => { throw new Error('NFT write denied'); },
    deleteNFTImage: assert.fail, setNfts: assert.fail, setForm: assert.fail, removeImage: assert.fail,
    setLoading() {}, setUploading() {},
    setError: (value) => { state.error = value; }, setSuccess: (value) => { state.success = value; },
  })({ preventDefault() {} });
  assert.equal(state.success, '');
  assert.match(state.error, /NFT write denied/);
  assert.match(state.error, /uploaded image is retained/);
  assert.equal(state.nfts.length, 1);
});

test('on-chain cancellation waits for trusted indexing instead of fabricating catalog state', () => {
  const node = collection.nodes.find((entry) =>
    entry.type === 'VariableDeclarator' && entry.id.name === 'handleCancelListing');
  assert.ok(node);
  const source = collection.source.slice(...node.init.range);
  assert.doesNotMatch(source, /setListings\s*\(/);
  assert.match(source, /fetchListings\s*\(/);
  assert.match(source, /trusted indexer/i);
});
