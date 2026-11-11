// Verifies the existing marketplace-deploy ABI, never browser purchase claims.
const { isDeepStrictEqual } = require('node:util');
const {
  U64_MAX, YOCTO_PER_NEAR, check, object, exactKeys, accountId, hash, uint,
  height, base64, utf8, chainJson, functionCall,
} = require('./validation');

const verifiedFacts = new WeakSet();
const isVerifiedFacts = (facts) => object(facts) && verifiedFacts.has(facts);

class RejectedPurchase extends Error {
  constructor(message) { super(message); this.name = 'RejectedPurchase'; }
}

function executionStatus(status) {
  if (exactKeys(status, ['SuccessValue'])) {
    base64(status.SuccessValue); // Empty base64 is a valid successful void return.
    return 'value';
  }
  if (exactKeys(status, ['SuccessReceiptId']) && hash(status.SuccessReceiptId)) return 'receipt';
  if (exactKeys(status, ['Failure']) && object(status.Failure)) return 'failure';
  throw new Error('Missing, pending or malformed execution status');
}

// receipt_ids link execution outcomes. Promise dependency data IDs are checked
// separately below; a shared transaction hash alone does not link two calls.
function receiptGraph(result) {
  check(result?.final_execution_status === 'FINAL', 'Transaction receipts are not FINAL');
  check(Array.isArray(result.receipts_outcome) && result.receipts_outcome.length <= 256,
    'Missing or excessive receipt outcomes');
  const root = result.transaction_outcome;
  const nodes = new Map();
  for (const node of [root, ...result.receipts_outcome]) {
    check(object(node) && hash(node.id) && hash(node.block_hash) && object(node.outcome),
      'Malformed execution outcome');
    check(!nodes.has(node.id), 'Duplicate outcome ID');
    const out = node.outcome;
    check(accountId(out.executor_id) && Array.isArray(out.logs) &&
      out.logs.every((log) => typeof log === 'string') && Array.isArray(out.receipt_ids) &&
      out.receipt_ids.every(hash) && new Set(out.receipt_ids).size === out.receipt_ids.length,
    'Malformed receipt links or logs');
    const status = executionStatus(out.status);
    if (status === 'receipt') check(out.receipt_ids.includes(out.status.SuccessReceiptId),
      'SuccessReceiptId is not a generated receipt');
    nodes.set(node.id, node);
  }
  const visited = new Set();
  const parents = new Map();
  function visit(id) {
    check(nodes.has(id), 'Incomplete receipt graph: missing outcome');
    check(!visited.has(id), 'Cyclic or multiply linked receipt graph');
    visited.add(id);
    for (const child of nodes.get(id).outcome.receipt_ids) {
      parents.set(child, id);
      visit(child);
    }
  }
  visit(root.id);
  check(visited.size === nodes.size, 'Unrelated receipt outcome');
  const finalStatus = executionStatus(result.status);
  check(finalStatus !== 'receipt', 'Invalid transaction final status');
  let terminal = root;
  while (executionStatus(terminal.outcome.status) === 'receipt') {
    terminal = nodes.get(terminal.outcome.status.SuccessReceiptId);
  }
  check(isDeepStrictEqual(terminal.outcome.status, result.status), 'Conflicting final return value');
  return { nodes, parents, root, terminal };
}

function marketplaceEvent(log, expectedEvent, saleId) {
  check(typeof log === 'string' && log.startsWith('EVENT_JSON:'), 'Missing marketplace event');
  const event = chainJson(log.slice('EVENT_JSON:'.length));
  check(exactKeys(event, ['standard', 'version', 'event', 'data']) &&
    event.standard === 'near_marketplace' && event.version === '1.0.0' &&
    event.event === expectedEvent && object(event.data) && event.data.sale_id === saleId,
  'Malformed or conflicting marketplace event');
}

