//! Immutable, admission-only role installed during fresh self-bootstrap.
use super::{wire, IntegrationError, Result};
use crate::host_store::read_bounded;
use near_marketplace_foundation::persistence::DeploymentDomain;
use near_sdk::{env, AccountId};

pub(crate) const OWNER_KEY: &[u8] = b"nmp:runtime:admission-owner:v1";
const MAGIC: &[u8] = b"NMPAOWN1";

pub(crate) fn encode(domain: &DeploymentDomain, owner: &AccountId) -> Result<Vec<u8>> {
    if owner == &domain.marketplace || owner == &domain.nft {
        return Err(IntegrationError::Unauthorized);
    }
    let mut bytes = MAGIC.to_vec();
    bytes.extend(domain.commitment()?);
    bytes.extend(borsh::to_vec(owner).map_err(|_| IntegrationError::Admission)?);
    bytes.extend(wire::hash(&bytes));
    Ok(bytes)
}

pub(super) fn authenticate(domain: &DeploymentDomain) -> Result<AccountId> {
    if !env::attached_deposit().is_zero() {
        return Err(IntegrationError::AttachedValue);
    }
    authenticate_identity(domain)
}

pub(crate) fn authenticate_identity(domain: &DeploymentDomain) -> Result<AccountId> {
    if env::promise_results_count() != 0 {
        return Err(IntegrationError::Context);
    }
    let bytes = read_bounded(OWNER_KEY).ok_or(IntegrationError::Unauthorized)?;
    if !(78..=140).contains(&bytes.len()) || !bytes.starts_with(MAGIC) {
        return Err(IntegrationError::Admission);
    }
    // Length check precedes Borsh allocation. Re-encoding verifies domain,
    // checksum, exact length, role exclusions and canonical account syntax.
    let n = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
    if !(2..=64).contains(&n) || bytes.len() != 76 + n {
        return Err(IntegrationError::Admission);
    }
    let owner: AccountId =
        borsh::from_slice(&bytes[40..44 + n]).map_err(|_| IntegrationError::Admission)?;
    if encode(domain, &owner)? != bytes {
        return Err(IntegrationError::Admission);
    }
    // The signer is never a substitute for the immediate authenticated caller.
    if env::predecessor_account_id() != owner {
        return Err(IntegrationError::Unauthorized);
    }
    Ok(owner)
}
