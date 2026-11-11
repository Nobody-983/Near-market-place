// Verifies seller/fee/refund/storage withdrawal receipts and registration-excess
// refunds. Facts enter the reconciler only after a FINAL callback and matching
// historical contract state are proven. Database completion is never inferred
// from a browser claim, a transfer hash or a pending record.

const { isDeepStrictEqual } = require('node:util');
const { ChainVerifier, receiptGraph, executionStatus } = require('./verifier');
const {
  U64_MAX, check, object, exactKeys, accountId, hash, uint, height,
  chainJson, functionCall,
} = require('./validation');

const TRUE_B64 = Buffer.from('true').toString('base64');
const FALSE_B64 = Buffer.from('false').toString('base64');

const WITHDRAWAL_METHODS = new Set([
  'withdraw_proceeds',
  'withdraw_fees',
  'withdraw_refund',
  'storage_withdraw',
  'storage_deposit',
]);

const KIND_BY_METHOD = {
  withdraw_proceeds: 'seller_proceeds',
  withdraw_fees: 'marketplace_fees',
  withdraw_refund: 'buyer_refund',
  storage_withdraw: 'storage_balance',
  storage_deposit: 'registration_excess',
};

const KINDS = new Set(Object.values(KIND_BY_METHOD));
const verifiedFacts = new WeakSet();
const isVerifiedWithdrawalFacts = (facts) => object(facts) && verifiedFacts.has(facts);

class RejectedWithdrawal extends Error {
  constructor(message) { super(message); this.name = 'RejectedWithdrawal'; }
}

function subsetKeys(value, keys) {
  return object(value) && Object.keys(value).every((key) => keys.includes(key));
}

function big(value) { return BigInt(value); }
function canonical(value) { return BigInt(value).toString(); }

function parseMarketplaceEvent(log, expectedEvent) {
  check(typeof log === 'string' && log.startsWith('EVENT_JSON:'), 'Missing withdrawal event');
  const event = chainJson(log.slice('EVENT_JSON:'.length));
  check(exactKeys(event, ['standard', 'version', 'event', 'data']) &&
    event.standard === 'near_marketplace' && event.version === '1.0.0' &&
    event.event === expectedEvent && object(event.data), 'Malformed or unrelated withdrawal event');
  return event.data;
}

class WithdrawalVerifier {
  constructor(config) {
    this.networkId = config.networkId;
    this.rpc = config.rpc;
    this.marketplaceContractId = config.marketplaceContractId;
    this.nftContractId = config.nftContractId;
    this.marketplaceCodeHash = config.marketplaceCodeHash;
    this.nftCodeHash = config.nftCodeHash;
    // Reuse the reviewed RPC/view helpers instead of duplicating their checks.
    this.base = new ChainVerifier(config);
  }

  matchesWithdrawal(tx) {
    if (tx?.receiver_id !== this.marketplaceContractId || !Array.isArray(tx.actions) ||
        tx.actions.length !== 1 || !tx.actions[0]?.FunctionCall) return false;
    const method = tx.actions[0].FunctionCall.method_name;
    if (!WITHDRAWAL_METHODS.has(method)) return false;
    if (method !== 'storage_deposit') return true;
    // Only a registration-only deposit refunds excess. A normal storage deposit
    // is a funding action, not a withdrawal, and is intentionally ignored.
    try {
      const args = chainJson(Buffer.from(tx.actions[0].FunctionCall.args, 'base64').toString('utf8'));
      return object(args) && args.registration_only === true;
    } catch { return false; }
  }

  async verifyTransaction(discoveredTx, inclusion) {
    try {
      return await this.verifyEvidence(discoveredTx, inclusion);
    } catch (error) {
      if (error?.name === 'RpcError') throw error;
      if (error instanceof RejectedWithdrawal) throw error;
      throw new RejectedWithdrawal(error?.message || 'Invalid withdrawal evidence');
    }
  }

