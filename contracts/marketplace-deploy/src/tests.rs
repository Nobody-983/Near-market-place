use crate::{
    migration::{OldContract, OldLedger},
    *,
};
use near_contract_standards::storage_management::StorageManagement;
use near_sdk::{test_utils::VMContextBuilder, testing_env, Gas, NearToken, PromiseResult};

const ONE_NEAR: u128 = 10u128.pow(24);
const PRICE: u128 = 1_000_000;

fn account(value: &str) -> AccountId {
    value.parse().unwrap()
}
fn market() -> AccountId {
    account("market.testnet")
}
fn nft() -> AccountId {
    account("nft.testnet")
}
fn owner() -> AccountId {
    account("owner.testnet")
}
fn seller() -> AccountId {
    account("seller.testnet")
}
fn buyer() -> AccountId {
    account("buyer.testnet")
}
fn other() -> AccountId {
    account("other.testnet")
}

fn context(
    predecessor: AccountId,
    signer: AccountId,
    deposit: u128,
    timestamp: u64,
) -> VMContextBuilder {
    let mut value = VMContextBuilder::new();
    value
        .current_account_id(market())
        .predecessor_account_id(predecessor)
        .signer_account_id(signer)
        .attached_deposit(NearToken::from_yoctonear(deposit))
        .block_timestamp(timestamp)
        .account_balance(NearToken::from_near(100))
        .prepaid_gas(Gas::from_tgas(300));
    value
}

fn env(predecessor: AccountId, signer: AccountId, deposit: u128, timestamp: u64) {
    testing_env!(context(predecessor, signer, deposit, timestamp).build());
}

fn callback(result: PromiseResult, timestamp: u64) {
    testing_env!(
        context(market(), market(), 0, timestamp).build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        vec![result]
    );
}

fn setup() -> Contract {
    env(market(), market(), 0, 1);
    let mut contract = Contract::new(nft(), owner(), 100, None);
    env(seller(), seller(), ONE_NEAR, 2);
    contract.storage_deposit(None, None);
    env(owner(), owner(), ONE_NEAR, 3);
    contract.storage_deposit(None, None);
    env(owner(), owner(), 1, 4);
    contract.set_trading_enabled(true);
    contract
}

fn terms(token_id: &str, seller_id: &AccountId, price: u128, expires_at: Option<u64>) -> String {
    serde_json::json!({
        "standard": "near_marketplace", "version": "1.0.0", "marketplace_id": market(),
        "nft_contract_id": nft(), "token_id": token_id, "seller_id": seller_id,
        "price": price.to_string(), "fee_bps": 100, "fee_account": owner(),
        "expires_at": expires_at.map(|v| v.to_string())
    })
    .to_string()
}

fn list(contract: &mut Contract, token_id: &str, approval_id: u64, timestamp: u64) -> String {
    env(nft(), seller(), 0, timestamp);
    match contract.nft_on_approve(
        token_id.into(),
        seller(),
        approval_id,
        terms(token_id, &seller(), PRICE, None),
    ) {
        near_sdk::PromiseOrValue::Value(value) => value,
        _ => panic!("listing must return an id"),
    }
}

fn buy(contract: &mut Contract, listing_id: &str, who: AccountId, timestamp: u64) -> String {
    env(who.clone(), who.clone(), PRICE, timestamp);
    let _ = contract.buy_nft(listing_id.to_string());
    let generation = contract
        .listing_metadata(listing_id.to_string())
        .unwrap()
        .generation
        .0;
    Contract::sale_id(listing_id, &who, generation)
}

#[test]
fn successful_listing_is_bound_to_signed_terms_and_storage() {
    let mut contract = setup();
    let before = contract
        .storage_balance_of(seller())
        .unwrap()
        .available
        .as_yoctonear();
    let listing_id = list(&mut contract, "token-1", 7, 10);
    let listing = contract.listing(listing_id.clone()).unwrap();
    let meta = contract.listing_metadata(listing_id).unwrap();
    assert!(listing.active);
    assert_eq!(listing.approval_id, U64(7));
    assert_eq!(meta.generation, U64(1));
    assert!(
        contract
            .storage_balance_of(seller())
            .unwrap()
            .available
            .as_yoctonear()
            < before
    );
}

