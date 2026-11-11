//! Actual generated Wasm entry points, using synthetic trie/dependency fixtures.
//! MockedExternal does not implement receipt rollback or real cross-contract calls.
use super::lifecycle_tests::{candidate, funded_setup, sponsor, FUNDS};
use super::*;
use near_parameters::RuntimeConfigStore;
use near_primitives_core::{
    account::AccountContract as RuntimeContract,
    types::{Balance, Gas as RuntimeGas},
    version::PROTOCOL_VERSION,
};
use near_vm_runner::{
    logic::{
        mocks::mock_external::MockedExternal, types::PromiseResult as HostResult, ReturnData,
        VMContext, VMOutcome,
    },
    ContractCode, MockContractRuntimeCache,
};
use std::{path::PathBuf, sync::OnceLock};

struct Harness {
    ext: MockedExternal,
    ctx: VMContext,
    domain: DeploymentDomain,
    cache: MockContractRuntimeCache,
}
impl Harness {
    fn new() -> Self {
        let mode = if cfg!(feature = "production") {
            "production"
        } else if cfg!(feature = "test-hooks") {
            "testnet,test-hooks"
        } else {
            "testnet"
        };
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            "target/{mode}/wasm32-unknown-unknown/release/near_marketplace_runtime_prototype.wasm"
        ));
        let code = ContractCode::new(std::fs::read(path).expect("Build runtime Wasm first"), None);
        let hash = *code.hash();
        let mut d = domain();
        d.marketplace_code = hash.0;
        let ctx = VMContext {
            current_account_id: d.marketplace.clone(),
            signer_account_id: account("signer.testnet"),
            signer_account_pk: vec![0; 33],
            predecessor_account_id: d.marketplace.clone(),
            refund_to_account_id: d.marketplace.clone(),
            input: vec![].into(),
            promise_results: vec![].into(),
            block_height: 1,
            block_timestamp: 20,
            epoch_height: 1,
            account_balance: Balance::from_near(100),
            account_locked_balance: Balance::ZERO,
            storage_usage: code.code().len() as u64,
            account_contract: RuntimeContract::Local(hash),
            attached_deposit: Balance::ZERO,
            prepaid_gas: RuntimeGas::from_teragas(300),
            random_seed: vec![0; 32],
            view_config: None,
            output_data_receivers: vec![],
        };
        static CACHE: OnceLock<MockContractRuntimeCache> = OnceLock::new();
        Self {
            ext: MockedExternal::with_code(code),
            ctx,
            domain: d,
            cache: CACHE.get_or_init(MockContractRuntimeCache::default).clone(),
        }
    }
    fn pending() -> (Self, Admission, PendingVerification) {
        let mut h = Self::new();
        setup_domain(&h.domain);
        let a = provision(
            &h.domain,
            terms(&h.domain, u64::MAX),
            NearToken::from_near(1).as_yoctonear(),
        );
        let p = start(&h.domain, &a);
        h.take_native_storage();
        (h, a, p)
    }
    fn take_native_storage(&mut self) {
        self.ctx.storage_usage += env::storage_usage();
        self.ext.fake_trie = near_sdk::mock::with_mocked_blockchain(|b| b.take_storage());
    }
    fn results(&mut self, a: &Admission) {
        self.ctx.promise_results = vec![
            HostResult::Successful(
                token_result(&self.domain, a.operation.verification().unwrap().1).into(),
            ),
            HostResult::Successful(b"true".to_vec().into()),
        ]
        .into();
    }
    fn call(&mut self, method: &str, args: serde_json::Value) -> VMOutcome {
        let actions_before = self.ext.action_log.len();
        let config = RuntimeConfigStore::new(None)
            .get_config(PROTOCOL_VERSION)
            .clone();
        let mut ctx = self.ctx.clone();
        ctx.input = serde_json::to_vec(&args).unwrap().into();
        let prepared = near_vm_runner::prepare(
            &self.ext,
            config.wasm_config.clone(),
            Some(&self.cache),
            ctx.make_gas_counter(&config.wasm_config),
            method,
        );
        let result =
            near_vm_runner::run(prepared, &mut self.ext, &ctx, config.fees.clone()).unwrap();
        if result.aborted.is_none() {
            self.ctx.storage_usage = result.storage_usage;
        }
        if method != "dispatch_nft_admission" {
            assert_eq!(
                self.ext.action_log.len(),
                actions_before,
                "gated intake and callbacks must not send actions"
            );
        }
        result
    }
    fn reject(&mut self, method: &str, args: serde_json::Value) {
        let actions_before = self.ext.action_log.len();
        let before = self.ext.fake_trie.clone();
        let r = self.call(method, args);
        assert!(r.aborted.is_some(), "must abort");
        assert_eq!(
            before, self.ext.fake_trie,
            "preflight rejection must not write"
        );
        assert_eq!(
            self.ext.action_log.len(),
            actions_before,
            "rejection must not dispatch"
        );
    }
}

