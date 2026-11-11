//! Offline execution of the approved bytes, with public preparation inputs.
//! No node, signer, network, deployment, or real receipt execution exists here.
#![cfg(all(feature = "testnet", feature = "vm-tests"))]

use near_parameters::{view::RuntimeConfigView, RuntimeConfigStore};
use near_primitives_core::{
    account::AccountContract,
    types::{Balance, Gas},
    version::PROTOCOL_VERSION,
};
use near_sdk::{
    json_types::Base64VecU8,
    serde_json::{self, json, Value},
};
use near_vm_runner::{
    logic::{mocks::mock_external::MockedExternal, types::PromiseResult, ReturnData, VMContext},
    ContractCode, MockContractRuntimeCache,
};
use std::{collections::BTreeMap, path::PathBuf};

struct Harness {
    ext: MockedExternal,
    ctx: VMContext,
    cache: MockContractRuntimeCache,
    measurements: BTreeMap<String, Value>,
    gas_per_method: Value,
}

impl Harness {
    fn new(code: Vec<u8>, account: &str, public: &Value, input: &Value) -> Self {
        let code = ContractCode::new(code, None);
        let code_hash = *code.hash();
        let account_id = account.parse().unwrap();
        let ctx = VMContext {
            current_account_id: account_id,
            signer_account_id: account.parse().unwrap(),
            signer_account_pk: vec![0; 33], // VM context placeholder, no signing
            predecessor_account_id: account.parse().unwrap(),
            refund_to_account_id: account.parse().unwrap(),
            input: vec![].into(),
            promise_results: vec![].into(),
            block_height: input["block_height"].as_u64().unwrap(),
            block_timestamp: input["block_timestamp"].as_str().unwrap().parse().unwrap(),
            epoch_height: 1,
            account_balance: Balance::from_yoctonear(
                public["amount"].as_str().unwrap().parse().unwrap(),
            ),
            account_locked_balance: Balance::ZERO,
            storage_usage: public["storage_usage"].as_u64().unwrap() + code.code().len() as u64,
            account_contract: AccountContract::Local(code_hash),
            attached_deposit: Balance::ZERO,
            prepaid_gas: Gas::from_gas(input["probe_gas"].as_u64().unwrap()),
            random_seed: vec![0; 32],
            view_config: None,
            output_data_receivers: vec![],
        };
        Self {
            ext: MockedExternal::with_code(code),
            ctx,
            cache: MockContractRuntimeCache::default(),
            measurements: BTreeMap::new(),
            gas_per_method: input["gas_per_method"].clone(),
        }
    }

    fn caller(&mut self, account: &str, deposit: &str) {
        self.ctx.predecessor_account_id = account.parse().unwrap();
        self.ctx.signer_account_id = account.parse().unwrap();
        self.ctx.attached_deposit = Balance::from_yoctonear(deposit.parse().unwrap());
    }

    fn call(&mut self, method: &str, args: Value) -> Vec<u8> {
        let config = RuntimeConfigStore::new(None)
            .get_config(PROTOCOL_VERSION)
            .clone();
        let mut ctx = self.ctx.clone();
        ctx.prepaid_gas = Gas::from_gas(self.gas_per_method[method].as_u64().unwrap());
        ctx.input = serde_json::to_vec(&args).unwrap().into();
        let before = ctx.storage_usage;
        let trie_before = self.ext.fake_trie.clone();
        let actions_before = self.ext.action_log.len();
        let prepared = near_vm_runner::prepare(
            &self.ext,
            config.wasm_config.clone(),
            Some(&self.cache),
            ctx.make_gas_counter(&config.wasm_config),
            method,
        );
        let outcome =
            near_vm_runner::run(prepared, &mut self.ext, &ctx, config.fees.clone()).unwrap();
        assert!(outcome.aborted.is_none(), "{method}: {:?}", outcome.aborted);
        self.ctx.storage_usage = outcome.storage_usage;
        self.ctx.account_balance = outcome.balance;
        let result = match outcome.return_data {
            ReturnData::Value(bytes) => bytes,
            ReturnData::None => vec![],
            other => panic!("unexpected return {other:?}"),
        };
        let extra = config.fees.storage_usage_config.num_extra_bytes_record;
        let growth = |trie: &std::collections::HashMap<Vec<u8>, Vec<u8>>| -> u64 {
            trie.iter()
                .map(|(k, v)| (k.len() + v.len()) as u64 + extra)
                .sum()
        };
        assert_eq!(
            outcome.storage_usage - before,
            growth(&self.ext.fake_trie) - growth(&trie_before)
        );
        self.measurements.insert(method.into(), json!({
            "storage_delta_bytes": outcome.storage_usage - before,
            "input_bytes": ctx.input.len(),
            "storage_usage_bytes": outcome.storage_usage,
            "burnt_gas": outcome.burnt_gas.as_gas().to_string(),
            "used_gas": outcome.used_gas.as_gas().to_string(),
            "result_bytes": result.len(),
            "new_action_log_entries": self.ext.action_log.len() - actions_before,
            "records": self.ext.fake_trie.iter().filter(|(k, v)| trie_before.get(*k) != Some(*v))
                .map(|(k,v)| json!({"key_hex": k.iter().map(|b|format!("{b:02x}")).collect::<String>(),
                    "key_bytes": k.len(), "value_bytes": v.len(),
                    "previous_value_bytes": trie_before.get(k).map(Vec::len)})).collect::<Vec<_>>(),
        }));
        result
    }
}