#[test]
fn registration_only_keeps_only_registration_cost_and_refunds_all_excess() {
    env(market(), market(), 0, 1);
    let mut contract = Contract::new(nft(), owner(), 100, None);

    env(seller(), seller(), ONE_NEAR, 2);
    let balance = contract.storage_deposit(None, Some(true));
    let pending = contract.pending_withdrawal(seller()).unwrap();
    let expected_refund = ONE_NEAR - balance.total.as_yoctonear();
    assert_eq!(pending.amount.0, expected_refund);
    assert_eq!(pending.kind, WithdrawalKind::RegistrationExcess);
    callback(PromiseResult::Successful(Vec::new()), 3);
    assert!(contract.on_withdrawal_result(seller(), U64(1)));
    let balance = contract.storage_balance_of(seller()).unwrap();
    assert_eq!(balance.available.as_yoctonear(), 0);
    assert_eq!(contract.ledger.storage_liability, 0);
    assert!(contract.pending_withdrawal(seller()).is_none());

    let registered_total = balance.total.as_yoctonear();
    env(seller(), seller(), ONE_NEAR, 4);
    let unchanged = contract.storage_deposit(None, Some(true));
    let pending = contract.pending_withdrawal(seller()).unwrap();
    assert_eq!(pending.amount.0, ONE_NEAR);
    assert_eq!(pending.kind, WithdrawalKind::RegistrationExcess);
    callback(PromiseResult::Successful(Vec::new()), 5);
    assert!(contract.on_withdrawal_result(seller(), U64(2)));
    assert_eq!(unchanged.total.as_yoctonear(), registered_total);
    let unchanged = contract.storage_balance_of(seller()).unwrap();
    assert_eq!(unchanged.total.as_yoctonear(), registered_total);
    assert_eq!(unchanged.available.as_yoctonear(), 0);
    assert_eq!(contract.ledger.storage_liability, 0);
    assert!(contract.pending_withdrawal(seller()).is_none());
}

#[test]
fn invalid_listing_terms_and_unallowed_contract_are_rejected() {
    let mut contract = setup();
    env(nft(), seller(), 0, 10);
    let bad = terms("different", &seller(), PRICE, None);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| contract.nft_on_approve(
            "token-1".into(),
            seller(),
            1,
            bad
        )))
        .is_err()
    );
    env(account("evil.testnet"), seller(), 0, 11);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| contract.nft_on_approve(
            "token-1".into(),
            seller(),
            1,
            terms("token-1", &seller(), PRICE, None)
        )))
        .is_err()
    );
}

#[test]
fn cancellation_is_authorized_and_releases_reserved_storage() {
    let mut contract = setup();
    let id = list(&mut contract, "token-1", 1, 10);
    let before = contract
        .storage_balance_of(seller())
        .unwrap()
        .available
        .as_yoctonear();
    env(buyer(), buyer(), 1, 11);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || contract.cancel_listing(id.clone())
    ))
    .is_err());
    env(seller(), seller(), 1, 12);
    contract.cancel_listing(id.clone());
    assert_eq!(
        contract.listing_metadata(id).unwrap().status,
        ListingStatus::Cancelled
    );
    assert!(
        contract
            .storage_balance_of(seller())
            .unwrap()
            .available
            .as_yoctonear()
            > before
    );
}

#[test]
fn expiration_is_permissionless_but_only_after_deadline() {
    let mut contract = setup();
    env(nft(), seller(), 0, 10);
    let id = match contract.nft_on_approve(
        "exp".into(),
        seller(),
        1,
        terms("exp", &seller(), PRICE, Some(20)),
    ) {
        near_sdk::PromiseOrValue::Value(value) => value,
        _ => unreachable!(),
    };
    env(other(), other(), 0, 19);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || contract.expire_listing(id.clone())
    ))
    .is_err());
    env(other(), other(), 0, 20);
    contract.expire_listing(id.clone());
    assert_eq!(
        contract.listing_metadata(id).unwrap().status,
        ListingStatus::Expired
    );
}

