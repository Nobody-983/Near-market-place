//! Execute the actual NFT Wasm ABI. MockedExternal is not a node transaction:
//! no receipt rollback, finality, OOG recovery or live migration is proved here.
use super::*;
use std::{path::PathBuf, sync::OnceLock};

use near_parameters::RuntimeConfigStore;
use near_primitives_core::{
    account::AccountContract,
    types::{Balance, Gas as RuntimeGas},
    version::PROTOCOL_VERSION,
};
use near_vm_runner::{
    logic::{mocks::mock_external::MockedExternal, ReturnData, VMContext, VMOutcome},
    ContractCode, MockContractRuntimeCache,
};
use serde_json::{json, Value};

struct Harness {
    ext: MockedExternal,
    context: VMContext,
    cache: MockContractRuntimeCache,
}

#[cfg(feature = "testnet")]
#[test]
fn wasm_testnet_init_and_mint_require_authority_and_funded_storage() {
    let mut h = Harness::new();
    let args = json!({"mint_authority_id":alice(),"metadata":contract_metadata(),"marketplace_id":marketplace()});
    h.context.predecessor_account_id = bob();
    h.rejects_without_writes("new_testnet", args.clone());
    h.context.predecessor_account_id = contract_id();
    success(h.call("new_testnet", args.clone()));
    h.rejects_without_writes("new_testnet", args);
    assert_eq!(
        success(h.call("testnet_mint_authority", json!({}))),
        b"\"alice.testnet\""
    );
    let args = json!({"token_id":"x".repeat(1024),"token_owner_id":alice(),"token_metadata":token_metadata("test")});
    h.context.attached_deposit = Balance::from_near(1);
    h.context.predecessor_account_id = bob();
    h.context.signer_account_id = alice();
    h.rejects_without_writes("nft_mint", args.clone());
    h.context.predecessor_account_id = alice();
    let mut bad = args.clone();
    bad["token_owner_id"] = json!(bob());
    h.rejects_without_writes("nft_mint", bad);
    let root = h.ext.fake_trie[b"STATE".as_slice()].clone();
    let token: Value = serde_json::from_slice(&success(h.call("nft_mint", args.clone()))).unwrap();
    assert_eq!(token["owner_id"], json!(alice()));
    let minted_root = &h.ext.fake_trie[b"STATE".as_slice()];
    // SDK enumeration length advances inside STATE; the frozen layout/size
    // stays compatible. This NFT root is intentionally not receiverless.
    assert_eq!(root.len(), minted_root.len());
    let _: crate::migration::BindingV2State = borsh::from_slice(minted_root).unwrap();
    h.rejects_without_writes("nft_mint", args);
}

#[cfg(not(feature = "testnet"))]
#[test]
fn wasm_default_and_production_have_no_testnet_authority_or_mint_activation() {
    let mut h = Harness::new();
    h.rejects_without_writes("new_testnet", json!({}));
    h.rejects_without_writes("testnet_mint_authority", json!({}));
    success(h.call("new_with_marketplace", init_args()));
    h.context.predecessor_account_id = alice();
    h.context.attached_deposit = Balance::from_near(1);
    h.rejects_without_writes(
        "nft_mint",
        json!({"token_id":"test","token_owner_id":alice(),"token_metadata":token_metadata("test")}),
    );
}

