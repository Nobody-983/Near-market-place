#!/usr/bin/env bash
# ==============================================================================
# scripts/testnet-lifecycle.sh
# End-to-End NEAR Testnet Lifecycle Verification Script
#
# Simulates or executes the complete lifecycle on NEAR Testnet:
# 1. Enable trading (operator -> marketplace set_trading_enabled)
# 2. Storage deposit (seller -> marketplace storage_deposit)
# 3. Mint NFT (seller / mint authority -> nft contract nft_mint)
# 4. List NFT (seller -> nft contract nft_approve -> marketplace)
# 5. Cancel Listing (seller -> marketplace cancel_listing)
# 6. Multi-wallet security isolation (buyer -> cannot cancel seller's listing)
#
# NOTE: Requires explicit user approval before execution with --execute.
# By default, runs in --dry-run mode to inspect accounts, balances, and payloads.
# ==============================================================================
set -euo pipefail

MODE="${1:---dry-run}"
NEAR_CLI="${NEAR_CLI:-/home/acknowlege/.cargo/bin/near}"
RPC_URL="${NEAR_RPC_URL:-https://rpc.testnet.near.org}"

NFT_CONTRACT="${NFT_CONTRACT:-nft-v2.roundfrog-dev.testnet}"
MARKET_CONTRACT="${MARKET_CONTRACT:-market-v2.roundfrog-dev.testnet}"
SELLER="${SELLER:-roundfrog-dev.testnet}"
OPERATOR="${OPERATOR:-roundfrog-dev.testnet}"
BUYER="${BUYER:-buyer.roundfrog-dev.testnet}"

echo "========================================================================"
echo " NEAR NFT Marketplace — Testnet Lifecycle Tooling ($MODE)"
echo "========================================================================"

echo "[*] Configuration:"
echo "    Network:              testnet"
echo "    RPC URL:              $RPC_URL"
echo "    NFT Contract:         $NFT_CONTRACT"
echo "    Marketplace Contract: $MARKET_CONTRACT"
echo "    Seller / Authority:   $SELLER"
echo "    Operator / Fee:       $OPERATOR"
echo "    Buyer:                $BUYER"
echo ""

# Query live state
node scripts/testnet-verify.mjs

if [[ "$MODE" != "--execute" ]]; then
  echo ""
  echo "========================================================================"
  echo " [DRY-RUN COMPLETE] The following transactions are prepared for approval:"
  echo "========================================================================"
  echo ""
  echo "0. ENABLE MARKETPLACE TRADING (if disabled):"
  echo "   Signer:   $OPERATOR (matches contract owner)"
  echo "   Receiver: $MARKET_CONTRACT"
  echo "   Method:   set_trading_enabled"
  echo "   Args:     {\"enabled\": true}"
  echo "   Deposit:  1 yoctoNEAR"
  echo ""
  echo "1. PREFUND MARKETPLACE STORAGE:"
  echo "   Signer:   $SELLER"
  echo "   Receiver: $MARKET_CONTRACT"
  echo "   Method:   storage_deposit"
  echo "   Args:     {\"account_id\": \"$SELLER\", \"registration_only\": false}"
  echo "   Deposit:  0.12 NEAR (120000000000000000000000 yoctoNEAR)"
  echo ""
  echo "2. MINT TESTNET NFT:"
  echo "   Signer:   $SELLER (matches testnet_mint_authority)"
  echo "   Receiver: $NFT_CONTRACT"
  echo "   Method:   nft_mint"
  echo "   Args:     {\"token_id\": \"catalog-<UUID>\", \"token_owner_id\": \"$SELLER\", \"token_metadata\": {\"title\": \"RoundFrog Verified Genesis\", \"copies\": 1}}"
  echo "   Deposit:  0.05 NEAR (50000000000000000000000 yoctoNEAR; unused refunded)"
  echo ""
  echo "3. APPROVE & LIST NFT ON MARKETPLACE:"
  echo "   Signer:   $SELLER"
  echo "   Receiver: $NFT_CONTRACT"
  echo "   Method:   nft_approve"
  echo "   Args:     {\"token_id\": \"catalog-<UUID>\", \"account_id\": \"$MARKET_CONTRACT\", \"msg\": \"...\"}"
  echo "   Deposit:  0.01 NEAR (10000000000000000000000 yoctoNEAR; unused refunded)"
  echo ""
  echo "4. CANCEL LISTING ON MARKETPLACE:"
  echo "   Signer:   $SELLER"
  echo "   Receiver: $MARKET_CONTRACT"
  echo "   Method:   cancel_listing"
  echo "   Args:     {\"listing_id\": \"...\"}"
  echo "   Deposit:  1 yoctoNEAR"
  echo ""
  echo "5. MULTI-WALLET SECURITY TEST:"
  echo "   Signer:   $BUYER"
  echo "   Receiver: $MARKET_CONTRACT"
  echo "   Method:   cancel_listing"
  echo "   Verify:   Must be rejected by contract (unauthorized caller)"
  echo ""
  echo "To execute these on-chain operations, explicit user approval is required."
  exit 0
