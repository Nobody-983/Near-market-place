// NEAR contract interaction helpers for the marketplace.
import { nearToYocto, validateNearAmount } from './utils';
import {
  MARKETPLACE_CONTRACT_ID,
  NETWORK_ID,
  NFT_CONTRACT_ID,
  REVIEWED_MARKETPLACE_CODE_HASH,
  REVIEWED_MARKETPLACE_CONTRACT_ID,
  REVIEWED_NFT_CODE_HASH,
  REVIEWED_NFT_CONTRACT_ID,
} from '../../config/near';
import { LISTING_STORAGE_TARGET_YOCTO } from '../../config/marketplace';

const runtimeEnv = import.meta.env || {};
export const NFT_APPROVAL_DEPOSIT_YOCTO =
  runtimeEnv.VITE_NFT_APPROVAL_DEPOSIT_YOCTO ||
  '10000000000000000000000'; // 0.01 NEAR; the NFT contract refunds unused storage deposit.
export const NFT_MINT_DEPOSIT_YOCTO =
  runtimeEnv.VITE_NFT_MINT_DEPOSIT_YOCTO ||
  '50000000000000000000000'; // 0.05 NEAR; unused mint storage is refunded by the contract.

function providerFor(wallet) {
  if (wallet?.provider) return wallet.provider;
  if (wallet?.selector?.options?.provider) return wallet.selector.options.provider;
  const nodeUrl = wallet?.selector?.options?.network?.nodeUrl ||
    (NETWORK_ID === 'testnet' ? 'https://rpc.testnet.near.org' : null);
  if (nodeUrl && typeof fetch === 'function') {
    return {
      query: async (params) => {
        const response = await fetch(nodeUrl, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            jsonrpc: '2.0',
            id: 'contract-query',
            method: 'query',
            params,
          }),
        });
        if (!response.ok) throw new Error('NEAR RPC query failed');
        const data = await response.json();
        if (data.error) throw new Error(data.error.data || data.error.message || 'NEAR RPC query error');
        return data.result;
      },
      txStatus: async (txHash, accountId, waitUntil = 'FINAL') => {
        const response = await fetch(nodeUrl, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            jsonrpc: '2.0',
            id: 'contract-tx-status',
            method: 'tx',
            params: {
              tx_hash: txHash,
              sender_account_id: accountId,
              wait_until: waitUntil,
            },
          }),
        });
        if (!response.ok) throw new Error('NEAR RPC txStatus failed');
        const data = await response.json();
        if (data.error) throw new Error(data.error.data || data.error.message || 'NEAR RPC txStatus error');
        return data.result;
      },
    };
  }
  return null;
}

function assertReviewedTestnetConfiguration() {
  if (NETWORK_ID !== 'testnet' ||
      NFT_CONTRACT_ID !== REVIEWED_NFT_CONTRACT_ID ||
      MARKETPLACE_CONTRACT_ID !== REVIEWED_MARKETPLACE_CONTRACT_ID) {
    throw new Error('Configured NEAR contracts do not match the reviewed Testnet deployment');
  }
}

export async function assertReviewedContractCode(contractId, wallet) {
  assertReviewedTestnetConfiguration();
  const expectedCodeHash = contractId === REVIEWED_NFT_CONTRACT_ID
    ? REVIEWED_NFT_CODE_HASH
    : contractId === REVIEWED_MARKETPLACE_CONTRACT_ID
      ? REVIEWED_MARKETPLACE_CODE_HASH
      : null;
  if (!expectedCodeHash) throw new Error('Contract is not part of the reviewed Testnet deployment');
  const provider = providerFor(wallet);
  if (!provider) throw new Error('Wallet not connected');
  const account = await provider.query({
    request_type: 'view_account',
    account_id: contractId,
    finality: 'final',
  });
  if (account?.code_hash !== expectedCodeHash) {
    throw new Error(`Deployed code hash for ${contractId} does not match the reviewed Testnet contract`);
  }
}

function decodeRpcBytes(value) {
  if (typeof value === 'string') return globalThis.Buffer.from(value, 'base64').toString('utf8');
  if (Array.isArray(value) || value instanceof Uint8Array) {
    return globalThis.Buffer.from(value).toString('utf8');
  }
  throw new Error('NEAR RPC returned an invalid contract result');
}