impl Harness {
    fn new() -> Self {
        let mode = if cfg!(feature = "testnet") {
            "testnet/"
        } else if cfg!(feature = "production") {
            "production/"
        } else {
            ""
        };
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            "target/{mode}wasm32-unknown-unknown/release/near_nft_contract.wasm"
        ));
        let code = ContractCode::new(std::fs::read(&path).expect("Build NFT Wasm first"), None);
        let hash = *code.hash();
        let context = VMContext {
            current_account_id: contract_id(),
            signer_account_id: contract_id(),
            signer_account_pk: vec![0; 33],
            predecessor_account_id: contract_id(),
            refund_to_account_id: contract_id(),
            input: Vec::new().into(),
            promise_results: Vec::new().into(),
            block_height: 1,
            block_timestamp: 1,
            epoch_height: 1,
            account_balance: Balance::from_near(100),
            account_locked_balance: Balance::ZERO,
            storage_usage: code.code().len() as u64,
            account_contract: AccountContract::Local(hash),
            attached_deposit: Balance::ZERO,
            prepaid_gas: RuntimeGas::from_teragas(300),
            random_seed: vec![0; 32],
            view_config: None,
            output_data_receivers: vec![],
        };
        static CACHE: OnceLock<MockContractRuntimeCache> = OnceLock::new();
        Self {
            ext: MockedExternal::with_code(code),
            context,
            cache: CACHE.get_or_init(MockContractRuntimeCache::default).clone(),
        }
    }

    fn seeded(contract: Contract) -> Self {
        let root = borsh::to_vec(&contract).unwrap();
        let usage = env::storage_usage();
        let mut harness = Self::new();
        harness.ext.fake_trie = near_sdk::mock::with_mocked_blockchain(|b| b.take_storage());
        harness.ext.fake_trie.insert(b"STATE".to_vec(), root);
        harness.context.storage_usage += usage;
        harness.context.predecessor_account_id = marketplace();
        harness.context.attached_deposit = Balance::from_yoctonear(1);
        harness
    }

    fn call(&mut self, method: &str, input: Value) -> VMOutcome {
        let config = RuntimeConfigStore::new(None)
            .get_config(PROTOCOL_VERSION)
            .clone();
        let mut context = self.context.clone();
        context.input = serde_json::to_vec(&input).unwrap().into();
        let prepared = near_vm_runner::prepare(
            &self.ext,
            config.wasm_config.clone(),
            Some(&self.cache),
            context.make_gas_counter(&config.wasm_config),
            method,
        );
        let result =
            near_vm_runner::run(prepared, &mut self.ext, &context, config.fees.clone()).unwrap();
        if result.aborted.is_none() {
            self.context.storage_usage = result.storage_usage;
        }
        result
    }

    fn rejects_without_writes(&mut self, method: &str, args: Value) {
        let storage = self.ext.fake_trie.clone();
        let actions = self.ext.action_log.len();
        let result = self.call(method, args);
        assert!(
            result.aborted.is_some(),
            "must abort rather than return success"
        );
        assert!(result.logs.is_empty());
        assert_eq!(self.ext.action_log.len(), actions);
        assert_eq!(
            self.ext.fake_trie, storage,
            "authorization must fail before writes"
        );
    }
}

fn success(result: VMOutcome) -> Vec<u8> {
    assert!(
        result.aborted.is_none(),
        "unexpected VM failure: {:?}",
        result.aborted
    );
    match result.return_data {
        ReturnData::Value(bytes) => bytes,
        ReturnData::None => Vec::new(), // Initializers return void.
        other => panic!("expected synchronous return, got {other:?}"),
    }
}

fn init_args() -> Value {
    json!({ "owner_id": alice(), "metadata": contract_metadata(), "marketplace_id": marketplace() })
}

fn versioned_send(h: &mut Harness, id: &str) -> Value {
    h.context.predecessor_account_id = alice();
    h.context.attached_deposit = Balance::from_yoctonear(1);
    h.context.promise_results = vec![].into();
    let start = h.ext.action_log.len();
    let outcome = h.call(
        "nft_transfer_call",
        json!({"receiver_id":bob(),"token_id":id,"approval_id":null,"memo":null,"msg":"return"}),
    );
    assert!(outcome.aborted.is_none(), "{:?}", outcome.aborted);
    h.ext.action_log[start..]
        .iter()
        .find_map(|a| match a {
            near_vm_runner::logic::mocks::mock_external::MockAction::FunctionCallWeight {
                method_name,
                args,
                ..
            } if method_name == b"nft_resolve_transfer_versioned" => {
                Some(serde_json::from_slice(args).unwrap())
            }
            _ => None,
        })
        .expect("must schedule versioned resolver")
}

fn versioned_result(h: &mut Harness, args: Value) -> Vec<u8> {
    h.context.predecessor_account_id = contract_id();
    h.context.attached_deposit = Balance::ZERO;
    h.context.promise_results = vec![near_vm_runner::logic::types::PromiseResult::Successful(
        b"true".to_vec().into(),
    )]
    .into();
    success(h.call("nft_resolve_transfer_versioned", args))
}

#[test]
fn wasm_refund_cycles_use_absent_maps_and_never_schedule_native_refunds() {
    for id in ["cycle".to_string(), "L".repeat(MAX_TOKEN_ID_BYTES)] {
        let mut c = bound_contract();
        mint(&mut c, &id, alice());
        let mut h = Harness::seeded(c);
        for _ in 0..6 {
            let args = versioned_send(&mut h, &id);
            assert_eq!(args["approved_account_ids"], Value::Null);
            let count = h.ext.action_log.len();
            assert_eq!(versioned_result(&mut h, args.clone()), b"false");
            assert_eq!(h.ext.action_log.len(), count);
            h.context.predecessor_account_id = alice();
            h.context.attached_deposit = Balance::from_yoctonear(1);
            h.context.promise_results = vec![].into();
            success(h.call("nft_revoke_all", json!({"token_id":id})));
            assert_eq!(h.ext.action_log.len(), count);
            let before = h.ext.fake_trie.clone();
            assert_eq!(versioned_result(&mut h, args), b"true");
            assert_eq!(h.ext.fake_trie, before);
        }
    }
}