  async receiptBodies(result, graph) {
    const bodies = new Map();
    for (const receipt of result.receipts || []) {
      check(hash(receipt?.receipt_id), 'Malformed receipt body');
      if (exactKeys(receipt.receipt, ['Data'])) continue;
      check(!bodies.has(receipt.receipt_id), 'Duplicate receipt body');
      bodies.set(receipt.receipt_id, receipt);
    }
    for (const id of graph.nodes.keys()) {
      if (bodies.has(id) || id === graph.root.id) continue;
      const receipt = await this.rpc('EXPERIMENTAL_receipt', { receipt_id: id });
      check(receipt?.receipt_id === id, 'Missing receipt body');
      bodies.set(id, receipt);
    }
    return bodies;
  }

  async canonicalBlocks(graph, inclusion) {
    const blocks = new Map();
    for (const node of graph.nodes.values()) {
      if (blocks.has(node.block_hash)) continue;
      const block = await this.rpc('block', { block_id: node.block_hash });
      check(block?.header?.hash === node.block_hash && height(block.header.height) &&
        block.header.height >= inclusion.height, 'Invalid withdrawal execution block');
      const canonicalHead = await this.rpc('block', { block_id: block.header.height });
      check(canonicalHead?.header?.hash === node.block_hash, 'Withdrawal execution is not canonical');
      blocks.set(node.block_hash, block.header);
    }
    const finalBlock = await this.rpc('block', { finality: 'final' });
    check(height(finalBlock?.header?.height) &&
      [...blocks.values()].every((header) => header.height <= finalBlock.header.height),
    'Withdrawal execution is not finalized');
    for (const [child, parent] of graph.parents) {
      check(blocks.get(graph.nodes.get(child).block_hash).height >=
        blocks.get(graph.nodes.get(parent).block_hash).height,
      'Withdrawal receipt executed before its parent');
    }
    return blocks;
  }

