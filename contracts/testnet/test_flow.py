import copy
import json
import shlex
import sys
import unittest
from flow import (operation, deployment, sha, b64, command, number, STAGES,
                  envelope, domain_hash, integer, verify_records, string)


def fixture():
    # Synthetic codec/command fixture, NEVER deployment or financial defaults.
    return {"accounts":{"nft":"nft.testnet","marketplace":"market.testnet","admission_owner":"owner.testnet","seller":"seller.testnet"},
            "genesis_block_hash":"1"*31+"2","artifact_sha256":{"nft":"04"*32,"marketplace":"03"*32},
            "flow":{"token_id":"é"*512,"generation":"1","fee_bps":123,"fee_cap_bps":456,"fee_account":"fees.testnet",
                    "price_yocto":"1000","expires_at_ns":"18446744073709551615","policy_version":"7","policy_hash":"07"*32,"correlation":"08"*32,
                    "first_call_gas":"20000000000000","second_call_gas":"20000000000000","callback_gas":"40000000000000","max_result_bytes":4096,
                    "transaction_gas":{s:"100000000000001" for s in STAGES}, "funding_yocto":"1000000000000000000000000",
                    "mint_deposit_yocto":"1000000000000000000000000","approval_deposit_yocto":"1000000000000000000000000","reserve_yocto":"1000"}}


class FlowTests(unittest.TestCase):
    def test_lossless_integer_bounds_and_no_economic_defaults(self):
        m = fixture()
        op,_ = operation(m, 2**64-1)
        self.assertIn(bytes([255])*8,op)
        for value in [-1,True,1.5,"01",2**64]:
            with self.assertRaises(ValueError): number({"x":value},"x")
        del m["flow"]["fee_bps"]
        with self.assertRaises(KeyError): operation(m,1)

    def test_fee_cap_matches_contract_strict_upper_bound(self):
        m = fixture()
        m["flow"]["fee_cap_bps"] = "10000"
        with self.assertRaisesRegex(ValueError, "fee terms"):
            operation(m, 1)
        m["flow"]["fee_cap_bps"] = "9999"
        operation(m, 1)

    def test_canonical_manifest_and_identity_affect_commitments(self):
        m = fixture(); n = copy.deepcopy(m)
        self.assertEqual(deployment(m),deployment(dict(reversed(list(m.items())))))
        n["accounts"]["seller"] = "different.testnet"
        self.assertNotEqual(operation(m,1),operation(n,1))
        n = copy.deepcopy(m); n["flow"]["generation"]="2"
        with self.assertRaises(ValueError): operation(n,1)

    def test_commands_are_atomic_self_deployments_and_fixed_testnet(self):
        for stage in STAGES:
            if stage in ("inspect","verify"): continue
            argv = shlex.split(command(fixture(),stage,{"untrusted":"$(never-run); `no`"}))
            self.assertEqual(argv[-4:],["network-config","testnet","sign-with-keychain","send"])
            self.assertEqual(argv[argv.index("prepaid-gas")+1],"100.000000000001 Tgas")
            if stage.endswith("deploy"):
                self.assertIn("with-init-call",argv)
                self.assertNotIn("without-init-call",argv)
                self.assertNotIn("sign-as",argv)
            else:
                self.assertIn("sign-as",argv)
            self.assertEqual(json.loads(argv[argv.index("json-args")+1])["untrusted"],"$(never-run); `no`")

    def test_finalized_record_verifier_checks_exact_operation_and_reservation(self):
        import base64
        m = fixture(); op,terms = operation(m,2**64-1); domain = deployment(m)
        reserve = sha(b"nmp:listing-storage:v1\0"+sha(op)); values={}
        def put(key,raw): values[key]=raw
        life = b"NMPALIF1"+sha(op)+b"\2"+integer(1000,16)
        put(b"nmp:runtime:admission-life:v1:"+reserve,life+sha(life))
        for key,value in [(b"\2"+integer(1,8),b"\2"+integer(1,8)+terms+b"\1"+integer(1,8)+sha(op)+b"\0"),
                          (b"\6"+integer(1,8),b"\6"+op[len(b"near-marketplace:operation:v1\0"):]+b"\2")]:
            raw=b"NMPREC01\x01\0"+domain_hash(domain)+key+integer(1,8)+value
            put(b"nmp:r:\x01\0"+domain_hash(domain)+key,raw+sha(b"near-marketplace:record:v1\0"+raw))
        raw=b"NMPSTG01"+domain_hash(domain)+b"\2"+reserve+integer(1,8)+b"\2"+string("owner.testnet")+integer(1000,16)+integer(600,16)+integer(400,16)+integer(3,8)
        key=b"nmp:s:\x01\0"+domain_hash(domain)+b"\2"+reserve
        put(key,raw+sha(b"near-marketplace:storage-record:v1\0"+raw))
        class Fake:
            def checked(self,method,params):
                k=base64.b64decode(params["prefix_base64"])
                return {"values":[{"key":b64(k),"value":b64(values[k])}]}
        self.assertEqual(verify_records(m,Fake(),"synthetic",op,terms)["verification"],"active")
        values[key]=values[key][:-1]+bytes([values[key][-1]^1])
        with self.assertRaises(ValueError): verify_records(m,Fake(),"synthetic",op,terms)


if __name__ == "__main__":
    if sys.argv[1:] == ["--codec-fixture"]:
        m = fixture(); op,_ = operation(m,2**64-1)
        print(json.dumps({"domain":b64(deployment(m)),"operation":b64(op)}))
    else: unittest.main()
