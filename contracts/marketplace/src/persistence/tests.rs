use std::panic::{catch_unwind, AssertUnwindSafe};

use super::codec::{checksum, Value};
use super::*;
use crate::{
    ledger::{BalanceObservation, Claim, ClaimState, OperatingContribution, PaymentPolicy},
    state::{Listing, ListingState, ListingTerms, Marketplace, Sale, TransferState},
    types::{Attempt, CallbackResult, ClaimId, FeeTerms, Generation, SaleId, TokenKey},
    AccountId, Error,
};

fn account(s: &str) -> AccountId {
    s.parse().unwrap()
}

#[test]
fn unverified_maximum_does_not_poison_watermark_after_cancel_invalid_or_restart() {
    for invalid in [false, true] {
        let mut r = repo();
        let p = run(
            &mut r,
            OfflineCommand::Listing {
                terms: terms("poison", u64::MAX),
                now_ns: 1,
                budgets: budgets(0),
            },
        );
        let g = p.generation().unwrap();
        let w = RecordKey::ApprovalWatermark(terms("poison", 0).token);
        let before = r.read(&w).unwrap().unwrap();
        assert!(matches!(
            before.value,
            Value::ApprovalWatermark(crate::state::ApprovalWatermark {
                authenticated: false,
                high_water: 0
            })
        ));
        let terminal = if invalid {
            OfflineCommand::Verify {
                generation: g,
                attempt: p.attempt().unwrap(),
                owner: None,
                approval: None,
                now_ns: 2,
            }
        } else {
            OfflineCommand::Cancel {
                generation: g,
                seller: account("seller.testnet"),
            }
        };
        run(&mut r, terminal);
        assert_eq!(r.read(&w).unwrap().unwrap(), before);
        let mut r = Repository::open(r.into_store(), domain()).unwrap();
        let real = listing(&mut r, "poison", 1);
        assert!(r
            .prepare(OfflineCommand::Verify {
                generation: g,
                attempt: p.attempt().unwrap(),
                owner: Some(account("seller.testnet")),
                approval: Some(u64::MAX),
                now_ns: 3
            })
            .is_err());
        run(
            &mut r,
            OfflineCommand::Cancel {
                generation: real,
                seller: account("seller.testnet"),
            },
        );
        assert!(r
            .prepare(OfflineCommand::Listing {
                terms: terms("poison", 1),
                now_ns: 4,
                budgets: budgets(0)
            })
            .is_err());
        assert!(matches!(
            r.read(&w).unwrap().unwrap().value,
            Value::ApprovalWatermark(crate::state::ApprovalWatermark {
                authenticated: true,
                high_water: 1
            })
        ));
    }
}

#[test]
fn watermark_is_preallocated_bounded_and_corruption_is_rejected() {
    let mut r = repo();
    let p = run(
        &mut r,
        OfflineCommand::Listing {
            terms: terms("bounded", 1),
            now_ns: 1,
            budgets: budgets(0),
        },
    );
    let prepared = r
        .prepare(OfflineCommand::Verify {
            generation: p.generation().unwrap(),
            attempt: p.attempt().unwrap(),
            owner: Some(account("seller.testnet")),
            approval: Some(1),
            now_ns: 2,
        })
        .unwrap();
    for (key, value) in prepared.batch().writes() {
        assert_eq!(r.store.read(key).unwrap().unwrap().len(), value.len());
    }
    let key = RecordKey::ApprovalWatermark(terms("bounded", 1).token);
    let mut record = r.read(&key).unwrap().unwrap();
    record.value = Value::ApprovalWatermark(crate::state::ApprovalWatermark {
        authenticated: false,
        high_water: 42,
    });
    assert!(encode_record(&record, &domain()).is_err());
}

