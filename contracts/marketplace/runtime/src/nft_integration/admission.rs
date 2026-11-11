//! Frozen v1 admission encoding; lifecycle and owner authority are additive.
use super::{wire, IntegrationError, Result};
use crate::{host_store::read_bounded, HostStore};
use near_marketplace_foundation::persistence::{
    DeploymentDomain, OperationContext, StorageRepository,
};
use near_sdk::{env, AccountId};

const MAGIC: &[u8] = b"NMPADM01";
const TAG: &[u8] = b"nmp:nft-admission:v1\0";
const PREFIX: &[u8] = b"nmp:runtime:nft-admission:v1:";
pub(super) struct Admission {
    pub operation: OperationContext,
    pub storage_owner: AccountId,
    pub creation_bytes: u64,
    pub local_gas: u64,
}
impl Admission {
    pub fn key(
        domain: &DeploymentDomain,
        token: &str,
        seller: &AccountId,
        approval: u64,
    ) -> Result<Vec<u8>> {
        if token.trim().is_empty() || token.len() > 1024 {
            return Err(IntegrationError::Message);
        }
        let mut payload = TAG.to_vec();
        payload.extend(domain.commitment()?);
        payload.extend(
            borsh::to_vec(&(token, seller, approval)).map_err(|_| IntegrationError::Admission)?,
        );
        let mut key = PREFIX.to_vec();
        key.extend(wire::hash(&payload));
        Ok(key)
    }
    pub fn reservation(&self) -> Result<[u8; 32]> {
        let mut bytes = b"nmp:listing-storage:v1\0".to_vec();
        bytes.extend(self.operation.commitment()?);
        Ok(wire::hash(&bytes))
    }
    pub fn load(
        domain: &DeploymentDomain,
        token: &str,
        seller: &AccountId,
        approval: u64,
    ) -> Result<Self> {
        let key = Self::key(domain, token, seller, approval)?;
        Self::load_key(domain, &key)
    }
    /// A 32-byte locator, never an authorization or receipt hash. Recompute the
    /// original domain-bound key from the decoded record before returning it.
    pub fn load_id(domain: &DeploymentDomain, id: &[u8]) -> Result<Self> {
        if id.len() != 32 {
            return Err(IntegrationError::Admission);
        }
        let mut key = PREFIX.to_vec();
        key.extend_from_slice(id);
        Self::load_key(domain, &key)
    }
    fn load_key(domain: &DeploymentDomain, key: &[u8]) -> Result<Self> {
        let bytes = read_bounded(key).ok_or(IntegrationError::MissingAdmission)?;
        let value = Self::decode(&bytes, domain)?;
        let (_, terms) = value
            .operation
            .verification()
            .ok_or(IntegrationError::Admission)?;
        if Self::key(
            domain,
            terms.token.token(),
            &terms.seller,
            terms.approval_id,
        )? != key
        {
            return Err(IntegrationError::Admission);
        }
        Ok(value)
    }
    pub(super) fn decode(bytes: &[u8], domain: &DeploymentDomain) -> Result<Self> {
        if bytes.len() > 4096 || bytes.len() < 60 {
            return Err(IntegrationError::Admission);
        }
        let (data, checksum) = bytes.split_at(bytes.len() - 32);
        let mut tagged = TAG.to_vec();
        tagged.extend(data);
        if wire::hash(&tagged) != checksum || !data.starts_with(MAGIC) {
            return Err(IntegrationError::Admission);
        }
        let mut cursor = &data[MAGIC.len()..];
        fn take<'a>(cursor: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
            let bytes = cursor.get(..n).ok_or(IntegrationError::Admission)?;
            *cursor = &cursor[n..];
            Ok(bytes)
        }
        let length = u32::from_le_bytes(take(&mut cursor, 4)?.try_into().unwrap()) as usize;
        let operation = OperationContext::from_canonical_bytes(take(&mut cursor, length)?, domain)?;
        wire::verification_calls(&operation)?;
        let length = u32::from_le_bytes(take(&mut cursor, 4)?.try_into().unwrap()) as usize;
        if !(2..=64).contains(&length) {
            return Err(IntegrationError::Admission);
        }
        let storage_owner = std::str::from_utf8(take(&mut cursor, length)?)
            .map_err(|_| IntegrationError::Admission)?
            .parse::<AccountId>()
            .map_err(|_| IntegrationError::Admission)?;
        let creation_bytes = u64::from_le_bytes(take(&mut cursor, 8)?.try_into().unwrap());
        let local_gas = u64::from_le_bytes(take(&mut cursor, 8)?.try_into().unwrap());
        if !cursor.is_empty()
            || creation_bytes == 0
            || local_gas == 0
            || storage_owner == domain.marketplace
        {
            return Err(IntegrationError::Admission);
        }
        Ok(Self {
            operation,
            storage_owner,
            creation_bytes,
            local_gas,
        })
    }
    pub fn check_reservation(
        &self,
        domain: &DeploymentDomain,
        nonce: u64,
        bytes: u64,
    ) -> Result<()> {
        let (owner, remaining, cursor) = StorageRepository::open(HostStore, domain.clone())?
            .reservation_status(self.reservation()?)?;
        let required = u128::from(bytes)
            .checked_mul(env::storage_byte_cost().as_yoctonear())
            .ok_or(IntegrationError::Storage)?;
        if owner != self.storage_owner || cursor != nonce || remaining < required {
            return Err(IntegrationError::Storage);
        }
        Ok(())
    }
    pub fn encode(&self) -> Vec<u8> {
        let op = self.operation.canonical_bytes().unwrap();
        let mut bytes = MAGIC.to_vec();
        bytes.extend((op.len() as u32).to_le_bytes());
        bytes.extend(op);
        bytes.extend(borsh::to_vec(&self.storage_owner).unwrap());
        bytes.extend(self.creation_bytes.to_le_bytes());
        bytes.extend(self.local_gas.to_le_bytes());
        let mut tagged = TAG.to_vec();
        tagged.extend(&bytes);
        bytes.extend(wire::hash(&tagged));
        bytes
    }
}
