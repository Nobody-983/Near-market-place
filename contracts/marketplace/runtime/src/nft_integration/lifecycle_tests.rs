use super::*;
use crate::nft_integration::lifecycle::Lifecycle;

pub(super) fn sponsor() -> AccountId {
    account("sponsor.testnet")
}
pub(super) const FUNDS: u128 = 1_000_000_000_000_000_000_000_000;
pub(super) fn funded_setup(d: &DeploymentDomain) {
    setup_domain_with_owner(d, Some(sponsor()));
    let mut repo = StorageRepository::open(HostStore, d.clone()).unwrap();
    // Synthetic prepaid credit. No production funding/evidence API is implied.
    let after = observation();
    let batch = repo
        .prepare(StorageCommand::Fund {
            credit: [199; 32],
            owner: sponsor(),
            amount: 5 * FUNDS,
            before: BalanceObservation {
                account_balance: after.account_balance - 5 * FUNDS,
                ..after
            },
            after,
        })
        .unwrap();
    repo.commit(&batch).unwrap();
    context(d, sponsor(), 10, vec![]);
}
pub(super) fn candidate(d: &DeploymentDomain, t: ListingTerms) -> Admission {
    let p = Repository::open(HostStore, d.clone())
        .unwrap()
        .prepare(OfflineCommand::Listing {
            terms: t,
            now_ns: 10,
            budgets: budgets(),
        })
        .unwrap();
    Admission {
        operation: p.operation().unwrap().clone(),
        storage_owner: sponsor(),
        creation_bytes: 32_768,
        local_gas: Gas::from_tgas(30).as_gas(),
    }
}
pub(super) fn create(d: &DeploymentDomain, a: &Admission) -> Result<AdmissionStatus> {
    create_admission(
        d,
        &a.operation.canonical_bytes().unwrap(),
        a.storage_owner.clone(),
        FUNDS,
        a.creation_bytes,
        a.local_gas,
    )
}
fn release(d: &DeploymentDomain, a: &Admission) -> Result<AdmissionStatus> {
    let (g, t) = a.operation.verification().unwrap();
    release_admission(d, t.token.token(), &t.seller, t.approval_id, g.value())
}

#[test]
fn unverified_maximum_release_and_invalidation_preserve_replay_without_poisoning() {
    for invalidate in [false, true] {
        let d = domain();
        funded_setup(&d);
        let bad = candidate(&d, terms(&d, u64::MAX));
        create(&d, &bad).unwrap();
        if invalidate {
            dispatch(&d, &super::dispatch_tests::locator(&d, &bad)).unwrap();
            context(
                &d,
                d.marketplace.clone(),
                20,
                vec![PromiseResult::Failed, PromiseResult::Failed],
            );
            assert_eq!(complete(&d, 1, 1).unwrap(), VerificationOutcome::Invalid);
        } else {
            release(&d, &bad).unwrap();
        }
        context(&d, sponsor(), 21, vec![]);
        let retained = create(&d, &bad).unwrap();
        assert!(matches!(
            retained.state,
            AdmissionState::Released | AdmissionState::Completed
        ));
        assert!(dispatch(&d, &super::dispatch_tests::locator(&d, &bad)).is_err());
        let real = candidate(&d, terms(&d, 1));
        create(&d, &real).unwrap();
        let root = env::storage_read(b"STATE");
        context(&d, d.marketplace.clone(), 22, valid_results(&d, &bad));
        assert_eq!(complete(&d, 1, 1).unwrap(), VerificationOutcome::Ignored);
        assert_eq!(env::storage_read(b"STATE"), root);
        context(&d, sponsor(), 23, vec![]);
        dispatch(&d, &super::dispatch_tests::locator(&d, &real)).unwrap();
        context(&d, d.marketplace.clone(), 24, valid_results(&d, &real));
        assert_eq!(complete(&d, 2, 1).unwrap(), VerificationOutcome::Active);
        // Callback used preallocated watermark bytes, never another reservation.
        assert_eq!(
            StorageRepository::open(HostStore, d)
                .unwrap()
                .reservation_status(real.reservation().unwrap())
                .unwrap()
                .2,
            3
        );
    }
}
#[test]
fn owner_authority_requires_predecessor_own_credit_zero_value_and_no_results() {
    let d = setup();
    let a = candidate(&d, terms(&d, 1));
    context(&d, sponsor(), 10, vec![]);
    assert_eq!(create(&d, &a).unwrap_err(), IntegrationError::Unauthorized);
    funded_setup(&d);
    let before = env::storage_read(b"STATE");
    context(&d, account("attacker.testnet"), 10, vec![]);
    assert_eq!(create(&d, &a).unwrap_err(), IntegrationError::Unauthorized);
    context(&d, sponsor(), 10, vec![]);
    let mut wrong = candidate(&d, terms(&d, 1));
    wrong.storage_owner = account("seller.testnet");
    assert_eq!(
        create(&d, &wrong).unwrap_err(),
        IntegrationError::Unauthorized
    );
    context(
        &d,
        sponsor(),
        10,
        vec![PromiseResult::Successful(b"true".to_vec())],
    );
    assert_eq!(create(&d, &a).unwrap_err(), IntegrationError::Context);
    let mut c = VMContextBuilder::new();
    c.current_account_id(d.marketplace.clone())
        .predecessor_account_id(sponsor())
        .attached_deposit(NearToken::from_yoctonear(1));
    testing_env!(c.build());
    assert_eq!(create(&d, &a).unwrap_err(), IntegrationError::AttachedValue);
    assert_eq!(env::storage_read(b"STATE"), before);
}

