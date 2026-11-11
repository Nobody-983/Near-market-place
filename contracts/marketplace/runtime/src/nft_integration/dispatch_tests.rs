use super::lifecycle_tests::{candidate, create, funded_setup, sponsor};
use super::*;

pub(super) fn locator(d: &DeploymentDomain, a: &Admission) -> Vec<u8> {
    let (_, t) = a.operation.verification().unwrap();
    let key = Admission::key(d, t.token.token(), &t.seller, t.approval_id).unwrap();
    key[key.len() - 32..].to_vec()
}
pub(super) fn ready(d: &DeploymentDomain) -> Admission {
    funded_setup(d);
    let a = candidate(d, terms(d, u64::MAX));
    create(d, &a).unwrap();
    a
}

#[test]
fn dispatch_requires_owner_predecessor_zero_deposit_and_no_result_injection() {
    let d = domain();
    let a = ready(&d);
    let id = locator(&d, &a);
    let root = env::storage_read(b"STATE");
    for caller in [
        account("attacker.testnet"),
        d.nft.clone(),
        d.marketplace.clone(),
    ] {
        context(&d, caller, 20, vec![]);
        assert_eq!(
            dispatch(&d, &id).unwrap_err(),
            IntegrationError::Unauthorized
        );
        assert_eq!(env::storage_read(b"STATE"), root);
        assert!(get_created_receipts().is_empty());
    }
    context(&d, sponsor(), 20, valid_results(&d, &a));
    assert_eq!(dispatch(&d, &id).unwrap_err(), IntegrationError::Context);
    let mut c = VMContextBuilder::new();
    c.current_account_id(d.marketplace.clone())
        .predecessor_account_id(sponsor())
        .signer_account_id(sponsor())
        .attached_deposit(NearToken::from_yoctonear(1));
    testing_env!(c.build());
    assert_eq!(
        dispatch(&d, &id).unwrap_err(),
        IntegrationError::AttachedValue
    );
    assert_eq!(env::storage_read(b"STATE"), root);
}

#[test]
fn locator_cannot_override_persisted_identity_or_adopt_legacy_records() {
    let d = domain();
    let a = ready(&d);
    let id = locator(&d, &a);
    let root = env::storage_read(b"STATE");
    for bad in [
        vec![],
        vec![0; 31],
        vec![0; 33],
        vec![0; 32],
        a.operation.canonical_bytes().unwrap(),
    ] {
        assert!(dispatch(&d, &bad).is_err());
    }
    let mut other = d.clone();
    other.genesis = [81; 32];
    assert!(dispatch(&d, &locator(&other, &a)).is_err());
    // Copy a valid admission to a different locator: decoder recomputes key.
    let mut wrong_key = Admission::key(&d, "wrong", &sponsor(), 1).unwrap();
    env::storage_write(&wrong_key, &a.encode());
    let wrong_id = wrong_key.split_off(wrong_key.len() - 32);
    assert!(dispatch(&d, &wrong_id).is_err());
    assert_eq!(env::storage_read(b"STATE"), root);
    let (_, t) = a.operation.verification().unwrap();
    release_admission(&d, t.token.token(), &t.seller, t.approval_id, 1).unwrap();
    assert!(dispatch(&d, &id).is_err());
    let mut legacy_terms = terms(&d, 1);
    legacy_terms.token = TokenKey::new(d.nft.clone(), "legacy".into()).unwrap();
    let legacy = provision(&d, legacy_terms, NearToken::from_near(1).as_yoctonear());
    assert!(dispatch(&d, &locator(&d, &legacy)).is_err());
}

