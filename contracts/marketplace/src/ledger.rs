//! Principal and operating-capital accounting. No native NEAR is accepted or
//! sent here. Balance/debit/result inputs are trusted runtime-adapter facts,
//! never evidence inferred from wallet responses or balance changes alone.
use std::collections::BTreeMap;

use borsh::{BorshDeserialize, BorshSerialize};

use crate::{
    records::Records,
    state::Sale,
    types::{Attempt, CallbackResult, ClaimId, Generation, SaleId},
    AccountId, Error, Result,
};

fn add(a: u128, b: u128) -> Result<u128> {
    a.checked_add(b).ok_or(Error::Overflow)
}
fn sub(a: u128, b: u128) -> Result<u128> {
    a.checked_sub(b).ok_or(Error::InsufficientBalance)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PaymentPolicy {
    pub(crate) retry_delay_ns: u64,
}

impl PaymentPolicy {
    pub fn new(retry_delay_ns: Option<u64>) -> Result<Self> {
        let retry_delay_ns = retry_delay_ns.ok_or(Error::MissingConfiguration)?;
        if retry_delay_ns == 0 {
            return Err(Error::MissingConfiguration);
        }
        Ok(Self { retry_delay_ns })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::tests::{buy, fixture};

    fn audit(ledger: &Ledger) {
        let mut expected = LedgerTotals::default();
        for (_, principal) in ledger.principals.iter() {
            expected.principal += principal.price;
            expected.held += principal.held;
        }
        for (_, claim) in ledger.claims.iter() {
            match claim.state {
                ClaimState::Available | ClaimState::Retryable => expected.available += claim.amount,
                ClaimState::InFlight | ClaimState::Unknown => {
                    expected.in_flight += claim.amount;
                    if claim.debit_pending {
                        expected.queued_payments += claim.amount;
                    }
                }
                ClaimState::Paid => expected.paid += claim.amount,
            }
        }
        for (_, amount) in ledger.reservations.iter() {
            expected.reserved_operating += amount;
        }
        for (_, credit) in ledger.operating_credits.iter() {
            expected.contributions += credit.amount;
            expected.deficit_repairs += credit.deficit_repair;
            assert_eq!(
                credit.amount,
                credit.deficit_repair + credit.free_capital_added
            );
        }
        assert_eq!(ledger.totals, expected);
        assert_eq!(
            ledger.free_capital + expected.reserved_operating,
            expected.contributions - expected.deficit_repairs
        );
        expected.validate().unwrap();
    }

    #[test]
    fn maintained_totals_match_independent_history_after_every_transition() {
        for delivered in [true, false] {
            for payment_result in [CallbackResult::Succeeded, CallbackResult::Failed] {
                let (mut market, generation) = fixture(100);
                let sale = buy(&mut market, generation).unwrap();
                audit(market.ledger());
                let transfer = market.sale(sale).unwrap().transfer();
                market
                    .record_transfer_result(sale, generation, transfer, CallbackResult::Unknown)
                    .unwrap();
                audit(market.ledger());
                market
                    .record_transfer_result(
                        sale,
                        generation,
                        transfer,
                        if delivered {
                            CallbackResult::Succeeded
                        } else {
                            CallbackResult::Failed
                        },
                    )
                    .unwrap();
                let ledger = market.ledger_mut();
                audit(ledger);
                let observation = BalanceObservation {
                    account_balance: 1_000,
                    ..Default::default()
                };
                ledger
                    .record_operating_credit(
                        [8; 32],
                        900,
                        BalanceObservation {
                            account_balance: 100,
                            ..observation
                        },
                        observation,
                    )
                    .unwrap();
                audit(ledger);
                let policy = ReservePolicy::new(Some(1), Some(1), Some(1), Some(100)).unwrap();
                ledger
                    .reserve_operating([9; 32], 100, Admission::Return, Some(&policy), observation)
                    .unwrap();
                audit(ledger);
                let ids = ledger.claims_for(sale).unwrap().to_vec();
                let mut liquid = 1_000;
                for id in ids {
                    let amount = ledger.claim(id).unwrap().amount();
                    let attempt = ledger
                        .dispatch(
                            id,
                            [5; 32],
                            10,
                            BalanceObservation {
                                account_balance: liquid,
                                ..observation
                            },
                        )
                        .unwrap();
                    audit(ledger);
                    ledger
                        .record_payment_result(id, attempt, CallbackResult::Unknown)
                        .unwrap();
                    audit(ledger);
                    ledger.record_debit(id, attempt).unwrap();
                    liquid -= amount;
                    audit(ledger);
                    ledger.record_debit(id, attempt).unwrap(); // duplicate debit never subtracts twice
                    audit(ledger);
                    ledger
                        .record_payment_result(id, attempt, payment_result)
                        .unwrap();
                    audit(ledger);
                    if payment_result == CallbackResult::Failed {
                        liquid += amount; // actual refunded cash, not an expectation
                        let retry = ledger
                            .dispatch(
                                id,
                                [6; 32],
                                20,
                                BalanceObservation {
                                    account_balance: liquid,
                                    ..observation
                                },
                            )
                            .unwrap();
                        audit(ledger);
                        ledger.record_debit(id, retry).unwrap();
                        liquid -= amount;
                        audit(ledger);
                        ledger
                            .record_payment_result(id, retry, CallbackResult::Succeeded)
                            .unwrap();
                        audit(ledger);
                    }
                    let before = ledger.clone();
                    assert!(ledger
                        .record_payment_result(id, attempt, CallbackResult::Failed)
                        .is_err());
                    assert_eq!(*ledger, before);
                }
                assert_eq!(ledger.totals.paid, 100);
            }
        }
    }

    #[test]
    fn backing_checks_access_no_history_records_even_after_many_sales() {
        let (mut market, generation) = fixture(1);
        let request = market.listing(generation).unwrap().terms().clone();
        for index in 0..2_000 {
            let g = if index == 0 {
                generation
            } else {
                let mut next = request.clone();
                next.approval_id = index;
                let g = market.record_listing(next.clone(), 1, [3; 32]).unwrap();
                let verify = market.listing(g).unwrap().verification();
                market
                    .verify_listing(g, verify, Some(&next.seller), Some(index), 2)
                    .unwrap();
                g
            };
            let sale = buy(&mut market, g).unwrap();
            let attempt = market.sale(sale).unwrap().transfer();
            market
                .record_transfer_result(sale, g, attempt, CallbackResult::Failed)
                .unwrap();
            let id = market.ledger().claims_for(sale).unwrap()[0];
            let ledger = market.ledger_mut();
            let attempt = ledger
                .dispatch(
                    id,
                    [5; 32],
                    10,
                    BalanceObservation {
                        account_balance: 1,
                        ..Default::default()
                    },
                )
                .unwrap();
            ledger.record_debit(id, attempt).unwrap();
            ledger
                .record_payment_result(id, attempt, CallbackResult::Succeeded)
                .unwrap();
        }
        let ledger = market.ledger();
        audit(ledger);
        assert_eq!(ledger.totals.paid, 2_000);
        assert_eq!(ledger.free_operating(BalanceObservation::default()), Ok(0));
        // The production Records API exposes only point lookups; its iterator is
        // compiled solely for this independent test audit, never for backing checks.
    }

    #[test]
    fn claim_id_exhaustion_rolls_back_the_entire_settlement_transition() {
        for last in [u64::MAX - 1, u64::MAX] {
            let (mut market, generation) = fixture(100);
            let sale = buy(&mut market, generation).unwrap();
            market.ledger_mut().last_claim = last;
            let attempt = market.sale(sale).unwrap().transfer();
            let before = market.clone();
            assert_eq!(
                market.record_transfer_result(sale, generation, attempt, CallbackResult::Succeeded),
                Err(Error::Overflow)
            );
            assert_eq!(market, before);
        }
    }

    #[test]
    fn payment_attempt_exhaustion_cannot_reset_or_resend() {
        let (mut market, generation) = fixture(100);
        let sale = buy(&mut market, generation).unwrap();
        let transfer = market.sale(sale).unwrap().transfer();
        market
            .record_transfer_result(sale, generation, transfer, CallbackResult::Failed)
            .unwrap();
        let id = market.ledger().claims_for(sale).unwrap()[0];
        let ledger = market.ledger_mut();
        let claim = ledger.claims.get_mut(&id).unwrap();
        claim.state = ClaimState::Retryable;
        claim.attempt = Some(Attempt {
            nonce: u64::MAX,
            operation_hash: [5; 32],
        });
        claim.last_dispatch_ns = Some(0);
        let before = ledger.clone();
        let balance = BalanceObservation {
            account_balance: 100,
            ..Default::default()
        };
        assert_eq!(
            ledger.dispatch(id, [6; 32], 10, balance),
            Err(Error::Overflow)
        );
        assert_eq!(*ledger, before);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum ClaimState {
    Available,
    InFlight,
    Unknown,
    Retryable,
    Paid,
}

/// Role allocations are retained even when seller == treasury and one transfer
/// pays both. Zero-valued obligations are never created.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Claim {
    pub(crate) id: ClaimId,
    pub(crate) sale: SaleId,
    pub(crate) generation: Generation,
    pub(crate) recipient: AccountId,
    pub(crate) seller_amount: u128,
    pub(crate) fee_amount: u128,
    pub(crate) refund_amount: u128,
    pub(crate) amount: u128,
    pub(crate) state: ClaimState,
    pub(crate) attempt: Option<Attempt>,
    pub(crate) last_dispatch_ns: Option<u64>,
    pub(crate) debit_pending: bool,
}

impl Claim {
    pub fn id(&self) -> ClaimId {
        self.id
    }
    pub fn sale(&self) -> SaleId {
        self.sale
    }
    pub fn generation(&self) -> Generation {
        self.generation
    }
    pub fn recipient(&self) -> &AccountId {
        &self.recipient
    }
    pub fn amount(&self) -> u128 {
        self.amount
    }
    pub fn role_amounts(&self) -> (u128, u128, u128) {
        (self.seller_amount, self.fee_amount, self.refund_amount)
    }
    pub fn state(&self) -> ClaimState {
        self.state
    }
    pub fn attempt(&self) -> Option<Attempt> {
        self.attempt
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub(crate) struct Principal {
    pub(crate) generation: Generation,
    pub(crate) price: u128,
    pub(crate) seller: AccountId,
    pub(crate) treasury: AccountId,
    pub(crate) buyer: AccountId,
    pub(crate) fee: u128,
    pub(crate) held: u128,
    pub(crate) allocated: bool,
    pub(crate) claims: Vec<ClaimId>,
}

/// A/S/Q/L/U inputs from one execution boundary. `external_queued` excludes
/// payment commitments tracked below; `storage_credits` excludes storage stake.
/// Neither expected refunds nor expected top-ups belong in account_balance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BalanceObservation {
    pub account_balance: u128,
    pub storage_stake: u128,
    pub external_queued: u128,
    pub storage_credits: u128,
    pub unattributed: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrincipalSummary {
    pub principal: u128,
    pub held: u128,
    pub available: u128,
    pub in_flight: u128,
    pub paid: u128,
}

/// Fixed-size maintained totals. Updates are checked before record mutation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct LedgerTotals {
    pub principal: u128,
    pub held: u128,
    pub available: u128,
    pub in_flight: u128,
    pub paid: u128,
    pub queued_payments: u128,
    pub reserved_operating: u128,
    pub contributions: u128,
    pub deficit_repairs: u128,
}

impl LedgerTotals {
    pub(crate) fn validate(&self) -> Result<()> {
        if add(
            add(self.held, self.available)?,
            add(self.in_flight, self.paid)?,
        )? != self.principal
            || self.queued_payments > self.in_flight
            || self.deficit_repairs > self.contributions
        {
            return Err(Error::AccountingInvariant);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct OperatingContribution {
    pub amount: u128,
    pub deficit_repair: u128,
    pub free_capital_added: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReservePolicy {
    hard_floor: u128,
    target: u128,
    max_operation: u128,
}

impl ReservePolicy {
    /// Explicit proposed values, not governance approval. No financial defaults.
    pub fn new(
        emergency: Option<u128>,
        safety: Option<u128>,
        headroom: Option<u64>,
        max_operation: Option<u128>,
    ) -> Result<Self> {
        let hard_floor = add(
            emergency.ok_or(Error::MissingConfiguration)?,
            safety.ok_or(Error::MissingConfiguration)?,
        )?;
        let maximum = max_operation.ok_or(Error::MissingConfiguration)?;
        if maximum == 0 {
            return Err(Error::MissingConfiguration);
        }
        let capacity = maximum
            .checked_mul(u128::from(headroom.ok_or(Error::MissingConfiguration)?))
            .ok_or(Error::Overflow)?;
        Ok(Self {
            hard_floor,
            target: add(hard_floor, capacity)?,
            max_operation: maximum,
        })
    }
    pub fn hard_floor(&self) -> u128 {
        self.hard_floor
    }
    pub fn target(&self) -> u128 {
        self.target
    }
}

/// Calculates p * B(N) + N * 1 yocto + M. `remaining_bytes` must come from
/// measured runtime bounds including all funded attempts, excluding bytes in S.
/// This quotes accounting only; no deposit or return operation is implemented.
pub fn return_reserve(
    byte_cost: Option<u128>,
    remaining_bytes: Option<u64>,
    attempts: Option<u64>,
    margin: Option<u128>,
) -> Result<u128> {
    let price = byte_cost.ok_or(Error::MissingConfiguration)?;
    let bytes = remaining_bytes.ok_or(Error::MissingConfiguration)?;
    let attempts = attempts.ok_or(Error::MissingConfiguration)?;
    let margin = margin.ok_or(Error::MissingConfiguration)?;
    if price == 0 || attempts == 0 {
        return Err(Error::MissingConfiguration);
    }
    add(
        add(
            price
                .checked_mul(u128::from(bytes))
                .ok_or(Error::Overflow)?,
            u128::from(attempts),
        )?,
        margin,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Return,
    ListingOrSale,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ledger {
    pub(crate) principals: Records<SaleId, Principal>,
    pub(crate) claims: Records<ClaimId, Claim>,
    pub(crate) last_claim: u64,
    pub(crate) payment_policy: Option<PaymentPolicy>,
    pub(crate) free_capital: u128,
    pub(crate) reservations: Records<[u8; 32], u128>,
    pub(crate) operating_credits: Records<[u8; 32], OperatingContribution>,
    pub(crate) totals: LedgerTotals,
    pub(crate) financial_hold: bool,
}

impl Ledger {
    pub(crate) fn new(payment_policy: Option<PaymentPolicy>) -> Self {
        Self {
            principals: Records::new(),
            claims: Records::new(),
            last_claim: 0,
            payment_policy,
            free_capital: 0,
            reservations: Records::new(),
            operating_credits: Records::new(),
            totals: LedgerTotals::default(),
            financial_hold: false,
        }
    }

    pub fn financial_hold(&self) -> bool {
        self.financial_hold
    }
    pub fn free_capital(&self) -> u128 {
        self.free_capital
    }
    pub fn reserved_capital(&self) -> Result<u128> {
        Ok(self.totals.reserved_operating)
    }
    pub fn totals(&self) -> LedgerTotals {
        self.totals
    }

    pub fn storage_protection(&self) -> crate::storage_accounting::LegacyProtection {
        crate::storage_accounting::LegacyProtection {
            totals: self.totals,
            free_capital: self.free_capital,
            financial_hold: self.financial_hold,
        }
    }
    pub fn contribution(&self, id: [u8; 32]) -> Option<OperatingContribution> {
        self.operating_credits.get(&id).copied()
    }
    pub fn claim(&self, id: ClaimId) -> Option<&Claim> {
        self.claims.get(&id)
    }
    pub fn claims_for(&self, sale: SaleId) -> Result<&[ClaimId]> {
        Ok(&self.principals.get(&sale).ok_or(Error::UnknownSale)?.claims)
    }

    pub(crate) fn hold_principal(&mut self, sale: &Sale) -> Result<()> {
        if self.principals.contains_key(&sale.id()) {
            return Err(Error::Conflict);
        }
        let mut totals = self.totals;
        totals.principal = add(totals.principal, sale.terms().price)?;
        totals.held = add(totals.held, sale.terms().price)?;
        totals.validate()?;
        let fee = sale.terms().fees.fee(sale.terms().price)?;
        self.principals.insert(
            sale.id(),
            Principal {
                generation: sale.generation(),
                price: sale.terms().price,
                seller: sale.terms().seller.clone(),
                treasury: sale.terms().fees.treasury().clone(),
                buyer: sale.buyer().clone(),
                fee,
                held: sale.terms().price,
                allocated: false,
                claims: Vec::new(),
            },
        );
        self.totals = totals;
        Ok(())
    }

    pub(crate) fn allocate(&mut self, sale: SaleId, delivered: bool) -> Result<()> {
        let principal = self.principals.get(&sale).ok_or(Error::UnknownSale)?;
        if principal.allocated {
            return Err(Error::WrongState);
        }
        let mut totals = self.totals;
        totals.held = sub(totals.held, principal.price)?;
        totals.available = add(totals.available, principal.price)?;
        totals.validate()?;
        let mut allocations: BTreeMap<AccountId, (u128, u128, u128)> = BTreeMap::new();
        if delivered {
            allocations.insert(
                principal.seller.clone(),
                (principal.price - principal.fee, 0, 0),
            );
            if principal.fee > 0 {
                allocations.entry(principal.treasury.clone()).or_default().1 = principal.fee;
            }
        } else {
            allocations.insert(principal.buyer.clone(), (0, 0, principal.price));
        }
        let mut next = self.last_claim;
        let mut prepared = Vec::new();
        for (recipient, (seller_amount, fee_amount, refund_amount)) in allocations {
            next = next.checked_add(1).ok_or(Error::Overflow)?;
            let amount = add(add(seller_amount, fee_amount)?, refund_amount)?;
            prepared.push(Claim {
                id: ClaimId(next),
                sale,
                generation: principal.generation,
                recipient,
                seller_amount,
                fee_amount,
                refund_amount,
                amount,
                state: ClaimState::Available,
                attempt: None,
                last_dispatch_ns: None,
                debit_pending: false,
            });
        }
        let principal = self.principals.get_mut(&sale).unwrap();
        principal.claims = prepared.iter().map(|claim| claim.id).collect();
        principal.held = 0;
        principal.allocated = true;
        for claim in prepared {
            self.claims.insert(claim.id, claim);
        }
        self.last_claim = next;
        self.totals = totals;
        Ok(())
    }

    pub fn principal_summary(&self, sale: SaleId) -> Result<PrincipalSummary> {
        let principal = self.principals.get(&sale).ok_or(Error::UnknownSale)?;
        // Fixed v1 maximum: seller + fee, or one refund. Never traverse history.
        if principal.claims.len() > 2 {
            return Err(Error::AccountingInvariant);
        }
        let mut summary = PrincipalSummary {
            principal: principal.price,
            held: principal.held,
            available: 0,
            in_flight: 0,
            paid: 0,
        };
        for id in &principal.claims {
            let claim = &self.claims[id];
            let bucket = match claim.state {
                ClaimState::Available | ClaimState::Retryable => &mut summary.available,
                ClaimState::InFlight | ClaimState::Unknown => &mut summary.in_flight,
                ClaimState::Paid => &mut summary.paid,
            };
            *bucket = add(*bucket, claim.amount)?;
        }
        if add(
            add(summary.held, summary.available)?,
            add(summary.in_flight, summary.paid)?,
        )? != principal.price
        {
            return Err(Error::InsufficientBalance);
        }
        Ok(summary)
    }

    /// A - S - Q - L - U - C. Outstanding debited sends remain in principal
    /// history, but are not subtracted from A twice. Pending debits remain in Q.
    fn protected_balance(&self, balance: BalanceObservation) -> Result<u128> {
        self.totals.validate()?;
        let external = add(
            add(balance.storage_stake, balance.external_queued)?,
            add(balance.storage_credits, balance.unattributed)?,
        )?;
        add(
            external,
            add(
                add(self.totals.held, self.totals.available)?,
                add(self.totals.queued_payments, self.totals.reserved_operating)?,
            )?,
        )
    }

    pub fn free_operating(&self, balance: BalanceObservation) -> Result<u128> {
        sub(balance.account_balance, self.protected_balance(balance)?)
    }

    fn check_backing(&self, balance: BalanceObservation) -> Result<u128> {
        let free = self.free_operating(balance)?;
        if free < self.free_capital {
            return Err(Error::InsufficientBalance);
        }
        Ok(free)
    }

    /// Separate non-panicking latch; a caller may persist it even when a send
    /// would revert. No clear/resume function exists pending governance approval.
    pub fn check_health(&mut self, balance: BalanceObservation) -> Result<()> {
        if let Err(error) = self.check_backing(balance) {
            self.financial_hold = true;
            return Err(error);
        }
        if self.financial_hold {
            return Err(Error::FinancialHold);
        }
        Ok(())
    }

    /// Adapter-only accounting for an identified, actually credited contribution.
    /// The future funding method/terms require approval. Direct transfers cannot
    /// call this library and unexpected money stays unattributed. No sweep exists.
    pub fn record_operating_credit(
        &mut self,
        credit: [u8; 32],
        amount: u128,
        before: BalanceObservation,
        after: BalanceObservation,
    ) -> Result<OperatingContribution> {
        if credit == [0; 32] || amount == 0 {
            return Err(Error::InvalidOperation);
        }
        if self.operating_credits.contains_key(&credit) {
            return Err(Error::Conflict);
        }
        // Both snapshots describe this one credited contribution, before other
        // mutations. It must not reclassify U, storage credits, or queued money.
        if after
            != (BalanceObservation {
                account_balance: add(before.account_balance, amount)?,
                ..before
            })
        {
            return Err(Error::InvalidOperation);
        }
        let required = add(self.protected_balance(before)?, self.free_capital)?;
        let deficit = required.saturating_sub(before.account_balance);
        let repair = amount.min(deficit);
        let allocation = OperatingContribution {
            amount,
            deficit_repair: repair,
            free_capital_added: amount - repair,
        };
        let capital = add(self.free_capital, allocation.free_capital_added)?;
        let mut totals = self.totals;
        totals.contributions = add(totals.contributions, amount)?;
        totals.deficit_repairs = add(totals.deficit_repairs, repair)?;
        totals.validate()?;
        self.free_capital = capital;
        self.operating_credits.insert(credit, allocation);
        self.totals = totals;
        // Partial repairs are accepted. Neither exact repair nor surplus clears
        // the hold; all existing principal, claims, locks and reserves survive.
        self.financial_hold |= deficit > 0;
        Ok(allocation)
    }

    /// Reserves only explicitly attributed operating capital. A purchase, storage
    /// credit, or unexpected balance increase cannot become operating capital.
    /// This does not admit a deposit or authorize a return transaction.
    pub fn reserve_operating(
        &mut self,
        id: [u8; 32],
        amount: u128,
        admission: Admission,
        policy: Option<&ReservePolicy>,
        balance: BalanceObservation,
    ) -> Result<()> {
        if self.financial_hold {
            return Err(Error::FinancialHold);
        }
        let policy = policy.ok_or(Error::MissingConfiguration)?;
        if id == [0; 32] || amount == 0 || amount > policy.max_operation {
            return Err(Error::InvalidOperation);
        }
        if self.reservations.contains_key(&id) {
            return Err(Error::Conflict);
        }
        self.check_backing(balance)?;
        let remaining = self
            .free_capital
            .checked_sub(amount)
            .ok_or(Error::InsufficientReserve)?;
        let threshold = match admission {
            Admission::Return => policy.hard_floor,
            Admission::ListingOrSale => policy.target,
        };
        if remaining < threshold {
            return Err(Error::InsufficientReserve);
        }
        let reserved = add(self.totals.reserved_operating, amount)?;
        self.free_capital = remaining;
        self.reservations.insert(id, amount);
        self.totals.reserved_operating = reserved;
        Ok(())
    }

    /// Models one bounded send. Caller never supplies beneficiary or amount.
    /// Commit this reservation in the same NEAR receipt as the future Transfer.
    pub fn dispatch(
        &mut self,
        id: ClaimId,
        operation_hash: [u8; 32],
        now_ns: u64,
        balance: BalanceObservation,
    ) -> Result<Attempt> {
        if self.financial_hold {
            return Err(Error::FinancialHold);
        }
        let policy = self.payment_policy.ok_or(Error::MissingConfiguration)?;
        let claim = self.claims.get(&id).ok_or(Error::UnknownClaim)?;
        if !matches!(claim.state, ClaimState::Available | ClaimState::Retryable) {
            return Err(Error::WrongState);
        }
        if let Some(last) = claim.last_dispatch_ns {
            let earliest = last
                .checked_add(policy.retry_delay_ns)
                .ok_or(Error::Overflow)?;
            if now_ns < earliest {
                return Err(Error::TooSoon);
            }
        }
        self.check_backing(balance)?;
        let attempt = Attempt::next(claim.attempt.map_or(0, |a| a.nonce), operation_hash)?;
        let mut totals = self.totals;
        totals.available = sub(totals.available, claim.amount)?;
        totals.in_flight = add(totals.in_flight, claim.amount)?;
        totals.queued_payments = add(totals.queued_payments, claim.amount)?;
        totals.validate()?;
        let claim = self.claims.get_mut(&id).unwrap();
        claim.state = ClaimState::InFlight;
        claim.attempt = Some(attempt);
        claim.last_dispatch_ns = Some(now_ns);
        claim.debit_pending = true;
        self.totals = totals;
        Ok(attempt)
    }

    fn in_flight(&self, id: ClaimId, attempt: Attempt) -> Result<&Claim> {
        let claim = self.claims.get(&id).ok_or(Error::UnknownClaim)?;
        if claim.attempt != Some(attempt) {
            return Err(Error::StaleAttempt);
        }
        if !matches!(claim.state, ClaimState::InFlight | ClaimState::Unknown) {
            return Err(Error::WrongState);
        }
        Ok(claim)
    }

    /// Exactly when the pinned runtime's A already excludes the queued debit.
    /// It is NOT inferred from balance changes, a timeout, or a user assertion.
    pub fn record_debit(&mut self, id: ClaimId, attempt: Attempt) -> Result<()> {
        let claim = self.in_flight(id, attempt)?;
        let queued = if claim.debit_pending {
            sub(self.totals.queued_payments, claim.amount)?
        } else {
            self.totals.queued_payments
        };
        self.claims.get_mut(&id).unwrap().debit_pending = false;
        self.totals.queued_payments = queued;
        Ok(())
    }

    /// Normal callback data only, not quorum/proof-based recovery. A known failed
    /// send restores debt, not cash; dispatch still checks actual liquid backing.
    pub fn record_payment_result(
        &mut self,
        id: ClaimId,
        attempt: Attempt,
        result: CallbackResult,
    ) -> Result<()> {
        let claim = self.in_flight(id, attempt)?;
        if result != CallbackResult::Unknown && claim.debit_pending {
            return Err(Error::WrongState);
        }
        let mut totals = self.totals;
        if result != CallbackResult::Unknown {
            totals.in_flight = sub(totals.in_flight, claim.amount)?;
            match result {
                CallbackResult::Succeeded => totals.paid = add(totals.paid, claim.amount)?,
                CallbackResult::Failed => totals.available = add(totals.available, claim.amount)?,
                CallbackResult::Unknown => unreachable!(),
            }
        }
        totals.validate()?;
        self.claims.get_mut(&id).unwrap().state = match result {
            CallbackResult::Succeeded => ClaimState::Paid,
            CallbackResult::Failed => ClaimState::Retryable,
            CallbackResult::Unknown => ClaimState::Unknown,
        };
        self.totals = totals;
        Ok(())
    }
}
