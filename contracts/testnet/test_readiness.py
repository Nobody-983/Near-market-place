import copy
import unittest
from readiness import RPC, accounts_preflight, validate_manifest, flow_plan, exports


class ReadinessTests(unittest.TestCase):
    def manifest(self):
        accounts = {role: role.replace("_", "-") + ".testnet" for role in ["nft", "marketplace", "admission_owner", "seller"]}
        return {"network": "testnet", "rpc_url": "https://rpc.testnet.near.org", "accounts": accounts,
                "genesis_height": 0, "genesis_block_hash": "1" * 32,
                "artifact_sha256": {"nft": "a", "marketplace": "b"},
                "expected_access_keys": {r: [{"public_key": "public-only", "permission": "FullAccess"}] for r in accounts}}

    def test_mainnet_shared_accounts_and_wrong_artifacts_reject(self):
        good = self.manifest()
        built = {"nft": {"sha256": "a"}, "marketplace": {"sha256": "b"}}
        validate_manifest(good, built)
        for change in range(5):
            bad = copy.deepcopy(good)
            if change == 0: bad["network"] = "mainnet"
            if change == 1: bad["rpc_url"] = "https://rpc.mainnet.near.org"
            if change == 2: bad["accounts"]["nft"] = "nft.near"
            if change == 3: bad["accounts"]["nft"] = bad["accounts"]["marketplace"]
            if change == 4: bad["artifact_sha256"]["nft"] = "changed"
            with self.assertRaises(ValueError): validate_manifest(bad, built)

    def test_identity_is_rechecked_for_each_operation_and_wrong_chain_stops(self):
        rpc = RPC(self.manifest())
        calls = []
        def raw(method, params):
            calls.append(method)
            if method == "status": return {"chain_id": "testnet", "sync_info": {"syncing": False}}
            if method == "block": return {"header": {"hash": "1" * 32}}
            return {}
        rpc.raw = raw
        rpc.checked("query", {}); rpc.checked("query", {})
        self.assertEqual(calls, ["status", "block", "query"] * 2)
        rpc.raw = lambda method, params: {"chain_id": "mainnet"}
        with self.assertRaises(ValueError): rpc.checked("query", {})

    def test_existing_contracts_rejected_and_full_flow_remains_blocked(self):
        class Fake:
            def checked(self, method, params):
                if method == "block": return {"header": {"hash": "final"}}
                if params["request_type"] == "view_account": return {"code_hash": "existing"}
                return {"keys": [{"public_key": "public-only", "access_key": {"permission": "FullAccess"}}]}
        with self.assertRaisesRegex(ValueError, "existing contract"):
            accounts_preflight(self.manifest(), Fake())
        self.assertIn("authority", flow_plan()[0]["status"])
        self.assertEqual(flow_plan()[2]["method"], "fund_testnet_storage")

    def test_invalid_or_truncated_artifacts_reject(self):
        for data in [b"", b"not wasm", b"\0asm\x01\0\0\0\x07\x0a"]:
            with self.assertRaises(ValueError): exports(data)

    def test_preparation_template_cannot_authorize_command_generation(self):
        manifest = self.manifest()
        manifest["preparation"] = {"status": "blocked", "decisions": ["expiry", "budgets"]}
        built = {"nft": {"sha256": "a"}, "marketplace": {"sha256": "b"}}
        with self.assertRaisesRegex(ValueError, "Preparation template is not approved"):
            validate_manifest(manifest, built)
        # Changing only the status must not bypass unresolved review flags.
        manifest["preparation"]["status"] = "approved"
        with self.assertRaisesRegex(ValueError, "Preparation template is not approved"):
            validate_manifest(manifest, built)


if __name__ == "__main__":
    unittest.main()
