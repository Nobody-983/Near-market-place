const runtimeEnv = import.meta.env || {};

export const NETWORK_ID = runtimeEnv.VITE_NEAR_NETWORK_ID || 'testnet';

// Verified browser-CORS-compatible production Testnet RPC endpoints
export const FASTNEAR_TESTNET_RPC = 'https://test.rpc.fastnear.com';
export const DEFAULT_TESTNET_RPC = FASTNEAR_TESTNET_RPC;
export const OFFICIAL_TESTNET_RPC = 'https://rpc.testnet.near.org';
export const DEV_RPC_PROXY = '/near-rpc';
export const TESTNET_FALLBACK_RPCS = [
  FASTNEAR_TESTNET_RPC,
  OFFICIAL_TESTNET_RPC,
];

export function resolveNodeUrl(configuredUrl) {
  if (configuredUrl) {
    if (configuredUrl.startsWith('/')) {
      if (typeof window !== 'undefined' && window.location?.origin) {
        return `${window.location.origin}${configuredUrl}`;
      }
      return `http://localhost:5173${configuredUrl}`;
    }
    return configuredUrl;
  }
  // In development, default to Vite dev proxy if no explicit URL is configured
  if (runtimeEnv.DEV) {
    if (typeof window !== 'undefined' && window.location?.origin) {
      return `${window.location.origin}${DEV_RPC_PROXY}`;
    }
    return `http://localhost:5173${DEV_RPC_PROXY}`;
  }
  // In production, default to browser-CORS-compatible endpoint
  return FASTNEAR_TESTNET_RPC;
}

export const NODE_URL = resolveNodeUrl(runtimeEnv.VITE_NEAR_NODE_URL);
export const WALLET_URL = runtimeEnv.VITE_NEAR_WALLET_URL || 'https://testnet.mynearwallet.com/';
export const HELPER_URL = runtimeEnv.VITE_NEAR_HELPER_URL || 'https://helper.testnet.near.org';
export const EXPLORER_URL = runtimeEnv.VITE_NEAR_EXPLORER_URL || 'https://testnet.nearblocks.io';

// Legacy RoundFrog Testnet deployments
export const LEGACY_NFT_CONTRACT_ID = 'nft.roundfrog-dev.testnet';
export const LEGACY_MARKETPLACE_CONTRACT_ID = 'market.roundfrog-dev.testnet';
export const LEGACY_NFT_CODE_HASH = 'DRmwjtfeBzXW2XgXMumytEY5scsDzEtCGsbVNKjdEtW8';
export const LEGACY_MARKETPLACE_CODE_HASH = 'Fp3ggF3WmSMQC7m9FU717jA8nB897sYW6bik7Ysqipw7';

// Reviewed permissionless NFT and enhanced marketplace contract hashes
export const PERMISSIONLESS_NFT_CODE_HASH = '7Xwtpb4JsXxmcdgCNEJdQVnAUQqYjWYFAvbtK7vXexh7';
export const ENHANCED_MARKETPLACE_CODE_HASH = '2vpgmXoZSpnPgcAkE6hR4dy9EnjogNLcW2vbkECADM2y';

// Configurable contract accounts with fallback to legacy
export const NFT_CONTRACT_ID = runtimeEnv.VITE_NFT_CONTRACT_ID || LEGACY_NFT_CONTRACT_ID;
export const MARKETPLACE_CONTRACT_ID = runtimeEnv.VITE_MARKETPLACE_CONTRACT_ID || LEGACY_MARKETPLACE_CONTRACT_ID;
export const EDITIONS_CONTRACT_ID = runtimeEnv.VITE_EDITIONS_CONTRACT_ID || 'editions.roundfrog-dev.testnet';

// Active reviewed aliases for backward-compatible interface and assertion checks
export const REVIEWED_NFT_CONTRACT_ID = NFT_CONTRACT_ID;
export const REVIEWED_MARKETPLACE_CONTRACT_ID = MARKETPLACE_CONTRACT_ID;
export const REVIEWED_NFT_CODE_HASH = runtimeEnv.VITE_NFT_CODE_HASH || (
  NFT_CONTRACT_ID === LEGACY_NFT_CONTRACT_ID ? LEGACY_NFT_CODE_HASH : PERMISSIONLESS_NFT_CODE_HASH
);
export const REVIEWED_MARKETPLACE_CODE_HASH = runtimeEnv.VITE_MARKETPLACE_CODE_HASH || (
  MARKETPLACE_CONTRACT_ID === LEGACY_MARKETPLACE_CONTRACT_ID ? LEGACY_MARKETPLACE_CODE_HASH : ENHANCED_MARKETPLACE_CODE_HASH
);

export const GAS = '300000000000000';
export const DEPOSIT_STORAGE = '1250000000000000000000'; // 0.00125 NEAR