#[test]
fn legacy_records_without_watermark_keep_their_existing_replay_barrier() {
    let mut r = repo();
    let p = run(
        &mut r,
        OfflineCommand::Listing {
            terms: terms("legacy", u64::MAX),
            now_ns: 1,
            budgets: budgets(0),
        },
    );
    run(
        &mut r,
        OfflineCommand::Cancel {
            generation: p.generation().unwrap(),
            seller: account("seller.testnet"),
        },
    );
    let key = RecordKey::ApprovalWatermark(terms("legacy", 1).token)
        .storage_key(&domain())
        .unwrap();
    r.store.bytes.remove(&key); // frozen v1 fixture had no additive watermark
    let r = Repository::open(r.into_store(), domain()).unwrap();
    assert!(r
        .prepare(OfflineCommand::Listing {
            terms: terms("legacy", 1),
            now_ns: 3,
            budgets: budgets(0)
        })
        .is_err());
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
        marketplace: account("market.testnet"),
        nft: account("nft.testnet"),
    }
}
fn budgets(kind: u8) -> Budgets {
    Budgets {
        first_call_gas: if kind == 2 { 0 } else { 101 },
        second_call_gas: if kind == 0 { 102 } else { 0 },
        callback_gas: 103,
        max_result_bytes: 4096,
        call_abi: [5; 32],
        callback_abi: [6; 32],
    }
}
fn terms(token: &str, approval_id: u64) -> ListingTerms {
    ListingTerms {
        token: TokenKey::new(domain().nft, token.into()).unwrap(),
        seller: account("seller.testnet"),
        approval_id,
        price: 1_000,
        expires_at_ns: 10_000,
        fees: FeeTerms::new(Some(1_000), Some(2_000), Some(account("fees.testnet"))).unwrap(),
        policy_version: 1,
        policy_hash: [7; 32],
        correlation: [8; 32],
    }
}
fn repo() -> Repository<MemoryStore> {
    Repository::new_offline(domain(), Some(PaymentPolicy::new(Some(10)).unwrap())).unwrap()
}
fn run(repo: &mut Repository<MemoryStore>, command: OfflineCommand) -> Prepared {
    let p = repo.prepare(command).unwrap();
    repo.commit(&p).unwrap();
    p
}
fn listing(repo: &mut Repository<MemoryStore>, token: &str, approval: u64) -> Generation {
    let p = run(
        repo,
        OfflineCommand::Listing {
            terms: terms(token, approval),
            now_ns: 1,
            budgets: budgets(0),
        },
    );
    let generation = p.generation().unwrap();
    run(
        repo,
        OfflineCommand::Verify {
            generation,
            attempt: p.attempt().unwrap(),
            owner: Some(account("seller.testnet")),
            approval: Some(approval),
            now_ns: 2,
        },
    );
    generation
}
fn purchase(repo: &mut Repository<MemoryStore>, generation: Generation) -> (SaleId, Attempt) {
    let l = repo.read(&RecordKey::Listing(generation)).unwrap().unwrap();
    let p = run(
        repo,
        OfflineCommand::Purchase {
            generation,
            token: l.listing().unwrap().terms().token.clone(),
            buyer: account("buyer.testnet"),
            exact_price: 1_000,
            now_ns: 3,
            budgets: budgets(1),
        },
    );
    (p.sale().unwrap(), p.attempt().unwrap())
}
fn claims(repo: &Repository<MemoryStore>, sale: SaleId) -> Vec<ClaimId> {
    let Value::Principal(p) = repo
        .read(&RecordKey::Principal(sale))
        .unwrap()
        .unwrap()
        .value
    else {
        panic!()
    };
    p.claims
}
fn settled(repo: &mut Repository<MemoryStore>, result: CallbackResult) -> (Generation, SaleId) {
    let generation = listing(repo, "art/é", 0);
    let (sale, attempt) = purchase(repo, generation);
    run(
        repo,
        OfflineCommand::TransferResult {
            sale,
            generation,
            attempt,
            result,
        },
    );
    (generation, sale)
}
fn dispatch(repo: &mut Repository<MemoryStore>, claim: ClaimId, now_ns: u64) -> Attempt {
    run(
        repo,
        OfflineCommand::Dispatch {
            claim,
            now_ns,
            balance: BalanceObservation {
                account_balance: 1_000_000,
                ..Default::default()
            },
            budgets: budgets(2),
        },
    )
    .attempt()
    .unwrap()
}
fn reopen(repo: Repository<MemoryStore>) -> Repository<MemoryStore> {
    // Test-only serialization of the entire backing fixture demonstrates a fresh
    // process image. No history serialization API exists on the production trait.
    let bytes = borsh::to_vec(&repo.into_store().bytes).unwrap();
    let store = MemoryStore {
        bytes: borsh::from_slice(&bytes).unwrap(),
        ..Default::default()
    };
    Repository::open(store, domain()).unwrap()
}
fn raw(record: &Record) -> Vec<u8> {
    let mut bytes = borsh::to_vec(record).unwrap();
    bytes.extend(checksum(&bytes));
    bytes
}
fn resign(bytes: &mut Vec<u8>) {
    bytes.truncate(bytes.len() - 32);
    bytes.extend(checksum(bytes));
}

#[test]
fn bootstrap_batch_preserves_v1_empty_root_and_cannot_replace_existing_state() {
    let domain = domain();
    let offline = Repository::<MemoryStore>::new_offline(domain.clone(), None).unwrap();
    let expected = offline.into_store().read(b"STATE").unwrap().unwrap();
    let batch = bootstrap_batch(&domain).unwrap();
    assert_eq!(batch.record_count(), 1);
    assert_eq!(
        batch.checks().collect::<Vec<_>>(),
        vec![(b"STATE".as_slice(), None)]
    );
    let mut host = MemoryStore::default();
    host.apply(&batch).unwrap();
    assert_eq!(host.read(b"STATE").unwrap().unwrap(), expected);
    let before = host.bytes.clone();
    assert_eq!(host.apply(&batch), Err(StorageError::Conflict));
    assert_eq!(host.bytes, before);
    host.bytes
        .insert(b"STATE".to_vec(), b"corrupt legacy root".to_vec());
    let before = host.bytes.clone();
    assert_eq!(host.apply(&batch), Err(StorageError::Conflict));
    assert_eq!(host.bytes, before);
}

