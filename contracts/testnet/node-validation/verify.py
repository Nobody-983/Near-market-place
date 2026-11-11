#!/usr/bin/env python3
"""Independent offline verifier; no signer, subprocess or RPC imports.

Inputs are raw node RPC evidence. Missing evidence is never inferred as success.
"""
import argparse
import base64
import json
from pathlib import Path
from common import *
from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

def signed_transaction(encoded, submitted, expected_hash, public_key):
    """Independent bounded Borsh V0 decoder; compare every authorized action."""
    raw=base64.b64decode(encoded,validate=True); pos=0
    def take(n):
        nonlocal pos
        require(0<=n<=len(raw)-pos,'truncated signed transaction')
        v=raw[pos:pos+n]; pos+=n; return v
    def num(n): return int.from_bytes(take(n),'little')
    def blob(): return take(num(4))
    signer=blob().decode(); require(take(1)==b'\0','Ed25519 public key required')
    key=take(32); nonce=num(8); receiver=blob().decode(); block=take(32)
    count=num(4); require(1<=count<=3,'action count')
    actions=[]
    for _ in range(count):
        kind=num(1)
        if kind==1: actions.append({'kind':'deploy','code':b64(blob())})
        elif kind==2:
            method=blob().decode(); args=json.loads(blob()); gas=num(8); deposit=num(16)
            actions.append({'kind':'call','method':method,'args':args,'gas':str(gas),'deposit':str(deposit)})
        else: raise CheckError('forbidden action in signed bytes')
    tx=raw[:pos]
    require(take(1)==b'\0','Ed25519 signature required')
    signature=take(64)
    require(pos==len(raw),'signature/trailing bytes')
    require(base58(sha(tx))==expected_hash,'transaction hash differs from signed bytes')
    try: Ed25519PublicKey.from_public_bytes(key).verify(signature,sha(tx))
    except (InvalidSignature, ValueError) as exc: raise CheckError('invalid transaction signature') from exc
    require('ed25519:'+base58(key)==public_key,'signed public key inventory mismatch')
    require({'signer':signer,'receiver':receiver,'nonce':str(nonce),'block_hash':base58(block),'actions':actions}==submitted,'signed transaction differs from approved actions')
    require(signer in ACCOUNTS and receiver in (NFT,MARKET),'signed account allowlist')
    return {'hash':expected_hash,'actions':len(actions),'signer':signer,'receiver':receiver}

def storage_body(rows,domain,key):
    storage_key=b'nmp:s:root:v1' if key==b'\0' else b'nmp:s:\x01\0'+domain_hash(domain)+key
    raw=rows[storage_key]; prefix=b'NMPSTG01'+domain_hash(domain)+key
    require(raw.startswith(prefix) and len(raw)>len(prefix)+8+32,'storage envelope prefix')
    require(int.from_bytes(raw[len(prefix):len(prefix)+8],'little')>0,'zero storage revision')
    require(sha(b'near-marketplace:storage-record:v1\0'+raw[:-32])==raw[-32:],'storage envelope checksum')
    return raw[len(prefix)+8:-32]

def storage_totals(body):
    require(len(body)==112,'storage totals size')
    names=('funded','available','reserved','staked','in_flight','queued','withdrawn')
    v={k:int.from_bytes(body[i*16:(i+1)*16],'little') for i,k in enumerate(names)}
    require(v['funded']==v['available']+v['reserved']+v['staked']+v['in_flight']+v['withdrawn'],'storage totals conservation')
    require(v['queued']<=v['in_flight'],'queued subset')
    return v

def accounting(snapshot,m):
    rows=state_map(snapshot,MARKET); domain=deployment(m)
    root=storage_body(rows,domain,b'\0')
    require(root[:1]==b'\0' and len(root)==130,'storage root size/type')
    totals=storage_totals(root[1:113])
    require(root[-1]==0,'financial hold unexpectedly enabled')
    owner_key=b'\1'+string(OWNER)
    key=b'nmp:s:\x01\0'+domain_hash(domain)+owner_key
    if key in rows:
        owner=storage_body(rows,domain,owner_key)
        require(owner[:1]==b'\1','owner totals type')
        require(storage_totals(owner[1:])==totals,'global and only owner totals disagree')
    else: require(all(v==0 for v in totals.values()),'totals without owner')
    require(totals['in_flight']==totals['queued']==totals['withdrawn']==0,'unexpected external payout accounting')
    return {k:str(v) for k,v in totals.items()}

