use super::lifecycle_tests::{sponsor, FUNDS};
use super::*;
use crate::storage_accounting;

#[test]
fn operator_python_payload_is_accepted_by_frozen_rust_operation_codec() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testnet/test_flow.py");
    let output = std::process::Command::new("python3")
        .arg("-B")
        .arg(path)
        .arg("--codec-fixture")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let domain: near_sdk::json_types::Base64VecU8 =
        serde_json::from_value(value["domain"].clone()).unwrap();
    let domain: DeploymentDomain = borsh::from_slice(&domain.0).unwrap();
    let bytes: near_sdk::json_types::Base64VecU8 =
        serde_json::from_value(value["operation"].clone()).unwrap();
    let op = OperationContext::from_canonical_bytes(&bytes.0, &domain).unwrap();
    assert_eq!(op.canonical_bytes().unwrap(), bytes.0);
    wire::verification_calls(&op).unwrap();
    let (generation, terms) = op.verification().unwrap();
    assert_eq!(generation.value(), 1);
    assert_eq!(terms.approval_id, u64::MAX);
    assert_eq!(terms.token.token(), "é".repeat(512));
}

#[test]
fn funding_preserves_sticky_holds_and_unattributed_funds() {
    let d = domain();
    setup_domain_with_owner(&d, Some(sponsor()));
    let mut repo = StorageRepository::open(HostStore, d.clone()).unwrap();
    let after = storage_accounting::observe();
    let batch = repo
        .prepare(StorageCommand::ReceiveUnattributed {
            credit: [33; 32],
            amount: 100,
            before: BalanceObservation {
                account_balance: after.account_balance - 100,
                ..after
            },
            after,
        })
        .unwrap();
    repo.commit(&batch).unwrap();
    let batch = repo
        .prepare(StorageCommand::CheckHealth {
            balance: BalanceObservation::default(),
        })
        .unwrap();
    repo.commit(&batch).unwrap();
    let before = repo.snapshot().unwrap();
    assert!(before.1.financial_hold());
    funding_context(&d, sponsor(), FUNDS);
    crate::Runtime::fund_testnet_storage(U64(1));
    let after = repo.snapshot().unwrap();
    assert_eq!(before.0, after.0);
    assert_eq!(before.1.unattributed(), after.1.unattributed());
    assert!(after.1.financial_hold());
}

fn funding_context(d: &DeploymentDomain, caller: AccountId, amount: u128) {
    let mut c = VMContextBuilder::new();
    c.current_account_id(d.marketplace.clone())
        .predecessor_account_id(caller)
        .signer_account_id(account("not-authority.testnet"))
        .account_contract(AccountContract::Local(d.marketplace_code).into())
        .account_balance(NearToken::from_near(100))
        .storage_usage(env::storage_usage())
        .attached_deposit(NearToken::from_yoctonear(amount));
    testing_env!(c.build());
}

#[test]
fn actual_deposit_funds_only_owner_and_meters_all_metadata() {
    let d = domain();
    setup_domain_with_owner(&d, Some(sponsor()));
    for nonce in [0, u64::MAX] {
        funding_context(&d, sponsor(), FUNDS);
        let before = storage_accounting::observe();
        let protection = StorageRepository::open(HostStore, d.clone())
            .unwrap()
            .snapshot()
            .unwrap()
            .0;
        let status = crate::Runtime::fund_testnet_storage(U64(nonce));
        let growth = storage_accounting::observe().storage_stake - before.storage_stake;
        assert_eq!(status.received.0, FUNDS);
        assert_eq!(status.storage_charged.0, growth);
        assert!(growth > 0);
        let repo = StorageRepository::open(HostStore, d.clone()).unwrap();
        assert_eq!(repo.snapshot().unwrap().0, protection);
        let t = repo.account(&sponsor()).unwrap().unwrap();
        assert_eq!(t.funded, t.available + t.staked);
        assert_eq!(t.reserved + t.queued + t.in_flight + t.withdrawn, 0);
        assert!(repo
            .account(&account("not-authority.testnet"))
            .unwrap()
            .is_none());
        assert!(get_created_receipts().is_empty());
    }
}

#[test]
fn unauthorized_zero_and_replayed_deposits_fail_before_writes() {
    let d = domain();
    setup_domain_with_owner(&d, Some(sponsor()));
    funding_context(&d, sponsor(), FUNDS);
    crate::Runtime::fund_testnet_storage(U64(1));
    for (caller, amount, nonce) in [
        (account("attacker.testnet"), FUNDS, 2),
        (sponsor(), 0, 2),
        (sponsor(), FUNDS, 1),
    ] {
        funding_context(&d, caller, amount);
        let before = StorageRepository::open(HostStore, d.clone())
            .unwrap()
            .snapshot()
            .unwrap();
        assert!(
            std::panic::catch_unwind(|| crate::Runtime::fund_testnet_storage(U64(nonce))).is_err()
        );
        assert_eq!(
            StorageRepository::open(HostStore, d.clone())
                .unwrap()
                .snapshot()
                .unwrap(),
            before
        );
    }
}
