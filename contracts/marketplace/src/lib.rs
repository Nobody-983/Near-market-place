//! Offline, deterministic foundations, NOT a deployable NEAR contract.
//!
//! No method sends promises, accepts chain deposits, or authenticates receipts.
//! A future reviewed adapter must supply authentic observations and enforce all
//! authorization/storage/gas checks before calling these state primitives.
//! Recovery and public transaction entry points are deliberately absent.
//!
//! History-bearing aggregates deliberately cannot become a single serialized
//! contract state. Only bounded records/totals can be encoded by a future store.
//! ```compile_fail
//! fn encode(state: &near_marketplace_foundation::state::Marketplace) {
//!     let _ = borsh::to_vec(state);
//! }
//! ```
//! ```compile_fail
//! fn encode(ledger: &near_marketplace_foundation::ledger::Ledger) {
//!     let _ = borsh::to_vec(ledger);
//! }
//! ```
#![forbid(unsafe_code)]

#[cfg(all(feature = "testnet", feature = "production"))]
compile_error!("testnet and production artifacts must be built separately");

pub mod isolation;
pub mod ledger;
pub mod persistence;
mod records;
pub mod state;
pub mod storage_accounting;
pub mod types;

pub use near_account_id::AccountId;
pub use types::{Error, Result};

#[derive(Clone, Copy, Debug)]
pub enum PublicAction {
    Mint,
    Deposit,
    List,
    Purchase,
    Withdraw,
    Recovery,
}

/// No feature flag, configuration value or wallet identity enables an action.
pub fn authorize_public_action(_: PublicAction) -> Result<()> {
    Err(Error::Disabled)
}
