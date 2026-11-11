use near_marketplace_foundation::{
    authorize_public_action,
    ledger::{
        return_reserve, Admission, BalanceObservation, ClaimState, PaymentPolicy, ReservePolicy,
    },
    state::{ListingState, ListingTerms, Marketplace, TransferState},
    types::{Attempt, CallbackResult, FeeTerms, Generation, SaleId, TokenKey},
    AccountId, Error, PublicAction,
};

// All identities, rates, times and balances below are synthetic test fixtures.
// They are neither approved financial parameters nor deployable configuration.
fn account(name: &str) -> AccountId {
    name.parse().unwrap()
}
fn balance(amount: u128) -> BalanceObservation {
    BalanceObservation {
        account_balance: amount,
        ..Default::default()
    }
}
fn market() -> Marketplace {
    Marketplace::new(
        account("nft.testnet"),
        account("market.testnet"),
        Some(PaymentPolicy::new(Some(10)).unwrap()),
    )
    .unwrap()
}
fn terms(approval_id: u64) -> ListingTerms {
    ListingTerms {
        token: TokenKey::new(account("nft.testnet"), "art:1".into()).unwrap(),
        seller: account("seller.testnet"),
        approval_id,
        price: 100,
        expires_at_ns: 1_000,
        fees: FeeTerms::new(Some(1_000), Some(2_000), Some(account("fees.testnet"))).unwrap(),
        policy_version: 1,
        policy_hash: [1; 32],
        correlation: [2; 32],
    }
}
fn active(market: &mut Marketplace, terms: ListingTerms) -> Generation {
    let owner = terms.seller.clone();
    let approval = terms.approval_id;
    let generation = market.record_listing(terms, 1, [3; 32]).unwrap();
    let attempt = market.listing(generation).unwrap().verification();
    market
        .verify_listing(generation, attempt, Some(&owner), Some(approval), 2)
        .unwrap();
    generation
}
fn purchase(market: &mut Marketplace, generation: Generation) -> SaleId {
    let terms = market.listing(generation).unwrap().terms().clone();
    market
        .record_purchase(
            generation,
            &terms.token,
            account("buyer.testnet"),
            terms.price,
            3,
            [4; 32],
        )
        .unwrap()
}
fn result(market: &mut Marketplace, sale: SaleId, outcome: CallbackResult) -> Result<(), Error> {
    let record = market.sale(sale).unwrap();
    market.record_transfer_result(sale, record.generation(), record.transfer(), outcome)
}
fn allocated(outcome: CallbackResult) -> (Marketplace, SaleId) {
    let mut market = market();
    let generation = active(&mut market, terms(0));
    let sale = purchase(&mut market, generation);
    result(&mut market, sale, outcome).unwrap();
    (market, sale)
}
fn reserve_policy() -> ReservePolicy {
    ReservePolicy::new(Some(10), Some(10), Some(2), Some(50)).unwrap()
}

#[test]
fn every_public_action_is_unconditionally_disabled() {
    for action in [
        PublicAction::Mint,
        PublicAction::Deposit,
        PublicAction::List,
        PublicAction::Purchase,
        PublicAction::Withdraw,
        PublicAction::Recovery,
    ] {
        assert_eq!(authorize_public_action(action), Err(Error::Disabled));
    }
}

#[test]
fn listing_requires_the_pinned_nft_seller_and_explicit_fee_snapshot() {
    for mutation in 0..5 {
        let mut market = market();
        let mut request = terms(0);
        match mutation {
            0 => request.token = TokenKey::new(account("foreign.testnet"), "art:1".into()).unwrap(),
            1 => request.seller = account("market.testnet"),
            2 => request.policy_hash = [0; 32],
            3 => {
                request.fees =
                    FeeTerms::new(Some(0), Some(0), Some(account("market.testnet"))).unwrap()
            }
            _ => request.price = 0,
        }
        let before = market.clone();
        assert!(market.record_listing(request, 1, [3; 32]).is_err());
        assert_eq!(market, before);
    }
}

