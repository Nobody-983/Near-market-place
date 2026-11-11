//! Additive storage-accounting codec/repository. Frozen NMPREC01 records and
//! operation vectors are untouched. Every batch CASes and advances STATE too.
use std::collections::BTreeMap;

use borsh::{BorshDeserialize, BorshSerialize};
use sha2::{Digest, Sha256};

use super::{
    codec::Value as V1Value, decode_record, encode_record, AtomicBatch, AtomicStore,
    DeploymentDomain, MemoryStore, Record, RecordKey, StorageError, StorageResult,
};
use crate::{
    storage_accounting::{
        add, credit_boundary, sub, Credit, CreditKind, LegacyProtection, Reservation,
        StorageAccounting, StorageCommand, StorageTotals, StorageWithdrawal, WithdrawalState,
    },
    types::CallbackResult,
    AccountId, Error,
};

pub const STORAGE_ROOT_KEY: &[u8] = b"nmp:s:root:v1";
pub const MAX_STORAGE_RECORD_BYTES: usize = 1024;
const MAGIC: [u8; 8] = *b"NMPSTG01";
const TAG: &[u8] = b"near-marketplace:storage-record:v1\0";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, BorshSerialize, BorshDeserialize)]
enum Key {
    Root,
    Account(AccountId),
    Reservation([u8; 32]),
    Withdrawal([u8; 32]),
    Credit([u8; 32]),
    // Append-only discriminant: existing v1 keys/values are unchanged.
    AdmissionGuard([u8; 32]),
}
impl Key {
    fn bytes(&self, domain: &DeploymentDomain) -> StorageResult<Vec<u8>> {
        match self {
            Self::Account(a) if a == &domain.marketplace => return Err(StorageError::Identity),
            Self::Reservation(id)
            | Self::Withdrawal(id)
            | Self::Credit(id)
            | Self::AdmissionGuard(id)
                if id == &[0; 32] =>
            {
                return Err(StorageError::Identity)
            }
            _ => {}
        }
        if *self == Self::Root {
            return Ok(STORAGE_ROOT_KEY.to_vec());
        }
        let mut bytes = b"nmp:s:\x01\x00".to_vec();
        bytes.extend(domain.commitment()?);
        bytes.extend(borsh::to_vec(self).map_err(|_| StorageError::Encoding)?);
        Ok(bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
enum Value {
    Root(StorageAccounting),
    Account(StorageTotals),
    Reservation(Reservation),
    Withdrawal(StorageWithdrawal),
    Credit(Credit),
    AdmissionGuard { operation: [u8; 32], released: bool },
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct Envelope {
    magic: [u8; 8],
    domain: [u8; 32],
    key: Key,
    revision: u64,
    value: Value,
}

fn digest(tag: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(tag);
    h.update(bytes);
    h.finalize().into()
}
impl Envelope {
    fn validate(&self, domain: &DeploymentDomain, key: &Key) -> StorageResult<()> {
        key.bytes(domain)?;
        if self.magic != MAGIC
            || self.domain != domain.commitment()?
            || &self.key != key
            || self.revision == 0
        {
            return Err(StorageError::Identity);
        }
        match (key, &self.value) {
            (Key::AdmissionGuard(_), Value::AdmissionGuard { operation, .. })
                if *operation != [0; 32] => {}
            (Key::Root, Value::Root(r)) => {
                r.totals.validate()?;
                add(r.totals.funded, r.unattributed)?;
            }
            (Key::Account(_), Value::Account(t)) => t.validate()?,
            (Key::Reservation(_), Value::Reservation(r)) => {
                r.validate()?;
                Key::Account(r.owner.clone()).bytes(domain)?;
            }
            (Key::Withdrawal(id), Value::Withdrawal(w)) => {
                w.validate()?;
                Key::Account(w.owner.clone()).bytes(domain)?;
                if w.commitment != withdrawal_commitment(domain, *id, &w.owner, w.amount)? {
                    return Err(StorageError::Identity);
                }
            }
            (Key::Credit(_), Value::Credit(c)) if c.amount > 0 => match &c.kind {
                CreditKind::StorageFunding(a) | CreditKind::StorageFundingCharged(a) => {
                    Key::Account(a.clone()).bytes(domain)?;
                }
                CreditKind::WithdrawalRefund {
                    withdrawal,
                    commitment,
                } if *withdrawal == [0; 32] || *commitment == [0; 32] => {
                    return Err(StorageError::Identity)
                }
                _ => {}
            },
            _ => return Err(StorageError::Invariant),
        }
        Ok(())
    }
    fn encode(&self, domain: &DeploymentDomain) -> StorageResult<Vec<u8>> {
        self.validate(domain, &self.key)?;
        let mut bytes = borsh::to_vec(self).map_err(|_| StorageError::Encoding)?;
        bytes.extend(digest(TAG, &bytes));
        if bytes.len() > MAX_STORAGE_RECORD_BYTES {
            return Err(StorageError::Bounds);
        }
        Ok(bytes)
    }
}

// Only AccountId fields can allocate. Validate every length/discriminant and
// exact consumption before invoking Borsh. No Vec/map is encoded in a value.
fn decode(bytes: &[u8], domain: &DeploymentDomain, key: &Key) -> StorageResult<Envelope> {
    if bytes.len() > MAX_STORAGE_RECORD_BYTES || bytes.len() < 32 {
        return Err(StorageError::Bounds);
    }
    let (payload, hash) = bytes.split_at(bytes.len() - 32);
    if digest(TAG, payload).as_slice() != hash {
        return Err(StorageError::Encoding);
    }
    let mut p = Cursor(payload);
    p.take(40)?;
    match p.byte()? {
        0 => {}
        1 => p.account()?,
        2..=5 => {
            p.take(32)?;
        }
        _ => return Err(StorageError::Schema),
    }
    p.take(8)?;
    match p.byte()? {
        0 => {
            p.take(8 * 16)?;
            p.flag()?;
        }
        1 => {
            p.take(7 * 16)?;
        }
        2 => {
            p.account()?;
            p.take(3 * 16 + 8)?;
        }
        3 => {
            p.account()?;
            p.take(16 + 32)?;
            if p.byte()? > 4 {
                return Err(StorageError::Schema);
            }
            p.flag()?;
        }
        4 => {
            p.take(16)?;
            match p.byte()? {
                0 | 3 => p.account()?,
                1 => {}
                2 => {
                    p.take(64)?;
                }
                _ => return Err(StorageError::Schema),
            }
        }
        5 => {
            p.take(32)?;
            p.flag()?;
        }
        _ => return Err(StorageError::Schema),
    }
    if !p.0.is_empty() {
        return Err(StorageError::Encoding);
    }
    let value: Envelope = borsh::from_slice(payload).map_err(|_| StorageError::Encoding)?;
    value.validate(domain, key)?;
    Ok(value)
}
struct Cursor<'a>(&'a [u8]);
impl Cursor<'_> {
    fn take(&mut self, n: usize) -> StorageResult<&[u8]> {
        if n > self.0.len() {
            return Err(StorageError::Encoding);
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn byte(&mut self) -> StorageResult<u8> {
        Ok(self.take(1)?[0])
    }
    fn flag(&mut self) -> StorageResult<()> {
        if self.byte()? > 1 {
            Err(StorageError::Encoding)
        } else {
            Ok(())
        }
    }
    fn account(&mut self) -> StorageResult<()> {
        let n = u32::from_le_bytes(self.take(4)?.try_into().unwrap()) as usize;
        if !(2..=64).contains(&n) {
            return Err(StorageError::Bounds);
        }
        self.take(n)?;
        Ok(())
    }
}

fn withdrawal_commitment(
    domain: &DeploymentDomain,
    id: [u8; 32],
    owner: &AccountId,
    amount: u128,
) -> StorageResult<[u8; 32]> {
    let bytes = borsh::to_vec(&(domain.commitment()?, id, owner, amount, 1u64))
        .map_err(|_| StorageError::Encoding)?;
    Ok(digest(b"near-marketplace:storage-withdrawal:v1\0", &bytes))
}

fn legacy(record: &Record) -> StorageResult<LegacyProtection> {
    let V1Value::Root(root) = &record.value else {
        return Err(StorageError::Invariant);
    };
    Ok(LegacyProtection {
        totals: root.totals,
        free_capital: root.free_capital,
        financial_hold: root.financial_hold,
    })
}

/// Pure fresh-bootstrap factory; no initializer/migration authority. Requires
/// exactly the canonical empty v1 root. It cannot adopt a populated old ledger.
pub fn bootstrap_storage_batch(domain: &DeploymentDomain) -> StorageResult<AtomicBatch> {
    let old = super::bootstrap_batch(domain)?;
    let root = old.writes().next().ok_or(StorageError::Missing)?.1.to_vec();
    let head = Envelope {
        magic: MAGIC,
        domain: domain.commitment()?,
        key: Key::Root,
        revision: 1,
        value: Value::Root(StorageAccounting::empty()),
    }
    .encode(domain)?;
    Ok(AtomicBatch {
        domain: domain.commitment()?,
        checks: vec![
            (b"STATE".to_vec(), Some(root.clone())),
            (STORAGE_ROOT_KEY.to_vec(), None),
        ],
        writes: vec![(b"STATE".to_vec(), root), (STORAGE_ROOT_KEY.to_vec(), head)],
        limit: 2,
    })
}

pub struct StorageRepository<S: AtomicStore> {
    store: S,
    domain: DeploymentDomain,
}
impl StorageRepository<MemoryStore> {
    pub fn new_offline(domain: DeploymentDomain) -> StorageResult<Self> {
        let mut store = super::Repository::new_offline(domain.clone(), None)?.into_store();
        store.apply(&bootstrap_storage_batch(&domain)?)?;
        Self::open(store, domain)
    }
}
impl<S: AtomicStore> StorageRepository<S> {
    pub fn open(store: S, domain: DeploymentDomain) -> StorageResult<Self> {
        // Reuse the existing artifact/domain checks; no missing-root fallback.
        let base = super::Repository::open(store, domain.clone())?;
        let value = Self {
            store: base.into_store(),
            domain,
        };
        value.snapshot()?;
        Ok(value)
    }
    pub fn into_store(self) -> S {
        self.store
    }
    fn read(&self, key: &Key) -> StorageResult<Option<Envelope>> {
        self.store
            .read(&key.bytes(&self.domain)?)?
            .map(|b| decode(&b, &self.domain, key))
            .transpose()
    }
    pub fn snapshot(&self) -> StorageResult<(LegacyProtection, StorageAccounting)> {
        let b = self.store.read(b"STATE")?.ok_or(StorageError::Missing)?;
        let root = decode_record(&b, &self.domain, &RecordKey::Root)?;
        let Some(Envelope {
            value: Value::Root(head),
            ..
        }) = self.read(&Key::Root)?
        else {
            return Err(StorageError::Missing);
        };
        Ok((legacy(&root)?, head))
    }
    pub fn account(&self, owner: &AccountId) -> StorageResult<Option<StorageTotals>> {
        match self.read(&Key::Account(owner.clone()))? {
            None => Ok(None),
            Some(Envelope {
                value: Value::Account(a),
                ..
            }) => Ok(Some(a)),
            _ => Err(StorageError::Invariant),
        }
    }
    pub fn reservation_remaining(&self, id: [u8; 32]) -> StorageResult<u128> {
        if self
            .admission_guard(id)?
            .is_some_and(|(_, released)| released)
        {
            return Err(StorageError::Conflict);
        }
        match self.read(&Key::Reservation(id))? {
            Some(Envelope {
                value: Value::Reservation(r),
                ..
            }) => Ok(r.remaining),
            _ => Err(StorageError::Missing),
        }
    }
    /// Read-only attribution and replay cursor for an internally bound operation.
    pub fn reservation_status(&self, id: [u8; 32]) -> StorageResult<(AccountId, u128, u64)> {
        if self
            .admission_guard(id)?
            .is_some_and(|(_, released)| released)
        {
            return Err(StorageError::Conflict);
        }
        match self.read(&Key::Reservation(id))? {
            Some(Envelope {
                value: Value::Reservation(r),
                ..
            }) => Ok((r.owner, r.remaining, r.nonce)),
            _ => Err(StorageError::Missing),
        }
    }
    /// None denotes a legacy reservation, which has no unused-release proof.
    pub fn admission_guard(&self, id: [u8; 32]) -> StorageResult<Option<([u8; 32], bool)>> {
        match self.read(&Key::AdmissionGuard(id))? {
            None => Ok(None),
            Some(Envelope {
                value:
                    Value::AdmissionGuard {
                        operation,
                        released,
                    },
                ..
            }) => Ok(Some((operation, released))),
            _ => Err(StorageError::Invariant),
        }
    }
    pub fn withdrawal(&self, id: [u8; 32]) -> StorageResult<StorageWithdrawal> {
        match self.read(&Key::Withdrawal(id))? {
            Some(Envelope {
                value: Value::Withdrawal(w),
                ..
            }) => Ok(w),
            _ => Err(StorageError::Missing),
        }
    }
    pub fn commit(&mut self, batch: &AtomicBatch) -> StorageResult<()> {
        self.store.apply(batch)
    }
    pub fn prepare(&self, command: StorageCommand) -> StorageResult<AtomicBatch> {
        let root_bytes = self.store.read(b"STATE")?.ok_or(StorageError::Missing)?;
        let mut root = decode_record(&root_bytes, &self.domain, &RecordKey::Root)?;
        let protection = legacy(&root)?;
        let mut view = View {
            repo: self,
            read: BTreeMap::new(),
            writes: BTreeMap::new(),
        };
        let Value::Root(mut head) = view.get(Key::Root)?.ok_or(StorageError::Missing)? else {
            return Err(StorageError::Invariant);
        };
        view.execute(&mut head, protection, command)?;
        // One sticky hold across both accounting subsystems. Persist it in the
        // same CAS so legacy payment admission cannot miss a storage shortfall.
        if head.financial_hold {
            let V1Value::Root(base) = &mut root.value else {
                return Err(StorageError::Invariant);
            };
            base.financial_hold = true;
        }
        view.put(Key::Root, Value::Root(head))?;
        let root = Record::new(
            &self.domain,
            RecordKey::Root,
            root.revision()
                .checked_add(1)
                .ok_or(StorageError::Overflow)?,
            root.value,
        )?;
        let mut checks = vec![(b"STATE".to_vec(), Some(root_bytes))];
        let mut writes = vec![(b"STATE".to_vec(), encode_record(&root, &self.domain)?)];
        for (key, bytes) in view.read {
            checks.push((key.bytes(&self.domain)?, bytes));
        }
        for (key, value) in view.writes {
            writes.push((key.bytes(&self.domain)?, value.encode(&self.domain)?));
        }
        let batch = AtomicBatch {
            domain: self.domain.commitment()?,
            checks,
            writes,
            limit: 8,
        };
        batch.validate()?;
        Ok(batch)
    }
}

struct View<'a, S: AtomicStore> {
    repo: &'a StorageRepository<S>,
    read: BTreeMap<Key, Option<Vec<u8>>>,
    writes: BTreeMap<Key, Envelope>,
}
impl<S: AtomicStore> View<'_, S> {
    fn get(&mut self, key: Key) -> StorageResult<Option<Value>> {
        if let Some(v) = self.writes.get(&key) {
            return Ok(Some(v.value.clone()));
        }
        if !self.read.contains_key(&key) {
            if self.read.len() >= 7 {
                return Err(StorageError::Bounds);
            }
            let b = self.repo.store.read(&key.bytes(&self.repo.domain)?)?;
            self.read.insert(key.clone(), b);
        }
        self.read[&key]
            .as_ref()
            .map(|b| decode(b, &self.repo.domain, &key).map(|e| e.value))
            .transpose()
    }
    fn put(&mut self, key: Key, value: Value) -> StorageResult<()> {
        self.get(key.clone())?;
        let previous = self.read[&key]
            .as_ref()
            .map(|b| decode(b, &self.repo.domain, &key))
            .transpose()?;
        let revision = previous.map_or(Ok(1), |e| {
            e.revision.checked_add(1).ok_or(StorageError::Overflow)
        })?;
        self.writes.insert(
            key.clone(),
            Envelope {
                magic: MAGIC,
                domain: self.repo.domain.commitment()?,
                key,
                revision,
                value,
            },
        );
        Ok(())
    }
    fn absent(&mut self, key: Key) -> StorageResult<()> {
        if self.get(key)?.is_some() {
            Err(StorageError::Conflict)
        } else {
            Ok(())
        }
    }
    fn account(&mut self, owner: &AccountId) -> StorageResult<StorageTotals> {
        match self.get(Key::Account(owner.clone()))? {
            Some(Value::Account(a)) => Ok(a),
            None => Ok(StorageTotals::default()),
            _ => Err(StorageError::Invariant),
        }
    }
    fn withdrawal(
        &mut self,
        id: [u8; 32],
        commitment: [u8; 32],
    ) -> StorageResult<StorageWithdrawal> {
        let Some(Value::Withdrawal(w)) = self.get(Key::Withdrawal(id))? else {
            return Err(StorageError::Missing);
        };
        if w.commitment != commitment {
            return Err(Error::StaleAttempt.into());
        }
        let Some(Value::Account(account)) = self.get(Key::Account(w.owner.clone()))? else {
            return Err(StorageError::Missing);
        };
        if w.amount > account.funded
            || (matches!(
                w.state,
                WithdrawalState::Queued | WithdrawalState::Debited | WithdrawalState::Unknown
            ) && w.amount > account.in_flight)
            || (w.state == WithdrawalState::Queued && w.amount > account.queued)
            || (w.state == WithdrawalState::Paid && w.amount > account.withdrawn)
        {
            return Err(StorageError::Invariant);
        }
        Ok(w)
    }
    fn update_account<F>(
        &mut self,
        head: &mut StorageAccounting,
        owner: &AccountId,
        mut update: F,
    ) -> StorageResult<()>
    where
        F: FnMut(&mut StorageTotals) -> crate::Result<()>,
    {
        let mut account = self.account(owner)?;
        if account.funded > head.totals.funded
            || account.available > head.totals.available
            || account.reserved > head.totals.reserved
            || account.staked > head.totals.staked
            || account.in_flight > head.totals.in_flight
            || account.queued > head.totals.queued
            || account.withdrawn > head.totals.withdrawn
        {
            return Err(StorageError::Invariant);
        }
        update(&mut account)?;
        update(&mut head.totals)?;
        account.validate()?;
        head.totals.validate()?;
        self.put(Key::Account(owner.clone()), Value::Account(account))
    }
    fn execute(
        &mut self,
        head: &mut StorageAccounting,
        legacy: LegacyProtection,
        cmd: StorageCommand,
    ) -> StorageResult<()> {
        use StorageCommand::*;
        match cmd {
            ChargeFundingStorage {
                credit,
                before,
                after,
            } => {
                let Some(Value::Credit(Credit {
                    amount,
                    kind: CreditKind::StorageFunding(owner),
                })) = self.get(Key::Credit(credit))?
                else {
                    return Err(StorageError::Conflict);
                };
                let cost = after
                    .storage_stake
                    .checked_sub(before.storage_stake)
                    .ok_or(Error::InvalidOperation)?;
                if cost > amount
                    || after
                        != (crate::ledger::BalanceObservation {
                            storage_stake: after.storage_stake,
                            ..before
                        })
                {
                    return Err(Error::InvalidOperation.into());
                }
                head.check_backing(legacy, before)?;
                self.update_account(head, &owner, |a| {
                    a.available = sub(a.available, cost)?;
                    a.staked = add(a.staked, cost)?;
                    Ok(())
                })?;
                head.check_backing(legacy, after)?;
                self.put(
                    Key::Credit(credit),
                    Value::Credit(Credit {
                        amount,
                        kind: CreditKind::StorageFundingCharged(owner),
                    }),
                )?;
            }
            Fund {
                credit,
                owner,
                amount,
                before,
                after,
            } => {
                credit_boundary(before, after, amount)?;
                self.absent(Key::Credit(credit))?;
                // A storage credit creates an equal liability, never repairs a
                // deficit or grants operating capital. Holds are not cleared.
                self.update_account(head, &owner, |a| {
                    a.funded = add(a.funded, amount)?;
                    a.available = add(a.available, amount)?;
                    Ok(())
                })?;
                head.check_backing(legacy, after)?;
                self.put(
                    Key::Credit(credit),
                    Value::Credit(Credit {
                        amount,
                        kind: CreditKind::StorageFunding(owner),
                    }),
                )?;
            }
            Reserve {
                reservation,
                owner,
                amount,
                balance,
            } => {
                head.admit(legacy, balance)?;
                if amount == 0 {
                    return Err(Error::InvalidOperation.into());
                }
                self.absent(Key::Reservation(reservation))?;
                self.absent(Key::AdmissionGuard(reservation))?;
                self.update_account(head, &owner, |a| {
                    a.available = sub(a.available, amount)?;
                    a.reserved = add(a.reserved, amount)?;
                    Ok(())
                })?;
                self.put(
                    Key::Reservation(reservation),
                    Value::Reservation(Reservation {
                        owner,
                        allocated: amount,
                        remaining: amount,
                        staked: 0,
                        nonce: 0,
                    }),
                )?;
            }
            ReserveAdmission {
                reservation,
                operation,
                owner,
                amount,
                balance,
            } => {
                if operation == [0; 32] {
                    return Err(StorageError::Identity);
                }
                self.absent(Key::AdmissionGuard(reservation))?;
                self.execute(
                    head,
                    legacy,
                    Reserve {
                        reservation,
                        owner,
                        amount,
                        balance,
                    },
                )?;
                self.put(
                    Key::AdmissionGuard(reservation),
                    Value::AdmissionGuard {
                        operation,
                        released: false,
                    },
                )?;
            }
            ReleaseAdmission {
                reservation,
                operation,
                balance,
            } => {
                let Some(Value::AdmissionGuard {
                    operation: bound,
                    released: false,
                }) = self.get(Key::AdmissionGuard(reservation))?
                else {
                    return Err(StorageError::Conflict);
                };
                let Some(Value::Reservation(r)) = self.get(Key::Reservation(reservation))? else {
                    return Err(StorageError::Missing);
                };
                if operation != bound || r.nonce != 1 || r.staked == 0 {
                    return Err(StorageError::Conflict);
                }
                let account = self.account(&r.owner)?;
                if r.remaining > account.reserved || r.staked > account.staked {
                    return Err(StorageError::Invariant);
                }
                // Reclassify a still-protected liability, even during a sticky
                // hold. Never clear holds, refund stake, or create free capital.
                head.check_backing(legacy, balance)?;
                self.update_account(head, &r.owner, |a| {
                    a.reserved = sub(a.reserved, r.remaining)?;
                    a.available = add(a.available, r.remaining)?;
                    Ok(())
                })?;
                self.put(
                    Key::AdmissionGuard(reservation),
                    Value::AdmissionGuard {
                        operation,
                        released: true,
                    },
                )?;
                // Frozen reservation stays intact as history. Its guard denies
                // every future consumption/read-as-live and ID reuse.
            }
            Consume {
                reservation,
                nonce,
                before,
                after,
            } => {
                if matches!(
                    self.get(Key::AdmissionGuard(reservation))?,
                    Some(Value::AdmissionGuard { released: true, .. })
                ) {
                    return Err(StorageError::Conflict);
                }
                let Some(Value::Reservation(mut r)) = self.get(Key::Reservation(reservation))?
                else {
                    return Err(StorageError::Missing);
                };
                if r.nonce.checked_add(1).ok_or(StorageError::Overflow)? != nonce {
                    return Err(Error::StaleAttempt.into());
                }
                let amount = after
                    .storage_stake
                    .checked_sub(before.storage_stake)
                    .ok_or(Error::InvalidOperation)?;
                let Some(Value::Account(account)) = self.get(Key::Account(r.owner.clone()))? else {
                    return Err(StorageError::Missing);
                };
                if r.remaining > account.reserved || r.staked > account.staked {
                    return Err(StorageError::Invariant);
                }
                if after
                    != (crate::ledger::BalanceObservation {
                        storage_stake: after.storage_stake,
                        ..before
                    })
                {
                    return Err(Error::InvalidOperation.into());
                }
                head.check_backing(legacy, before)?;
                r.remaining = sub(r.remaining, amount)?;
                r.staked = add(r.staked, amount)?;
                r.nonce = nonce;
                self.update_account(head, &r.owner, |a| {
                    a.reserved = sub(a.reserved, amount)?;
                    a.staked = add(a.staked, amount)?;
                    Ok(())
                })?;
                head.check_backing(legacy, after)?;
                self.put(Key::Reservation(reservation), Value::Reservation(r))?;
            }
            Withdraw {
                withdrawal,
                owner,
                amount,
                balance,
            } => {
                head.admit(legacy, balance)?;
                if amount == 0 {
                    return Err(Error::InvalidOperation.into());
                }
                self.absent(Key::Withdrawal(withdrawal))?;
                self.update_account(head, &owner, |a| {
                    a.available = sub(a.available, amount)?;
                    a.in_flight = add(a.in_flight, amount)?;
                    a.queued = add(a.queued, amount)?;
                    Ok(())
                })?;
                let commitment =
                    withdrawal_commitment(&self.repo.domain, withdrawal, &owner, amount)?;
                self.put(
                    Key::Withdrawal(withdrawal),
                    Value::Withdrawal(StorageWithdrawal {
                        owner,
                        amount,
                        commitment,
                        state: WithdrawalState::Queued,
                        refund_attributed: false,
                    }),
                )?;
            }
            Debit {
                withdrawal,
                commitment,
                before,
                after,
            } => {
                let mut w = self.withdrawal(withdrawal, commitment)?;
                if w.state != WithdrawalState::Queued {
                    return Err(Error::WrongState.into());
                }
                if after
                    != (crate::ledger::BalanceObservation {
                        account_balance: sub(before.account_balance, w.amount)?,
                        ..before
                    })
                {
                    return Err(Error::InvalidOperation.into());
                }
                head.check_backing(legacy, before)?;
                self.update_account(head, &w.owner, |a| {
                    a.queued = sub(a.queued, w.amount)?;
                    Ok(())
                })?;
                head.check_backing(legacy, after)?;
                w.state = WithdrawalState::Debited;
                self.put(Key::Withdrawal(withdrawal), Value::Withdrawal(w))?;
            }
            WithdrawalResult {
                withdrawal,
                commitment,
                result,
            } => {
                let mut w = self.withdrawal(withdrawal, commitment)?;
                if !matches!(w.state, WithdrawalState::Debited | WithdrawalState::Unknown) {
                    return Err(Error::WrongState.into());
                }
                if result != CallbackResult::Unknown {
                    self.update_account(head, &w.owner, |a| {
                        a.in_flight = sub(a.in_flight, w.amount)?;
                        if result == CallbackResult::Succeeded {
                            a.withdrawn = add(a.withdrawn, w.amount)?;
                        } else {
                            a.available = add(a.available, w.amount)?;
                        }
                        Ok(())
                    })?;
                }
                w.state = match result {
                    CallbackResult::Succeeded => WithdrawalState::Paid,
                    CallbackResult::Failed => WithdrawalState::Failed,
                    CallbackResult::Unknown => WithdrawalState::Unknown,
                };
                self.put(Key::Withdrawal(withdrawal), Value::Withdrawal(w))?;
            }
            ReceiveUnattributed {
                credit,
                amount,
                before,
                after,
            } => {
                credit_boundary(before, after, amount)?;
                self.absent(Key::Credit(credit))?;
                head.unattributed = add(head.unattributed, amount)?;
                self.put(
                    Key::Credit(credit),
                    Value::Credit(Credit {
                        amount,
                        kind: CreditKind::Unattributed,
                    }),
                )?;
            }
            AttributeWithdrawalRefund {
                credit,
                withdrawal,
                commitment,
            } => {
                let mut w = self.withdrawal(withdrawal, commitment)?;
                let Some(Value::Credit(mut c)) = self.get(Key::Credit(credit))? else {
                    return Err(StorageError::Missing);
                };
                if w.state != WithdrawalState::Failed
                    || w.refund_attributed
                    || !matches!(c.kind, CreditKind::Unattributed)
                    || c.amount != w.amount
                {
                    return Err(Error::WrongState.into());
                }
                // Requires a future authenticated association with this exact
                // failed Transfer. An amount/hash/balance match is NOT evidence.
                head.unattributed = sub(head.unattributed, c.amount)?;
                w.refund_attributed = true;
                c.kind = CreditKind::WithdrawalRefund {
                    withdrawal,
                    commitment,
                };
                self.put(Key::Withdrawal(withdrawal), Value::Withdrawal(w))?;
                self.put(Key::Credit(credit), Value::Credit(c))?;
            }
            CheckHealth { balance } => {
                if head.check_backing(legacy, balance).is_err() || legacy.financial_hold() {
                    head.financial_hold = true;
                }
            }
        }
        head.totals.validate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
