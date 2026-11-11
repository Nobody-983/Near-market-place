import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { test } from 'node:test';
import vm from 'node:vm';

const require = createRequire(new URL('../frontend/Nft/package.json', import.meta.url));
const { parse } = require('espree');

const marketplaceSql = await readFile(
  new URL('../supabase/migrations/20261029000100_near_marketplace.sql', import.meta.url),
  'utf8',
);

const gameJsx = await readFile(
  new URL('../frontend/Nft/src/pages/game/Game.jsx', import.meta.url),
  'utf8',
);

function functionDefinition(sql, name) {
  const start = sql.indexOf(`create function ${name}(`);
  const end = sql.indexOf('\n$$;', start);
  assert.ok(start >= 0 && end > start, `${name} must exist in migration`);
  return sql.slice(start, end).replace(/--[^\n]*/g, '');
}

const claimGameRewardSql = functionDefinition(marketplaceSql, 'public.claim_game_reward');

test('game milestone levels match the required tiers and tap counts', () => {
  assert.ok(gameJsx.includes('level: 1'));
  assert.ok(gameJsx.includes('tapsRequired: 500'));
  assert.ok(gameJsx.includes('reward: "Bronze NFT"'));

  assert.ok(gameJsx.includes('level: 2'));
  assert.ok(gameJsx.includes('tapsRequired: 2000'));
  assert.ok(gameJsx.includes('reward: "Silver NFT"'));

  assert.ok(gameJsx.includes('level: 3'));
  assert.ok(gameJsx.includes('tapsRequired: 5000'));
  assert.ok(gameJsx.includes('reward: "Gold NFT"'));
});

test('claim_game_reward SQL enforces verified wallet session and authenticated identity', () => {
  assert.match(
    claimGameRewardSql,
    /if v_user is null or not marketplace_private\.has_verified_wallet_session\(\) then\s+raise exception 'A verified wallet session is required' using errcode = '42501';/
  );
});

test('claim_game_reward SQL strictly validates reward levels and thresholds', () => {
  assert.match(claimGameRewardSql, /when 1 then v_required := 500;\s+v_name := 'Bronze NFT';/);
  assert.match(claimGameRewardSql, /when 2 then v_required := 2000;\s+v_name := 'Silver NFT';/);
  assert.match(claimGameRewardSql, /when 3 then v_required := 5000;\s+v_name := 'Gold NFT';/);
  assert.match(claimGameRewardSql, /else raise exception 'Reward level must be 1, 2, or 3' using errcode = '22023';/);
});

test('claim_game_reward SQL prevents duplicate claims idempotently', () => {
  assert.match(
    claimGameRewardSql,
    /select nft_id into v_nft from public\.reward_claims where user_id = v_user and reward_level = p_reward_level;\s+if found then return v_nft; end if;/
  );
});

test('claim_game_reward SQL checks tap count requirement and marks NFT as game_reward', () => {
  assert.match(
    claimGameRewardSql,
    /if v_taps < v_required then\s+raise exception 'Not enough taps for this reward' using errcode = '23514';/
  );
  assert.match(
    claimGameRewardSql,
    /insert into public\.nfts\(creator_id, name, description, image_url, category, price, source\)/
  );
  assert.match(
    claimGameRewardSql,
    /values \(v_user, v_name, 'Off-chain NEAR Tap Game collectible; no on-chain token or cash value\.',\s+'\/rewards\/' \|\| p_reward_level::text \|\| '\.jpeg', 'Gaming', 0, 'game_reward'\)/
  );
  assert.match(
    claimGameRewardSql,
    /insert into public\.reward_claims\(user_id, reward_level, nft_id\) values \(v_user, p_reward_level, v_nft\);/
  );
});

test('game reward NFTs are restricted from on-chain listing and deletion', () => {
  assert.match(
    marketplaceSql,
    /constraint game_rewards_are_offchain check \(source <> 'game_reward' or token_id is null\)/
  );
  assert.match(
    marketplaceSql,
    /if v_nft\.source = 'game_reward' or v_nft\.network_id <> new\.network_id then\s+raise exception 'NFT is not eligible for this marketplace listing'/
  );
});