#[test]
fn measured_creation_and_unused_release_preserve_attribution_and_replay_barriers() {
    let d = domain();
    funded_setup(&d);
    let mut t = terms(&d, u64::MAX);
    t.token = TokenKey::new(d.nft.clone(), "x".repeat(1024)).unwrap();
    let a = candidate(&d, t);
    let repo = StorageRepository::open(HostStore, d.clone()).unwrap();
    let before = repo.snapshot().unwrap();
    let usage = env::storage_usage();
    let status = create(&d, &a).unwrap();
    assert_eq!(status.state, AdmissionState::Ready);
    assert_eq!(status.generation.0, 1);
    assert!(get_created_receipts().is_empty());
    let cost = u128::from(env::storage_usage() - usage) * env::storage_byte_cost().as_yoctonear();
    let after = repo.snapshot().unwrap();
    assert!(cost > 0);
    assert_eq!(before.0, after.0);
    assert_eq!(after.1.totals().staked, cost);
    assert_eq!(after.1.totals().reserved, FUNDS - cost);
    assert_eq!(after.1.totals().available, 4 * FUNDS);
    assert_eq!(
        repo.reservation_status(a.reservation().unwrap()).unwrap(),
        (sponsor(), FUNDS - cost, 1)
    );
    let root = env::storage_read(b"STATE");
    assert_eq!(create(&d, &a).unwrap(), status);
    assert_eq!(env::storage_read(b"STATE"), root);
    context(&d, sponsor(), 20, vec![]); // a new invocation, only persistent data
    let usage = env::storage_usage();
    assert_eq!(release(&d, &a).unwrap().state, AdmissionState::Released);
    assert_eq!(env::storage_usage(), usage);
    assert_eq!(listing(&d, 1), ListingState::Cancelled);
    let final_head = repo.snapshot().unwrap();
    assert_eq!(final_head.0, before.0);
    assert_eq!(final_head.1.totals().funded, 5 * FUNDS);
    assert_eq!(final_head.1.totals().staked, cost);
    assert_eq!(final_head.1.totals().reserved, 0);
    assert_eq!(final_head.1.totals().available, 5 * FUNDS - cost);
    assert!(repo
        .reservation_remaining(a.reservation().unwrap())
        .is_err());
    let root = env::storage_read(b"STATE");
    assert_eq!(release(&d, &a).unwrap().state, AdmissionState::Released);
    assert_eq!(create(&d, &a).unwrap().state, AdmissionState::Released);
    assert_eq!(env::storage_read(b"STATE"), root);
    context(&d, d.nft.clone(), 20, vec![]);
    let (_, t) = a.operation.verification().unwrap();
    assert!(begin(
        &d,
        t.token.token().into(),
        t.seller.clone(),
        t.approval_id,
        &message(t)
    )
    .is_err());
}