#[test]
fn owner_dispatch_uses_persisted_ordered_graph_and_duplicate_is_read_only() {
    let d = domain();
    let a = ready(&d);
    let id = locator(&d, &a);
    let storage = StorageRepository::open(HostStore, d.clone()).unwrap();
    let before = storage.snapshot().unwrap();
    let usage = env::storage_usage();
    let status = dispatch(&d, &id).unwrap();
    assert_eq!(status.state, AdmissionState::Dispatched);
    assert_eq!((status.generation.0, status.attempt_nonce.0), (1, 1));
    assert_eq!(env::storage_usage(), usage);
    assert_eq!(storage.snapshot().unwrap(), before);
    assert_eq!(
        storage
            .reservation_status(a.reservation().unwrap())
            .unwrap()
            .2,
        2
    );
    let receipts = get_created_receipts();
    assert_eq!(receipts.len(), 3);
    let mut indices = vec![];
    for (i, r) in receipts.iter().enumerate() {
        assert_eq!(
            r.receiver_id,
            if i == 2 {
                d.marketplace.clone()
            } else {
                d.nft.clone()
            }
        );
        assert_eq!(r.actions.len(), 1);
        if let near_sdk::mock::MockAction::FunctionCallWeight {
            receipt_index,
            method_name,
            args,
            attached_deposit,
            prepaid_gas,
            gas_weight,
        } = &r.actions[0]
        {
            let (name, expected, gas) = match i {
                0 => (
                    "nft_token",
                    json!({"token_id":"art"}),
                    budgets().first_call_gas,
                ),
                1 => (
                    "nft_is_approved",
                    json!({"token_id":"art","approved_account_id":d.marketplace,"approval_id":u64::MAX}),
                    budgets().second_call_gas,
                ),
                _ => (
                    "on_nft_verified",
                    json!({"generation":"1","attempt_nonce":"1"}),
                    budgets().callback_gas,
                ),
            };
            assert_eq!(method_name, name.as_bytes());
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(args).unwrap(),
                expected
            );
            assert!(attached_deposit.is_zero());
            assert_eq!(gas_weight.0, 0);
            assert_eq!(prepaid_gas.as_gas(), gas);
            if i < 2 {
                assert!(r.receipt_indices.is_empty());
                indices.push(*receipt_index);
            } else {
                assert_eq!(r.receipt_indices, indices);
            }
        } else {
            panic!("Only pinned function calls are allowed");
        }
    }
    let root = env::storage_read(b"STATE");
    context(&d, sponsor(), 20, vec![]); // restart host context, retain only trie
    assert_eq!(dispatch(&d, &id).unwrap(), status);
    assert_eq!(env::storage_read(b"STATE"), root);
    assert!(get_created_receipts().is_empty());
    context(&d, d.marketplace.clone(), 30, valid_results(&d, &a));
    assert_eq!(complete(&d, 1, 1).unwrap(), VerificationOutcome::Active);
    assert_eq!(storage.snapshot().unwrap(), before);
    assert_eq!(
        storage
            .reservation_status(a.reservation().unwrap())
            .unwrap()
            .2,
        3
    );
    context(&d, sponsor(), 40, vec![]);
    assert!(dispatch(&d, &id).is_err());
    assert!(near_marketplace_foundation::authorize_public_action(
        near_marketplace_foundation::PublicAction::List
    )
    .is_err());
    assert!(near_marketplace_foundation::authorize_public_action(
        near_marketplace_foundation::PublicAction::Purchase
    )
    .is_err());
}

pub(super) fn invalid_results(
    d: &DeploymentDomain,
    a: &Admission,
    scenario: usize,
) -> Vec<PromiseResult> {
    let mut views = valid_results(d, a);
    let (_, terms) = a.operation.verification().unwrap();
    match scenario {
        0 => views[0] = PromiseResult::Failed,
        1 => views[1] = PromiseResult::Failed,
        2 => views[0] = PromiseResult::Successful(b"null".to_vec()),
        3 => {
            let mut t = terms.clone();
            t.seller = account("attacker.testnet");
            views[0] = PromiseResult::Successful(token_result(d, &t));
        }
        4 => {
            let mut t = terms.clone();
            t.approval_id = 1;
            views[0] = PromiseResult::Successful(token_result(d, &t));
        }
        5 => views[1] = PromiseResult::Successful(b"false".to_vec()),
        6 => views[0] = PromiseResult::Successful(vec![b'x'; 4097]),
        7 => views[1] = PromiseResult::Successful(vec![b'x'; 6]),
        8 => views[0] = PromiseResult::Successful(b"{broken".to_vec()),
        9 => views[1] = PromiseResult::Successful(b"null".to_vec()),
        10 => views[1] = PromiseResult::Successful(token_result(d, terms)), // duplicated token result
        11 => views[0] = PromiseResult::Successful(b"true".to_vec()), // duplicated bool result
        12 => {
            let mut t = terms.clone();
            t.token = TokenKey::new(d.nft.clone(), "wrong".into()).unwrap();
            views[0] = PromiseResult::Successful(token_result(d, &t));
        }
        _ => views[0] = PromiseResult::Successful(b"{}".to_vec()),
    }
    views
}

