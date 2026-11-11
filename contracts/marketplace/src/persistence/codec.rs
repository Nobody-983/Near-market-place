use borsh::{BorshDeserialize, BorshSerialize};
use sha2::{Digest, Sha256};

use crate::{
    ledger::{Claim, ClaimState, LedgerTotals, OperatingContribution, PaymentPolicy, Principal},
    state::{ApprovalWatermark, Listing, ListingState, ListingTerms, Sale, TokenSlot},
    types::{Attempt, ClaimId, Generation, SaleId, TokenKey},
};

use super::{
    operation::{DeploymentDomain, Descriptor, OperationContext},
    StorageError, StorageResult,
};

pub const MAX_RECORD_BYTES: usize = 16 * 1024;
const MAGIC: [u8; 8] = *b"NMPREC01";
const CHECKSUM_TAG: &[u8] = b"near-marketplace:record:v1\0";

/// Typed suffixes. Tags are fixed for v1; no string concatenation of identities.
/// Tag 11 is reserved for future storage accounts and deliberately unsupported.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, BorshSerialize, BorshDeserialize)]
#[repr(u8)]
#[borsh(use_discriminant = true)]
pub enum RecordKey {
    Root = 0,
    Token(TokenKey) = 1,
    Listing(Generation) = 2,
    Sale(SaleId) = 3,
    Principal(SaleId) = 4,
    Claim(ClaimId) = 5,
    Verification(Generation) = 6,
    Transfer(SaleId) = 7,
    Payment(ClaimId) = 8,
    Reservation([u8; 32]) = 9,
    Contribution([u8; 32]) = 10,
    ApprovalWatermark(TokenKey) = 12,
}

impl RecordKey {
    pub fn storage_key(&self, domain: &DeploymentDomain) -> StorageResult<Vec<u8>> {
        self.validate(domain)?;
        if *self == Self::Root {
            return Ok(b"STATE".to_vec());
        }
        let mut key = b"nmp:r:\x01\x00".to_vec();
        key.extend(domain.commitment()?);
        key.extend(borsh::to_vec(self).map_err(|_| StorageError::Encoding)?);
        Ok(key)
    }