  async verifyEvidence(discoveredTx, inclusion) {
    check(this.matchesWithdrawal(discoveredTx) && discoveredTx.signer_id !== this.marketplaceContractId &&
      accountId(discoveredTx.signer_id) && hash(discoveredTx.hash) &&
      hash(inclusion?.hash) && height(inclusion.height), 'Invalid withdrawal transaction discovery');
    const method = discoveredTx.actions[0].FunctionCall.method_name;
    const expectedKind = KIND_BY_METHOD[method];
    const call = functionCall(discoveredTx.actions, method);
    const signer = discoveredTx.signer_id;
    const parsed = this.parseCall(method, call, signer);

    const result = await this.base.transaction(discoveredTx.hash, signer);
    check(isDeepStrictEqual(result?.transaction, discoveredTx), 'Withdrawal transaction differs from its chunk');
    check(result.final_execution_status === 'FINAL', 'Withdrawal is not FINAL');
    const graph = receiptGraph(result);
    check(graph.root.id === discoveredTx.hash && graph.root.outcome.executor_id === signer &&
      graph.root.block_hash === inclusion.hash && graph.root.outcome.logs.length === 0 &&
      graph.root.outcome.receipt_ids.length === 1 &&
      exactKeys(graph.root.outcome.status, ['SuccessReceiptId']),
    'Withdrawal outcome is not bound to the scanned block');

    const callId = graph.root.outcome.status.SuccessReceiptId;
    const callNode = graph.nodes.get(callId);
    check(callNode?.outcome.executor_id === this.marketplaceContractId,
      'Missing withdrawal call execution');
    const receipts = await this.receiptBodies(result, graph);
    const callReceipt = receipts.get(callId);
    check(callReceipt?.predecessor_id === signer && callReceipt.receiver_id === this.marketplaceContractId &&
      exactKeys(callReceipt.receipt, ['Action']) &&
      isDeepStrictEqual(callReceipt.receipt.Action.actions, discoveredTx.actions) &&
      callReceipt.receipt.Action.input_data_ids.length === 0,
    'Withdrawal receipt is not the direct signed marketplace call');

    const children = callNode.outcome.receipt_ids;
    check(children.length === 2, 'Withdrawal must produce exactly a transfer and callback receipt');
    let callbackId;
    let transferId;
    for (const child of children) {
      const receipt = receipts.get(child);
      check(receipt && exactKeys(receipt.receipt, ['Action']), 'Unrelated withdrawal child receipt');
      const actions = receipt.receipt.Action.actions;
      if (actions.length === 1 && exactKeys(actions[0], ['FunctionCall']) &&
          actions[0].FunctionCall.method_name === 'on_withdrawal_result') {
        check(!callbackId, 'Duplicate withdrawal callback');
        callbackId = child;
      } else {
        check(!transferId, 'Duplicate withdrawal transfer');
        transferId = child;
      }
    }
    check(callbackId && transferId, 'Withdrawal requires one transfer and one callback receipt');
    // Promise-returning withdrawals end at the callback. A registration-only
    // storage_deposit returns its StorageBalance synchronously, so its terminal
    // receipt is the call itself while the refund callback runs beside it.
    check(graph.terminal.id === (method === 'storage_deposit' ? callId : callbackId),
      'Unexpected withdrawal terminal receipt');
    const callbackReceipt = receipts.get(callbackId);
    const callbackCall = functionCall(callbackReceipt.receipt.Action.actions, 'on_withdrawal_result');
    check(exactKeys(callbackCall.json, ['account_id', 'nonce']) &&
      callbackCall.json.account_id === parsed.account && callbackCall.deposit === '0' &&
      callbackReceipt.predecessor_id === this.marketplaceContractId &&
      graph.parents.get(callbackId) === callId &&
      callbackReceipt.receipt.Action.input_data_ids.length === 1 &&
      hash(callbackReceipt.receipt.Action.input_data_ids[0]) &&
      callbackReceipt.receipt.Action.output_data_receivers.length === 0,
    'Withdrawal callback arguments or dependency mismatch');
    const nonce = uint(callbackCall.json.nonce, U64_MAX, true).toString();

    const transferReceipt = receipts.get(transferId);
    const transferAction = transferReceipt.receipt.Action;
    check(transferReceipt.predecessor_id === this.marketplaceContractId &&
      transferReceipt.receiver_id === parsed.recipient &&
      graph.parents.get(transferId) === callId && transferAction.actions.length === 1 &&
      exactKeys(transferAction.actions[0], ['Transfer']) &&
      exactKeys(transferAction.actions[0].Transfer, ['deposit']) &&
      transferAction.input_data_ids.length === 0 &&
      transferAction.output_data_receivers.length === 1 &&
      exactKeys(transferAction.output_data_receivers[0], ['data_id', 'receiver_id']) &&
      transferAction.output_data_receivers[0].data_id === callbackReceipt.receipt.Action.input_data_ids[0] &&
      transferAction.output_data_receivers[0].receiver_id === this.marketplaceContractId,
    'Withdrawal transfer or promise dependency mismatch');
    const transferred = uint(transferAction.actions[0].Transfer.deposit);

    const callbackNode = graph.nodes.get(callbackId);
    check(executionStatus(callbackNode.outcome.status) === 'value', 'Withdrawal callback did not return a result');
    const callbackValue = callbackNode.outcome.status.SuccessValue;
    check([TRUE_B64, FALSE_B64].includes(callbackValue), 'Withdrawal callback returned a non-boolean value');

    const eventLogs = callbackNode.outcome.logs;
    check(eventLogs.length === 1, 'Withdrawal callback must emit exactly one event');
    const succeeded = callbackValue === TRUE_B64;
    const eventName = succeeded ? 'withdrawal_succeeded' : 'withdrawal_failed';
    const data = parseMarketplaceEvent(eventLogs[0], eventName);
    if (succeeded) {
      check(exactKeys(data, ['account_id', 'recipient_id', 'amount', 'nonce', 'kind']),
        'Malformed successful withdrawal event');
      check(data.recipient_id === parsed.recipient, 'Withdrawal recipient conflicts with the signer');
    } else {
      check(exactKeys(data, ['account_id', 'amount', 'nonce', 'kind']),
        'Malformed failed withdrawal event');
    }
    check(KINDS.has(data.kind) && data.kind === expectedKind, 'Withdrawal kind conflicts with the call');
    check(data.account_id === parsed.account, 'Withdrawal account conflicts with the call');
    check(canonical(uint(data.nonce, U64_MAX, true)) === nonce, 'Withdrawal nonce conflicts with the callback');
    const amount = canonical(uint(data.amount, undefined, true));
    check(canonical(transferred) === amount, 'Transfer amount conflicts with the withdrawal event');
    if (parsed.amount !== null) check(amount === parsed.amount, 'Withdrawal amount conflicts with the signed call');

    for (const [id, node] of graph.nodes) {
      if (id === callbackId) continue;
      check(node.outcome.logs.length === 0, 'Unexpected log outside the withdrawal callback');
      if (id === transferId) {
        const status = executionStatus(node.outcome.status);
        check(succeeded ? status !== 'failure' : status === 'failure',
          succeeded ? 'Successful withdrawal has a failed transfer' : 'Failed withdrawal has a successful transfer');
      } else {
        check(executionStatus(node.outcome.status) !== 'failure', 'Withdrawal request or callback failed');
      }
    }

    const blocks = await this.canonicalBlocks(graph, inclusion);
    const callBlock = blocks.get(callNode.block_hash);
    const callbackBlock = blocks.get(callbackNode.block_hash);
    check(callbackBlock.height >= callBlock.height, 'Withdrawal callback executed before its request');
    for (const block of [callBlock, callbackBlock]) {
      const state = await this.base.accountState(this.marketplaceContractId, block.hash);
      check(state?.block_hash === block.hash && state.block_height === block.height &&
        state.code_hash === this.marketplaceCodeHash, 'Withdrawal used unreviewed marketplace code');
    }
    const parentBlock = await this.rpc('block', { block_id: callBlock.prev_hash });
    check(hash(callBlock.prev_hash) && parentBlock?.header?.hash === callBlock.prev_hash &&
      height(parentBlock.header.height) && parentBlock.header.height < callBlock.height,
    'Missing withdrawal pre-state block');
    const pre = { hash: parentBlock.header.hash, height: parentBlock.header.height };
    const post = callbackBlock;

    const view = (account, name, args, block) => this.base.contractState(account, name, args, block);
    const market = this.marketplaceContractId;
    const prePending = await view(market, 'pending_withdrawal', { account_id: parsed.account }, pre);
    const postPending = await view(market, 'pending_withdrawal', { account_id: parsed.account }, post);
    check(prePending === null && postPending === null, 'Withdrawal pending record was not cleared');
    const preLedger = await view(market, 'ledger', {}, pre);
    const postLedger = await view(market, 'ledger', {}, post);
    check(canonical(uint(preLedger.withdrawal_in_flight)) ===
      canonical(uint(postLedger.withdrawal_in_flight)), 'Withdrawal remains in flight after its callback');

    if (expectedKind === 'buyer_refund') {
      const preSettlement = await view(market, 'settlement', { sale_id: parsed.saleId }, pre);
      const postSettlement = await view(market, 'settlement', { sale_id: parsed.saleId }, post);
      check(preSettlement?.sale_id === parsed.saleId && preSettlement.buyer === signer &&
        canonical(uint(preSettlement.price, undefined, true)) === amount, 'Refund sale does not belong to the buyer');
      const expectedPre = 'refund_available';
      const expectedPost = succeeded ? 'refunded' : 'refund_available';
      check(preSettlement.status === expectedPre && postSettlement?.status === expectedPost,
        'Refund settlement status does not match the callback');
    }

    if (expectedKind === 'seller_proceeds' || expectedKind === 'marketplace_fees') {
      const methodName = expectedKind === 'seller_proceeds' ? 'seller_proceeds' : 'fee_proceeds';
      const preBalance = await view(market, methodName, { [expectedKind === 'seller_proceeds' ? 'seller_id' : 'account_id']: parsed.account }, pre);
      const postBalance = await view(market, methodName, { [expectedKind === 'seller_proceeds' ? 'seller_id' : 'account_id']: parsed.account }, post);
      const before = big(uint(preBalance));
      const after = big(uint(postBalance));
      check(after === (succeeded ? before - big(amount) : before) && after >= 0n,
        'Withdrawn balance was not debited exactly once');
      const counter = expectedKind === 'seller_proceeds' ? 'seller_withdrawn' : 'fee_withdrawn';
      check(big(uint(postLedger[counter])) === big(uint(preLedger[counter])) + (succeeded ? big(amount) : 0n),
        'Withdrawal counter does not match the callback');
      const liability = expectedKind === 'seller_proceeds' ? 'seller_liability' : 'fee_liability';
      check(big(uint(postLedger[liability])) ===
        big(uint(preLedger[liability])) - (succeeded ? big(amount) : 0n),
      'Withdrawal liability does not match the callback');
    } else if (expectedKind === 'storage_balance' || expectedKind === 'registration_excess') {
      const preStorage = await view(market, 'storage_balance_of', { account_id: parsed.account }, pre);
      const postStorage = await view(market, 'storage_balance_of', { account_id: parsed.account }, post);
      check(postStorage !== null, 'Withdrawal storage account is missing');
      const preTotal = big(uint(preStorage?.total ?? '0'));
      const preUsed = big(uint(preStorage?.used ?? '0'));
      const postTotal = big(uint(postStorage.total));
      const postUsed = big(uint(postStorage.used));
      if (expectedKind === 'storage_balance') {
        check(preStorage !== null, 'Storage withdrawal from an unregistered account');
        check(postUsed === preUsed && postTotal ===
          (succeeded ? preTotal - big(amount) : preTotal),
        'Storage withdrawal balance does not match the callback');
        check(big(uint(postLedger.storage_liability)) ===
          big(uint(preLedger.storage_liability)) - (succeeded ? big(amount) : 0n),
        'Storage withdrawal liability does not match the callback');
      } else {
        const deposit = big(parsed.deposit);
        const preAvailable = preTotal - preUsed;
        const postAvailable = postTotal - postUsed;
        check(big(amount) <= deposit, 'Registration refund exceeds its deposit');
        if (succeeded) {
          check(postTotal === preTotal + deposit - big(amount) &&
            postAvailable === (preStorage === null ? 0n : preAvailable),
          'Registration-excess refund does not match the storage deposit');
        } else {
          check(postTotal === preTotal + deposit &&
            postAvailable === preAvailable + big(amount),
          'Failed registration-excess refund did not restore the storage balance');
        }
        check(big(uint(postLedger.storage_liability)) ===
          big(uint(preLedger.storage_liability)) + (succeeded ? 0n : big(amount)),
        'Registration-excess liability does not match the callback');
      }
    }

    const facts = Object.freeze({
      txHash: discoveredTx.hash, networkId: this.networkId,
      marketplaceContractId: this.marketplaceContractId, nftContractId: this.nftContractId,
      accountWalletAddress: parsed.account, recipientWalletAddress: parsed.recipient,
      method, kind: expectedKind, amountYocto: amount, nonce,
      referenceSaleId: parsed.saleId, outcome: succeeded ? 'succeeded' : 'failed',
      requestReceiptId: callId, callbackReceiptId: callbackId, transferReceiptId: transferId,
      requestBlockHash: callBlock.hash, requestBlockHeight: callBlock.height,
      callbackBlockHash: callbackBlock.hash, callbackBlockHeight: callbackBlock.height,
      scannedBlockHash: inclusion.hash, scannedBlockHeight: inclusion.height,
    });
    verifiedFacts.add(facts);
    return facts;
  }