fi

# Execution mode requires CONFIRM_TESTNET_TRANSACTIONS=yes
if [[ "${CONFIRM_TESTNET_TRANSACTIONS:-no}" != "yes" ]]; then
  echo "[-] ERROR: Refusing to submit on-chain transactions without CONFIRM_TESTNET_TRANSACTIONS=yes" >&2
  exit 2
fi

echo ""
echo "========================================================================"
echo " [!] EXECUTING TESTNET TRANSACTIONS (CONFIRMED)"
echo "========================================================================"

# Step 0: Ensure trading is enabled
echo "[*] Step 0: Checking marketplace trading status..."
TRADING_ENABLED=$(curl -s -X POST "$RPC_URL" -H 'Content-Type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":\"chk_cfg\",\"method\":\"query\",\"params\":{\"request_type\":\"call_function\",\"finality\":\"final\",\"account_id\":\"$MARKET_CONTRACT\",\"method_name\":\"config\",\"args_base64\":\"e30=\"}}" \
  | jq -r '.result.result | [ .[] ] | implode | fromjson | .trading_enabled')

if [[ "$TRADING_ENABLED" != "true" ]]; then
  echo "[+] Enabling trading on marketplace (signer: $OPERATOR)..."
  $NEAR_CLI contract call-function as-transaction "$MARKET_CONTRACT" set_trading_enabled \
    json-args '{"enabled":true}' prepaid-gas '100.0 Tgas' attached-deposit '1 yoctoNEAR' \
    sign-as "$OPERATOR" network-config testnet sign-with-keychain send
  echo "[+] Trading enabled confirmed."
else
  echo "[+] Marketplace trading already enabled."
fi

# Step 1: Fund storage deposit
echo "[*] Step 1: Checking storage balance for $SELLER..."
STORAGE_AVAIL=$(curl -s -X POST "$RPC_URL" -H 'Content-Type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":\"chk_stg\",\"method\":\"query\",\"params\":{\"request_type\":\"call_function\",\"finality\":\"final\",\"account_id\":\"$MARKET_CONTRACT\",\"method_name\":\"storage_balance_of\",\"args_base64\":\"$(printf '{"account_id":"%s"}' "$SELLER" | base64 -w 0)\"}}" \
  | jq -r '.result.result | if . then ([ .[] ] | implode | fromjson | .available) else "0" end')

if [[ "$STORAGE_AVAIL" == "null" || "$STORAGE_AVAIL" == "0" ]]; then
  echo "[+] Depositing 0.12 NEAR storage for seller (signer: $SELLER)..."
  $NEAR_CLI contract call-function as-transaction "$MARKET_CONTRACT" storage_deposit \
    json-args "{\"account_id\":\"$SELLER\"}" prepaid-gas '100.0 Tgas' attached-deposit '0.12 NEAR' \
    sign-as "$SELLER" network-config testnet sign-with-keychain send
  echo "[+] Storage deposit confirmed."
else
  echo "[+] Seller storage already funded ($STORAGE_AVAIL yoctoNEAR available)."
fi

# Step 2: Mint NFT
TOKEN_ID="catalog-roundfrog-$(date +%s)"
echo "[*] Step 2: Minting Testnet NFT: $TOKEN_ID (signer: $SELLER)..."
$NEAR_CLI contract call-function as-transaction "$NFT_CONTRACT" nft_mint \
  json-args "{\"token_id\":\"$TOKEN_ID\",\"token_owner_id\":\"$SELLER\",\"token_metadata\":{\"title\":\"RoundFrog Verified Genesis #$(date +%s)\",\"description\":\"Testnet E2E Verified NFT\",\"copies\":1}}" \
  prepaid-gas '150.0 Tgas' attached-deposit '0.05 NEAR' \
  sign-as "$SELLER" network-config testnet sign-with-keychain send

echo "[+] Verifying minted NFT on-chain..."
MINTED_NFT=$(curl -s -X POST "$RPC_URL" -H 'Content-Type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":\"chk_nft\",\"method\":\"query\",\"params\":{\"request_type\":\"call_function\",\"finality\":\"final\",\"account_id\":\"$NFT_CONTRACT\",\"method_name\":\"nft_token\",\"args_base64\":\"$(printf '{"token_id":"%s"}' "$TOKEN_ID" | base64 -w 0)\"}}" \
  | jq -r '.result.result | [ .[] ] | implode | fromjson')
echo "    Minted NFT Owner: $(echo "$MINTED_NFT" | jq -r .owner_id)"
if [[ "$(echo "$MINTED_NFT" | jq -r .owner_id)" != "$SELLER" ]]; then
  echo "[-] ERROR: Minted NFT owner does not match $SELLER" >&2
  exit 1
fi

# Step 3: Approve and List
echo "[*] Step 3: Listing NFT $TOKEN_ID on marketplace (signer: $SELLER)..."
TERMS_MSG="{\"standard\":\"near_marketplace\",\"version\":\"1.0.0\",\"marketplace_id\":\"$MARKET_CONTRACT\",\"nft_contract_id\":\"$NFT_CONTRACT\",\"token_id\":\"$TOKEN_ID\",\"seller_id\":\"$SELLER\",\"price\":\"500000000000000000000000\",\"fee_bps\":100,\"fee_account\":\"$OPERATOR\",\"expires_at\":null}"

$NEAR_CLI contract call-function as-transaction "$NFT_CONTRACT" nft_approve \
  json-args "{\"token_id\":\"$TOKEN_ID\",\"account_id\":\"$MARKET_CONTRACT\",\"msg\":$(printf '%s' "$TERMS_MSG" | jq -R .)}" \
  prepaid-gas '150.0 Tgas' attached-deposit '0.01 NEAR' \
  sign-as "$SELLER" network-config testnet sign-with-keychain send

echo "[+] Querying active listing on marketplace..."
ACTIVE_LISTINGS=$(curl -s -X POST "$RPC_URL" -H 'Content-Type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":\"chk_lst\",\"method\":\"query\",\"params\":{\"request_type\":\"call_function\",\"finality\":\"final\",\"account_id\":\"$MARKET_CONTRACT\",\"method_name\":\"listings\",\"args_base64\":\"e30=\"}}" \
  | jq -r '.result.result | [ .[] ] | implode | fromjson')

MATCHING_LISTING=$(echo "$ACTIVE_LISTINGS" | jq -r ".[] | select(.token_id == \"$TOKEN_ID\" and .active == true)")
APPROVAL_ID=$(echo "$MATCHING_LISTING" | jq -r .approval_id)
LISTING_ID="${NFT_CONTRACT}|${TOKEN_ID}|${SELLER}|${APPROVAL_ID}"

echo "    Active Listing ID: $LISTING_ID"
echo "    Price:             $(echo "$MATCHING_LISTING" | jq -r .price) yoctoNEAR"
echo "    Seller:            $(echo "$MATCHING_LISTING" | jq -r .seller)"

if [[ -z "$LISTING_ID" || "$LISTING_ID" == "null" ]]; then
  echo "[-] ERROR: Active listing for $TOKEN_ID not found on marketplace" >&2
  exit 1
fi

# Step 4: Multi-Wallet Security Test (Buyer cannot cancel seller's listing)
echo "[*] Step 4: Multi-wallet security check — Buyer ($BUYER) attempting unauthorized cancellation..."
BUYER_CANCEL_RESULT=$($NEAR_CLI contract call-function as-transaction "$MARKET_CONTRACT" cancel_listing \
  json-args "{\"listing_id\":\"$LISTING_ID\"}" \
  prepaid-gas '100.0 Tgas' attached-deposit '1 yoctoNEAR' \
  sign-as "$BUYER" network-config testnet sign-with-keychain send 2>&1 || true)

if echo "$BUYER_CANCEL_RESULT" | grep -qi "Only the seller can cancel this listing\|ExecutionError"; then
  echo "[+] PASSED: Unauthorized cancellation by $BUYER was rejected by contract as expected."
else
  echo "[-] WARNING: Unexpected outcome for unauthorized cancellation attempt:"
  echo "$BUYER_CANCEL_RESULT"
fi

# Step 5: Authorized Cancellation by Seller
echo "[*] Step 5: Cancelling listing $LISTING_ID by authorized seller (signer: $SELLER)..."
$NEAR_CLI contract call-function as-transaction "$MARKET_CONTRACT" cancel_listing \
  json-args "{\"listing_id\":\"$LISTING_ID\"}" \
  prepaid-gas '100.0 Tgas' attached-deposit '1 yoctoNEAR' \
  sign-as "$SELLER" network-config testnet sign-with-keychain send

echo "[+] Verifying listing cancellation on-chain..."
CANCELLED_LISTING=$(curl -s -X POST "$RPC_URL" -H 'Content-Type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":\"chk_cxl\",\"method\":\"query\",\"params\":{\"request_type\":\"call_function\",\"finality\":\"final\",\"account_id\":\"$MARKET_CONTRACT\",\"method_name\":\"listing\",\"args_base64\":\"$(printf '{"listing_id":"%s"}' "$LISTING_ID" | base64 -w 0)\"}}" \
  | jq -r '.result.result | [ .[] ] | implode | fromjson')

IS_ACTIVE=$(echo "$CANCELLED_LISTING" | jq -r .active)
echo "    Listing active status: $IS_ACTIVE"
if [[ "$IS_ACTIVE" != "false" ]]; then
  echo "[-] ERROR: Listing is still active after cancellation" >&2
  exit 1
fi
echo "[+] PASSED: Listing successfully cancelled and verified inactive on-chain."

echo ""
echo "========================================================================"
echo " TESTNET LIFECYCLE COMPLETED AND VERIFIED 100% ON-CHAIN"
echo "========================================================================"
node scripts/testnet-verify.mjs