#[test]
fn successful_purchase_accounts_exact_seller_and_fee_proceeds() {
    let mut contract = setup();
    let listing_id = list(&mut contract, "token-1", 1, 10);
    let sale_id = buy(&mut contract, &listing_id, buyer(), 11);
    assert_eq!(contract.ledger.pending_sale_liability, PRICE);
    callback(PromiseResult::Successful(b"true".to_vec()), 12);
    let _ = contract.on_transfer_result(sale_id.clone());
    assert_eq!(contract.seller_proceeds(seller()).0, 990_000);
    assert_eq!(contract.fee_proceeds(owner()).0, 10_000);
    assert_eq!(
        contract.settlement(sale_id).unwrap().status,
        SettlementStatus::Settled
    );
}

#[test]
fn incorrect_amount_duplicate_and_concurrent_purchase_are_rejected() {
    let mut contract = setup();
    let id = list(&mut contract, "token-1", 1, 10);
    env(buyer(), buyer(), PRICE - 1, 11);
    assert!(std::panic::catch_unwind(
        std::panic::AssertUnwindSafe(|| contract.buy_nft(id.clone()))
    )
    .is_err());
    let _ = buy(&mut contract, &id, buyer(), 12);
    env(other(), other(), PRICE, 13);
    assert!(std::panic::catch_unwind(
        std::panic::AssertUnwindSafe(|| contract.buy_nft(id.clone()))
    )
    .is_err());
}

#[test]
fn failed_nft_transfer_creates_exact_withdrawable_refund_and_is_idempotent() {
    let mut contract = setup();
    let id = list(&mut contract, "token-1", 1, 10);
    let sale_id = buy(&mut contract, &id, buyer(), 11);
    callback(PromiseResult::Failed, 12);
    let _ = contract.on_transfer_result(sale_id.clone());
    assert_eq!(contract.buyer_refund(buyer()).0, PRICE);
    assert_eq!(
        contract.settlement(sale_id.clone()).unwrap().status,
        SettlementStatus::RefundAvailable
    );
    callback(PromiseResult::Failed, 13);
    let _ = contract.on_transfer_result(sale_id.clone());
    assert_eq!(contract.buyer_refund(buyer()).0, PRICE);
    env(buyer(), buyer(), ONE_NEAR, 13);
    contract.storage_deposit(None, None);
    env(buyer(), buyer(), 1, 14);
    let _ = contract.withdraw_refund(sale_id.clone());
    callback(PromiseResult::Successful(Vec::new()), 15);
    assert!(contract.on_withdrawal_result(buyer(), U64(1)));
    assert_eq!(
        contract.settlement(sale_id).unwrap().status,
        SettlementStatus::Refunded
    );
}

#[test]
fn failed_payout_restores_seller_funds() {
    let mut contract = setup();
    let id = list(&mut contract, "token-1", 1, 10);
    let sale_id = buy(&mut contract, &id, buyer(), 11);
    callback(PromiseResult::Successful(b"true".to_vec()), 12);
    let _ = contract.on_transfer_result(sale_id);
    env(seller(), seller(), 1, 13);
    let _ = contract.withdraw_proceeds(U128(990_000));
    assert_eq!(contract.seller_proceeds(seller()).0, 0);
    callback(PromiseResult::Failed, 14);
    assert!(!contract.on_withdrawal_result(seller(), U64(1)));
    assert_eq!(contract.seller_proceeds(seller()).0, 990_000);
}

#[test]
fn fee_withdrawal_is_isolated_by_recipient() {
    let mut contract = setup();
    let id = list(&mut contract, "token-1", 1, 10);
    let sale_id = buy(&mut contract, &id, buyer(), 11);
    callback(PromiseResult::Successful(b"true".to_vec()), 12);
    let _ = contract.on_transfer_result(sale_id);
    env(other(), other(), 1, 13);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || contract.withdraw_fees(U128(1))
    ))
    .is_err());
    env(owner(), owner(), 1, 14);
    let _ = contract.withdraw_fees(U128(10_000));
    assert_eq!(contract.fee_proceeds(owner()).0, 0);
}

#[test]
fn stale_callbacks_and_repeated_approval_ids_cannot_restore_or_replace_newer_sale() {
    let mut contract = setup();
    let first = list(&mut contract, "token-1", 1, 10);
    let second = list(&mut contract, "token-1", 2, 11);
    assert_eq!(
        contract.listing_metadata(first).unwrap().status,
        ListingStatus::Replaced
    );
    env(nft(), seller(), 0, 12);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| contract.nft_on_approve(
            "token-1".into(),
            seller(),
            1,
            terms("token-1", &seller(), PRICE, None)
        )))
        .is_err()
    );
    assert!(contract.listing(second).unwrap().active);
}