#[test]
fn direct_custody_revocation_and_reapproval_cannot_activate_a_candidate() {
    for (owner, approval) in [
        (Some(account("market.testnet")), Some(7)),
        (Some(account("seller.testnet")), None),
        (Some(account("seller.testnet")), Some(8)),
        (None, None),
    ] {
        let mut market = market();
        let generation = market.record_listing(terms(7), 1, [3; 32]).unwrap();
        let attempt = market.listing(generation).unwrap().verification();
        market
            .verify_listing(generation, attempt, owner.as_ref(), approval, 2)
            .unwrap();
        assert_eq!(
            market.listing(generation).unwrap().state(),
            ListingState::Invalid
        );
        // An invalid observation must not poison the authenticated watermark.
        // Runtime admission tombstones separately reject the same admission tuple.
        let next = market.record_listing(terms(6), 1, [4; 32]).unwrap();
        assert!(next.value() > generation.value());
        let before = market.clone();
        assert!(market
            .verify_listing(
                generation,
                attempt,
                Some(&account("seller.testnet")),
                Some(7),
                2
            )
            .is_err());
        assert_eq!(market, before);
    }
}

#[test]
fn cancellation_and_old_callbacks_cannot_resurrect_a_generation() {
    let mut market = market();
    let old = market.record_listing(terms(9), 1, [3; 32]).unwrap();
    let verification = market.listing(old).unwrap().verification();
    assert_eq!(
        market.cancel_listing(old, &account("attacker.testnet")),
        Err(Error::InvalidSeller)
    );
    market
        .cancel_listing(old, &account("seller.testnet"))
        .unwrap();
    // Cancelling an unverified higher ID does not consume lower real approvals.
    let new = active(&mut market, terms(8));
    assert!(new.value() > old.value());
    let before = market.clone();
    assert_eq!(
        market.verify_listing(
            old,
            verification,
            Some(&account("seller.testnet")),
            Some(9),
            2
        ),
        Err(Error::WrongState)
    );
    assert_eq!(market, before);
}

#[test]
fn buy_lock_excludes_second_buyer_cancel_and_new_listing() {
    let mut market = market();
    let generation = active(&mut market, terms(0));
    let sale = purchase(&mut market, generation);
    let before = market.clone();
    assert_eq!(
        market.record_purchase(
            generation,
            &terms(0).token,
            account("other.testnet"),
            100,
            4,
            [5; 32]
        ),
        Err(Error::Conflict)
    );
    assert_eq!(
        market.cancel_listing(generation, &account("seller.testnet")),
        Err(Error::WrongState)
    );
    assert_eq!(
        market.record_listing(terms(1), 4, [3; 32]),
        Err(Error::Conflict)
    );
    assert_eq!(market, before);
    assert_eq!(market.ledger().principal_summary(sale).unwrap().held, 100);
}

#[test]
fn cancellation_before_purchase_price_token_buyer_and_expiry_checks() {
    let mut market = market();
    let generation = active(&mut market, terms(0));
    for (buyer, price, time, token) in [
        ("seller.testnet", 100, 3, terms(0).token),
        ("market.testnet", 100, 3, terms(0).token),
        ("buyer.testnet", 99, 3, terms(0).token),
        ("buyer.testnet", 101, 3, terms(0).token),
        ("buyer.testnet", 100, 1_000, terms(0).token),
        (
            "buyer.testnet",
            100,
            3,
            TokenKey::new(account("nft.testnet"), "wrong".into()).unwrap(),
        ),
    ] {
        let before = market.clone();
        assert!(market
            .record_purchase(generation, &token, account(buyer), price, time, [4; 32])
            .is_err());
        assert_eq!(market, before);
    }
    market
        .cancel_listing(generation, &account("seller.testnet"))
        .unwrap();
    assert_eq!(
        market.record_purchase(
            generation,
            &terms(0).token,
            account("buyer.testnet"),
            100,
            3,
            [4; 32]
        ),
        Err(Error::Conflict)
    );
}

