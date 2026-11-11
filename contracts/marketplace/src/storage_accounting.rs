//! Storage-credit accounting, separate from the frozen v1 operating ledger.
//! These are internal model transitions, NOT authenticated payment/refund APIs.
//! Account/purpose selection and historical refund evidence belong to a future
//! approved adapter. No transition grants governance or release authority.
use borsh::{BorshDeserialize, BorshSerialize};

use crate::{ledger::BalanceObservation, AccountId, Error, Result};

pub(crate) fn add(a: u128, b: u128) -> Result<u128> {
    a.checked_add(b).ok_or(Error::Overflow)
}
pub(crate) fn sub(a: u128, b: u128) -> Result<u128> {
    a.checked_sub(b).ok_or(Error::InsufficientBalance)
}

/// Maintained both per storage account and globally. `queued` is a subset of
/// `in_flight`, not another entitlement. `staked` is already included in host S.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct StorageTotals {
    pub funded: u128,
    pub available: u128,
    pub reserved: u128,
    pub staked: u128,
    pub in_flight: u128,
    pub queued: u128,
    pub withdrawn: u128,
}

impl StorageTotals {
    pub fn validate(&self) -> Result<()> {
        let local = add(add(self.available, self.reserved)?, self.staked)?;
        if add(local, add(self.in_flight, self.withdrawn)?)? != self.funded
            || self.queued > self.in_flight
        {
            return Err(Error::AccountingInvariant);
        }
        Ok(())
    }

    pub fn protected_liquid(&self) -> Result<u128> {
        self.validate()?;
        add(add(self.available, self.reserved)?, self.queued)
    }
}

/// Frozen v1 accounting observed at the same root revision as a storage batch.
/// Actual principal/claim records are never mutated by the storage subsystem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LegacyProtection {
    pub(crate) totals: crate::ledger::LedgerTotals,
    pub(crate) free_capital: u128,
    pub(crate) financial_hold: bool,
}

impl LegacyProtection {
    pub fn financial_hold(&self) -> bool {
        self.financial_hold
    }