export async function view(contractId, methodName, args = {}, wallet) {
  if (!contractId) throw new Error('Contract is not configured');
  const provider = providerFor(wallet);
  if (!provider) throw new Error('Wallet not connected');
  const result = await provider.query({
    request_type: 'call_function',
    account_id: contractId,
    method_name: methodName,
    args_base64: globalThis.Buffer.from(JSON.stringify(args)).toString('base64'),
    finality: 'final',
  });
  const text = decodeRpcBytes(result?.result);
  return text ? JSON.parse(text) : null;
}

export async function call(contractId, methodName, args = {}, attachedYocto = '0', wallet, gas = '100000000000000') {
  if (wallet && typeof wallet.signAndSendTransaction === 'function' && !wallet.selector) {
    const tx = {
      receiverId: contractId,
      actions: [
        {
          type: 'FunctionCall',
          params: {
            methodName,
            args,
            gas,
            deposit: attachedYocto,
          },
        },
      ],
    };
    tx.actions[0].params.attachedDeposit = attachedYocto;
    tx.actions[0].params.attached_deposit = attachedYocto;
    return wallet.signAndSendTransaction(tx);
  }
  if (!wallet?.selector) throw new Error('Wallet not connected');
  const walletAccount = await wallet.selector.wallet();
  const signerAccount = walletAccount.accounts[0]?.accountId;
  if (!signerAccount) throw new Error('No connected account');

  const params = {
    methodName,
    args,
    gas,
    deposit: attachedYocto,
    attachedDeposit: attachedYocto,
    attached_deposit: attachedYocto,
  };
  const result = await walletAccount.signAndSendTransaction({
    signerId: signerAccount,
    receiverId: contractId,
    actions: [
      {
        type: 'FunctionCall',
        params,
      },
    ],
  });
  return result;
}

export function marketplaceConfig(wallet) {
  return view(MARKETPLACE_CONTRACT_ID, 'config', {}, wallet);
}

export function marketplaceLedger(wallet) {
  return view(MARKETPLACE_CONTRACT_ID, 'ledger', {}, wallet);
}

export function marketplaceListing(listingId, wallet) {
  return view(MARKETPLACE_CONTRACT_ID, 'listing', { listing_id: listingId }, wallet);
}

export function marketplaceListingMetadata(listingId, wallet) {
  return view(MARKETPLACE_CONTRACT_ID, 'listing_metadata', { listing_id: listingId }, wallet);
}

export function marketplaceSale(saleId, wallet) {
  return view(MARKETPLACE_CONTRACT_ID, 'sale', { sale_id: saleId }, wallet);
}

export function marketplaceSettlement(saleId, wallet) {
  return view(MARKETPLACE_CONTRACT_ID, 'settlement', { sale_id: saleId }, wallet);
}

export function marketplaceTransactions(wallet, fromIndex = '0', limit = 50) {
  return view(MARKETPLACE_CONTRACT_ID, 'transactions', { from_index: fromIndex, limit }, wallet);
}

export function marketplaceStorageBalance(accountId, wallet) {
  return view(MARKETPLACE_CONTRACT_ID, 'storage_balance_of', { account_id: accountId }, wallet);
}

export function marketplaceStorageBounds(wallet) {
  return view(MARKETPLACE_CONTRACT_ID, 'storage_balance_bounds', {}, wallet);
}

export function marketplaceSellerProceeds(accountId, wallet) {
  return view(MARKETPLACE_CONTRACT_ID, 'seller_proceeds', { seller_id: accountId }, wallet);
}

export function marketplaceFeeProceeds(accountId, wallet) {
  return view(MARKETPLACE_CONTRACT_ID, 'fee_proceeds', { account_id: accountId }, wallet);
}

export function marketplaceBuyerRefund(accountId, wallet) {
  return view(MARKETPLACE_CONTRACT_ID, 'buyer_refund', { buyer_id: accountId }, wallet);
}

export function nftToken(tokenId, wallet, nftContractId = NFT_CONTRACT_ID) {
  return view(nftContractId, 'nft_token', { token_id: tokenId }, wallet);
}

export function nftMarketplaceId(wallet, nftContractId = NFT_CONTRACT_ID) {
  return view(nftContractId, 'marketplace_id', {}, wallet);
}