#[test]
fn trading_and_allowlist_administration_are_owner_only() {
    let mut contract = setup();
    env(other(), other(), 1, 10);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || contract.set_trading_enabled(false)
    ))
    .is_err());
    env(owner(), owner(), 1, 11);
    contract.set_nft_contract_allowed(account("second-nft.testnet"), true);
    assert!(contract
        .config()
        .allowed_nft_contracts
        .contains(&account("second-nft.testnet")));
}

#[test]
fn migration_preserves_legacy_liabilities_and_fails_closed() {
    env(market(), market(), 0, 1);
    let old = OldContract {
        config: Config {
            nft_contract_id: nft(),
            owner: owner(),
            fee_bps: 100,
            fee_account: None,
        },
        listings: LookupMap::new(b"l"),
        sales: LookupMap::new(b"s"),
        seller_proceeds: LookupMap::new(b"p"),
        ledger: OldLedger {
            seller_liability: 100,
            fee_liability: 10,
            seller_withdrawn: 25,
            fee_withdrawn: 2,
        },
    };
    env::state_write(&old);
    let migrated = Contract::migrate();
    assert!(!migrated.config().trading_enabled);
    assert_eq!(migrated.ledger.seller_liability, 75);
    assert_eq!(migrated.ledger.fee_liability, 8);
    assert_eq!(migrated.fee_proceeds(owner()).0, 8);
}

#[test]
fn bounded_history_and_full_liability_accounting_are_auditable() {
    let mut contract = setup();
    let id = list(&mut contract, "token-1", 1, 10);
    let sale_id = buy(&mut contract, &id, buyer(), 11);
    callback(PromiseResult::Successful(b"true".to_vec()), 12);
    let _ = contract.on_transfer_result(sale_id.clone());
    assert_eq!(contract.listings(Some(U64(0)), Some(500)).len(), 1);
    assert_eq!(
        contract.transactions(Some(U64(0)), Some(500))[0].sale_id,
        sale_id
    );
    assert_eq!(
        contract.ledger.total_liability(),
        contract.ledger.seller_liability
            + contract.ledger.fee_liability
            + contract.ledger.storage_liability
    );
}

#[test]
fn trading_starts_disabled_and_a_malicious_nft_cannot_forge_the_seller_signer() {
    env(market(), market(), 0, 1);
    let mut contract = Contract::new(nft(), owner(), 100, None);
    env(seller(), seller(), ONE_NEAR, 2);
    contract.storage_deposit(None, None);
    env(nft(), seller(), 0, 3);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| contract.nft_on_approve(
            "token".into(),
            seller(),
            1,
            terms("token", &seller(), PRICE, None)
        )))
        .is_err()
    );
    env(owner(), owner(), 1, 4);
    contract.set_trading_enabled(true);
    env(nft(), other(), 0, 5);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| contract.nft_on_approve(
            "token".into(),
            seller(),
            1,
            terms("token", &seller(), PRICE, None)
        )))
        .is_err()
    );
}

#[test]
fn listing_without_prefunded_storage_is_rejected() {
    env(market(), market(), 0, 1);
    let mut contract = Contract::new(nft(), owner(), 100, None);
    env(owner(), owner(), 1, 2);
    contract.set_trading_enabled(true);
    env(nft(), seller(), 0, 3);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| contract.nft_on_approve(
            "token".into(),
            seller(),
            1,
            terms("token", &seller(), PRICE, None)
        )))
        .is_err()
    );
}

#[test]
fn a_failed_refund_payout_restores_the_exact_refund() {
    let mut contract = setup();
    let id = list(&mut contract, "token-1", 1, 10);
    let sale_id = buy(&mut contract, &id, buyer(), 11);
    callback(PromiseResult::Failed, 12);
    let _ = contract.on_transfer_result(sale_id.clone());
    env(buyer(), buyer(), ONE_NEAR, 13);
    contract.storage_deposit(None, None);
    env(buyer(), buyer(), 1, 14);
    let _ = contract.withdraw_refund(sale_id.clone());
    callback(PromiseResult::Failed, 15);
    assert!(!contract.on_withdrawal_result(buyer(), U64(1)));
    assert_eq!(contract.buyer_refund(buyer()).0, PRICE);
    assert_eq!(
        contract.settlement(sale_id).unwrap().status,
        SettlementStatus::RefundAvailable
    );
}

