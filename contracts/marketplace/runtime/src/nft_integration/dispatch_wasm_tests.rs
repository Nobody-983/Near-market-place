//! Generated dispatch ABI and promise graph in the VM. Trie, account balances
//! and dependency results are fixtures, not live cross-contract/receipt evidence.
use super::super::dispatch_tests::{invalid_results, locator};
use super::*;
use near_vm_runner::logic::mocks::mock_external::MockAction;

fn input(h: &Harness, a: &Admission) -> serde_json::Value {
    json!({"admission_id":near_sdk::json_types::Base64VecU8(locator(&h.domain, a))})
}
fn ready() -> (Harness, Admission) {
    let (mut h, a) = admission_fixture();
    result(h.call("create_nft_admission", admission_args(&a)));
    (h, a)
}
fn send(h: &mut Harness, a: &Admission) -> Vec<u8> {
    let args = input(h, a);
    result(h.call("dispatch_nft_admission", args))
}
fn callback_args(a: &Admission) -> serde_json::Value {
    json!({"generation":a.operation.verification().unwrap().0.value().to_string(),"attempt_nonce":"1"})
}
fn callback(h: &mut Harness, a: &Admission) -> Vec<u8> {
    h.ctx.predecessor_account_id = h.domain.marketplace.clone();
    result(h.call("on_nft_verified", callback_args(a)))
}
fn record_bytes(h: &Harness, key: RecordKey) -> Vec<u8> {
    h.ext.fake_trie[&key.storage_key(&h.domain).unwrap()].clone()
}

#[test]
fn wasm_dispatch_rejects_unauthorized_deposits_results_missing_and_released_admissions() {
    let (mut h, a) = ready();
    let args = input(&h, &a);
    for caller in [
        account("attacker.testnet"),
        h.domain.nft.clone(),
        h.domain.marketplace.clone(),
    ] {
        h.ctx.predecessor_account_id = caller;
        h.ctx.signer_account_id = sponsor();
        h.reject("dispatch_nft_admission", args.clone());
    }
    h.ctx.predecessor_account_id = sponsor();
    h.ctx.attached_deposit = Balance::from_yoctonear(1);
    h.reject("dispatch_nft_admission", args.clone());
    h.ctx.attached_deposit = Balance::ZERO;
    h.results(&a);
    h.reject("dispatch_nft_admission", args.clone());
    h.ctx.promise_results = vec![].into();
    for id in [vec![], vec![0; 31], vec![0; 32], vec![0; 33], vec![0; 4097]] {
        h.reject(
            "dispatch_nft_admission",
            json!({"admission_id":near_sdk::json_types::Base64VecU8(id)}),
        );
    }
    let mut other = h.domain.clone();
    other.genesis = [88; 32];
    h.reject(
        "dispatch_nft_admission",
        json!({"admission_id":near_sdk::json_types::Base64VecU8(locator(&other, &a))}),
    );
    result(h.call("release_nft_admission", release_args(&a)));
    h.reject("dispatch_nft_admission", args);
}