class ChainVerifier {
  constructor(config) {
    this.networkId = config.networkId;
    this.rpc = config.rpc;
    this.marketplaceContractId = config.marketplaceContractId;
    this.nftContractId = config.nftContractId;
    this.marketplaceCodeHash = config.marketplaceCodeHash;
    this.nftCodeHash = config.nftCodeHash;
    check(typeof this.rpc === 'function', 'A trusted RPC client is required');
    check(['testnet', 'mainnet'].includes(this.networkId) && accountId(this.marketplaceContractId) &&
      accountId(this.nftContractId) && this.marketplaceContractId !== this.nftContractId,
    'Invalid chain identity configuration');
    check(hash(this.marketplaceCodeHash) && hash(this.nftCodeHash),
      'Reviewed marketplace and NFT code hashes must be pinned');
  }

  async status() {
    const result = await this.rpc('status', []);
    check(result?.chain_id === this.networkId && result?.sync_info?.syncing === false &&
      height(result.sync_info.latest_block_height), 'RPC network mismatch or node is syncing');
    return result;
  }

  async transaction(txHash, account) {
    check(hash(txHash) && accountId(account), 'Invalid transaction lookup identity');
    // Compatible with older nodes as well as the 2.14 tx_status alias. Receipt
    // bodies authenticate predecessor/action/callback data links.
    return this.rpc('EXPERIMENTAL_tx_status', {
      tx_hash: txHash, sender_account_id: account, wait_until: 'FINAL',
    });
  }

  async accountState(account, blockHash) {
    return this.rpc('query', { request_type: 'view_account', account_id: account, block_id: blockHash });
  }