def outcomes(result):
    rows = [result["transaction_outcome"], *result["receipts_outcome"]]
    require(len({r["id"] for r in rows}) == len(rows), "duplicate outcomes")
    return {r["id"]: r for r in rows}

def graph(result, bodies):
    require(result["final_execution_status"] == "FINAL", "receipt graph not FINAL")
    obs = outcomes(result)
    require(result['transaction_outcome']['id']==result['transaction']['hash'],'transaction outcome identity')
    roots = result["transaction_outcome"]["outcome"]["receipt_ids"]
    required = {rid for r in obs.values() for rid in r["outcome"]["receipt_ids"]}
    require(required <= obs.keys(), "outcome graph is incomplete")
    require(set(obs) - {result["transaction_outcome"]["id"]} == required, "orphan receipt outcome")
    missing = required - bodies.keys()
    if missing:
        raise Inconclusive("missing authentic receipt bodies: " + ",".join(sorted(missing)))
    for rid in required:
        require(bodies[rid]["receipt_id"] == rid, "receipt ID mismatch")
        status = obs[rid]["outcome"]["status"]
        require("Failure" in status or "SuccessValue" in status or "SuccessReceiptId" in status, "nonterminal receipt")
    # Prove reachability and reject cycles, not just set equality.
    visited=set(); pending=list(roots)
    while pending:
        rid=pending.pop()
        require(rid not in visited,'duplicate edge/cycle in receipt graph')
        visited.add(rid); pending.extend(obs[rid]['outcome']['receipt_ids'])
    require(visited==required,'disconnected receipt graph')
    return {"outcomes": len(obs), "receipts": len(required), "roots": roots,
            "tokens_burnt": str(sum(int(r["outcome"]["tokens_burnt"]) for r in obs.values()))}

def action_body(receipt):
    body = receipt["receipt"]
    return body.get("Action", body.get("PromiseYield"))

def transaction_binding(e):
    submitted=e['submitted']; result=e['result']; roots=result['transaction_outcome']['outcome']['receipt_ids']
    require(len(roots)==1,'expected one initial action receipt')
    receipt=e['bodies'][roots[0]]; body=action_body(receipt)
    require(body is not None,'initial receipt must carry actions')
    require(receipt['predecessor_id']==submitted['signer'] and receipt['receiver_id']==submitted['receiver'],'initial receipt identity')
    key=e['before']['accounts'][submitted['signer']]['keys']['keys'][0]['public_key']
    require(body['signer_id']==submitted['signer'] and body['signer_public_key']==key,'initial receipt signer')
    require(not body.get('input_data_ids') and not body.get('output_data_receivers'),'unexpected initial dependencies')
    require(len(body['actions'])==len(submitted['actions']),'initial action count')
    for actual,expected in zip(body['actions'],submitted['actions']):
        if expected['kind']=='deploy':
            # Pinned nearcore ActionView carries base64(SHA256(code)), even
            # though the field is named code. Signed Borsh retains the bytes.
            require(set(actual)=={'DeployContract'} and actual['DeployContract']['code']==b64(sha(base64.b64decode(expected['code'],validate=True))),'deployed code hash differs from signed transaction')
        else:
            require(set(actual)=={'FunctionCall'},'initial action kind')
            actual=actual['FunctionCall']
            require(actual['method_name']==expected['method'] and json.loads(base64.b64decode(actual['args'],validate=True))==expected['args'],'initial call differs from signed transaction')
            require(int(actual['gas'])==int(expected['gas']) and int(actual['deposit'])==int(expected['deposit']),'initial call gas/deposit')
    status=outcomes(result)[roots[0]]['outcome']['status']
    require(type(e.get('expected_success')) is bool,'expected receipt outcome missing')
    require(('Failure' not in status)==e['expected_success'],'unexpected initial receipt outcome')
    if e.get('expected_failure_action_index') is not None:
        require('Failure' in status,'expected initializer failure')
        error=status['Failure'].get('ActionError',{})
        require(error.get('index')==e['expected_failure_action_index'],'failure occurred in the wrong action')
        require('FunctionCallError' in error.get('kind',{}),'initializer did not fail during function execution')
    require(submitted['block_hash']==e['before']['block_hash'],'transaction reference block mismatch')
    for account in ACCOUNTS:
        old=e['before']['accounts'][account]['keys']['keys']
        new=e['after']['accounts'][account]['keys']['keys']
        require(len(old)==len(new)==1 and old[0]['public_key']==new[0]['public_key'],'key inventory changed')
        require(old[0]['access_key']['permission']==new[0]['access_key']['permission']=='FullAccess','key permission changed')
        nonce=int(old[0]['access_key']['nonce'])
        expected_nonce=int(submitted['nonce']) if account==submitted['signer'] else nonce
        if account==submitted['signer']: require(expected_nonce==nonce+1,'signer nonce is not sequential')
        require(int(new[0]['access_key']['nonce'])==expected_nonce,'final signer nonce mismatch')
    return {'initial_receipt':roots[0],'success':e['expected_success']}