#[test]
fn wasm_dispatch_graph_uses_only_persisted_terms_and_restart_retries_never_resend() {
    let (mut h, a) = ready();
    let mut args = input(&h, &a);
    // Extra SDK-ignored fields are neither call inputs nor evidence.
    for (key, value) in [
        ("seller", json!("attacker.testnet")),
        ("nft", json!("evil.testnet")),
        ("token_id", json!("wrong")),
        ("approval_id", json!(1)),
        ("generation", json!("999")),
        ("attempt_nonce", json!("9")),
        ("result", json!(true)),
    ] {
        args[key] = value;
    }
    let usage = h.ctx.storage_usage;
    let pending = result(h.call("dispatch_nft_admission", args));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&pending).unwrap(),
        json!({"generation":"1","attempt_nonce":"1","state":"dispatched"})
    );
    assert_eq!(h.ctx.storage_usage, usage);
    assert_eq!(h.ext.action_log.len(), 6);
    let mut deps = vec![];
    for (i, pair) in h.ext.action_log.as_chunks::<2>().0.iter().enumerate() {
        let MockAction::CreateReceipt {
            receipt_indices,
            receiver_id,
        } = &pair[0]
        else {
            panic!("receipt expected");
        };
        let MockAction::FunctionCallWeight {
            receipt_index,
            method_name,
            args,
            attached_deposit,
            prepaid_gas,
            gas_weight,
        } = &pair[1]
        else {
            panic!("only pinned calls allowed");
        };
        let (method, target, expected, gas) = match i {
            0 => (
                "nft_token",
                &h.domain.nft,
                json!({"token_id":"art"}),
                budgets().first_call_gas,
            ),
            1 => (
                "nft_is_approved",
                &h.domain.nft,
                json!({"token_id":"art","approved_account_id":h.domain.marketplace,"approval_id":u64::MAX}),
                budgets().second_call_gas,
            ),
            _ => (
                "on_nft_verified",
                &h.domain.marketplace,
                callback_args(&a),
                budgets().callback_gas,
            ),
        };
        assert_eq!(method_name, method.as_bytes());
        assert_eq!(receiver_id, target);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(args).unwrap(),
            expected
        );
        assert_eq!(attached_deposit, &Balance::ZERO);
        assert_eq!(prepaid_gas, &RuntimeGas::from_gas(gas));
        assert_eq!(gas_weight.0, 0);
        if i < 2 {
            assert!(receipt_indices.is_empty());
            deps.push(*receipt_index);
        } else {
            assert_eq!(receipt_indices, &deps);
        }
    }
    let before = h.ext.fake_trie.clone();
    let actions = h.ext.action_log.clone();
    assert_eq!(send(&mut h, &a), pending);
    assert_eq!(h.ext.fake_trie, before);
    assert_eq!(h.ext.action_log, actions);
    let mut restarted = Harness::new();
    restarted.ext.fake_trie = h.ext.fake_trie;
    restarted.ctx = h.ctx;
    assert_eq!(send(&mut restarted, &a), pending);
    assert!(restarted.ext.action_log.is_empty());
    restarted.results(&a);
    assert_eq!(callback(&mut restarted, &a), b"\"active\"");
    let final_state = restarted.ext.fake_trie.clone();
    assert_eq!(callback(&mut restarted, &a), b"\"ignored\"");
    assert_eq!(restarted.ext.fake_trie, final_state);
    restarted.ctx.predecessor_account_id = sponsor();
    restarted.ctx.promise_results = vec![].into();
    let args = input(&restarted, &a);
    restarted.reject("dispatch_nft_admission", args);
    let status: serde_json::Value =
        serde_json::from_slice(&result(restarted.call("storage_status", json!({})))).unwrap();
    assert_eq!(status["trading_enabled"], false);
}

#[test]
fn wasm_dispatched_failed_wrong_missing_oversized_malformed_and_duplicate_views_never_activate() {
    for scenario in 0..14 {
        let (mut h, a) = ready();
        send(&mut h, &a);
        let storage_root =
            h.ext.fake_trie[near_marketplace_foundation::persistence::STORAGE_ROOT_KEY].clone();
        // Only host dependency fixtures supply results to the callback.
        h.ctx.promise_results = invalid_results(&h.domain, &a, scenario)
            .into_iter()
            .map(|p| match p {
                PromiseResult::Successful(bytes) => HostResult::Successful(bytes.into()),
                PromiseResult::Failed => HostResult::Failed,
            })
            .collect::<Vec<_>>()
            .into();
        assert_eq!(callback(&mut h, &a), b"\"invalid\"");
        let bytes = record_bytes(&h, RecordKey::Listing(Generation::from_value(1).unwrap()));
        let r = near_marketplace_foundation::persistence::decode_record(
            &bytes,
            &h.domain,
            &RecordKey::Listing(Generation::from_value(1).unwrap()),
        )
        .unwrap();
        assert_eq!(r.listing().unwrap().state(), ListingState::Invalid);
        // Accounting envelope revision advances; totals and credit do not.
        assert_ne!(
            h.ext.fake_trie[near_marketplace_foundation::persistence::STORAGE_ROOT_KEY],
            storage_root
        );
        h.ctx.predecessor_account_id = sponsor();
        h.ctx.promise_results = vec![].into();
        h.reject("release_nft_admission", release_args(&a));
        let args = input(&h, &a);
        h.reject("dispatch_nft_admission", args);
    }
}