  parseCall(method, call, signer) {
    const recipient = signer;
    if (method === 'withdraw_proceeds' || method === 'withdraw_fees') {
      check(call.deposit === '1' && exactKeys(call.json, ['amount']),
        'Proceeds/fee withdrawal must attach one yoctoNEAR and an exact amount');
      return { account: signer, recipient, amount: canonical(uint(call.json.amount, undefined, true)),
        saleId: null, deposit: null };
    }
    if (method === 'withdraw_refund') {
      check(call.deposit === '1' && exactKeys(call.json, ['sale_id']) &&
        typeof call.json.sale_id === 'string' && call.json.sale_id.length > 0,
      'Refund withdrawal must attach one yoctoNEAR and an exact sale ID');
      return { account: signer, recipient, amount: null, saleId: call.json.sale_id, deposit: null };
    }
    if (method === 'storage_withdraw') {
      check(call.deposit === '1' && subsetKeys(call.json, ['amount']) &&
        (call.json.amount === null || call.json.amount === undefined ||
          typeof call.json.amount === 'string'), 'Storage withdrawal arguments are invalid');
      return { account: signer, recipient,
        amount: call.json.amount == null ? null : canonical(uint(call.json.amount, undefined, true)),
        saleId: null, deposit: null };
    }
    if (method === 'storage_deposit') {
      check(subsetKeys(call.json, ['account_id', 'registration_only']) &&
        call.json.registration_only === true &&
        (call.json.account_id === undefined || call.json.account_id === null ||
          accountId(call.json.account_id)), 'Registration-only storage deposit arguments are invalid');
      const account = call.json.account_id ?? signer;
      return { account, recipient, amount: null, saleId: null,
        deposit: canonical(uint(call.deposit, undefined, true)) };
    }
    throw new Error('Unsupported withdrawal method');
  }
}