#[path = "dispatch_wasm_tests.rs"]
mod dispatch_wasm;

#[cfg(feature = "testnet")]
#[test]
fn wasm_funding_abi_exact_credit_replay_protection_and_owner_only() {
    let mut h = Harness::new();
    setup_domain_with_owner(&h.domain, Some(sponsor()));
    h.take_native_storage();
    h.ctx.predecessor_account_id = account("attacker.testnet");
    h.ctx.attached_deposit = Balance::from_near(2);
    h.reject(
        "fund_testnet_storage",
        json!({"funding_nonce":"18446744073709551615"}),
    );
    h.ctx.predecessor_account_id = sponsor();
    for bad in [
        json!({"funding_nonce":1}),
        json!({"funding_nonce":"1","owner_id":"attacker.testnet"}),
        json!({"funding_nonce":"18446744073709551616"}),
    ] {
        h.reject("fund_testnet_storage", bad);
    }
    h.ctx.promise_results = vec![HostResult::Successful(b"true".to_vec().into())].into();
    h.reject("fund_testnet_storage", json!({"funding_nonce":"1"}));
    h.ctx.promise_results = vec![].into();
    let before = h.ctx.storage_usage;
    let status: serde_json::Value = serde_json::from_slice(&result(h.call(
        "fund_testnet_storage",
        json!({"funding_nonce":"18446744073709551615"}),
    )))
    .unwrap();
    let received = status["received"]
        .as_str()
        .unwrap()
        .parse::<u128>()
        .unwrap();
    let cost = status["storage_charged"]
        .as_str()
        .unwrap()
        .parse::<u128>()
        .unwrap();
    assert_eq!(received, 2 * FUNDS);
    assert_eq!(status["available"], json!((received - cost).to_string()));
    assert_eq!(
        cost,
        u128::from(h.ctx.storage_usage - before) * env::storage_byte_cost().as_yoctonear()
    );
    assert!(cost > 0);
    h.reject(
        "fund_testnet_storage",
        json!({"funding_nonce":"18446744073709551615"}),
    );
    h.ctx.attached_deposit = Balance::ZERO;
    h.reject("fund_testnet_storage", json!({"funding_nonce":"2"}));
    // Reopen actual Wasm-written records. Funding makes ordinary admission
    // creation possible without injecting any synthetic credit into the trie.
    near_sdk::mock::with_mocked_blockchain(|b| {
        b.take_storage();
    });
    for (key, value) in &h.ext.fake_trie {
        env::storage_write(key, value);
    }
    let a = candidate(&h.domain, terms(&h.domain, 1));
    result(h.call("create_nft_admission", admission_args(&a)));
    let locator = super::dispatch_tests::locator(&h.domain, &a);
    result(h.call(
        "dispatch_nft_admission",
        json!({"admission_id":near_sdk::json_types::Base64VecU8(locator)}),
    ));
    h.ctx.predecessor_account_id = h.domain.marketplace.clone();
    h.results(&a);
    assert_eq!(
        result(h.call(
            "on_nft_verified",
            json!({"generation":"1","attempt_nonce":"1"})
        )),
        b"\"active\""
    );
    // Dependency results above are synthetic; this is not a live NFT flow.
}

