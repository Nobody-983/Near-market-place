// Offline withdrawal/refund reconciliation regressions: synthetic NEAR RPC
// fixtures with exact receipt graphs, service RPC calls and state checks.
// Run: node tests/withdrawals.test.mjs
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const { WithdrawalVerifier, WithdrawalReconciler, RejectedWithdrawal } = require('../services/settlement/withdrawals.js');
const { scanBlock } = require('../services/settlement/index.js');

const H = {
  tx: '6RWmTYhXCzjMjoY3Mz1rfFcnBm8E6XeDDbFEPUA4sv1w',
  call: '9SP8Y3sVADWNN5QoEB5CsvPUE5HT4o8YfBaCnhLss87K',
  transfer: '8ZD92cLpoCEU46hPGFfk3VqZpU8s6DoQhZ4pCMqWwDT6',
  callback: 'BoQHueiPH9e4C7fkxouV4tFpGZ4hK5fJhKQnPBWwPazS',
  inclusion: '3rEx3xmgLCRgUfSgueD71YNrJYQYNvhtkXaqVQxdmj4U',
  parent: 'HDgWEk2okmDdAFAVf6ffxGBH6F6vdLM1X3H5Fmaafe4S',
  callbackBlock: 'J3KbUXF9YPu2eGnbDCACxGvmMDZMdP7acGYhVLHGu9y2',
  final: 'e2XGEosf843XMiCJHvZufvHKyw419ZYibDBdVJQr9cB',
  data: 'CzPafxtJmM1FnRoasKWAVhceJzZzkz9RKUBQQ4kY9V1v',
};
const heights = new Map([[H.parent, 99], [H.inclusion, 100], [H.callbackBlock, 101], [H.final, 110]]);
const byHeight = new Map([[99, H.parent], [100, H.inclusion], [101, H.callbackBlock], [110, H.final]]);
const codeHash = 'F1LjFY79WBVaTiLJ2hExa7rTBwZJWvm38aB7prE6w1us';
const nftCodeHash = 'CzPafxtJmM1FnRoasKWAVhceJzZzkz9RKUBQQ4kY9V1v';
const market = 'market.testnet';
const nft = 'nft.testnet';
const b64 = (value) => Buffer.from(JSON.stringify(value)).toString('base64');
const truth = Buffer.from('true').toString('base64');
const falsity = Buffer.from('false').toString('base64');
const ZERO_LEDGER = {
  seller_liability: '0', fee_liability: '0', refund_liability: '0',
  pending_sale_liability: '0', withdrawal_in_flight: '0', storage_liability: '0',
  seller_withdrawn: '0', fee_withdrawn: '0', refunds_withdrawn: '0',
  control_storage_reserve: '0',
};

function event(eventName, data) {
  return `EVENT_JSON:${JSON.stringify({ standard: 'near_marketplace', version: '1.0.0', event: eventName, data })}`;
}

function body(id, predecessor, receiver, actions, options = {}) {
  return {
    receipt_id: id,
    predecessor_id: predecessor,
    receiver_id: receiver,
    receipt: {
      Action: {
        signer_id: predecessor,
        signer_public_key: 'ed25519:11111111111111111111111111111111',
        gas_price: '0',
        output_data_receivers: options.output || [],
        input_data_ids: options.input || [],
        actions,
        is_promise_yield: false,
      },
    },
  };
}