#[test]
fn concurrent_admissions_allocate_once_and_stale_requests_cannot_replace_them() {
    let d = domain();
    funded_setup(&d);
    let a = candidate(&d, terms(&d, 1));
    let mut t = terms(&d, 1);
    t.token = TokenKey::new(d.nft.clone(), "other".into()).unwrap();
    let stale = candidate(&d, t.clone()); // same proposed global generation
    create(&d, &a).unwrap();
    let root = env::storage_read(b"STATE");
    assert_eq!(create(&d, &stale).unwrap_err(), IntegrationError::Context);
    assert_eq!(env::storage_read(b"STATE"), root);
    assert!(Repository::open(HostStore, d.clone())
        .unwrap()
        .prepare(OfflineCommand::Listing {
            terms: terms(&d, 2),
            now_ns: 20,
            budgets: budgets()
        })
        .is_err());
    let next = candidate(&d, t);
    assert_eq!(create(&d, &next).unwrap().generation.0, 2);
    let mut changed = Admission {
        local_gas: a.local_gas + 1,
        ..a
    };
    assert_eq!(
        create(&d, &changed).unwrap_err(),
        IntegrationError::Admission
    );
    changed.local_gas -= 1;
    release(&d, &changed).unwrap();
    let fresh = candidate(&d, terms(&d, 2));
    assert_eq!(create(&d, &fresh).unwrap().generation.0, 3);
    context(&d, d.marketplace.clone(), 30, valid_results(&d, &changed));
    let root = env::storage_read(b"STATE");
    assert_eq!(complete(&d, 1, 1).unwrap(), VerificationOutcome::Ignored);
    assert_eq!(env::storage_read(b"STATE"), root);
    assert_eq!(listing(&d, 3), ListingState::Verifying);
}

#[test]
fn managed_dispatch_is_once_and_pending_missing_or_completed_callbacks_cannot_release() {
    let d = domain();
    funded_setup(&d);
    let a = candidate(&d, terms(&d, 7));
    create(&d, &a).unwrap();
    context(&d, d.marketplace.clone(), 20, valid_results(&d, &a));
    assert_eq!(complete(&d, 1, 1).unwrap_err(), IntegrationError::Context); // never dispatched
    let p = start(&d, &a);
    let storage = StorageRepository::open(HostStore, d.clone()).unwrap();
    let remaining = storage
        .reservation_status(a.reservation().unwrap())
        .unwrap();
    assert_eq!(remaining.2, 2);
    context(&d, sponsor(), 20, vec![]);
    assert_eq!(release(&d, &a).unwrap_err(), IntegrationError::Context);
    assert_eq!(create(&d, &a).unwrap().state, AdmissionState::Dispatched); // idempotent, no send
    assert!(get_created_receipts().is_empty());
    context(&d, d.marketplace.clone(), 20, vec![]);
    assert_eq!(
        complete(&d, 1, 1).unwrap_err(),
        IntegrationError::ResultCount
    );
    assert_eq!(
        storage
            .reservation_status(a.reservation().unwrap())
            .unwrap(),
        remaining
    );
    context(&d, sponsor(), 5000, vec![]); // timeout isn't closure
    assert!(release(&d, &a).is_err());
    context(&d, d.marketplace.clone(), 25, valid_results(&d, &a));
    assert_eq!(
        complete(&d, p.generation.0, 1).unwrap(),
        VerificationOutcome::Active
    );
    let after = storage
        .reservation_status(a.reservation().unwrap())
        .unwrap();
    assert_eq!((after.1, after.2), (remaining.1, 3));
    assert_eq!(complete(&d, 1, 1).unwrap(), VerificationOutcome::Ignored);
    context(&d, sponsor(), 30, vec![]);
    assert!(release(&d, &a).is_err()); // used reservation remains protected
}