#[test]
fn expiration_verification_attempt_and_stale_listing_are_terminal() {
    let mut market = market();
    let generation = market.record_listing(terms(0), 1, [3; 32]).unwrap();
    let correct = market.listing(generation).unwrap().verification();
    let forged = Attempt {
        nonce: correct.nonce + 1,
        ..correct
    };
    let before = market.clone();
    assert_eq!(
        market.verify_listing(
            generation,
            forged,
            Some(&account("seller.testnet")),
            Some(0),
            2
        ),
        Err(Error::StaleAttempt)
    );
    assert_eq!(market, before);
    market
        .verify_listing(
            generation,
            correct,
            Some(&account("seller.testnet")),
            Some(0),
            1_000,
        )
        .unwrap();
    assert_eq!(
        market.listing(generation).unwrap().state(),
        ListingState::Invalid
    );
    let next = active(&mut market, terms(1));
    market.invalidate_listing(next).unwrap();
    assert_eq!(market.listing(next).unwrap().state(), ListingState::Stale);
    assert_eq!(
        market.record_listing(terms(1), 3, [3; 32]),
        Err(Error::StaleApproval)
    );
}

#[test]
fn unknown_transfer_keeps_principal_and_lock_until_matching_normal_callback() {
    let (mut market, sale) = allocated(CallbackResult::Unknown);
    let record = market.sale(sale).unwrap().clone();
    assert_eq!(record.outcome(), TransferState::Unknown);
    assert!(market.ledger().claims_for(sale).unwrap().is_empty());
    assert_eq!(market.ledger().principal_summary(sale).unwrap().held, 100);
    assert_eq!(
        market.record_listing(terms(1), 4, [3; 32]),
        Err(Error::Conflict)
    );
    let before = market.clone();
    let wrong = Attempt {
        operation_hash: [9; 32],
        ..record.transfer()
    };
    assert_eq!(
        market.record_transfer_result(sale, record.generation(), wrong, CallbackResult::Failed),
        Err(Error::StaleAttempt)
    );
    assert_eq!(market, before);
    result(&mut market, sale, CallbackResult::Succeeded).unwrap();
    assert_eq!(
        market.ledger().principal_summary(sale).unwrap().available,
        100
    );
    assert_eq!(
        result(&mut market, sale, CallbackResult::Failed),
        Err(Error::WrongState)
    );
}

#[test]
fn failed_wrapper_creates_one_full_refund_and_new_listing_cannot_rewrite_it() {
    let (mut market, sale) = allocated(CallbackResult::Failed);
    let claims = market.ledger().claims_for(sale).unwrap();
    assert_eq!(claims.len(), 1);
    let refund = market.ledger().claim(claims[0]).unwrap().clone();
    assert_eq!(refund.recipient(), &account("buyer.testnet"));
    assert_eq!(refund.role_amounts(), (0, 0, 100));
    let mut changed = terms(1);
    changed.price = 200;
    changed.fees = FeeTerms::new(Some(0), Some(0), Some(account("replacement.testnet"))).unwrap();
    let next = active(&mut market, changed);
    purchase(&mut market, next);
    let before = market.clone();
    assert_eq!(
        result(&mut market, sale, CallbackResult::Succeeded),
        Err(Error::WrongState)
    );
    assert_eq!(market, before);
    assert_eq!(market.ledger().claim(refund.id()), Some(&refund));
    assert_eq!(market.sale(sale).unwrap().terms().price, 100);
}

#[test]
fn transfer_result_permutations_never_allocate_principal_twice() {
    let outcomes = [
        CallbackResult::Unknown,
        CallbackResult::Succeeded,
        CallbackResult::Failed,
    ];
    for first in outcomes {
        for second in outcomes {
            for third in outcomes {
                let mut market = market();
                let generation = active(&mut market, terms(0));
                let sale = purchase(&mut market, generation);
                for outcome in [first, second, third] {
                    let before = market.clone();
                    if result(&mut market, sale, outcome).is_err() {
                        assert_eq!(market, before);
                    }
                    let summary = market.ledger().principal_summary(sale).unwrap();
                    assert_eq!(
                        summary.held + summary.available + summary.in_flight + summary.paid,
                        100
                    );
                }
            }
        }
    }
}