#[test]
fn restart_preserves_consumed_approvals_paid_claims_unknown_attempts_and_totals() {
    let mut r = repo();
    let (generation, sale) = settled(&mut r, CallbackResult::Succeeded);
    let ids = claims(&r, sale);
    let paid = dispatch(&mut r, ids[0], 4);
    run(
        &mut r,
        OfflineCommand::Debit {
            claim: ids[0],
            attempt: paid,
        },
    );
    run(
        &mut r,
        OfflineCommand::PaymentResult {
            claim: ids[0],
            attempt: paid,
            result: CallbackResult::Succeeded,
        },
    );
    let pending = dispatch(&mut r, ids[1], 4);
    run(
        &mut r,
        OfflineCommand::Debit {
            claim: ids[1],
            attempt: pending,
        },
    );
    let commitment = r
        .read(&RecordKey::Payment(ids[1]))
        .unwrap()
        .unwrap()
        .operation()
        .unwrap()
        .commitment()
        .unwrap();
    run(
        &mut r,
        OfflineCommand::PaymentResult {
            claim: ids[1],
            attempt: pending,
            result: CallbackResult::Unknown,
        },
    );
    let totals = r.totals().unwrap();
    let mut r = reopen(r);
    assert_eq!(r.totals().unwrap(), totals);
    assert_eq!(
        r.read(&RecordKey::Claim(ids[0]))
            .unwrap()
            .unwrap()
            .claim()
            .unwrap()
            .state(),
        ClaimState::Paid
    );
    assert_eq!(
        r.read(&RecordKey::Payment(ids[1]))
            .unwrap()
            .unwrap()
            .operation()
            .unwrap()
            .commitment()
            .unwrap(),
        commitment
    );
    assert!(r
        .prepare(OfflineCommand::Dispatch {
            claim: ids[1],
            now_ns: 100,
            balance: BalanceObservation {
                account_balance: 1_000_000,
                ..Default::default()
            },
            budgets: budgets(2)
        })
        .is_err());
    assert!(r
        .prepare(OfflineCommand::Listing {
            terms: terms("art/é", 0),
            now_ns: 100,
            budgets: budgets(0)
        })
        .is_err());
    let next = listing(&mut r, "art/é", 1);
    assert!(next.value() > generation.value());
    run(
        &mut r,
        OfflineCommand::PaymentResult {
            claim: ids[1],
            attempt: pending,
            result: CallbackResult::Succeeded,
        },
    );
    assert_eq!(r.totals().unwrap().paid, 1_000);
    assert!(r
        .prepare(OfflineCommand::PaymentResult {
            claim: ids[1],
            attempt: pending,
            result: CallbackResult::Succeeded
        })
        .is_err());
}

#[test]
fn every_write_failure_and_panic_rolls_back_records_totals_and_attempts() {
    let mut r = repo();
    let generation = listing(&mut r, "art", 0);
    let (sale, attempt) = purchase(&mut r, generation);
    let p = r
        .prepare(OfflineCommand::TransferResult {
            sale,
            generation,
            attempt,
            result: CallbackResult::Succeeded,
        })
        .unwrap();
    assert_eq!(claims(&r, sale).len(), 0);
    let before = r.store.bytes.clone();
    for n in 1..=p.batch.writes.len() {
        r.store.fail_after = Some(n);
        assert_eq!(r.commit(&p), Err(StorageError::InjectedFailure));
        assert_eq!(r.store.bytes, before, "error after write {n}");
        r.store.fail_after = None;
        r.store.panic_after = Some(n);
        assert!(catch_unwind(AssertUnwindSafe(|| r.commit(&p))).is_err());
        assert_eq!(r.store.bytes, before, "panic after write {n}");
        r.store.panic_after = None;
    }
    r.commit(&p).unwrap();
    assert_eq!(claims(&r, sale).len(), 2);
    assert_eq!(r.totals().unwrap().available, 1_000);
    assert_eq!(r.commit(&p), Err(StorageError::Conflict));
}

#[test]
fn stale_sets_duplicate_keys_and_changed_read_dependencies_reject_atomically() {
    let mut r = repo();
    let generation = listing(&mut r, "art", 0);
    let p = r
        .prepare(OfflineCommand::Cancel {
            generation,
            seller: account("seller.testnet"),
        })
        .unwrap();
    let before = r.store.bytes.clone();
    let mut invalid = p.clone();
    invalid.batch.writes.push(invalid.batch.writes[0].clone());
    assert_eq!(r.commit(&invalid), Err(StorageError::Conflict));
    assert_eq!(r.store.bytes, before);
    let mut invalid = p.clone();
    invalid.batch.checks.push(invalid.batch.checks[0].clone());
    assert_eq!(r.commit(&invalid), Err(StorageError::Conflict));
    assert_eq!(r.store.bytes, before);
    let mut invalid = p.clone();
    invalid.batch.limit = 1;
    assert_eq!(r.commit(&invalid), Err(StorageError::Bounds));
    assert_eq!(r.store.bytes, before);
    run(&mut r, OfflineCommand::Stale { generation });
    assert_eq!(r.commit(&p), Err(StorageError::Conflict));
    // A backend read changing underneath a prepared batch also invalidates it.
    let mut r = repo();
    let generation = listing(&mut r, "art", 0);
    let p = r
        .prepare(OfflineCommand::Cancel {
            generation,
            seller: account("seller.testnet"),
        })
        .unwrap();
    let key = RecordKey::Verification(generation)
        .storage_key(&domain())
        .unwrap();
    r.store.bytes.get_mut(&key).unwrap()[0] ^= 1;
    let before = r.store.bytes.clone();
    assert_eq!(r.commit(&p), Err(StorageError::Conflict));
    assert_eq!(r.store.bytes, before);
}