class WithdrawalReconciler {
  constructor(config, supabase) {
    this.config = config;
    this.supabase = supabase;
  }

  async reconcile(facts) {
    if (!isVerifiedWithdrawalFacts(facts)) return { ok: false, error: 'Unverified withdrawal facts' };
    if (!hash(facts.txHash) || !accountId(facts.accountWalletAddress) ||
        !accountId(facts.recipientWalletAddress) || facts.networkId !== this.config.networkId ||
        facts.marketplaceContractId !== this.config.marketplaceContractId ||
        !KINDS.has(facts.kind) || !['succeeded', 'failed'].includes(facts.outcome)) {
      return { ok: false, error: 'Verified withdrawal facts conflict with settlement configuration' };
    }
    try {
      uint(facts.amountYocto, undefined, true);
      uint(facts.nonce, U64_MAX, true);
    } catch (error) { return { ok: false, error: error.message }; }

    const intent = await this.supabase.rpc('record_withdrawal_intent', {
      p_transaction_hash: facts.txHash,
      p_network_id: facts.networkId,
      p_marketplace_contract_id: facts.marketplaceContractId,
      p_account_wallet_address: facts.accountWalletAddress,
      p_recipient_wallet_address: facts.recipientWalletAddress,
      p_kind: facts.kind,
      p_amount_yocto: facts.amountYocto,
      p_nonce: facts.nonce,
      p_reference_sale_id: facts.referenceSaleId,
      p_request_block_hash: facts.requestBlockHash,
      p_request_block_height: facts.requestBlockHeight,
    });
    if (intent.error) return { ok: false, error: intent.error.message };
    if (typeof intent.data !== 'string' || !intent.data) {
      return { ok: false, error: 'Withdrawal intent did not return a row ID' };
    }

    const name = facts.outcome === 'succeeded' ? 'confirm_withdrawal' : 'fail_withdrawal';
    const confirmation = await this.supabase.rpc(name, {
      p_transaction_hash: facts.txHash,
      p_network_id: facts.networkId,
      p_marketplace_contract_id: facts.marketplaceContractId,
      p_nonce: facts.nonce,
      p_amount_yocto: facts.amountYocto,
      p_callback_block_hash: facts.callbackBlockHash,
      p_callback_block_height: facts.callbackBlockHeight,
    });
    if (confirmation.error) return { ok: false, error: confirmation.error.message };
    if (confirmation.data !== intent.data) {
      return { ok: false, error: 'Unexpected withdrawal reconciliation result' };
    }
    return { ok: true, id: intent.data, outcome: facts.outcome };
  }
}

module.exports = {
  WithdrawalVerifier, WithdrawalReconciler, RejectedWithdrawal,
  isVerifiedWithdrawalFacts, WITHDRAWAL_METHODS, KIND_BY_METHOD,
};