#[test]
fn fees_round_down_without_overflow_or_implicit_defaults() {
    assert_eq!(
        FeeTerms::new(None, Some(100), Some(account("fees.testnet"))),
        Err(Error::MissingConfiguration)
    );
    assert_eq!(
        FeeTerms::new(Some(0), None, Some(account("fees.testnet"))),
        Err(Error::MissingConfiguration)
    );
    assert_eq!(
        FeeTerms::new(Some(0), Some(0), None),
        Err(Error::MissingConfiguration)
    );
    assert_eq!(
        FeeTerms::new(Some(101), Some(100), Some(account("fees.testnet"))),
        Err(Error::InvalidFee)
    );
    assert_eq!(
        FeeTerms::new(Some(0), Some(10_000), Some(account("fees.testnet"))),
        Err(Error::InvalidFee)
    );
    for rate in [0, 1, 500, 9_999] {
        let fees = FeeTerms::new(Some(rate), Some(9_999), Some(account("fees.testnet"))).unwrap();
        for price in [1, 9_999, 10_000, 10_001, u128::MAX] {
            let fee = fees.fee(price).unwrap();
            assert!(fee < price);
            assert_eq!(price - fee + fee, price);
        }
    }
    assert_eq!(terms(0).fees.fee(1).unwrap(), 0);
}

#[test]
fn overlapping_beneficiaries_merge_and_zero_fee_creates_no_claim() {
    for merged in [false, true] {
        let mut market = market();
        let mut request = terms(0);
        request.fees = if merged {
            FeeTerms::new(Some(1_000), Some(2_000), Some(request.seller.clone())).unwrap()
        } else {
            FeeTerms::new(Some(0), Some(0), Some(account("fees.testnet"))).unwrap()
        };
        let generation = active(&mut market, request);
        let sale = purchase(&mut market, generation);
        result(&mut market, sale, CallbackResult::Succeeded).unwrap();
        let claims = market.ledger().claims_for(sale).unwrap();
        assert_eq!(claims.len(), 1);
        let claim = market.ledger().claim(claims[0]).unwrap();
        assert_eq!(claim.amount(), 100);
        assert_eq!(
            claim.role_amounts(),
            if merged { (90, 10, 0) } else { (100, 0, 0) }
        );
    }
}

#[test]
fn failed_payment_cannot_retry_without_actual_refund_liquidity_or_cooldown() {
    let (mut market, sale) = allocated(CallbackResult::Failed);
    let id = market.ledger().claims_for(sale).unwrap()[0];
    let ledger = market.ledger_mut();
    let first = ledger.dispatch(id, [5; 32], 10, balance(100)).unwrap();
    // A conclusive callback requires the runtime adapter to account for the debit.
    assert_eq!(
        ledger.record_payment_result(id, first, CallbackResult::Failed),
        Err(Error::WrongState)
    );
    ledger.record_debit(id, first).unwrap();
    ledger
        .record_payment_result(id, first, CallbackResult::Failed)
        .unwrap();
    assert_eq!(
        ledger.dispatch(id, [6; 32], 20, balance(0)),
        Err(Error::InsufficientBalance)
    );
    assert_eq!(
        ledger.dispatch(id, [6; 32], 19, balance(100)),
        Err(Error::TooSoon)
    );
    let second = ledger.dispatch(id, [6; 32], 20, balance(100)).unwrap();
    assert!(second.nonce > first.nonce);
    let before = ledger.clone();
    assert_eq!(
        ledger.record_payment_result(id, first, CallbackResult::Succeeded),
        Err(Error::StaleAttempt)
    );
    assert_eq!(*ledger, before);
    ledger.record_debit(id, second).unwrap();
    ledger
        .record_payment_result(id, second, CallbackResult::Succeeded)
        .unwrap();
    assert_eq!(ledger.principal_summary(sale).unwrap().paid, 100);
    assert_eq!(
        ledger.dispatch(id, [7; 32], 30, balance(100)),
        Err(Error::WrongState)
    );
    assert_eq!(
        ledger.record_payment_result(id, second, CallbackResult::Succeeded),
        Err(Error::WrongState)
    );
}