#[cfg(feature = "testnet")]
#[test]
fn wasm_underfunded_metadata_aborts_and_never_reports_credit_success() {
    let mut h = Harness::new();
    setup_domain_with_owner(&h.domain, Some(sponsor()));
    h.take_native_storage();
    h.ctx.predecessor_account_id = sponsor();
    h.ctx.attached_deposit = Balance::from_yoctonear(1);
    assert!(h
        .call("fund_testnet_storage", json!({"funding_nonce":"1"}))
        .aborted
        .is_some());
    // MockedExternal keeps interim writes; only a node can verify receipt rollback.
}

#[cfg(feature = "production")]
#[test]
fn wasm_production_has_no_funding_export() {
    let mut h = Harness::new();
    h.reject("fund_testnet_storage", json!({"funding_nonce":"1"}));
}

#[test]
fn wasm_unverified_maximum_release_and_invalid_results_do_not_poison_real_approval() {
    for invalid in [false, true] {
        let (mut h, a) = admission_fixture();
        result(h.call("create_nft_admission", admission_args(&a)));
        if invalid {
            let locator = super::dispatch_tests::locator(&h.domain, &a);
            result(h.call(
                "dispatch_nft_admission",
                json!({"admission_id":near_sdk::json_types::Base64VecU8(locator)}),
            ));
            h.ctx.predecessor_account_id = h.domain.marketplace.clone();
            h.ctx.promise_results = vec![HostResult::Failed, HostResult::Failed].into();
            assert_eq!(
                result(h.call(
                    "on_nft_verified",
                    json!({"generation":"1","attempt_nonce":"1"})
                )),
                b"\"invalid\""
            );
        } else {
            result(h.call("release_nft_admission", release_args(&a)));
        }
        h.ctx.predecessor_account_id = sponsor();
        h.ctx.promise_results = vec![].into();
        // Exact replay returns the retained terminal status.
        result(h.call("create_nft_admission", admission_args(&a)));
        // Reopen the actual Wasm-produced trie to prepare generation two.
        near_sdk::mock::with_mocked_blockchain(|b| {
            b.take_storage();
        });
        for (key, bytes) in &h.ext.fake_trie {
            env::storage_write(key, bytes);
        }
        let next = lifecycle_tests::candidate(&h.domain, terms(&h.domain, 1));
        result(h.call("create_nft_admission", admission_args(&next)));
        let before = h.ext.fake_trie.clone();
        h.ctx.predecessor_account_id = h.domain.marketplace.clone();
        h.results(&a);
        assert_eq!(
            result(h.call(
                "on_nft_verified",
                json!({"generation":"1","attempt_nonce":"1"})
            )),
            b"\"ignored\""
        );
        assert_eq!(h.ext.fake_trie, before);
    }
}
fn result(r: VMOutcome) -> Vec<u8> {
    assert!(r.aborted.is_none(), "{:?}", r.aborted);
    match r.return_data {
        ReturnData::Value(b) => b,
        other => panic!("unexpected {other:?}"),
    }
}
fn args(p: &PendingVerification) -> serde_json::Value {
    json!({"generation":p.generation.0.to_string(),"attempt_nonce":p.attempt_nonce.0.to_string()})
}

fn admission_args(a: &Admission) -> serde_json::Value {
    json!({
        "operation": near_sdk::json_types::Base64VecU8(a.operation.canonical_bytes().unwrap()),
        "storage_owner_id": a.storage_owner,
        "reserve_amount": FUNDS.to_string(),
        "creation_bytes": a.creation_bytes.to_string(),
        "local_gas": a.local_gas.to_string(),
    })
}
fn release_args(a: &Admission) -> serde_json::Value {
    let (g, t) = a.operation.verification().unwrap();
    json!({"token_id":t.token.token(), "owner_id":t.seller, "approval_id":t.approval_id.to_string(), "generation":g.value().to_string()})
}
fn admission_fixture() -> (Harness, Admission) {
    let mut h = Harness::new();
    funded_setup(&h.domain);
    let a = candidate(&h.domain, terms(&h.domain, u64::MAX));
    h.take_native_storage();
    h.ctx.predecessor_account_id = sponsor();
    (h, a)
}

