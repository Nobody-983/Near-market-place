//! Internal host boundaries, no JSON ABI or inferred receipt/refund evidence.
use crate::HostStore;
use near_marketplace_foundation::{
    ledger::BalanceObservation,
    persistence::{AtomicBatch, AtomicStore, DeploymentDomain, StorageRepository},
    storage_accounting::StorageCommand,
};
use near_sdk::env;

/// Valid only at this prototype's storage-only boundary: no outgoing actions or
/// legacy external credits exist. Future adapters must supply maintained legacy
/// Q/L/U partitions instead of reusing these zeroes for active trading.
pub(crate) fn observe() -> BalanceObservation {
    BalanceObservation {
        account_balance: env::account_balance().as_yoctonear(),
        storage_stake: u128::from(env::storage_usage())
            .checked_mul(env::storage_byte_cost().as_yoctonear())
            .unwrap_or_else(|| env::panic_str("Storage cost overflow")),
        ..Default::default()
    }
}

pub fn assert_storage_backing(domain: &DeploymentDomain) {
    let repo = StorageRepository::open(HostStore, domain.clone())
        .unwrap_or_else(|_| env::panic_str("Invalid storage accounting records"));
    let (legacy, storage) = repo
        .snapshot()
        .unwrap_or_else(|_| env::panic_str("Invalid storage accounting snapshot"));
    storage
        .check_backing(legacy, observe())
        .unwrap_or_else(|_| env::panic_str("Unfunded runtime storage or liabilities"));
}

/// Apply an internally prepared, storage-only v1 batch and charge its entire
/// measured growth to ONE existing storage reservation. No caller-selected
/// funding/beneficiary, accounting batch, promise or balance result is admitted.
/// The future caller must bind this reservation/nonce to its stored operation.
/// This is NOT a public transaction method or a purchase/payment adapter.
pub fn apply_reserved_storage(
    domain: &DeploymentDomain,
    reservation: [u8; 32],
    nonce: u64,
    batch: &AtomicBatch,
) {
    if batch.writes().any(|(key, _)| key.starts_with(b"nmp:s:")) {
        env::panic_str("Payload cannot rewrite storage accounting");
    }
    meter_storage(domain, reservation, nonce, false, || {
        HostStore
            .apply(batch)
            .unwrap_or_else(|_| env::panic_str("Payload write failed"));
    });
}

/// Internal synchronous boundary only. `new_reservation` includes its allocation
/// metadata in the measured charge; the closure must create it from prepaid
/// credits in this receipt. A closure may only return success or abort.
pub(crate) fn meter_storage(
    domain: &DeploymentDomain,
    reservation: [u8; 32],
    nonce: u64,
    new_reservation: bool,
    write: impl FnOnce(),
) {
    if !env::attached_deposit().is_zero() {
        env::panic_str("Storage-only boundary rejects attached value");
    }
    let mut repo = StorageRepository::open(HostStore, domain.clone())
        .unwrap_or_else(|_| env::panic_str("Storage accounting missing"));
    let (protection, storage) = repo.snapshot().unwrap();
    // Check existence before the first payload write. No implicit funding.
    if !new_reservation {
        repo.reservation_remaining(reservation)
            .unwrap_or_else(|_| env::panic_str("Storage reservation missing"));
    }
    let before = observe();
    storage
        .check_backing(protection, before)
        .unwrap_or_else(|_| env::panic_str("Storage backing shortfall"));
    write();
    let after = observe();
    if repo.snapshot().unwrap().0 != protection {
        env::panic_str("Storage boundary cannot mutate financial state");
    }
    let charge = repo
        .prepare(StorageCommand::Consume {
            reservation,
            nonce,
            before,
            after,
        })
        .unwrap_or_else(|_| env::panic_str("Unfunded storage expenditure"));
    // All charge records already exist and all changed fields are fixed width.
    // Check this before committing; don't forget accounting's own byte cost.
    for (key, value) in charge.writes() {
        let old = HostStore
            .read(key)
            .unwrap()
            .unwrap_or_else(|| env::panic_str("Charge record missing"));
        if old.len() != value.len() {
            env::panic_str("Unreserved charge metadata growth");
        }
    }
    let usage = env::storage_usage();
    repo.commit(&charge)
        .unwrap_or_else(|_| env::panic_str("Storage charge failed"));
    if env::storage_usage() != usage {
        env::panic_str("Unexpected accounting storage growth");
    }
    assert_storage_backing(domain);
    // A failure anywhere above aborts the receipt, never returns an ordinary
    // Err after writing. Node-level rollback is not established by native mocks.
}