  async contractState(account, method, args, block) {
    const result = await this.rpc('query', {
      request_type: 'call_function', account_id: account, method_name: method,
      args_base64: Buffer.from(JSON.stringify(args)).toString('base64'), block_id: block.hash,
    });
    check(result?.block_hash === block.hash && result.block_height === block.height &&
      Array.isArray(result.result) && result.result.length <= 2_000_000 &&
      result.result.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255) &&
      Array.isArray(result.logs) && result.logs.length === 0 && !result.error,
    'Missing or conflicting historical contract state');
    return chainJson(utf8(Uint8Array.from(result.result)));
  }

  matchesMarketplace(tx) { return tx?.receiver_id === this.marketplaceContractId; }

  matchesPurchase(tx) {
    return this.matchesMarketplace(tx) && Array.isArray(tx.actions) && tx.actions.length === 1 &&
      tx.actions[0]?.FunctionCall?.method_name === 'buy_nft';
  }

  isFinalSuccess(result) {
    try {
      const graph = receiptGraph(result);
      return executionStatus(result.status) === 'value' &&
        [...graph.nodes.values()].every((node) => executionStatus(node.outcome.status) !== 'failure');
    } catch { return false; }
  }

  // Legacy name retained. This parses only the NFT's pinned NEP-171 transfer
  // log; marketplace sale events are authenticated separately below. It
  // returns log data only, never reconciler-authorized facts.
  parseSaleEvent(result, receiptId) {
    const node = result?.receipts_outcome?.find((entry) => entry.id === receiptId);
    check(node?.outcome?.executor_id === this.nftContractId &&
      executionStatus(node.outcome.status) === 'value' && node.outcome.status.SuccessValue === 'dHJ1ZQ==',
    'NFT transfer must return exactly JSON true');
    const logs = node.outcome.logs;
    check(Array.isArray(logs) && logs.length === 1 && logs[0].startsWith('EVENT_JSON:'),
      'Missing or conflicting NFT transfer event');
    const event = chainJson(logs[0].slice('EVENT_JSON:'.length));
    check(exactKeys(event, ['standard', 'version', 'event', 'data']) &&
      event.standard === 'nep171' && event.version === '1.0.0' && event.event === 'nft_transfer' &&
      Array.isArray(event.data) && event.data.length === 1, 'Malformed or unrelated NFT event');
    const data = event.data[0];
    // The pinned SDK omits authorized_id for this approved non-owner caller;
    // authorization is proved by the receipt predecessor and exact arguments.
    check(exactKeys(data, ['old_owner_id', 'new_owner_id', 'token_ids']) &&
      accountId(data.old_owner_id) && accountId(data.new_owner_id) &&
      Array.isArray(data.token_ids) && data.token_ids.length === 1 && typeof data.token_ids[0] === 'string',
    'Malformed NFT transfer data');
    return data;
  }

  async verifyTransaction(discoveredTx, inclusion) {
    try {
      return await this.verifyTransactionEvidence(discoveredTx, inclusion);
    } catch (error) {
      if (error?.name === 'RpcError') throw error;
      if (error instanceof RejectedPurchase) throw error;
      throw new RejectedPurchase(error?.message || 'Invalid purchase evidence');
    }
  }

  async verifyTransactionEvidence(discoveredTx, inclusion) {
    check(this.matchesMarketplace(discoveredTx) && discoveredTx.signer_id !== this.marketplaceContractId &&
      hash(inclusion?.hash) && height(inclusion.height), 'Invalid marketplace transaction discovery');
    const originalCall = functionCall(discoveredTx.actions, 'buy_nft');
    check(exactKeys(originalCall.json, ['listing_id']) && typeof originalCall.json.listing_id === 'string',
      'buy_nft requires the contract listing_id');
    const contractListingId = originalCall.json.listing_id;
    const parts = contractListingId.split('|');
    const generation = parts.at(-1);
    const seller = parts.at(-2);
    const token = parts.slice(1, -2).join('|');
    check(parts.length >= 4 && parts[0] === this.nftContractId && accountId(seller) &&
      token.trim().length > 0 && !token.includes('\0') && Buffer.byteLength(token) <= 1024 &&
      Buffer.byteLength(contractListingId) <= 1024, 'Invalid contract listing identity');
    uint(generation, U64_MAX, true);
    const buyer = discoveredTx.signer_id;
    check(accountId(buyer) && buyer !== seller && seller !== this.marketplaceContractId &&
      seller !== this.nftContractId, 'Invalid buyer or seller');
    uint(originalCall.deposit, undefined, true);
    const purchaseSaleId = `${contractListingId}|${buyer}|${generation}`;

    const result = await this.transaction(discoveredTx.hash, buyer);
    check(isDeepStrictEqual(result?.transaction, discoveredTx), 'Transaction differs from its chunk');
    const graph = receiptGraph(result);
    check(graph.root.id === discoveredTx.hash && graph.root.outcome.executor_id === buyer &&
      graph.root.block_hash === inclusion.hash && graph.root.outcome.logs.length === 0,
    'Transaction outcome is not linked to the scanned block/signer');
    if ([...graph.nodes.values()].some((node) => executionStatus(node.outcome.status) === 'failure')) {
      throw new RejectedPurchase('Final transaction or receipt failed');
    }
    check(result.status.SuccessValue === '' && graph.root.outcome.receipt_ids.length === 1,
      'Unexpected purchase return value or transaction receipts');

    const receipts = new Map();
    check(result.receipts === undefined || Array.isArray(result.receipts), 'Malformed receipt bodies');
    const dataReceipts = [];
    for (const receipt of result.receipts || []) {
      check(hash(receipt?.receipt_id), 'Malformed receipt body');
      if (exactKeys(receipt.receipt, ['Data'])) {
        dataReceipts.push(receipt);
        continue;
      }
      check(!receipts.has(receipt.receipt_id) && graph.nodes.has(receipt.receipt_id) &&
        receipt.receipt_id !== graph.root.id, 'Duplicate or unrelated action receipt body');
      receipts.set(receipt.receipt_id, receipt);
    }
    for (const node of result.receipts_outcome) {
      if (!receipts.has(node.id)) {
        const receipt = await this.rpc('EXPERIMENTAL_receipt', { receipt_id: node.id });
        check(receipt?.receipt_id === node.id, 'Missing receipt body');
        receipts.set(node.id, receipt);
      }
    }
    for (const [id, receipt] of receipts) {
      check(accountId(receipt.predecessor_id) && receipt.receiver_id === graph.nodes.get(id).outcome.executor_id &&
        exactKeys(receipt.receipt, ['Action']), 'Receipt identity does not match its execution');
      const action = receipt.receipt.Action;
      check(action.signer_id === buyer && Array.isArray(action.actions) &&
        Array.isArray(action.input_data_ids) && Array.isArray(action.output_data_receivers) &&
        !action.is_promise_yield && (action.refund_to === undefined || action.refund_to === buyer),
      'Unsupported receipt signer or promise');
    }
    const buyId = graph.root.outcome.status.SuccessReceiptId;
    const buy = receipts.get(buyId);
    check(buy?.predecessor_id === buyer && buy.receiver_id === this.marketplaceContractId &&
      isDeepStrictEqual(buy.receipt.Action.actions, discoveredTx.actions) &&
      buy.receipt.Action.input_data_ids.length === 0 && buy.receipt.Action.output_data_receivers.length === 0,
    'Purchase receipt is not the direct signed marketplace call');
    const callbackId = graph.nodes.get(buyId).outcome.status.SuccessReceiptId;
    const callback = receipts.get(callbackId);
    check(callback && callbackId === graph.terminal.id && callback.predecessor_id === this.marketplaceContractId &&
      callback.receiver_id === this.marketplaceContractId && graph.parents.get(callbackId) === buyId,
    'Missing or unrelated marketplace callback');
    const callbackCall = functionCall(callback.receipt.Action.actions, 'on_transfer_result');
    const currentCallbackAbi = exactKeys(callbackCall.json, ['sale_id']);
    const legacyCallbackAbi = exactKeys(callbackCall.json, ['sale_id', 'seller_share', 'fee_share']);
    check(callbackCall.deposit === '0' && (currentCallbackAbi || legacyCallbackAbi) &&
      callbackCall.json.sale_id === purchaseSaleId && callback.receipt.Action.input_data_ids.length === 1 &&
      hash(callback.receipt.Action.input_data_ids[0]) && callback.receipt.Action.output_data_receivers.length === 0,
    'Callback sale or dependency mismatch');
    const transferReceipts = [...receipts.values()].filter((r) => r.receiver_id === this.nftContractId);
    check(transferReceipts.length === 1, 'Missing or conflicting NFT receipt');
    const transfer = transferReceipts[0];
    const transferId = transfer.receipt_id;
    const transferCall = functionCall(transfer.receipt.Action.actions, 'marketplace_transfer_approved');
    const transferApprovalId = uint(transferCall.json.approval_id, U64_MAX, true).toString();
    check(transfer.predecessor_id === this.marketplaceContractId && graph.parents.get(transferId) === buyId &&
      transferCall.deposit === '1' &&
      exactKeys(transferCall.json, ['expected_seller_id', 'token_id', 'approval_id', 'receiver_id']) &&
      transferCall.json.expected_seller_id === seller && transferCall.json.token_id === token &&
      transferApprovalId === String(transferCall.json.approval_id) && transferCall.json.receiver_id === buyer &&
      transfer.receipt.Action.input_data_ids.length === 0 &&
      transfer.receipt.Action.output_data_receivers.length === 1 &&
      exactKeys(transfer.receipt.Action.output_data_receivers[0], ['data_id', 'receiver_id']) &&
      transfer.receipt.Action.output_data_receivers[0].data_id === callback.receipt.Action.input_data_ids[0] &&
      transfer.receipt.Action.output_data_receivers[0].receiver_id === this.marketplaceContractId,
    'NFT transfer arguments or callback dependency mismatch');
    for (const receipt of dataReceipts) {
      const data = receipt.receipt.Data;
      check(receipt.predecessor_id === this.nftContractId &&
        receipt.receiver_id === this.marketplaceContractId &&
        data?.data_id === callback.receipt.Action.input_data_ids[0] &&
        data.is_promise_resume === false, 'Unrelated promise data receipt');
    }
    const event = this.parseSaleEvent(result, transferId);
    check(event.old_owner_id === seller && event.new_owner_id === buyer && event.token_ids[0] === token,
      'NFT event conflicts with the purchase');
    for (const [id, receipt] of receipts) {
      const out = graph.nodes.get(id).outcome;
      if (id !== transferId && legacyCallbackAbi) check(out.logs.length === 0, 'Unexpected event outside the NFT receipt');
      if (id !== transferId && currentCallbackAbi) {
        if (id === buyId) {
          check(out.logs.length === 1, 'Missing pending-sale event');
          marketplaceEvent(out.logs[0], 'sale_pending', purchaseSaleId);
        } else if (id === callbackId) {
          check(out.logs.length === 1, 'Missing settled-sale event');
          marketplaceEvent(out.logs[0], 'sale_settled', purchaseSaleId);
        } else {
          check(out.logs.length === 0, 'Unexpected marketplace event log');
        }
      }
      if ([buyId, callbackId, transferId].includes(id)) continue;
      check(receipt.predecessor_id === 'system' && receipt.receiver_id === buyer &&
        receipt.receipt.Action.gas_price === '0' && receipt.receipt.Action.input_data_ids.length === 0 &&
        receipt.receipt.Action.output_data_receivers.length === 0 &&
        receipt.receipt.Action.actions.length === 1 && exactKeys(receipt.receipt.Action.actions[0], ['Transfer']) &&
        exactKeys(receipt.receipt.Action.actions[0].Transfer, ['deposit']) &&
        out.status.SuccessValue === '' && out.receipt_ids.length === 0, 'Unexpected purchase side effect');
      uint(receipt.receipt.Action.actions[0].Transfer.deposit);
    }

    // FINAL comes from the trusted node; bind each execution to its canonical
    // block, and pin all contract views to the callback's block.
    const blocks = new Map();
    for (const node of graph.nodes.values()) {
      if (blocks.has(node.block_hash)) continue;
      const block = await this.rpc('block', { block_id: node.block_hash });
      check(block?.header?.hash === node.block_hash && height(block.header.height) &&
        block.header.height >= inclusion.height, 'Invalid execution block');
      const canonical = await this.rpc('block', { block_id: block.header.height });
      check(canonical?.header?.hash === node.block_hash, 'Execution is not on the canonical chain');
      blocks.set(node.block_hash, block.header);
    }
    const finalBlock = await this.rpc('block', { finality: 'final' });
    check(height(finalBlock?.header?.height) && [...blocks.values()].every((b) => b.height <= finalBlock.header.height),
      'Receipt execution block is not finalized');
    for (const [id, parent] of graph.parents) {
      check(blocks.get(graph.nodes.get(id).block_hash).height >= blocks.get(graph.nodes.get(parent).block_hash).height,
        'Receipt executed before its parent');
    }
    const callbackBlock = blocks.get(graph.nodes.get(callbackId).block_hash);
    check(callbackBlock.height >= blocks.get(graph.nodes.get(transferId).block_hash).height,
      'Callback executed before NFT delivery');
    const identities = new Set();
    for (const [id, account, code] of [
      [buyId, this.marketplaceContractId, this.marketplaceCodeHash],
      [callbackId, this.marketplaceContractId, this.marketplaceCodeHash],
      [transferId, this.nftContractId, this.nftCodeHash],
      [callbackId, this.nftContractId, this.nftCodeHash],
    ]) {
      const block = blocks.get(graph.nodes.get(id).block_hash);
      const key = `${account}:${block.hash}`;
      if (identities.has(key)) continue;
      const state = await this.accountState(account, block.hash);
      check(state?.block_hash === block.hash && state.block_height === block.height && state.code_hash === code,
        'Unreviewed contract code or mismatched historical account');
      identities.add(key);
    }
    const config = await this.contractState(this.marketplaceContractId, 'config', {}, callbackBlock);
    const binding = await this.contractState(this.nftContractId, 'marketplace_id', {}, callbackBlock);
    check(config?.nft_contract_id === this.nftContractId && binding === this.marketplaceContractId,
      'Marketplace/NFT contract binding mismatch');
    const sale = await this.contractState(this.marketplaceContractId, 'sale', { sale_id: purchaseSaleId }, callbackBlock);
    const listing = sale?.listing;
    check((sale?.listing_id === contractListingId || (legacyCallbackAbi && sale?.listing_id === purchaseSaleId)) &&
      sale.buyer === buyer && String(sale.purchase_generation) === generation &&
      sale.settled === true && sale.nft_delivered === true && listing?.active === false && listing.cancelled_at === null &&
      listing.nft_contract_id === this.nftContractId && listing.token_id === token && listing.seller === seller &&
      String(listing.approval_id) === transferApprovalId && String(listing.price) === originalCall.deposit,
    'Missing, incomplete or conflicting settled sale state');
    const price = uint(listing.price, undefined, true);
    const fee = price * uint(listing.fee_bps, 10_000n) / 10_000n;
    if (legacyCallbackAbi) {
      check(uint(callbackCall.json.fee_share) === fee && uint(callbackCall.json.seller_share) === price - fee,
        'Callback accounting does not match the sale');
    } else {
      const settlement = await this.contractState(this.marketplaceContractId, 'settlement',
        { sale_id: purchaseSaleId }, callbackBlock);
      check(settlement?.sale_id === purchaseSaleId && settlement.listing_id === contractListingId &&
        settlement.nft_contract_id === this.nftContractId && settlement.token_id === token &&
        settlement.seller === seller && settlement.buyer === buyer &&
        String(settlement.approval_id) === transferApprovalId && String(settlement.generation) === generation &&
        settlement.status === 'settled' && uint(settlement.price) === price &&
        uint(settlement.fee_share) === fee && uint(settlement.seller_share) === price - fee,
      'Settlement record does not match the authenticated receipt graph');
    }
    const liveListing = await this.contractState(this.marketplaceContractId, 'listing',
      { listing_id: contractListingId }, callbackBlock);
    check(isDeepStrictEqual(liveListing, listing), 'Listing state conflicts with settled sale');
    const nft = await this.contractState(this.nftContractId, 'nft_token', { token_id: token }, callbackBlock);
    check(nft?.token_id === token && nft.owner_id === buyer, 'NFT ownership does not confirm delivery');

    const facts = Object.freeze({
      txHash: discoveredTx.hash, networkId: this.networkId,
      marketplaceContractId: this.marketplaceContractId, nftContractId: this.nftContractId,
      // SQL's contract_sale_id identifies the listed offer. The marketplace's
      // purchase-specific Sale key is verified and retained separately.
      contractSaleId: contractListingId, purchaseSaleId, tokenId: token,
      buyerWalletAddress: buyer, sellerWalletAddress: seller, priceYocto: originalCall.deposit,
      approvalId: transferApprovalId, generation, purchaseReceiptId: buyId, nftReceiptId: transferId,
      callbackReceiptId: callbackId, blockHash: inclusion.hash, blockHeight: inclusion.height,
      settlementBlockHash: callbackBlock.hash, settlementBlockHeight: callbackBlock.height,
    });
    verifiedFacts.add(facts);
    return facts;
  }
}

module.exports = {
  ChainVerifier, RejectedPurchase, isVerifiedFacts, receiptGraph, executionStatus, YOCTO_PER_NEAR,
};
