#!/usr/bin/env bash
# ==============================================================================
# scripts/test-all.sh
# Comprehensive Test & Quality Verification Suite for NEAR NFT Marketplace
# ==============================================================================
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

export PATH="$HOME/.nvm/versions/node/v22.23.4/bin:$PATH"

echo "========================================================================"
echo " 1. Contract Reproducibility & Specification Check"
echo "========================================================================"
bash "$ROOT_DIR/scripts/verify-contracts.sh"

echo ""
echo "========================================================================"
echo " 2. JavaScript Test Suite (Node.js v22 --experimental-vm-modules)"
echo "========================================================================"
node --experimental-vm-modules --test tests/*.test.mjs

echo ""
echo "========================================================================"
echo " 3. Python Testnet Tests"
echo "========================================================================"
python3 -m unittest discover -s contracts/testnet -p "test_*.py"

echo ""
echo "========================================================================"
echo " 4. Rust Smart Contract Test Suites"
echo "========================================================================"
echo "[*] Running NFT Contract tests (default)..."
cargo test --manifest-path contracts/nft/Cargo.toml --locked --target x86_64-unknown-linux-gnu

echo "[*] Running NFT Contract tests (testnet feature)..."
cargo test --manifest-path contracts/nft/Cargo.toml --locked --target x86_64-unknown-linux-gnu --no-default-features --features testnet

echo "[*] Running Marketplace Contract tests..."
cargo test --manifest-path contracts/marketplace-deploy/Cargo.toml --locked --target x86_64-unknown-linux-gnu

echo ""
echo "========================================================================"
echo " 5. Rust Code Quality (Formatting & Clippy)"
echo "========================================================================"
echo "[*] Checking Rust formatting..."
cargo fmt --check --manifest-path contracts/nft/Cargo.toml
cargo fmt --check --manifest-path contracts/marketplace-deploy/Cargo.toml

echo "[*] Running Clippy on wasm targets..."
cargo clippy --manifest-path contracts/nft/Cargo.toml --lib --target wasm32-unknown-unknown -- -D warnings
cargo clippy --manifest-path contracts/marketplace-deploy/Cargo.toml --lib --target wasm32-unknown-unknown -- -D warnings

echo "[*] Running Clippy on host test targets..."
cargo clippy --manifest-path contracts/nft/Cargo.toml --tests --target x86_64-unknown-linux-gnu -- -D warnings
cargo clippy --manifest-path contracts/marketplace-deploy/Cargo.toml --tests --target x86_64-unknown-linux-gnu -- -D warnings

echo ""
echo "========================================================================"
echo " 6. Frontend Code Quality (ESLint & Production Build)"
echo "========================================================================"
npm --prefix frontend/Nft run lint
npm --prefix frontend/Nft run build

echo ""
echo "========================================================================"
echo " 7. PostgreSQL Migration & Security Tests (Disposable Cluster)"
echo "========================================================================"
bash -c '
set -euo pipefail
PG_DIR="/tmp/pg_test_cluster_suite"
rm -rf "$PG_DIR"
/usr/lib/postgresql/15/bin/initdb -D "$PG_DIR" --no-sync > /dev/null
/usr/lib/postgresql/15/bin/pg_ctl -D "$PG_DIR" -l "$PG_DIR/logfile" -o "-k /tmp -p 54333" start > /dev/null
trap "/usr/lib/postgresql/15/bin/pg_ctl -D \"$PG_DIR\" -m immediate stop > /dev/null 2>&1 || true; rm -rf \"$PG_DIR\"" EXIT

PSQL="/usr/bin/psql -h /tmp -p 54333 -d postgres -v ON_ERROR_STOP=1"
$PSQL -f tests/postgres/supabase_stubs.sql > /dev/null
$PSQL -f supabase/migrations/20261029000100_near_marketplace.sql > /dev/null
$PSQL -f supabase/migrations/20261029000200_near_wallet_auth.sql > /dev/null
$PSQL -f supabase/migrations/20261029000300_settlement_reconcile.sql > /dev/null
$PSQL -f supabase/migrations/20261103000100_harden_settlement_reconcile.sql > /dev/null
$PSQL -f supabase/migrations/20261105000100_catalog_chain_indexing.sql > /dev/null
$PSQL -f supabase/migrations/20261106000100_withdrawal_reconciliation.sql > /dev/null
$PSQL -f tests/postgres/catalog_chain_indexing.test.sql > /dev/null
$PSQL -f tests/postgres/withdrawal_reconciliation.test.sql > /dev/null
echo "[+] All 6 SQL migrations and PostgreSQL security tests passed cleanly."
'

echo ""
echo "========================================================================"
echo " ALL TEST SUITES PASSED CLEANLY (100% SUCCESS)"
echo "========================================================================"

