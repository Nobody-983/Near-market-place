//! Deterministic records only. Observations must come from a future authenticated
//! adapter. In particular, listing verification is never transfer authorization.
use borsh::{BorshDeserialize, BorshSerialize};

use crate::{
    ledger::{Ledger, PaymentPolicy},
    records::Records,
    types::{Attempt, CallbackResult, FeeTerms, Generation, SaleId, TokenKey},
    AccountId, Error, Result,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum ListingState {
    Verifying,
    Active,
    Cancelled,
    Invalid,
    Stale,
    Locked,
    Review,
    Sold,
    Failed,
}

impl ListingState {
    fn terminal(self) -> bool {
        matches!(
            self,
            Self::Cancelled | Self::Invalid | Self::Stale | Self::Sold | Self::Failed
        )
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn fixture(price: u128) -> (Marketplace, Generation) {
        let nft: AccountId = "nft.testnet".parse().unwrap();
        let seller: AccountId = "seller.testnet".parse().unwrap();
        let mut market = Marketplace::new(
            nft.clone(),
            "market.testnet".parse().unwrap(),
            Some(PaymentPolicy::new(Some(10)).unwrap()),
        )
        .unwrap();
        let generation = market
            .record_listing(
                ListingTerms {
                    token: TokenKey::new(nft, "art".into()).unwrap(),
                    seller: seller.clone(),
                    approval_id: 0,
                    price,
                    expires_at_ns: 1_000,
                    fees: FeeTerms::new(
                        Some(1_000),
                        Some(2_000),
                        Some("fees.testnet".parse().unwrap()),
                    )
                    .unwrap(),
                    policy_version: 1,
                    policy_hash: [1; 32],
                    correlation: [2; 32],
                },
                1,
                [3; 32],
            )
            .unwrap();
        let attempt = market.listing(generation).unwrap().verification();
        market
            .verify_listing(generation, attempt, Some(&seller), Some(0), 2)
            .unwrap();
        (market, generation)
    }

    pub(crate) fn buy(market: &mut Marketplace, generation: Generation) -> Result<SaleId> {
        let terms = market.listing(generation).unwrap().terms().clone();
        market.record_purchase(
            generation,
            &terms.token,
            "buyer.testnet".parse().unwrap(),
            terms.price,
            3,
            [4; 32],
        )
    }

    #[test]
    fn generation_and_sale_exhaustion_leave_every_record_unchanged() {
        let (mut market, generation) = fixture(100);
        market.last_sale = u64::MAX;
        let before = market.clone();
        assert_eq!(buy(&mut market, generation), Err(Error::Overflow));
        assert_eq!(market, before);
        let mut terms = market.listing(generation).unwrap().terms().clone();
        market.cancel_listing(generation, &terms.seller).unwrap();
        terms.approval_id = 1;
        market.last_generation = u64::MAX;
        let before = market.clone();
        assert_eq!(
            market.record_listing(terms, 3, [5; 32]),
            Err(Error::Overflow)
        );
        assert_eq!(market, before);
    }

    #[test]
    fn principal_aggregate_overflow_does_not_lock_the_second_listing() {
        let (mut market, generation) = fixture(u128::MAX);
        buy(&mut market, generation).unwrap();
        let mut terms = market.listing(generation).unwrap().terms().clone();
        terms.token = TokenKey::new(terms.token.nft().clone(), "other".into()).unwrap();
        terms.price = 1;
        let next = market.record_listing(terms.clone(), 3, [5; 32]).unwrap();
        let attempt = market.listing(next).unwrap().verification();
        market
            .verify_listing(next, attempt, Some(&terms.seller), Some(0), 4)
            .unwrap();
        let before = market.clone();
        assert_eq!(buy(&mut market, next), Err(Error::Overflow));
        assert_eq!(market, before);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ListingTerms {
    pub token: TokenKey,
    pub seller: AccountId,
    pub approval_id: u64,
    pub price: u128,
    pub expires_at_ns: u64,
    pub fees: FeeTerms,
    pub policy_version: u64,
    pub policy_hash: [u8; 32],
    pub correlation: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Listing {
    pub(crate) generation: Generation,
    pub(crate) terms: ListingTerms,
    pub(crate) state: ListingState,
    pub(crate) verification: Attempt,
    pub(crate) sale: Option<SaleId>,
}

impl Listing {
    pub fn generation(&self) -> Generation {
        self.generation
    }
    pub fn terms(&self) -> &ListingTerms {
        &self.terms
    }
    pub fn state(&self) -> ListingState {
        self.state
    }
    pub fn verification(&self) -> Attempt {
        self.verification
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum TransferState {
    Pending,
    Unknown,
    Delivered,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Sale {
    pub(crate) id: SaleId,
    pub(crate) generation: Generation,
    pub(crate) terms: ListingTerms,
    pub(crate) buyer: AccountId,
    pub(crate) transfer: Attempt,
    pub(crate) outcome: TransferState,
}

impl Sale {
    pub fn id(&self) -> SaleId {
        self.id
    }
    pub fn generation(&self) -> Generation {
        self.generation
    }
    pub fn terms(&self) -> &ListingTerms {
        &self.terms
    }
    pub fn buyer(&self) -> &AccountId {
        &self.buyer
    }
    pub fn transfer(&self) -> Attempt {
        self.transfer
    }
    pub fn outcome(&self) -> TransferState {
        self.outcome
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub(crate) struct TokenSlot {
    pub(crate) latest: Generation,
    pub(crate) consumed_approval: u64,
    pub(crate) sale_lock: Option<SaleId>,
}

/// Additive record: an unverified candidate is never an authenticated high-water
/// mark. Fixed-width encoding reserves callback space even before first success.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub(crate) struct ApprovalWatermark {
    pub(crate) authenticated: bool,
    pub(crate) high_water: u64,
}

/// This aggregate commits listing, sale and principal changes together. It has
/// no NEAR ABI, no forwarding facility, and no deposit/return/recovery operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marketplace {
    pub(crate) nft: AccountId,
    pub(crate) marketplace: AccountId,
    pub(crate) last_generation: u64,
    pub(crate) last_sale: u64,
    pub(crate) slots: Records<TokenKey, TokenSlot>,
    pub(crate) approval_watermarks: Records<TokenKey, ApprovalWatermark>,
    pub(crate) listings: Records<Generation, Listing>,
    pub(crate) sales: Records<SaleId, Sale>,
    pub(crate) ledger: Ledger,
}

impl Marketplace {
    pub fn new(
        nft: AccountId,
        marketplace: AccountId,
        payment_policy: Option<PaymentPolicy>,
    ) -> Result<Self> {
        if nft == marketplace {
            return Err(Error::InvalidIdentity);
        }
        Ok(Self {
            nft,
            marketplace,
            last_generation: 0,
            last_sale: 0,
            slots: Records::new(),
            approval_watermarks: Records::new(),
            listings: Records::new(),
            sales: Records::new(),
            ledger: Ledger::new(payment_policy),
        })
    }

    pub fn listing(&self, id: Generation) -> Option<&Listing> {
        self.listings.get(&id)
    }
    pub fn sale(&self, id: SaleId) -> Option<&Sale> {
        self.sales.get(&id)
    }
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }
    pub fn ledger_mut(&mut self) -> &mut Ledger {
        &mut self.ledger
    }

    /// Adapter must authenticate our NFT hook, approved policy and prepaid storage.
    /// The full u64 stays lossless internally; a future JSON adapter must separately
    /// reject approval IDs outside its explicitly tested safe serialization range.
    pub fn record_listing(
        &mut self,
        terms: ListingTerms,
        now_ns: u64,
        operation_hash: [u8; 32],
    ) -> Result<Generation> {
        if self.ledger.financial_hold() {
            return Err(Error::FinancialHold);
        }
        if terms.token.nft() != &self.nft {
            return Err(Error::InvalidIdentity);
        }
        if terms.seller == self.marketplace {
            return Err(Error::InvalidSeller);
        }
        if terms.fees.treasury() == &self.marketplace || terms.fees.treasury() == &self.nft {
            return Err(Error::InvalidFee);
        }
        terms.fees.fee(terms.price)?;
        if terms.policy_hash == [0; 32] {
            return Err(Error::MissingConfiguration);
        }
        if terms.expires_at_ns <= now_ns {
            return Err(Error::Expired);
        }
        if let Some(slot) = self.slots.get(&terms.token) {
            if slot.sale_lock.is_some() || !self.listings[&slot.latest].state.terminal() {
                return Err(Error::Conflict);
            }
            let watermark = self
                .approval_watermarks
                .get(&terms.token)
                .ok_or(Error::AccountingInvariant)?;
            if watermark.authenticated && terms.approval_id <= watermark.high_water {
                return Err(Error::StaleApproval);
            }
        }
        let generation = Generation(self.last_generation.checked_add(1).ok_or(Error::Overflow)?);
        let verification = Attempt::next(0, operation_hash)?;
        if !self.approval_watermarks.contains_key(&terms.token) {
            self.approval_watermarks.insert(
                terms.token.clone(),
                ApprovalWatermark {
                    authenticated: false,
                    high_water: 0,
                },
            );
        }
        self.slots.insert(
            terms.token.clone(),
            TokenSlot {
                latest: generation,
                consumed_approval: terms.approval_id,
                sale_lock: None,
            },
        );
        self.listings.insert(
            generation,
            Listing {
                generation,
                terms,
                state: ListingState::Verifying,
                verification,
                sale: None,
            },
        );
        self.last_generation = generation.value();
        Ok(generation)
    }

    /// Missing/malformed observations retire this candidate. A view remains only
    /// provisional: the mandatory NFT wrapper must recheck seller/exact approval.
    pub fn verify_listing(
        &mut self,
        generation: Generation,
        attempt: Attempt,
        owner: Option<&AccountId>,
        approval: Option<u64>,
        now_ns: u64,
    ) -> Result<()> {
        let listing = self
            .listings
            .get_mut(&generation)
            .ok_or(Error::UnknownListing)?;
        if listing.state != ListingState::Verifying {
            return Err(Error::WrongState);
        }
        if listing.verification != attempt {
            return Err(Error::StaleAttempt);
        }
        if self
            .slots
            .get(&listing.terms.token)
            .is_none_or(|s| s.latest != generation || s.sale_lock.is_some())
        {
            return Err(Error::StaleAttempt);
        }
        let watermark = self
            .approval_watermarks
            .get_mut(&listing.terms.token)
            .ok_or(Error::AccountingInvariant)?;
        listing.state = if owner == Some(&listing.terms.seller)
            && approval == Some(listing.terms.approval_id)
            && now_ns < listing.terms.expires_at_ns
        {
            if watermark.authenticated && listing.terms.approval_id <= watermark.high_water {
                return Err(Error::StaleApproval);
            }
            watermark.authenticated = true;
            watermark.high_water = listing.terms.approval_id;
            ListingState::Active
        } else {
            ListingState::Invalid
        };
        Ok(())
    }

    pub fn cancel_listing(&mut self, generation: Generation, seller: &AccountId) -> Result<()> {
        let listing = self
            .listings
            .get_mut(&generation)
            .ok_or(Error::UnknownListing)?;
        if seller != &listing.terms.seller {
            return Err(Error::InvalidSeller);
        }
        if !matches!(
            listing.state,
            ListingState::Verifying | ListingState::Active
        ) {
            return Err(Error::WrongState);
        }
        listing.state = ListingState::Cancelled;
        Ok(())
    }

    /// Only a conclusive adapter observation may invalidate an active listing.
    /// Refresh requests/results and unknown freshness are outside this foundation.
    pub fn invalidate_listing(&mut self, generation: Generation) -> Result<()> {
        let listing = self
            .listings
            .get_mut(&generation)
            .ok_or(Error::UnknownListing)?;
        if listing.state != ListingState::Active {
            return Err(Error::WrongState);
        }
        listing.state = ListingState::Stale;
        Ok(())
    }

    /// Models the atomic lock + principal receipt + single wrapper scheduling
    /// transition. Does not send a transfer. Storage/gas/reserve admission and
    /// authentic payer/deposit checks belong to the future contract adapter.
    pub fn record_purchase(
        &mut self,
        generation: Generation,
        token: &TokenKey,
        buyer: AccountId,
        exact_price: u128,
        now_ns: u64,
        wrapper_call_hash: [u8; 32],
    ) -> Result<SaleId> {
        if self.ledger.financial_hold() {
            return Err(Error::FinancialHold);
        }
        let listing = self
            .listings
            .get(&generation)
            .ok_or(Error::UnknownListing)?;
        if &listing.terms.token != token {
            return Err(Error::InvalidToken);
        }
        let slot = self.slots.get(token).ok_or(Error::UnknownListing)?;
        if slot.latest != generation
            || slot.sale_lock.is_some()
            || listing.state != ListingState::Active
        {
            return Err(Error::Conflict);
        }
        if buyer == listing.terms.seller || buyer == self.marketplace {
            return Err(Error::InvalidBuyer);
        }
        if exact_price != listing.terms.price {
            return Err(Error::InvalidPrice);
        }
        if now_ns >= listing.terms.expires_at_ns {
            return Err(Error::Expired);
        }
        let id = SaleId(self.last_sale.checked_add(1).ok_or(Error::Overflow)?);
        let transfer = Attempt::next(0, wrapper_call_hash)?;
        let sale = Sale {
            id,
            generation,
            terms: listing.terms.clone(),
            buyer,
            transfer,
            outcome: TransferState::Pending,
        };
        // All fallible preparation occurs before any listing/slot mutation.
        self.ledger.hold_principal(&sale)?;
        self.last_sale = id.value();
        let listing = self.listings.get_mut(&generation).unwrap();
        listing.state = ListingState::Locked;
        listing.sale = Some(id);
        self.slots.get_mut(token).unwrap().sale_lock = Some(id);
        self.sales.insert(id, sale);
        Ok(id)
    }

    /// Only an exact, authenticated normal callback can resolve Pending/Unknown.
    /// Unknown never refunds, creates debt, releases the lock, or resends. There
    /// is no operator/evidence recovery entry point, timeout or manual override.
    pub fn record_transfer_result(
        &mut self,
        id: SaleId,
        generation: Generation,
        attempt: Attempt,
        result: CallbackResult,
    ) -> Result<()> {
        let sale = self.sales.get(&id).ok_or(Error::UnknownSale)?;
        if sale.generation != generation || sale.transfer != attempt {
            return Err(Error::StaleAttempt);
        }
        if !matches!(
            sale.outcome,
            TransferState::Pending | TransferState::Unknown
        ) {
            return Err(Error::WrongState);
        }
        let token = sale.terms.token.clone();
        if self.slots[&token].sale_lock != Some(id) {
            return Err(Error::Conflict);
        }
        let (outcome, listing_state) = match result {
            CallbackResult::Unknown => (TransferState::Unknown, ListingState::Review),
            CallbackResult::Succeeded => {
                self.ledger.allocate(id, true)?;
                (TransferState::Delivered, ListingState::Sold)
            }
            CallbackResult::Failed => {
                self.ledger.allocate(id, false)?;
                (TransferState::Failed, ListingState::Failed)
            }
        };
        self.sales.get_mut(&id).unwrap().outcome = outcome;
        self.listings.get_mut(&generation).unwrap().state = listing_state;
        if result != CallbackResult::Unknown {
            self.slots.get_mut(&token).unwrap().sale_lock = None;
        }
        Ok(())
    }
}