#[test]
#[ignore = "explicit public input/output paths required; offline measurements only"]
fn measure_preparation() {
    let input: Value = serde_json::from_slice(
        &std::fs::read(
            std::env::var("NEAR_PREPARATION_INPUT").expect("public measurement input path"),
        )
        .unwrap(),
    )
    .unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let snapshot: Value = serde_json::from_slice(
        &std::fs::read(root.join("contracts/testnet/public/rpc-snapshot.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        snapshot["protocol_config"]["protocol_version"],
        json!(PROTOCOL_VERSION)
    );
    let config = RuntimeConfigStore::new(None)
        .get_config(PROTOCOL_VERSION)
        .clone();
    let local = serde_json::to_value(RuntimeConfigView::from((*config).clone())).unwrap();
    let remote: RuntimeConfigView =
        serde_json::from_value(snapshot["protocol_config"]["runtime_config"].clone()).unwrap();
    let remote = serde_json::to_value(remote).unwrap();
    for field in [
        "wasm_config",
        "transaction_costs",
        "storage_amount_per_byte",
        "min_gas_purchase_price",
    ] {
        assert_eq!(
            local[field], remote[field],
            "RPC and VM config differ: {field}"
        );
    }
    assert_eq!(
        config.storage_amount_per_byte().as_yoctonear(),
        near_sdk::env::storage_byte_cost().as_yoctonear()
    );
    let m = &input["manifest"];
    let f = &m["flow"];
    let a = &m["accounts"];
    let nft_id = a["nft"].as_str().unwrap();
    let market_id = a["marketplace"].as_str().unwrap();
    let operator = a["seller"].as_str().unwrap();
    assert_eq!(a["seller"], a["admission_owner"]);
    let nft_code = std::fs::read(root.join(
        "contracts/nft/target/testnet/wasm32-unknown-unknown/release/near_nft_contract.wasm",
    ))
    .unwrap();
    let market_code = std::fs::read(root.join("contracts/marketplace/runtime/target/testnet/wasm32-unknown-unknown/release/near_marketplace_runtime_prototype.wasm")).unwrap();
    for (name, code) in [("nft", &nft_code), ("marketplace", &market_code)] {
        let hex = near_sdk::env::sha256(code)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(m["artifact_sha256"][name], hex);
    }
    let mut nft = Harness::new(
        nft_code,
        nft_id,
        &snapshot["accounts"][nft_id]["view_account"],
        &input,
    );
    let mut market = Harness::new(
        market_code,
        market_id,
        &snapshot["accounts"][market_id]["view_account"],
        &input,
    );
    nft.call("new_testnet", json!({"mint_authority_id": operator, "marketplace_id": market_id, "metadata": f["nft_metadata"]}));
    market.call(
        "bootstrap_with_admission_owner",
        json!({"manifest": input["domain"], "admission_owner_id": operator}),
    );
    nft.caller(operator, f["mint_deposit_yocto"].as_str().unwrap());
    nft.call("nft_mint", json!({"token_id": f["token_id"], "token_owner_id": operator, "token_metadata": f["token_metadata"]}));
    nft.caller(operator, f["approval_deposit_yocto"].as_str().unwrap());
    nft.call(
        "nft_approve",
        json!({"token_id": f["token_id"], "account_id": market_id, "msg": null}),
    );
    nft.caller(market_id, "0");
    let token = nft.call("nft_token", json!({"token_id": f["token_id"]}));
    let approved = nft.call(
        "nft_is_approved",
        json!({"token_id": f["token_id"], "approved_account_id": market_id, "approval_id": 1}),
    );
    assert_eq!(approved, b"true");
    market.caller(operator, f["funding_yocto"].as_str().unwrap());
    let funding: Value = serde_json::from_slice(&market.call(
        "fund_testnet_storage",
        json!({"funding_nonce": f["funding_nonce"]}),
    ))
    .unwrap();
    market.caller(operator, "0");
    market.call("create_nft_admission", json!({"operation": input["operation"], "storage_owner_id": operator,
        "reserve_amount": f["reserve_yocto"], "creation_bytes": f["creation_bytes"], "local_gas": f["local_gas"]}));
    market.call(
        "dispatch_nft_admission",
        json!({"admission_id": input["admission_id"]}),
    );
    // Only in this offline harness: pass the actual NFT Wasm return bytes to the
    // callback VM context. This does not establish node delivery or rollback.
    market.caller(market_id, "0");
    market.ctx.promise_results = vec![
        PromiseResult::Successful(token.clone().into()),
        PromiseResult::Successful(approved.into()),
    ]
    .into();
    assert_eq!(
        market.call(
            "on_nft_verified",
            json!({"generation": "1", "attempt_nonce": "1"})
        ),
        b"\"active\""
    );
    market.ctx.promise_results = vec![].into();
    let status: Value = serde_json::from_slice(&market.call("storage_status", json!({}))).unwrap();
    assert_eq!(status["trading_enabled"], false);
    let report = json!({"kind": "offline Wasm measurement; not node/receipt evidence", "protocol_version": PROTOCOL_VERSION,
        "rpc_vm_cost_configs_match": true, "storage_byte_cost_yocto": config.storage_amount_per_byte().as_yoctonear().to_string(),
        "input_sha256": near_sdk::env::sha256(serde_json::to_vec(&input).unwrap()).iter().map(|b|format!("{b:02x}")).collect::<String>(),
        "nft": nft.measurements, "marketplace": market.measurements, "funding_result": funding,
        "token_result": serde_json::from_slice::<Value>(&token).unwrap(), "storage_status": status,
        "marketplace_records": market.ext.fake_trie.iter().map(|(k,v)| json!({"key": Base64VecU8(k.clone()), "value": Base64VecU8(v.clone())})).collect::<Vec<_>>()});
    std::fs::write(
        std::env::var("NEAR_PREPARATION_OUTPUT").expect("public output path"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}