#[test]
fn successful_withdrawal_and_callback_replay_cannot_pay_twice() {
    let mut contract = setup();
    let id = list(&mut contract, "token-1", 1, 10);
    let sale_id = buy(&mut contract, &id, buyer(), 11);
    callback(PromiseResult::Successful(b"true".to_vec()), 12);
    let _ = contract.on_transfer_result(sale_id);
    env(seller(), seller(), 1, 13);
    let _ = contract.withdraw_proceeds(U128(990_000));
    callback(PromiseResult::Successful(Vec::new()), 14);
    assert!(contract.on_withdrawal_result(seller(), U64(1)));
    callback(PromiseResult::Successful(Vec::new()), 15);
    assert!(!contract.on_withdrawal_result(seller(), U64(1)));
    assert_eq!(contract.ledger.seller_withdrawn, 990_000);
}

#[test]
fn fee_changes_apply_only_to_future_signed_listings() {
    let mut contract = setup();
    let old = list(&mut contract, "old", 1, 10);
    env(owner(), owner(), 1, 11);
    contract.set_fee_policy(500, Some(other()));
    let sale_id = buy(&mut contract, &old, buyer(), 12);
    callback(PromiseResult::Successful(b"true".to_vec()), 13);
    let _ = contract.on_transfer_result(sale_id);
    assert_eq!(contract.fee_proceeds(owner()).0, 10_000);
    assert_eq!(contract.fee_proceeds(other()).0, 0);
}

#[test]
fn storage_withdrawal_debits_the_exact_requested_amount() {
    let mut contract = setup();
    let before = contract
        .storage_balance_of(seller())
        .unwrap()
        .available
        .as_yoctonear();
    let amount = NearToken::from_millinear(10);
    env(seller(), seller(), 1, 10);
    contract.storage_withdraw(Some(amount));
    assert!(contract.pending_withdrawal(seller()).is_some());
    callback(PromiseResult::Successful(Vec::new()), 11);
    assert!(contract.on_withdrawal_result(seller(), U64(1)));
    let after = contract
        .storage_balance_of(seller())
        .unwrap()
        .available
        .as_yoctonear();
    assert_eq!(after, before - amount.as_yoctonear());
    assert!(contract.pending_withdrawal(seller()).is_none());
}

#[test]
fn maximum_storage_withdrawal_refunds_all_available_balance() {
    let mut contract = setup();
    let before = contract
        .storage_balance_of(seller())
        .unwrap()
        .available
        .as_yoctonear();
    assert!(before > 0);

    env(seller(), seller(), 1, 10);
    contract.storage_withdraw(None);
    callback(PromiseResult::Successful(Vec::new()), 11);
    assert!(contract.on_withdrawal_result(seller(), U64(1)));
    assert_eq!(
        contract
            .storage_balance_of(seller())
            .unwrap()
            .available
            .as_yoctonear(),
        0
    );
    assert!(contract.pending_withdrawal(seller()).is_none());
}

#[test]
fn failed_storage_withdraw_restores_exact_available_balance_and_liability() {
    let mut contract = setup();
    let before_balance = contract
        .storage_balance_of(seller())
        .unwrap()
        .available
        .as_yoctonear();
    let before_liability = contract.ledger.storage_liability;
    let amount = NearToken::from_millinear(10);
    env(seller(), seller(), 1, 10);
    contract.storage_withdraw(Some(amount));
    let pending = contract.pending_withdrawal(seller()).unwrap();
    assert_eq!(pending.amount.0, amount.as_yoctonear());
    assert_eq!(pending.kind, WithdrawalKind::StorageBalance);
    callback(PromiseResult::Failed, 11);
    assert!(!contract.on_withdrawal_result(seller(), U64(1)));
    assert_eq!(
        contract
            .storage_balance_of(seller())
            .unwrap()
            .available
            .as_yoctonear(),
        before_balance
    );
    assert_eq!(contract.ledger.storage_liability, before_liability);
    assert!(contract.pending_withdrawal(seller()).is_none());
    contract.assert_backed();
}

