use std::collections::BTreeMap;

use crate::{
    ledger::{BalanceObservation, ClaimState, Ledger, LedgerTotals, PaymentPolicy},
    records::Records,
    state::{ApprovalWatermark, ListingState, ListingTerms, Marketplace, TransferState},
    types::{Attempt, CallbackResult, ClaimId, Generation, SaleId, TokenKey},
    AccountId,
};

use super::{
    codec::{OperationRecord, Progress, RootState, Value},
    decode_record, encode_record,
    operation::Descriptor,
    ArtifactDomain, AtomicBatch, AtomicStore, Budgets, DeploymentDomain, MemoryStore,
    OperationContext, Record, RecordKey, StorageError, StorageResult,
};

/// OFFLINE model inputs only. This enum is not serializable, has no NEAR ABI and
/// authenticates NOTHING. A future host adapter must derive observations from
/// authenticated execution, never deserialize commands from external requests.
/// No funding, policy changes, recovery, custody returns or activation are added.
#[derive(Clone, Debug)]
pub enum OfflineCommand {
    Listing {
        terms: ListingTerms,
        now_ns: u64,
        budgets: Budgets,
    },
    Verify {
        generation: Generation,
        attempt: Attempt,
        owner: Option<AccountId>,
        approval: Option<u64>,
        now_ns: u64,
    },
    Cancel {
        generation: Generation,
        seller: AccountId,
    },
    Stale {
        generation: Generation,
    },
    Purchase {
        generation: Generation,
        token: TokenKey,
        buyer: AccountId,
        exact_price: u128,
        now_ns: u64,
        budgets: Budgets,
    },
    TransferResult {
        sale: SaleId,
        generation: Generation,
        attempt: Attempt,
        result: CallbackResult,
    },
    Dispatch {
        claim: ClaimId,
        now_ns: u64,
        balance: BalanceObservation,
        budgets: Budgets,
    },
    Debit {
        claim: ClaimId,
        attempt: Attempt,
    },
    PaymentResult {
        claim: ClaimId,
        attempt: Attempt,
        result: CallbackResult,
    },
}

