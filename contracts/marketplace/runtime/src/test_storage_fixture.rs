//! Synthetic credit/operation setup ONLY for native/explicit fault-artifact tests.
//! Prefunded test balance is assigned to a fixture account; this is not a
//! verified deposit, funding policy, receipt attestation or public funding API.
use crate::{storage_accounting::apply_reserved_storage, HostStore};
use near_marketplace_foundation::{
    ledger::BalanceObservation,
    persistence::{Budgets, DeploymentDomain, OfflineCommand, Repository, StorageRepository},
    state::ListingTerms,
    storage_accounting::StorageCommand,
    types::{FeeTerms, TokenKey},
};
use near_sdk::{env, NearToken};

fn observation() -> BalanceObservation {
    BalanceObservation {
        account_balance: env::account_balance().as_yoctonear(),
        storage_stake: u128::from(env::storage_usage()) * env::storage_byte_cost().as_yoctonear(),
        ..Default::default()
    }
}

pub(crate) fn exercise(domain: &DeploymentDomain, underfunded: bool) -> u128 {
    let mut storage = StorageRepository::open(HostStore, domain.clone()).unwrap();
    let payer = "payer.testnet".parse().unwrap();
    let amount = if underfunded {
        1
    } else {
        NearToken::from_near(1).as_yoctonear()
    };
    let after = observation();
    let fund = storage
        .prepare(StorageCommand::Fund {
            credit: [71; 32],
            owner: payer,
            amount,
            before: BalanceObservation {
                account_balance: after.account_balance - amount,
                ..after
            },
            after,
        })
        .unwrap();
    storage.commit(&fund).unwrap();
    let reserve = storage
        .prepare(StorageCommand::Reserve {
            reservation: [72; 32],
            owner: "payer.testnet".parse().unwrap(),
            amount,
            balance: observation(),
        })
        .unwrap();
    storage.commit(&reserve).unwrap();
    // No approval hook, listing activation, promise or public action is run.
    let data = Repository::open(HostStore, domain.clone())
        .unwrap()
        .prepare(OfflineCommand::Listing {
            terms: ListingTerms {
                token: TokenKey::new(domain.nft.clone(), "é".repeat(512)).unwrap(),
                seller: "seller.testnet".parse().unwrap(),
                approval_id: u64::MAX,
                price: 1000,
                expires_at_ns: 10000,
                fees: FeeTerms::new(Some(0), Some(0), Some("fees.testnet".parse().unwrap()))
                    .unwrap(),
                policy_version: 1,
                policy_hash: [73; 32],
                correlation: [74; 32],
            },
            now_ns: 1,
            budgets: Budgets {
                first_call_gas: 101,
                second_call_gas: 102,
                callback_gas: 103,
                max_result_bytes: 4096,
                call_abi: [75; 32],
                callback_abi: [76; 32],
            },
        })
        .unwrap();
    let before = env::storage_usage();
    apply_reserved_storage(domain, [72; 32], 1, data.batch());
    let charged =
        u128::from(env::storage_usage() - before) * env::storage_byte_cost().as_yoctonear();
    let (_, head) = storage.snapshot().unwrap();
    assert_eq!(head.totals().staked, charged);
    assert_eq!(
        storage.reservation_remaining([72; 32]).unwrap(),
        amount - charged
    );
    assert_eq!(head.totals().available, 0);
    charged
}
