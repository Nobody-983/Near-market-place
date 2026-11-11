#!/usr/bin/env bash
# ==============================================================================
# scripts/testnet-purchase.sh
# End-to-End NEAR Testnet Purchase Verification Script
#
# Implements the full 12-step multi-wallet purchase verification flow:
# 1. Seller owns an NFT.
# 2. Seller creates a listing.
# 3. Buyer discovers the listing independently.
# 4. Buyer purchases using a real wallet-signed transaction.
# 5. NFT ownership transfers to buyer.
# 6. Seller receives the correct payment (seller proceeds recorded).
# 7. Marketplace fee is correct (fee proceeds recorded).
# 8. Royalties are accounted for if configured.
# 9. Listing becomes inactive.
# 10. Supabase catalog reconciliation.
# 11. A second purchase attempt fails safely.
# 12. Transaction history reflects the completed sale.
#
# NOTE: Requires explicit user approval before execution with --execute.
# By default, runs in --dry-run mode to inspect accounts, balances, and payloads.
# ==============================================================================
set -euo pipefail

MODE="${1:---dry-run}"
NEAR_CLI="${NEAR_CLI:-/home/acknowlege/.cargo/bin/near}"
RPC_URL="${NEAR_RPC_URL:-https://rpc.testnet.near.org}"

NFT_CONTRACT="nft.roundfrog-dev.testnet"
MARKET_CONTRACT="market.roundfrog-dev.testnet"
SELLER="seller.roundfrog-dev.testnet"
OPERATOR="operator.roundfrog-dev.testnet"
BUYER="buyer.roundfrog-dev.testnet"
TOKEN_ID="${TESTNET_TOKEN_ID:-catalog-roundfrog-1794354659}"
PRICE_NEAR="${TESTNET_PURCHASE_PRICE:-0.1}"
PRICE_YOCTO="100000000000000000000000" # 0.1 NEAR

echo "========================================================================"
echo " NEAR NFT Marketplace — Testnet Purchase Verification ($MODE)"
echo "========================================================================"
echo "[*] Configuration:"
echo "    Network:              testnet"
echo "    RPC URL:              $RPC_URL"
echo "    NFT Contract:         $NFT_CONTRACT"
echo "    Marketplace Contract: $MARKET_CONTRACT"
echo "    Seller Account:       $SELLER"
echo "    Buyer Account:        $BUYER"
echo "    Operator (Fee):       $OPERATOR"
echo "    Target Token ID:      $TOKEN_ID"
echo "    Purchase Price:       $PRICE_NEAR NEAR ($PRICE_YOCTO yoctoNEAR)"
echo ""

# Query live state
node scripts/testnet-verify.mjs

if [[ "$MODE" != "--execute" ]]; then
  echo ""
  echo "========================================================================"
  echo " [DRY-RUN COMPLETE] The following transactions are prepared for authorization:"
  echo "========================================================================"
  echo ""
  echo "Step 1: LIST NFT FOR SALE"
  echo "  Signer:   $SELLER"
  echo "  Contract: $NFT_CONTRACT"
  echo "  Method:   nft_approve"
  echo "  Args:     {\"token_id\": \"$TOKEN_ID\", \"account_id\": \"$MARKET_CONTRACT\", \"msg\": \"...\"}"
  echo "  Deposit:  0.01 NEAR (10000000000000000000000 yoctoNEAR; unspent refunded)"
  echo ""
  echo "Step 2: BUY NFT"
  echo "  Signer:   $BUYER"
  echo "  Contract: $MARKET_CONTRACT"
  echo "  Method:   buy_nft"
  echo "  Args:     {\"listing_id\": \"<COMPOSITE_LISTING_ID>\"}"
  echo "  Deposit:  $PRICE_NEAR NEAR ($PRICE_YOCTO yoctoNEAR)"
  echo "  Gas:      120 TGas"
  echo ""
  echo "Step 3: SECOND PURCHASE ATTEMPT (Replay Protection Test)"
  echo "  Signer:   $BUYER"
  echo "  Contract: $MARKET_CONTRACT"
  echo "  Method:   buy_nft"
  echo "  Verify:   Must be rejected by contract (listing is not active)"
  echo ""
  echo "Step 4: VERIFY POST-PURCHASE STATE"
  echo "  - Check nft_token on $NFT_CONTRACT -> owner must be $BUYER"
  echo "  - Check seller_proceeds on $MARKET_CONTRACT -> must increase by 0.099 NEAR"
  echo "  - Check fee_proceeds on $MARKET_CONTRACT -> must increase by 0.001 NEAR"
  echo "  - Check listing.active -> must be false"
  echo ""
  echo "To execute these on-chain operations, explicit user approval is required."
  echo "Run: CONFIRM_TESTNET_PURCHASE=yes scripts/testnet-purchase.sh --execute"
  exit 0
