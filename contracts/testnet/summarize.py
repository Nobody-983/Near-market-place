#!/usr/bin/env python3
"""Derive public funding calculations from saved RPC and offline VM evidence."""
import base64
import json
from pathlib import Path
from flow import operation, verify_records

PUBLIC = Path(__file__).resolve().parent / "public"


def summarize(snapshot, measurement, vm_input):
    manifest = vm_input["manifest"]
    flow = manifest["flow"]
    config = snapshot["protocol_config"]["runtime_config"]
    costs = config["transaction_costs"]
    actions = costs["action_creation_config"]
    byte_cost = int(config["storage_amount_per_byte"])
    # Protocol 88 reserves receipt gas at at least min_gas_purchase_price.
    # Using only the (lower) block header gas price underestimates prefunding.
    price = max(int(snapshot["final_header"]["gas_price"]), int(config["min_gas_purchase_price"]))

    def fee(value, send):
        return int(value[send]) + int(value["execution"])

    stages = {}
    for stage, role, method in (
        ("nft-deploy", "nft", "new_testnet"),
        ("marketplace-deploy", "marketplace", "bootstrap_with_admission_owner"),
        ("fund", "marketplace", "fund_testnet_storage"),
        ("mint", "nft", "nft_mint"),
        ("approve", "nft", "nft_approve"),
        ("create", "marketplace", "create_nft_admission"),
        ("dispatch", "marketplace", "dispatch_nft_admission"),
    ):
        deploy = stage.endswith("-deploy")
        send = "send_sir" if deploy else "send_not_sir"
        row = measurement[role][method]
        top_fees = fee(costs["action_receipt_creation_config"], send)
        top_fees += fee(actions["function_call_cost"], send)
        top_fees += (len(method.encode()) + row["input_bytes"]) * fee(actions["function_call_cost_per_byte"], send)
        deploy_fees = 0
        if deploy:
            deploy_fees = fee(actions["deploy_contract_cost"], send)
            deploy_fees += snapshot["artifacts"][role]["bytes"] * fee(actions["deploy_contract_cost_per_byte"], send)
        attached = int(flow["transaction_gas"][stage])
        stages[stage] = {"function_call_prepaid_gas": str(attached),
            "top_level_receipt_and_call_fees_gas": str(top_fees),
            "deploy_action_send_and_execution_gas": str(deploy_fees),
            "prefunding_at_observed_purchase_price_yocto": str((attached + top_fees + deploy_fees) * price),
            "tool_ceiling_300_tgas_prefunding_yocto": str((300 * 10**12 + top_fees + deploy_fees) * price)}

    funding = {}
    for role, method in (("nft", "new_testnet"), ("marketplace", "bootstrap_with_admission_owner")):
        account = manifest["accounts"][role]
        balance = int(snapshot["accounts"][account]["view_account"]["amount"])
        usage = measurement[role][method]["storage_usage_bytes"]
        required = usage * byte_cost + int(stages[role + "-deploy"]["prefunding_at_observed_purchase_price_yocto"])
        funding[role] = {"account": account, "balance_yocto": str(balance),
            "code_bytes": snapshot["artifacts"][role]["bytes"],
            "code_stake_yocto": str(snapshot["artifacts"][role]["bytes"] * byte_cost),
            "account_code_initialized_storage_bytes": usage,
            "initialized_storage_stake_yocto": str(usage * byte_cost),
            "storage_plus_candidate_gas_prefunding_yocto": str(required),
            "remaining_headroom_yocto": str(balance - required),
            "sufficient_at_snapshot": balance >= required}
    account = manifest["accounts"]["seller"]
    balance = int(snapshot["accounts"][account]["view_account"]["amount"])
    stake = snapshot["accounts"][account]["view_account"]["storage_usage"] * byte_cost
    deposits = sum(int(flow[k]) for k in ("funding_yocto", "mint_deposit_yocto", "approval_deposit_yocto"))
    gas = sum(int(stages[k]["prefunding_at_observed_purchase_price_yocto"]) for k in ("fund", "mint", "approve", "create", "dispatch"))
    funding["operator"] = {"account": account, "balance_yocto": str(balance), "existing_storage_stake_yocto": str(stake),
        "all_candidate_deposits_yocto": str(deposits), "all_candidate_gas_prefunding_yocto": str(gas),
        "storage_plus_candidate_gas_and_deposits_yocto": str(stake + deposits + gas),
        "remaining_headroom_yocto": str(balance - stake - deposits - gas), "sufficient_at_snapshot": balance >= stake + deposits + gas}

    class OfflineRecords:
        def checked(self, method, params):
            assert method == "query" and params["request_type"] == "view_state"
            prefix = base64.b64decode(params["prefix_base64"])
            return {"values": [r for r in measurement["marketplace_records"] if base64.b64decode(r["key"]).startswith(prefix)]}

    op, terms = operation(manifest, 1)
    final = verify_records(manifest, OfflineRecords(), "offline-VM-only", op, terms)
    return {"status": "preparation_only_not_authorization", "snapshot_height": snapshot["final_header"]["height"],
        "gas_price_burn_yocto_per_gas": snapshot["final_header"]["gas_price"],
        "min_gas_purchase_price_yocto_per_gas": config["min_gas_purchase_price"],
        "calculation_price_yocto_per_gas": str(price), "storage_byte_cost_yocto": str(byte_cost),
        "limitations": ["Based on snapshot protocol and gas prices; recheck immediately before any later transaction",
            "Local VM gas and trie accounting exclude real node execution/receipt behavior",
            "Attached deployment gas budgets apply to initialization; deploy action fees are additional",
            "Candidate gas margin is 2x measured burn rounded up to Tgas; dispatch also covers child gas, local guard and host action fees",
            "Expiry is still unset; final metadata/terms/manifest bytes require remeasurement before approval",
            "No claim of node rollback, cross-shard delivery, OOG safety or key custody"],
        "stages": stages, "funding": funding, "offline_final_record_verification": final}


if __name__ == "__main__":
    def read(name):
        return json.loads((PUBLIC / name).read_text())
    result = summarize(read("rpc-snapshot.json"), read("measurements.json"), read("measurement-input.json"))
    (PUBLIC / "funding-calculations.json").write_text(json.dumps(result, indent=2) + "\n")
