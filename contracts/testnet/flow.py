#!/usr/bin/env python3
"""Read-only testnet preflight, canonical payloads and exact CLI commands.

Never signs, broadcasts or accesses credentials. Run each stage immediately before
its manually reviewed CLI command. Uses a trusted RPC, not cryptographic proofs.
"""
import argparse
import base64
import hashlib
import json
import re
import shlex
from pathlib import Path
from readiness import RPC, NFT, MARKET, artifacts, validate_manifest

STAGES = ("nft-deploy", "marketplace-deploy", "fund", "mint", "approve", "inspect", "create", "dispatch", "verify")
VERIFY_ABI = b"near-nft-5.30:verify:v1:nft_token(token_id:string)->Token|null;nft_is_approved(token_id:string,approved_account_id:AccountId,approval_id:u64-number)->bool;ordered-join"
CALLBACK_ABI = b"nmp:on_nft_verified:v1(generation:U64-string,attempt_nonce:U64-string)->active|invalid|ignored;results=2"
OP_TAG = b"near-marketplace:operation:v1\0"


def sha(b): return hashlib.sha256(b).digest()
def b64(b): return base64.b64encode(b).decode()
def integer(n, size): return int(n).to_bytes(size, "little")
def string(s):
    b = s.encode()
    return integer(len(b), 4) + b