def function_receipts(evidence, method):
    found = []
    for r in evidence["bodies"].values():
        b = action_body(r)
        if b is None: continue
        for action in b["actions"]:
            if "FunctionCall" in action and action["FunctionCall"]["method_name"] == method:
                found.append((r, action["FunctionCall"]))
    return found

def dispatch_graph(evidence, m, approval):
    graph(evidence["result"], evidence["bodies"])
    f = m["flow"]
    methods = ["nft_token", "nft_is_approved", "on_nft_verified"]
    expected_args = [{"token_id": f["token_id"]}, {"token_id": f["token_id"], "approved_account_id": MARKET, "approval_id": approval},
                     {"generation": "1", "attempt_nonce": "1"}]
    nodes = []
    for method, args, gas in zip(methods, expected_args, [f["first_call_gas"], f["second_call_gas"], f["callback_gas"]]):
        rows = function_receipts(evidence, method)
        require(len(rows) == 1, "expected exactly one " + method)
        r, call = rows[0]
        require(r["predecessor_id"] == MARKET and r["receiver_id"] == (MARKET if method == methods[2] else NFT), "child identity")
        require(int(call["deposit"]) == 0 and int(call["gas"]) == int(gas), "child immutable gas/deposit")
        require(json.loads(base64.b64decode(call["args"])) == args, "child arguments")
        require(len(action_body(r)["actions"]) == 1, "unexpected child actions")
        nodes.append(r)
    outputs = []
    for r in nodes[:2]:
        b = action_body(r)
        require(b["input_data_ids"] == [], "unexpected NFT input dependencies")
        require(len(b["output_data_receivers"]) == 1, "missing output dependency")
        target = b["output_data_receivers"][0]
        require(target["receiver_id"] == MARKET, "data receiver")
        outputs.append(target["data_id"])
    require(action_body(nodes[2])["input_data_ids"] == outputs, "callback dependency order")
    return {method: nodes[i]["receipt_id"] for i, method in enumerate(methods)}