#[test]
fn wasm_stale_and_forged_resolvers_cannot_restore_authority() {
    let mut h = Harness::seeded(approved_contract());
    let args = versioned_send(&mut h, "sale");
    h.context.predecessor_account_id = alice();
    h.context.attached_deposit = Balance::ZERO;
    h.rejects_without_writes("nft_resolve_transfer_versioned", args.clone());
    h.context.predecessor_account_id = contract_id();
    h.rejects_without_writes("nft_resolve_transfer", args.clone());
    h.rejects_without_writes("nft_resolve_transfer_versioned", args.clone()); // no dependency
    h.context.predecessor_account_id = bob();
    h.context.attached_deposit = Balance::from_yoctonear(1);
    success(h.call(
        "nft_transfer",
        json!({"receiver_id":alice(),"token_id":"sale","approval_id":null,"memo":null}),
    ));
    h.context.predecessor_account_id = alice();
    success(h.call("nft_revoke_all", json!({"token_id":"sale"})));
    h.context.attached_deposit = Balance::from_near(1);
    success(h.call(
        "nft_approve",
        json!({"token_id":"sale","account_id":marketplace(),"msg":null}),
    ));
    h.context.attached_deposit = Balance::from_yoctonear(1);
    success(h.call(
        "nft_transfer",
        json!({"receiver_id":bob(),"token_id":"sale","approval_id":null,"memo":null}),
    ));
    let before = h.ext.fake_trie.clone();
    assert_eq!(versioned_result(&mut h, args), b"true");
    assert_eq!(h.ext.fake_trie, before);
}

#[test]
fn wasm_legacy_initializers_cannot_be_raced_or_called_through_another_signer() {
    for (method, args) in [
        ("new", init_args()),
        (
            "new_default_meta",
            json!({"owner_id":alice(),"name":"x","symbol":"X","icon":null}),
        ),
    ] {
        let mut h = Harness::new();
        h.context.predecessor_account_id = alice();
        h.rejects_without_writes(method, args.clone());
        h.context.predecessor_account_id = contract_id();
        h.context.signer_account_id = alice();
        h.rejects_without_writes(method, args.clone());
        h.context.signer_account_id = contract_id();
        success(h.call(method, args));
    }
}

fn transfer_args(id: Value) -> Value {
    json!({ "expected_seller_id": alice(), "token_id": "sale", "approval_id": id, "receiver_id": bob() })
}

#[test]
fn wasm_initializers_reject_external_binding_and_existing_roots() {
    let mut h = Harness::new();
    h.context.predecessor_account_id = alice();
    h.rejects_without_writes("new_with_marketplace", init_args());
    h.context.predecessor_account_id = contract_id();
    success(h.call("new_with_marketplace", init_args()));
    assert_eq!(
        success(h.call("marketplace_id", json!({}))),
        b"\"market.testnet\""
    );
    for method in ["new", "new_with_marketplace"] {
        h.rejects_without_writes(method, init_args());
    }
    h.rejects_without_writes(
        "new_default_meta",
        json!({"owner_id": alice(), "name": "x", "symbol": "X", "icon": null}),
    );
    for method in ["set_marketplace_id", "migrate", "prepare_binding_v2"] {
        h.rejects_without_writes(method, json!({}));
    }
}

