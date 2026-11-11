#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" != "--execute" || "${CONFIRM_TESTNET_DEPLOYMENT:-}" != "roundfrog-dev.testnet" ]]; then
  echo "Refusing to mutate Testnet." >&2
  echo "After explicit authorization, run:" >&2
  echo "  CONFIRM_TESTNET_DEPLOYMENT=roundfrog-dev.testnet bash $0 --execute" >&2
  exit 2
fi

root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
nft_wasm="$root_dir/contracts/nft/target/testnet/wasm32-unknown-unknown/release/near_nft_contract.wasm"
market_wasm="$root_dir/contracts/marketplace-deploy/target/testnet/wasm32-unknown-unknown/release/near_marketplace.wasm"

expected_nft_sha="b8a500297d6e93bc79c3f06834d09f31bf384816301d179d1aabb425a6a4a84d"
expected_market_sha="dc10f32c3680fa1d403baf5bbec2a71634d4e343b5a7a783a84a965aff94e846"
[[ "$(sha256sum "$nft_wasm" | cut -d ' ' -f 1)" == "$expected_nft_sha" ]]
[[ "$(sha256sum "$market_wasm" | cut -d ' ' -f 1)" == "$expected_market_sha" ]]

funding="roundfrog-dev.testnet"
nft="nft.roundfrog-dev.testnet"
market="market.roundfrog-dev.testnet"
operator="operator.roundfrog-dev.testnet"
seller="seller.roundfrog-dev.testnet"
buyer="buyer.roundfrog-dev.testnet"

# This is intentionally a fresh-account, one-shot sequence. Inspect every
# account first and do not run it after a partial deployment without review.
near account create-account fund-myself "$nft" '6 NEAR' autogenerate-new-keypair save-to-keychain \
  sign-as "$funding" network-config testnet sign-with-keychain send
near account create-account fund-myself "$market" '6 NEAR' autogenerate-new-keypair save-to-keychain \
  sign-as "$funding" network-config testnet sign-with-keychain send
near account create-account fund-myself "$operator" '1 NEAR' autogenerate-new-keypair save-to-keychain \
  sign-as "$funding" network-config testnet sign-with-keychain send
near account create-account fund-myself "$seller" '3 NEAR' autogenerate-new-keypair save-to-keychain \
  sign-as "$funding" network-config testnet sign-with-keychain send
near account create-account fund-myself "$buyer" '3 NEAR' autogenerate-new-keypair save-to-keychain \
  sign-as "$funding" network-config testnet sign-with-keychain send

nft_args="{\"mint_authority_id\":\"$seller\",\"metadata\":{\"spec\":\"nft-1.0.0\",\"name\":\"RoundFrog Testnet Collection\",\"symbol\":\"RFROG\",\"icon\":null,\"base_uri\":null,\"reference\":null,\"reference_hash\":null},\"marketplace_id\":\"$market\"}"
market_args="{\"nft_contract_id\":\"$nft\",\"owner\":\"$operator\",\"fee_bps\":100,\"fee_account\":\"$operator\"}"

# Deploy + initialize atomically. The contracts reject non-self initialization.
near contract deploy "$nft" use-file "$nft_wasm" with-init-call new_testnet json-args "$nft_args" \
  prepaid-gas '300 Tgas' attached-deposit '0 NEAR' network-config testnet sign-with-keychain send
near contract deploy "$market" use-file "$market_wasm" with-init-call new json-args "$market_args" \
  prepaid-gas '300 Tgas' attached-deposit '0 NEAR' network-config testnet sign-with-keychain send

near contract call-function as-read-only "$nft" marketplace_id json-args '{}' network-config testnet now
near contract call-function as-read-only "$nft" testnet_mint_authority json-args '{}' network-config testnet now
near contract call-function as-read-only "$market" config json-args '{}' network-config testnet now
