//! Admission-only owner authority, prepaid storage and retained replay barriers.
//! No sale authorization, external evidence, promise, or transfer entry point.
use super::{admission::Admission, authority, wire, IntegrationError, Result};
use crate::{host_store::read_bounded, storage_accounting, HostStore};
use near_marketplace_foundation::{
    persistence::{
        AtomicStore, DeploymentDomain, OfflineCommand, OperationContext, RecordKey, Repository,
        StorageRepository,
    },
    state::ListingState,
    storage_accounting::StorageCommand,
};
use near_sdk::{env, json_types::U64, near, AccountId};

#[near(serializers = [json])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionState {
    Ready,
    Dispatched,
    Completed,
    Released,
}

#[near(serializers = [json])]
#[derive(Debug, PartialEq, Eq)]
pub struct AdmissionStatus {
    pub generation: U64,
    pub attempt_nonce: U64,
    pub state: AdmissionState,
}

#[derive(Clone, Debug)]
pub(super) struct Lifecycle {
    pub state: AdmissionState,
    pub amount: u128,
}
impl Lifecycle {
    fn key(grant: &Admission) -> Result<Vec<u8>> {
        let mut key = b"nmp:runtime:admission-life:v1:".to_vec();
        key.extend(grant.reservation()?);
        Ok(key)
    }
    fn encode(&self, grant: &Admission) -> Result<Vec<u8>> {
        let mut bytes = b"NMPALIF1".to_vec();
        bytes.extend(grant.operation.commitment()?);
        bytes.push(match self.state {
            AdmissionState::Ready => 0,
            AdmissionState::Dispatched => 1,
            AdmissionState::Completed => 2,
            AdmissionState::Released => 3,
        });
        bytes.extend(self.amount.to_le_bytes());
        bytes.extend(wire::hash(&bytes));
        Ok(bytes)
    }
    pub fn load(domain: &DeploymentDomain, grant: &Admission) -> Result<Option<Self>> {
        let bytes = read_bounded(&Self::key(grant)?);
        let guard = StorageRepository::open(HostStore, domain.clone())?
            .admission_guard(grant.reservation()?)?;
        let Some(bytes) = bytes else {
            // Never downgrade a missing/corrupt managed lifecycle to legacy.
            return if guard.is_none() {
                Ok(None)
            } else {
                Err(IntegrationError::Admission)
            };
        };
        if bytes.len() != 89 || !bytes.starts_with(b"NMPALIF1") {
            return Err(IntegrationError::Admission);
        }
        let state = match bytes[40] {
            0 => AdmissionState::Ready,
            1 => AdmissionState::Dispatched,
            2 => AdmissionState::Completed,
            3 => AdmissionState::Released,
            _ => return Err(IntegrationError::Admission),
        };
        let value = Self {
            state,
            amount: u128::from_le_bytes(bytes[41..57].try_into().unwrap()),
        };
        if value.amount == 0
            || value.encode(grant)? != bytes
            || guard
                != Some((
                    grant.operation.commitment()?,
                    state == AdmissionState::Released,
                ))
        {
            return Err(IntegrationError::Admission);
        }
        if state != AdmissionState::Released {
            grant.check_reservation(
                domain,
                match state {
                    AdmissionState::Ready => 1,
                    AdmissionState::Dispatched => 2,
                    AdmissionState::Completed => 3,
                    AdmissionState::Released => unreachable!(),
                },
                0,
            )?;
        }
        Ok(Some(value))
    }
    pub fn write(&self, grant: &Admission) {
        let key = Self::key(grant).unwrap_or_else(|_| env::panic_str("Invalid lifecycle key"));
        let bytes = self
            .encode(grant)
            .unwrap_or_else(|_| env::panic_str("Invalid lifecycle"));
        env::storage_write(&key, &bytes);
    }
    pub(super) fn status(&self, grant: &Admission) -> Result<AdmissionStatus> {
        Ok(AdmissionStatus {
            generation: U64(grant
                .operation
                .verification()
                .ok_or(IntegrationError::Context)?
                .0
                .value()),
            attempt_nonce: U64(1),
            state: self.state,
        })
    }
}