function scenario({
  method = 'withdraw_proceeds', args = {}, deposit = '1', signer = 'seller.testnet',
  succeeded = true, amount, nonce = '1', kind = 'seller_proceeds',
  callReturn = null, eventKind, view,
}) {
  const tx = {
    hash: H.tx, signer_id: signer, receiver_id: market,
    actions: [{
      FunctionCall: {
        method_name: method, args: b64(args), gas: 60_000_000_000_000, deposit,
      },
    }],
  };
  const eventName = succeeded ? 'withdrawal_succeeded' : 'withdrawal_failed';
  const data = succeeded
    ? { account_id: args.account_id || signer, recipient_id: signer, amount, nonce, kind }
    : { account_id: args.account_id || signer, amount, nonce, kind };
  const callbackActions = [{
    FunctionCall: {
      method_name: 'on_withdrawal_result',
      args: b64({ account_id: args.account_id || signer, nonce }),
      gas: 20_000_000_000_000,
      deposit: '0',
    },
  }];
  const callStatus = callReturn
    ? { SuccessValue: b64(callReturn) }
    : { SuccessReceiptId: H.callback };
  const result = {
    final_execution_status: 'FINAL',
    transaction: tx,
    status: callReturn ? { SuccessValue: b64(callReturn) } : { SuccessValue: succeeded ? truth : falsity },
    transaction_outcome: {
      id: tx.hash, block_hash: H.inclusion,
      outcome: {
        executor_id: signer, logs: [], receipt_ids: [H.call],
        status: { SuccessReceiptId: H.call },
      },
    },
    receipts_outcome: [
      {
        id: H.call, block_hash: H.inclusion,
        outcome: {
          executor_id: market, logs: [], receipt_ids: [H.transfer, H.callback], status: callStatus,
        },
      },
      {
        id: H.transfer, block_hash: H.callbackBlock,
        outcome: {
          executor_id: signer, logs: [], receipt_ids: [],
          status: succeeded ? { SuccessValue: '' }
            : { Failure: { ActionError: { index: 0, kind: 'FunctionCallError' } } },
        },
      },
      {
        id: H.callback, block_hash: H.callbackBlock,
        outcome: {
          executor_id: market, logs: [event(eventKind || eventName, data)], receipt_ids: [],
          status: { SuccessValue: succeeded ? truth : falsity },
        },
      },
    ],
    receipts: [
      body(H.call, signer, market, tx.actions),
      body(H.transfer, market, signer, [{ Transfer: { deposit: amount } }], {
        output: [{ data_id: H.data, receiver_id: market }],
      }),
      body(H.callback, market, market, callbackActions, { input: [H.data] }),
    ],
  };
  const rpc = async (rpcMethod, params) => {
    if (rpcMethod === 'EXPERIMENTAL_tx_status') return structuredClone(result);
    if (rpcMethod === 'EXPERIMENTAL_receipt') {
      const receipt = result.receipts.find((entry) => entry.receipt_id === params.receipt_id);
      assert.ok(receipt, `unexpected receipt request ${params.receipt_id}`);
      return structuredClone(receipt);
    }
    if (rpcMethod === 'block') {
      if (params.finality === 'final') return { header: { hash: H.final, height: 110 } };
      if (params.block_id === H.final) return { header: { hash: H.final, height: 110 } };
      const hash = typeof params.block_id === 'number' ? byHeight.get(params.block_id) : params.block_id;
      if (!heights.has(hash)) throw new Error(`unexpected block ${params.block_id}`);
      return {
        header: {
          hash, height: heights.get(hash),
          ...(hash === H.inclusion ? { prev_hash: H.parent } : {}),
        },
      };
    }
    if (rpcMethod === 'query' && params.request_type === 'view_account') {
      return { block_hash: params.block_id, block_height: heights.get(params.block_id), code_hash: codeHash };
    }
    if (rpcMethod === 'query' && params.request_type === 'call_function') {
      const parsedArgs = JSON.parse(Buffer.from(params.args_base64, 'base64').toString('utf8'));
      return {
        block_hash: params.block_id, block_height: heights.get(params.block_id),
        logs: [], result: [...Buffer.from(JSON.stringify(view(params.method_name, parsedArgs, params.block_id)))],
      };
    }
    throw new Error(`Unexpected RPC ${rpcMethod}`);
  };
  return { tx, result, rpc };
}

function verifier(rpc) {
  return new WithdrawalVerifier({
    networkId: 'testnet', rpc, marketplaceContractId: market, nftContractId: nft,
    marketplaceCodeHash: codeHash, nftCodeHash,
  });
}

function run(overrides) {
  const s = scenario(overrides);
  return verifier(s.rpc).verifyTransaction(s.tx, { hash: H.inclusion, height: 100 });
}

function baseView(overrides = {}) {
  const values = {
    pending_withdrawal: null,
    ledger: ZERO_LEDGER,
    ...overrides,
  };
  return values;
}

