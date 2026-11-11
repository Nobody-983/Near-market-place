import unittest
from unittest.mock import patch
from snapshot import collect


class SnapshotTests(unittest.TestCase):
    def test_discovery_disagreement_stops_before_account_reads(self):
        calls = []

        def raw(_rpc, method, _params):
            calls.append(method)
            if method == "status":
                return {"chain_id": "testnet", "sync_info": {"syncing": False}, "genesis_hash": "expected"}
            if method == "EXPERIMENTAL_genesis_config":
                return {"chain_id": "testnet", "genesis_height": 42}
            return {"header": {"height": 42, "hash": "different", "prev_hash": "1" * 32}}

        with patch("snapshot.RPC.raw", raw):
            with self.assertRaisesRegex(ValueError, "Genesis discovery disagrees"):
                collect()
        self.assertEqual(calls, ["status", "EXPERIMENTAL_genesis_config", "block"])

    def test_wrong_finalized_query_block_rejects(self):
        def raw(_rpc, method, _params):
            if method == "status":
                return {"chain_id": "testnet", "sync_info": {"syncing": False}, "genesis_hash": "expected"}
            if method == "EXPERIMENTAL_genesis_config":
                return {"chain_id": "testnet", "genesis_height": 42}
            return {"header": {"height": 42, "hash": "expected", "prev_hash": "1" * 32}}

        def checked(_rpc, method, _params):
            if method == "block":
                return {"header": {"hash": "final"}}
            if method == "EXPERIMENTAL_protocol_config":
                return {"chain_id": "testnet", "genesis_height": 42}
            return {"block_hash": "wrong"}

        with patch("snapshot.RPC.raw", raw), patch("snapshot.RPC.checked", checked):
            with self.assertRaisesRegex(ValueError, "pinned finalized block"):
                collect()