/// Recheck exact linked records without scanning history. Verifying codec
/// invariants exclude any sale; the token slot must still name this generation.
pub(super) fn check_pending(domain: &DeploymentDomain, grant: &Admission) -> Result<()> {
    let (generation, terms) = grant
        .operation
        .verification()
        .ok_or(IntegrationError::Context)?;
    let repo = Repository::open(HostStore, domain.clone())?;
    let record = repo
        .read(&RecordKey::Listing(generation))?
        .ok_or(IntegrationError::Context)?;
    let listing = record.listing().ok_or(IntegrationError::Context)?;
    let record = repo
        .read(&RecordKey::Verification(generation))?
        .ok_or(IntegrationError::Context)?;
    if listing.state() != ListingState::Verifying
        || listing.verification().nonce != 1
        || OperationContext::for_listing(domain, listing, grant.operation.budgets())?
            != grant.operation
        || record.operation() != Some(&grant.operation)
        || !record.operation_pending()
        || listing.verification().operation_hash != grant.operation.commitment()?
        || !repo
            .read(&RecordKey::Token(terms.token.clone()))?
            .is_some_and(|r| r.token_verification_matches(generation, terms.approval_id))
    {
        return Err(IntegrationError::Context);
    }
    Ok(())
}

pub(crate) fn create(
    domain: &DeploymentDomain,
    operation: &[u8],
    storage_owner: AccountId,
    amount: u128,
    creation_bytes: u64,
    local_gas: u64,
) -> Result<AdmissionStatus> {
    let owner = authority::authenticate(domain)?;
    // Narrow consent: only the authenticated owner's own existing credit. No
    // authority over seller/third-party funds and no default payer policy.
    if storage_owner != owner {
        return Err(IntegrationError::Unauthorized);
    }
    if operation.len() > 4096 || creation_bytes == 0 || local_gas == 0 {
        return Err(IntegrationError::Admission);
    }
    let operation = OperationContext::from_canonical_bytes(operation, domain)?;
    wire::verification_calls(&operation)?;
    let grant = Admission {
        operation,
        storage_owner,
        creation_bytes,
        local_gas,
    };
    let encoded = grant.encode();
    Admission::decode(&encoded, domain)?;
    let (_, terms) = grant
        .operation
        .verification()
        .ok_or(IntegrationError::Context)?;
    let key = Admission::key(
        domain,
        terms.token.token(),
        &terms.seller,
        terms.approval_id,
    )?;
    if let Some(old) = read_bounded(&key) {
        if old != encoded {
            return Err(IntegrationError::Admission);
        }
        let life = Lifecycle::load(domain, &grant)?.ok_or(IntegrationError::Admission)?;
        if life.amount != amount {
            return Err(IntegrationError::Admission);
        }
        // Exact retries return the retained status, even after release. Never
        // recreate records or charge again, even if the token has moved on.
        return life.status(&grant);
    }
    if read_bounded(&Lifecycle::key(&grant)?).is_some() {
        return Err(IntegrationError::Admission);
    }
    let budget = u128::from(creation_bytes)
        .checked_mul(env::storage_byte_cost().as_yoctonear())
        .ok_or(IntegrationError::Storage)?;
    if amount < budget {
        return Err(IntegrationError::Storage);
    }
    let command = OfflineCommand::Listing {
        terms: terms.clone(),
        now_ns: env::block_timestamp(),
        budgets: grant.operation.budgets(),
    };
    let prepared = Repository::open(HostStore, domain.clone())?.prepare(command.clone())?;
    if prepared.operation() != Some(&grant.operation) {
        return Err(IntegrationError::Context);
    }
    let reservation = grant.reservation()?;
    let reserve = StorageRepository::open(HostStore, domain.clone())?.prepare(
        StorageCommand::ReserveAdmission {
            reservation,
            operation: grant.operation.commitment()?,
            owner,
            amount,
            balance: storage_accounting::observe(),
        },
    )?;
    let life = Lifecycle {
        state: AdmissionState::Ready,
        amount,
    };
    let status = life.status(&grant)?;
    let usage = env::storage_usage();
    // From here every failure aborts. Receipt atomicity, not a Rust Result,
    // couples reservation allocation, generation allocation and measured cost.
    storage_accounting::meter_storage(domain, reservation, 1, true, || {
        HostStore
            .apply(&reserve)
            .unwrap_or_else(|_| env::panic_str("Admission reserve write failed"));
        // Reserve advanced STATE's revision but not its generation. Reprepare
        // against that exact root, preserving all existing record validations.
        let mut repo = Repository::open(HostStore, domain.clone()).unwrap();
        let prepared = repo
            .prepare(command)
            .unwrap_or_else(|_| env::panic_str("Admission generation changed"));
        if prepared.operation() != Some(&grant.operation) {
            env::panic_str("Admission identity changed");
        }
        repo.commit(&prepared)
            .unwrap_or_else(|_| env::panic_str("Admission listing write failed"));
        env::storage_write(&key, &encoded);
        life.write(&grant);
        if env::storage_usage()
            .checked_sub(usage)
            .is_none_or(|n| n > creation_bytes)
        {
            env::panic_str("Admission storage exceeds authorized budget");
        }
    });
    Ok(status)
}