#[test]
fn unknown_payment_never_resends_and_delayed_success_resolves_only_once() {
    let (mut market, sale) = allocated(CallbackResult::Failed);
    let id = market.ledger().claims_for(sale).unwrap()[0];
    let ledger = market.ledger_mut();
    let attempt = ledger.dispatch(id, [5; 32], 10, balance(100)).unwrap();
    ledger
        .record_payment_result(id, attempt, CallbackResult::Unknown)
        .unwrap();
    assert_eq!(ledger.free_operating(balance(100)).unwrap(), 0); // pending debit Q
    assert_eq!(
        ledger.dispatch(id, [6; 32], u64::MAX, balance(1_000)),
        Err(Error::WrongState)
    );
    ledger.record_debit(id, attempt).unwrap();
    assert_eq!(ledger.free_operating(balance(0)).unwrap(), 0); // no second subtraction
    ledger
        .record_payment_result(id, attempt, CallbackResult::Succeeded)
        .unwrap();
    assert_eq!(ledger.principal_summary(sale).unwrap().paid, 100);
}

#[test]
fn split_payments_preserve_paid_claims_when_other_recipient_fails() {
    let (mut market, sale) = allocated(CallbackResult::Succeeded);
    let ids = market.ledger().claims_for(sale).unwrap().to_vec();
    let ledger = market.ledger_mut();
    let first_amount = ledger.claim(ids[0]).unwrap().amount();
    let first = ledger.dispatch(ids[0], [5; 32], 10, balance(100)).unwrap();
    let second = ledger.dispatch(ids[1], [6; 32], 10, balance(100)).unwrap();
    assert_eq!(ledger.free_operating(balance(100)).unwrap(), 0);
    ledger.record_debit(ids[0], first).unwrap();
    ledger.record_debit(ids[1], second).unwrap();
    ledger
        .record_payment_result(ids[0], first, CallbackResult::Succeeded)
        .unwrap();
    ledger
        .record_payment_result(ids[1], second, CallbackResult::Failed)
        .unwrap();
    assert_eq!(ledger.principal_summary(sale).unwrap().paid, first_amount);
    let before = ledger.clone();
    assert_eq!(
        ledger.dispatch(ids[1], [7; 32], 20, balance(0)),
        Err(Error::InsufficientBalance)
    );
    assert_eq!(*ledger, before);
    ledger
        .dispatch(ids[1], [7; 32], 20, balance(100 - first_amount))
        .unwrap();
    assert_eq!(ledger.claim(ids[0]).unwrap().state(), ClaimState::Paid);
}

#[test]
fn payment_callback_permutations_preserve_principal_and_terminal_claims() {
    let outcomes = [
        CallbackResult::Unknown,
        CallbackResult::Succeeded,
        CallbackResult::Failed,
    ];
    for first in outcomes {
        for second in outcomes {
            let (mut market, sale) = allocated(CallbackResult::Failed);
            let id = market.ledger().claims_for(sale).unwrap()[0];
            let ledger = market.ledger_mut();
            let attempt = ledger.dispatch(id, [5; 32], 10, balance(100)).unwrap();
            ledger.record_debit(id, attempt).unwrap();
            for outcome in [first, second] {
                let before = ledger.clone();
                if ledger.record_payment_result(id, attempt, outcome).is_err() {
                    assert_eq!(*ledger, before);
                }
                let summary = ledger.principal_summary(sale).unwrap();
                assert_eq!(summary.available + summary.in_flight + summary.paid, 100);
            }
        }
    }
}