#[test]
fn corruption_truncation_trailing_bytes_and_unknown_schemas_never_default() {
    let mut r = repo();
    let (_, sale) = settled(&mut r, CallbackResult::Succeeded);
    for key in [
        RecordKey::Root,
        RecordKey::Listing(Generation(1)),
        RecordKey::Principal(sale),
        RecordKey::Claim(claims(&r, sale)[0]),
        RecordKey::Transfer(sale),
    ] {
        let record = r.read(&key).unwrap().unwrap();
        let bytes = encode_record(&record, &domain()).unwrap();
        assert_eq!(decode_record(&bytes, &domain(), &key).unwrap(), record);
        for index in 0..bytes.len() {
            let mut bad = bytes.clone();
            bad[index] ^= 1;
            assert!(
                decode_record(&bad, &domain(), &key).is_err(),
                "{key:?} byte {index}"
            );
            assert!(decode_record(&bytes[..index], &domain(), &key).is_err());
        }
        let mut bad = bytes.clone();
        bad.insert(bad.len() - 32, 0);
        resign(&mut bad);
        assert!(decode_record(&bad, &domain(), &key).is_err());
        let mut bad = bytes;
        bad[8] = 2;
        resign(&mut bad);
        assert_eq!(
            decode_record(&bad, &domain(), &key),
            Err(StorageError::Schema)
        );
    }
    assert!(decode_record(&vec![0; MAX_RECORD_BYTES + 1], &domain(), &RecordKey::Root).is_err());
    let mut store = r.into_store();
    store.bytes.remove(b"STATE".as_slice());
    assert!(matches!(
        Repository::open(store, domain()),
        Err(StorageError::Missing)
    ));
    let mut store = MemoryStore::default();
    store
        .bytes
        .insert(b"STATE".to_vec(), b"legacy state".to_vec());
    assert!(Repository::open(store, domain()).is_err());
}

