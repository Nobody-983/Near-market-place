//! Execute the built artifact with NEAR's Wasm VM. MockedExternal is NOT a node
//! trie/receipt transaction and does not roll back. Tests deliberately expose
//! that limitation; a node/sandbox rollback acceptance test remains required.

use std::{collections::BTreeSet, path::PathBuf, sync::OnceLock};

use near_marketplace_foundation::persistence::{
    bootstrap_batch, decode_record, ArtifactDomain, AtomicStore, DeploymentDomain, MemoryStore,
    RecordKey, Repository, MAX_RECORD_BYTES,
};
use near_marketplace_runtime_prototype::BOOTSTRAP_KEY;
use near_parameters::RuntimeConfigStore;
use near_primitives_core::{
    account::AccountContract,
    config::ViewConfig,
    hash::CryptoHash,
    types::{Balance, Gas},
    version::PROTOCOL_VERSION,
};
use near_sdk::{
    json_types::Base64VecU8,
    serde_json::{self, json},
};
use near_vm_runner::{
    logic::{
        mocks::mock_external::MockedExternal, types::PromiseResult, ReturnData, VMContext,
        VMOutcome,
    },
    ContractCode, MockContractRuntimeCache,
};

struct Harness {
    ext: MockedExternal,
    context: VMContext,
    domain: DeploymentDomain,
    cache: MockContractRuntimeCache,
}

fn wasm(mode: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "target/{mode}/wasm32-unknown-unknown/release/near_marketplace_runtime_prototype.wasm"
    ));
    std::fs::read(&path).unwrap_or_else(|e| panic!("Build artifact first: {}: {e}", path.display()))
}

impl Harness {
    fn new(mode: &str) -> Self {
        let code = ContractCode::new(wasm(mode), None);
        let hash = *code.hash();
        let domain = DeploymentDomain {
            environment: if mode == "production" {
                ArtifactDomain::Production
            } else {
                ArtifactDomain::Testnet
            },
            genesis: [1; 32],
            manifest: [2; 32],
            marketplace_code: hash.0,
            nft_code: [4; 32],
            marketplace: "market.testnet".parse().unwrap(),
            nft: "nft.testnet".parse().unwrap(),
        };
        let context = VMContext {
            current_account_id: domain.marketplace.clone(),
            signer_account_id: domain.marketplace.clone(),
            signer_account_pk: vec![0; 33],
            predecessor_account_id: domain.marketplace.clone(),
            refund_to_account_id: domain.marketplace.clone(),
            input: [].into(),
            promise_results: [].into(),
            block_height: 10,
            block_timestamp: 42,
            epoch_height: 1,
            account_balance: Balance::from_near(100),
            account_locked_balance: Balance::ZERO,
            storage_usage: code.code().len() as u64,
            account_contract: AccountContract::Local(hash),
            attached_deposit: Balance::ZERO,
            prepaid_gas: Gas::from_teragas(300),
            random_seed: vec![0; 32],
            view_config: None,
            output_data_receivers: vec![],
        };
        Self {
            ext: MockedExternal::with_code(code),
            context,
            domain,
            cache: shared_cache(),
        }
    }

    fn call(&mut self, method: &str, input: serde_json::Value) -> VMOutcome {
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
        let outcome =
            near_vm_runner::run(prepared, &mut self.ext, &context, config.fees.clone()).unwrap();
        assert!(
            self.ext.action_log.is_empty(),
            "prototype must not schedule actions"
        );
        // MockedExternal writes immediately, even on abort. Deliberately do not
        // hide this behind a simulated transaction/rollback wrapper.
        if outcome.aborted.is_none() {
            self.context.storage_usage = outcome.storage_usage;
        }
        outcome
    }

    fn args(&self) -> serde_json::Value {
        manifest_args(&borsh::to_vec(&self.domain).unwrap())
    }

    fn bootstrap(&mut self) {
        let args = self.args();
        success(self.call("bootstrap", args));
    }

    fn seed_existing(&mut self) {
        let store = Repository::<MemoryStore>::new_offline(self.domain.clone(), None)
            .unwrap()
            .into_store();
        self.ext
            .fake_trie
            .insert(b"STATE".to_vec(), store.read(b"STATE").unwrap().unwrap());
        let mut sidecar = b"NMPBOOT1".to_vec();
        sidecar.extend(borsh::to_vec(&self.domain).unwrap());
        self.ext.fake_trie.insert(BOOTSTRAP_KEY.to_vec(), sidecar);
    }
}