#[test]
fn reserve_cannot_consume_purchase_principal_or_unattributed_cash() {
    let (mut market, sale) = allocated(CallbackResult::Unknown);
    let ledger = market.ledger_mut();
    assert_eq!(
        ledger.record_operating_credit([10; 32], 1, balance(100), balance(100)),
        Err(Error::InvalidOperation)
    );
    assert_eq!(
        ledger.reserve_operating(
            [11; 32],
            1,
            Admission::Return,
            Some(&reserve_policy()),
            balance(100)
        ),
        Err(Error::InsufficientReserve)
    );
    let observation = BalanceObservation {
        unattributed: 100,
        ..balance(200)
    };
    assert_eq!(
        ledger.record_operating_credit([10; 32], 100, observation, observation),
        Err(Error::InvalidOperation)
    );
    assert_eq!(ledger.principal_summary(sale).unwrap().held, 100);
}

#[test]
fn every_balance_partition_is_protected_and_reservation_cannot_be_reused() {
    let (mut market, _) = allocated(CallbackResult::Unknown);
    let ledger = market.ledger_mut();
    let observation = BalanceObservation {
        account_balance: 500,
        storage_stake: 50,
        external_queued: 60,
        storage_credits: 70,
        unattributed: 80,
    }; // 140 free after held principal and all other protected partitions
    ledger
        .record_operating_credit(
            [10; 32],
            140,
            BalanceObservation {
                account_balance: 360,
                ..observation
            },
            observation,
        )
        .unwrap();
    assert_eq!(
        ledger.record_operating_credit(
            [10; 32],
            140,
            BalanceObservation {
                account_balance: 360,
                ..observation
            },
            observation
        ),
        Err(Error::Conflict)
    );
    ledger
        .reserve_operating(
            [11; 32],
            20,
            Admission::ListingOrSale,
            Some(&reserve_policy()),
            observation,
        )
        .unwrap();
    assert_eq!(ledger.free_capital(), 120);
    assert_eq!(ledger.reserved_capital().unwrap(), 20);
    assert_eq!(ledger.free_operating(observation).unwrap(), 120);
    let before = ledger.clone();
    assert_eq!(
        ledger.reserve_operating(
            [11; 32],
            20,
            Admission::Return,
            Some(&reserve_policy()),
            observation
        ),
        Err(Error::Conflict)
    );
    assert_eq!(*ledger, before);
}

#[test]
fn reserve_hard_floor_target_exhaustion_and_missing_policy_fail_closed() {
    let mut market = market();
    let ledger = market.ledger_mut();
    let policy = reserve_policy();
    assert_eq!(policy.hard_floor(), 20);
    assert_eq!(policy.target(), 120);
    ledger
        .record_operating_credit([10; 32], 70, balance(0), balance(70))
        .unwrap();
    assert_eq!(
        ledger.reserve_operating([11; 32], 50, Admission::Return, None, balance(70)),
        Err(Error::MissingConfiguration)
    );
    assert_eq!(
        ledger.reserve_operating(
            [11; 32],
            50,
            Admission::ListingOrSale,
            Some(&policy),
            balance(70)
        ),
        Err(Error::InsufficientReserve)
    );
    ledger
        .reserve_operating([11; 32], 50, Admission::Return, Some(&policy), balance(70))
        .unwrap();
    let before = ledger.clone();
    assert_eq!(
        ledger.reserve_operating([12; 32], 1, Admission::Return, Some(&policy), balance(70)),
        Err(Error::InsufficientReserve)
    );
    assert_eq!(*ledger, before);
}

#[test]
fn shortfall_hold_preserves_debt_allows_callbacks_and_never_auto_resumes() {
    let (mut market, sale) = allocated(CallbackResult::Succeeded);
    let ids = market.ledger().claims_for(sale).unwrap().to_vec();
    let ledger = market.ledger_mut();
    let attempt = ledger.dispatch(ids[0], [5; 32], 10, balance(100)).unwrap();
    let sent = ledger.claim(ids[0]).unwrap().amount();
    ledger.record_debit(ids[0], attempt).unwrap();
    assert_eq!(
        ledger.check_health(balance(0)),
        Err(Error::InsufficientBalance)
    );
    assert!(ledger.financial_hold());
    ledger
        .record_payment_result(ids[0], attempt, CallbackResult::Succeeded)
        .unwrap();
    assert_eq!(
        ledger.dispatch(ids[1], [6; 32], 20, balance(100 - sent)),
        Err(Error::FinancialHold)
    );
    ledger
        .record_operating_credit([10; 32], 200, balance(100 - sent), balance(300 - sent))
        .unwrap();
    assert_eq!(
        ledger.check_health(balance(300 - sent)),
        Err(Error::FinancialHold)
    );
    assert_eq!(
        ledger.principal_summary(sale).unwrap().available,
        100 - sent
    );
    assert_eq!(
        market.record_listing(terms(1), 4, [3; 32]),
        Err(Error::FinancialHold)
    );
}

