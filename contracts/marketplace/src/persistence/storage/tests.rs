use super::*;
use crate::ledger::{BalanceObservation, LedgerTotals};
use crate::persistence::{ArtifactDomain, Budgets, OfflineCommand, Repository};
use crate::state::ListingTerms;
use crate::types::{FeeTerms, TokenKey};

fn admission_reserve(r: &mut StorageRepository<MemoryStore>) {
    run(
        r,
        StorageCommand::ReserveAdmission {
            reservation: [2; 32],
            operation: [9; 32],
            owner: owner(),
            amount: 800,
            balance: balance(1100, 100),
        },
    );
}
fn admission_release() -> StorageCommand {
    StorageCommand::ReleaseAdmission {
        reservation: [2; 32],
        operation: [9; 32],
        balance: balance(1100, 400),
    }
}

#[test]
fn guarded_release_retains_stake_and_tombstone_and_never_spends_other_accounts() {
    let mut r = funded();
    admission_reserve(&mut r);
    unchanged(&r, admission_release()); // creation storage has not been charged
    run(
        &mut r,
        StorageCommand::Consume {
            reservation: [2; 32],
            nonce: 1,
            before: balance(1100, 100),
            after: balance(1100, 400),
        },
    );
    let before = r.snapshot().unwrap();
    unchanged(
        &r,
        StorageCommand::ReleaseAdmission {
            reservation: [2; 32],
            operation: [8; 32],
            balance: balance(1100, 400),
        },
    );
    let raw = r
        .store
        .read(&Key::Reservation([2; 32]).bytes(&domain()).unwrap())
        .unwrap();
    run(&mut r, admission_release());
    assert_eq!(r.snapshot().unwrap().0, before.0);
    let total = r.snapshot().unwrap().1.totals();
    assert_eq!(
        (total.funded, total.available, total.reserved, total.staked),
        (1000, 700, 0, 300)
    );
    assert_eq!(
        raw,
        r.store
            .read(&Key::Reservation([2; 32]).bytes(&domain()).unwrap())
            .unwrap()
    );
    assert_eq!(r.admission_guard([2; 32]).unwrap(), Some(([9; 32], true)));
    assert!(r.reservation_remaining([2; 32]).is_err());
    assert!(r.reservation_status([2; 32]).is_err());
    unchanged(&r, admission_release());
    unchanged(
        &r,
        StorageCommand::Consume {
            reservation: [2; 32],
            nonce: 2,
            before: balance(1100, 400),
            after: balance(1100, 400),
        },
    );
    unchanged(
        &r,
        StorageCommand::Reserve {
            reservation: [2; 32],
            owner: owner(),
            amount: 1,
            balance: balance(1100, 400),
        },
    );
}

#[test]
fn legacy_and_dispatched_reservations_have_no_unused_release_capability() {
    for managed in [false, true] {
        let mut r = funded();
        if managed {
            admission_reserve(&mut r);
        } else {
            reserve(&mut r);
        }
        run(
            &mut r,
            StorageCommand::Consume {
                reservation: [2; 32],
                nonce: 1,
                before: balance(1100, 100),
                after: balance(1100, 400),
            },
        );
        if managed {
            run(
                &mut r,
                StorageCommand::Consume {
                    reservation: [2; 32],
                    nonce: 2,
                    before: balance(1100, 400),
                    after: balance(1100, 400),
                },
            );
        }
        unchanged(&r, admission_release());
    }
}

#[test]
fn reserve_and_release_batches_are_bounded_atomic_and_restart_safe() {
    let mut r = funded();
    let reserve = r
        .prepare(StorageCommand::ReserveAdmission {
            reservation: [2; 32],
            operation: [9; 32],
            owner: owner(),
            amount: 800,
            balance: balance(1100, 100),
        })
        .unwrap();
    assert_eq!(reserve.record_count(), 5);
    for step in 1..=reserve.writes.len() {
        let before = r.store.bytes.clone();
        r.store.fail_after = Some(step);
        assert!(r.commit(&reserve).is_err());
        assert_eq!(r.store.bytes, before); // MemoryStore atomicity only
    }
    r.store.fail_after = None;
    r.commit(&reserve).unwrap();
    run(
        &mut r,
        StorageCommand::Consume {
            reservation: [2; 32],
            nonce: 1,
            before: balance(1100, 100),
            after: balance(1100, 400),
        },
    );
    let release = r.prepare(admission_release()).unwrap();
    assert_eq!(release.record_count(), 5);
    for step in 1..=release.writes.len() {
        let before = r.store.bytes.clone();
        r.store.fail_after = Some(step);
        assert!(r.commit(&release).is_err());
        assert_eq!(r.store.bytes, before);
    }
    r.store.fail_after = None;
    r.commit(&release).unwrap();
    let mut restarted = StorageRepository::open(r.into_store(), domain()).unwrap();
    assert!(restarted.commit(&release).is_err());
    unchanged(&restarted, admission_release());
}