#[test]
fn wasm_owner_bootstrap_is_explicit_immutable_and_preserves_v1_root() {
    let mut h = Harness::new();
    h.ctx.signer_account_id = h.domain.marketplace.clone();
    let manifest = near_sdk::json_types::Base64VecU8(borsh::to_vec(&h.domain).unwrap());
    let args = json!({"manifest":manifest, "admission_owner_id":sponsor()});
    h.ctx.predecessor_account_id = account("attacker.testnet");
    h.reject("bootstrap_with_admission_owner", args.clone());
    h.ctx.predecessor_account_id = h.domain.marketplace.clone();
    let mut invalid = args.clone();
    invalid["admission_owner_id"] = json!(h.domain.marketplace);
    h.reject("bootstrap_with_admission_owner", invalid);
    assert!(h
        .call("bootstrap_with_admission_owner", args.clone())
        .aborted
        .is_none());
    let expected = near_marketplace_foundation::persistence::bootstrap_batch(&h.domain)
        .unwrap()
        .writes()
        .next()
        .unwrap()
        .1
        .to_vec();
    assert_eq!(h.ext.fake_trie[b"STATE".as_slice()], expected);
    h.reject("bootstrap_with_admission_owner", args);
    h.reject("bootstrap", json!({"manifest":manifest}));
}

#[test]
fn wasm_admission_authentication_identity_deposits_and_storage_exhaustion_reject_without_writes() {
    let (mut h, a) = admission_fixture();
    let good = admission_args(&a);
    h.ctx.predecessor_account_id = account("attacker.testnet");
    h.ctx.signer_account_id = sponsor();
    h.reject("create_nft_admission", good.clone());
    h.ctx.predecessor_account_id = sponsor();
    let mut wrong_owner = good.clone();
    wrong_owner["storage_owner_id"] = json!("seller.testnet");
    h.reject("create_nft_admission", wrong_owner);
    h.ctx.attached_deposit = Balance::from_yoctonear(1);
    h.reject("create_nft_admission", good.clone());
    h.ctx.attached_deposit = Balance::ZERO;
    h.ctx.promise_results = vec![HostResult::Successful(b"true".to_vec().into())].into();
    h.reject("create_nft_admission", good.clone());
    h.ctx.promise_results = vec![].into();
    for amount in [1, 6 * FUNDS] {
        let mut exhausted = good.clone();
        exhausted["reserve_amount"] = json!(amount.to_string());
        h.reject("create_nft_admission", exhausted);
    }
    let mut corrupt = good.clone();
    corrupt["operation"] = json!(near_sdk::json_types::Base64VecU8(vec![0; 4097]));
    h.reject("create_nft_admission", corrupt);
    let saved = h.ext.fake_trie.remove(authority::OWNER_KEY).unwrap();
    h.reject("create_nft_admission", good.clone());
    h.ext.fake_trie.insert(authority::OWNER_KEY.to_vec(), saved);
    h.ctx.account_balance = Balance::from_yoctonear(1);
    h.reject("create_nft_admission", good);
}