#[test]
fn arithmetic_overflow_and_unfunded_spends_do_not_mutate_state() {
    assert_eq!(
        return_reserve(Some(u128::MAX), Some(2), Some(1), Some(0)),
        Err(Error::Overflow)
    );
    assert_eq!(
        return_reserve(Some(1), Some(2), None, Some(0)),
        Err(Error::MissingConfiguration)
    );
    assert_eq!(
        return_reserve(Some(1), Some(2), Some(0), Some(0)),
        Err(Error::MissingConfiguration)
    );
    assert_eq!(
        return_reserve(Some(10), Some(20), Some(3), Some(7)).unwrap(),
        210
    );
    assert_eq!(
        ReservePolicy::new(Some(u128::MAX), Some(1), Some(1), Some(1)),
        Err(Error::Overflow)
    );
    assert_eq!(
        ReservePolicy::new(Some(0), Some(0), Some(2), Some(u128::MAX)),
        Err(Error::Overflow)
    );
    let (mut market, sale) = allocated(CallbackResult::Failed);
    let id = market.ledger().claims_for(sale).unwrap()[0];
    let ledger = market.ledger_mut();
    let before = ledger.clone();
    assert_eq!(
        ledger.dispatch(id, [5; 32], 10, balance(99)),
        Err(Error::InsufficientBalance)
    );
    assert_eq!(*ledger, before);
    let observation = BalanceObservation {
        storage_stake: u128::MAX,
        external_queued: 1,
        ..balance(u128::MAX)
    };
    assert_eq!(
        ledger.dispatch(id, [5; 32], 10, observation),
        Err(Error::Overflow)
    );
    assert_eq!(*ledger, before);
}

#[test]
fn missing_payment_policy_cannot_dispatch() {
    let mut market =
        Marketplace::new(account("nft.testnet"), account("market.testnet"), None).unwrap();
    let generation = active(&mut market, terms(0));
    let sale = purchase(&mut market, generation);
    result(&mut market, sale, CallbackResult::Failed).unwrap();
    let id = market.ledger().claims_for(sale).unwrap()[0];
    assert_eq!(
        market.ledger_mut().dispatch(id, [5; 32], 10, balance(100)),
        Err(Error::MissingConfiguration)
    );
}

#[test]
fn bounded_record_roundtrip_preserves_unknown_outcome_and_structured_token_identity() {
    let (market, sale) = allocated(CallbackResult::Unknown);
    let record = market.sale(sale).unwrap();
    let encoded = borsh::to_vec(record).unwrap();
    assert_eq!(
        &borsh::from_slice::<near_marketplace_foundation::state::Sale>(&encoded).unwrap(),
        record
    );
    let totals = market.ledger().totals();
    assert_eq!(
        borsh::from_slice::<near_marketplace_foundation::ledger::LedgerTotals>(
            &borsh::to_vec(&totals).unwrap()
        )
        .unwrap(),
        totals
    );
    let first = TokenKey::new(account("a.near"), "b.near:c".into()).unwrap();
    let second = TokenKey::new(account("b.near"), "a.near:c".into()).unwrap();
    assert_ne!(
        borsh::to_vec(&first).unwrap(),
        borsh::to_vec(&second).unwrap()
    );
    assert_ne!(
        TokenKey::new(account("a.near"), "Token".into()).unwrap(),
        TokenKey::new(account("a.near"), "token".into()).unwrap()
    );
}