#[test]
fn failed_maximum_storage_withdraw_restores_full_available_balance() {
    let mut contract = setup();
    let before = contract
        .storage_balance_of(seller())
        .unwrap()
        .available
        .as_yoctonear();
    let before_liability = contract.ledger.storage_liability;
    assert!(before > 0);
    env(seller(), seller(), 1, 10);
    contract.storage_withdraw(None);
    callback(PromiseResult::Failed, 11);
    assert!(!contract.on_withdrawal_result(seller(), U64(1)));
    assert_eq!(
        contract
            .storage_balance_of(seller())
            .unwrap()
            .available
            .as_yoctonear(),
        before
    );
    assert_eq!(contract.ledger.storage_liability, before_liability);
    assert!(contract.pending_withdrawal(seller()).is_none());
    contract.assert_backed();
}

#[test]
fn failed_registration_only_refund_preserves_refundable_balance() {
    env(market(), market(), 0, 1);
    let mut contract = Contract::new(nft(), owner(), 100, None);
    env(seller(), seller(), ONE_NEAR, 2);
    let balance = contract.storage_deposit(None, Some(true));
    let registered_total = balance.total.as_yoctonear();
    let expected_refund = ONE_NEAR - registered_total;
    assert_eq!(balance.available.as_yoctonear(), 0);
    let pending = contract.pending_withdrawal(seller()).unwrap();
    assert_eq!(pending.amount.0, expected_refund);
    assert_eq!(pending.kind, WithdrawalKind::RegistrationExcess);
    assert_eq!(pending.recipient_id, seller());
    assert_eq!(contract.ledger.storage_liability, 0);

    callback(PromiseResult::Failed, 3);
    assert!(!contract.on_withdrawal_result(seller(), U64(1)));
    let restored = contract.storage_balance_of(seller()).unwrap();
    assert_eq!(restored.total.as_yoctonear(), ONE_NEAR);
    assert_eq!(restored.available.as_yoctonear(), expected_refund);
    assert_eq!(contract.ledger.storage_liability, expected_refund);
    assert!(contract.pending_withdrawal(seller()).is_none());
    contract.assert_backed();
}

#[test]
fn successful_registration_only_refund_debits_correct_amount() {
    env(market(), market(), 0, 1);
    let mut contract = Contract::new(nft(), owner(), 100, None);
    env(seller(), seller(), ONE_NEAR, 2);
    let balance = contract.storage_deposit(None, Some(true));
    let registered_total = balance.total.as_yoctonear();
    let expected_refund = ONE_NEAR - registered_total;
    callback(PromiseResult::Successful(Vec::new()), 3);
    assert!(contract.on_withdrawal_result(seller(), U64(1)));
    let after = contract.storage_balance_of(seller()).unwrap();
    assert_eq!(after.total.as_yoctonear(), registered_total);
    assert_eq!(after.available.as_yoctonear(), 0);
    assert_eq!(contract.ledger.storage_liability, 0);
    assert!(contract.pending_withdrawal(seller()).is_none());
    assert_eq!(expected_refund, ONE_NEAR - after.total.as_yoctonear());
    contract.assert_backed();
}

#[test]
fn first_storage_withdrawal_reserves_nonce_and_pending_control_storage() {
    env(market(), market(), 0, 1);
    let mut contract = Contract::new(nft(), owner(), 100, None);
    env(seller(), seller(), ONE_NEAR, 2);
    contract.storage_deposit(None, None);
    let before_reserve = contract.ledger.control_storage_reserve;
    env(seller(), seller(), 1, 3);
    contract.storage_withdraw(Some(NearToken::from_millinear(10)));
    let pending = contract.pending_withdrawal(seller()).unwrap();
    assert_eq!(pending.kind, WithdrawalKind::StorageBalance);
    assert!(pending.temporary_storage_cost.0 > 0);
    assert!(contract.ledger.control_storage_reserve > before_reserve);
    let min_reserve = pending.temporary_storage_cost.0;
    assert!(contract.ledger.control_storage_reserve >= before_reserve + min_reserve);
    contract.assert_backed();
}

#[test]
fn registration_only_refund_reserves_control_storage() {
    env(market(), market(), 0, 1);
    let mut contract = Contract::new(nft(), owner(), 100, None);
    env(seller(), seller(), ONE_NEAR, 2);
    contract.storage_deposit(None, Some(true));
    let pending = contract.pending_withdrawal(seller()).unwrap();
    assert_eq!(pending.kind, WithdrawalKind::RegistrationExcess);
    assert!(pending.temporary_storage_cost.0 > 0);
    assert!(contract.ledger.control_storage_reserve >= pending.temporary_storage_cost.0);
    contract.assert_backed();
}