impl OfflineCommand {
    // Includes the root, required absence checks and unchanged dependencies.
    fn limit(&self) -> usize {
        match self {
            Self::Listing { .. } => 7,
            Self::Verify { .. } => 6,
            Self::Cancel { .. } | Self::Stale { .. } => 4,
            Self::Purchase { .. } => 9,
            Self::TransferResult { .. } => 13,
            Self::Dispatch { .. } | Self::Debit { .. } | Self::PaymentResult { .. } => 16,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Prepared {
    pub(super) batch: AtomicBatch,
    operation: Option<OperationContext>,
    generation: Option<Generation>,
    sale: Option<SaleId>,
    attempt: Option<Attempt>,
}
impl Prepared {
    pub fn batch(&self) -> &AtomicBatch {
        &self.batch
    }
    pub fn operation(&self) -> Option<&OperationContext> {
        self.operation.as_ref()
    }
    pub fn generation(&self) -> Option<Generation> {
        self.generation
    }
    pub fn sale(&self) -> Option<SaleId> {
        self.sale
    }
    pub fn attempt(&self) -> Option<Attempt> {
        self.attempt
    }
}

/// Owns a byte backend, not a serialized history-bearing Marketplace aggregate.
/// State reopens exclusively from validated root/point records. No missing-root
/// fallback or legacy/raw-payload migration is attempted.
pub struct Repository<S: AtomicStore> {
    domain: DeploymentDomain,
    pub(super) store: S,
}

impl Repository<MemoryStore> {
    /// Fresh offline fixture only. Cannot initialize over an existing backend;
    /// the eventual deploy/initialize procedure is a separate reviewed milestone.
    pub fn new_offline(
        domain: DeploymentDomain,
        policy: Option<PaymentPolicy>,
    ) -> StorageResult<Self> {
        check_artifact(&domain)?;
        let root = Record::new(
            &domain,
            RecordKey::Root,
            1,
            Value::Root(RootState {
                last_generation: 0,
                last_sale: 0,
                last_claim: 0,
                payment_policy: policy,
                free_capital: 0,
                totals: LedgerTotals::default(),
                financial_hold: false,
                trading_paused: true,
            }),
        )?;
        let mut store = MemoryStore::default();
        store.apply(&AtomicBatch {
            domain: domain.commitment()?,
            limit: 1,
            checks: vec![(b"STATE".to_vec(), None)],
            writes: vec![(b"STATE".to_vec(), encode_record(&root, &domain)?)],
        })?;
        Self::open(store, domain)
    }
}

impl<S: AtomicStore> Repository<S> {
    pub fn open(store: S, domain: DeploymentDomain) -> StorageResult<Self> {
        check_artifact(&domain)?;
        let repo = Self { domain, store };
        repo.read(&RecordKey::Root)?.ok_or(StorageError::Missing)?;
        Ok(repo)
    }
    pub fn into_store(self) -> S {
        self.store
    }
    pub fn read(&self, key: &RecordKey) -> StorageResult<Option<Record>> {
        self.store
            .read(&key.storage_key(&self.domain)?)?
            .map(|bytes| decode_record(&bytes, &self.domain, key))
            .transpose()
    }
    pub fn totals(&self) -> StorageResult<LedgerTotals> {
        match self
            .read(&RecordKey::Root)?
            .ok_or(StorageError::Missing)?
            .value
        {
            Value::Root(root) => Ok(root.totals),
            _ => Err(StorageError::Invariant),
        }
    }
    pub fn commit(&mut self, prepared: &Prepared) -> StorageResult<()> {
        if prepared.batch.domain != self.domain.commitment()? {
            return Err(StorageError::Domain);
        }
        self.store.apply(&prepared.batch)
    }

    pub fn prepare(&self, command: OfflineCommand) -> StorageResult<Prepared> {
        let limit = command.limit();
        let mut view = View::load_root(self, limit)?;
        view.load_inputs(&command)?;
        view.validate_links()?;
        let mut prepared = Prepared {
            batch: AtomicBatch {
                domain: self.domain.commitment()?,
                checks: vec![],
                writes: vec![],
                limit,
            },
            operation: None,
            generation: None,
            sale: None,
            attempt: None,
        };
        // Existing business rules run against an owned, bounded view with ALL
        // original counters/totals restored. Never reset with Marketplace::new.
        match command {
            OfflineCommand::Listing {
                terms,
                now_ns,
                budgets,
            } => {
                let expected = Generation(
                    view.market
                        .last_generation
                        .checked_add(1)
                        .ok_or(StorageError::Overflow)?,
                );
                let op = OperationContext::new(
                    &self.domain,
                    Descriptor::Verification {
                        generation: expected,
                        terms: terms.clone(),
                    },
                    1,
                    budgets,
                )?;
                let id = view
                    .market
                    .record_listing(terms, now_ns, op.commitment()?)?;
                if id != expected {
                    return Err(StorageError::Invariant);
                }
                let attempt = view.market.listings[&id].verification;
                view.operations.insert(
                    RecordKey::Verification(id),
                    OperationRecord {
                        context: op.clone(),
                        progress: Progress::Pending,
                    },
                );
                prepared.generation = Some(id);
                prepared.attempt = Some(attempt);
                prepared.operation = Some(op);
            }
            OfflineCommand::Verify {
                generation,
                attempt,
                owner,
                approval,
                now_ns,
            } => {
                view.market.verify_listing(
                    generation,
                    attempt,
                    owner.as_ref(),
                    approval,
                    now_ns,
                )?;
                view.progress(RecordKey::Verification(generation), Progress::Terminal)?;
            }
            OfflineCommand::Cancel { generation, seller } => {
                view.market.cancel_listing(generation, &seller)?;
                view.progress(RecordKey::Verification(generation), Progress::Terminal)?;
            }
            OfflineCommand::Stale { generation } => {
                view.market.invalidate_listing(generation)?;
            }
            OfflineCommand::Purchase {
                generation,
                token,
                buyer,
                exact_price,
                now_ns,
                budgets,
            } => {
                let expected = SaleId(
                    view.market
                        .last_sale
                        .checked_add(1)
                        .ok_or(StorageError::Overflow)?,
                );
                let terms = view
                    .market
                    .listings
                    .get(&generation)
                    .ok_or(StorageError::Missing)?
                    .terms
                    .clone();
                let op = OperationContext::new(
                    &self.domain,
                    Descriptor::ApprovedTransfer {
                        sale: expected,
                        generation,
                        terms,
                        receiver: buyer.clone(),
                    },
                    1,
                    budgets,
                )?;
                let id = view.market.record_purchase(
                    generation,
                    &token,
                    buyer,
                    exact_price,
                    now_ns,
                    op.commitment()?,
                )?;
                if id != expected {
                    return Err(StorageError::Invariant);
                }
                let attempt = view.market.sales[&id].transfer;
                view.operations.insert(
                    RecordKey::Transfer(id),
                    OperationRecord {
                        context: op.clone(),
                        progress: Progress::Pending,
                    },
                );
                prepared.sale = Some(id);
                prepared.attempt = Some(attempt);
                prepared.operation = Some(op);
            }
            OfflineCommand::TransferResult {
                sale,
                generation,
                attempt,
                result,
            } => {
                view.market
                    .record_transfer_result(sale, generation, attempt, result)?;
                view.progress(RecordKey::Transfer(sale), outcome_progress(result))?;
            }
            OfflineCommand::Dispatch {
                claim,
                now_ns,
                balance,
                budgets,
            } => {
                let c = view
                    .market
                    .ledger
                    .claim(claim)
                    .ok_or(StorageError::Missing)?;
                let nonce = c
                    .attempt()
                    .map_or(0, |a| a.nonce)
                    .checked_add(1)
                    .ok_or(StorageError::Overflow)?;
                let op = OperationContext::for_payment(&self.domain, c, nonce, budgets)?;
                let attempt =
                    view.market
                        .ledger
                        .dispatch(claim, op.commitment()?, now_ns, balance)?;
                view.operations.insert(
                    RecordKey::Payment(claim),
                    OperationRecord {
                        context: op.clone(),
                        progress: Progress::Pending,
                    },
                );
                prepared.attempt = Some(attempt);
                prepared.operation = Some(op);
            }
            OfflineCommand::Debit { claim, attempt } => {
                view.market.ledger.record_debit(claim, attempt)?;
            }
            OfflineCommand::PaymentResult {
                claim,
                attempt,
                result,
            } => {
                view.market
                    .ledger
                    .record_payment_result(claim, attempt, result)?;
                view.progress(RecordKey::Payment(claim), outcome_progress(result))?;
            }
        }
        view.validate_links()?;
        view.prepare_batch(&mut prepared.batch)?;
        Ok(prepared)
    }
}

fn check_artifact(domain: &DeploymentDomain) -> StorageResult<()> {
    domain.validate()?;
    if (cfg!(feature = "testnet") && domain.environment != ArtifactDomain::Testnet)
        || (cfg!(feature = "production") && domain.environment != ArtifactDomain::Production)
    {
        return Err(StorageError::Domain);
    }
    // No feature means offline codec/model use only, never deployment readiness.
    Ok(())
}

fn outcome_progress(result: CallbackResult) -> Progress {
    if result == CallbackResult::Unknown {
        Progress::Unknown
    } else {
        Progress::Terminal
    }
}

struct View<'a, S: AtomicStore> {
    repo: &'a Repository<S>,
    reads: BTreeMap<RecordKey, Option<Record>>,
    operations: BTreeMap<RecordKey, OperationRecord>,
    market: Marketplace,
    limit: usize,
}

impl<'a, S: AtomicStore> View<'a, S> {
    fn load_root(repo: &'a Repository<S>, limit: usize) -> StorageResult<Self> {
        let root = repo.read(&RecordKey::Root)?.ok_or(StorageError::Missing)?;
        let Value::Root(r) = &root.value else {
            return Err(StorageError::Invariant);
        };
        let market = Marketplace {
            nft: repo.domain.nft.clone(),
            marketplace: repo.domain.marketplace.clone(),
            last_generation: r.last_generation,
            last_sale: r.last_sale,
            slots: Records::new(),
            approval_watermarks: Records::new(),
            listings: Records::new(),
            sales: Records::new(),
            ledger: Ledger {
                principals: Records::new(),
                claims: Records::new(),
                last_claim: r.last_claim,
                payment_policy: r.payment_policy,
                free_capital: r.free_capital,
                reservations: Records::new(),
                operating_credits: Records::new(),
                totals: r.totals,
                financial_hold: r.financial_hold,
            },
        };
        Ok(Self {
            repo,
            reads: BTreeMap::from([(RecordKey::Root, Some(root))]),
            operations: BTreeMap::new(),
            market,
            limit,
        })
    }

    fn load(&mut self, key: RecordKey, required: bool) -> StorageResult<()> {
        if !self.reads.contains_key(&key) {
            if self.reads.len() >= self.limit {
                return Err(StorageError::Bounds);
            }
            let record = self.repo.read(&key)?;
            if let Some(record) = &record {
                match (&key, &record.value) {
                    (RecordKey::ApprovalWatermark(token), Value::ApprovalWatermark(w)) => {
                        self.market.approval_watermarks.insert(token.clone(), *w);
                    }
                    (RecordKey::Token(id), Value::Token(v)) => {
                        self.market.slots.insert(id.clone(), v.clone());
                    }
                    (RecordKey::Listing(id), Value::Listing(v)) => {
                        self.market.listings.insert(*id, v.clone());
                    }
                    (RecordKey::Sale(id), Value::Sale(v)) => {
                        self.market.sales.insert(*id, v.clone());
                    }
                    (RecordKey::Principal(id), Value::Principal(v)) => {
                        self.market.ledger.principals.insert(*id, v.clone());
                    }
                    (RecordKey::Claim(id), Value::Claim(v)) => {
                        self.market.ledger.claims.insert(*id, v.clone());
                    }
                    (_, Value::Operation(v)) => {
                        self.operations.insert(key.clone(), v.as_ref().clone());
                    }
                    _ => return Err(StorageError::Invariant),
                }
            }
            self.reads.insert(key.clone(), record);
        }
        if required && self.reads[&key].is_none() {
            return Err(StorageError::Missing);
        }
        Ok(())
    }

    fn absent(&mut self, key: RecordKey) -> StorageResult<()> {
        self.load(key.clone(), false)?;
        if self.reads[&key].is_some() {
            return Err(StorageError::Conflict);
        }
        Ok(())
    }
    fn load_listing(&mut self, generation: Generation) -> StorageResult<()> {
        self.load(RecordKey::Listing(generation), true)?;
        self.load(RecordKey::Verification(generation), true)
    }
    fn load_slot(&mut self, token: TokenKey) -> StorageResult<()> {
        self.load(RecordKey::Token(token.clone()), false)?;
        self.load(RecordKey::ApprovalWatermark(token.clone()), false)?;
        if let Some(slot) = self.market.slots.get(&token) {
            // Existing v1 records had already consumed this value. Never silently
            // relax their replay barrier. New candidates use an explicit record.
            if !self.market.approval_watermarks.contains_key(&token) {
                self.market.approval_watermarks.insert(
                    token.clone(),
                    ApprovalWatermark {
                        authenticated: true,
                        high_water: slot.consumed_approval,
                    },
                );
            }
            self.load(RecordKey::Listing(slot.latest), true)?;
        }
        Ok(())
    }
    fn sale_bundle(&mut self, sale: SaleId) -> StorageResult<()> {
        self.load(RecordKey::Sale(sale), true)?;
        let s = self.market.sales[&sale].clone();
        self.load_listing(s.generation)?;
        self.load_slot(s.terms.token.clone())?;
        if !self.market.slots.contains_key(&s.terms.token) {
            return Err(StorageError::Missing);
        }
        self.load(RecordKey::Principal(sale), true)?;
        self.load(RecordKey::Transfer(sale), true)?;
        for id in self.market.ledger.principals[&sale].claims.clone() {
            self.load(RecordKey::Claim(id), true)?;
        }
        Ok(())
    }

    fn load_inputs(&mut self, command: &OfflineCommand) -> StorageResult<()> {
        match command {
            OfflineCommand::Listing { terms, .. } => {
                super::codec::validate_terms(terms, &self.repo.domain)?;
                self.load_slot(terms.token.clone())?;
                let id = Generation(
                    self.market
                        .last_generation
                        .checked_add(1)
                        .ok_or(StorageError::Overflow)?,
                );
                self.absent(RecordKey::Listing(id))?;
                self.absent(RecordKey::Verification(id))?;
            }
            OfflineCommand::Verify { generation, .. } => {
                self.load_listing(*generation)?;
                let token = self.market.listings[generation].terms.token.clone();
                self.load_slot(token)?;
            }
            OfflineCommand::Cancel { generation, .. } | OfflineCommand::Stale { generation } => {
                self.load_listing(*generation)?
            }
            OfflineCommand::Purchase {
                generation, token, ..
            } => {
                self.load_listing(*generation)?;
                self.load_slot(token.clone())?;
                let id = SaleId(
                    self.market
                        .last_sale
                        .checked_add(1)
                        .ok_or(StorageError::Overflow)?,
                );
                self.absent(RecordKey::Sale(id))?;
                self.absent(RecordKey::Principal(id))?;
                self.absent(RecordKey::Transfer(id))?;
            }
            OfflineCommand::TransferResult { sale, result, .. } => {
                self.sale_bundle(*sale)?;
                let p = &self.market.ledger.principals[sale];
                let count = match result {
                    CallbackResult::Unknown => 0,
                    CallbackResult::Succeeded if p.fee > 0 && p.seller != p.treasury => 2,
                    _ => 1,
                };
                for step in 1..=count {
                    let id = ClaimId(
                        self.market
                            .ledger
                            .last_claim
                            .checked_add(step)
                            .ok_or(StorageError::Overflow)?,
                    );
                    self.absent(RecordKey::Claim(id))?;
                }
            }
            OfflineCommand::Dispatch { claim, .. }
            | OfflineCommand::Debit { claim, .. }
            | OfflineCommand::PaymentResult { claim, .. } => {
                self.load(RecordKey::Claim(*claim), true)?;
                let c = self.market.ledger.claims[claim].clone();
                self.sale_bundle(c.sale)?;
                self.load(RecordKey::Payment(*claim), c.attempt.is_some())?;
                if c.attempt.is_none() && self.operations.contains_key(&RecordKey::Payment(*claim))
                {
                    return Err(StorageError::Invariant);
                }
            }
        }
        Ok(())
    }

    fn progress(&mut self, key: RecordKey, progress: Progress) -> StorageResult<()> {
        self.operations
            .get_mut(&key)
            .ok_or(StorageError::Missing)?
            .progress = progress;
        Ok(())
    }

    fn root(&self) -> RootState {
        RootState {
            last_generation: self.market.last_generation,
            last_sale: self.market.last_sale,
            last_claim: self.market.ledger.last_claim,
            payment_policy: self.market.ledger.payment_policy,
            free_capital: self.market.ledger.free_capital,
            totals: self.market.ledger.totals,
            financial_hold: self.market.ledger.financial_hold,
            trading_paused: true,
        }
    }
    fn value(&self, key: &RecordKey) -> Option<Value> {
        match key {
            RecordKey::Root => Some(Value::Root(self.root())),
            RecordKey::Token(id) => self.market.slots.get(id).cloned().map(Value::Token),
            RecordKey::ApprovalWatermark(id) => self
                .market
                .approval_watermarks
                .get(id)
                .copied()
                .map(Value::ApprovalWatermark),
            RecordKey::Listing(id) => self.market.listings.get(id).cloned().map(Value::Listing),
            RecordKey::Sale(id) => self.market.sales.get(id).cloned().map(Value::Sale),
            RecordKey::Principal(id) => self
                .market
                .ledger
                .principals
                .get(id)
                .cloned()
                .map(Value::Principal),
            RecordKey::Claim(id) => self.market.ledger.claims.get(id).cloned().map(Value::Claim),
            RecordKey::Verification(_) | RecordKey::Transfer(_) | RecordKey::Payment(_) => self
                .operations
                .get(key)
                .cloned()
                .map(|v| Value::Operation(Box::new(v))),
            _ => None,
        }
    }

    fn validate_links(&self) -> StorageResult<()> {
        self.root().validate()?;
        for key in self.reads.keys() {
            let Some(value) = self.value(key) else {
                continue;
            };
            Record::new(&self.repo.domain, key.clone(), 1, value.clone())?;
            match value {
                Value::Token(slot) => {
                    let listing = self
                        .market
                        .listings
                        .get(&slot.latest)
                        .ok_or(StorageError::Missing)?;
                    let RecordKey::Token(token) = key else {
                        return Err(StorageError::Identity);
                    };
                    if &listing.terms.token != token
                        || slot.consumed_approval != listing.terms.approval_id
                        || slot.sale_lock
                            != (if matches!(
                                listing.state,
                                ListingState::Locked | ListingState::Review
                            ) {
                                listing.sale
                            } else {
                                None
                            })
                    {
                        return Err(StorageError::Invariant);
                    }
                    let w = self
                        .market
                        .approval_watermarks
                        .get(token)
                        .ok_or(StorageError::Missing)?;
                    if (w.authenticated && w.high_water > slot.consumed_approval)
                        || (matches!(
                            listing.state,
                            ListingState::Active
                                | ListingState::Stale
                                | ListingState::Locked
                                | ListingState::Review
                                | ListingState::Sold
                                | ListingState::Failed
                        ) && (!w.authenticated || w.high_water != listing.terms.approval_id))
                    {
                        return Err(StorageError::Invariant);
                    }
                }
                Value::ApprovalWatermark(_) => {
                    let RecordKey::ApprovalWatermark(token) = key else {
                        return Err(StorageError::Identity);
                    };
                    if !self.market.slots.contains_key(token) {
                        return Err(StorageError::Missing);
                    }
                }
                Value::Listing(l) if l.generation.value() > self.market.last_generation => {
                    return Err(StorageError::Invariant)
                }
                Value::Sale(s) => {
                    let l = self
                        .market
                        .listings
                        .get(&s.generation)
                        .ok_or(StorageError::Missing)?;
                    let p = self
                        .market
                        .ledger
                        .principals
                        .get(&s.id)
                        .ok_or(StorageError::Missing)?;
                    let state = match s.outcome {
                        TransferState::Pending => ListingState::Locked,
                        TransferState::Unknown => ListingState::Review,
                        TransferState::Delivered => ListingState::Sold,
                        TransferState::Failed => ListingState::Failed,
                    };
                    if s.id.value() > self.market.last_sale
                        || l.sale != Some(s.id)
                        || l.terms != s.terms
                        || l.state != state
                        || p.generation != s.generation
                        || p.price != s.terms.price
                        || p.buyer != s.buyer
                        || p.seller != s.terms.seller
                        || &p.treasury != s.terms.fees.treasury()
                        || p.fee != s.terms.fees.fee(s.terms.price)?
                        || p.allocated
                            != matches!(s.outcome, TransferState::Delivered | TransferState::Failed)
                    {
                        return Err(StorageError::Invariant);
                    }
                    if matches!(s.outcome, TransferState::Pending | TransferState::Unknown) {
                        let slot = self
                            .market
                            .slots
                            .get(&s.terms.token)
                            .ok_or(StorageError::Missing)?;
                        if slot.latest != s.generation || slot.sale_lock != Some(s.id) {
                            return Err(StorageError::Invariant);
                        }
                    }
                    let mut roles = (0u128, 0u128, 0u128);
                    for id in &p.claims {
                        let c = self
                            .market
                            .ledger
                            .claims
                            .get(id)
                            .ok_or(StorageError::Missing)?;
                        if c.sale != s.id
                            || c.generation != s.generation
                            || c.id.value() > self.market.ledger.last_claim
                            || (c.seller_amount > 0 && c.recipient != p.seller)
                            || (c.fee_amount > 0 && c.recipient != p.treasury)
                            || (c.refund_amount > 0 && c.recipient != p.buyer)
                        {
                            return Err(StorageError::Invariant);
                        }
                        roles.0 = roles
                            .0
                            .checked_add(c.seller_amount)
                            .ok_or(StorageError::Overflow)?;
                        roles.1 = roles
                            .1
                            .checked_add(c.fee_amount)
                            .ok_or(StorageError::Overflow)?;
                        roles.2 = roles
                            .2
                            .checked_add(c.refund_amount)
                            .ok_or(StorageError::Overflow)?;
                    }
                    let expected = match s.outcome {
                        TransferState::Delivered => (p.price - p.fee, p.fee, 0),
                        TransferState::Failed => (0, 0, p.price),
                        _ => (0, 0, 0),
                    };
                    if roles != expected {
                        return Err(StorageError::Invariant);
                    }
                    self.market.ledger.principal_summary(s.id)?;
                }
                Value::Claim(c) => {
                    let p = self
                        .market
                        .ledger
                        .principals
                        .get(&c.sale)
                        .ok_or(StorageError::Missing)?;
                    if c.id.value() > self.market.ledger.last_claim || !p.claims.contains(&c.id) {
                        return Err(StorageError::Invariant);
                    }
                }
                Value::Operation(op) => {
                    let (expected, attempt, progress) = match key {
                        RecordKey::Verification(id) => {
                            let l = self.market.listings.get(id).ok_or(StorageError::Missing)?;
                            (
                                OperationContext::for_listing(
                                    &self.repo.domain,
                                    l,
                                    op.context.budgets,
                                )?,
                                l.verification,
                                if l.state == ListingState::Verifying {
                                    Progress::Pending
                                } else {
                                    Progress::Terminal
                                },
                            )
                        }
                        RecordKey::Transfer(id) => {
                            let s = self.market.sales.get(id).ok_or(StorageError::Missing)?;
                            (
                                OperationContext::for_sale(
                                    &self.repo.domain,
                                    s,
                                    op.context.budgets,
                                )?,
                                s.transfer,
                                match s.outcome {
                                    TransferState::Pending => Progress::Pending,
                                    TransferState::Unknown => Progress::Unknown,
                                    _ => Progress::Terminal,
                                },
                            )
                        }
                        RecordKey::Payment(id) => {
                            let c = self
                                .market
                                .ledger
                                .claims
                                .get(id)
                                .ok_or(StorageError::Missing)?;
                            let a = c.attempt.ok_or(StorageError::Invariant)?;
                            (
                                OperationContext::for_payment(
                                    &self.repo.domain,
                                    c,
                                    a.nonce,
                                    op.context.budgets,
                                )?,
                                a,
                                match c.state {
                                    ClaimState::InFlight => Progress::Pending,
                                    ClaimState::Unknown => Progress::Unknown,
                                    ClaimState::Paid | ClaimState::Retryable => Progress::Terminal,
                                    _ => return Err(StorageError::Invariant),
                                },
                            )
                        }
                        _ => return Err(StorageError::Identity),
                    };
                    if expected != op.context
                        || attempt.operation_hash != op.context.commitment()?
                        || progress != op.progress
                    {
                        return Err(StorageError::Invariant);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn prepare_batch(&self, batch: &mut AtomicBatch) -> StorageResult<()> {
        let Value::Root(before) = &self.reads[&RecordKey::Root]
            .as_ref()
            .ok_or(StorageError::Missing)?
            .value
        else {
            return Err(StorageError::Invariant);
        };
        let after = self.root();
        if after.last_generation < before.last_generation
            || after.last_sale < before.last_sale
            || after.last_claim < before.last_claim
            || (before.financial_hold && !after.financial_hold)
            || after.free_capital != before.free_capital
        {
            return Err(StorageError::Invariant);
        }
        // Independently reconcile each maintained principal bucket against only
        // the bounded records touched by this transition, not historical scans.
        let mut old = [0u128; 6];
        let mut new = [0u128; 6];
        for (key, record) in &self.reads {
            if let Some(record) = record {
                contribution(&record.value, &mut old)?;
            }
            if let Some(value) = self.value(key) {
                contribution(&value, &mut new)?;
            }
        }
        let a = buckets(before.totals);
        let b = buckets(after.totals);
        for i in 0..6 {
            if a[i].checked_sub(old[i]).and_then(|v| v.checked_add(new[i])) != Some(b[i]) {
                return Err(StorageError::Invariant);
            }
        }
        if (
            before.totals.reserved_operating,
            before.totals.contributions,
            before.totals.deficit_repairs,
        ) != (
            after.totals.reserved_operating,
            after.totals.contributions,
            after.totals.deficit_repairs,
        ) {
            return Err(StorageError::Invariant);
        }
        for (key, previous) in &self.reads {
            let raw_key = key.storage_key(&self.repo.domain)?;
            batch.checks.push((
                raw_key.clone(),
                previous
                    .as_ref()
                    .map(|r| encode_record(r, &self.repo.domain))
                    .transpose()?,
            ));
            if let Some(value) = self.value(key) {
                if *key != RecordKey::Root && previous.as_ref().is_some_and(|r| r.value == value) {
                    continue;
                }
                let revision = previous
                    .as_ref()
                    .map_or(0, |r| r.revision)
                    .checked_add(1)
                    .ok_or(StorageError::Overflow)?;
                let record = Record::new(&self.repo.domain, key.clone(), revision, value)?;
                batch
                    .writes
                    .push((raw_key, encode_record(&record, &self.repo.domain)?));
            } else if previous.is_some() {
                return Err(StorageError::Invariant);
            }
        }
        batch.validate()
    }
}

fn buckets(t: LedgerTotals) -> [u128; 6] {
    [
        t.principal,
        t.held,
        t.available,
        t.in_flight,
        t.paid,
        t.queued_payments,
    ]
}
fn contribution(value: &Value, sums: &mut [u128; 6]) -> StorageResult<()> {
    let mut delta = [0u128; 6];
    match value {
        Value::Principal(p) => {
            delta[0] = p.price;
            delta[1] = p.held;
        }
        Value::Claim(c) => {
            let i = match c.state {
                ClaimState::Available | ClaimState::Retryable => 2,
                ClaimState::InFlight | ClaimState::Unknown => 3,
                ClaimState::Paid => 4,
            };
            delta[i] = c.amount;
            if c.debit_pending {
                delta[5] = c.amount;
            }
        }
        _ => {}
    }
    for i in 0..6 {
        sums[i] = sums[i]
            .checked_add(delta[i])
            .ok_or(StorageError::Overflow)?;
    }
    Ok(())
}