#[test]
fn guard_corruption_overflow_and_insolvency_fail_closed() {
    let mut r = funded();
    admission_reserve(&mut r);
    run(
        &mut r,
        StorageCommand::Consume {
            reservation: [2; 32],
            nonce: 1,
            before: balance(1100, 100),
            after: balance(1100, 400),
        },
    );
    unchanged(
        &r,
        StorageCommand::ReleaseAdmission {
            reservation: [2; 32],
            operation: [9; 32],
            balance: balance(1099, 400),
        },
    );
    let key = Key::AdmissionGuard([2; 32]);
    let raw_key = key.bytes(&domain()).unwrap();
    let bytes = r.store.read(&raw_key).unwrap().unwrap();
    for n in 0..bytes.len() {
        let mut bad = bytes.clone();
        bad[n] ^= 1;
        assert!(decode(&bad, &domain(), &key).is_err());
    }
    let mut record = decode(&bytes, &domain(), &key).unwrap();
    record.revision = u64::MAX;
    r.store
        .bytes
        .insert(raw_key, record.encode(&domain()).unwrap());
    unchanged(&r, admission_release());
}

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
fn owner() -> AccountId {
    "payer.testnet".parse().unwrap()
}
fn balance(a: u128, s: u128) -> BalanceObservation {
    BalanceObservation {
        account_balance: a,
        storage_stake: s,
        ..Default::default()
    }
}
fn run(repo: &mut StorageRepository<MemoryStore>, cmd: StorageCommand) {
    let b = repo.prepare(cmd).unwrap();
    repo.commit(&b).unwrap();
}
fn funded() -> StorageRepository<MemoryStore> {
    let mut r = StorageRepository::new_offline(domain()).unwrap();
    run(
        &mut r,
        StorageCommand::Fund {
            credit: [1; 32],
            owner: owner(),
            amount: 1000,
            before: balance(100, 100),
            after: balance(1100, 100),
        },
    );
    r
}

#[test]
fn funding_metadata_is_charged_once_in_fixed_width_and_survives_restart() {
    for cost in [0, 123, 1000] {
        let mut r = funded();
        let protection = r.snapshot().unwrap().0;
        let command = StorageCommand::ChargeFundingStorage {
            credit: [1; 32],
            before: balance(1100, 100),
            after: balance(1100, 100 + cost),
        };
        let batch = r.prepare(command.clone()).unwrap();
        for (key, bytes) in batch.writes() {
            assert_eq!(r.store.read(key).unwrap().unwrap().len(), bytes.len());
        }
        r.commit(&batch).unwrap();
        let r = StorageRepository::open(r.into_store(), domain()).unwrap();
        assert_eq!(r.snapshot().unwrap().0, protection);
        let t = r.account(&owner()).unwrap().unwrap();
        assert_eq!((t.funded, t.available, t.staked), (1000, 1000 - cost, cost));
        unchanged(&r, command);
    }
}

#[test]
fn funding_metadata_cannot_spend_other_credits_or_invent_balance_deltas() {
    let r = funded();
    for (credit, before, after) in [
        ([9; 32], balance(1100, 100), balance(1100, 110)),
        ([1; 32], balance(1100, 100), balance(1100, 1101)),
        ([1; 32], balance(1100, 100), balance(1100, 99)),
        ([1; 32], balance(1100, 100), balance(1200, 200)),
    ] {
        unchanged(
            &r,
            StorageCommand::ChargeFundingStorage {
                credit,
                before,
                after,
            },
        );
    }
}
fn reserve(r: &mut StorageRepository<MemoryStore>) {
    run(
        r,
        StorageCommand::Reserve {
            reservation: [2; 32],
            owner: owner(),
            amount: 800,
            balance: balance(1100, 100),
        },
    );
}
fn unchanged(r: &StorageRepository<MemoryStore>, cmd: StorageCommand) {
    let before = r.store.bytes.clone();
    assert!(r.prepare(cmd).is_err());
    assert_eq!(r.store.bytes, before);
}
fn begin_withdrawal(r: &mut StorageRepository<MemoryStore>) -> [u8; 32] {
    run(
        r,
        StorageCommand::Withdraw {
            withdrawal: [3; 32],
            owner: owner(),
            amount: 200,
            balance: balance(1100, 100),
        },
    );
    let c = r.withdrawal([3; 32]).unwrap().commitment();
    run(
        r,
        StorageCommand::Debit {
            withdrawal: [3; 32],
            commitment: c,
            before: balance(1100, 100),
            after: balance(900, 100),
        },
    );
    c
}