fn shared_cache() -> MockContractRuntimeCache {
    static CACHE: OnceLock<MockContractRuntimeCache> = OnceLock::new();
    CACHE.get_or_init(MockContractRuntimeCache::default).clone()
}

fn manifest_args(bytes: &[u8]) -> serde_json::Value {
    json!({ "manifest": Base64VecU8(bytes.to_vec()) })
}
fn success(outcome: VMOutcome) -> VMOutcome {
    assert!(
        outcome.aborted.is_none(),
        "unexpected VM failure: {:?}",
        outcome.aborted
    );
    outcome
}
fn failure(outcome: VMOutcome, message: &str) {
    let error = outcome
        .aborted
        .expect("operation must abort, not return an ordinary error")
        .to_string();
    assert!(error.contains(message), "expected {message:?}, got {error}");
}

#[test]
fn compiled_exports_keep_both_trading_gates_closed() {
    for mode in ["testnet", "production", "testnet,test-hooks"] {
        let bytes = wasm(mode);
        wasmparser::Validator::new().validate_all(&bytes).unwrap();
        let mut exports = BTreeSet::new();
        let mut imports = BTreeSet::new();
        for payload in wasmparser::Parser::new(0).parse_all(&bytes) {
            match payload.unwrap() {
                wasmparser::Payload::ExportSection(section) => {
                    for export in section {
                        let export = export.unwrap();
                        if export.kind == wasmparser::ExternalKind::Func {
                            exports.insert(export.name.to_owned());
                        }
                    }
                }
                wasmparser::Payload::ImportSection(section) => {
                    for import in section.into_imports() {
                        let import = import.unwrap();
                        assert_eq!(import.module, "env");
                        imports.insert(import.name.to_owned());
                    }
                }
                _ => {}
            }
        }
        for required in [
            "storage_read",
            "storage_write",
            "storage_has_key",
            "register_len",
            "read_register",
            "panic_utf8",
            "sha256",
            "current_contract_code",
        ] {
            assert!(
                imports.contains(required),
                "missing real NEAR host binding {required} in {mode}"
            );
        }
        // The SDK adds a receiverless, read-only NEP-330 metadata export.
        let mut expected: BTreeSet<String> = [
            "bootstrap",
            "bootstrap_with_admission_owner",
            "create_nft_admission",
            "dispatch_nft_admission",
            "release_nft_admission",
            "storage_status",
            "contract_source_metadata",
            "nft_on_approve",
            "on_nft_verified",
        ]
        .map(str::to_owned)
        .into();
        if mode.starts_with("testnet") {
            expected.insert("fund_testnet_storage".into());
        }
        if mode.contains("test-hooks") {
            expected.extend(
                [
                    "test_abort_bootstrap",
                    "test_abort_after_existing_write",
                    "test_conflicting_write_set",
                    "test_metered_storage",
                ]
                .map(str::to_owned),
            );
        }
        assert_eq!(exports, expected, "unexpected ABI in {mode}");
    }
}

#[test]
fn wasm_bootstrap_matches_offline_v1_and_receiverless_status_preserves_all_bytes() {
    for mode in ["testnet", "production"] {
        let mut h = Harness::new(mode);
        h.bootstrap();
        let batch = bootstrap_batch(&h.domain).unwrap();
        let expected = batch.writes().next().unwrap().1;
        assert_eq!(h.ext.fake_trie.get(b"STATE".as_slice()).unwrap(), expected);
        assert_eq!(
            decode_record(expected, &h.domain, &RecordKey::Root)
                .unwrap()
                .revision(),
            1
        );
        let before = h.ext.fake_trie.clone();
        let outcome = success(h.call("storage_status", json!({})));
        let ReturnData::Value(bytes) = outcome.return_data else {
            panic!("missing JSON status")
        };
        let status: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(status["trading_enabled"], false);
        assert_eq!(status["root_revision"], "1");
        assert_eq!(h.ext.fake_trie, before);
        h.context.view_config = Some(ViewConfig {
            max_gas_burnt: Gas::from_teragas(300),
        });
        success(h.call("storage_status", json!({})));
        assert_eq!(h.ext.fake_trie, before);
        println!(
            "{mode}: protocol {PROTOCOL_VERSION}, exact v1 root {} bytes; status gas {}",
            expected.len(),
            outcome.burnt_gas
        );
    }
}

