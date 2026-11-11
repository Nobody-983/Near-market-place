#!/usr/bin/env python3
"""Read-only artifact/account preflight and blocked integration plan. Never signs.

No credential access, account creation, broadcast, deployment or activation code.
The RPC checks are operational checks against an explicitly trusted endpoint,
not cryptographic receipt proofs. Missing policy/runtime capabilities stay blocked.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
NFT = ROOT / "nft/target/testnet/wasm32-unknown-unknown/release/near_nft_contract.wasm"
MARKET = ROOT / "marketplace/runtime/target/testnet/wasm32-unknown-unknown/release/near_marketplace_runtime_prototype.wasm"
NFT_EXPORTS = {
    "new_testnet", "testnet_mint_authority",
    "new", "new_default_meta", "new_with_marketplace", "marketplace_id",
    "nft_mint", "contract_owner_id", "token_id_status", "nft_transfer",
    "nft_transfer_call", "nft_token", "nft_resolve_transfer",
    "nft_resolve_transfer_versioned", "marketplace_transfer_approved",
    "nft_approve", "nft_revoke", "nft_revoke_all", "nft_is_approved",
    "nft_total_supply", "nft_tokens", "nft_supply_for_owner", "nft_tokens_for_owner",
    "nft_metadata", "contract_source_metadata",
}
MARKET_EXPORTS = {
    "fund_testnet_storage",
    "bootstrap", "bootstrap_with_admission_owner", "storage_status",
    "create_nft_admission", "dispatch_nft_admission", "release_nft_admission",
    "nft_on_approve", "on_nft_verified", "contract_source_metadata",
}
BLOCKERS = [
    "Dedicated testnet accounts, local CLI/key custody and operator-approved test inputs are required.",
    "Real finalized mint/approval/admission/verification flow has not run.",
    "Node receipt rollback, resolver ordering and OOG regressions remain unverified.",
]
PREPARATION_REVIEWS = (
    "independent_genesis_review", "key_inventory_and_local_custody_review",
    "fixture_approved", "budgets_and_retained_funding_approved",
    "independent_node_measurement", "cli_version_and_network_profile_verified",
)


def uleb(data, offset):
    value = 0
    for shift in range(0, 35, 7):
        if offset >= len(data):
            raise ValueError("truncated Wasm integer")
        byte = data[offset]
        offset += 1
        value |= (byte & 127) << shift
        if byte < 128:
            return value, offset
    raise ValueError("oversized Wasm integer")


def exports(data):
    if data[:8] != b"\0asm\x01\0\0\0":
        raise ValueError("invalid Wasm header")
    offset, result = 8, set()
    while offset < len(data):
        kind = data[offset]
        length, offset = uleb(data, offset + 1)
        end = offset + length
        if end > len(data):
            raise ValueError("truncated Wasm section")
        if kind == 7:
            count, cursor = uleb(data, offset)
            for _ in range(count):
                n, cursor = uleb(data, cursor)
                if cursor + n >= end:
                    raise ValueError("truncated Wasm export")
                name = data[cursor:cursor + n].decode("utf-8")
                cursor += n
                entry_kind = data[cursor]
                _, cursor = uleb(data, cursor + 1)
                if entry_kind == 0:
                    if name in result:
                        raise ValueError("duplicate export")
                    result.add(name)
            if cursor != end:
                raise ValueError("invalid export section")
        offset = end
    return result


def artifacts():
    result = {}
    production_nft = NFT_EXPORTS - {"new_testnet", "testnet_mint_authority"}
    production_market = MARKET_EXPORTS - {"fund_testnet_storage"}
    checks = [("nft", NFT, NFT_EXPORTS), ("marketplace", MARKET, MARKET_EXPORTS),
              ("nft_default", ROOT / "nft/target/wasm32-unknown-unknown/release/near_nft_contract.wasm", production_nft),
              ("nft_production", ROOT / "nft/target/production/wasm32-unknown-unknown/release/near_nft_contract.wasm", production_nft),
              ("marketplace_production", ROOT / "marketplace/runtime/target/production/wasm32-unknown-unknown/release/near_marketplace_runtime_prototype.wasm", production_market)]
    for name, path, expected in checks:
        data = path.read_bytes()
        actual = exports(data)
        if actual != expected:
            raise ValueError(f"{name} ABI mismatch: missing={expected - actual}, extra={actual - expected}")
        result[name] = {"sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data), "exports": sorted(actual)}
    return result


def validate_manifest(manifest, built):
    if "preparation" in manifest:
        review = manifest["preparation"]
        if (not isinstance(review, dict) or review.get("status") != "approved"
                or any(review.get(key) is not True for key in PREPARATION_REVIEWS)
                or manifest.get("flow", {}).get("expires_at_ns") is None):
            raise ValueError("Preparation template is not approved; resolve its listed decisions first")
    if manifest.get("network") != "testnet":
        raise ValueError("only testnet is permitted")
    # A pinned endpoint keeps this tool from following user-selected mainnet or
    # local endpoints. Verify the response identity again for EVERY operation.
    if manifest.get("rpc_url") != "https://rpc.testnet.near.org":
        raise ValueError("use the pinned testnet RPC URL")
    accounts = manifest["accounts"]
    for role in ["nft", "marketplace", "admission_owner", "seller"]:
        value = accounts[role]
        if not isinstance(value, str) or not 2 <= len(value) <= 64 or not re.fullmatch(r"[a-z0-9]+(?:[._-][a-z0-9]+)*\.testnet", value):
            raise ValueError(f"invalid dedicated testnet account: {role}")
    if accounts["nft"] == accounts["marketplace"] or accounts["admission_owner"] in [accounts["nft"], accounts["marketplace"]]:
        raise ValueError("contract accounts and admission owner must be separate")
    if type(manifest.get("genesis_height")) is not int or manifest["genesis_height"] < 0:
        raise ValueError("reviewed genesis height required")
    if not re.fullmatch(r"[1-9A-HJ-NP-Za-km-z]{32,44}", manifest.get("genesis_block_hash", "")):
        raise ValueError("reviewed genesis block hash required")
    for role in ["nft", "marketplace"]:
        if manifest["artifact_sha256"][role] != built[role]["sha256"]:
            raise ValueError(f"unapproved artifact hash: {role}")
    for role in accounts:
        if not manifest["expected_access_keys"].get(role):
            raise ValueError(f"explicit public access-key inventory required: {role}")


class RPC:
    def __init__(self, manifest):
        self.manifest = manifest

    def raw(self, method, params):
        payload = json.dumps({"jsonrpc": "2.0", "id": "readiness", "method": method, "params": params}).encode()
        request = urllib.request.Request(self.manifest["rpc_url"], data=payload, headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(request, timeout=20) as response:
            body = response.read(2 * 1024 * 1024 + 1)
        if len(body) > 2 * 1024 * 1024:
            raise ValueError("RPC response exceeds bound")
        value = json.loads(body)
        if "error" in value or "result" not in value:
            # Do not guess account nonexistence from a transport/RPC error.
            raise ValueError(f"RPC {method} failed; account/identity remains unverified")
        return value["result"]

    def checked(self, method, params):
        status = self.raw("status", [])
        if status.get("chain_id") != "testnet" or status.get("sync_info", {}).get("syncing") is not False:
            raise ValueError("RPC is not synchronized testnet")
        genesis = self.raw("block", {"block_id": self.manifest["genesis_height"]})
        if genesis["header"]["hash"] != self.manifest["genesis_block_hash"]:
            raise ValueError("genesis identity mismatch")
        return self.raw(method, params)


def accounts_preflight(manifest, rpc):
    final = rpc.checked("block", {"finality": "final"})["header"]["hash"]
    result = {}
    for role, account in manifest["accounts"].items():
        base = {"account_id": account, "block_id": final}
        state = rpc.checked("query", {**base, "request_type": "view_account"})
        keys = rpc.checked("query", {**base, "request_type": "view_access_key_list"})["keys"]
        actual = sorted((k["public_key"], json.dumps(k["access_key"]["permission"], sort_keys=True)) for k in keys)
        expected = sorted((k["public_key"], json.dumps(k["permission"], sort_keys=True)) for k in manifest["expected_access_keys"][role])
        if actual != expected:
            raise ValueError(f"access-key inventory mismatch: {role}")
        if role in ("nft", "marketplace"):
            if state["code_hash"] != "11111111111111111111111111111111":
                raise ValueError(f"refusing existing contract: {role}")
            values = rpc.checked("query", {**base, "request_type": "view_state", "prefix_base64": ""})["values"]
            if values:
                raise ValueError(f"refusing existing storage: {role}")
        result[role] = {"account": account, "balance_yocto": state["amount"], "storage_usage": state["storage_usage"], "keys_match": True}
    return {"final_block": final, "accounts": result, "custody_proven": False}


def flow_plan():
    return [
        {"step": "mint", "method": "nft_mint", "status": "testnet artifact + new_testnet authority; recipient must equal authority"},
        {"step": "approve", "method": "nft_approve", "arguments": "token_id, account_id=pinned marketplace, msg=null", "status": "requires funded token; null msg avoids disabled public hook"},
        {"step": "fund admission", "method": "fund_testnet_storage", "arguments": "funding_nonce: decimal U64 string; positive attached deposit", "status": "testnet only; immutable owner funds self; gross deposit less measured metadata = usable credit"},
        {"step": "create admission", "method": "create_nft_admission", "arguments": "canonical operation, storage_owner_id=immutable owner, explicit reserve_amount/creation_bytes/local_gas", "status": "requires approved terms and owner credit; zero deposit"},
        {"step": "dispatch", "method": "dispatch_nft_admission", "arguments": "32-byte admission_id locator only", "status": "immutable owner, zero deposit"},
        {"step": "verify", "method": "on_nft_verified", "status": "NEAR dependencies only; never call directly or inject results; retain full finalized receipt graph"},
    ]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, help="reviewed PUBLIC configuration only; not a credential file")
    parser.add_argument("--check-accounts", action="store_true", help="read-only finalized testnet queries")
    args = parser.parse_args()
    built = artifacts()
    report = {"artifacts": built, "flow": flow_plan(), "deployment_enabled": False, "blockers": BLOCKERS}
    if args.manifest:
        manifest = json.loads(args.manifest.read_text())
        validate_manifest(manifest, built)
        if args.check_accounts:
            report["preflight"] = accounts_preflight(manifest, RPC(manifest))
    elif args.check_accounts:
        parser.error("--check-accounts requires a reviewed --manifest")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