#[test]
fn funding_reservations_and_withdrawals_are_disjoint_and_replay_protected() {
    let mut r = funded();
    reserve(&mut r);
    assert_eq!(r.account(&owner()).unwrap().unwrap().available, 200);
    unchanged(
        &r,
        StorageCommand::Reserve {
            reservation: [2; 32],
            owner: owner(),
            amount: 1,
            balance: balance(1100, 100),
        },
    );
    unchanged(
        &r,
        StorageCommand::Withdraw {
            withdrawal: [3; 32],
            owner: owner(),
            amount: 201,
            balance: balance(1100, 100),
        },
    );
    let commitment = begin_withdrawal(&mut r);
    let w = r.withdrawal([3; 32]).unwrap();
    assert_eq!(w.recipient(), &owner());
    assert_eq!(w.amount(), 200);
    let (l, s) = r.snapshot().unwrap();
    s.check_backing(l, balance(900, 100)).unwrap();
    assert_eq!(s.totals.queued, 0);
    assert_eq!(s.totals.in_flight, 200);
    assert_eq!(s.totals.reserved, 800);
    unchanged(
        &r,
        StorageCommand::Debit {
            withdrawal: [3; 32],
            commitment,
            before: balance(1100, 100),
            after: balance(900, 100),
        },
    );
    unchanged(
        &r,
        StorageCommand::Fund {
            credit: [1; 32],
            owner: owner(),
            amount: 1000,
            before: balance(1100, 100),
            after: balance(2100, 100),
        },
    );
}

#[test]
fn measured_consumption_moves_c_to_s_once_including_exact_reserve_exhaustion() {
    let mut r = funded();
    reserve(&mut r);
    let command = StorageCommand::Consume {
        reservation: [2; 32],
        nonce: 1,
        before: balance(1100, 100),
        after: balance(1100, 400),
    };
    run(&mut r, command.clone());
    unchanged(&r, command);
    assert_eq!(r.reservation_remaining([2; 32]).unwrap(), 500);
    let (l, s) = r.snapshot().unwrap();
    s.check_backing(l, balance(1100, 400)).unwrap();
    assert_eq!(s.totals.staked, 300);
    assert_eq!(s.totals.available, 200);
    unchanged(
        &r,
        StorageCommand::Consume {
            reservation: [2; 32],
            nonce: 3,
            before: balance(1100, 400),
            after: balance(1100, 500),
        },
    );
    run(
        &mut r,
        StorageCommand::Consume {
            reservation: [2; 32],
            nonce: 2,
            before: balance(1100, 400),
            after: balance(1100, 900),
        },
    );
    assert_eq!(r.reservation_remaining([2; 32]).unwrap(), 0);
    unchanged(
        &r,
        StorageCommand::Consume {
            reservation: [2; 32],
            nonce: 3,
            before: balance(1100, 900),
            after: balance(1100, 901),
        },
    );
    // Fixed-size completion updates consume a nonce, but no reservation twice.
    run(
        &mut r,
        StorageCommand::Consume {
            reservation: [2; 32],
            nonce: 3,
            before: balance(1100, 900),
            after: balance(1100, 900),
        },
    );
    let reopened = StorageRepository::open(r.into_store(), domain()).unwrap();
    assert_eq!(reopened.snapshot().unwrap().1.totals.staked, 800);
}

