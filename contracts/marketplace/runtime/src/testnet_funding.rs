//! Testnet-only deposit boundary. No payment, refund or withdrawal dispatch.
use crate::{
    checked_runtime_domain, nft_integration::authority, storage_accounting, HostStore, Runtime,
};
use near_marketplace_foundation::{
    persistence::{AtomicStore, StorageRepository},
    storage_accounting::StorageCommand,
};
use near_sdk::{
    env,
    json_types::{U128, U64},
    near, AccountId,
};

#[near(serializers = [json])]
pub struct FundingStatus {
    pub owner_id: AccountId,
    pub funding_nonce: U64,
    pub received: U128,
    pub storage_charged: U128,
    pub available: U128,
}

impl Runtime {
    /// A fresh nonce identifies each deposit. A duplicate ABORTS, so no second
    /// attached deposit is retained without a corresponding credit. NEAR handles
    /// failed-receipt refunds; this contract schedules none.
    pub fn fund_testnet_storage(funding_nonce: U64) -> FundingStatus {
        let domain = checked_runtime_domain();
        let owner = authority::authenticate_identity(&domain)
            .unwrap_or_else(|_| env::panic_str("Only immutable admission owner may fund"));
        for account in [&domain.marketplace, &domain.nft, &owner] {
            if !account.as_str().ends_with(".testnet") {
                env::panic_str("Testnet funding identity required");
            }
        }
        let amount = env::attached_deposit().as_yoctonear();
        if amount == 0 {
            env::panic_str("Positive storage deposit required");
        }
        let after = storage_accounting::observe();
        let before = near_marketplace_foundation::ledger::BalanceObservation {
            account_balance: after
                .account_balance
                .checked_sub(amount)
                .unwrap_or_else(|| env::panic_str("Deposit balance mismatch")),
            ..after
        };
        let mut repo = StorageRepository::open(HostStore, domain.clone()).unwrap();
        let (protection, head) = repo.snapshot().unwrap();
        head.check_backing(protection, before)
            .unwrap_or_else(|_| env::panic_str("Existing backing shortfall"));
        let mut identity = b"nmp:testnet:storage-funding:v1\0".to_vec();
        identity.extend(domain.commitment().unwrap());
        identity.extend(borsh::to_vec(&owner).unwrap());
        identity.extend(funding_nonce.0.to_le_bytes());
        let credit = env::sha256(&identity).try_into().unwrap();
        let fund = repo
            .prepare(StorageCommand::Fund {
                credit,
                owner: owner.clone(),
                amount,
                before,
                after,
            })
            .unwrap_or_else(|_| env::panic_str("Funding rejected or nonce reused"));
        // All errors after the first write must abort this receipt. The second
        // batch converts only this deposit's measured metadata to funded stake.
        repo.commit(&fund)
            .unwrap_or_else(|_| env::panic_str("Funding write failed"));
        let measured = storage_accounting::observe();
        let charged = measured
            .storage_stake
            .checked_sub(after.storage_stake)
            .unwrap_or_else(|| env::panic_str("Funding storage decreased"));
        let charge = repo
            .prepare(StorageCommand::ChargeFundingStorage {
                credit,
                before: after,
                after: measured,
            })
            .unwrap_or_else(|_| env::panic_str("Deposit cannot fund its metadata"));
        for (key, value) in charge.writes() {
            if HostStore
                .read(key)
                .unwrap()
                .is_none_or(|old| old.len() != value.len())
            {
                env::panic_str("Funding charge must be fixed width");
            }
        }
        repo.commit(&charge)
            .unwrap_or_else(|_| env::panic_str("Funding charge failed"));
        if storage_accounting::observe() != measured || repo.snapshot().unwrap().0 != protection {
            env::panic_str("Funding changed protected state or accounting size");
        }
        storage_accounting::assert_storage_backing(&domain);
        FundingStatus {
            owner_id: owner.clone(),
            funding_nonce,
            received: U128(amount),
            storage_charged: U128(charged),
            available: U128(repo.account(&owner).unwrap().unwrap().available),
        }
    }
}

// SDK 5.30 rejects #[payable] on receiverless methods. A &mut self wrapper
// would overwrite our frozen STATE envelope. This bounded, testnet-only export
// deliberately performs neither SDK state_read nor state_write.
#[cfg(target_arch = "wasm32")]
#[allow(unsafe_code)]
#[no_mangle]
pub extern "C" fn fund_testnet_storage() {
    env::setup_panic_hook();
    #[derive(near_sdk::serde::Deserialize)]
    #[serde(crate = "near_sdk::serde", deny_unknown_fields)]
    struct Args {
        funding_nonce: U64,
    }
    let register = 0x4d50_4631;
    // SAFETY: host input writes only its register; no guest pointer is supplied.
    unsafe {
        near_sys::input(register);
    }
    let input = env::read_register_bounded(register, 128)
        .expect("Missing funding input")
        .expect("Oversized funding input");
    let args: Args = near_sdk::serde_json::from_slice(&input).expect("Invalid funding arguments");
    let result = Runtime::fund_testnet_storage(args.funding_nonce);
    env::value_return(&near_sdk::serde_json::to_vec(&result).unwrap());
}
