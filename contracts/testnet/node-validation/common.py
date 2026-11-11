"""Public inputs and pure codecs. This module never opens a network socket."""
import base64
import hashlib
import json
import os
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PARENT = HERE.parent
sys.path.insert(0, str(PARENT))
from flow import b64, compact, deployment, domain_hash, envelope, integer, operation, sha, string, unbase58

VERSION = "2.14.0-rc.5"
COMMIT = "b7b4737cab46f17a6daa2ed694d7aebbd204363a"
PROTOCOL = 88
CAP = 300_000_000_000_000
BYTE_COST = 10**19
NFT = "local-nft-contract.testnet"
MARKET = "local-market-contract.testnet"
OWNER = "local-market-operator.testnet"
ACCOUNTS = (NFT, MARKET, OWNER)
HASHES = {"nft": "b8a500297d6e93bc79c3f06834d09f31bf384816301d179d1aabb425a6a4a84d",
          "marketplace": "5f0b522306a87bc01beebe4c7575b8ae705785d38f003feae18eebfe2835708a"}
CASES = [f"S{i:02d}" for i in range(1, 18)] + [f"L{i:02d}" for i in range(1, 4)]

class CheckError(ValueError):
    pass

class Inconclusive(CheckError):
    pass

def require(condition, message):
    if not condition:
        raise CheckError(message)

def load(path):
    return json.loads(Path(path).read_text())

def save(path, data):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    with tmp.open('w') as f:
        f.write(json.dumps(data, indent=2, sort_keys=True) + "\n")
        f.flush()
        os.fsync(f.fileno())
    tmp.replace(path)
    directory=os.open(path.parent,os.O_RDONLY|os.O_DIRECTORY)
    try: os.fsync(directory)
    finally: os.close(directory)

def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def base58(data):
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    n, result = int.from_bytes(data, "big"), ""
    while n:
        n, r = divmod(n, 58)
        result = alphabet[r] + result
    return "1" * (len(data) - len(data.lstrip(b"\0"))) + result

def call(method, args, gas, deposit=0):
    require(0 < int(gas) <= CAP, "gas outside tool cap")
    return {"kind": "call", "method": method, "args": args, "gas": str(gas), "deposit": str(deposit)}

def state_map(snapshot, account):
    rows = snapshot["accounts"][account]["state"]["values"]
    decoded = [(base64.b64decode(r["key"], validate=True), base64.b64decode(r["value"], validate=True)) for r in rows]
    require(len(dict(decoded)) == len(decoded), "duplicate state keys")
    return dict(decoded)

def locator(manifest, approval):
    f = manifest["flow"]
    return sha(b"nmp:nft-admission:v1\0" + domain_hash(deployment(manifest)) + string(f["token_id"]) + string(OWNER) + integer(approval, 8))

def protocol_check(actual, reference):
    require(actual["protocol_version"] == PROTOCOL, "protocol mismatch")
    # Compare all runtime parameters, not only the storage scalar. Topology and
    # local chain identity intentionally differ; no runtime-cost exceptions.
    require(actual["runtime_config"] == reference["runtime_config"], "runtime configuration differs from pinned public snapshot")
    require(actual["gas_limit"] == reference["gas_limit"], "chunk gas limit mismatch")

def manifest(template, identity, keys, window_ns, changes=None):
    require(0 < window_ns < 2**63, "finite positive expiry window required")
    require(identity["chain_id"].startswith("nft-validation-local-"), "local chain identity required")
    f = json.loads(json.dumps(template["flow"]))
    f["fee_account"] = OWNER
    f["expires_at_ns"] = str(int(identity["timestamp_nanosec"]) + window_ns)
    require(int(f["expires_at_ns"]) < 2**64 - 1, "expiry overflow/sentinel")
    policy = {"purpose": "isolated noncommercial validation", "fee_account": OWNER,
              **{k: f[k] for k in ("price_yocto", "fee_bps", "fee_cap_bps", "policy_version")}}
    f["policy_hash"] = sha(compact(policy).encode()).hex()
    f["correlation"] = sha(compact({"chain": identity, "token": f["token_id"], "policy": policy}).encode()).hex()
    for k, v in (changes or {}).items():
        if k == "transaction_gas": f[k].update(v)
        else: f[k] = v
    return {"purpose": "isolated-node-validation-only", "network": identity["chain_id"],
            "genesis_height": identity["genesis_height"], "genesis_block_hash": identity["genesis_hash"],
            "accounts": {"nft": NFT, "marketplace": MARKET, "seller": OWNER, "admission_owner": OWNER},
            "artifact_sha256": HASHES,
            "expected_access_keys": {role: [{"public_key": keys[acct], "permission": "FullAccess"}]
                for role, acct in {"nft": NFT, "marketplace": MARKET, "seller": OWNER, "admission_owner": OWNER}.items()},
            "fixture_policy": policy, "flow": f}