export async function connectedAccountId(wallet) {
  if (wallet?.selector) {
    const active = wallet.selector.store?.getState?.().accounts
      ?.find((account) => account.active)?.accountId;
    if (active) return active;
    try {
      const selected = await wallet.selector.wallet();
      const accountId = selected?.accounts?.[0]?.accountId;
      if (accountId) return accountId;
    } catch {
      // Wallet module might not be selected yet
    }
  }
  const accountId = wallet?.accountId || wallet?.getAccountId?.();
  if (!accountId) throw new Error('No connected NEAR account');
  return accountId;
}

export async function requireConnectedAccount(expectedAccountId, wallet) {
  const currentAccountId = await connectedAccountId(wallet);
  if (currentAccountId !== expectedAccountId) {
    throw new Error('The connected wallet changed during this transaction. Refresh before continuing.');
  }
  return currentAccountId;
}

export async function listNftOnChain({ nftContractId, tokenId, priceNear, expiresAt, wallet }) {
  assertReviewedTestnetConfiguration();
  await Promise.all([
    assertReviewedContractCode(nftContractId, wallet),
    assertReviewedContractCode(MARKETPLACE_CONTRACT_ID, wallet),
  ]);
  const price = nearToYocto(priceNear, { allowZero: false });
  const [config, sellerId] = await Promise.all([marketplaceConfig(wallet), connectedAccountId(wallet)]);
  if (!config.trading_enabled) throw new Error('Marketplace trading is disabled');
  if (!config.allowed_nft_contracts?.includes(nftContractId)) throw new Error('NFT contract is not allowlisted');
  const msg = JSON.stringify({
    standard: 'near_marketplace', version: '1.0.0', marketplace_id: MARKETPLACE_CONTRACT_ID,
    nft_contract_id: nftContractId, token_id: tokenId, seller_id: sellerId, price,
    fee_bps: config.fee_bps, fee_account: config.fee_account,
    expires_at: expiresAt == null ? null : String(expiresAt),
  });
  const approveResult = await call(
    nftContractId,
    'nft_approve',
    { token_id: tokenId, account_id: MARKETPLACE_CONTRACT_ID, msg },
    NFT_APPROVAL_DEPOSIT_YOCTO,
    wallet,
    '120000000000000',
  );
  return approveResult;
}

export async function verifyListingPreconditions({ nftContractId, tokenId, sellerId, wallet }) {
  assertReviewedTestnetConfiguration();
  if (nftContractId !== NFT_CONTRACT_ID) throw new Error('NFT belongs to an unreviewed contract');
  await requireConnectedAccount(sellerId, wallet);
  const [, , config, boundMarketplace, token] = await Promise.all([
    assertReviewedContractCode(nftContractId, wallet),
    assertReviewedContractCode(MARKETPLACE_CONTRACT_ID, wallet),
    marketplaceConfig(wallet),
    nftMarketplaceId(wallet, nftContractId),
    nftToken(tokenId, wallet, nftContractId),
  ]);
  if (config?.state_version !== 2) throw new Error('Marketplace state version is not approved');
  if (!config.trading_enabled) throw new Error('Marketplace trading is paused on NEAR Testnet');
  if (!config.allowed_nft_contracts?.includes(nftContractId)) {
    throw new Error('NFT contract is not allowlisted by the marketplace');
  }
  if (boundMarketplace !== MARKETPLACE_CONTRACT_ID) {
    throw new Error('NFT contract is not bound to the configured marketplace');
  }
  if (!token || token.token_id !== tokenId || token.owner_id !== sellerId) {
    throw new Error('Connected wallet is not the current on-chain NFT owner');
  }
  return { config, token };
}

export async function verifyMintPreconditions({ signerId, wallet }) {
  assertReviewedTestnetConfiguration();
  if (!signerId) throw new Error('Connect the configured NEAR Testnet mint-authority wallet first');
  await requireConnectedAccount(signerId, wallet);
  const [, , boundMarketplace, mintAuthority] = await Promise.all([
    assertReviewedContractCode(NFT_CONTRACT_ID, wallet),
    assertReviewedContractCode(MARKETPLACE_CONTRACT_ID, wallet),
    nftMarketplaceId(wallet, NFT_CONTRACT_ID),
    view(NFT_CONTRACT_ID, 'testnet_mint_authority', {}, wallet),
  ]);
  if (boundMarketplace !== MARKETPLACE_CONTRACT_ID) {
    throw new Error('NFT contract is not bound to the reviewed marketplace');
  }
  if (mintAuthority !== signerId) {
    throw new Error('Connected wallet is not the configured Testnet mint authority');
  }
  return { marketplaceId: boundMarketplace, mintAuthority };
}