#[test]
fn failed_or_unverifiable_views_retire_only_candidate_not_funds_or_nft_ownership() {
    for scenario in 0..14 {
        let d = domain();
        let a = ready(&d);
        let id = locator(&d, &a);
        dispatch(&d, &id).unwrap();
        let storage = StorageRepository::open(HostStore, d.clone()).unwrap();
        let before = storage.snapshot().unwrap();
        context(
            &d,
            d.marketplace.clone(),
            20,
            invalid_results(&d, &a, scenario),
        );
        assert_eq!(complete(&d, 1, 1).unwrap(), VerificationOutcome::Invalid);
        assert_eq!(listing(&d, 1), ListingState::Invalid);
        assert_eq!(storage.snapshot().unwrap(), before);
        assert!(get_created_receipts().is_empty());
        context(&d, sponsor(), 30, vec![]);
        assert!(dispatch(&d, &id).is_err());
        assert!(release_admission(&d, "art", &terms(&d, 1).seller, u64::MAX, 1).is_err());
    }
}

#[test]
fn missing_callback_or_wrong_context_is_unknown_and_never_resends_or_releases() {
    let d = domain();
    let a = ready(&d);
    let id = locator(&d, &a);
    let status = dispatch(&d, &id).unwrap();
    let root = env::storage_read(b"STATE");
    for count in [0, 1, 3] {
        context(
            &d,
            d.marketplace.clone(),
            20,
            (0..count)
                .map(|_| PromiseResult::Successful(b"true".to_vec()))
                .collect(),
        );
        assert_eq!(
            complete(&d, 1, 1).unwrap_err(),
            IntegrationError::ResultCount
        );
        assert_eq!(env::storage_read(b"STATE"), root);
    }
    context(&d, account("attacker.testnet"), 20, valid_results(&d, &a));
    assert_eq!(
        complete(&d, 1, 1).unwrap_err(),
        IntegrationError::Unauthorized
    );
    context(&d, d.marketplace.clone(), 20, valid_results(&d, &a));
    assert_eq!(complete(&d, 1, 2).unwrap_err(), IntegrationError::Context);
    context(&d, sponsor(), 5000, vec![]);
    assert_eq!(dispatch(&d, &id).unwrap(), status); // late retry is only observation
    assert!(get_created_receipts().is_empty());
    assert_eq!(env::storage_read(b"STATE"), root);
    assert!(release_admission(&d, "art", &terms(&d, 1).seller, u64::MAX, 1).is_err());
    context(&d, d.marketplace.clone(), 5000, valid_results(&d, &a));
    assert_eq!(complete(&d, 1, 1).unwrap(), VerificationOutcome::Invalid); // valid views, expired terms
}

#[test]
fn stale_ready_and_delayed_completed_callbacks_cannot_modify_a_successor() {
    let d = domain();
    funded_setup(&d);
    let old = candidate(&d, terms(&d, 1));
    create(&d, &old).unwrap();
    // Simulate a separately persisted cancellation: no dispatch can repair or
    // reinterpret contradictory ready state, even before a successor exists.
    let mut repo = Repository::open(HostStore, d.clone()).unwrap();
    let cancel = repo
        .prepare(OfflineCommand::Cancel {
            generation: Generation::from_value(1).unwrap(),
            seller: terms(&d, 1).seller,
        })
        .unwrap();
    repo.commit(&cancel).unwrap();
    assert!(dispatch(&d, &locator(&d, &old)).is_err());
    let a = candidate(&d, terms(&d, 2));
    create(&d, &a).unwrap();
    dispatch(&d, &locator(&d, &a)).unwrap();
    context(
        &d,
        d.marketplace.clone(),
        20,
        vec![PromiseResult::Failed, PromiseResult::Failed],
    );
    complete(&d, 2, 1).unwrap();
    context(&d, sponsor(), 30, vec![]);
    let newer = candidate(&d, terms(&d, 3));
    create(&d, &newer).unwrap();
    let root = env::storage_read(b"STATE");
    assert!(dispatch(&d, &locator(&d, &a)).is_err());
    context(&d, d.marketplace.clone(), 40, valid_results(&d, &a));
    assert_eq!(complete(&d, 2, 1).unwrap(), VerificationOutcome::Ignored);
    assert_eq!(complete(&d, 1, 1).unwrap(), VerificationOutcome::Ignored);
    assert_eq!(env::storage_read(b"STATE"), root);
    assert_eq!(listing(&d, 3), ListingState::Verifying);
}