#[test]
fn existing_v1_root_is_read_without_sdk_state_deserialization_or_replacement() {
    let mut h = Harness::new("testnet");
    h.seed_existing();
    h.ext
        .fake_trie
        .insert(b"unrelated:legacy".to_vec(), vec![7; 33]);
    let before = h.ext.fake_trie.clone();
    success(h.call("storage_status", json!({})));
    failure(h.call("bootstrap", h.args()), "Already initialized");
    assert_eq!(h.ext.fake_trie, before);
}

#[test]
fn initialization_checks_runtime_predecessor_signer_deposit_and_callback_context() {
    for case in 0..4 {
        let mut h = Harness::new("testnet");
        match case {
            0 => h.context.predecessor_account_id = "attacker.testnet".parse().unwrap(),
            1 => h.context.signer_account_id = "attacker.testnet".parse().unwrap(),
            2 => h.context.attached_deposit = Balance::from_yoctonear(1),
            3 => h.context.promise_results = vec![PromiseResult::Successful(vec![].into())].into(),
            _ => unreachable!(),
        }
        let outcome = h.call("bootstrap", h.args());
        assert!(outcome.aborted.is_some(), "case {case} must abort");
        assert!(h.ext.fake_trie.is_empty());
    }
}

#[test]
fn rejects_wrong_domain_code_and_cross_environment_manifest() {
    for mode in ["testnet", "production"] {
        for case in 0..6 {
            let mut h = Harness::new(mode);
            match case {
                0 => {
                    h.domain.environment = if mode == "testnet" {
                        ArtifactDomain::Production
                    } else {
                        ArtifactDomain::Testnet
                    }
                }
                1 => h.domain.marketplace = "other.testnet".parse().unwrap(),
                2 => h.domain.marketplace_code = [9; 32],
                3 => h.domain.genesis = [0; 32],
                4 => h.domain.nft = h.domain.marketplace.clone(),
                5 => h.context.account_contract = AccountContract::Local(CryptoHash([9; 32])),
                _ => unreachable!(),
            }
            assert!(
                h.call("bootstrap", h.args()).aborted.is_some(),
                "{mode}: {case}"
            );
            assert!(h.ext.fake_trie.is_empty());
        }
    }
}

#[test]
fn malformed_or_unbounded_manifest_never_writes_state() {
    let valid = borsh::to_vec(&Harness::new("testnet").domain).unwrap();
    let mut huge_length = valid.clone();
    huge_length[129..133].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut trailing = valid.clone();
    trailing.push(0);
    let mut bad_tag = valid.clone();
    bad_tag[0] = 255;
    for bytes in [
        vec![],
        valid[..130].to_vec(),
        huge_length,
        trailing,
        bad_tag,
        vec![0; 266],
    ] {
        let mut h = Harness::new("testnet");
        assert!(h.call("bootstrap", manifest_args(&bytes)).aborted.is_some());
        assert!(h.ext.fake_trie.is_empty());
    }
}

#[test]
fn existing_corrupt_oversized_and_partial_roots_are_never_reinitialized() {
    let mut partial = Harness::new("testnet");
    partial.ext.fake_trie.insert(
        near_marketplace_foundation::persistence::STORAGE_ROOT_KEY.to_vec(),
        vec![],
    );
    let before = partial.ext.fake_trie.clone();
    failure(
        partial.call("bootstrap", partial.args()),
        "Already initialized",
    );
    assert_eq!(partial.ext.fake_trie, before);
    for bytes in [
        vec![],
        b"legacy root".to_vec(),
        vec![5; MAX_RECORD_BYTES + 1],
    ] {
        let mut h = Harness::new("testnet");
        h.ext.fake_trie.insert(b"STATE".to_vec(), bytes);
        let before = h.ext.fake_trie.clone();
        failure(h.call("bootstrap", h.args()), "Already initialized");
        assert_eq!(h.ext.fake_trie, before);
    }
    let mut h = Harness::new("testnet");
    h.ext.fake_trie.insert(BOOTSTRAP_KEY.to_vec(), vec![]);
    let before = h.ext.fake_trie.clone();
    failure(h.call("bootstrap", h.args()), "Already initialized");
    failure(
        h.call("storage_status", json!({})),
        "Invalid bootstrap schema",
    );
    assert_eq!(h.ext.fake_trie, before);
}