export async function ensureMarketplaceListingStorage({ sellerId, wallet }) {
  const target = BigInt(LISTING_STORAGE_TARGET_YOCTO);
  if (target <= 0n) throw new Error('Invalid marketplace listing storage target');
  const [balance, bounds] = await Promise.all([
    marketplaceStorageBalance(sellerId, wallet),
    marketplaceStorageBounds(wallet),
  ]);
  const available = BigInt(balance?.available || '0');
  if (available >= target) return { funded: false, balance };
  const minimum = BigInt(bounds?.min || '0');
  const shortfall = target - available;
  const deposit = balance ? shortfall : shortfall + minimum;
  const result = await depositMarketplaceStorage(deposit.toString(), wallet, sellerId);
  const txHash = transactionHash(result);
  const finalResult = await waitForFinal(txHash, sellerId, wallet);
  assertFinalSuccess(finalResult, 'Marketplace storage deposit');
  await requireConnectedAccount(sellerId, wallet);
  const confirmed = await marketplaceStorageBalance(sellerId, wallet);
  if (BigInt(confirmed?.available || '0') < target) {
    throw new Error('Marketplace storage funding was not confirmed');
  }
  return { funded: true, transactionHash: txHash, balance: confirmed };
}

export async function mintNftOnChain({ tokenId, metadata, wallet, ownerId, nftContractId = NFT_CONTRACT_ID }) {
  assertReviewedTestnetConfiguration();
  if (nftContractId !== REVIEWED_NFT_CONTRACT_ID) {
    throw new Error('NFT contract is not part of the reviewed Testnet deployment');
  }
  const signerId = ownerId || await connectedAccountId(wallet);
  await verifyMintPreconditions({ signerId, wallet });
  return call(
    nftContractId,
    'nft_mint',
    { token_id: tokenId, token_owner_id: signerId, token_metadata: metadata },
    NFT_MINT_DEPOSIT_YOCTO,
    wallet,
    '150000000000000',
  );
}

export async function cancelListingOnChain(listingId, wallet) {
  await assertReviewedContractCode(MARKETPLACE_CONTRACT_ID, wallet);
  return call(MARKETPLACE_CONTRACT_ID, 'cancel_listing', { listing_id: listingId }, '1', wallet);
}

export async function expireListingOnChain(listingId, wallet) {
  return call(MARKETPLACE_CONTRACT_ID, 'expire_listing', { listing_id: listingId }, '0', wallet);
}

export async function buyNftOnChain(listingId, priceYocto, wallet) {
  return call(MARKETPLACE_CONTRACT_ID, 'buy_nft', { listing_id: listingId }, priceYocto, wallet, '120000000000000');
}

export async function depositMarketplaceStorage(amountYocto, wallet, accountId = undefined) {
  return call(MARKETPLACE_CONTRACT_ID, 'storage_deposit', {
    account_id: accountId, registration_only: false,
  }, amountYocto, wallet, '30000000000000');
}

export async function withdrawMarketplaceStorage(amountYocto, wallet) {
  return call(MARKETPLACE_CONTRACT_ID, 'storage_withdraw', {
    amount: amountYocto == null ? null : amountYocto,
  }, '1', wallet, '50000000000000');
}

export async function withdrawProceedsOnChain(amountYocto, wallet) {
  return call(
    MARKETPLACE_CONTRACT_ID,
    'withdraw_proceeds',
    { amount: amountYocto },
    '1',
    wallet,
  );
}

export async function withdrawFeesOnChain(amountYocto, wallet) {
  return call(
    MARKETPLACE_CONTRACT_ID,
    'withdraw_fees',
    { amount: amountYocto },
    '1',
    wallet,
  );
}

export async function withdrawRefundOnChain(saleId, wallet) {
  return call(MARKETPLACE_CONTRACT_ID, 'withdraw_refund', { sale_id: saleId }, '1', wallet);
}

export async function waitForFinal(txHash, signerAccount, wallet, timeoutMs = 60000) {
  if (!/^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(txHash || '')) {
    throw new Error('Wallet returned an invalid NEAR transaction hash');
  }
  const provider = providerFor(wallet);
  if (!provider) throw new Error('Wallet not connected');
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const result = await provider.txStatus(txHash, signerAccount, 'FINAL');
      if (result.status && typeof result.status === 'object' && 'SuccessValue' in result.status) {
        return result;
      }
      if (result.status && typeof result.status === 'object' && 'Failure' in result.status) {
        return result;
      }
    } catch {
      // A finalized status may not be available immediately after wallet return.
    }
    await new Promise((r) => setTimeout(r, 2000));
  }
  throw new Error('Transaction confirmation timed out');
}


