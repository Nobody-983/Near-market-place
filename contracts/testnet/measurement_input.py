#!/usr/bin/env python3
"""Build offline VM inputs and an explicitly blocked public manifest candidate.

No RPC or transaction commands. Probe ceilings and expiry sentinels are NEVER
deployment defaults. Candidate budgets are derived from saved VM observations.
"""
import argparse
import copy
import json
from pathlib import Path
from flow import b64, compact, deployment, domain_hash, integer, operation, sha, string

PUBLIC = Path(__file__).resolve().parent / "public"
TGAS = 10**12
TOOL_GAS_LIMIT = 300 * TGAS


def candidate(snapshot):
    policy = json.loads((PUBLIC / "test-policy-candidate.json").read_text())
    m = {**snapshot["network"], "accounts": snapshot["roles"],
         "artifact_sha256": {r: snapshot["artifacts"][r]["sha256"] for r in ("nft", "marketplace")},
         "expected_access_keys": {r: [{"public_key": k["public_key"], "permission": k["access_key"]["permission"]}
             for k in snapshot["accounts"][account]["view_access_key_list"]["keys"]]
             for r, account in snapshot["roles"].items()},
         "preparation": {"status": "blocked", "independent_genesis_review": False,
             "key_inventory_and_local_custody_review": False, "fixture_approved": False,
             "budgets_and_retained_funding_approved": False, "independent_node_measurement": False,
             "cli_version_and_network_profile_verified": False,
             "note": "Every proposed flow value requires review; expires_at_ns must be supplied. Do not initialize from this template."}}
    f = {k: policy[k] for k in ("price_yocto", "fee_bps", "fee_cap_bps", "fee_account", "policy_version", "generation", "funding_nonce")}
    f.update(nft_metadata={"spec": "nft-1.0.0", "name": "Ahmed testnet preparation", "symbol": "ATEST"},
             token_id="prep-ahmed-20261102-01", token_metadata={"title": "Preparation fixture", "description": "Test-only ownership and approval verification; no sale.", "copies": 1},
             expires_at_ns=None, policy_hash=sha(compact(policy).encode()).hex())
    correlation = {"purpose": "testnet preparation correlation v1", "accounts": m["accounts"],
                   "token_id": f["token_id"], "policy_hash": f["policy_hash"]}
    f["correlation"] = sha(compact(correlation).encode()).hex()
    m["flow"] = f
    return m


def fill_probe(m, byte_cost):
    f = m["flow"]
    # 16,384 is the runtime record/result bound. Eight records provide a probe
    # ceiling only; final values use actual measured growth, never this ceiling.
    probe_bytes = 16384 * 8
    f.update(creation_bytes=str(probe_bytes), reserve_yocto=str(probe_bytes * byte_cost),
             funding_yocto=str(2 * probe_bytes * byte_cost),
             mint_deposit_yocto=str(16384 * byte_cost), approval_deposit_yocto=str(16384 * byte_cost),
             max_result_bytes="16384")
    for key in ("first_call_gas", "second_call_gas", "callback_gas", "local_gas"):
        f[key] = str(TOOL_GAS_LIMIT // 8)
    f["transaction_gas"] = {s: str(TOOL_GAS_LIMIT) for s in ("nft-deploy", "marketplace-deploy", "fund", "mint", "approve", "create", "dispatch")}


def fill_measured(m, report, byte_cost):
    f = m["flow"]
    nft, market = report["nft"], report["marketplace"]
    def gas(row):
        # Explicit proposed margin: double measured gas, round up to whole Tgas.
        return str(((2 * int(row["burnt_gas"]) + TGAS - 1) // TGAS) * TGAS)
    f["mint_deposit_yocto"] = str(nft["nft_mint"]["storage_delta_bytes"] * byte_cost)
    f["approval_deposit_yocto"] = str(max(1, nft["nft_approve"]["storage_delta_bytes"] * byte_cost))
    # Reserve twice measured creation growth; no hidden allowance or claim that
    # this margin has been human approved. Funding overhead is separately added.
    f["creation_bytes"] = str(2 * market["create_nft_admission"]["storage_delta_bytes"])
    f["reserve_yocto"] = str(int(f["creation_bytes"]) * byte_cost)
    f["funding_yocto"] = str(int(f["reserve_yocto"]) + market["fund_testnet_storage"]["storage_delta_bytes"] * byte_cost)
    f["first_call_gas"] = gas(nft["nft_token"])
    f["second_call_gas"] = gas(nft["nft_is_approved"])
    f["callback_gas"] = gas(market["on_nft_verified"])
    f["local_gas"] = gas(market["dispatch_nft_admission"])
    f["max_result_bytes"] = str(2 * max(nft["nft_token"]["result_bytes"], 5))
    f["transaction_gas"] = {stage: gas(row) for stage, row in (
        ("nft-deploy", nft["new_testnet"]), ("marketplace-deploy", market["bootstrap_with_admission_owner"]),
        ("fund", market["fund_testnet_storage"]), ("mint", nft["nft_mint"]),
        ("approve", nft["nft_approve"]), ("create", market["create_nft_admission"]))}
    f["transaction_gas"]["dispatch"] = str(sum(int(f[k]) for k in ("first_call_gas", "second_call_gas", "callback_gas")) + 2 * int(f["local_gas"]))


def vm_input(m, snapshot):
    m = copy.deepcopy(m)
    # Fixed-width offline sentinel only. The deployable manifest keeps null.
    m["flow"]["expires_at_ns"] = str(2**64 - 1)
    a, f = m["accounts"], m["flow"]
    op, _ = operation(m, 1)
    domain = deployment(m)
    locator = sha(b"nmp:nft-admission:v1\0" + domain_hash(domain) + string(f["token_id"]) + string(a["seller"]) + integer(1, 8))
    gas = {method: int(f["transaction_gas"][stage]) for method, stage in (
        ("new_testnet", "nft-deploy"), ("bootstrap_with_admission_owner", "marketplace-deploy"),
        ("nft_mint", "mint"), ("nft_approve", "approve"), ("fund_testnet_storage", "fund"),
        ("create_nft_admission", "create"), ("dispatch_nft_admission", "dispatch"))}
    gas.update(nft_token=int(f["first_call_gas"]), nft_is_approved=int(f["second_call_gas"]),
               on_nft_verified=int(f["callback_gas"]), storage_status=TOOL_GAS_LIMIT)
    return {"warning": "Offline measurement ONLY; expiry is a sentinel, all policy/budgets unapproved", "manifest": m,
            "domain": b64(domain), "operation": b64(op), "admission_id": b64(locator), "gas_per_method": gas,
            "probe_gas": TOOL_GAS_LIMIT, "block_height": snapshot["final_header"]["height"],
            "block_timestamp": snapshot["final_header"]["timestamp_nanosec"]}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--measured", type=Path, help="derive candidates from a saved probe report")
    args = p.parse_args()
    snapshot = json.loads((PUBLIC / "rpc-snapshot.json").read_text())
    m = candidate(snapshot)
    byte_cost = int(snapshot["protocol_config"]["runtime_config"]["storage_amount_per_byte"])
    if args.measured:
        fill_measured(m, json.loads(args.measured.read_text()), byte_cost)
        (PUBLIC / "manifest.template.json").write_text(json.dumps(m, indent=2) + "\n")
        name = "measurement-input.json"
    else:
        fill_probe(m, byte_cost)
        name = "probe-input.json"
    (PUBLIC / name).write_text(json.dumps(vm_input(m, snapshot), indent=2) + "\n")


if __name__ == "__main__":
    main()