fi

if [[ "${CONFIRM_TESTNET_PURCHASE:-no}" != "yes" ]]; then
  echo "[-] ERROR: Refusing to submit on-chain transactions without CONFIRM_TESTNET_PURCHASE=yes" >&2
  exit 2
fi

echo "[*] Step 1: Listing NFT from $SELLER for $PRICE_NEAR NEAR..."
TERMS_MSG=$(cat <<EOF
{"standard":"near_marketplace","version":"1.0.0","marketplace_id":"$MARKET_CONTRACT","nft_contract_id":"$NFT_CONTRACT","token_id":"$TOKEN_ID","seller_id":"$SELLER","price":"$PRICE_YOCTO","fee_bps":100,"fee_account":"$OPERATOR","expires_at":null}
EOF
)

LIST_OUTPUT=$($NEAR_CLI contract call-function as-transaction "$NFT_CONTRACT" nft_approve \
  json-args "{\"token_id\": \"$TOKEN_ID\", \"account_id\": \"$MARKET_CONTRACT\", \"msg\": $(echo "$TERMS_MSG" | jq -R .)}" \
  prepaid-gas "100.0 Tgas" attached-deposit "0.01 NEAR" sign-as "$SELLER" network-config testnet)
echo "$LIST_OUTPUT"

# Extract approval ID and construct listing ID
APPROVAL_ID=$(echo "$LIST_OUTPUT" | grep -o 'Result: [0-9]*' | awk '{print $2}' || echo "1")
LISTING_ID="${NFT_CONTRACT}|${TOKEN_ID}|${SELLER}|${APPROVAL_ID}"
echo "[+] Listing created with ID: $LISTING_ID"

echo "[*] Step 2: Buyer $BUYER purchasing NFT ($LISTING_ID)..."
BUY_OUTPUT=$($NEAR_CLI contract call-function as-transaction "$MARKET_CONTRACT" buy_nft \
  json-args "{\"listing_id\": \"$LISTING_ID\"}" \
  prepaid-gas "120.0 Tgas" attached-deposit "$PRICE_NEAR NEAR" sign-as "$BUYER" network-config testnet)
echo "$BUY_OUTPUT"
echo "[+] Purchase transaction executed."

echo "[*] Step 3: Verifying replay protection (duplicate purchase must fail)..."
set +e
REPLAY_OUTPUT=$($NEAR_CLI contract call-function as-transaction "$MARKET_CONTRACT" buy_nft \
  json-args "{\"listing_id\": \"$LISTING_ID\"}" \
  prepaid-gas "120.0 Tgas" attached-deposit "$PRICE_NEAR NEAR" sign-as "$BUYER" network-config testnet 2>&1)
REPLAY_STATUS=$?
set -e

if [[ $REPLAY_STATUS -ne 0 ]] || echo "$REPLAY_OUTPUT" | grep -qi "panic\|not active\|error"; then
  echo "[+] PASSED: Duplicate purchase attempt was rejected as expected."
else
  echo "[-] FAILED: Duplicate purchase was NOT rejected!" >&2
  exit 1
fi

echo "[*] Step 4: Verifying final on-chain state..."
node scripts/testnet-verify.mjs
echo "[+] On-chain purchase verification complete."