#[test]
fn checksummed_invalid_constructor_values_and_length_bombs_are_rejected() {
    let mut r = repo();
    let generation = listing(&mut r, "art", 0);
    let key = RecordKey::Listing(generation);
    let valid = r.read(&key).unwrap().unwrap();
    let mut invalid = valid.clone();
    let Value::Listing(l) = &mut invalid.value else {
        panic!()
    };
    // Borsh can bypass constructors; the persistent boundary may not.
    l.terms.token = borsh::from_slice(&borsh::to_vec(&(domain().nft, " ")).unwrap()).unwrap();
    assert!(encode_record(&invalid, &domain()).is_err());
    assert!(decode_record(&raw(&invalid), &domain(), &key).is_err());
    let mut invalid = valid;
    let Value::Listing(l) = &mut invalid.value else {
        panic!()
    };
    l.terms.fees =
        borsh::from_slice(&borsh::to_vec(&(3_000u16, 2_000u16, account("fees.testnet"))).unwrap())
            .unwrap();
    assert!(decode_record(&raw(&invalid), &domain(), &key).is_err());
    let token_key = RecordKey::Token(TokenKey::new(domain().nft.clone(), "art".into()).unwrap());
    let token = r.read(&token_key).unwrap().unwrap();
    let mut bomb = raw(&token);
    let token_length = 8 + 2 + 32 + 1 + 4 + domain().nft.as_str().len();
    bomb[token_length..token_length + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    resign(&mut bomb);
    assert_eq!(
        decode_record(&bomb, &domain(), &token_key),
        Err(StorageError::Bounds)
    );
    let (sale, _) = purchase(&mut r, generation);
    let key = RecordKey::Principal(sale);
    let p = r.read(&key).unwrap().unwrap();
    let mut bomb = raw(&p);
    let n = bomb.len() - 32 - 4;
    bomb[n..n + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    resign(&mut bomb);
    assert_eq!(
        decode_record(&bomb, &domain(), &key),
        Err(StorageError::Bounds)
    );
}

#[test]
fn cross_domain_kind_and_identity_substitution_rejects() {
    let mut r = repo();
    let generation = listing(&mut r, "art", 0);
    let record = r.read(&RecordKey::Listing(generation)).unwrap().unwrap();
    let bytes = raw(&record);
    assert_eq!(
        decode_record(&bytes, &domain(), &RecordKey::Listing(Generation(2))),
        Err(StorageError::Identity)
    );
    assert_eq!(
        decode_record(&bytes, &domain(), &RecordKey::Sale(SaleId(1))),
        Err(StorageError::Identity)
    );
    let mut other = domain();
    other.genesis[0] ^= 1;
    assert_eq!(
        decode_record(&bytes, &other, record.key()),
        Err(StorageError::Domain)
    );
    assert_ne!(
        record.key().storage_key(&domain()).unwrap(),
        record.key().storage_key(&other).unwrap()
    );
    assert_ne!(
        RecordKey::Sale(SaleId(1)).storage_key(&domain()).unwrap(),
        RecordKey::Principal(SaleId(1))
            .storage_key(&domain())
            .unwrap()
    );
    assert!(matches!(
        Repository::open(r.into_store(), other),
        Err(StorageError::Domain)
    ));
}

#[test]
fn failed_payment_retry_changes_attempt_and_rejects_old_callbacks_after_restart() {
    let mut r = repo();
    let (_, sale) = settled(&mut r, CallbackResult::Failed);
    let id = claims(&r, sale)[0];
    let first = dispatch(&mut r, id, 4);
    run(
        &mut r,
        OfflineCommand::Debit {
            claim: id,
            attempt: first,
        },
    );
    run(
        &mut r,
        OfflineCommand::PaymentResult {
            claim: id,
            attempt: first,
            result: CallbackResult::Failed,
        },
    );
    let mut r = reopen(r);
    let second = dispatch(&mut r, id, 14);
    assert_eq!(second.nonce, first.nonce + 1);
    assert_ne!(second.operation_hash, first.operation_hash);
    let before = r.store.bytes.clone();
    assert!(r
        .prepare(OfflineCommand::PaymentResult {
            claim: id,
            attempt: first,
            result: CallbackResult::Succeeded
        })
        .is_err());
    assert_eq!(r.store.bytes, before);
    assert!(r
        .prepare(OfflineCommand::PaymentResult {
            claim: id,
            attempt: second,
            result: CallbackResult::Succeeded
        })
        .is_err());
    run(
        &mut r,
        OfflineCommand::Debit {
            claim: id,
            attempt: second,
        },
    );
    run(
        &mut r,
        OfflineCommand::PaymentResult {
            claim: id,
            attempt: second,
            result: CallbackResult::Succeeded,
        },
    );
    assert_eq!(r.totals().unwrap().paid, 1_000);
}

#[test]
fn unknown_transfer_preserves_commitment_principal_and_lock_across_restart() {
    let mut r = repo();
    let generation = listing(&mut r, "art", 0);
    let (sale, attempt) = purchase(&mut r, generation);
    run(
        &mut r,
        OfflineCommand::TransferResult {
            sale,
            generation,
            attempt,
            result: CallbackResult::Unknown,
        },
    );
    let mut r = reopen(r);
    assert_eq!(r.totals().unwrap().held, 1_000);
    assert!(r
        .prepare(OfflineCommand::Cancel {
            generation,
            seller: account("seller.testnet")
        })
        .is_err());
    assert!(r
        .prepare(OfflineCommand::Listing {
            terms: terms("art", 1),
            now_ns: 4,
            budgets: budgets(0)
        })
        .is_err());
    assert_eq!(
        r.read(&RecordKey::Transfer(sale))
            .unwrap()
            .unwrap()
            .operation()
            .unwrap()
            .commitment()
            .unwrap(),
        attempt.operation_hash
    );
    run(
        &mut r,
        OfflineCommand::TransferResult {
            sale,
            generation,
            attempt,
            result: CallbackResult::Succeeded,
        },
    );
    assert_eq!(r.totals().unwrap().held, 0);
    assert_eq!(r.totals().unwrap().available, 1_000);
}

#[test]
fn prepared_concurrent_purchases_and_cancel_cannot_both_commit() {
    let mut r = repo();
    let generation = listing(&mut r, "art", 0);
    let command = OfflineCommand::Purchase {
        generation,
        token: terms("art", 0).token,
        buyer: account("buyer.testnet"),
        exact_price: 1_000,
        now_ns: 3,
        budgets: budgets(1),
    };
    let first = r.prepare(command.clone()).unwrap();
    let second = r.prepare(command).unwrap();
    let cancel = r
        .prepare(OfflineCommand::Cancel {
            generation,
            seller: account("seller.testnet"),
        })
        .unwrap();
    r.commit(&first).unwrap();
    let before = r.store.bytes.clone();
    assert_eq!(r.commit(&second), Err(StorageError::Conflict));
    assert_eq!(r.commit(&cancel), Err(StorageError::Conflict));
    assert_eq!(r.store.bytes, before);
    assert_eq!(r.totals().unwrap().principal, 1_000);
}

#[test]
fn root_revisions_and_ids_exhaust_without_partial_mutation() {
    let mut r = repo();
    let generation = listing(&mut r, "art", 0);
    let mut root = r.read(&RecordKey::Root).unwrap().unwrap();
    root.revision = u64::MAX;
    r.store.bytes.insert(b"STATE".to_vec(), raw(&root));
    let before = r.store.bytes.clone();
    assert_eq!(
        r.prepare(OfflineCommand::Cancel {
            generation,
            seller: account("seller.testnet")
        })
        .unwrap_err(),
        StorageError::Overflow
    );
    assert_eq!(r.store.bytes, before);
    root.revision = 2;
    let Value::Root(state) = &mut root.value else {
        panic!()
    };
    state.last_generation = u64::MAX;
    r.store.bytes.insert(b"STATE".to_vec(), raw(&root));
    let before = r.store.bytes.clone();
    assert!(r
        .prepare(OfflineCommand::Listing {
            terms: terms("new", 0),
            now_ns: 1,
            budgets: budgets(0)
        })
        .is_err());
    assert_eq!(r.store.bytes, before);
}

#[test]
fn malformed_linked_records_and_global_bucket_mismatches_fail_before_writes() {
    let mut r = repo();
    let (_, sale) = settled(&mut r, CallbackResult::Succeeded);
    let id = claims(&r, sale)[0];
    let mut record = r.read(&RecordKey::Claim(id)).unwrap().unwrap();
    let Value::Claim(c) = &mut record.value else {
        panic!()
    };
    c.recipient = account("attacker.testnet");
    r.store
        .bytes
        .insert(record.key().storage_key(&domain()).unwrap(), raw(&record));
    let before = r.store.bytes.clone();
    assert!(r
        .prepare(OfflineCommand::Dispatch {
            claim: id,
            now_ns: 4,
            balance: BalanceObservation {
                account_balance: 1_000_000,
                ..Default::default()
            },
            budgets: budgets(2)
        })
        .is_err());
    assert_eq!(r.store.bytes, before);
    let mut r = repo();
    let generation = listing(&mut r, "art", 0);
    let (sale, attempt) = purchase(&mut r, generation);
    let mut root = r.read(&RecordKey::Root).unwrap().unwrap();
    let Value::Root(state) = &mut root.value else {
        panic!()
    };
    state.totals.held = 0;
    state.totals.available = 1_000; // locally conserved but disagrees with touched principal
    r.store.bytes.insert(b"STATE".to_vec(), raw(&root));
    let before = r.store.bytes.clone();
    assert!(r
        .prepare(OfflineCommand::TransferResult {
            sale,
            generation,
            attempt,
            result: CallbackResult::Succeeded
        })
        .is_err());
    assert_eq!(r.store.bytes, before);
}

#[test]
fn retained_reserves_contributions_and_financial_hold_survive_reopen() {
    let mut r = repo();
    let mut root = r.read(&RecordKey::Root).unwrap().unwrap();
    let Value::Root(state) = &mut root.value else {
        panic!()
    };
    state.free_capital = 20;
    state.totals.reserved_operating = 30;
    state.totals.contributions = 70;
    state.totals.deficit_repairs = 20;
    state.financial_hold = true;
    r.store
        .bytes
        .insert(b"STATE".to_vec(), encode_record(&root, &domain()).unwrap());
    for (key, value) in [
        (RecordKey::Reservation([9; 32]), Value::Reservation(30)),
        (
            RecordKey::Contribution([10; 32]),
            Value::Contribution(OperatingContribution {
                amount: 70,
                deficit_repair: 20,
                free_capital_added: 50,
            }),
        ),
    ] {
        let record = Record::new(&domain(), key.clone(), 1, value).unwrap();
        r.store.bytes.insert(
            key.storage_key(&domain()).unwrap(),
            encode_record(&record, &domain()).unwrap(),
        );
    }
    let before = r.store.bytes.clone();
    let r = reopen(r);
    assert_eq!(r.store.bytes, before);
    assert!(r
        .prepare(OfflineCommand::Listing {
            terms: terms("art", 0),
            now_ns: 1,
            budgets: budgets(0)
        })
        .is_err());
    assert_eq!(
        r.read(&RecordKey::Contribution([10; 32]))
            .unwrap()
            .unwrap()
            .revision(),
        1
    );
    assert_eq!(r.totals().unwrap().reserved_operating, 30);
}

#[test]
fn bounded_point_access_is_independent_of_retained_history() {
    let mut r = repo();
    let mut counts = Vec::new();
    for n in 0..300 {
        let p = run(
            &mut r,
            OfflineCommand::Listing {
                terms: terms(&format!("token-{n}"), 0),
                now_ns: 1,
                budgets: budgets(0),
            },
        );
        counts.push(p.batch().record_count());
        run(
            &mut r,
            OfflineCommand::Cancel {
                generation: p.generation().unwrap(),
                seller: account("seller.testnet"),
            },
        );
    }
    assert!(counts.iter().all(|n| *n == counts[0]));
    assert!(counts[0] <= 6);
    assert!(r.store.bytes.len() > 900);
    let generation = listing(&mut r, "last", 0);
    let (sale, attempt) = purchase(&mut r, generation);
    let p = run(
        &mut r,
        OfflineCommand::TransferResult {
            sale,
            generation,
            attempt,
            result: CallbackResult::Succeeded,
        },
    );
    assert!(p.batch().record_count() <= 12);
    assert_eq!(r.totals().unwrap().available, 1_000);
}

#[test]
fn persistent_model_matches_existing_transition_rules_and_payload_encoding() {
    let mut r = repo();
    let mut original = Marketplace::new(
        domain().nft,
        domain().marketplace,
        Some(PaymentPolicy::new(Some(10)).unwrap()),
    )
    .unwrap();
    let t = terms("é", 0);
    let p = run(
        &mut r,
        OfflineCommand::Listing {
            terms: t.clone(),
            now_ns: 1,
            budgets: budgets(0),
        },
    );
    let generation = original
        .record_listing(t, 1, p.attempt().unwrap().operation_hash)
        .unwrap();
    original
        .verify_listing(
            generation,
            p.attempt().unwrap(),
            Some(&account("seller.testnet")),
            Some(0),
            2,
        )
        .unwrap();
    run(
        &mut r,
        OfflineCommand::Verify {
            generation,
            attempt: p.attempt().unwrap(),
            owner: Some(account("seller.testnet")),
            approval: Some(0),
            now_ns: 2,
        },
    );
    assert_eq!(
        borsh::to_vec(original.listing(generation).unwrap()).unwrap(),
        borsh::to_vec(
            r.read(&RecordKey::Listing(generation))
                .unwrap()
                .unwrap()
                .listing()
                .unwrap()
        )
        .unwrap()
    );
    let (sale, attempt) = purchase(&mut r, generation);
    assert_eq!(
        original
            .record_purchase(
                generation,
                &terms("é", 0).token,
                account("buyer.testnet"),
                1_000,
                3,
                attempt.operation_hash
            )
            .unwrap(),
        sale
    );
    for result in [CallbackResult::Unknown, CallbackResult::Succeeded] {
        original
            .record_transfer_result(sale, generation, attempt, result)
            .unwrap();
        run(
            &mut r,
            OfflineCommand::TransferResult {
                sale,
                generation,
                attempt,
                result,
            },
        );
        assert_eq!(r.totals().unwrap(), original.ledger().totals());
        assert_eq!(
            r.read(&RecordKey::Sale(sale))
                .unwrap()
                .unwrap()
                .sale()
                .unwrap(),
            original.sale(sale).unwrap()
        );
    }
    for id in claims(&r, sale) {
        assert_eq!(
            r.read(&RecordKey::Claim(id))
                .unwrap()
                .unwrap()
                .claim()
                .unwrap(),
            original.ledger().claim(id).unwrap()
        );
    }
}

#[test]
fn budgets_missing_abi_wrong_environment_and_all_public_actions_fail_closed() {
    let r = repo();
    let mut b = budgets(0);
    b.call_abi = [0; 32];
    assert!(r
        .prepare(OfflineCommand::Listing {
            terms: terms("art", 0),
            now_ns: 1,
            budgets: b
        })
        .is_err());
    assert_eq!(r.totals().unwrap().principal, 0);
    for action in [
        crate::PublicAction::Mint,
        crate::PublicAction::Deposit,
        crate::PublicAction::List,
        crate::PublicAction::Purchase,
        crate::PublicAction::Withdraw,
        crate::PublicAction::Recovery,
    ] {
        assert_eq!(crate::authorize_public_action(action), Err(Error::Disabled));
    }
    let mut d = domain();
    d.environment = if d.environment == ArtifactDomain::Testnet {
        ArtifactDomain::Production
    } else {
        ArtifactDomain::Testnet
    };
    if cfg!(any(feature = "testnet", feature = "production")) {
        assert!(Repository::new_offline(d, None).is_err());
    }
}

fn vector_contexts() -> Vec<OperationContext> {
    let mut output = vec![];
    for environment in [ArtifactDomain::Testnet, ArtifactDomain::Production] {
        let mut d = domain();
        d.environment = environment;
        let mut t = terms(&"é".repeat(512), u64::MAX);
        t.price = u128::MAX;
        t.expires_at_ns = u64::MAX;
        t.policy_version = u64::MAX;
        t.fees = FeeTerms::new(Some(123), Some(1_000), Some(account("fees.testnet"))).unwrap();
        let a = Attempt {
            nonce: u64::MAX,
            operation_hash: [11; 32],
        };
        let l = Listing {
            generation: Generation(u64::MAX),
            terms: t.clone(),
            state: ListingState::Verifying,
            verification: a,
            sale: None,
        };
        let s = Sale {
            id: SaleId(u64::MAX - 1),
            generation: l.generation,
            terms: t,
            buyer: account("buyer.testnet"),
            transfer: a,
            outcome: TransferState::Pending,
        };
        let c = Claim {
            id: ClaimId(u64::MAX - 2),
            sale: s.id,
            generation: l.generation,
            recipient: account("seller.testnet"),
            seller_amount: u128::MAX - 1,
            fee_amount: 1,
            refund_amount: 0,
            amount: u128::MAX,
            state: ClaimState::Available,
            attempt: None,
            last_dispatch_ns: None,
            debit_pending: false,
        };
        output.extend([
            OperationContext::for_listing(&d, &l, budgets(0)).unwrap(),
            OperationContext::for_sale(&d, &s, budgets(1)).unwrap(),
            OperationContext::for_payment(&d, &c, u64::MAX, budgets(2)).unwrap(),
        ]);
    }
    output
}

#[test]
fn canonical_contexts_are_bounded_lossless_and_bind_every_encoded_field() {
    let contexts = vector_contexts();
    let hashes: Vec<_> = contexts.iter().map(|c| c.commitment().unwrap()).collect();
    for (i, context) in contexts.iter().enumerate() {
        let bytes = context.canonical_bytes().unwrap();
        assert_eq!(
            OperationContext::from_canonical_bytes(&bytes, &context.domain).unwrap(),
            *context
        );
        for j in 0..i {
            assert_ne!(hashes[i], hashes[j]);
        }
        let mut bad = bytes.clone();
        bad.push(0);
        assert!(OperationContext::from_canonical_bytes(&bad, &context.domain).is_err());
        for n in 0..bytes.len() {
            assert!(OperationContext::from_canonical_bytes(&bytes[..n], &context.domain).is_err());
        }
        let mut different = context.clone();
        different.nonce -= 1;
        assert_ne!(different.commitment().unwrap(), hashes[i]);
        let mut different = context.clone();
        different.budgets.callback_abi[0] ^= 1;
        assert_ne!(different.commitment().unwrap(), hashes[i]);
        let mut different = context.clone();
        different.budgets.callback_gas += 1;
        assert_ne!(different.commitment().unwrap(), hashes[i]);
        let mut wrong = context.domain.clone();
        wrong.manifest[0] ^= 1;
        assert_eq!(
            OperationContext::from_canonical_bytes(&bytes, &wrong),
            Err(StorageError::Domain)
        );
    }
}

#[test]
fn operation_golden_vectors_match_independent_python_encoder() {
    let expected = include_str!("../../tests/fixtures/operation_v1.sha256");
    let contexts = vector_contexts();
    assert_eq!(contexts.len(), expected.lines().count());
    for (context, line) in contexts.iter().zip(expected.lines()) {
        let (size, digest) = line.split_once(' ').unwrap();
        assert_eq!(
            context.canonical_bytes().unwrap().len(),
            size.parse::<usize>().unwrap()
        );
        let actual: String = context
            .commitment()
            .unwrap()
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect();
        assert_eq!(actual, digest);
    }
}

#[test]
fn recipient_amount_source_and_exact_approval_changes_change_intent_hash() {
    use super::operation::Descriptor;
    for context in vector_contexts() {
        let original = context.commitment().unwrap();
        let mut changed = context.clone();
        match &mut changed.descriptor {
            Descriptor::Verification { terms, .. } | Descriptor::ApprovedTransfer { terms, .. } => {
                terms.approval_id -= 1
            }
            Descriptor::Payment {
                amount,
                seller_amount,
                ..
            } => {
                *amount -= 1;
                *seller_amount -= 1;
            }
        }
        assert_ne!(changed.commitment().unwrap(), original);
        let mut changed = context.clone();
        match &mut changed.descriptor {
            Descriptor::Verification { generation, .. }
            | Descriptor::ApprovedTransfer { generation, .. }
            | Descriptor::Payment { generation, .. } => generation.0 -= 1,
        }
        assert_ne!(changed.commitment().unwrap(), original);
        let mut changed = context.clone();
        match &mut changed.descriptor {
            Descriptor::Verification { terms, .. } => terms.seller = account("other.testnet"),
            Descriptor::ApprovedTransfer { receiver, .. } => *receiver = account("other.testnet"),
            Descriptor::Payment { recipient, .. } => *recipient = account("other.testnet"),
        }
        assert_ne!(changed.commitment().unwrap(), original);
    }
}

#[test]
fn orphan_claim_or_forged_intent_cannot_be_dispatched() {
    let mut r = repo();
    let (_, sale) = settled(&mut r, CallbackResult::Succeeded);
    let id = claims(&r, sale)[0];
    let mut orphan = r.read(&RecordKey::Claim(id)).unwrap().unwrap();
    orphan.key = RecordKey::Claim(ClaimId(3));
    let Value::Claim(c) = &mut orphan.value else {
        panic!()
    };
    c.id = ClaimId(3);
    let mut root = r.read(&RecordKey::Root).unwrap().unwrap();
    let Value::Root(state) = &mut root.value else {
        panic!()
    };
    state.last_claim = 3;
    r.store.bytes.insert(b"STATE".to_vec(), raw(&root));
    r.store
        .bytes
        .insert(orphan.key.storage_key(&domain()).unwrap(), raw(&orphan));
    assert!(r
        .prepare(OfflineCommand::Dispatch {
            claim: ClaimId(3),
            now_ns: 4,
            balance: BalanceObservation {
                account_balance: 1_000_000,
                ..Default::default()
            },
            budgets: budgets(2)
        })
        .is_err());
    let attempt = dispatch(&mut r, id, 4);
    let key = RecordKey::Payment(id);
    let mut op = r.read(&key).unwrap().unwrap();
    let Value::Operation(v) = &mut op.value else {
        panic!()
    };
    let super::operation::Descriptor::Payment { recipient, .. } = &mut v.context.descriptor else {
        panic!()
    };
    *recipient = account("attacker.testnet");
    r.store
        .bytes
        .insert(key.storage_key(&domain()).unwrap(), raw(&op));
    let before = r.store.bytes.clone();
    assert!(r
        .prepare(OfflineCommand::Debit { claim: id, attempt })
        .is_err());
    assert_eq!(r.store.bytes, before);
}

fn audit_totals(r: &Repository<MemoryStore>) {
    let mut totals = crate::ledger::LedgerTotals::default();
    for bytes in r.store.bytes.values() {
        // Test-only full audit of stored fixture, independent of maintained totals.
        let raw: Record = borsh::from_slice(&bytes[..bytes.len() - 32]).unwrap();
        let record = decode_record(bytes, &domain(), raw.key()).unwrap();
        match record.value {
            Value::Principal(p) => {
                totals.principal += p.price;
                totals.held += p.held;
            }
            Value::Claim(c) => {
                match c.state {
                    ClaimState::Available | ClaimState::Retryable => totals.available += c.amount,
                    ClaimState::InFlight | ClaimState::Unknown => totals.in_flight += c.amount,
                    ClaimState::Paid => totals.paid += c.amount,
                }
                if c.debit_pending {
                    totals.queued_payments += c.amount;
                }
            }
            _ => {}
        }
    }
    assert_eq!(r.totals().unwrap(), totals);
}

#[test]
fn callback_permutations_after_restart_conserve_principal_and_never_pay_twice() {
    let results = [
        CallbackResult::Unknown,
        CallbackResult::Failed,
        CallbackResult::Succeeded,
    ];
    for first in results {
        for second in results {
            let mut r = repo();
            let generation = listing(&mut r, "art", 0);
            let (sale, attempt) = purchase(&mut r, generation);
            for result in [first, second] {
                let before = r.store.bytes.clone();
                match r.prepare(OfflineCommand::TransferResult {
                    sale,
                    generation,
                    attempt,
                    result,
                }) {
                    Ok(p) => r.commit(&p).unwrap(),
                    Err(_) => assert_eq!(r.store.bytes, before),
                }
                audit_totals(&r);
                r = reopen(r);
            }
            for id in claims(&r, sale) {
                let attempt = dispatch(&mut r, id, 4);
                audit_totals(&r);
                run(&mut r, OfflineCommand::Debit { claim: id, attempt });
                audit_totals(&r);
                for result in [first, second] {
                    let before = r.store.bytes.clone();
                    match r.prepare(OfflineCommand::PaymentResult {
                        claim: id,
                        attempt,
                        result,
                    }) {
                        Ok(p) => r.commit(&p).unwrap(),
                        Err(_) => assert_eq!(r.store.bytes, before),
                    }
                    audit_totals(&r);
                    r = reopen(r);
                }
            }
            assert!(r.totals().unwrap().paid <= 1_000);
        }
    }
}