    fn validate(&self, domain: &DeploymentDomain) -> StorageResult<()> {
        domain.validate()?;
        let valid = match self {
            Self::Root => true,
            Self::Token(t) | Self::ApprovalWatermark(t) => {
                validate_token(t, domain)?;
                true
            }
            Self::Listing(id) | Self::Verification(id) => id.value() != 0,
            Self::Sale(id) | Self::Principal(id) | Self::Transfer(id) => id.value() != 0,
            Self::Claim(id) | Self::Payment(id) => id.value() != 0,
            Self::Reservation(id) | Self::Contribution(id) => *id != [0; 32],
        };
        if !valid {
            return Err(StorageError::Identity);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub(crate) struct RootState {
    pub last_generation: u64,
    pub last_sale: u64,
    pub last_claim: u64,
    pub payment_policy: Option<PaymentPolicy>,
    pub free_capital: u128,
    pub totals: LedgerTotals,
    pub financial_hold: bool,
    pub trading_paused: bool,
}

impl RootState {
    pub(crate) fn validate(&self) -> StorageResult<()> {
        self.totals.validate()?;
        if !self.trading_paused
            || self.payment_policy.is_some_and(|p| p.retry_delay_ns == 0)
            || self
                .free_capital
                .checked_add(self.totals.reserved_operating)
                != self
                    .totals
                    .contributions
                    .checked_sub(self.totals.deficit_repairs)
        {
            return Err(StorageError::Invariant);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[repr(u8)]
#[borsh(use_discriminant = true)]
pub(crate) enum Progress {
    Pending = 0,
    Unknown = 1,
    Terminal = 2,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub(crate) struct OperationRecord {
    pub context: OperationContext,
    pub progress: Progress,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[repr(u8)]
#[borsh(use_discriminant = true)]
pub(crate) enum Value {
    Root(RootState) = 0,
    Token(TokenSlot) = 1,
    Listing(Listing) = 2,
    Sale(Sale) = 3,
    Principal(Principal) = 4,
    Claim(Claim) = 5,
    Operation(Box<OperationRecord>) = 6,
    Reservation(u128) = 7,
    Contribution(OperatingContribution) = 8,
    ApprovalWatermark(ApprovalWatermark) = 9,
}

/// Opaque validated envelope. Existing model payload field order is unchanged.
/// The checksum detects accidental corruption, NOT malicious storage replacement
/// or receipt truth. Only a trusted backend may supply authoritative state.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Record {
    magic: [u8; 8],
    schema: u16,
    domain: [u8; 32],
    pub(crate) key: RecordKey,
    pub(crate) revision: u64,
    pub(crate) value: Value,
}

impl Record {
    pub(crate) fn new(
        domain: &DeploymentDomain,
        key: RecordKey,
        revision: u64,
        value: Value,
    ) -> StorageResult<Self> {
        let record = Self {
            magic: MAGIC,
            schema: 1,
            domain: domain.commitment()?,
            key,
            revision,
            value,
        };
        record.validate(domain, &record.key)?;
        Ok(record)
    }
    pub fn key(&self) -> &RecordKey {
        &self.key
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn listing(&self) -> Option<&Listing> {
        if let Value::Listing(v) = &self.value {
            Some(v)
        } else {
            None
        }
    }
    pub fn sale(&self) -> Option<&Sale> {
        if let Value::Sale(v) = &self.value {
            Some(v)
        } else {
            None
        }
    }
    pub fn claim(&self) -> Option<&Claim> {
        if let Value::Claim(v) = &self.value {
            Some(v)
        } else {
            None
        }
    }
    pub fn operation(&self) -> Option<&OperationContext> {
        if let Value::Operation(v) = &self.value {
            Some(&v.context)
        } else {
            None
        }
    }
    pub fn operation_terminal(&self) -> bool {
        matches!(&self.value, Value::Operation(op) if op.progress == Progress::Terminal)
    }
    pub fn operation_pending(&self) -> bool {
        matches!(&self.value, Value::Operation(op) if op.progress == Progress::Pending)
    }
    pub fn latest_generation(&self) -> Option<Generation> {
        match &self.value {
            Value::Token(slot) => Some(slot.latest),
            _ => None,
        }
    }
    pub fn token_verification_matches(&self, generation: Generation, approval: u64) -> bool {
        matches!(&self.value, Value::Token(slot)
            if slot.latest == generation && slot.consumed_approval == approval && slot.sale_lock.is_none())
    }

    fn validate(&self, domain: &DeploymentDomain, key: &RecordKey) -> StorageResult<()> {
        if self.magic != MAGIC || self.schema != 1 {
            return Err(StorageError::Schema);
        }
        if self.domain != domain.commitment()? {
            return Err(StorageError::Domain);
        }
        if &self.key != key || self.revision == 0 {
            return Err(StorageError::Identity);
        }
        key.validate(domain)?;
        match (key, &self.value) {
            (RecordKey::ApprovalWatermark(_), Value::ApprovalWatermark(w)) => {
                if !w.authenticated && w.high_water != 0 {
                    return Err(StorageError::Invariant);
                }
            }
            (RecordKey::Root, Value::Root(root)) => root.validate()?,
            (RecordKey::Token(_), Value::Token(slot)) => {
                if slot.latest.value() == 0 || slot.sale_lock.is_some_and(|s| s.value() == 0) {
                    return Err(StorageError::Invariant);
                }
            }
            (RecordKey::Listing(id), Value::Listing(listing)) if id == &listing.generation => {
                validate_terms(&listing.terms, domain)?;
                validate_attempt(listing.verification)?;
                let has_sale = matches!(
                    listing.state,
                    ListingState::Locked
                        | ListingState::Review
                        | ListingState::Sold
                        | ListingState::Failed
                );
                if has_sale != listing.sale.is_some()
                    || listing.sale.is_some_and(|s| s.value() == 0)
                {
                    return Err(StorageError::Invariant);
                }
            }
            (RecordKey::Sale(id), Value::Sale(sale)) if id == &sale.id => {
                validate_terms(&sale.terms, domain)?;
                validate_attempt(sale.transfer)?;
                if sale.generation.value() == 0
                    || sale.buyer == sale.terms.seller
                    || sale.buyer == domain.marketplace
                {
                    return Err(StorageError::Invariant);
                }
            }
            (RecordKey::Principal(_), Value::Principal(p)) => {
                if p.generation.value() == 0
                    || p.price == 0
                    || p.fee >= p.price
                    || p.seller == p.buyer
                    || p.seller == domain.marketplace
                    || p.buyer == domain.marketplace
                    || p.treasury == domain.marketplace
                    || p.treasury == domain.nft
                    || p.claims.len() > 2
                    || p.claims.iter().any(|id| id.value() == 0)
                    || (p.claims.len() == 2 && p.claims[0] == p.claims[1])
                    || (p.allocated && (p.held != 0 || p.claims.is_empty()))
                    || (!p.allocated && (p.held != p.price || !p.claims.is_empty()))
                {
                    return Err(StorageError::Invariant);
                }
            }
            (RecordKey::Claim(id), Value::Claim(c)) if id == &c.id => {
                if c.sale.value() == 0
                    || c.generation.value() == 0
                    || c.amount == 0
                    || c.recipient == domain.marketplace
                    || c.seller_amount
                        .checked_add(c.fee_amount)
                        .and_then(|v| v.checked_add(c.refund_amount))
                        != Some(c.amount)
                    || (c.refund_amount != 0 && (c.seller_amount != 0 || c.fee_amount != 0))
                {
                    return Err(StorageError::Invariant);
                }
                if let Some(a) = c.attempt {
                    validate_attempt(a)?;
                }
                if c.state == ClaimState::Available {
                    if c.attempt.is_some() || c.last_dispatch_ns.is_some() || c.debit_pending {
                        return Err(StorageError::Invariant);
                    }
                } else if c.attempt.is_none()
                    || c.last_dispatch_ns.is_none()
                    || (matches!(c.state, ClaimState::Paid | ClaimState::Retryable)
                        && c.debit_pending)
                {
                    return Err(StorageError::Invariant);
                }
            }
            (key, Value::Operation(op)) => {
                op.context.validate()?;
                if &op.context.domain != domain {
                    return Err(StorageError::Domain);
                }
                let identity_matches = match (key, &op.context.descriptor) {
                    (RecordKey::Verification(id), Descriptor::Verification { generation, .. }) => {
                        id == generation
                    }
                    (RecordKey::Transfer(id), Descriptor::ApprovedTransfer { sale, .. }) => {
                        id == sale
                    }
                    (RecordKey::Payment(id), Descriptor::Payment { claim, .. }) => id == claim,
                    _ => false,
                };
                if !identity_matches {
                    return Err(StorageError::Identity);
                }
            }
            (RecordKey::Reservation(_), Value::Reservation(amount)) if *amount > 0 => {}
            (RecordKey::Contribution(_), Value::Contribution(c))
                if c.amount > 0
                    && c.deficit_repair.checked_add(c.free_capital_added) == Some(c.amount) => {}
            _ => return Err(StorageError::Identity),
        }
        Ok(())
    }
}

pub(crate) fn validate_token(token: &TokenKey, domain: &DeploymentDomain) -> StorageResult<()> {
    TokenKey::new(token.nft().clone(), token.token().to_owned())?;
    if token.nft() != &domain.nft {
        return Err(StorageError::Identity);
    }
    Ok(())
}

pub(crate) fn validate_terms(terms: &ListingTerms, domain: &DeploymentDomain) -> StorageResult<()> {
    validate_token(&terms.token, domain)?;
    terms.fees.fee(terms.price)?;
    if terms.seller == domain.marketplace
        || terms.fees.treasury() == &domain.marketplace
        || terms.fees.treasury() == &domain.nft
        || terms.policy_hash == [0; 32]
        || terms.expires_at_ns == 0
    {
        return Err(StorageError::Invariant);
    }
    Ok(())
}

fn validate_attempt(attempt: Attempt) -> StorageResult<()> {
    if attempt.nonce == 0 || attempt.operation_hash == [0; 32] {
        return Err(StorageError::Invariant);
    }
    Ok(())
}

pub fn encode_record(record: &Record, domain: &DeploymentDomain) -> StorageResult<Vec<u8>> {
    record.validate(domain, &record.key)?;
    let mut bytes = borsh::to_vec(record).map_err(|_| StorageError::Encoding)?;
    if bytes.len() + 32 > MAX_RECORD_BYTES {
        return Err(StorageError::Bounds);
    }
    bytes.extend(checksum(&bytes));
    Ok(bytes)
}

pub fn decode_record(
    bytes: &[u8],
    domain: &DeploymentDomain,
    key: &RecordKey,
) -> StorageResult<Record> {
    if bytes.len() > MAX_RECORD_BYTES || bytes.len() < 32 {
        return Err(StorageError::Bounds);
    }
    let (payload, digest) = bytes.split_at(bytes.len() - 32);
    if checksum(payload).as_slice() != digest {
        return Err(StorageError::Encoding);
    }
    // Preflight every variable-length field BEFORE derived Borsh allocates.
    let mut reader = WireReader(payload);
    reader.take(8 + 2 + 32)?;
    reader.key()?;
    reader.take(8)?;
    reader.value()?;
    reader.end()?;
    let record: Record = borsh::from_slice(payload).map_err(|_| StorageError::Encoding)?;
    record.validate(domain, key)?;
    Ok(record)
}

pub(crate) fn checksum(bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(CHECKSUM_TAG);
    hash.update(bytes);
    hash.finalize().into()
}

/// Allocation-free structural preflight. Strings are byte-bounded, vectors have
/// protocol bounds, and options/enums/bools reject noncanonical discriminants.
pub(crate) struct WireReader<'a>(pub &'a [u8]);
impl<'a> WireReader<'a> {
    fn take(&mut self, n: usize) -> StorageResult<&'a [u8]> {
        if n > self.0.len() {
            return Err(StorageError::Encoding);
        }
        let (value, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(value)
    }
    fn byte(&mut self) -> StorageResult<u8> {
        Ok(self.take(1)?[0])
    }
    fn flag(&mut self) -> StorageResult<bool> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(StorageError::Encoding),
        }
    }
    fn len(&mut self, max: usize) -> StorageResult<usize> {
        let n = u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| StorageError::Encoding)?,
        ) as usize;
        if n > max {
            return Err(StorageError::Bounds);
        }
        Ok(n)
    }
    fn string(&mut self, max: usize) -> StorageResult<()> {
        let len = self.len(max)?;
        std::str::from_utf8(self.take(len)?).map_err(|_| StorageError::Encoding)?;
        Ok(())
    }
    fn account(&mut self) -> StorageResult<()> {
        self.string(64)
    }
    fn token(&mut self) -> StorageResult<()> {
        self.account()?;
        self.string(1_024)
    }
    fn optional(&mut self, n: usize) -> StorageResult<()> {
        if self.flag()? {
            self.take(n)?;
        }
        Ok(())
    }
    fn terms(&mut self) -> StorageResult<()> {
        self.token()?;
        self.account()?;
        self.take(8 + 16 + 8 + 2 + 2)?;
        self.account()?;
        self.take(8 + 32 + 32)?;
        Ok(())
    }
    fn key(&mut self) -> StorageResult<()> {
        match self.byte()? {
            0 => {}
            1 | 12 => self.token()?,
            2..=8 => {
                self.take(8)?;
            }
            9..=10 => {
                self.take(32)?;
            }
            _ => return Err(StorageError::Schema),
        };
        Ok(())
    }
    fn domain(&mut self) -> StorageResult<()> {
        self.flag()?;
        self.take(4 * 32)?;
        self.account()?;
        self.account()
    }
    pub(crate) fn operation(&mut self) -> StorageResult<()> {
        self.take(2)?;
        self.domain()?;
        match self.byte()? {
            0 => {
                self.take(8)?;
                self.terms()?;
            }
            1 => {
                self.take(16)?;
                self.terms()?;
                self.account()?;
            }
            2 => {
                self.take(24)?;
                self.account()?;
                self.take(4 * 16)?;
            }
            _ => return Err(StorageError::Schema),
        }
        self.take(8 + 3 * 8 + 4 + 2 * 32)?;
        Ok(())
    }
    fn value(&mut self) -> StorageResult<()> {
        match self.byte()? {
            0 => {
                self.take(24)?;
                self.optional(8)?;
                self.take(16 + 9 * 16)?;
                self.flag()?;
                self.flag()?;
            }
            1 => {
                self.take(16)?;
                self.optional(8)?;
            }
            2 => {
                self.take(8)?;
                self.terms()?;
                if self.byte()? > 8 {
                    return Err(StorageError::Schema);
                }
                self.take(40)?;
                self.optional(8)?;
            }
            3 => {
                self.take(16)?;
                self.terms()?;
                self.account()?;
                self.take(40)?;
                if self.byte()? > 3 {
                    return Err(StorageError::Schema);
                }
            }
            4 => {
                self.take(24)?;
                self.account()?;
                self.account()?;
                self.account()?;
                self.take(32)?;
                self.flag()?;
                let n = self.len(2)?;
                self.take(n * 8)?;
            }
            5 => {
                self.take(24)?;
                self.account()?;
                self.take(64)?;
                if self.byte()? > 4 {
                    return Err(StorageError::Schema);
                }
                self.optional(40)?;
                self.optional(8)?;
                self.flag()?;
            }
            6 => {
                self.operation()?;
                if self.byte()? > 2 {
                    return Err(StorageError::Schema);
                }
            }
            7 => {
                self.take(16)?;
            }
            8 => {
                self.take(48)?;
            }
            9 => {
                self.flag()?;
                self.take(8)?;
            }
            _ => return Err(StorageError::Schema),
        }
        Ok(())
    }
    pub(crate) fn end(&self) -> StorageResult<()> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(StorageError::Encoding)
        }
    }
}