#[test]
fn failed_expenditure_shortfall_and_fabricated_balance_changes_leave_no_writes() {
    let mut r = funded();
    reserve(&mut r);
    for (before, after) in [
        (balance(1100, 100), balance(1100, 901)),
        (balance(1000, 100), balance(1000, 200)),
        (balance(1100, 100), balance(1000, 200)),
        (balance(1100, 100), balance(1100, 99)),
    ] {
        unchanged(
            &r,
            StorageCommand::Consume {
                reservation: [2; 32],
                nonce: 1,
                before,
                after,
            },
        );
    }
    let mut after = balance(1200, 100);
    after.unattributed = 100;
    unchanged(
        &r,
        StorageCommand::Fund {
            credit: [5; 32],
            owner: owner(),
            amount: 100,
            before: balance(1100, 100),
            after,
        },
    );
    // Crediting a storage liability cannot repair an existing cash deficit.
    unchanged(
        &r,
        StorageCommand::Fund {
            credit: [5; 32],
            owner: owner(),
            amount: 100,
            before: balance(1000, 100),
            after: balance(1100, 100),
        },
    );
}

#[test]
fn unknown_withdrawal_and_early_refund_remain_locked_until_exact_conclusive_result() {
    let mut r = funded();
    reserve(&mut r);
    let c = begin_withdrawal(&mut r);
    run(
        &mut r,
        StorageCommand::WithdrawalResult {
            withdrawal: [3; 32],
            commitment: c,
            result: CallbackResult::Unknown,
        },
    );
    run(
        &mut r,
        StorageCommand::ReceiveUnattributed {
            credit: [4; 32],
            amount: 200,
            before: balance(900, 100),
            after: balance(1100, 100),
        },
    );
    let attribute = StorageCommand::AttributeWithdrawalRefund {
        credit: [4; 32],
        withdrawal: [3; 32],
        commitment: c,
    };
    unchanged(&r, attribute.clone());
    unchanged(
        &r,
        StorageCommand::Withdraw {
            withdrawal: [9; 32],
            owner: owner(),
            amount: 1,
            balance: balance(1100, 100),
        },
    );
    assert_eq!(
        r.withdrawal([3; 32]).unwrap().state(),
        WithdrawalState::Unknown
    );
    run(
        &mut r,
        StorageCommand::WithdrawalResult {
            withdrawal: [3; 32],
            commitment: c,
            result: CallbackResult::Failed,
        },
    );
    let (l, s) = r.snapshot().unwrap();
    assert!(s.check_backing(l, balance(1100, 100)).is_err()); // U still protected
    run(&mut r, attribute.clone());
    unchanged(&r, attribute);
    let (l, s) = r.snapshot().unwrap();
    s.check_backing(l, balance(1100, 100)).unwrap();
    assert_eq!(s.totals.available, 200);
    assert_eq!(s.totals.funded, 1000);
    assert_eq!(s.unattributed, 0);
    unchanged(
        &r,
        StorageCommand::WithdrawalResult {
            withdrawal: [3; 32],
            commitment: c,
            result: CallbackResult::Succeeded,
        },
    );
}

#[test]
fn failure_restores_debt_not_cash_and_payment_success_cannot_claim_a_refund() {
    for result in [CallbackResult::Failed, CallbackResult::Succeeded] {
        let mut r = funded();
        reserve(&mut r);
        let c = begin_withdrawal(&mut r);
        run(
            &mut r,
            StorageCommand::WithdrawalResult {
                withdrawal: [3; 32],
                commitment: c,
                result,
            },
        );
        if result == CallbackResult::Failed {
            unchanged(
                &r,
                StorageCommand::Withdraw {
                    withdrawal: [9; 32],
                    owner: owner(),
                    amount: 200,
                    balance: balance(900, 100),
                },
            );
        }
        run(
            &mut r,
            StorageCommand::ReceiveUnattributed {
                credit: [4; 32],
                amount: 199,
                before: balance(900, 100),
                after: balance(1099, 100),
            },
        );
        unchanged(
            &r,
            StorageCommand::AttributeWithdrawalRefund {
                credit: [4; 32],
                withdrawal: [3; 32],
                commitment: c,
            },
        );
        unchanged(
            &r,
            StorageCommand::AttributeWithdrawalRefund {
                credit: [4; 32],
                withdrawal: [3; 32],
                commitment: [8; 32],
            },
        );
        assert_eq!(r.snapshot().unwrap().1.unattributed, 199);
    }
}