test('successful seller withdrawal is bound to the callback, transfer and final state', async () => {
  const preLedger = { ...ZERO_LEDGER, seller_liability: '2000' };
  const postLedger = { ...ZERO_LEDGER, seller_liability: '1000', seller_withdrawn: '1000' };
  const views = {
    [H.parent]: baseView({ ledger: preLedger, seller_proceeds: '2000' }),
    [H.callbackBlock]: baseView({ ledger: postLedger, seller_proceeds: '1000' }),
  };
  const facts = await run({
    amount: '1000', kind: 'seller_proceeds', args: { amount: '1000' },
    view: (method, args, block) => {
      assert.deepEqual(args, method === 'seller_proceeds' ? { seller_id: 'seller.testnet' } : args);
      const value = views[block][method];
      assert.notEqual(value, undefined, `unexpected view ${method} at ${block}`);
      return value;
    },
  });
  assert.equal(facts.outcome, 'succeeded');
  assert.equal(facts.kind, 'seller_proceeds');
  assert.equal(facts.amountYocto, '1000');
  assert.equal(facts.nonce, '1');
  assert.equal(facts.accountWalletAddress, 'seller.testnet');
  assert.equal(facts.recipientWalletAddress, 'seller.testnet');
  assert.equal(facts.callbackBlockHash, H.callbackBlock);
  assert.equal(facts.transferReceiptId, H.transfer);
});

test('failed transfer records a failure only after restored balances and cleared pending state', async () => {
  const ledger = { ...ZERO_LEDGER, fee_liability: '500' };
  const views = {
    [H.parent]: baseView({ ledger, fee_proceeds: '500' }),
    [H.callbackBlock]: baseView({ ledger, fee_proceeds: '500' }),
  };
  const facts = await run({
    method: 'withdraw_fees', args: { amount: '500' },
    amount: '500', kind: 'marketplace_fees', succeeded: false,
    view: (method, _args, block) => views[block][method],
  });
  assert.equal(facts.outcome, 'failed');
  assert.equal(facts.kind, 'marketplace_fees');
});

test('registration-excess refund verifies the deposit delta and storage liability', async () => {
  const views = {
    [H.parent]: baseView({ storage_balance_of: null }),
    [H.callbackBlock]: baseView({
      storage_balance_of: { total: '500', used: '500', available: '0' },
      ledger: { ...ZERO_LEDGER, storage_liability: '0' },
    }),
  };
  const facts = await run({
    method: 'storage_deposit', deposit: '5000', kind: 'registration_excess', amount: '4500',
    args: { registration_only: true },
    callReturn: { total: '500', used: '500', available: '0' },
    view: (method, _args, block) => views[block][method],
  });
  assert.equal(facts.kind, 'registration_excess');
  assert.equal(facts.amountYocto, '4500');
});

test('failed registration refund keeps the deposit and restored liability', async () => {
  const views = {
    [H.parent]: baseView({ storage_balance_of: null }),
    [H.callbackBlock]: baseView({
      storage_balance_of: { total: '5000', used: '500', available: '4500' },
      ledger: { ...ZERO_LEDGER, storage_liability: '4500' },
    }),
  };
  const facts = await run({
    method: 'storage_deposit', deposit: '5000', kind: 'registration_excess', amount: '4500',
    args: { account_id: 'seller.testnet', registration_only: true },
    callReturn: { total: '500', used: '500', available: '0' }, succeeded: false,
    view: (method, _args, block) => views[block][method],
  });
  assert.equal(facts.outcome, 'failed');
});

test('storage withdrawal debits exactly the requested amount', async () => {
  const views = {
    [H.parent]: baseView({
      storage_balance_of: { total: '2000', used: '500', available: '1500' },
      ledger: { ...ZERO_LEDGER, storage_liability: '1500' },
    }),
    [H.callbackBlock]: baseView({
      storage_balance_of: { total: '1250', used: '500', available: '750' },
      ledger: { ...ZERO_LEDGER, storage_liability: '750' },
    }),
  };
  const facts = await run({
    method: 'storage_withdraw', kind: 'storage_balance', amount: '750',
    args: { amount: '750' }, view: (method, _args, block) => views[block][method],
  });
  assert.equal(facts.kind, 'storage_balance');
});

test('buyer refund success requires the settlement to become refunded', async () => {
  const settlement = {
    sale_id: 'sale-1', buyer: 'seller.testnet', price: '1000', status: 'refund_available',
  };
  const views = {
    [H.parent]: baseView({
      settlement, buyer_refund: '1000',
      ledger: { ...ZERO_LEDGER, refund_liability: '1000' },
    }),
    [H.callbackBlock]: baseView({
      settlement: { ...settlement, status: 'refunded' }, buyer_refund: '0',
      ledger: { ...ZERO_LEDGER, refund_liability: '0', refunds_withdrawn: '1000' },
    }),
  };
  const facts = await run({
    method: 'withdraw_refund', kind: 'buyer_refund', amount: '1000',
    args: { sale_id: 'sale-1' }, view: (method, _args, block) => views[block][method],
  });
  assert.equal(facts.referenceSaleId, 'sale-1');
  assert.equal(facts.kind, 'buyer_refund');
});

