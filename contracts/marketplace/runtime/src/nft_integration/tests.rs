use super::*;
#[cfg(feature = "testnet")]
#[path = "funding_tests.rs"]
mod funding_tests;
use near_marketplace_foundation::{
    ledger::BalanceObservation,
    persistence::{ArtifactDomain, Budgets},
    state::ListingTerms,
    storage_accounting::StorageCommand,
    types::{CallbackResult, FeeTerms, TokenKey},
};
use near_sdk::{
    serde_json::{self, json},
    test_utils::{get_created_receipts, VMContextBuilder},
    testing_env, AccountContract, PromiseResult,
};

fn domain() -> DeploymentDomain {
    DeploymentDomain {
        environment: if cfg!(feature = "production") {
            ArtifactDomain::Production
        } else {
            ArtifactDomain::Testnet
        },
        genesis: [1; 32],
        manifest: [2; 32],
        marketplace_code: [3; 32],
        nft_code: [4; 32],
        marketplace: "market.testnet".parse().unwrap(),
        nft: "nft.testnet".parse().unwrap(),
    }
}
fn account(a: &str) -> AccountId {
    a.parse().unwrap()
}
fn context(d: &DeploymentDomain, caller: AccountId, now: u64, results: Vec<PromiseResult>) {
    let mut c = VMContextBuilder::new();
    c.current_account_id(d.marketplace.clone())
        .predecessor_account_id(caller)
        .signer_account_id(account("untrusted-signer.testnet"))
        .account_contract(AccountContract::Local(d.marketplace_code).into())
        .account_balance(NearToken::from_near(100))
        .storage_usage(env::storage_usage())
        .block_timestamp(now)
        .prepaid_gas(Gas::from_tgas(300));
    testing_env!(
        c.build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        results
    );
}
fn setup() -> DeploymentDomain {
    let d = domain();
    setup_domain(&d);
    d
}
fn setup_domain(d: &DeploymentDomain) {
    setup_domain_with_owner(d, None);
}
fn setup_domain_with_owner(d: &DeploymentDomain, owner: Option<AccountId>) {
    near_sdk::mock::with_mocked_blockchain(|b| {
        b.take_storage();
    });
    let mut c = VMContextBuilder::new();
    c.current_account_id(d.marketplace.clone())
        .predecessor_account_id(d.marketplace.clone())
        .signer_account_id(d.marketplace.clone())
        .account_balance(NearToken::from_near(100))
        .account_contract(AccountContract::Local(d.marketplace_code).into());
    testing_env!(c.build());
    let manifest = near_sdk::json_types::Base64VecU8(borsh::to_vec(d).unwrap());
    if let Some(owner) = owner {
        crate::Runtime::bootstrap_with_admission_owner(manifest, owner);
    } else {
        crate::Runtime::bootstrap(manifest);
    }
}
fn budgets() -> Budgets {
    Budgets {
        first_call_gas: Gas::from_tgas(20).as_gas(),
        second_call_gas: Gas::from_tgas(20).as_gas(),
        callback_gas: Gas::from_tgas(40).as_gas(),
        max_result_bytes: 4096,
        call_abi: wire::hash(wire::VERIFY_ABI),
        callback_abi: wire::hash(wire::CALLBACK_ABI),
    }
}
fn terms(d: &DeploymentDomain, approval: u64) -> ListingTerms {
    ListingTerms {
        token: TokenKey::new(d.nft.clone(), "art".into()).unwrap(),
        seller: account("seller.testnet"),
        approval_id: approval,
        price: 1000,
        expires_at_ns: 1000,
        fees: FeeTerms::new(Some(123), Some(456), Some(account("treasury.testnet"))).unwrap(),
        policy_version: 7,
        policy_hash: [11; 32],
        correlation: [12; 32],
    }
}
fn message(t: &ListingTerms) -> String {
    json!({"version":1,"price":t.price.to_string(),"expires_at_ns":t.expires_at_ns.to_string(),"policy_version":t.policy_version.to_string()}).to_string()
}
fn observation() -> BalanceObservation {
    BalanceObservation {
        account_balance: env::account_balance().as_yoctonear(),
        storage_stake: u128::from(env::storage_usage()) * env::storage_byte_cost().as_yoctonear(),
        ..Default::default()
    }
}
// Explicit synthetic policy/consent/credit. Never represents real receipt
// evidence; this bypasses the owner-created admission lifecycle for legacy tests.
fn provision(d: &DeploymentDomain, t: ListingTerms, amount: u128) -> Admission {
    let p = Repository::open(HostStore, d.clone())
        .unwrap()
        .prepare(OfflineCommand::Listing {
            terms: t,
            now_ns: 10,
            budgets: budgets(),
        })
        .unwrap();
    let admission = Admission {
        operation: p.operation().unwrap().clone(),
        storage_owner: account("sponsor.testnet"),
        creation_bytes: 16_384,
        local_gas: Gas::from_tgas(30).as_gas(),
    };
    install_admission(d, &admission, amount);
    admission
}
fn install_admission(d: &DeploymentDomain, admission: &Admission, amount: u128) {
    let (_, t) = admission.operation.verification().unwrap();
    let key = Admission::key(d, t.token.token(), &t.seller, t.approval_id).unwrap();
    assert!(!env::storage_has_key(&key));
    env::storage_write(&key, &admission.encode());
    let mut storage = StorageRepository::open(HostStore, d.clone()).unwrap();
    let after = observation();
    let mut id = admission.reservation().unwrap();
    id[0] ^= 0x80;
    let batch = storage
        .prepare(StorageCommand::Fund {
            credit: id,
            owner: admission.storage_owner.clone(),
            amount,
            before: BalanceObservation {
                account_balance: after.account_balance - amount,
                ..after
            },
            after,
        })
        .unwrap();
    storage.commit(&batch).unwrap();
    let batch = storage
        .prepare(StorageCommand::Reserve {
            reservation: admission.reservation().unwrap(),
            owner: admission.storage_owner.clone(),
            amount,
            balance: observation(),
        })
        .unwrap();
    storage.commit(&batch).unwrap();
}
fn start(d: &DeploymentDomain, a: &Admission) -> PendingVerification {
    context(d, d.nft.clone(), 10, vec![]);
    let (_, t) = a.operation.verification().unwrap();
    begin(
        d,
        t.token.token().into(),
        t.seller.clone(),
        t.approval_id,
        &message(t),
    )
    .unwrap()
}
fn token_result(d: &DeploymentDomain, t: &ListingTerms) -> Vec<u8> {
    serde_json::to_vec(
        &json!({"token_id":t.token.token(),"owner_id":t.seller,"metadata":null,
        "approved_account_ids":{d.marketplace.as_str():t.approval_id}}),
    )
    .unwrap()
}
fn valid_results(d: &DeploymentDomain, a: &Admission) -> Vec<PromiseResult> {
    vec![
        PromiseResult::Successful(token_result(d, a.operation.verification().unwrap().1)),
        PromiseResult::Successful(b"true".to_vec()),
    ]
}
fn listing(d: &DeploymentDomain, g: u64) -> ListingState {
    Repository::open(HostStore, d.clone())
        .unwrap()
        .read(&RecordKey::Listing(Generation::from_value(g).unwrap()))
        .unwrap()
        .unwrap()
        .listing()
        .unwrap()
        .state()
}