#[test]
fn wasm_admission_creation_release_restart_duplicates_and_stale_callback_are_safe() {
    let (mut h, a) = admission_fixture();
    let create = admission_args(&a);
    let usage = h.ctx.storage_usage;
    let ready = result(h.call("create_nft_admission", create.clone()));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&ready).unwrap(),
        json!({"generation":"1","attempt_nonce":"1","state":"ready"})
    );
    assert!(h.ctx.storage_usage > usage);
    let before = h.ext.fake_trie.clone();
    assert_eq!(
        result(h.call("create_nft_admission", create.clone())),
        ready
    );
    assert_eq!(h.ext.fake_trie, before);
    let mut wrong = create.clone();
    wrong["reserve_amount"] = json!((FUNDS + 1).to_string());
    h.reject("create_nft_admission", wrong);
    let mut wrong_release = release_args(&a);
    wrong_release["generation"] = json!("2");
    h.reject("release_nft_admission", wrong_release);
    h.ctx.predecessor_account_id = h.domain.marketplace.clone();
    h.results(&a);
    h.reject(
        "on_nft_verified",
        json!({"generation":"1","attempt_nonce":"1"}),
    );
    h.ctx.predecessor_account_id = sponsor();
    h.ctx.promise_results = vec![].into();
    let usage = h.ctx.storage_usage;
    let released = result(h.call("release_nft_admission", release_args(&a)));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&released).unwrap()["state"],
        "released"
    );
    assert_eq!(h.ctx.storage_usage, usage);
    let before = h.ext.fake_trie.clone();
    assert_eq!(
        result(h.call("release_nft_admission", release_args(&a))),
        released
    );
    assert_eq!(result(h.call("create_nft_admission", create)), released);
    assert_eq!(h.ext.fake_trie, before);
    // Rebuild the VM harness with only persisted bytes/context, no invocation
    // object retained. This does not emulate a blockchain/node restart.
    let mut restarted = Harness::new();
    restarted.ext.fake_trie = h.ext.fake_trie.clone();
    restarted.ctx = h.ctx;
    restarted.ctx.predecessor_account_id = restarted.domain.marketplace.clone();
    restarted.results(&a);
    assert_eq!(
        result(restarted.call(
            "on_nft_verified",
            json!({"generation":"1","attempt_nonce":"1"})
        )),
        b"\"ignored\""
    );
    assert_eq!(restarted.ext.fake_trie, before);
}

#[test]
fn wasm_concurrent_admissions_reject_stale_generation_without_overwriting() {
    let mut h = Harness::new();
    funded_setup(&h.domain);
    let first = candidate(&h.domain, terms(&h.domain, 1));
    let mut t = terms(&h.domain, 2);
    t.token = TokenKey::new(h.domain.nft.clone(), "other".into()).unwrap();
    let stale = candidate(&h.domain, t);
    h.take_native_storage();
    h.ctx.predecessor_account_id = sponsor();
    result(h.call("create_nft_admission", admission_args(&first)));
    h.reject("create_nft_admission", admission_args(&stale));
    h.ctx.predecessor_account_id = account("attacker.testnet");
    h.reject("release_nft_admission", release_args(&first));
}

#[test]
fn wasm_pending_unknown_callback_and_legacy_admissions_cannot_release() {
    let mut h = Harness::new();
    funded_setup(&h.domain);
    let a = candidate(&h.domain, terms(&h.domain, 1));
    lifecycle_tests::create(&h.domain, &a).unwrap();
    start(&h.domain, &a); // synthetic native dispatch; no runtime dispatch export
    h.take_native_storage();
    h.ctx.predecessor_account_id = sponsor();
    h.reject("release_nft_admission", release_args(&a));
    h.ctx.predecessor_account_id = h.domain.marketplace.clone();
    h.reject(
        "on_nft_verified",
        json!({"generation":"1","attempt_nonce":"1"}),
    );
    h.ctx.predecessor_account_id = sponsor();
    h.ctx.block_timestamp = 5000;
    h.reject("release_nft_admission", release_args(&a));
    h.ctx.predecessor_account_id = h.domain.marketplace.clone();
    h.ctx.block_timestamp = 20;
    h.results(&a);
    result(h.call(
        "on_nft_verified",
        json!({"generation":"1","attempt_nonce":"1"}),
    ));
    h.ctx.predecessor_account_id = sponsor();
    h.ctx.promise_results = vec![].into();
    h.reject("release_nft_admission", release_args(&a));
    let (mut legacy, a, _) = Harness::pending();
    legacy.ctx.predecessor_account_id = sponsor();
    legacy.reject("release_nft_admission", release_args(&a));
}