#[test]
fn corrupt_or_oversized_host_values_abort_before_unbounded_guest_decode() {
    for (key, bytes, message) in [
        (b"STATE".as_slice(), vec![0; 100], "Invalid root envelope"),
        (
            b"STATE".as_slice(),
            vec![0; MAX_RECORD_BYTES],
            "Invalid root envelope",
        ),
        (
            b"STATE".as_slice(),
            vec![0; MAX_RECORD_BYTES + 1],
            "Stored value exceeds bound",
        ),
        (
            BOOTSTRAP_KEY,
            vec![0; MAX_RECORD_BYTES + 1],
            "Stored value exceeds bound",
        ),
    ] {
        let mut h = Harness::new("testnet");
        h.seed_existing();
        h.ext.fake_trie.insert(key.to_vec(), bytes);
        let before = h.ext.fake_trie.clone();
        failure(h.call("storage_status", json!({})), message);
        assert_eq!(h.ext.fake_trie, before);
    }
}

#[test]
fn failures_after_each_bootstrap_write_abort_but_mock_is_not_receipt_rollback() {
    for stage in 1..=3 {
        let mut h = Harness::new("testnet,test-hooks");
        let mut args = h.args();
        args["stage"] = json!(stage);
        failure(
            h.call("test_abort_bootstrap", args),
            "Injected bootstrap abort",
        );
        // Demonstrates that the VM executed the write then aborted. The mock
        // retains it: this is explicitly NOT a passing receipt rollback test.
        assert!(h.ext.fake_trie.contains_key(b"STATE".as_slice()));
        assert_eq!(h.ext.fake_trie.contains_key(BOOTSTRAP_KEY), stage >= 2);
        assert_eq!(
            h.ext
                .fake_trie
                .contains_key(near_marketplace_foundation::persistence::STORAGE_ROOT_KEY),
            stage == 3
        );
    }
    let mut h = Harness::new("testnet,test-hooks");
    h.bootstrap();
    failure(
        h.call("test_abort_after_existing_write", json!({})),
        "Injected post-write abort",
    );
    assert!(h
        .ext
        .fake_trie
        .contains_key(b"nmp:runtime:test:rollback".as_slice()));
}

#[test]
fn insufficient_storage_backing_aborts_after_writes_not_an_ordinary_error() {
    let mut h = Harness::new("testnet");
    h.context.account_balance = Balance::ZERO;
    failure(h.call("bootstrap", h.args()), "Unfunded bootstrap storage");
    assert!(
        h.ext.fake_trie.contains_key(b"STATE".as_slice()),
        "must reach post-write backing check"
    );
}

#[test]
fn backend_precondition_failure_aborts_without_partial_writes() {
    let mut h = Harness::new("testnet,test-hooks");
    h.bootstrap();
    let before = h.ext.fake_trie.clone();
    failure(
        h.call("test_conflicting_write_set", json!({})),
        "Atomic precondition failed",
    );
    assert_eq!(h.ext.fake_trie, before);
}

#[test]
fn actual_wasm_storage_growth_is_funded_and_underfunded_charge_aborts() {
    let mut h = Harness::new("testnet,test-hooks");
    h.bootstrap();
    let output = success(h.call("test_metered_storage", json!({"underfunded":false})));
    let ReturnData::Value(bytes) = output.return_data else {
        panic!("Missing metered amount")
    };
    let amount: near_sdk::json_types::U128 = serde_json::from_slice(&bytes).unwrap();
    assert!(amount.0 > 0);
    println!("Wasm measured storage expenditure: {} yoctoNEAR", amount.0);
    let mut h = Harness::new("testnet,test-hooks");
    h.bootstrap();
    failure(
        h.call("test_metered_storage", json!({"underfunded":true})),
        "Unfunded storage expenditure",
    );
    // MockedExternal retains the payload writes on abort. This proves the
    // failure reached the post-write check, NOT that a node rolled it back.
    assert!(h.ext.fake_trie.keys().any(|k| k.starts_with(b"nmp:r:")));
}
