const { isDeepStrictEqual } = require('node:util');
const { receiptGraph } = require('../settlement/verifier');
const {
  U64_MAX, check, object, exactKeys, accountId, hash, uint, height,
  chainJson, functionCall, utf8,
} = require('../settlement/validation');

const verifiedFacts = new WeakSet();
const isVerifiedCatalogFacts = (facts) => object(facts) && verifiedFacts.has(facts);
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

class RejectedIndexEvent extends Error {
  constructor(message) { super(message); this.name = 'RejectedIndexEvent'; }
}

function parsedEvent(log) {
  check(typeof log === 'string' && log.startsWith('EVENT_JSON:'), 'Expected a NEAR event log');
  const event = chainJson(log.slice('EVENT_JSON:'.length));
  check(exactKeys(event, ['standard', 'version', 'event', 'data']), 'Malformed event envelope');
  return event;
}

class CatalogChainVerifier {
  constructor(config) {
    this.networkId = config.networkId;
    this.rpc = config.rpc;
    this.marketplaceContractId = config.marketplaceContractId;
    this.nftContractId = config.nftContractId;
    this.marketplaceCodeHash = config.marketplaceCodeHash;
    this.nftCodeHash = config.nftCodeHash;
    check(typeof this.rpc === 'function' && this.networkId === 'testnet', 'Catalog indexer requires Testnet RPC');
    check(accountId(this.marketplaceContractId) && accountId(this.nftContractId) &&
      hash(this.marketplaceCodeHash) && hash(this.nftCodeHash), 'Invalid pinned contract identity');
  }

  matches(tx) {
    if (!Array.isArray(tx?.actions) || tx.actions.length !== 1 || !tx.actions[0]?.FunctionCall) return false;
    const method = tx.actions[0].FunctionCall.method_name;
    return (tx.receiver_id === this.nftContractId && ['nft_mint', 'nft_approve'].includes(method)) ||
      (tx.receiver_id === this.marketplaceContractId && method === 'cancel_listing');
  }

  async transaction(hashValue, signer) {
    return this.rpc('EXPERIMENTAL_tx_status', {
      tx_hash: hashValue, sender_account_id: signer, wait_until: 'FINAL',
    });
  }