pub(crate) fn release(
    domain: &DeploymentDomain,
    token: &str,
    seller: &AccountId,
    approval: u64,
    generation: u64,
) -> Result<AdmissionStatus> {
    let owner = authority::authenticate(domain)?;
    let grant = Admission::load(domain, token, seller, approval)?;
    if grant.storage_owner != owner
        || grant
            .operation
            .verification()
            .ok_or(IntegrationError::Context)?
            .0
            .value()
            != generation
    {
        return Err(IntegrationError::Unauthorized);
    }
    let mut life = Lifecycle::load(domain, &grant)?.ok_or(IntegrationError::Admission)?;
    if life.state == AdmissionState::Released {
        return life.status(&grant);
    }
    if life.state != AdmissionState::Ready {
        return Err(IntegrationError::Context);
    }
    check_pending(domain, &grant)?;
    grant.check_reservation(domain, 1, 0)?;
    let (id, terms) = grant.operation.verification().unwrap();
    let mut repo = Repository::open(HostStore, domain.clone())?;
    let cancel = repo.prepare(OfflineCommand::Cancel {
        generation: id,
        seller: terms.seller.clone(),
    })?;
    let command = StorageCommand::ReleaseAdmission {
        reservation: grant.reservation()?,
        operation: grant.operation.commitment()?,
        balance: storage_accounting::observe(),
    };
    let storage = StorageRepository::open(HostStore, domain.clone())?;
    let release = storage.prepare(command.clone())?;
    for batch in [cancel.batch(), &release] {
        for (key, value) in batch.writes() {
            if HostStore
                .read(key)?
                .is_none_or(|old| old.len() != value.len())
            {
                return Err(IntegrationError::Storage);
            }
        }
    }
    let protection = storage.snapshot()?.0;
    let usage = env::storage_usage();
    life.state = AdmissionState::Released;
    let status = life.status(&grant)?;
    repo.commit(&cancel)
        .unwrap_or_else(|_| env::panic_str("Admission cancellation failed"));
    let mut storage = StorageRepository::open(HostStore, domain.clone()).unwrap();
    let release = storage
        .prepare(command)
        .unwrap_or_else(|_| env::panic_str("Admission release failed"));
    storage
        .commit(&release)
        .unwrap_or_else(|_| env::panic_str("Admission release write failed"));
    life.write(&grant);
    if env::storage_usage() != usage || storage.snapshot().unwrap().0 != protection {
        env::panic_str("Admission release changed protected state or storage size");
    }
    storage_accounting::assert_storage_backing(domain);
    Ok(status)
}