#[test]
fn wasm_wrapper_validates_generated_json_abi_before_any_write() {
    let mut h = Harness::seeded(approved_contract());
    for invalid in [
        json!(1),
        json!(null),
        json!("18446744073709551616"),
        json!(-1),
        json!(1.5),
    ] {
        h.rejects_without_writes("marketplace_transfer_approved", transfer_args(invalid));
    }
    for field in [
        "expected_seller_id",
        "token_id",
        "approval_id",
        "receiver_id",
    ] {
        let mut args = transfer_args(json!("1"));
        args.as_object_mut().unwrap().remove(field);
        h.rejects_without_writes("marketplace_transfer_approved", args);
    }
    for (field, value) in [
        ("expected_seller_id", json!(bob())),
        ("receiver_id", json!(marketplace())),
        ("receiver_id", json!(alice())),
        ("token_id", json!("missing")),
        ("token_id", json!("x".repeat(MAX_TOKEN_ID_BYTES + 1))),
        ("approval_id", json!("2")),
    ] {
        let mut args = transfer_args(json!("1"));
        args[field] = value;
        h.rejects_without_writes("marketplace_transfer_approved", args);
    }
    h.context.predecessor_account_id = bob();
    h.context.signer_account_id = marketplace();
    h.rejects_without_writes("marketplace_transfer_approved", transfer_args(json!("1")));
    h.context.predecessor_account_id = marketplace();
    for amount in [0, 2] {
        h.context.attached_deposit = Balance::from_yoctonear(amount);
        h.rejects_without_writes("marketplace_transfer_approved", transfer_args(json!("1")));
    }
    h.context.attached_deposit = Balance::from_yoctonear(1);
    let outcome = h.call("marketplace_transfer_approved", transfer_args(json!("1")));
    assert_eq!(outcome.logs.len(), 1);
    assert_eq!(success(outcome), b"true");
    assert!(
        h.ext.action_log.is_empty(),
        "wrapper must not schedule promises"
    );
    h.rejects_without_writes("marketplace_transfer_approved", transfer_args(json!("1")));
    h.context.attached_deposit = Balance::ZERO;
    let token: Value =
        serde_json::from_slice(&success(h.call("nft_token", json!({"token_id": "sale"})))).unwrap();
    assert_eq!(token["owner_id"], "bob.testnet");
    assert_eq!(token["approved_account_ids"], json!({}));
}

#[test]
fn wasm_full_width_approval_interoperates_with_existing_numeric_views() {
    let mut contract = approved_contract();
    contract
        .tokens
        .approvals_by_id
        .as_mut()
        .unwrap()
        .insert(&"sale".into(), &HashMap::from([(marketplace(), u64::MAX)]));
    contract
        .tokens
        .next_approval_id_by_id
        .as_mut()
        .unwrap()
        .insert(&"sale".into(), &u64::MAX);
    let mut h = Harness::seeded(contract);
    h.context.attached_deposit = Balance::ZERO;
    assert_eq!(
        success(h.call(
            "nft_is_approved",
            json!({
                "token_id": "sale", "approved_account_id": marketplace(), "approval_id": u64::MAX
            })
        )),
        b"true"
    );
    let token: Value =
        serde_json::from_slice(&success(h.call("nft_token", json!({"token_id": "sale"})))).unwrap();
    assert_eq!(
        token["approved_account_ids"][marketplace().as_str()].as_u64(),
        Some(u64::MAX)
    );
    h.context.attached_deposit = Balance::from_yoctonear(1);
    h.rejects_without_writes(
        "marketplace_transfer_approved",
        transfer_args(json!((u64::MAX - 1).to_string())),
    );
    assert_eq!(
        success(h.call(
            "marketplace_transfer_approved",
            transfer_args(json!(u64::MAX.to_string()))
        )),
        b"true"
    );
}

#[test]
fn wasm_marketplace_custody_remains_ineligible_even_as_owner_caller() {
    let mut contract = approved_contract();
    next_receipt(alice(), one_yocto());
    contract.nft_transfer(marketplace(), "sale".into(), None, None);
    let mut h = Harness::seeded(contract);
    h.rejects_without_writes("marketplace_transfer_approved", transfer_args(json!("1")));
    let mut args = transfer_args(json!("1"));
    args["expected_seller_id"] = json!(marketplace());
    h.rejects_without_writes("marketplace_transfer_approved", args);
}

#[test]
fn wasm_previous_root_is_not_silently_reinitialized_or_reinterpreted() {
    testing_env!(context(alice(), NearToken::from_near(10)).build());
    let old = previous_layout_fixture();
    let source = borsh::to_vec(&old).unwrap();
    let converted = prepare_binding_v2(&source, &contract_id(), marketplace()).unwrap();
    let mut h = Harness::seeded(converted);
    let target = h.ext.fake_trie[b"STATE".as_slice()].clone();
    h.ext.fake_trie.insert(b"STATE".to_vec(), source);
    h.context.predecessor_account_id = contract_id();
    h.context.attached_deposit = Balance::ZERO;
    h.rejects_without_writes("new_with_marketplace", init_args());
    h.rejects_without_writes("marketplace_id", json!({}));
    // Fixture installs the independently tested converted root, not a deployed
    // migration. Every non-root key is preserved verbatim.
    let before = h.ext.fake_trie.clone();
    h.ext.fake_trie.insert(b"STATE".to_vec(), target);
    assert_eq!(
        success(h.call("marketplace_id", json!({}))),
        b"\"market.testnet\""
    );
    for (key, value) in before {
        if key != b"STATE" {
            assert_eq!(h.ext.fake_trie[&key], value);
        }
    }
}