#[test]
fn wasm_post_write_admission_exhaustion_aborts_instead_of_returning_success() {
    let (mut h, a) = admission_fixture();
    let mut input = admission_args(&a);
    input["creation_bytes"] = json!("1");
    let before = h.ext.fake_trie.clone();
    let r = h.call("create_nft_admission", input);
    assert!(r.aborted.is_some());
    assert!(format!("{:?}", r.aborted).contains("Admission storage exceeds authorized budget"));
    // Explicitly demonstrate the harness limitation: writes remain on abort.
    // Actual atomic rollback across these batches requires a NEAR node test.
    assert_ne!(h.ext.fake_trie, before);
}

#[test]
fn wasm_unused_release_preserves_claims_unknown_sale_unattributed_credit_and_holds() {
    let mut h = Harness::new();
    let (a, keys) = lifecycle_tests::protected_fixture(&h.domain);
    let before = StorageRepository::open(HostStore, h.domain.clone())
        .unwrap()
        .snapshot()
        .unwrap();
    h.take_native_storage();
    h.ctx.predecessor_account_id = sponsor();
    let records: Vec<_> = keys
        .iter()
        .map(|key| {
            let key = key.storage_key(&h.domain).unwrap();
            let bytes = h.ext.fake_trie[&key].clone();
            (key, bytes)
        })
        .collect();
    let output = result(h.call("release_nft_admission", release_args(&a)));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output).unwrap()["state"],
        "released"
    );
    for (key, bytes) in records {
        assert_eq!(h.ext.fake_trie[&key], bytes);
    }
    // Open the Wasm output with the independent byte repository to compare
    // maintained accounting. All balances/results remain synthetic fixtures.
    for (key, value) in &h.ext.fake_trie {
        env::storage_write(key, value);
    }
    let after = StorageRepository::open(HostStore, h.domain.clone())
        .unwrap()
        .snapshot()
        .unwrap();
    assert_eq!(after.0, before.0);
    assert_eq!(after.1.unattributed(), before.1.unattributed());
    assert!(after.1.financial_hold());
    assert_eq!(after.1.totals().staked, before.1.totals().staked);
    assert_eq!(
        after.1.totals().protected_liquid(),
        before.1.totals().protected_liquid()
    );
}

#[test]
fn wasm_hook_authentication_and_gates_cannot_be_bypassed() {
    let (mut h, a, _) = Harness::pending();
    let (_, t) = a.operation.verification().unwrap();
    let input =
        json!({"token_id":"art","owner_id":t.seller,"approval_id":u64::MAX,"msg":message(t)});
    h.ctx.predecessor_account_id = account("fake-nft.testnet");
    h.ctx.signer_account_id = h.domain.nft.clone();
    h.reject("nft_on_approve", input.clone());
    h.ctx.predecessor_account_id = h.domain.nft.clone();
    h.reject("nft_on_approve", input.clone()); // authentic caller still cannot open gate
    let mut wrong_encoding = input;
    wrong_encoding["approval_id"] = json!(u64::MAX.to_string());
    h.reject("nft_on_approve", wrong_encoding);
}