test('tampered event recipient, amount, nonce and failed-transfer state are rejected', async () => {
  const preLedger = { ...ZERO_LEDGER, seller_liability: '2000' };
  const postLedger = { ...ZERO_LEDGER, seller_liability: '1000', seller_withdrawn: '1000' };
  const views = {
    [H.parent]: baseView({ ledger: preLedger, seller_proceeds: '2000' }),
    [H.callbackBlock]: baseView({ ledger: postLedger, seller_proceeds: '1000' }),
  };
  const view = (method, _args, block) => views[block][method];
  for (const mutate of [
    (result) => {
      const log = JSON.parse(result.receipts_outcome[2].outcome.logs[0].slice('EVENT_JSON:'.length));
      log.data.recipient_id = 'mallory.testnet';
      result.receipts_outcome[2].outcome.logs[0] = `EVENT_JSON:${JSON.stringify(log)}`;
    },
    (result) => {
      const log = JSON.parse(result.receipts_outcome[2].outcome.logs[0].slice('EVENT_JSON:'.length));
      log.data.amount = '999';
      result.receipts_outcome[2].outcome.logs[0] = `EVENT_JSON:${JSON.stringify(log)}`;
    },
    (result) => {
      const log = JSON.parse(result.receipts_outcome[2].outcome.logs[0].slice('EVENT_JSON:'.length));
      log.data.nonce = '2';
      result.receipts_outcome[2].outcome.logs[0] = `EVENT_JSON:${JSON.stringify(log)}`;
    },
    (result) => {
      result.receipts_outcome[2].outcome.status = { SuccessValue: falsity };
      result.status = { SuccessValue: falsity };
    },
  ]) {
    const s = scenario({ amount: '1000', args: { amount: '1000' }, view });
    mutate(s.result);
    const baseRpc = scenario({ amount: '1000', args: { amount: '1000' }, view }).rpc;
    s.rpc = async (method, params) => {
      if (method === 'EXPERIMENTAL_tx_status') return structuredClone(s.result);
      return baseRpc(method, params);
    };
    await assert.rejects(
      verifier(s.rpc).verifyTransaction(s.tx, { hash: H.inclusion, height: 100 }),
      RejectedWithdrawal,
    );
  }
});

test('a failed event with a successful transfer is rejected as inconsistent', async () => {
  const ledger = { ...ZERO_LEDGER, seller_liability: '1000' };
  const views = {
    [H.parent]: baseView({ ledger, seller_proceeds: '1000' }),
    [H.callbackBlock]: baseView({ ledger, seller_proceeds: '1000' }),
  };
  const s = scenario({
    amount: '1000', args: { amount: '1000' }, succeeded: false,
    view: (method, _args, block) => views[block][method],
  });
  s.result.receipts_outcome[1].outcome.status = { SuccessValue: '' };
  await assert.rejects(
    verifier(s.rpc).verifyTransaction(s.tx, { hash: H.inclusion, height: 100 }),
    /Failed withdrawal has a successful transfer/,
  );
});

test('non-canonical execution, unreviewed code and mismatched balances are rejected', async () => {
  const preLedger = { ...ZERO_LEDGER, seller_liability: '2000' };
  const postLedger = { ...ZERO_LEDGER, seller_liability: '1000', seller_withdrawn: '1000' };
  const views = {
    [H.parent]: baseView({ ledger: preLedger, seller_proceeds: '2000' }),
    [H.callbackBlock]: baseView({ ledger: postLedger, seller_proceeds: '1000' }),
  };
  const view = (method, _args, block) => views[block][method];
  const s = scenario({ amount: '1000', args: { amount: '1000' }, view });
  const original = s.rpc;
  s.rpc = async (method, params) => {
    if (method === 'block' && params.block_id === 101) {
      return { header: { hash: H.final, height: 101 } };
    }
    return original(method, params);
  };
  await assert.rejects(verifier(s.rpc).verifyTransaction(s.tx, { hash: H.inclusion, height: 100 }),
    /not canonical/);

  const badCode = scenario({ amount: '1000', args: { amount: '1000' }, view });
  const codeRpc = badCode.rpc;
  badCode.rpc = async (method, params) => {
    if (method === 'query' && params.request_type === 'view_account') {
      return { block_hash: params.block_id, block_height: heights.get(params.block_id), code_hash: nftCodeHash };
    }
    return codeRpc(method, params);
  };
  await assert.rejects(verifier(badCode.rpc).verifyTransaction(badCode.tx, { hash: H.inclusion, height: 100 }),
    /unreviewed marketplace code/);

  const badBalance = scenario({ amount: '1000', args: { amount: '1000' }, view: (method, _args, block) =>
    method === 'seller_proceeds' ? (block === H.parent ? '2000' : '1500') : views[block][method] });
  await assert.rejects(verifier(badBalance.rpc).verifyTransaction(badBalance.tx, { hash: H.inclusion, height: 100 }),
    /debited exactly once/);
});

