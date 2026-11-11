use crate::AccountId;
use borsh::{BorshDeserialize, BorshSerialize};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Disabled,
    MissingConfiguration,
    WrongEnvironment,
    InvalidIdentity,
    InvalidToken,
    InvalidPrice,
    InvalidFee,
    Overflow,
    Conflict,
    StaleApproval,
    InvalidSeller,
    InvalidBuyer,
    Expired,
    UnknownListing,
    UnknownSale,
    UnknownClaim,
    WrongState,
    StaleAttempt,
    InvalidOperation,
    InsufficientReserve,
    InsufficientBalance,
    FinancialHold,
    TooSoon,
    AccountingInvariant,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

macro_rules! identifier {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, BorshSerialize, BorshDeserialize,
        )]
        pub struct $name(pub(crate) u64);
        impl $name {
            /// Checked wire identifier; this is not authorization to use a record.
            pub fn from_value(value: u64) -> Result<Self> {
                if value == 0 {
                    return Err(Error::InvalidOperation);
                }
                Ok(Self(value))
            }
            pub fn value(self) -> u64 {
                self.0
            }
        }
    };
}
identifier!(Generation);
identifier!(SaleId);
identifier!(ClaimId);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, BorshSerialize, BorshDeserialize)]
pub struct TokenKey {
    nft: AccountId,
    token: String,
}

impl TokenKey {
    pub fn new(nft: AccountId, token: String) -> Result<Self> {
        if token.trim().is_empty() || token.len() > 1_024 {
            return Err(Error::InvalidToken);
        }
        Ok(Self { nft, token })
    }
    pub fn nft(&self) -> &AccountId {
        &self.nft
    }
    pub fn token(&self) -> &str {
        &self.token
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct FeeTerms {
    rate_bps: u16,
    cap_bps: u16,
    treasury: AccountId,
}

impl FeeTerms {
    /// Explicit inputs only. This validates arithmetic, not governance approval.
    pub fn new(rate: Option<u16>, cap: Option<u16>, treasury: Option<AccountId>) -> Result<Self> {
        let (rate_bps, cap_bps, treasury) = (
            rate.ok_or(Error::MissingConfiguration)?,
            cap.ok_or(Error::MissingConfiguration)?,
            treasury.ok_or(Error::MissingConfiguration)?,
        );
        if cap_bps >= 10_000 || rate_bps > cap_bps {
            return Err(Error::InvalidFee);
        }
        Ok(Self {
            rate_bps,
            cap_bps,
            treasury,
        })
    }
    pub fn treasury(&self) -> &AccountId {
        &self.treasury
    }
    pub fn fee(&self, price: u128) -> Result<u128> {
        if price == 0 {
            return Err(Error::InvalidPrice);
        }
        if self.cap_bps >= 10_000 || self.rate_bps > self.cap_bps {
            return Err(Error::InvalidFee);
        }
        let rate = u128::from(self.rate_bps);
        // Quotient/remainder avoids overflowing P * bps for valid u128 prices.
        Ok((price / 10_000) * rate + (price % 10_000) * rate / 10_000)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Attempt {
    pub nonce: u64,
    pub operation_hash: [u8; 32],
}

impl Attempt {
    pub(crate) fn next(previous: u64, operation_hash: [u8; 32]) -> Result<Self> {
        if operation_hash == [0; 32] {
            return Err(Error::InvalidOperation);
        }
        Ok(Self {
            nonce: previous.checked_add(1).ok_or(Error::Overflow)?,
            operation_hash,
        })
    }
}

/// This is not receipt evidence. Only an authenticated future private callback
/// may supply a conclusive result. There is no recovery/attestation resolver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackResult {
    Succeeded,
    Failed,
    Unknown,
}