#[test]
fn successful_storage_callback_releases_temporary_but_keeps_nonce_reserve() {
    env(market(), market(), 0, 1);
    let mut contract = Contract::new(nft(), owner(), 100, None);
    env(seller(), seller(), ONE_NEAR, 2);
    contract.storage_deposit(None, None);
    env(seller(), seller(), 1, 3);
    contract.storage_withdraw(Some(NearToken::from_millinear(10)));
    let pending = contract.pending_withdrawal(seller()).unwrap();
    let during = contract.ledger.control_storage_reserve;
    let temp = pending.temporary_storage_cost.0;
    callback(PromiseResult::Successful(Vec::new()), 4);
    assert!(contract.on_withdrawal_result(seller(), U64(1)));
    assert_eq!(contract.ledger.control_storage_reserve, during - temp);
    assert!(contract.ledger.control_storage_reserve > 0);
    contract.assert_backed();
}

#[test]
fn failed_storage_callback_restores_balances_and_releases_temporary_reserve() {
    env(market(), market(), 0, 1);
    let mut contract = Contract::new(nft(), owner(), 100, None);
    env(seller(), seller(), ONE_NEAR, 2);
    contract.storage_deposit(None, None);
    let before_balance = contract
        .storage_balance_of(seller())
        .unwrap()
        .available
        .as_yoctonear();
    let before_liability = contract.ledger.storage_liability;
    env(seller(), seller(), 1, 3);
    contract.storage_withdraw(Some(NearToken::from_millinear(10)));
    let pending = contract.pending_withdrawal(seller()).unwrap();
    let during = contract.ledger.control_storage_reserve;
    let temp = pending.temporary_storage_cost.0;
    callback(PromiseResult::Failed, 4);
    assert!(!contract.on_withdrawal_result(seller(), U64(1)));
    assert_eq!(
        contract
            .storage_balance_of(seller())
            .unwrap()
            .available
            .as_yoctonear(),
        before_balance
    );
    assert_eq!(contract.ledger.storage_liability, before_liability);
    assert_eq!(contract.ledger.control_storage_reserve, during - temp);
    assert!(contract.pending_withdrawal(seller()).is_none());
    contract.assert_backed();
}

#[test]
fn repeated_storage_withdrawals_do_not_double_charge_permanent_nonce_storage() {
    env(market(), market(), 0, 1);
    let mut contract = Contract::new(nft(), owner(), 100, None);
    env(seller(), seller(), ONE_NEAR, 2);
    contract.storage_deposit(None, None);
    env(seller(), seller(), 1, 3);
    contract.storage_withdraw(Some(NearToken::from_millinear(1)));
    let first_pending = contract.pending_withdrawal(seller()).unwrap();
    callback(PromiseResult::Successful(Vec::new()), 4);
    assert!(contract.on_withdrawal_result(seller(), U64(1)));
    let after_first = contract.ledger.control_storage_reserve;
    assert!(after_first > 0);

    env(seller(), seller(), 1, 5);
    contract.storage_withdraw(Some(NearToken::from_millinear(1)));
    let second_pending = contract.pending_withdrawal(seller()).unwrap();
    let during_second = contract.ledger.control_storage_reserve;
    assert_eq!(
        during_second,
        after_first + second_pending.temporary_storage_cost.0
    );
    assert!(first_pending.temporary_storage_cost.0 > 0);
    callback(PromiseResult::Successful(Vec::new()), 6);
    assert!(contract.on_withdrawal_result(seller(), U64(2)));
    assert_eq!(contract.ledger.control_storage_reserve, after_first);
    contract.assert_backed();
}

#[test]
fn insufficient_protocol_reserve_prevents_unsafe_storage_withdrawal_scheduling() {
    env(market(), market(), 0, 1);
    let mut contract = Contract::new(nft(), owner(), 100, None);
    env(seller(), seller(), ONE_NEAR, 2);
    contract.storage_deposit(None, None);
    env(seller(), seller(), 1, 3);
    // Simulate an over-reserved/under-backed protocol reserve state.
    contract.ledger.control_storage_reserve = contract.protocol_storage_available() + 1;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        contract.storage_withdraw(Some(NearToken::from_millinear(1)));
    }));
    assert!(result.is_err());
}