  async contractState(account, method, args, block) {
    const result = await this.rpc('query', {
      request_type: 'call_function', account_id: account, method_name: method,
      args_base64: Buffer.from(JSON.stringify(args)).toString('base64'), block_id: block.hash,
    });
    check(result?.block_hash === block.hash && result.block_height === block.height &&
      Array.isArray(result.result) && result.result.length <= 2_000_000 &&
      result.result.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255),
    'Historical contract view mismatch');
    return chainJson(utf8(Uint8Array.from(result.result)));
  }

  async verify(discoveredTx, inclusion) {
    try {
      return await this.verifyEvidence(discoveredTx, inclusion);
    } catch (error) {
      if (error?.name === 'RpcError') throw error;
      if (error instanceof RejectedIndexEvent) throw error;
      throw new RejectedIndexEvent(error?.message || 'Invalid catalog chain evidence');
    }
  }

  async verifyEvidence(discoveredTx, inclusion) {
    check(this.matches(discoveredTx) && hash(discoveredTx.hash) && accountId(discoveredTx.signer_id) &&
      hash(inclusion?.hash) && height(inclusion.height), 'Invalid indexed transaction discovery');
    const method = discoveredTx.actions[0].FunctionCall.method_name;
    const call = functionCall(discoveredTx.actions, method);
    const result = await this.transaction(discoveredTx.hash, discoveredTx.signer_id);
    check(isDeepStrictEqual(result?.transaction, discoveredTx), 'Transaction differs from scanned chunk');
    const graph = receiptGraph(result);
    check(graph.root.id === discoveredTx.hash && graph.root.block_hash === inclusion.hash &&
      graph.root.outcome.executor_id === discoveredTx.signer_id && graph.root.outcome.logs.length === 0 &&
      graph.root.outcome.receipt_ids.length === 1 && exactKeys(result.status, ['SuccessValue']) &&
      [...graph.nodes.values()].every((node) => !exactKeys(node.outcome.status, ['Failure'])),
    'Transaction outcome is not bound to the scanned block');

    const contractId = method === 'nft_mint' ? this.nftContractId : this.marketplaceContractId;
    const standard = method === 'nft_mint' ? 'nep171' : 'near_marketplace';
    const eventName = method === 'nft_mint' ? 'nft_mint' : method === 'nft_approve' ? 'listing_created' : 'listing_cancelled';
    const eventNodes = [...graph.nodes.values()].filter((node) =>
      node.outcome.executor_id === (method === 'nft_mint' ? this.nftContractId : this.marketplaceContractId) &&
      node.outcome.logs.some((log) => log.startsWith('EVENT_JSON:'))
    );
    check(eventNodes.length === 1 && eventNodes[0].outcome.logs.length === 1 &&
      exactKeys(eventNodes[0].outcome.status, ['SuccessValue']), 'Missing, failed or ambiguous catalog event');
    const event = parsedEvent(eventNodes[0].outcome.logs[0]);
    check(event.standard === standard && event.version === '1.0.0' && event.event === eventName,
      'Unexpected catalog event');

    const blocks = new Map();
    for (const node of graph.nodes.values()) {
      if (blocks.has(node.block_hash)) continue;
      const executionBlock = await this.rpc('block', { block_id: node.block_hash });
      check(executionBlock?.header?.hash === node.block_hash && height(executionBlock.header.height) &&
        executionBlock.header.height >= inclusion.height, 'Missing event execution block');
      const canonical = await this.rpc('block', { block_id: executionBlock.header.height });
      check(canonical?.header?.hash === node.block_hash, 'Catalog execution is not canonical');
      blocks.set(node.block_hash, executionBlock.header);
    }
    const finalBlock = await this.rpc('block', { finality: 'final' });
    check(height(finalBlock?.header?.height) &&
      [...blocks.values()].every((header) => header.height <= finalBlock.header.height),
    'Catalog event is not canonical and final');
    for (const [childId, parentId] of graph.parents) {
      check(blocks.get(graph.nodes.get(childId).block_hash).height >=
        blocks.get(graph.nodes.get(parentId).block_hash).height, 'Catalog receipt executed before its parent');
    }
    const eventHeader = blocks.get(eventNodes[0].block_hash);
    const block = { hash: eventHeader.hash, height: eventHeader.height };
    const accountState = await this.rpc('query', {
      request_type: 'view_account', account_id: contractId, block_id: block.hash,
    });
    const expectedCodeHash = contractId === this.nftContractId ? this.nftCodeHash : this.marketplaceCodeHash;
    check(accountState?.block_hash === block.hash && accountState.block_height === block.height &&
      accountState.code_hash === expectedCodeHash, 'Catalog event used unreviewed contract code');
    if (method === 'nft_approve') {
      const nftState = await this.rpc('query', {
        request_type: 'view_account', account_id: this.nftContractId, block_id: block.hash,
      });
      check(nftState?.block_hash === block.hash && nftState.block_height === block.height &&
        nftState.code_hash === this.nftCodeHash, 'Listing approval used unreviewed NFT code');
    }

    let facts;
    if (method === 'nft_mint') facts = await this.verifyMint(discoveredTx, call, event, block);
    else if (method === 'nft_approve') facts = await this.verifyListing(discoveredTx, call, event, block);
    else facts = await this.verifyCancellation(discoveredTx, call, event, block);
    verifiedFacts.add(facts);
    return Object.freeze(facts);
  }

  async verifyMint(tx, call, event, block) {
    check(exactKeys(call.json, ['token_id', 'token_owner_id', 'token_metadata']) &&
      call.json.token_owner_id === tx.signer_id && call.json.token_id.startsWith('catalog-') &&
      UUID.test(call.json.token_id.slice('catalog-'.length)), 'Mint does not bind a catalog UUID');
    check(Array.isArray(event.data) && event.data.length === 1 &&
      event.data[0].owner_id === tx.signer_id && Array.isArray(event.data[0].token_ids) &&
      event.data[0].token_ids.length === 1 && event.data[0].token_ids[0] === call.json.token_id,
    'Mint event conflicts with signed mint');
    const token = await this.contractState(this.nftContractId, 'nft_token', { token_id: call.json.token_id }, block);
    check(token?.token_id === call.json.token_id && token.owner_id === tx.signer_id &&
      token.metadata?.title === call.json.token_metadata.title && token.metadata?.media === call.json.token_metadata.media,
    'Minted token state conflicts with transaction');
    return {
      kind: 'mint', txHash: tx.hash, networkId: this.networkId, nftContractId: this.nftContractId,
      catalogId: call.json.token_id.slice('catalog-'.length), tokenId: call.json.token_id,
      ownerWalletAddress: tx.signer_id, blockHash: block.hash, blockHeight: block.height,
    };
  }

  async verifyListing(tx, call, event, block) {
    check(exactKeys(call.json, ['token_id', 'account_id', 'msg']) &&
      call.json.account_id === this.marketplaceContractId && typeof call.json.msg === 'string',
    'Approval is not addressed to the reviewed marketplace');
    const terms = chainJson(call.json.msg);
    check(terms?.standard === 'near_marketplace' && terms.version === '1.0.0' &&
      terms.marketplace_id === this.marketplaceContractId && terms.nft_contract_id === this.nftContractId &&
      terms.token_id === call.json.token_id && terms.seller_id === tx.signer_id,
    'Seller-signed listing terms conflict with the approval');
    check(object(event.data) && event.data.nft_contract_id === this.nftContractId &&
      event.data.token_id === call.json.token_id && event.data.seller_id === tx.signer_id &&
      event.data.price === terms.price, 'Listing event conflicts with signed terms');
    const approvalId = uint(event.data.approval_id, U64_MAX, true).toString();
    const generation = uint(event.data.generation, U64_MAX, true).toString();
    const listingId = event.data.listing_id;
    check(typeof listingId === 'string' && listingId.endsWith(`|${tx.signer_id}|${generation}`),
      'Invalid contract listing identity');
    const [listing, meta, token] = await Promise.all([
      this.contractState(this.marketplaceContractId, 'listing', { listing_id: listingId }, block),
      this.contractState(this.marketplaceContractId, 'listing_metadata', { listing_id: listingId }, block),
      this.contractState(this.nftContractId, 'nft_token', { token_id: call.json.token_id }, block),
    ]);
    check(listing?.active === true && meta?.status === 'active' && listing.nft_contract_id === this.nftContractId &&
      listing.token_id === call.json.token_id && listing.seller === tx.signer_id &&
      String(listing.approval_id) === approvalId && String(listing.price) === terms.price &&
      String(meta.generation) === generation && token?.owner_id === tx.signer_id &&
      String(token.approved_account_ids?.[this.marketplaceContractId]) === approvalId,
    'Final listing, approval or ownership state conflicts with the event');
    return {
      kind: 'listing', txHash: tx.hash, networkId: this.networkId,
      marketplaceContractId: this.marketplaceContractId, nftContractId: this.nftContractId,
      contractListingId: listingId, tokenId: call.json.token_id, sellerWalletAddress: tx.signer_id,
      priceYocto: terms.price, approvalId, generation, blockHash: block.hash, blockHeight: block.height,
    };
  }

  async verifyCancellation(tx, call, event, block) {
    check(exactKeys(call.json, ['listing_id']) && event.data?.listing_id === call.json.listing_id,
      'Cancellation event conflicts with signed call');
    const [listing, meta] = await Promise.all([
      this.contractState(this.marketplaceContractId, 'listing', { listing_id: call.json.listing_id }, block),
      this.contractState(this.marketplaceContractId, 'listing_metadata', { listing_id: call.json.listing_id }, block),
    ]);
    check(listing?.seller === tx.signer_id && listing.active === false && meta?.status === 'cancelled',
      'Final listing state does not confirm seller cancellation');
    return {
      kind: 'cancellation', txHash: tx.hash, networkId: this.networkId,
      marketplaceContractId: this.marketplaceContractId, contractListingId: call.json.listing_id,
      sellerWalletAddress: tx.signer_id, blockHash: block.hash, blockHeight: block.height,
    };
  }
}

module.exports = { CatalogChainVerifier, RejectedIndexEvent, isVerifiedCatalogFacts };