    pub(crate) fn required(&self, balance: BalanceObservation) -> Result<u128> {
        self.totals.validate()?;
        let external = add(
            add(balance.storage_stake, balance.external_queued)?,
            add(balance.storage_credits, balance.unattributed)?,
        )?;
        let principal = add(
            add(self.totals.held, self.totals.available)?,
            self.totals.queued_payments,
        )?;
        add(
            add(external, principal)?,
            add(self.totals.reserved_operating, self.free_capital)?,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct StorageAccounting {
    pub(crate) totals: StorageTotals,
    pub(crate) unattributed: u128,
    pub(crate) financial_hold: bool,
}

impl StorageAccounting {
    pub fn empty() -> Self {
        Self {
            totals: StorageTotals::default(),
            unattributed: 0,
            financial_hold: false,
        }
    }
    pub fn totals(&self) -> StorageTotals {
        self.totals
    }
    pub fn unattributed(&self) -> u128 {
        self.unattributed
    }
    pub fn financial_hold(&self) -> bool {
        self.financial_hold
    }

    pub fn check_backing(
        &self,
        legacy: LegacyProtection,
        balance: BalanceObservation,
    ) -> Result<()> {
        self.totals.validate()?;
        // Historical spend is not another liquid reservation. Until an explicit
        // storage-release policy exists, no tracked stake may be freed/repriced.
        if self.totals.staked > balance.storage_stake {
            return Err(Error::InsufficientBalance);
        }
        let required = add(
            legacy.required(balance)?,
            add(self.totals.protected_liquid()?, self.unattributed)?,
        )?;
        sub(balance.account_balance, required)?;
        Ok(())
    }

    pub(crate) fn admit(
        &self,
        legacy: LegacyProtection,
        balance: BalanceObservation,
    ) -> Result<()> {
        if self.financial_hold || legacy.financial_hold {
            return Err(Error::FinancialHold);
        }
        self.check_backing(legacy, balance)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub(crate) struct Reservation {
    pub owner: AccountId,
    pub allocated: u128,
    pub remaining: u128,
    pub staked: u128,
    pub nonce: u64,
}
impl Reservation {
    pub fn validate(&self) -> Result<()> {
        if self.allocated == 0
            || add(self.remaining, self.staked)? != self.allocated
            || (self.staked > 0 && self.nonce == 0)
        {
            return Err(Error::AccountingInvariant);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum WithdrawalState {
    Queued,
    Debited,
    Unknown,
    Failed,
    Paid,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct StorageWithdrawal {
    pub(crate) owner: AccountId,
    pub(crate) amount: u128,
    // One immutable attempt per withdrawal; no resend/recovery/retry API.
    pub(crate) commitment: [u8; 32],
    pub(crate) state: WithdrawalState,
    pub(crate) refund_attributed: bool,
}
impl StorageWithdrawal {
    pub fn recipient(&self) -> &AccountId {
        &self.owner
    }
    pub fn amount(&self) -> u128 {
        self.amount
    }
    pub fn commitment(&self) -> [u8; 32] {
        self.commitment
    }
    pub fn state(&self) -> WithdrawalState {
        self.state
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if self.amount == 0
            || self.commitment == [0; 32]
            || (self.refund_attributed && self.state != WithdrawalState::Failed)
        {
            return Err(Error::AccountingInvariant);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub(crate) enum CreditKind {
    StorageFunding(AccountId),
    Unattributed,
    WithdrawalRefund {
        withdrawal: [u8; 32],
        commitment: [u8; 32],
    },
    // Append-only tag, same encoded width as StorageFunding. The funding
    // receipt charged its own measured metadata exactly once.
    StorageFundingCharged(AccountId),
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub(crate) struct Credit {
    pub amount: u128,
    pub kind: CreditKind,
}

/// Offline/internal intentions only. No wire codec or transaction entry point.
/// Snapshots and refund linkage must never be accepted from a public request.
#[derive(Clone, Debug)]
pub enum StorageCommand {
    /// Internal same-receipt completion of Fund, never caller-supplied evidence.
    ChargeFundingStorage {
        credit: [u8; 32],
        before: BalanceObservation,
        after: BalanceObservation,
    },
    Fund {
        credit: [u8; 32],
        owner: AccountId,
        amount: u128,
        before: BalanceObservation,
        after: BalanceObservation,
    },
    Reserve {
        reservation: [u8; 32],
        owner: AccountId,
        amount: u128,
        balance: BalanceObservation,
    },
    /// Preallocate a retained release guard for an exact admission operation.
    /// The adapter must authenticate funding consent and meter this metadata.
    ReserveAdmission {
        reservation: [u8; 32],
        operation: [u8; 32],
        owner: AccountId,
        amount: u128,
        balance: BalanceObservation,
    },
    /// Internal accounting only: caller must prove verification was never sent,
    /// atomically cancel the candidate, and retain its admission tombstone.
    /// Only cursor 1 (creation charge) is releasable. No value leaves the ledger.
    ReleaseAdmission {
        reservation: [u8; 32],
        operation: [u8; 32],
        balance: BalanceObservation,
    },
    Consume {
        reservation: [u8; 32],
        nonce: u64,
        before: BalanceObservation,
        after: BalanceObservation,
    },
    Withdraw {
        withdrawal: [u8; 32],
        owner: AccountId,
        amount: u128,
        balance: BalanceObservation,
    },
    Debit {
        withdrawal: [u8; 32],
        commitment: [u8; 32],
        before: BalanceObservation,
        after: BalanceObservation,
    },
    WithdrawalResult {
        withdrawal: [u8; 32],
        commitment: [u8; 32],
        result: crate::types::CallbackResult,
    },
    ReceiveUnattributed {
        credit: [u8; 32],
        amount: u128,
        before: BalanceObservation,
        after: BalanceObservation,
    },
    AttributeWithdrawalRefund {
        credit: [u8; 32],
        withdrawal: [u8; 32],
        commitment: [u8; 32],
    },
    CheckHealth {
        balance: BalanceObservation,
    },
}

pub(crate) fn credit_boundary(
    before: BalanceObservation,
    after: BalanceObservation,
    amount: u128,
) -> Result<()> {
    if amount == 0
        || after
            != (BalanceObservation {
                account_balance: add(before.account_balance, amount)?,
                ..before
            })
    {
        return Err(Error::InvalidOperation);
    }
    Ok(())
}