#[test]
fn mismatched_identity_inputs_corrupt_intents_and_exhausted_credit_do_not_write() {
    let d = domain();
    funded_setup(&d);
    let a = candidate(&d, terms(&d, 1));
    let before = env::storage_read(b"STATE");
    let mut bytes = a.operation.canonical_bytes().unwrap();
    bytes.push(0);
    assert!(create_admission(&d, &bytes, sponsor(), FUNDS, 32_768, a.local_gas).is_err());
    let mut other = d.clone();
    other.nft = account("different.testnet");
    assert!(create_admission(
        &other,
        &a.operation.canonical_bytes().unwrap(),
        sponsor(),
        FUNDS,
        32_768,
        a.local_gas
    )
    .is_err());
    assert!(create_admission(
        &d,
        &a.operation.canonical_bytes().unwrap(),
        sponsor(),
        1,
        32_768,
        a.local_gas
    )
    .is_err());
    assert!(create_admission(
        &d,
        &a.operation.canonical_bytes().unwrap(),
        sponsor(),
        6 * FUNDS,
        32_768,
        a.local_gas
    )
    .is_err());
    assert!(create_admission(
        &d,
        &a.operation.canonical_bytes().unwrap(),
        sponsor(),
        u128::MAX,
        u64::MAX,
        a.local_gas
    )
    .is_err());
    assert_eq!(env::storage_read(b"STATE"), before);
    create(&d, &a).unwrap();
    context(&d, d.nft.clone(), 20, vec![]);
    for (token, seller, approval) in [
        ("wrong", account("seller.testnet"), 1),
        ("art", sponsor(), 1),
        ("art", account("seller.testnet"), 2),
    ] {
        assert!(begin(&d, token.into(), seller, approval, &message(&terms(&d, 1))).is_err());
    }
    context(&d, sponsor(), 20, vec![]);
    assert!(release_admission(&d, "art", &account("seller.testnet"), 1, 2).is_err());
    assert_eq!(listing(&d, 1), ListingState::Verifying);
}

#[test]
fn missing_managed_lifecycle_never_downgrades_to_legacy_and_legacy_release_is_disabled() {
    let d = domain();
    funded_setup(&d);
    let a = candidate(&d, terms(&d, 1));
    create(&d, &a).unwrap();
    let mut key = b"nmp:runtime:admission-life:v1:".to_vec();
    key.extend(a.reservation().unwrap());
    let good = env::storage_read(&key).unwrap();
    env::storage_remove(&key);
    assert!(Lifecycle::load(&d, &a).is_err());
    assert!(release(&d, &a).is_err());
    env::storage_write(&key, &[0; 89]);
    assert!(Lifecycle::load(&d, &a).is_err());
    env::storage_write(&key, &good);
    assert!(release(&d, &a).is_ok());
    let legacy = provision(&d, terms(&d, 2), FUNDS);
    assert!(release(&d, &legacy).is_err());
}