#[test]
fn hook_checks_predecessor_and_requires_explicit_admission() {
    let d = setup();
    let t = terms(&d, 1);
    context(&d, account("fake-nft.testnet"), 10, vec![]);
    assert_eq!(authenticate_nft(&d), Err(IntegrationError::Unauthorized));
    context(&d, d.nft.clone(), 10, vec![]);
    assert!(matches!(
        begin(&d, "art".into(), t.seller.clone(), 1, &message(&t)),
        Err(IntegrationError::MissingAdmission)
    ));
    assert!(near_marketplace_foundation::authorize_public_action(
        near_marketplace_foundation::PublicAction::List
    )
    .is_err());
    assert!(near_marketplace_foundation::authorize_public_action(
        near_marketplace_foundation::PublicAction::Purchase
    )
    .is_err());
}

#[test]
fn listing_message_rejects_unknown_duplicate_missing_and_unbounded_terms() {
    let d = setup();
    context(&d, d.nft.clone(), 10, vec![]);
    let good = message(&terms(&d, 1));
    assert!(wire::message(&good, MAX_MESSAGE_BYTES).is_ok());
    for bad in [
        "{}".into(),
        "x".repeat(4097),
        good.replace("\"version\":1", "\"version\":2"),
        good.replace("\"1000\"", "\"0\""),
        good.replace("\"price\":\"1000\"", "\"price\":1000"),
        good.replace("{", "{\"results\":true,"),
        good.replace("{", "{\"version\":1,"),
    ] {
        assert!(
            wire::message(&bad, MAX_MESSAGE_BYTES).is_err(),
            "accepted {bad}"
        );
    }
}