def compact(value): return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def unbase58(s):
    alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    n = 0
    for c in s: n = n * 58 + alphabet.index(c)
    b = b"\0" * (len(s) - len(s.lstrip("1"))) + n.to_bytes((n.bit_length()+7)//8, "big")
    if len(b) != 32: raise ValueError("Expected 32-byte base58 hash")
    return b


def deployment(manifest):
    a = manifest["accounts"]
    return (b"\0" + unbase58(manifest["genesis_block_hash"]) + sha(compact(manifest).encode())
            + bytes.fromhex(manifest["artifact_sha256"]["marketplace"])
            + bytes.fromhex(manifest["artifact_sha256"]["nft"])
            + string(a["marketplace"]) + string(a["nft"]))


def domain_hash(domain): return sha(b"near-marketplace:deployment:v1\0" + domain)


def number(settings, key, bits=64, minimum=1):
    raw = settings[key]
    if isinstance(raw, bool) or not re.fullmatch(r"0|[1-9][0-9]*", str(raw)):
        raise ValueError(f"{key}: canonical nonnegative integer required")
    n = int(raw)
    if not minimum <= n < 2**bits: raise ValueError(f"{key}: out of range")
    return n


def operation(manifest, approval):
    a, f = manifest["accounts"], manifest["flow"]
    token = f["token_id"]
    if not token.strip() or len(token.encode()) > 1024: raise ValueError("Invalid token ID")
    if number(f, "generation") != 1: raise ValueError("This runbook supports the first fresh generation only")
    rate, cap = number(f, "fee_bps", 16, 0), number(f, "fee_cap_bps", 16, 0)
    if not rate <= cap < 10000: raise ValueError("Invalid test-only fee terms")
    if not re.fullmatch(r"[a-z0-9]+(?:[._-][a-z0-9]+)*\.testnet", f["fee_account"]):
        raise ValueError("Test-only fee account must be a testnet identity")
    if f["fee_account"] in (a["nft"], a["marketplace"]): raise ValueError("Invalid fee account")
    terms = (string(a["nft"]) + string(token) + string(a["seller"]) + integer(approval, 8)
             + integer(number(f, "price_yocto", 128), 16) + integer(number(f, "expires_at_ns"), 8)
             + integer(rate, 2) + integer(cap, 2) + string(f["fee_account"])
             + integer(number(f, "policy_version"), 8))
    for key in ("policy_hash", "correlation"):
        b = bytes.fromhex(f[key])
        if len(b) != 32 or b == bytes(32): raise ValueError(f"Invalid {key}")
        terms += b
    budgets = b"".join(integer(number(f, k), 8) for k in ("first_call_gas", "second_call_gas", "callback_gas"))
    limit = number(f, "max_result_bytes", 32)
    if not 5 <= limit <= 16384: raise ValueError("Result bound outside runtime limits")
    budgets += integer(limit, 4) + sha(VERIFY_ABI) + sha(CALLBACK_ABI)
    return OP_TAG + integer(1, 2) + deployment(manifest) + b"\0" + integer(1, 8) + terms + integer(1, 8) + budgets, terms


def query(rpc, account, block, **params):
    return rpc.checked("query", {"account_id": account, "block_id": block, **params})


def view(rpc, account, block, method, args):
    result = query(rpc, account, block, request_type="call_function", method_name=method, args_base64=b64(compact(args).encode()))
    if "error" in result or "result" not in result: raise ValueError(f"View failed: {method}")
    return json.loads(bytes(result["result"]))


def record(rpc, account, block, key, missing=False):
    values = query(rpc, account, block, request_type="view_state", prefix_base64=b64(key))["values"]
    exact = [base64.b64decode(v["value"], validate=True) for v in values if base64.b64decode(v["key"], validate=True) == key]
    if not exact and missing: return None
    if len(exact) != 1 or len(exact[0]) > 16384: raise ValueError("Missing/oversized persisted record")
    return exact[0]


def check_accounts(manifest, rpc, block, stage):
    a = manifest["accounts"]
    if a["seller"] in (a["nft"], a["marketplace"]): raise ValueError("Mint authority must be separate")
    for role, account in a.items():
        state = query(rpc, account, block, request_type="view_account")
        keys = query(rpc, account, block, request_type="view_access_key_list")["keys"]
        actual = sorted((k["public_key"], compact(k["access_key"]["permission"])) for k in keys)
        expected = sorted((k["public_key"], compact(k["permission"])) for k in manifest["expected_access_keys"][role])
        if actual != expected: raise ValueError(f"Key inventory changed: {role}")
        if role not in ("nft", "marketplace"): continue
        empty = stage == "nft-deploy" or (stage == "marketplace-deploy" and role == "marketplace")
        if empty:
            if state["code_hash"] != "1" * 32: raise ValueError(f"Refusing to overwrite {role}")
            if query(rpc, account, block, request_type="view_state", prefix_base64="")["values"]:
                raise ValueError(f"Refusing existing {role} storage")
        elif unbase58(state["code_hash"]).hex() != manifest["artifact_sha256"][role]:
            raise ValueError(f"Deployed code mismatch: {role}")
    if stage != "nft-deploy":
        for method, expected in [("marketplace_id", a["marketplace"]), ("contract_owner_id", a["seller"]), ("testnet_mint_authority", a["seller"])]:
            if view(rpc, a["nft"], block, method, {}) != expected: raise ValueError(f"NFT binding mismatch: {method}")
    if stage not in ("nft-deploy", "marketplace-deploy"):
        domain = deployment(manifest)
        if record(rpc, a["marketplace"], block, b"nmp:runtime:bootstrap:v1") != b"NMPBOOT1" + domain:
            raise ValueError("Bootstrap domain mismatch; manifest must remain unchanged")
        owner = b"NMPAOWN1" + domain_hash(domain) + string(a["admission_owner"])
        if record(rpc, a["marketplace"], block, b"nmp:runtime:admission-owner:v1") != owner + sha(owner):
            raise ValueError("Immutable admission owner mismatch")
        status = view(rpc, a["marketplace"], block, "storage_status", {})
        if status["trading_enabled"] or status["deployment_commitment"] != b64(domain_hash(domain)):
            raise ValueError("Unsafe runtime status")


def envelope(raw, domain, key):
    prefix = b"NMPREC01\x01\0" + domain_hash(domain) + key
    if len(raw) < len(prefix)+8+33 or not raw.startswith(prefix) or sha(b"near-marketplace:record:v1\0"+raw[:-32]) != raw[-32:]:
        raise ValueError("Invalid v1 envelope")
    if int.from_bytes(raw[len(prefix):len(prefix)+8], "little") == 0: raise ValueError("Zero revision")
    return raw[len(prefix)+8:-32]


def verify_records(manifest, rpc, block, op, terms):
    a, f, domain = manifest["accounts"], manifest["flow"], deployment(manifest)
    reservation = sha(b"nmp:listing-storage:v1\0" + sha(op))
    life = record(rpc, a["marketplace"], block, b"nmp:runtime:admission-life:v1:" + reservation)
    expected = b"NMPALIF1" + sha(op) + b"\2" + integer(number(f, "reserve_yocto",128),16)
    if life != expected + sha(expected): raise ValueError("Admission not completed for exact operation")
    for key, value in [
        (b"\2"+integer(1,8), b"\2"+integer(1,8)+terms+b"\1"+integer(1,8)+sha(op)+b"\0"),
        (b"\6"+integer(1,8), b"\6"+op[len(OP_TAG):]+b"\2"),
    ]:
        raw = record(rpc, a["marketplace"], block, b"nmp:r:\x01\0"+domain_hash(domain)+key)
        if envelope(raw, domain, key) != value: raise ValueError("Listing or verification record mismatch")
    raw = record(rpc, a["marketplace"], block, b"nmp:s:\x01\0"+domain_hash(domain)+b"\2"+reservation)
    prefix = b"NMPSTG01"+domain_hash(domain)+b"\2"+reservation
    if not raw.startswith(prefix) or sha(b"near-marketplace:storage-record:v1\0"+raw[:-32]) != raw[-32:]:
        raise ValueError("Invalid reservation envelope")
    body = raw[len(prefix)+8:-32]
    owner = b"\2"+string(a["admission_owner"])
    if not body.startswith(owner) or len(body) != len(owner)+56: raise ValueError("Wrong reservation owner/size")
    allocated, remaining, staked = [int.from_bytes(body[len(owner)+16*i:len(owner)+16*(i+1)],"little") for i in range(3)]
    if allocated != number(f,"reserve_yocto",128) or remaining+staked != allocated or body[-8:] != integer(3,8):
        raise ValueError("Reservation conservation/cursor mismatch")
    return {"final_block":block,"generation":"1","attempt_nonce":"1","lifecycle":"completed","verification":"active", "commercial_listing_enabled":False,"reservation_remaining":str(remaining),"reservation_staked":str(staked)}


def command(manifest, stage, args):
    a, f = manifest["accounts"], manifest["flow"]
    gas = number(f["transaction_gas"], stage)
    if gas > 300_000_000_000_000: raise ValueError("Transaction gas exceeds NEAR limit")
    if stage in ("nft-deploy", "marketplace-deploy"):
        nft = stage == "nft-deploy"
        cmd = ["near","contract","deploy",a["nft" if nft else "marketplace"],"use-file",str(NFT if nft else MARKET),"with-init-call","new_testnet" if nft else "bootstrap_with_admission_owner"]
        tail = []
        amount = 0
    else:
        nft = stage in ("mint","approve")
        method = {"fund":"fund_testnet_storage","mint":"nft_mint","approve":"nft_approve","create":"create_nft_admission","dispatch":"dispatch_nft_admission"}[stage]
        cmd = ["near","contract","call-function","as-transaction",a["nft" if nft else "marketplace"],method]
        tail = ["sign-as", a["seller" if nft else "admission_owner"]]
        amount = number(f,{"fund":"funding_yocto","mint":"mint_deposit_yocto","approve":"approval_deposit_yocto"}[stage],128) if stage in ("fund","mint","approve") else 0
    # near-gas accepts Tgas/Ggas, not an unqualified 'gas' unit. No float loss.
    gas_text = f"{gas // 10**12}.{gas % 10**12:012d} Tgas"
    return shlex.join(cmd+["json-args",compact(args),"prepaid-gas",gas_text,"attached-deposit",f"{amount} yoctoNEAR"]+tail+["network-config","testnet","sign-with-keychain","send"])


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--manifest",type=Path,required=True,help="PUBLIC manifest; never a credential file")
    p.add_argument("--stage",choices=STAGES,required=True)
    args = p.parse_args()
    m = json.loads(args.manifest.read_text()); built = artifacts(); validate_manifest(m,built)
    rpc = RPC(m)
    block = rpc.checked("block",{"finality":"final"})["header"]
    check_accounts(m,rpc,block["hash"],args.stage)
    a,f = m["accounts"],m["flow"]
    stage = args.stage
    payload = {}
    if stage == "nft-deploy": payload = {"mint_authority_id":a["seller"],"marketplace_id":a["marketplace"],"metadata":f["nft_metadata"]}
    elif stage == "marketplace-deploy": payload = {"manifest":b64(deployment(m)),"admission_owner_id":a["admission_owner"]}
    elif stage == "fund": payload = {"funding_nonce":str(number(f,"funding_nonce",64,0))}
    else:
        token = view(rpc,a["nft"],block["hash"],"nft_token",{"token_id":f["token_id"]})
        if stage == "mint":
            if token is not None: raise ValueError("Token already exists; do not remint")
            payload = {"token_id":f["token_id"],"token_owner_id":a["seller"],"token_metadata":f["token_metadata"]}
        else:
            if token is None or token["owner_id"] != a["seller"]: raise ValueError("Token owner mismatch")
            if stage == "inspect": print(compact({"final_block":block["hash"],"token":token})); return
            if stage == "approve": payload = {"token_id":f["token_id"],"account_id":a["marketplace"],"msg":None}
            else:
                approval = token["approved_account_ids"].get(a["marketplace"])
                if type(approval) is not int or not 0 <= approval < 2**64: raise ValueError("Missing exact numeric approval")
                if view(rpc,a["nft"],block["hash"],"nft_is_approved",{"token_id":f["token_id"],"approved_account_id":a["marketplace"],"approval_id":approval}) is not True: raise ValueError("Approval not active")
                op, terms = operation(m,approval)
                if stage == "create":
                    if number(f,"expires_at_ns") <= int(block["timestamp_nanosec"]): raise ValueError("Expired terms; new manifest requires new accounts")
                    payload = {"operation":b64(op),"storage_owner_id":a["admission_owner"],"reserve_amount":str(number(f,"reserve_yocto",128)),"creation_bytes":str(number(f,"creation_bytes")),"local_gas":str(number(f,"local_gas"))}
                elif stage == "dispatch":
                    locator = sha(b"nmp:nft-admission:v1\0"+domain_hash(deployment(m))+string(f["token_id"])+string(a["seller"])+integer(approval,8))
                    # Missing/stale approval yields a different locator and fails closed.
                    record(rpc,a["marketplace"],block["hash"],b"nmp:runtime:nft-admission:v1:"+locator)
                    payload = {"admission_id":b64(locator)}
                else:
                    print(compact(verify_records(m,rpc,block["hash"],op,terms))); return
    print(command(m,stage,payload))


if __name__ == "__main__": main()