#[test]
fn financial_hold_is_sticky_but_funded_completion_and_genuine_results_preserve_debt() {
    let mut r = funded();
    reserve(&mut r);
    let c = begin_withdrawal(&mut r);
    run(
        &mut r,
        StorageCommand::CheckHealth {
            balance: balance(899, 100),
        },
    );
    assert!(r.snapshot().unwrap().1.financial_hold());
    assert!(r.snapshot().unwrap().0.financial_hold());
    run(
        &mut r,
        StorageCommand::WithdrawalResult {
            withdrawal: [3; 32],
            commitment: c,
            result: CallbackResult::Unknown,
        },
    );
    run(
        &mut r,
        StorageCommand::Consume {
            reservation: [2; 32],
            nonce: 1,
            before: balance(900, 100),
            after: balance(900, 400),
        },
    );
    run(
        &mut r,
        StorageCommand::CheckHealth {
            balance: balance(900, 400),
        },
    );
    assert!(r.snapshot().unwrap().1.financial_hold());
    unchanged(
        &r,
        StorageCommand::Reserve {
            reservation: [9; 32],
            owner: owner(),
            amount: 1,
            balance: balance(900, 400),
        },
    );
    run(
        &mut r,
        StorageCommand::WithdrawalResult {
            withdrawal: [3; 32],
            commitment: c,
            result: CallbackResult::Succeeded,
        },
    );
    assert!(r.snapshot().unwrap().1.financial_hold());
    assert_eq!(r.snapshot().unwrap().1.totals.withdrawn, 200);
}