def data_dependencies(evidence):
    """Derive required data from authentic action bodies, never a claimed gap list.

    These fixed-artifact view calls return SuccessValue or Failure directly.
    Promise-return forwarding requires additional evidence and is not guessed.
    Captured data bodies prove payload consistency, not their delivery time.
    """
    obs=outcomes(evidence['result']); producers={}; consumers={}
    for rid, receipt in evidence['bodies'].items():
        body=action_body(receipt)
        if body is None: continue
        require(not body.get('is_promise_yield',False),'unexpected yield receipt')
        for target in body.get('output_data_receivers',[]):
            key=target['data_id']
            require(key not in producers,'duplicate output data ID')
            producers[key]=(receipt,target)
        for key in body.get('input_data_ids',[]):
            require(key not in consumers,'duplicate input data ID')
            consumers[key]=receipt
    require(producers.keys()==consumers.keys(),'unmatched input/output data dependencies')
    seen=set()
    for rid, receipt in evidence.get('data_receipts',{}).items():
        require(receipt['receipt_id']==rid,'DataReceipt ID mismatch')
        body=receipt['receipt'].get('Data')
        require(body is not None and not body.get('is_promise_resume',False),'unexpected data receipt kind')
        key=body['data_id']
        require(key in producers and key not in seen,'orphan/duplicate DataReceipt')
        producer,target=producers[key]; consumer=consumers[key]
        require(receipt['predecessor_id']==producer['receiver_id'],'data predecessor mismatch')
        require(receipt['receiver_id']==target['receiver_id']==consumer['receiver_id'],'data receiver mismatch')
        status=obs[producer['receipt_id']]['outcome']['status']
        if 'Failure' in status: expected=None
        elif 'SuccessValue' in status: expected=base64.b64decode(status['SuccessValue'],validate=True)
        else: raise Inconclusive('forwarded data result requires an additional receipt proof')
        actual=None if body['data'] is None else base64.b64decode(body['data'],validate=True)
        require(actual==expected,'DataReceipt payload differs from producer outcome')
        seen.add(key)
    missing=sorted(producers.keys()-seen)
    if missing: raise Inconclusive('missing authentic DataReceipt bodies: '+','.join(missing))
    return {'dependencies':len(producers),'data_receipts':len(seen)}

def storage_delta(before, after, account):
    a, b = state_map(before, account), state_map(after, account)
    state_bytes = lambda rows: sum(len(k) + len(v) + 40 for k, v in rows.items())
    growth = state_bytes(b) - state_bytes(a)
    observed = int(after["accounts"][account]["account"]["storage_usage"]) - int(before["accounts"][account]["account"]["storage_usage"])
    code_delta = after["accounts"][account]["code_bytes"] - before["accounts"][account]["code_bytes"]
    require(observed == growth + code_delta, "raw records/code do not reconcile with storage_usage")
    return {"bytes": observed, "state_bytes": growth, "code_bytes": code_delta, "stake_yocto": str(observed * BYTE_COST)}

def unchanged(before, after):
    for account in ACCOUNTS:
        require(state_map(before, account) == state_map(after, account), "application rollback failed: " + account)
        require(before["accounts"][account]["account"]["code_hash"] == after["accounts"][account]["account"]["code_hash"], "code rollback failed")
        require(before["accounts"][account]["account"]["storage_usage"] == after["accounts"][account]["account"]["storage_usage"], "storage rollback failed")

def balance(before, after, transactions):
    seen, burns = set(), 0
    for e in transactions:
        graph(e["result"], e["bodies"])
        for rid, row in outcomes(e["result"]).items():
            require(rid not in seen, "double-counted outcome")
            seen.add(rid)
            burns += int(row["outcome"]["tokens_burnt"])
        for r in e["bodies"].values():
            require(r["receiver_id"] in ACCOUNTS, "external balance flow requires explicit accounting")
    total = lambda s: sum(int(s["accounts"][a]["account"]["amount"]) + int(s["accounts"][a]["account"]["locked"]) for a in ACCOUNTS)
    delta = total(before) - total(after)
    require(delta == burns, f"balance delta {delta} != unique outcome burns {burns}")
    return {"balance_decrease": str(delta), "tokens_burnt": str(burns)}