#[test]
fn wasm_unknown_callback_context_preserves_pending_cursor_without_resend_or_release() {
    let (mut h, a) = ready();
    let pending = send(&mut h, &a);
    let before = h.ext.fake_trie.clone();
    let actions = h.ext.action_log.clone();
    h.ctx.predecessor_account_id = h.domain.marketplace.clone();
    for count in [0, 1, 3] {
        h.ctx.promise_results = (0..count)
            .map(|_| HostResult::Successful(b"true".to_vec().into()))
            .collect::<Vec<_>>()
            .into();
        h.reject("on_nft_verified", callback_args(&a));
    }
    h.results(&a);
    h.reject(
        "on_nft_verified",
        json!({"generation":"1","attempt_nonce":"2"}),
    );
    h.ctx.predecessor_account_id = sponsor();
    h.ctx.signer_account_id = h.domain.marketplace.clone();
    h.reject("on_nft_verified", callback_args(&a));
    h.ctx.promise_results = vec![].into();
    h.ctx.block_timestamp = 5000;
    assert_eq!(send(&mut h, &a), pending);
    h.reject("release_nft_admission", release_args(&a));
    assert_eq!(h.ext.fake_trie, before);
    assert_eq!(h.ext.action_log, actions);
    h.results(&a);
    assert_eq!(callback(&mut h, &a), b"\"invalid\""); // late success, expired candidate
}

#[test]
fn wasm_stale_and_completed_admission_dispatch_cannot_modify_new_generation() {
    let mut h = Harness::new();
    funded_setup(&h.domain);
    let old = candidate(&h.domain, terms(&h.domain, 1));
    lifecycle_tests::create(&h.domain, &old).unwrap();
    let mut repo = Repository::open(HostStore, h.domain.clone()).unwrap();
    let cancel = repo
        .prepare(OfflineCommand::Cancel {
            generation: Generation::from_value(1).unwrap(),
            seller: terms(&h.domain, 1).seller,
        })
        .unwrap();
    repo.commit(&cancel).unwrap();
    let next = candidate(&h.domain, terms(&h.domain, 2));
    lifecycle_tests::create(&h.domain, &next).unwrap();
    h.take_native_storage();
    h.ctx.predecessor_account_id = sponsor();
    let args = input(&h, &old);
    h.reject("dispatch_nft_admission", args);
    let newer_bytes = record_bytes(&h, RecordKey::Listing(Generation::from_value(2).unwrap()));
    h.results(&old);
    assert_eq!(callback(&mut h, &old), b"\"ignored\"");
    assert_eq!(
        record_bytes(&h, RecordKey::Listing(Generation::from_value(2).unwrap())),
        newer_bytes
    );
    h.ctx.predecessor_account_id = sponsor();
    h.ctx.promise_results = vec![].into();
    send(&mut h, &next);
    h.results(&next);
    assert_eq!(callback(&mut h, &next), b"\"active\"");
    h.ctx.predecessor_account_id = sponsor();
    h.ctx.promise_results = vec![].into();
    let args = input(&h, &next);
    h.reject("dispatch_nft_admission", args);
}

#[test]
fn wasm_dispatch_insolvency_gas_and_corrupted_lifecycle_fail_before_writes_or_sends() {
    let (mut h, a) = ready();
    let args = input(&h, &a);
    h.ctx.account_balance = Balance::from_yoctonear(1);
    h.reject("dispatch_nft_admission", args.clone());
    h.ctx.account_balance = Balance::from_near(100);
    h.ctx.prepaid_gas = RuntimeGas::from_teragas(100);
    h.reject("dispatch_nft_admission", args.clone());
    h.ctx.prepaid_gas = RuntimeGas::from_teragas(300);
    let mut key = b"nmp:runtime:admission-life:v1:".to_vec();
    key.extend(a.reservation().unwrap());
    let good = h.ext.fake_trie.remove(&key).unwrap();
    h.reject("dispatch_nft_admission", args.clone());
    h.ext.fake_trie.insert(key.clone(), vec![0; 89]);
    h.reject("dispatch_nft_admission", args.clone());
    h.ext.fake_trie.insert(key, good);
    h.ctx.block_timestamp = 1000;
    h.reject("dispatch_nft_admission", args);
}

#[test]
fn wasm_financial_hold_preserves_all_existing_claims_unknown_attempts_and_reservations() {
    let mut h = Harness::new();
    let (a, _) = lifecycle_tests::protected_fixture(&h.domain);
    h.take_native_storage();
    h.ctx.predecessor_account_id = sponsor();
    let args = input(&h, &a);
    h.reject("dispatch_nft_admission", args);
    // reject compares every persisted byte, including held principal, claims,
    // unknown transfer, unattributed balance, reservation and sticky holds.
}