#[test]
fn principal_unknown_sale_and_all_v1_records_survive_storage_consumption() {
    let r = funded();
    let mut base = Repository::open(r.into_store(), domain()).unwrap();
    let terms = ListingTerms {
        token: TokenKey::new(domain().nft, "art".into()).unwrap(),
        seller: "seller.testnet".parse().unwrap(),
        approval_id: 1,
        price: 100,
        expires_at_ns: 100,
        fees: FeeTerms::new(Some(0), Some(0), Some("fees.testnet".parse().unwrap())).unwrap(),
        policy_version: 1,
        policy_hash: [5; 32],
        correlation: [6; 32],
    };
    let budgets = Budgets {
        first_call_gas: 101,
        second_call_gas: 102,
        callback_gas: 103,
        max_result_bytes: 1000,
        call_abi: [7; 32],
        callback_abi: [8; 32],
    };
    let p = base
        .prepare(OfflineCommand::Listing {
            terms: terms.clone(),
            now_ns: 1,
            budgets,
        })
        .unwrap();
    base.commit(&p).unwrap();
    let g = p.generation().unwrap();
    let p = base
        .prepare(OfflineCommand::Verify {
            generation: g,
            attempt: p.attempt().unwrap(),
            owner: Some(terms.seller),
            approval: Some(1),
            now_ns: 2,
        })
        .unwrap();
    base.commit(&p).unwrap();
    let p = base
        .prepare(OfflineCommand::Purchase {
            generation: g,
            token: terms.token,
            buyer: "buyer.testnet".parse().unwrap(),
            exact_price: 100,
            now_ns: 3,
            budgets: Budgets {
                second_call_gas: 0,
                ..budgets
            },
        })
        .unwrap();
    base.commit(&p).unwrap();
    let p = base
        .prepare(OfflineCommand::TransferResult {
            sale: p.sale().unwrap(),
            generation: g,
            attempt: p.attempt().unwrap(),
            result: CallbackResult::Unknown,
        })
        .unwrap();
    base.commit(&p).unwrap();
    let mut r = StorageRepository::open(base.into_store(), domain()).unwrap();
    let protection = r.snapshot().unwrap().0;
    let old: BTreeMap<_, _> = r
        .store
        .bytes
        .iter()
        .filter(|(k, _)| k.starts_with(b"nmp:r:"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    run(
        &mut r,
        StorageCommand::Reserve {
            reservation: [2; 32],
            owner: owner(),
            amount: 800,
            balance: balance(1200, 100),
        },
    );
    run(
        &mut r,
        StorageCommand::Consume {
            reservation: [2; 32],
            nonce: 1,
            before: balance(1200, 100),
            after: balance(1200, 900),
        },
    );
    assert_eq!(r.snapshot().unwrap().0, protection);
    for (k, v) in old {
        assert_eq!(r.store.read(&k).unwrap().unwrap(), v);
    }
    assert_eq!(protection.totals.held, 100);
    // Exact storage-credit funding cannot steal the missing buyer principal.
    unchanged(
        &r,
        StorageCommand::Fund {
            credit: [9; 32],
            owner: owner(),
            amount: 100,
            before: balance(1100, 900),
            after: balance(1200, 900),
        },
    );
}

#[test]
fn legacy_claims_reserves_unattributed_cash_and_capital_are_all_protected() {
    let (mut legacy, s) = funded().snapshot().unwrap();
    legacy.totals = LedgerTotals {
        principal: 500,
        held: 100,
        available: 100,
        in_flight: 200,
        paid: 100,
        queued_payments: 50,
        reserved_operating: 75,
        contributions: 100,
        deficit_repairs: 0,
    };
    legacy.free_capital = 25;
    let b = BalanceObservation {
        account_balance: 1600,
        storage_stake: 100,
        external_queued: 40,
        storage_credits: 50,
        unattributed: 60,
    };
    s.check_backing(legacy, b).unwrap(); // 1000 + 100 + 200 + 50 + 100 + 40 + 50 + 60
    assert!(s
        .check_backing(
            legacy,
            BalanceObservation {
                account_balance: 1599,
                ..b
            }
        )
        .is_err());
}

#[test]
fn every_write_failure_rolls_back_storage_and_v1_root_in_memory_only() {
    let mut r = funded();
    reserve(&mut r);
    let batch = r
        .prepare(StorageCommand::Consume {
            reservation: [2; 32],
            nonce: 1,
            before: balance(1100, 100),
            after: balance(1100, 400),
        })
        .unwrap();
    for fail in 1..=batch.writes.len() {
        let before = r.store.bytes.clone();
        r.store.fail_after = Some(fail);
        assert!(r.commit(&batch).is_err());
        assert_eq!(r.store.bytes, before);
    }
    r.store.fail_after = None;
    r.commit(&batch).unwrap();
    assert!(r.commit(&batch).is_err());
}

#[test]
fn corrupt_lengths_checksums_totals_and_cross_domain_records_fail_closed() {
    let r = funded();
    let bytes = r.store.read(STORAGE_ROOT_KEY).unwrap().unwrap();
    for i in 0..bytes.len() {
        let mut corrupt = bytes.clone();
        corrupt[i] ^= 1;
        assert!(decode(&corrupt, &domain(), &Key::Root).is_err());
    }
    let mut e = decode(&bytes, &domain(), &Key::Root).unwrap();
    if let Value::Root(h) = &mut e.value {
        h.totals.available += 1;
    }
    let mut bad = borsh::to_vec(&e).unwrap();
    bad.extend(digest(TAG, &bad));
    assert!(decode(&bad, &domain(), &Key::Root).is_err());
    let mut d = domain();
    d.manifest = [9; 32];
    assert!(decode(&bytes, &d, &Key::Root).is_err());
    let key = Key::Account(owner());
    let mut bad = r
        .store
        .read(&key.bytes(&domain()).unwrap())
        .unwrap()
        .unwrap();
    bad.truncate(bad.len() - 32);
    bad[41..45].copy_from_slice(&u32::MAX.to_le_bytes());
    bad.extend(digest(TAG, &bad));
    assert_eq!(decode(&bad, &domain(), &key), Err(StorageError::Bounds));
    assert!(decode(
        &vec![0; MAX_STORAGE_RECORD_BYTES + 1],
        &domain(),
        &Key::Root
    )
    .is_err());
}

#[test]
fn checked_overflow_and_missing_records_cannot_reset_accounting() {
    let r = funded();
    unchanged(
        &r,
        StorageCommand::Fund {
            credit: [9; 32],
            owner: owner(),
            amount: u128::MAX,
            before: balance(1100, 100),
            after: balance(0, 100),
        },
    );
    unchanged(
        &r,
        StorageCommand::Consume {
            reservation: [9; 32],
            nonce: 1,
            before: balance(1100, 100),
            after: balance(1100, 101),
        },
    );
    let mut store = r.into_store();
    store.bytes.remove(STORAGE_ROOT_KEY);
    assert!(StorageRepository::open(store, domain()).is_err());
    let mut r = funded();
    reserve(&mut r);
    let key = Key::Reservation([2; 32]);
    let mut e = r.read(&key).unwrap().unwrap();
    if let Value::Reservation(a) = &mut e.value {
        a.nonce = u64::MAX;
    }
    r.store
        .bytes
        .insert(key.bytes(&domain()).unwrap(), e.encode(&domain()).unwrap());
    unchanged(
        &r,
        StorageCommand::Consume {
            reservation: [2; 32],
            nonce: 0,
            before: balance(1100, 100),
            after: balance(1100, 101),
        },
    );
}

#[test]
fn reservation_lookup_and_batch_size_stay_bounded_with_history() {
    let mut r = funded();
    for n in 10u64..110 {
        let mut id = [0; 32];
        id[..8].copy_from_slice(&n.to_le_bytes());
        run(
            &mut r,
            StorageCommand::ReceiveUnattributed {
                credit: id,
                amount: 1,
                before: balance(u128::from(1100 + n - 10), 100),
                after: balance(u128::from(1101 + n - 10), 100),
            },
        );
    }
    let b = r
        .prepare(StorageCommand::Reserve {
            reservation: [2; 32],
            owner: owner(),
            amount: 800,
            balance: balance(1200, 100),
        })
        .unwrap();
    assert_eq!(b.record_count(), 5); // includes retained admission guard absence
    r.commit(&b).unwrap();
    let b = r
        .prepare(StorageCommand::Consume {
            reservation: [2; 32],
            nonce: 1,
            before: balance(1200, 100),
            after: balance(1200, 400),
        })
        .unwrap();
    assert_eq!(b.record_count(), 5); // bounded guard read, independent of history
}

#[test]
fn another_storage_account_and_a_second_refund_cannot_spend_the_same_credit() {
    let mut r = funded();
    let other: AccountId = "other.testnet".parse().unwrap();
    unchanged(
        &r,
        StorageCommand::Reserve {
            reservation: [8; 32],
            owner: other.clone(),
            amount: 1,
            balance: balance(1100, 100),
        },
    );
    unchanged(
        &r,
        StorageCommand::Withdraw {
            withdrawal: [8; 32],
            owner: other,
            amount: 1,
            balance: balance(1100, 100),
        },
    );
    reserve(&mut r);
    let commitment = begin_withdrawal(&mut r);
    run(
        &mut r,
        StorageCommand::WithdrawalResult {
            withdrawal: [3; 32],
            commitment,
            result: CallbackResult::Failed,
        },
    );
    for id in [4, 5] {
        run(
            &mut r,
            StorageCommand::ReceiveUnattributed {
                credit: [id; 32],
                amount: 200,
                before: balance(900 + u128::from(id - 4) * 200, 100),
                after: balance(1100 + u128::from(id - 4) * 200, 100),
            },
        );
    }
    run(
        &mut r,
        StorageCommand::AttributeWithdrawalRefund {
            credit: [4; 32],
            withdrawal: [3; 32],
            commitment,
        },
    );
    unchanged(
        &r,
        StorageCommand::AttributeWithdrawalRefund {
            credit: [5; 32],
            withdrawal: [3; 32],
            commitment,
        },
    );
    let (_, h) = r.snapshot().unwrap();
    assert_eq!(h.unattributed(), 200);
    assert_eq!(h.totals().available, 200);
    assert_eq!(h.totals().funded, 1000);
}

#[test]
fn root_revision_overflow_and_orphan_reservations_reject_even_zero_cost_updates() {
    let mut r = funded();
    reserve(&mut r);
    let account_key = Key::Account(owner()).bytes(&domain()).unwrap();
    let account = r.store.bytes.remove(&account_key).unwrap();
    unchanged(
        &r,
        StorageCommand::Consume {
            reservation: [2; 32],
            nonce: 1,
            before: balance(1100, 100),
            after: balance(1100, 100),
        },
    );
    r.store.bytes.insert(account_key, account);
    let old = decode_record(
        &r.store.read(b"STATE").unwrap().unwrap(),
        &domain(),
        &RecordKey::Root,
    )
    .unwrap();
    let exhausted = Record::new(&domain(), RecordKey::Root, u64::MAX, old.value).unwrap();
    r.store.bytes.insert(
        b"STATE".to_vec(),
        encode_record(&exhausted, &domain()).unwrap(),
    );
    unchanged(
        &r,
        StorageCommand::Consume {
            reservation: [2; 32],
            nonce: 1,
            before: balance(1100, 100),
            after: balance(1100, 200),
        },
    );
    assert_eq!(
        StorageTotals {
            funded: u128::MAX,
            available: u128::MAX,
            staked: 1,
            ..Default::default()
        }
        .validate(),
        Err(Error::Overflow)
    );
}