def lifecycle(snapshot, m, approval, expected):
    op, terms = operation(m, approval)
    domain = deployment(m)
    rows = state_map(snapshot, MARKET)
    reservation = sha(b"nmp:listing-storage:v1\0" + sha(op))
    life = rows[b"nmp:runtime:admission-life:v1:" + reservation]
    states = {"ready": (0,0,0,1), "dispatched": (1,0,0,2), "active": (2,1,2,3), "invalid": (2,3,2,3), "released": (3,2,2,1)}
    life_n, listing_n, progress, cursor = states[expected]
    raw = b"NMPALIF1" + sha(op) + bytes([life_n]) + integer(m["flow"]["reserve_yocto"],16)
    require(life == raw + sha(raw), "lifecycle mismatch")
    for key, value in [(b"\2"+integer(1,8), b"\2"+integer(1,8)+terms+bytes([listing_n])+integer(1,8)+sha(op)+b"\0"),
                       (b"\6"+integer(1,8), b"\6"+op[len(b"near-marketplace:operation:v1\0"):] + bytes([progress]))]:
        require(envelope(rows[b"nmp:r:\x01\0"+domain_hash(domain)+key],domain,key) == value, "listing/operation mismatch")
    prefix = b"NMPSTG01" + domain_hash(domain) + b"\2" + reservation
    raw = rows[b"nmp:s:\x01\0"+domain_hash(domain)+b"\2"+reservation]
    require(raw.startswith(prefix) and sha(b"near-marketplace:storage-record:v1\0"+raw[:-32]) == raw[-32:], "reservation checksum")
    body = raw[len(prefix)+8:-32]
    owner = b"\2"+string(OWNER)
    require(body.startswith(owner) and len(body)==len(owner)+56, "reservation owner/size")
    values = [int.from_bytes(body[len(owner)+16*i:len(owner)+16*(i+1)],"little") for i in range(3)]
    allocated, remaining, staked = values
    require(body[-8:] == integer(cursor,8), "reservation cursor")
    # Release retains the frozen reservation; the admission guard and owner
    # credit change. Its remaining field is historical, not spendable twice.
    require(allocated == int(m["flow"]["reserve_yocto"]) and remaining+staked == allocated, "reservation conservation")
    guard=storage_body(rows,domain,b'\5'+reservation)
    require(guard==b'\5'+sha(op)+bytes([expected=='released']),'admission guard/release mismatch')
    totals=accounting(snapshot,m)
    require(int(totals['reserved'])==(0 if expected=='released' else remaining),'owner reserved attribution')
    require(snapshot["accounts"][MARKET]["views"]["storage_status"]["trading_enabled"] is False, "trading enabled")
    return {"state": expected, "cursor": cursor, "allocated": str(allocated), "remaining": str(remaining), "staked": str(staked)}

def verify_step(path, evidence_class='standalone-sandbox'):
    e = load(path)
    result = {"graph": graph(e["result"], e["bodies"])}
    require(evidence_class in ('standalone-sandbox','testloop-simulated-network') and e['evidence_class']==evidence_class,'evidence environment mismatch')
    require(e['result']['transaction']['hash']==e['transaction_hash'],'RPC transaction hash mismatch')
    result['transaction']=signed_transaction(e['signed_tx_base64'],e['submitted'],e['transaction_hash'],e['before']['accounts'][e['submitted']['signer']]['keys']['keys'][0]['public_key'])
    result['initial_receipt']=transaction_binding(e)
    before, after = e["before"], e["after"]
    require(before['header']['hash']==before['block_hash'] and after['header']['hash']==after['block_hash'],'snapshot header identity')
    require(int(after['header']['height'])>=int(before['header']['height']),'snapshot height decreased')
    for snap in (before, after):
        for row in snap["accounts"].values():
            for name in ("account", "keys", "state"):
                require(row[name]["block_hash"] == snap["block_hash"], "mixed-block snapshot")
    result["storage"] = {a: storage_delta(before,after,a) for a in ACCOUNTS}
    result["balance"] = balance(before,after,[e])
    if after['accounts'][MARKET]['code_bytes']:
        result['accounting']=accounting(after,e['manifest'])
    if e.get("assert_rollback"):
        unchanged(before,after)
    if e.get("expected_state"):
        result["state"] = lifecycle(after,e["manifest"],e["approval"],e["expected_state"])
    if e.get("assert_dispatch"):
        result["dispatch"] = dispatch_graph(e,e["manifest"],e["approval"])
    result['data_dependencies']=data_dependencies(e)
    return result

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("evidence", type=Path, nargs="+")
    a = p.parse_args()
    rows=[]
    for path in a.evidence:
        try: rows.append({"path":str(path),"status":"PASS","checks":verify_step(path)})
        except Inconclusive as e: rows.append({"path":str(path),"status":"INCONCLUSIVE","reason":str(e)})
        except (ValueError,KeyError,TypeError) as e: rows.append({"path":str(path),"status":"FAIL","reason":str(e)})
    print(json.dumps(rows,indent=2))
    return 0 if all(r["status"]=="PASS" for r in rows) else 1

if __name__ == "__main__":
    raise SystemExit(main())
