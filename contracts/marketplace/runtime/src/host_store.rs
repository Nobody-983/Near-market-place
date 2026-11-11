//! Raw NEAR trie access: no LookupMap key prefixing or value re-serialization.
use near_marketplace_foundation::persistence::{
    AtomicBatch, AtomicStore, StorageResult, MAX_RECORD_BYTES,
};
use near_sdk::env;

const MAX_KEY_BYTES: usize = 1_200;
// Distinct from the SDK's atomic/eviction registers. Calls are synchronous.
const READ_REGISTER: u64 = 0x4d50_5231;

/// The SDK's storage_read allocates without a cap. This small FFI wrapper loads
/// into a host register, then bounds the guest allocation before reading it.
/// It is the only handwritten unsafe block in the runtime crate.
#[allow(unsafe_code)]
pub(crate) fn read_bounded(key: &[u8]) -> Option<Vec<u8>> {
    if key.is_empty() || key.len() > MAX_KEY_BYTES {
        env::panic_str("Storage key exceeds bound");
    }
    // SAFETY: key points to an initialized slice alive throughout this synchronous
    // host call. Length matches the slice; the host copies bytes and writes only
    // its own register. No guest output pointer or retained reference is passed.
    let status =
        unsafe { near_sys::storage_read(key.len() as u64, key.as_ptr() as u64, READ_REGISTER) };
    match status {
        0 => None,
        1 => match env::read_register_bounded(READ_REGISTER, MAX_RECORD_BYTES) {
            Some(Ok(bytes)) => Some(bytes),
            Some(Err(_)) => env::panic_str("Stored value exceeds bound"),
            None => env::panic_str("Storage host omitted result"),
        },
        _ => env::panic_str("Invalid storage host status"),
    }
}

/// All errors abort, including precondition errors. In particular, there is no
/// ordinary Err path after the first host write. Receipt rollback belongs to the
/// NEAR runtime; the SDK native mock does not provide it.
#[derive(Default)]
pub struct HostStore;

impl AtomicStore for HostStore {
    fn read(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        Ok(read_bounded(key))
    }

    fn apply(&mut self, batch: &AtomicBatch) -> StorageResult<()> {
        batch
            .validate()
            .unwrap_or_else(|_| env::panic_str("Invalid atomic batch"));
        for (key, expected) in batch.checks() {
            if read_bounded(key).as_deref() != expected {
                env::panic_str("Atomic precondition failed");
            }
        }
        for (key, value) in batch.writes() {
            env::storage_write(key, value);
        }
        Ok(())
    }
}