#[test]
fn verification_dispatch_is_ordered_zero_value_and_persists_exact_full_width_intent() {
    let d = setup();
    let a = provision(
        &d,
        terms(&d, u64::MAX),
        NearToken::from_near(1).as_yoctonear(),
    );
    let p = start(&d, &a);
    assert_eq!(listing(&d, p.generation.0), ListingState::Verifying);
    let receipts = get_created_receipts();
    assert_eq!(receipts.len(), 3);
    let mut calls = vec![];
    for r in &receipts {
        for action in &r.actions {
            if let near_sdk::mock::MockAction::FunctionCallWeight {
                method_name,
                args,
                attached_deposit,
                gas_weight,
                ..
            } = action
            {
                assert!(attached_deposit.is_zero());
                assert_eq!(gas_weight.0, 0);
                calls.push((
                    r.receiver_id.clone(),
                    String::from_utf8(method_name.clone()).unwrap(),
                    serde_json::from_slice::<serde_json::Value>(args).unwrap(),
                ));
            }
        }
    }
    assert_eq!(
        calls.iter().map(|c| c.1.as_str()).collect::<Vec<_>>(),
        vec!["nft_token", "nft_is_approved", "on_nft_verified"]
    );
    assert_eq!(calls[0].0, d.nft);
    assert_eq!(calls[1].0, d.nft);
    assert_eq!(calls[2].0, d.marketplace);
    assert_eq!(calls[1].2["approval_id"].as_u64(), Some(u64::MAX));
    assert_eq!(
        calls[2].2,
        json!({"generation":p.generation.0.to_string(),"attempt_nonce":"1"})
    );
    let stored = Repository::open(HostStore, d.clone())
        .unwrap()
        .read(&RecordKey::Verification(
            Generation::from_value(p.generation.0).unwrap(),
        ))
        .unwrap()
        .unwrap();
    assert_eq!(stored.operation().unwrap(), &a.operation);
    let storage = StorageRepository::open(HostStore, d.clone()).unwrap();
    let (owner, left, nonce) = storage
        .reservation_status(a.reservation().unwrap())
        .unwrap();
    assert_eq!(owner, a.storage_owner);
    assert!(left < NearToken::from_near(1).as_yoctonear());
    assert_eq!(nonce, 1);
    context(&d, d.marketplace.clone(), 20, valid_results(&d, &a));
    assert_eq!(
        complete(&d, p.generation.0, 1).unwrap(),
        VerificationOutcome::Active
    );
    assert_eq!(listing(&d, p.generation.0), ListingState::Active);
    let (_, left_after, nonce) = storage
        .reservation_status(a.reservation().unwrap())
        .unwrap();
    assert_eq!(left, left_after);
    assert_eq!(nonce, 2);
    assert!(get_created_receipts().is_empty());
}

