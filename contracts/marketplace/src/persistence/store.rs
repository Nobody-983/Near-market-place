use std::collections::BTreeMap;

use super::{
    codec::{RootState, Value},
    encode_record,
    operation::hash,
    DeploymentDomain, Record, RecordKey, StorageError, StorageResult, MAX_RECORD_BYTES,
};

pub(crate) const MAX_KEYS: usize = 16;

/// Prepare a canonical empty v1 root, with no financial policy configured.
/// This is a pure batch factory, NOT initialization authority. A host adapter
/// must authenticate bootstrap, check its config sentinel, and fund storage.
/// The required-absent STATE precondition prevents replacing existing roots.
pub fn bootstrap_batch(domain: &DeploymentDomain) -> StorageResult<AtomicBatch> {
    let root = Record::new(
        domain,
        RecordKey::Root,
        1,
        Value::Root(RootState {
            last_generation: 0,
            last_sale: 0,
            last_claim: 0,
            payment_policy: None,
            free_capital: 0,
            totals: crate::ledger::LedgerTotals::default(),
            financial_hold: false,
            trading_paused: true,
        }),
    )?;
    Ok(AtomicBatch {
        domain: domain.commitment()?,
        checks: vec![(b"STATE".to_vec(), None)],
        writes: vec![(b"STATE".to_vec(), encode_record(&root, domain)?)],
        limit: 1,
    })
}

/// An internally prepared, bounded compare-and-swap. Fields have no external
/// constructor/deserializer. Hashes identify a batch, never receipt evidence.
#[derive(Clone, Debug)]
pub struct AtomicBatch {
    pub(crate) domain: [u8; 32],
    pub(crate) checks: Vec<(Vec<u8>, Option<Vec<u8>>)>,
    pub(crate) writes: Vec<(Vec<u8>, Vec<u8>)>,
    pub(crate) limit: usize,
}

impl AtomicBatch {
    pub fn commitment(&self) -> StorageResult<[u8; 32]> {
        self.validate()?;
        hash(
            b"near-marketplace:write-set:v1\0",
            &(self.domain, &self.checks, &self.writes),
        )
    }
    pub fn checks(&self) -> impl Iterator<Item = (&[u8], Option<&[u8]>)> {
        self.checks
            .iter()
            .map(|(k, v)| (k.as_slice(), v.as_deref()))
    }
    pub fn writes(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.writes
            .iter()
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
    }
    pub fn record_count(&self) -> usize {
        self.checks.len()
    }
    pub fn validate(&self) -> StorageResult<()> {
        if self.domain == [0; 32]
            || self.limit == 0
            || self.limit > MAX_KEYS
            || self.checks.is_empty()
            || self.checks.len() > self.limit
            || self.writes.is_empty()
            || self.writes.len() > self.limit
        {
            return Err(StorageError::Bounds);
        }
        for (i, (key, expected)) in self.checks.iter().enumerate() {
            if key.is_empty()
                || key.len() > 1_200
                || expected
                    .as_ref()
                    .is_some_and(|v| v.len() > MAX_RECORD_BYTES)
                || self.checks[..i].iter().any(|(k, _)| k == key)
            {
                return Err(StorageError::Conflict);
            }
        }
        for (i, (key, value)) in self.writes.iter().enumerate() {
            if value.len() > MAX_RECORD_BYTES
                || !self.checks.iter().any(|(k, _)| k == key)
                || self.writes[..i].iter().any(|(k, _)| k == key)
            {
                return Err(StorageError::Conflict);
            }
        }
        if !self.writes.iter().any(|(k, _)| k == b"STATE") {
            return Err(StorageError::Invariant);
        }
        Ok(())
    }
}

/// TRUSTED backend contract: compare ALL preconditions and apply ALL writes in
/// one transaction, rolling back on error/panic. A future NEAR implementation
/// must use receipt rollback, including promises. This trait cannot prove that
/// an arbitrary implementation honors its atomicity or storage-integrity rules.
pub trait AtomicStore {
    fn read(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>>;
    fn apply(&mut self, batch: &AtomicBatch) -> StorageResult<()>;
}

/// Byte-store test harness. Reopening a Repository retains these exact bytes.
/// No native storage, outgoing action or authenticated receipt is simulated.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MemoryStore {
    pub(super) bytes: BTreeMap<Vec<u8>, Vec<u8>>,
    #[cfg(test)]
    pub(super) fail_after: Option<usize>,
    #[cfg(test)]
    pub(super) panic_after: Option<usize>,
}

impl AtomicStore for MemoryStore {
    fn read(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        Ok(self.bytes.get(key).cloned())
    }
    fn apply(&mut self, batch: &AtomicBatch) -> StorageResult<()> {
        batch.validate()?;
        for (key, expected) in &batch.checks {
            if self.bytes.get(key) != expected.as_ref() {
                return Err(StorageError::Conflict);
            }
        }
        // Keep only touched pre-images. Never clone/scan historical storage.
        let mut rollback = Rollback {
            store: &mut self.bytes,
            previous: Vec::with_capacity(batch.writes.len()),
            committed: false,
        };
        for (index, (key, value)) in batch.writes.iter().enumerate() {
            rollback.previous.push((
                key.clone(),
                rollback.store.insert(key.clone(), value.clone()),
            ));
            #[cfg(test)]
            {
                assert_ne!(self.panic_after, Some(index + 1), "injected storage panic");
                if self.fail_after == Some(index + 1) {
                    return Err(StorageError::InjectedFailure);
                }
            }
            #[cfg(not(test))]
            let _ = index;
        }
        rollback.committed = true;
        Ok(())
    }
}

struct Rollback<'a> {
    store: &'a mut BTreeMap<Vec<u8>, Vec<u8>>,
    previous: Vec<(Vec<u8>, Option<Vec<u8>>)>,
    committed: bool,
}
impl Drop for Rollback<'_> {
    fn drop(&mut self) {
        if !self.committed {
            for (key, value) in self.previous.drain(..).rev() {
                match value {
                    Some(v) => {
                        self.store.insert(key, v);
                    }
                    None => {
                        self.store.remove(&key);
                    }
                }
            }
        }
    }
}
