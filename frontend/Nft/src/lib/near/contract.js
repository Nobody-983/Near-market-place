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

const FASTNEAR_TESTNET_RPC = 'https://test.rpc.fastnear.com';
const OFFICIAL_TESTNET_RPC = 'https://rpc.testnet.near.org';

export const LEGACY_NFT_CONTRACT_ID = 'nft.roundfrog-dev.testnet';
export const LEGACY_MARKETPLACE_CONTRACT_ID = 'market.roundfrog-dev.testnet';
export const LEGACY_NFT_CODE_HASH = 'DRmwjtfeBzXW2XgXMumytEY5scsDzEtCGsbVNKjdEtW8';
export const LEGACY_MARKETPLACE_CODE_HASH = 'Fp3ggF3WmSMQC7m9FU717jA8nB897sYW6bik7Ysqipw7';
export const PERMISSIONLESS_NFT_CODE_HASH = '7Xwtpb4JsXxmcdgCNEJdQVnAUQqYjWYFAvbtK7vXexh7';
export const ENHANCED_MARKETPLACE_CODE_HASH = '2vpgmXoZSpnPgcAkE6hR4dy9EnjogNLcW2vbkECADM2y';

const runtimeEnv = import.meta.env || {};
export const EDITIONS_CONTRACT_ID = runtimeEnv.VITE_EDITIONS_CONTRACT_ID || 'editions.roundfrog-dev.testnet';
export const NFT_APPROVAL_DEPOSIT_YOCTO =
  runtimeEnv.VITE_NFT_APPROVAL_DEPOSIT_YOCTO ||
  '10000000000000000000000'; // 0.01 NEAR; the NFT contract refunds unused storage deposit.
export const NFT_MINT_DEPOSIT_YOCTO =
  runtimeEnv.VITE_NFT_MINT_DEPOSIT_YOCTO ||
  '50000000000000000000000'; // 0.05 NEAR; unused mint storage is refunded by the contract.

function getNodeUrl(wallet) {
  if (wallet?.selector?.options?.network?.nodeUrl) return wallet.selector.options.network.nodeUrl;
  const configured = runtimeEnv.VITE_NEAR_NODE_URL;
  if (configured) {
    if (configured.startsWith('/')) {
      if (typeof window !== 'undefined' && window.location?.origin) {
        return `${window.location.origin}${configured}`;
      }
      return `http://localhost:5173${configured}`;
    }
    return configured;
  }
  if (runtimeEnv.DEV) {
    if (typeof window !== 'undefined' && window.location?.origin) {
      return `${window.location.origin}/near-rpc`;
    }
    return 'http://localhost:5173/near-rpc';
  }
  return 'https://test.rpc.fastnear.com';
}

function timeoutSignal(ms) {
  if (typeof AbortSignal !== 'undefined' && typeof AbortSignal.timeout === 'function') {
    return AbortSignal.timeout(ms);
  }
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), ms);
  timer?.unref?.();
  return controller.signal;
}

function getCandidateRpcEndpoints(wallet) {
  const primary = getNodeUrl(wallet);
  const candidates = [
    primary,
    FASTNEAR_TESTNET_RPC,
    OFFICIAL_TESTNET_RPC,
  ].filter(Boolean);
  return [...new Set(candidates)];
}

