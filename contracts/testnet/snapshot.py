#!/usr/bin/env python3
"""Collect PUBLIC preparation evidence only; never a deployment authorization.

Only the fixed accounts/endpoint and explicitly listed read-only RPC operations
are used. Genesis discovery is cross-checked before checked queries begin.
"""
import argparse
import datetime
import json
from pathlib import Path
from readiness import RPC, artifacts

ACCOUNTS = {
    "nft": "ahmed-nft-contract.testnet",
    "marketplace": "ahmed-market-contract.testnet",
    "seller": "ahmed-market-operator.testnet",
    "admission_owner": "ahmed-market-operator.testnet",
}


def collect():
    manifest = {"network": "testnet", "rpc_url": "https://rpc.testnet.near.org"}
    rpc = RPC(manifest)
    status = rpc.raw("status", [])
    genesis_config = rpc.raw("EXPERIMENTAL_genesis_config", [])
    if (status.get("chain_id") != "testnet"
            or status.get("sync_info", {}).get("syncing") is not False
            or genesis_config.get("chain_id") != "testnet"):
        raise ValueError("Not synchronized testnet")
    height = genesis_config["genesis_height"]
    genesis = rpc.raw("block", {"block_id": height})["header"]
    if (genesis["height"] != height or genesis["hash"] != status["genesis_hash"]
            or genesis["prev_hash"] != "1" * 32):
        raise ValueError("Genesis discovery disagrees with status/configuration")
    manifest.update(genesis_height=height, genesis_block_hash=genesis["hash"])
    block = rpc.checked("block", {"finality": "final"})["header"]
    protocol = rpc.checked("EXPERIMENTAL_protocol_config", {"block_id": block["hash"]})
    if protocol["chain_id"] != "testnet" or protocol["genesis_height"] != height:
        raise ValueError("Protocol identity mismatch")
    accounts = {}
    for account in dict.fromkeys(ACCOUNTS.values()):
        base = {"account_id": account, "block_id": block["hash"]}
        observations = {}
        for request in ("view_account", "view_access_key_list", "view_state"):
            params = {**base, "request_type": request}
            if request == "view_state":
                params["prefix_base64"] = ""
            result = rpc.checked("query", params)
            if result["block_hash"] != block["hash"]:
                raise ValueError("Query did not use pinned finalized block")
            observations[request] = result
        accounts[account] = observations
    return {
        "observed_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "purpose": "Public observations, not proof of custody or human approval",
        "network": manifest, "status": status, "genesis_header": genesis,
        "genesis_config_identity": {k: genesis_config[k] for k in (
            "chain_id", "genesis_height", "genesis_time", "protocol_version")},
        "final_header": block, "protocol_config": protocol,
        "roles": ACCOUNTS, "accounts": accounts, "artifacts": artifacts(),
        "independent_genesis_review": False, "custody_proven": False,
        "deployment_enabled": False,
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True,
                        help="new public evidence JSON; existing files are never overwritten")
    args = parser.parse_args()
    evidence = collect()
    with args.output.open("x") as output:
        json.dump(evidence, output, indent=2)
        output.write("\n")