pub(super) fn protected_fixture(d: &DeploymentDomain) -> (Admission, Vec<RecordKey>) {
    use near_marketplace_foundation::types::ClaimId;
    funded_setup(d);
    let mut keys = vec![];
    // Synthetic sale histories: an unresolved transfer plus a delivered sale
    // with allocated seller/fee claims. No runtime purchase method is involved.
    for (n, result) in [(1, CallbackResult::Unknown), (2, CallbackResult::Succeeded)] {
        let mut t = terms(d, 1);
        t.token = TokenKey::new(d.nft.clone(), format!("sale-{n}")).unwrap();
        let a = provision(d, t.clone(), FUNDS);
        let p = start(d, &a);
        context(d, d.marketplace.clone(), 20, valid_results(d, &a));
        complete(d, p.generation.0, 1).unwrap();
        let g = Generation::from_value(p.generation.0).unwrap();
        let mut repo = Repository::open(HostStore, d.clone()).unwrap();
        let purchase = repo
            .prepare(OfflineCommand::Purchase {
                generation: g,
                token: t.token,
                buyer: account("buyer.testnet"),
                exact_price: 1000,
                now_ns: 30,
                budgets: Budgets {
                    first_call_gas: 30,
                    second_call_gas: 0,
                    callback_gas: 30,
                    max_result_bytes: 4,
                    call_abi: wire::hash(wire::TRANSFER_ABI),
                    callback_abi: [77; 32],
                },
            })
            .unwrap();
        repo.commit(&purchase).unwrap();
        let sale = purchase.sale().unwrap();
        let outcome = repo
            .prepare(OfflineCommand::TransferResult {
                sale,
                generation: g,
                attempt: purchase.attempt().unwrap(),
                result,
            })
            .unwrap();
        repo.commit(&outcome).unwrap();
        keys.extend([
            RecordKey::Sale(sale),
            RecordKey::Principal(sale),
            RecordKey::Transfer(sale),
            RecordKey::Listing(g),
            RecordKey::Verification(g),
        ]);
    }
    keys.extend([
        RecordKey::Claim(ClaimId::from_value(1).unwrap()),
        RecordKey::Claim(ClaimId::from_value(2).unwrap()),
    ]);
    context(d, sponsor(), 30, vec![]);
    let ready = candidate(d, terms(d, 2));
    create(d, &ready).unwrap();
    let mut storage = StorageRepository::open(HostStore, d.clone()).unwrap();
    let after = observation();
    let u = storage
        .prepare(StorageCommand::ReceiveUnattributed {
            credit: [198; 32],
            amount: 100,
            before: BalanceObservation {
                account_balance: after.account_balance - 100,
                ..after
            },
            after,
        })
        .unwrap();
    storage.commit(&u).unwrap();
    let h = storage
        .prepare(StorageCommand::CheckHealth {
            balance: BalanceObservation {
                account_balance: 0,
                ..observation()
            },
        })
        .unwrap();
    storage.commit(&h).unwrap();
    (ready, keys)
}

#[test]
fn release_preserves_unknown_sales_claims_unattributed_funds_and_sticky_holds() {
    let d = domain();
    let (ready, keys) = protected_fixture(&d);
    let storage = StorageRepository::open(HostStore, d.clone()).unwrap();
    let before = storage.snapshot().unwrap();
    let records: Vec<_> = keys
        .iter()
        .map(|key| {
            let key = key.storage_key(&d).unwrap();
            let value = env::storage_read(&key).unwrap();
            (key, value)
        })
        .collect();
    assert!(before.0.financial_hold() && before.1.financial_hold());
    assert!(release_admission(&d, "sale-1", &account("seller.testnet"), 1, 1).is_err());
    release(&d, &ready).unwrap();
    let after = storage.snapshot().unwrap();
    assert_eq!(after.0, before.0);
    assert_eq!(after.1.unattributed(), 100);
    assert!(after.1.financial_hold());
    for (key, value) in records {
        assert_eq!(env::storage_read(&key).unwrap(), value);
    }
    assert_eq!(listing(&d, 1), ListingState::Review);
}

#[test]
fn managed_revocation_reapproval_wrong_owner_and_duplicate_dispatch_preserve_exact_attempt() {
    for scenario in 0..3 {
        let d = domain();
        funded_setup(&d);
        let a = candidate(&d, terms(&d, 7));
        create(&d, &a).unwrap();
        start(&d, &a);
        let root = env::storage_read(b"STATE");
        let (_, t) = a.operation.verification().unwrap();
        assert!(begin(&d, "art".into(), t.seller.clone(), 7, &message(t)).is_err());
        assert_eq!(env::storage_read(b"STATE"), root);
        let mut views = valid_results(&d, &a);
        match scenario {
            0 => views[1] = PromiseResult::Successful(b"false".to_vec()),
            1 => views[0] = PromiseResult::Successful(token_result(&d, &terms(&d, 8))),
            _ => {
                let mut wrong = terms(&d, 7);
                wrong.seller = account("attacker.testnet");
                views[0] = PromiseResult::Successful(token_result(&d, &wrong));
            }
        }
        context(&d, d.marketplace.clone(), 20, views);
        assert_eq!(complete(&d, 1, 1).unwrap(), VerificationOutcome::Invalid);
        context(&d, sponsor(), 20, vec![]);
        assert!(release(&d, &a).is_err());
        assert_eq!(create(&d, &a).unwrap().state, AdmissionState::Completed);
    }
}
