const runtimeEnv = import.meta.env || {};

export const NETWORK_ID = runtimeEnv.VITE_NEAR_NETWORK_ID || 'testnet';
export const NODE_URL = runtimeEnv.VITE_NEAR_NODE_URL || 'https://rpc.testnet.near.org';
export const WALLET_URL = runtimeEnv.VITE_NEAR_WALLET_URL || 'https://testnet.mynearwallet.com/';
export const HELPER_URL = runtimeEnv.VITE_NEAR_HELPER_URL || 'https://helper.testnet.near.org';
export const EXPLORER_URL = runtimeEnv.VITE_NEAR_EXPLORER_URL || 'https://testnet.nearblocks.io';

// Reviewed RoundFrog Testnet deployment identity. Any contract change requires
// an explicit source/artifact review and a corresponding frontend release.
export const REVIEWED_NFT_CONTRACT_ID = 'nft.roundfrog-dev.testnet';
export const REVIEWED_MARKETPLACE_CONTRACT_ID = 'market.roundfrog-dev.testnet';
export const REVIEWED_NFT_CODE_HASH = 'DRmwjtfeBzXW2XgXMumytEY5scsDzEtCGsbVNKjdEtW8';
export const REVIEWED_MARKETPLACE_CODE_HASH = 'Fp3ggF3WmSMQC7m9FU717jA8nB897sYW6bik7Ysqipw7';

export const NFT_CONTRACT_ID = runtimeEnv.VITE_NFT_CONTRACT_ID || '';
export const MARKETPLACE_CONTRACT_ID = runtimeEnv.VITE_MARKETPLACE_CONTRACT_ID || '';

export const GAS = '300000000000000';
export const DEPOSIT_STORAGE = '1250000000000000000000'; // 0.00125 NEAR
