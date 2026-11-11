//! Versioned byte records and an OFFLINE transactional model, not a NEAR adapter.
//!
//! No ABI, promises, receipt verifier, policy approval or activation is provided.
//! Model observations remain synthetic inputs. Commitments describe intent only.
//! A host backend must independently establish receipt atomicity and authority.

mod codec;
mod model;
mod operation;
mod storage;
mod store;

pub use codec::{decode_record, encode_record, Record, RecordKey, MAX_RECORD_BYTES};
pub use model::{OfflineCommand, Prepared, Repository};
pub use operation::{ArtifactDomain, Budgets, DeploymentDomain, OperationContext};
pub use storage::{
    bootstrap_storage_batch, StorageRepository, MAX_STORAGE_RECORD_BYTES, STORAGE_ROOT_KEY,
};
pub use store::{bootstrap_batch, AtomicBatch, AtomicStore, MemoryStore};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageError {
    Encoding,
    Bounds,
    Schema,
    Domain,
    Identity,
    Invariant,
    Missing,
    Conflict,
    Overflow,
    InjectedFailure,
    Model(crate::Error),
}

impl From<crate::Error> for StorageError {
    fn from(value: crate::Error) -> Self {
        Self::Model(value)
    }
}

pub type StorageResult<T> = std::result::Result<T, StorageError>;

#[cfg(test)]
mod tests;