#[test]
fn callback_authentication_missing_extra_results_and_wrong_attempt_preserve_pending() {
    let d = setup();
    let a = provision(&d, terms(&d, 1), NearToken::from_near(1).as_yoctonear());
    let p = start(&d, &a);
    let root = env::storage_read(b"STATE").unwrap();
    context(&d, account("attacker.testnet"), 20, valid_results(&d, &a));
    assert_eq!(
        complete(&d, p.generation.0, 1),
        Err(IntegrationError::Unauthorized)
    );
    for results in [
        vec![],
        vec![PromiseResult::Successful(b"true".to_vec())],
        (0..3)
            .map(|_| PromiseResult::Successful(b"true".to_vec()))
            .collect(),
    ] {
        context(&d, d.marketplace.clone(), 20, results);
        assert_eq!(
            complete(&d, p.generation.0, 1),
            Err(IntegrationError::ResultCount)
        );
        assert_eq!(env::storage_read(b"STATE").unwrap(), root);
    }
    context(&d, d.marketplace.clone(), 20, valid_results(&d, &a));
    assert_eq!(
        complete(&d, p.generation.0, 2),
        Err(IntegrationError::Context)
    );
    assert_eq!(listing(&d, p.generation.0), ListingState::Verifying);
    assert_eq!(
        complete(&d, p.generation.0, 1).unwrap(),
        VerificationOutcome::Active
    );
}

#[test]
fn failed_malformed_oversized_revoked_and_mismatched_views_never_activate() {
    for scenario in 0..13 {
        let d = setup();
        let a = provision(&d, terms(&d, 9), NearToken::from_near(1).as_yoctonear());
        let p = start(&d, &a);
        let mut results = valid_results(&d, &a);
        match scenario {
            0 => results[0] = PromiseResult::Failed,
            1 => results[1] = PromiseResult::Failed,
            2 => results[0] = PromiseResult::Successful(vec![b'x'; 4097]),
            3 => results[1] = PromiseResult::Successful(vec![b'x'; 6]),
            4 => results[0] = PromiseResult::Successful(b"null".to_vec()),
            5 => results[1] = PromiseResult::Successful(b"false".to_vec()),
            6 => results[1] = PromiseResult::Successful(b"\"yes\"".to_vec()),
            7 => {
                results[0] = PromiseResult::Successful(token_result(
                    &d,
                    &ListingTerms {
                        approval_id: 10,
                        ..terms(&d, 9)
                    },
                ))
            }
            8 => {
                results[0] = PromiseResult::Successful(token_result(
                    &d,
                    &ListingTerms {
                        seller: account("different.testnet"),
                        ..terms(&d, 9)
                    },
                ))
            }
            9 => {
                results[0] = PromiseResult::Successful(token_result(
                    &d,
                    &ListingTerms {
                        token: TokenKey::new(d.nft.clone(), "wrong".into()).unwrap(),
                        ..terms(&d, 9)
                    },
                ))
            }
            10 => results[1] = PromiseResult::Successful(token_result(&d, &terms(&d, 9))),
            11 => results[0] = PromiseResult::Successful(b"true".to_vec()),
            _ => results[0] = PromiseResult::Successful(b"{malformed".to_vec()),
        }
        context(&d, d.marketplace.clone(), 20, results);
        let principal = Repository::open(HostStore, d.clone())
            .unwrap()
            .totals()
            .unwrap();
        assert_eq!(
            complete(&d, p.generation.0, 1).unwrap(),
            VerificationOutcome::Invalid,
            "scenario {scenario}"
        );
        assert_eq!(listing(&d, p.generation.0), ListingState::Invalid);
        assert_eq!(
            Repository::open(HostStore, d.clone())
                .unwrap()
                .totals()
                .unwrap(),
            principal
        );
        assert!(get_created_receipts().is_empty());
    }
}

#[test]
fn duplicate_fields_approval_keys_and_stringified_numeric_ids_are_invalid() {
    let d = setup();
    let t = terms(&d, u64::MAX);
    let good = String::from_utf8(token_result(&d, &t)).unwrap();
    for bad in [
        good.replace("{", "{\"owner_id\":\"seller.testnet\","),
        good.replace("18446744073709551615", "\"18446744073709551615\""),
        good.replace(
            "\"market.testnet\":18446744073709551615",
            "\"market.testnet\":18446744073709551615,\"market.testnet\":18446744073709551615",
        ),
    ] {
        assert!(!wire::verify_views(bad.as_bytes(), b"true", &t, &d));
    }
}