test('reconciler persists verified facts only and never reports completion on SQL failure', async () => {
  const preLedger = { ...ZERO_LEDGER, seller_liability: '2000' };
  const postLedger = { ...ZERO_LEDGER, seller_liability: '1000', seller_withdrawn: '1000' };
  const views = {
    [H.parent]: baseView({ ledger: preLedger, seller_proceeds: '2000' }),
    [H.callbackBlock]: baseView({ ledger: postLedger, seller_proceeds: '1000' }),
  };
  const facts = await run({ amount: '1000', args: { amount: '1000' }, view: (method, _args, block) => views[block][method] });
  const calls = [];
  const rows = [];
  const supabase = {
    async rpc(name, params) {
      calls.push({ name, params });
      const result = rows.shift() || { data: '11111111-1111-4111-8111-111111111111', error: null };
      return result;
    },
  };
  const reconciler = new WithdrawalReconciler({ networkId: 'testnet', marketplaceContractId: market }, supabase);
  assert.equal((await reconciler.reconcile({ ...facts })).ok, false, 'plain facts are rejected');

  const ok = await reconciler.reconcile(facts);
  assert.deepEqual(ok, { ok: true, id: '11111111-1111-4111-8111-111111111111', outcome: 'succeeded' });
  assert.deepEqual(calls.map((call) => call.name), ['record_withdrawal_intent', 'confirm_withdrawal']);
  assert.equal(calls[0].params.p_amount_yocto, '1000');
  assert.equal(calls[0].params.p_nonce, '1');
  assert.equal(calls[1].params.p_callback_block_hash, H.callbackBlock);

  const failing = new WithdrawalReconciler({ networkId: 'testnet', marketplaceContractId: market }, {
    async rpc(name) {
      return name === 'record_withdrawal_intent'
        ? { data: '11111111-1111-4111-8111-111111111111', error: null }
        : { data: null, error: { message: 'database unavailable' } };
    },
  });
  const failure = await failing.reconcile(facts);
  assert.equal(failure.ok, false);
  assert.match(failure.error, /database unavailable/);
});

test('block scanner routes withdrawal and purchase transactions to their own verifiers', async () => {
  const seen = [];
  const block = {
    header: { hash: H.inclusion, height: 100 },
    chunks: [{ chunk_hash: H.data, shard_id: 0, height_included: 100 }],
  };
  const tx = (method) => ({
    hash: H.tx, signer_id: 'seller.testnet', receiver_id: market,
    actions: [{ FunctionCall: { method_name: method, args: b64({}), gas: 1, deposit: '1' } }],
  });
  const results = await scanBlock({
    rpc: async () => ({ header: { chunk_hash: H.data, height_included: 100 }, transactions: [tx('buy_nft'), tx('withdraw_fees')] }),
    block,
    verifier: {
      matchesPurchase: (transaction) => transaction.actions[0].FunctionCall.method_name === 'buy_nft',
      matchesMarketplace: () => true,
      async verifyTransaction(transaction) { seen.push(['purchase', transaction.actions[0].FunctionCall.method_name]); return { txHash: transaction.hash }; },
    },
    reconciler: { async reconcile() { return { ok: true }; } },
    withdrawalVerifier: {
      matchesWithdrawal: (transaction) => transaction.actions[0].FunctionCall.method_name === 'withdraw_fees',
      async verifyTransaction(transaction) { seen.push(['withdrawal', transaction.actions[0].FunctionCall.method_name]); return { txHash: transaction.hash }; },
    },
    withdrawalReconciler: { async reconcile() { return { ok: true }; } },
  });
  assert.deepEqual(seen, [['purchase', 'buy_nft'], ['withdrawal', 'withdraw_fees']]);
  assert.deepEqual(results.map((entry) => entry.type), ['purchase', 'withdrawal']);
});