#[test]
fn exhausted_approval_space_cannot_wrap_to_recreate_a_listing() {
    let mut market = market();
    let generation = active(&mut market, terms(u64::MAX));
    market
        .cancel_listing(generation, &account("seller.testnet"))
        .unwrap();
    for approval in [0, u64::MAX - 1, u64::MAX] {
        assert_eq!(
            market.record_listing(terms(approval), 3, [3; 32]),
            Err(Error::StaleApproval)
        );
    }
}

#[test]
fn exact_shortfall_partial_and_surplus_contributions_preserve_principal() {
    for amount in [5, 10, 25] {
        let (mut market, sale) = allocated(CallbackResult::Failed);
        let ledger = market.ledger_mut();
        let claim = ledger
            .claim(ledger.claims_for(sale).unwrap()[0])
            .unwrap()
            .clone();
        let before_summary = ledger.principal_summary(sale).unwrap();
        let credit = ledger
            .record_operating_credit([42; 32], amount, balance(90), balance(90 + amount))
            .unwrap();
        assert_eq!(credit.deficit_repair, amount.min(10));
        assert_eq!(credit.free_capital_added, amount.saturating_sub(10));
        assert_eq!(ledger.free_capital(), amount.saturating_sub(10));
        assert_eq!(ledger.contribution([42; 32]), Some(credit));
        assert_eq!(ledger.principal_summary(sale).unwrap(), before_summary);
        assert_eq!(ledger.claim(claim.id()), Some(&claim));
        assert!(ledger.financial_hold());
        assert_eq!(
            ledger.dispatch(claim.id(), [5; 32], 10, balance(90 + amount)),
            Err(Error::FinancialHold)
        );
        let before = ledger.clone();
        assert_eq!(
            ledger.record_operating_credit([42; 32], amount, balance(90), balance(90 + amount)),
            Err(Error::Conflict)
        );
        assert_eq!(*ledger, before);
    }
}

#[test]
fn sequential_repairs_and_reserved_capital_shortfall_do_not_create_free_money() {
    let (mut market, sale) = allocated(CallbackResult::Unknown);
    let ledger = market.ledger_mut();
    ledger
        .record_operating_credit([10; 32], 200, balance(100), balance(300))
        .unwrap();
    ledger
        .reserve_operating(
            [11; 32],
            50,
            Admission::Return,
            Some(&reserve_policy()),
            balance(300),
        )
        .unwrap();
    // All principal + reserved and free capital require 300; actual assets are 280.
    let first = ledger
        .record_operating_credit([12; 32], 5, balance(280), balance(285))
        .unwrap();
    let second = ledger
        .record_operating_credit([13; 32], 15, balance(285), balance(300))
        .unwrap();
    assert_eq!(first.deficit_repair, 5);
    assert_eq!(second.deficit_repair, 15);
    assert_eq!(ledger.free_capital(), 150);
    assert_eq!(ledger.reserved_capital(), Ok(50));
    assert_eq!(ledger.principal_summary(sale).unwrap().held, 100);
    assert_eq!(ledger.totals().deficit_repairs, 20);
    assert_eq!(ledger.check_health(balance(300)), Err(Error::FinancialHold));
}

#[test]
fn contribution_rejects_fabricated_balance_delta_and_liability_reclassification() {
    let (mut market, _) = allocated(CallbackResult::Unknown);
    let ledger = market.ledger_mut();
    for after in [
        balance(100),
        balance(111),
        BalanceObservation {
            unattributed: 0,
            ..balance(110)
        },
    ] {
        let before = ledger.clone();
        let observation = BalanceObservation {
            unattributed: 10,
            ..balance(100)
        };
        assert_eq!(
            ledger.record_operating_credit([10; 32], 10, observation, after),
            Err(Error::InvalidOperation)
        );
        assert_eq!(*ledger, before);
    }
    let before = ledger.clone();
    assert_eq!(
        ledger.record_operating_credit([10; 32], 1, balance(u128::MAX), balance(0)),
        Err(Error::Overflow)
    );
    assert_eq!(*ledger, before);
}