#[test]
fn duplicate_and_delayed_callbacks_do_not_rewrite_terminal_or_newer_generations() {
    let d = setup();
    let a = provision(&d, terms(&d, 1), NearToken::from_near(1).as_yoctonear());
    let p = start(&d, &a);
    context(
        &d,
        d.marketplace.clone(),
        20,
        vec![PromiseResult::Failed, PromiseResult::Failed],
    );
    assert_eq!(
        complete(&d, p.generation.0, 1).unwrap(),
        VerificationOutcome::Invalid
    );
    let next = provision(&d, terms(&d, 3), NearToken::from_near(1).as_yoctonear());
    let newer = start(&d, &next);
    let root = env::storage_read(b"STATE").unwrap();
    context(&d, d.marketplace.clone(), 30, valid_results(&d, &a));
    assert_eq!(
        complete(&d, p.generation.0, 1).unwrap(),
        VerificationOutcome::Ignored
    );
    assert_eq!(env::storage_read(b"STATE").unwrap(), root);
    assert_eq!(listing(&d, newer.generation.0), ListingState::Verifying);
    context(&d, d.marketplace.clone(), 30, valid_results(&d, &next));
    assert_eq!(
        complete(&d, newer.generation.0, 1).unwrap(),
        VerificationOutcome::Active
    );
    let root = env::storage_read(b"STATE").unwrap();
    assert_eq!(
        complete(&d, newer.generation.0, 1).unwrap(),
        VerificationOutcome::Ignored
    );
    assert_eq!(env::storage_read(b"STATE").unwrap(), root);
}

#[test]
fn missing_callback_has_no_timeout_retry_and_late_expired_result_retires_candidate() {
    let d = setup();
    let a = provision(&d, terms(&d, 1), NearToken::from_near(1).as_yoctonear());
    let p = start(&d, &a);
    context(&d, d.nft.clone(), 25, vec![]);
    assert!(begin(
        &d,
        "art".into(),
        account("seller.testnet"),
        1,
        &message(&terms(&d, 1))
    )
    .is_err());
    assert!(Repository::open(HostStore, d.clone())
        .unwrap()
        .prepare(OfflineCommand::Listing {
            terms: terms(&d, 2),
            now_ns: 25,
            budgets: budgets()
        })
        .is_err());
    assert_eq!(listing(&d, p.generation.0), ListingState::Verifying);
    assert_eq!(
        StorageRepository::open(HostStore, d.clone())
            .unwrap()
            .reservation_status(a.reservation().unwrap())
            .unwrap()
            .2,
        1
    );
    context(&d, d.marketplace.clone(), 1000, valid_results(&d, &a));
    assert_eq!(
        complete(&d, p.generation.0, 1).unwrap(),
        VerificationOutcome::Invalid
    );
}

#[test]
fn storage_exhaustion_wrong_funder_and_insufficient_gas_reject_before_writes() {
    for scenario in 0..3 {
        let d = setup();
        let mut a = provision(
            &d,
            terms(&d, 1),
            if scenario == 0 {
                1
            } else {
                NearToken::from_near(1).as_yoctonear()
            },
        );
        if scenario == 1 {
            a.storage_owner = account("different.testnet");
        }
        if scenario == 2 {
            a.local_gas = u64::MAX;
        }
        let (_, t) = a.operation.verification().unwrap();
        env::storage_write(
            &Admission::key(&d, "art", &t.seller, 1).unwrap(),
            &a.encode(),
        );
        let root = env::storage_read(b"STATE").unwrap();
        context(&d, d.nft.clone(), 10, vec![]);
        assert!(begin(&d, "art".into(), t.seller.clone(), 1, &message(t)).is_err());
        assert_eq!(env::storage_read(b"STATE").unwrap(), root);
        assert!(get_created_receipts().is_empty());
    }
}

#[test]
fn receipt_restart_reads_persisted_attempt_and_does_not_reset_counters() {
    let d = setup();
    let a = provision(&d, terms(&d, 1), NearToken::from_near(1).as_yoctonear());
    let p = start(&d, &a);
    let root = env::storage_read(b"STATE").unwrap();
    // testing_env replaces the VM context while preserving trie bytes; neither
    // the callback nor the repository holds the earlier invocation's objects.
    context(&d, d.marketplace.clone(), 20, valid_results(&d, &a));
    assert_eq!(env::storage_read(b"STATE").unwrap(), root);
    assert_eq!(
        complete(&d, p.generation.0, 1).unwrap(),
        VerificationOutcome::Active
    );
    assert_eq!(
        Repository::open(HostStore, d.clone())
            .unwrap()
            .read(&RecordKey::Verification(
                Generation::from_value(p.generation.0).unwrap()
            ))
            .unwrap()
            .unwrap()
            .operation()
            .unwrap(),
        &a.operation
    );
}