#[test]
fn dispatch_backing_and_gas_failures_preserve_ready_reservation_and_sticky_holds() {
    let d = domain();
    let a = ready(&d);
    let id = locator(&d, &a);
    let storage = StorageRepository::open(HostStore, d.clone()).unwrap();
    let before = storage.snapshot().unwrap();
    let root = env::storage_read(b"STATE");
    for (balance, gas) in [
        (NearToken::from_yoctonear(1), Gas::from_tgas(300)),
        (NearToken::from_near(100), Gas::from_tgas(100)),
    ] {
        let mut c = VMContextBuilder::new();
        c.current_account_id(d.marketplace.clone())
            .predecessor_account_id(sponsor())
            .account_balance(balance)
            .prepaid_gas(gas)
            .storage_usage(env::storage_usage())
            .block_timestamp(20);
        testing_env!(c.build());
        assert!(dispatch(&d, &id).is_err());
        assert_eq!(env::storage_read(b"STATE"), root);
        assert_eq!(storage.snapshot().unwrap(), before);
        assert!(get_created_receipts().is_empty());
    }
    context(&d, sponsor(), 20, vec![]);
    let mut storage = StorageRepository::open(HostStore, d.clone()).unwrap();
    let hold = storage
        .prepare(StorageCommand::CheckHealth {
            balance: BalanceObservation {
                account_balance: 0,
                ..observation()
            },
        })
        .unwrap();
    storage.commit(&hold).unwrap();
    let before = storage.snapshot().unwrap();
    let root = env::storage_read(b"STATE");
    assert_eq!(dispatch(&d, &id).unwrap_err(), IntegrationError::Storage);
    assert_eq!(env::storage_read(b"STATE"), root);
    assert_eq!(storage.snapshot().unwrap(), before);
}

#[test]
fn zero_remaining_credit_dispatches_without_growth_and_hold_allows_only_existing_callback() {
    let d = domain();
    let a = ready(&d);
    let charged = StorageRepository::open(HostStore, d.clone())
        .unwrap()
        .snapshot()
        .unwrap()
        .1
        .totals()
        .staked;
    funded_setup(&d);
    let mut a = candidate(&d, a.operation.verification().unwrap().1.clone());
    a.creation_bytes = (charged / env::storage_byte_cost().as_yoctonear()) as u64;
    create_admission(
        &d,
        &a.operation.canonical_bytes().unwrap(),
        sponsor(),
        charged,
        a.creation_bytes,
        a.local_gas,
    )
    .unwrap();
    let mut storage = StorageRepository::open(HostStore, d.clone()).unwrap();
    assert_eq!(
        storage
            .reservation_remaining(a.reservation().unwrap())
            .unwrap(),
        0
    );
    dispatch(&d, &locator(&d, &a)).unwrap();
    let hold = storage
        .prepare(StorageCommand::CheckHealth {
            balance: BalanceObservation {
                account_balance: 0,
                ..observation()
            },
        })
        .unwrap();
    storage.commit(&hold).unwrap();
    let before = storage.snapshot().unwrap();
    context(&d, sponsor(), 20, vec![]);
    assert_eq!(
        dispatch(&d, &locator(&d, &a)).unwrap().state,
        AdmissionState::Dispatched
    );
    assert!(get_created_receipts().is_empty());
    context(&d, d.marketplace.clone(), 20, valid_results(&d, &a));
    assert_eq!(complete(&d, 1, 1).unwrap(), VerificationOutcome::Active);
    assert_eq!(storage.snapshot().unwrap(), before);
    assert_eq!(
        storage
            .reservation_remaining(a.reservation().unwrap())
            .unwrap(),
        0
    );
}