function providerFor(wallet) {
  if (wallet?.provider) return wallet.provider;
  if (wallet?.selector?.options?.provider) return wallet.selector.options.provider;
  if (typeof fetch === 'function') {
    return {
      query: async (params) => {
        const endpoints = getCandidateRpcEndpoints(wallet);
        let lastError = null;
        for (let i = 0; i < endpoints.length; i++) {
          const endpoint = endpoints[i];
          try {
            const response = await fetch(endpoint, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({
                jsonrpc: '2.0',
                id: 'contract-query',
                method: 'query',
                params,
              }),
              signal: timeoutSignal(7000),
            });

            if (!response.ok) {
              if (response.status >= 500 && i < endpoints.length - 1) {
                console.warn(`[NEAR RPC] ${endpoint} returned HTTP ${response.status}. Retrying on fallback...`);
                continue;
              }
              throw new Error(`NEAR RPC query failed with HTTP ${response.status}`);
            }

            const data = await response.json();
            if (data.error) {
              if (data.error.code === -32000 && i < endpoints.length - 1) {
                console.warn(`[NEAR RPC] ${endpoint} returned error. Retrying on fallback...`);
                continue;
              }
              throw new Error(data.error.data || data.error.message || 'NEAR RPC query error');
            }
            return data.result;
          } catch (err) {
            lastError = err;
            if (i < endpoints.length - 1) {
              console.warn(`[NEAR RPC] ${endpoint} failed (${err.name}: ${err.message}). Retrying on fallback...`);
              continue;
            }
          }
        }
        throw (lastError || new Error('All configured NEAR RPC endpoints failed'));
      },
      txStatus: async (txHash, accountId, waitUntil = 'FINAL') => {
        const endpoints = getCandidateRpcEndpoints(wallet);
        let lastError = null;
        for (let i = 0; i < endpoints.length; i++) {
          const endpoint = endpoints[i];
          try {
            const response = await fetch(endpoint, {
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
              signal: timeoutSignal(7000),
            });

            if (!response.ok) {
              if (response.status >= 500 && i < endpoints.length - 1) {
                console.warn(`[NEAR RPC txStatus] ${endpoint} returned HTTP ${response.status}. Retrying on fallback...`);
                continue;
              }
              throw new Error(`NEAR RPC txStatus failed with HTTP ${response.status}`);
            }

            const data = await response.json();
            if (data.error) {
              if (data.error.code === -32000 && i < endpoints.length - 1) {
                console.warn(`[NEAR RPC txStatus] ${endpoint} returned error. Retrying on fallback...`);
                continue;
              }
              throw new Error(data.error.data || data.error.message || 'NEAR RPC txStatus error');
            }
            return data.result;
          } catch (err) {
            lastError = err;
            if (i < endpoints.length - 1) {
              console.warn(`[NEAR RPC txStatus] ${endpoint} failed (${err.name}: ${err.message}). Retrying on fallback...`);
              continue;
            }
          }
        }
        throw (lastError || new Error('All configured NEAR RPC endpoints failed'));
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
  const allowedHashes = [];
  if (contractId === NFT_CONTRACT_ID || contractId === REVIEWED_NFT_CONTRACT_ID || contractId === LEGACY_NFT_CONTRACT_ID) {
    allowedHashes.push(
      REVIEWED_NFT_CODE_HASH,
      LEGACY_NFT_CODE_HASH,
      PERMISSIONLESS_NFT_CODE_HASH,
      runtimeEnv.VITE_NFT_CODE_HASH,
    );
  } else if (contractId === MARKETPLACE_CONTRACT_ID || contractId === REVIEWED_MARKETPLACE_CONTRACT_ID || contractId === LEGACY_MARKETPLACE_CONTRACT_ID) {
    allowedHashes.push(
      REVIEWED_MARKETPLACE_CODE_HASH,
      LEGACY_MARKETPLACE_CODE_HASH,
      ENHANCED_MARKETPLACE_CODE_HASH,
      runtimeEnv.VITE_MARKETPLACE_CODE_HASH,
    );
  } else {
    throw new Error('Contract is not part of the reviewed Testnet deployment');
  }
  const filteredHashes = allowedHashes.filter(Boolean);
  const provider = providerFor(wallet);
  if (!provider) throw new Error('Wallet not connected');
  const account = await provider.query({
    request_type: 'view_account',
    account_id: contractId,
    finality: 'final',
  });
  if (!filteredHashes.includes(account?.code_hash)) {
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
  const signerAccount = await connectedAccountId(wallet);
  if (!signerAccount) throw new Error('No connected account');
  const walletAccount = await wallet.selector.wallet();

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

export function nftTokensForOwner(accountId, wallet, { fromIndex = '0', limit = 50, nftContractId = NFT_CONTRACT_ID } = {}) {
  return view(nftContractId, 'nft_tokens_for_owner', { account_id: accountId, from_index: String(fromIndex), limit: Number(limit) }, wallet);
}

export function nftTokens(wallet, { fromIndex = '0', limit = 50, nftContractId = NFT_CONTRACT_ID } = {}) {
  return view(nftContractId, 'nft_tokens', { from_index: String(fromIndex), limit: Number(limit) }, wallet);
}

export function nftTotalSupply(wallet, nftContractId = NFT_CONTRACT_ID) {
  return view(nftContractId, 'nft_total_supply', {}, wallet);
}

export function getMintAuthority(wallet, nftContractId = NFT_CONTRACT_ID) {
  return view(nftContractId, 'testnet_mint_authority', {}, wallet);
}

export async function marketplaceListings(wallet, { fromIndex = '0', limit = 50 } = {}) {
  const list = await view(MARKETPLACE_CONTRACT_ID, 'listings', { from_index: String(fromIndex), limit: Number(limit) }, wallet);
  return (list || []).map((item) => ({
    ...item,
    listing_id: item.listing_id || `${item.nft_contract_id}|${item.token_id}|${item.seller}|${item.approval_id}`,
  }));
}

export function nftMarketplaceId(wallet, nftContractId = NFT_CONTRACT_ID) {
  return view(nftContractId, 'marketplace_id', {}, wallet);
}

export async function connectedAccountId(wallet) {
  if (wallet?.accountId) return wallet.accountId;
  if (wallet?.selector) {
    const active = wallet.selector.store?.getState?.().accounts
      ?.find((account) => account.active)?.accountId;
    if (active) return active;
    try {
      const selected = await wallet.selector.wallet();
      const accounts = typeof selected?.getAccounts === 'function'
        ? await selected.getAccounts()
        : selected?.accounts;
      const accountId = accounts?.[0]?.accountId;
      if (accountId) return accountId;
    } catch {
      // Wallet module might not be selected yet
    }
  }
  const accountId = wallet?.getAccountId?.();
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

export async function verifyMintPreconditions({ signerId, wallet, allowPermissionless = true }) {
  assertReviewedTestnetConfiguration();
  if (!signerId) throw new Error('Connect a NEAR Testnet wallet first');
  await requireConnectedAccount(signerId, wallet);
  const [, , boundMarketplace, mintAuthority] = await Promise.all([
    assertReviewedContractCode(NFT_CONTRACT_ID, wallet),
    assertReviewedContractCode(MARKETPLACE_CONTRACT_ID, wallet),
    nftMarketplaceId(wallet, NFT_CONTRACT_ID),
    view(NFT_CONTRACT_ID, 'testnet_mint_authority', {}, wallet).catch(() => null),
  ]);
  if (boundMarketplace !== MARKETPLACE_CONTRACT_ID) {
    throw new Error('NFT contract is not bound to the reviewed marketplace');
  }
  const normalizedSigner = String(signerId).trim().toLowerCase();
  const normalizedAuthority = mintAuthority ? String(mintAuthority).trim().toLowerCase() : null;
  const isAuthority = normalizedAuthority ? normalizedAuthority === normalizedSigner : false;
  const isLegacy = NFT_CONTRACT_ID === LEGACY_NFT_CONTRACT_ID;
  if (!allowPermissionless || (isLegacy && !isAuthority)) {
    if (!isAuthority) {
      throw new Error(`Connected wallet (${signerId}) is not the configured Testnet mint authority (${mintAuthority}). Only ${mintAuthority} can mint new NFTs on this contract.`);
    }
  }
  return { marketplaceId: boundMarketplace, mintAuthority, isAuthority };
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
  if (nftContractId !== NFT_CONTRACT_ID && nftContractId !== LEGACY_NFT_CONTRACT_ID) {
    throw new Error('NFT contract is not part of the reviewed Testnet deployment');
  }
  const signerId = ownerId || await connectedAccountId(wallet);
  const isLegacy = nftContractId === LEGACY_NFT_CONTRACT_ID;
  const { mintAuthority, isAuthority } = await verifyMintPreconditions({
    signerId,
    wallet,
    allowPermissionless: !isLegacy,
  });
  if (isLegacy && !isAuthority) {
    throw new Error(`Connected wallet (${signerId}) is not the configured Testnet mint authority (${mintAuthority}). Connect with ${mintAuthority} to mint this NFT.`);
  }
  return call(
    nftContractId,
    'nft_mint',
    {
      token_id: tokenId,
      token_owner_id: signerId,
      token_metadata: metadata,
    },
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
  const outcome = Array.isArray(result) ? result[0] : result;
  const hash = outcome?.transaction?.hash ||
    outcome?.transaction_outcome?.id ||
    (typeof outcome === 'string' ? outcome : null);
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

// =========================================================
// NEP-245 MULTI-TOKEN EDITION HELPERS
// =========================================================

export function mtToken(tokenId, wallet, editionsContractId = EDITIONS_CONTRACT_ID) {
  return view(editionsContractId, 'mt_token', { token_id: tokenId }, wallet);
}

export function mtTokens(wallet, { fromIndex = '0', limit = 50, editionsContractId = EDITIONS_CONTRACT_ID } = {}) {
  return view(editionsContractId, 'mt_tokens', { from_index: String(fromIndex), limit: Number(limit) }, wallet);
}

export function mtTokensForOwner(accountId, wallet, { fromIndex = '0', limit = 50, editionsContractId = EDITIONS_CONTRACT_ID } = {}) {
  return view(editionsContractId, 'mt_tokens_for_owner', { account_id: accountId, from_index: String(fromIndex), limit: Number(limit) }, wallet);
}

export function mtBalanceOf(accountId, tokenId, wallet, editionsContractId = EDITIONS_CONTRACT_ID) {
  return view(editionsContractId, 'mt_balance_of', { account_id: accountId, token_id: tokenId }, wallet);
}

export function editionListing(listingId, wallet, editionsContractId = EDITIONS_CONTRACT_ID) {
  return view(editionsContractId, 'edition_listing', { listing_id: listingId }, wallet);
}

export function editionListings(wallet, { fromIndex = 0, limit = 50, editionsContractId = EDITIONS_CONTRACT_ID } = {}) {
  return view(editionsContractId, 'edition_listings', { from_index: Number(fromIndex), limit: Number(limit) }, wallet);
}

export function editionListingsForToken(tokenId, wallet, editionsContractId = EDITIONS_CONTRACT_ID) {
  return view(editionsContractId, 'edition_listings_for_token', { token_id: tokenId }, wallet);
}

export function editionListingsForSeller(sellerId, wallet, editionsContractId = EDITIONS_CONTRACT_ID) {
  return view(editionsContractId, 'edition_listings_for_seller', { seller_id: sellerId }, wallet);
}

export async function mintEditionOnChain({
  tokenId,
  supply,
  metadata,
  wallet,
  editionsContractId = EDITIONS_CONTRACT_ID,
}) {
  const signerId = await connectedAccountId(wallet);
  await requireConnectedAccount(signerId, wallet);
  return call(
    editionsContractId,
    'mt_mint',
    {
      token_id: tokenId,
      supply: String(supply),
      token_metadata: metadata,
    },
    NFT_MINT_DEPOSIT_YOCTO,
    wallet,
    '150000000000000',
  );
}

export async function listEditionOnChain({
  tokenId,
  quantity,
  pricePerCopyNear,
  wallet,
  editionsContractId = EDITIONS_CONTRACT_ID,
}) {
  const signerId = await connectedAccountId(wallet);
  await requireConnectedAccount(signerId, wallet);
  const priceYocto = nearToYocto(validateNearAmount(pricePerCopyNear, { allowZero: false }));
  return call(
    editionsContractId,
    'mt_list',
    {
      token_id: tokenId,
      quantity: String(quantity),
      price_per_copy: priceYocto,
    },
    '1',
    wallet,
    '150000000000000',
  );
}

export async function buyEditionOnChain({
  listingId,
  quantity,
  pricePerCopyNear,
  wallet,
  editionsContractId = EDITIONS_CONTRACT_ID,
}) {
  const signerId = await connectedAccountId(wallet);
  await requireConnectedAccount(signerId, wallet);
  const pricePerCopyYocto = BigInt(nearToYocto(validateNearAmount(pricePerCopyNear, { allowZero: false })));
  const totalDepositYocto = (pricePerCopyYocto * BigInt(quantity)).toString();
  return call(
    editionsContractId,
    'buy_edition',
    {
      listing_id: listingId,
      quantity: String(quantity),
    },
    totalDepositYocto,
    wallet,
    '150000000000000',
  );
}

export async function cancelEditionListingOnChain({
  listingId,
  wallet,
  editionsContractId = EDITIONS_CONTRACT_ID,
}) {
  const signerId = await connectedAccountId(wallet);
  await requireConnectedAccount(signerId, wallet);
  return call(
    editionsContractId,
    'cancel_edition_listing',
    { listing_id: listingId },
    '1',
    wallet,
    '100000000000000',
  );
}