#[test]
fn wasm_private_callback_rejects_forgery_counts_nonce_and_attached_value() {
    let (mut h, a, p) = Harness::pending();
    h.results(&a);
    h.ctx.predecessor_account_id = account("attacker.testnet");
    h.ctx.signer_account_id = h.domain.marketplace.clone();
    h.reject("on_nft_verified", args(&p));
    h.ctx.predecessor_account_id = h.domain.marketplace.clone();
    for count in [0, 1, 3] {
        h.ctx.promise_results = (0..count)
            .map(|_| HostResult::Successful(b"true".to_vec().into()))
            .collect::<Vec<_>>()
            .into();
        h.reject("on_nft_verified", args(&p));
    }
    h.results(&a);
    let mut wrong = args(&p);
    wrong["attempt_nonce"] = json!("2");
    h.reject("on_nft_verified", wrong);
    h.ctx.attached_deposit = Balance::from_yoctonear(1);
    h.reject("on_nft_verified", args(&p));
    h.ctx.attached_deposit = Balance::ZERO;
    let mut spoof = args(&p);
    spoof["result"] = json!(true);
    spoof["operation_hash"] = json!("invented");
    h.ctx.promise_results = vec![HostResult::Failed, HostResult::Failed].into();
    assert_eq!(result(h.call("on_nft_verified", spoof)), b"\"invalid\""); // ignored caller fields are never evidence
}

#[test]
fn wasm_verified_activation_is_persistent_and_duplicate_callback_is_noop() {
    let (mut h, a, p) = Harness::pending();
    h.results(&a);
    let root = h.ext.fake_trie[b"STATE".as_slice()].clone();
    assert_eq!(result(h.call("on_nft_verified", args(&p))), b"\"active\"");
    assert_ne!(h.ext.fake_trie[b"STATE".as_slice()], root);
    let before = h.ext.fake_trie.clone();
    assert_eq!(result(h.call("on_nft_verified", args(&p))), b"\"ignored\"");
    assert_eq!(before, h.ext.fake_trie);
    let status: serde_json::Value =
        serde_json::from_slice(&result(h.call("storage_status", json!({})))).unwrap();
    assert_eq!(status["trading_enabled"], false);
}

#[test]
fn wasm_failed_oversized_malformed_and_revoked_views_retire_without_payments() {
    for scenario in 0..6 {
        let (mut h, a, p) = Harness::pending();
        h.results(&a);
        let token = token_result(&h.domain, a.operation.verification().unwrap().1);
        h.ctx.promise_results = match scenario {
            0 => vec![
                HostResult::Failed,
                HostResult::Successful(b"true".to_vec().into()),
            ],
            1 => vec![
                HostResult::Successful(vec![b'x'; 4097].into()),
                HostResult::Successful(b"true".to_vec().into()),
            ],
            2 => vec![
                HostResult::Successful(token.into()),
                HostResult::Successful(vec![b'x'; 6].into()),
            ],
            3 => vec![
                HostResult::Successful(b"null".to_vec().into()),
                HostResult::Successful(b"true".to_vec().into()),
            ],
            4 => vec![
                HostResult::Successful(token.into()),
                HostResult::Successful(b"false".to_vec().into()),
            ],
            _ => vec![
                HostResult::Successful(b"true".to_vec().into()),
                HostResult::Successful(b"true".to_vec().into()),
            ],
        }
        .into();
        assert_eq!(result(h.call("on_nft_verified", args(&p))), b"\"invalid\"");
    }
}

#[test]
fn wasm_stale_generation_cannot_activate_its_successor() {
    let mut h = Harness::new();
    setup_domain(&h.domain);
    let old = provision(
        &h.domain,
        terms(&h.domain, 1),
        NearToken::from_near(1).as_yoctonear(),
    );
    let p = start(&h.domain, &old);
    context(
        &h.domain,
        h.domain.marketplace.clone(),
        20,
        vec![PromiseResult::Failed, PromiseResult::Failed],
    );
    complete(&h.domain, p.generation.0, 1).unwrap();
    let new = provision(
        &h.domain,
        terms(&h.domain, 3),
        NearToken::from_near(1).as_yoctonear(),
    );
    let next = start(&h.domain, &new);
    h.take_native_storage();
    h.results(&old);
    let before = h.ext.fake_trie.clone();
    assert_eq!(result(h.call("on_nft_verified", args(&p))), b"\"ignored\"");
    assert_eq!(h.ext.fake_trie, before);
    h.results(&new);
    assert_eq!(
        result(h.call("on_nft_verified", args(&next))),
        b"\"active\""
    );
}