export async function waitForSettlement(saleId, wallet, timeoutMs = 60000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const settlement = await marketplaceSettlement(saleId, wallet);
    if (settlement && settlement.status !== 'nft_transfer_pending') return settlement;
    await new Promise((resolve) => setTimeout(resolve, 2000));
  }
  throw new Error('Marketplace settlement confirmation timed out');
}

export function decodeSuccessValue(finalResult) {
  const encoded = finalResult?.status?.SuccessValue;
  if (typeof encoded !== 'string') throw new Error('Transaction did not return a success value');
  const text = globalThis.Buffer.from(encoded, 'base64').toString('utf8');
  return text ? JSON.parse(text) : null;
}

export function transactionHash(result) {
  const hash = result?.transaction?.hash;
  if (!/^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(hash || '')) {
    throw new Error('Wallet did not return a valid NEAR transaction hash');
  }
  return hash;
}

export function assertFinalSuccess(finalResult, label = 'Transaction') {
  if (!finalResult || finalResult.status?.Failure) throw new Error(`${label} failed on NEAR`);
  const failedReceipt = finalResult.receipts_outcome?.some((receipt) => receipt?.outcome?.status?.Failure);
  if (failedReceipt) throw new Error(`${label} failed in a NEAR receipt`);
  if (!finalResult.status || typeof finalResult.status !== 'object' || !('SuccessValue' in finalResult.status)) {
    throw new Error(`${label} did not reach a final successful outcome`);
  }
  return finalResult;
}

export function listingIdFromFinalResult(finalResult, { nftContractId, tokenId, sellerId }) {
  assertFinalSuccess(finalResult, 'Listing approval');
  const values = [finalResult.status, ...(finalResult.receipts_outcome || []).map((item) => item?.outcome?.status)]
    .filter((status) => status && typeof status.SuccessValue === 'string')
    .map((status) => {
      try {
        const text = globalThis.Buffer.from(status.SuccessValue, 'base64').toString('utf8');
        return text ? JSON.parse(text) : null;
      } catch {
        return null;
      }
    });
  const prefix = `${nftContractId}|${tokenId}|${sellerId}|`;
  const listingId = values.find((value) => typeof value === 'string' && value.startsWith(prefix));
  if (!listingId) throw new Error('Final NEAR receipts did not return the expected listing identity');
  return listingId;
}

// Compatibility exports for legacy tests
export async function listNFTForSale(wallet, { tokenId, price }) {
  const priceStr = validateNearAmount(price, { allowZero: false });
  const priceYocto = nearToYocto(priceStr);
  const msg = JSON.stringify({ price: priceYocto });
  if (wallet && typeof wallet.signAndSendTransaction === 'function') {
    const tx = {
      receiverId: NFT_CONTRACT_ID,
      actions: [
        {
          type: 'FunctionCall',
          params: {
            methodName: 'nft_approve',
            args: { token_id: tokenId, account_id: MARKETPLACE_CONTRACT_ID, msg },
            gas: '300000000000000',
            deposit: '1',
          },
        },
      ],
    };
    await wallet.signAndSendTransaction(tx);
    return tx;
  }
  return call(NFT_CONTRACT_ID, 'nft_approve', { token_id: tokenId, account_id: MARKETPLACE_CONTRACT_ID, msg }, '1', wallet);
}

export async function buyNFT(wallet, { saleId, price }) {
  const priceStr = validateNearAmount(price, { allowZero: false });
  const priceYocto = nearToYocto(priceStr);
  if (wallet && typeof wallet.signAndSendTransaction === 'function') {
    const tx = {
      receiverId: MARKETPLACE_CONTRACT_ID,
      actions: [
        {
          type: 'FunctionCall',
          params: {
            methodName: 'buy_nft',
            args: { listing_id: saleId },
            gas: '300000000000000',
            deposit: priceYocto,
          },
        },
      ],
    };
    await wallet.signAndSendTransaction(tx);
    return tx;
  }
  return call(MARKETPLACE_CONTRACT_ID, 'buy_nft', { listing_id: saleId }, priceYocto, wallet);
}