test('Game component progress calculation functions accurately reflect milestone advancement', () => {
  const tree = parse(gameJsx, {
    ecmaVersion: 'latest', sourceType: 'module', range: true, ecmaFeatures: { jsx: true },
  });

  function* walk(node) {
    if (!node || typeof node !== 'object') return;
    if (node.type) yield node;
    for (const child of Object.values(node)) {
      if (Array.isArray(child)) { for (const c of child) yield* walk(c); }
      else if (child && typeof child === 'object') yield* walk(child);
    }
  }

  const nodes = [...walk(tree)];
  const getProgressDecl = nodes.find(n => n.type === 'VariableDeclarator' && n.id.name === 'getProgress');
  const getRemainingDecl = nodes.find(n => n.type === 'VariableDeclarator' && n.id.name === 'getRemainingTaps');

  assert.ok(getProgressDecl, 'getProgress must exist');
  assert.ok(getRemainingDecl, 'getRemainingTaps must exist');

  const context = { tapCount: 250 };
  const getProgress = vm.runInNewContext(`(${gameJsx.slice(...getProgressDecl.init.range)})`, context);
  const getRemainingTaps = vm.runInNewContext(`(${gameJsx.slice(...getRemainingDecl.init.range)})`, context);

  const bronze = { tapsRequired: 500 };
  assert.equal(getProgress(bronze), 50);
  assert.equal(getRemainingTaps(bronze), 250);

  context.tapCount = 500;
  assert.equal(getProgress(bronze), 100);
  assert.equal(getRemainingTaps(bronze), 0);

  context.tapCount = 1000;
  assert.equal(getProgress(bronze), 100);
  assert.equal(getRemainingTaps(bronze), 0);
});

test('Game claimReward client handler validates preconditions before invoking RPC', () => {
  const tree = parse(gameJsx, {
    ecmaVersion: 'latest', sourceType: 'module', range: true, ecmaFeatures: { jsx: true },
  });

  function* walk(node) {
    if (!node || typeof node !== 'object') return;
    if (node.type) yield node;
    for (const child of Object.values(node)) {
      if (Array.isArray(child)) { for (const c of child) yield* walk(c); }
      else if (child && typeof child === 'object') yield* walk(child);
    }
  }

  const nodes = [...walk(tree)];
  const claimDecl = nodes.find(n => n.type === 'VariableDeclarator' && n.id.name === 'claimReward');
  assert.ok(claimDecl, 'claimReward must exist');

  // Verify claimReward returns early when user is not logged in or taps < tapsRequired
  let rpcCalled = false;
  const mockSupabase = {
    rpc: async () => {
      rpcCalled = true;
      return { data: 'test-nft-id', error: null };
    },
  };

  const bronze = { level: 1, tapsRequired: 500 };

  // 1. Unauthenticated user
  const unauthContext = {
    user: null,
    tapCount: 600,
    hasClaimedReward: () => false,
    setClaimingLevel: () => {},
    supabase: mockSupabase,
    loadGameData: async () => {},
  };
  const claimUnauth = vm.runInNewContext(`(${gameJsx.slice(...claimDecl.init.range)})`, unauthContext);
  claimUnauth(bronze);
  assert.equal(rpcCalled, false, 'RPC must not be called when unauthenticated');

  // 2. Insufficient taps
  const lowTapsContext = {
    user: { id: 'user-1' },
    tapCount: 400,
    hasClaimedReward: () => false,
    setClaimingLevel: () => {},
    supabase: mockSupabase,
    loadGameData: async () => {},
  };
  const claimLowTaps = vm.runInNewContext(`(${gameJsx.slice(...claimDecl.init.range)})`, lowTapsContext);
  claimLowTaps(bronze);
  assert.equal(rpcCalled, false, 'RPC must not be called when taps are insufficient');

  // 3. Already claimed
  const alreadyClaimedContext = {
    user: { id: 'user-1' },
    tapCount: 600,
    hasClaimedReward: () => true,
    setClaimingLevel: () => {},
    supabase: mockSupabase,
    loadGameData: async () => {},
  };
  const claimAlready = vm.runInNewContext(`(${gameJsx.slice(...claimDecl.init.range)})`, alreadyClaimedContext);
  claimAlready(bronze);
  assert.equal(rpcCalled, false, 'RPC must not be called when already claimed');

  // 4. Eligible caller
  let reloaded = false;
  const eligibleContext = {
    user: { id: 'user-1' },
    tapCount: 600,
    hasClaimedReward: () => false,
    setClaimingLevel: () => {},
    supabase: mockSupabase,
    loadGameData: async () => { reloaded = true; },
  };
  const claimEligible = vm.runInNewContext(`(${gameJsx.slice(...claimDecl.init.range)})`, eligibleContext);
  claimEligible(bronze).then(() => {
    assert.equal(rpcCalled, true, 'RPC must be called when eligible');
    assert.equal(reloaded, true, 'Game data must reload after successful claim');
  });
});