#[test]
fn assert_backed_detects_insufficient_backing_for_control_storage_reserve() {
    env(market(), market(), 0, 1);
    let contract = Contract::new(nft(), owner(), 100, None);
    let storage_stake = Contract::storage_cost(near_sdk::env::storage_usage());
    let liquid = near_sdk::env::account_balance()
        .as_yoctonear()
        .saturating_sub(storage_stake);
    let mut broken = contract;
    broken.ledger.control_storage_reserve = liquid + 1;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        broken.assert_backed();
    }));
    assert!(result.is_err());
}

#[test]
fn storage_withdrawal_callback_replay_cannot_pay_or_restore_twice() {
    let mut contract = setup();
    let amount = NearToken::from_millinear(10);
    let before = contract
        .storage_balance_of(seller())
        .unwrap()
        .available
        .as_yoctonear();
    let before_liability = contract.ledger.storage_liability;
    env(seller(), seller(), 1, 10);
    contract.storage_withdraw(Some(amount));
    callback(PromiseResult::Failed, 11);
    assert!(!contract.on_withdrawal_result(seller(), U64(1)));
    callback(PromiseResult::Successful(Vec::new()), 12);
    assert!(!contract.on_withdrawal_result(seller(), U64(1)));
    callback(PromiseResult::Failed, 13);
    assert!(!contract.on_withdrawal_result(seller(), U64(1)));
    assert_eq!(
        contract
            .storage_balance_of(seller())
            .unwrap()
            .available
            .as_yoctonear(),
        before
    );
    assert_eq!(contract.ledger.storage_liability, before_liability);
    contract.assert_backed();
}

#[test]
fn permissionless_multi_wallet_marketplace_lifecycle_is_enforced() {
    let mut contract = setup();
    let wallet_a = seller();
    let wallet_b = buyer();
    let price: u128 = 100_000_000_000_000_000_000_000; // 0.1 NEAR

    // 1. Wallet A funds storage and lists its own token
    let listing_a = list(&mut contract, "token-a", 1, 10);
    assert!(contract.listing(listing_a.clone()).unwrap().active);

    // 2. Wallet B attempts to list Wallet A's token without authorization (mismatched seller / signer)
    env(nft(), wallet_b.clone(), 0, 11);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        contract.nft_on_approve(
            "token-a".into(),
            wallet_a.clone(),
            2,
            terms("token-a", &wallet_a, price, None),
        )
    }))
    .is_err());

    // 3. Wallet B cannot cancel Wallet A's listing (unauthorized cancellation rejected)
    env(wallet_b.clone(), wallet_b.clone(), 1, 12);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        contract.cancel_listing(listing_a.clone())
    }))
    .is_err());
    assert!(contract.listing(listing_a.clone()).unwrap().active);

    // 4. Wallet B buys Wallet A's token
    let sale_id = buy(&mut contract, &listing_a, wallet_b.clone(), 13);
    callback(PromiseResult::Successful(b"true".to_vec()), 14);
    let _ = contract.on_transfer_result(sale_id.clone());
    let sale = contract.sale(sale_id.clone()).unwrap();
    assert!(sale.settled);
    assert_eq!(sale.buyer, wallet_b);
    assert!(!contract.listing(listing_a).unwrap().active);

    // Verify settlement accounting: 1% fee to fee_account, 99% to seller
    let (expected_seller, expected_fee) = Contract::split(PRICE, 100);
    assert_eq!(
        contract.seller_proceeds(wallet_a.clone()).0,
        expected_seller
    );
    assert_eq!(contract.fee_proceeds(owner()).0, expected_fee);

    // 5. Wallet A lists another token, and Wallet A (seller) cancels its own listing
    let listing_a2 = list(&mut contract, "token-a2", 3, 14);
    assert!(contract.listing(listing_a2.clone()).unwrap().active);
    env(wallet_a.clone(), wallet_a.clone(), 1, 15);
    contract.cancel_listing(listing_a2.clone());
    assert_eq!(
        contract.listing_metadata(listing_a2).unwrap().status,
        ListingStatus::Cancelled
    );
    contract.assert_backed();
}