#[test]
fn purchase_call_uses_only_exact_wrapper_and_unknown_sale_stays_locked() {
    let d = setup();
    let a = provision(
        &d,
        terms(&d, u64::MAX),
        NearToken::from_near(1).as_yoctonear(),
    );
    let p = start(&d, &a);
    context(&d, d.marketplace.clone(), 20, valid_results(&d, &a));
    complete(&d, p.generation.0, 1).unwrap();
    let mut repo = Repository::open(HostStore, d.clone()).unwrap();
    let b = Budgets {
        first_call_gas: Gas::from_tgas(30).as_gas(),
        second_call_gas: 0,
        callback_gas: Gas::from_tgas(30).as_gas(),
        max_result_bytes: 4,
        call_abi: wire::hash(wire::TRANSFER_ABI),
        callback_abi: [77; 32],
    }; // synthetic future callback ABI, never dispatched
    let purchase = repo
        .prepare(OfflineCommand::Purchase {
            generation: Generation::from_value(p.generation.0).unwrap(),
            token: terms(&d, 1).token,
            buyer: account("buyer.testnet"),
            exact_price: 1000,
            now_ns: 30,
            budgets: b,
        })
        .unwrap();
    repo.commit(&purchase).unwrap();
    let sale = purchase.sale().unwrap();
    let call = prepare_purchase_call(&d, sale).unwrap();
    assert_eq!(call.method, "marketplace_transfer_approved");
    assert_eq!(call.target, d.nft);
    assert_eq!(call.attached_yocto, 1);
    let args: serde_json::Value = serde_json::from_slice(&call.args).unwrap();
    assert_eq!(
        args,
        json!({"expected_seller_id":"seller.testnet","token_id":"art","approval_id":u64::MAX.to_string(),"receiver_id":"buyer.testnet"})
    );
    let unknown = repo
        .prepare(OfflineCommand::TransferResult {
            sale,
            generation: Generation::from_value(p.generation.0).unwrap(),
            attempt: purchase.attempt().unwrap(),
            result: CallbackResult::Unknown,
        })
        .unwrap();
    repo.commit(&unknown).unwrap();
    let root = env::storage_read(b"STATE").unwrap();
    let principal = repo.totals().unwrap();
    context(&d, d.marketplace.clone(), 40, valid_results(&d, &a));
    assert_eq!(
        complete(&d, p.generation.0, 1).unwrap(),
        VerificationOutcome::Ignored
    );
    assert!(prepare_purchase_call(&d, sale).is_err());
    assert_eq!(listing(&d, p.generation.0), ListingState::Review);
    assert_eq!(repo.totals().unwrap(), principal);
    assert_eq!(env::storage_read(b"STATE").unwrap(), root);
    assert!(get_created_receipts().is_empty());
}

#[test]
fn corrupt_and_cross_domain_admission_records_fail_closed() {
    let d = setup();
    let a = provision(&d, terms(&d, 1), NearToken::from_near(1).as_yoctonear());
    let (_, t) = a.operation.verification().unwrap();
    let key = Admission::key(&d, "art", &t.seller, 1).unwrap();
    let good = a.encode();
    for bad in [
        good[..good.len() - 1].to_vec(),
        {
            let mut b = good.clone();
            b.push(0);
            b
        },
        vec![0; 4097],
    ] {
        env::storage_write(&key, &bad);
        assert!(Admission::load(&d, "art", &t.seller, 1).is_err());
    }
    env::storage_write(&key, &good);
    let mut other = d.clone();
    other.genesis = [99; 32];
    assert!(Admission::load(&other, "art", &t.seller, 1).is_err());
}

#[cfg(feature = "vm-tests")]
#[path = "wasm_tests.rs"]
mod wasm;

#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;

#[path = "dispatch_tests.rs"]
mod dispatch_tests;
