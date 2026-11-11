//! Authenticated verification machinery. Public intake remains unconditionally
//! gated. The admission-only owner can reserve its own prepaid storage credits.
mod admission;
pub(crate) mod authority;
mod lifecycle;
mod wire;
pub(crate) use lifecycle::{create as create_admission, release as release_admission};
pub use lifecycle::{AdmissionState, AdmissionStatus};

use near_marketplace_foundation::{
    persistence::{
        AtomicStore, DeploymentDomain, OfflineCommand, OperationContext, RecordKey, Repository,
        StorageRepository,
    },
    state::{ListingState, TransferState},
    types::{Generation, SaleId},
};
use near_sdk::{env, json_types::U64, near, AccountId, Gas, GasWeight, NearToken, Promise};

use crate::{storage_accounting::apply_reserved_storage, HostStore};
use admission::Admission;

const MAX_MESSAGE_BYTES: usize = 4096;

#[derive(Debug, PartialEq, Eq)]
pub enum IntegrationError {
    Unauthorized,
    AttachedValue,
    Message,
    MissingAdmission,
    Admission,
    Storage,
    Gas,
    Context,
    ResultCount,
    Foundation(near_marketplace_foundation::persistence::StorageError),
}
type Result<T> = std::result::Result<T, IntegrationError>;
impl From<near_marketplace_foundation::persistence::StorageError> for IntegrationError {
    fn from(value: near_marketplace_foundation::persistence::StorageError) -> Self {
        Self::Foundation(value)
    }
}

#[near(serializers = [json])]
#[derive(Debug, PartialEq, Eq)]
pub struct PendingVerification {
    pub generation: U64,
    pub attempt_nonce: U64,
}

#[near(serializers = [json])]
#[derive(Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationOutcome {
    Active,
    Invalid,
    Ignored,
}

pub(crate) fn authenticate_nft(domain: &DeploymentDomain) -> Result<()> {
    if !env::attached_deposit().is_zero() {
        return Err(IntegrationError::AttachedValue);
    }
    if env::predecessor_account_id() != domain.nft {
        return Err(IntegrationError::Unauthorized);
    }
    Ok(())
}

/// Controlled dispatch uses only a locator for an existing owner-created grant.
/// No NFT identity, terms, generation, attempt or result comes from this request.
pub(crate) fn dispatch(domain: &DeploymentDomain, admission_id: &[u8]) -> Result<AdmissionStatus> {
    let owner = authority::authenticate(domain)?;
    let grant = Admission::load_id(domain, admission_id)?;
    if grant.storage_owner != owner {
        return Err(IntegrationError::Unauthorized);
    }
    let mut life =
        lifecycle::Lifecycle::load(domain, &grant)?.ok_or(IntegrationError::Admission)?; // no legacy dispatch adoption
    if !matches!(
        life.state,
        AdmissionState::Ready | AdmissionState::Dispatched
    ) {
        return Err(IntegrationError::Context);
    }
    lifecycle::check_pending(domain, &grant)?;
    // A retry observes the existing attempt, even after expiry or during a hold.
    // It never resends or interprets an absent callback as a failed operation.
    if life.state == AdmissionState::Dispatched {
        return life.status(&grant);
    }
    life.state = AdmissionState::Dispatched;
    let status = life.status(&grant)?;
    begin_admission(domain, &grant)?;
    Ok(status)
}

// Public hook remains gated. This internal path authenticates host identity;
// admissions authorize no NFT movement and tests cannot inject runtime evidence.
pub(crate) fn begin(
    domain: &DeploymentDomain,
    token_id: String,
    owner_id: AccountId,
    approval_id: u64,
    msg: &str,
) -> Result<PendingVerification> {
    authenticate_nft(domain)?;
    if env::promise_results_count() != 0 {
        return Err(IntegrationError::Context);
    }
    let message = wire::message(msg, MAX_MESSAGE_BYTES)?;
    let grant = Admission::load(domain, &token_id, &owner_id, approval_id)?;
    let (_, terms) = grant
        .operation
        .verification()
        .ok_or(IntegrationError::Context)?;
    if terms.token.token() != token_id
        || terms.seller != owner_id
        || terms.approval_id != approval_id
        || terms.price != message.price.0
        || terms.expires_at_ns != message.expires_at_ns.0
        || terms.policy_version != message.policy_version.0
    {
        return Err(IntegrationError::Admission);
    }
    begin_admission(domain, &grant)
}

/// Shared scheduling boundary reached only after owner or NFT authentication.
/// The gated hook retains its legacy codec path; controlled dispatch requires a
/// managed lifecycle before entering here. Every send derives from persisted data.
fn begin_admission(domain: &DeploymentDomain, grant: &Admission) -> Result<PendingVerification> {
    let (_, terms) = grant
        .operation
        .verification()
        .ok_or(IntegrationError::Context)?;
    let budgets = grant.operation.budgets();
    let repo = Repository::open(HostStore, domain.clone())?;
    let mut life = lifecycle::Lifecycle::load(domain, grant)?;
    let prepared = if let Some(managed) = &life {
        if managed.state != AdmissionState::Ready || env::block_timestamp() >= terms.expires_at_ns {
            return Err(IntegrationError::Context);
        }
        lifecycle::check_pending(domain, grant)?;
        grant.check_reservation(domain, 1, 0)?;
        None
    } else {
        // Preserve old admission encoding/semantics, but never infer release
        // safety for legacy records with no authenticated lifecycle history.
        let prepared = repo.prepare(OfflineCommand::Listing {
            terms: terms.clone(),
            now_ns: env::block_timestamp(),
            budgets,
        })?;
        if prepared.operation() != Some(&grant.operation) {
            return Err(IntegrationError::Context);
        }
        grant.check_reservation(domain, 0, grant.creation_bytes)?;
        Some(prepared)
    };
    let calls = wire::verification_calls(&grant.operation)?;
    let generation = grant.operation.verification().unwrap().0;
    let nonce = 1;
    let callback = wire::callback_args(generation.value(), nonce)?;
    let reservation = grant.reservation()?;
    let (protection, head) = StorageRepository::open(HostStore, domain.clone())?.snapshot()?;
    if head.financial_hold() || protection.financial_hold() {
        return Err(IntegrationError::Storage);
    }
    head.check_backing(protection, crate::storage_accounting::observe())
        .map_err(|_| IntegrationError::Storage)?;
    let required = budgets
        .first_call_gas
        .checked_add(budgets.second_call_gas)
        .and_then(|v| v.checked_add(budgets.callback_gas))
        .and_then(|v| v.checked_add(grant.local_gas))
        .and_then(|v| v.checked_add(env::used_gas().as_gas()))
        .ok_or(IntegrationError::Gas)?;
    if required > env::prepaid_gas().as_gas() {
        return Err(IntegrationError::Gas);
    }
    // Everything returning an ordinary Err is above this point. Any later failure
    // aborts the entire receipt, including writes and any constructed promises.
    let balance = env::account_balance();
    let usage = env::storage_usage();
    if let Some(prepared) = prepared {
        apply_reserved_storage(domain, reservation, 1, prepared.batch());
    } else {
        let managed = life.as_mut().unwrap();
        managed.state = AdmissionState::Dispatched;
        crate::storage_accounting::meter_storage(domain, reservation, 2, false, || {
            managed.write(grant)
        });
        if env::storage_usage() != usage {
            env::panic_str("Dispatch cannot allocate storage");
        }
    }
    if env::storage_usage()
        .checked_sub(usage)
        .is_none_or(|n| n > grant.creation_bytes)
    {
        env::panic_str("Listing storage exceeds authorized budget");
    }
    // Zero-value NFT views only. Eagerly drop the complete graph before the final
    // backing check; do not leave scheduling to a returned Promise destructor.
    let [token, approval] = calls;
    token
        .schedule()
        .and(approval.schedule())
        .then(
            Promise::new(domain.marketplace.clone()).function_call_weight(
                "on_nft_verified",
                callback,
                NearToken::ZERO,
                Gas::from_gas(budgets.callback_gas),
                GasWeight(0),
            ),
        )
        .detach();
    if env::account_balance() != balance {
        env::panic_str("Unexpected verification balance debit");
    }
    crate::storage_accounting::assert_storage_backing(domain);
    Ok(PendingVerification {
        generation: U64(generation.value()),
        attempt_nonce: U64(nonce),
    })
}

pub(crate) fn complete(
    domain: &DeploymentDomain,
    generation: u64,
    nonce: u64,
) -> Result<VerificationOutcome> {
    if env::predecessor_account_id() != env::current_account_id()
        || env::current_account_id() != domain.marketplace
    {
        return Err(IntegrationError::Unauthorized);
    }
    if !env::attached_deposit().is_zero() {
        return Err(IntegrationError::AttachedValue);
    }
    let generation = Generation::from_value(generation).map_err(|_| IntegrationError::Context)?;
    let repo = Repository::open(HostStore, domain.clone())?;
    let Some(record) = repo.read(&RecordKey::Listing(generation))? else {
        return Ok(VerificationOutcome::Ignored);
    };
    let listing = record.listing().ok_or(IntegrationError::Context)?;
    // In particular, Locked/Review cannot be settled, refunded or unlocked here.
    if listing.state() != ListingState::Verifying {
        return Ok(VerificationOutcome::Ignored);
    }
    if nonce != listing.verification().nonce {
        return Err(IntegrationError::Context);
    }
    let record = repo
        .read(&RecordKey::Verification(generation))?
        .ok_or(IntegrationError::Context)?;
    let operation = record.operation().ok_or(IntegrationError::Context)?;
    if !record.operation_pending()
        || *operation != OperationContext::for_listing(domain, listing, operation.budgets())?
        || operation.commitment()? != listing.verification().operation_hash
        || !repo
            .read(&RecordKey::Token(listing.terms().token.clone()))?
            .is_some_and(|r| r.token_verification_matches(generation, listing.terms().approval_id))
    {
        return Err(IntegrationError::Context);
    }
    wire::verification_calls(operation)?; // Pins target/method/ABI/budgets again.
    if env::promise_results_count() != operation.result_count() {
        return Err(IntegrationError::ResultCount);
    }
    let terms = listing.terms();
    let grant = Admission::load(
        domain,
        terms.token.token(),
        &terms.seller,
        terms.approval_id,
    )?;
    if grant.operation != *operation {
        return Err(IntegrationError::Context);
    }
    let mut life = lifecycle::Lifecycle::load(domain, &grant)?;
    if life
        .as_ref()
        .is_some_and(|l| l.state != AdmissionState::Dispatched)
    {
        return Err(IntegrationError::Context);
    }
    grant.check_reservation(domain, if life.is_some() { 2 } else { 1 }, 0)?;
    let reservation = grant.reservation()?;
    // Only these host reads supply observations. Count and context checks happen
    // before any read; missing callbacks leave the original attempt pending.
    let bound = operation.budgets().max_result_bytes as usize;
    let owner_result = env::promise_result_checked(0, bound);
    let approval_result = env::promise_result_checked(1, 5);
    let verified = owner_result
        .as_ref()
        .ok()
        .zip(approval_result.as_ref().ok())
        .is_some_and(|(token, approval)| wire::verify_views(token, approval, terms, domain));
    let prepared = repo.prepare(OfflineCommand::Verify {
        generation,
        attempt: listing.verification(),
        owner: verified.then(|| terms.seller.clone()),
        approval: verified.then_some(terms.approval_id),
        now_ns: env::block_timestamp(),
    })?;
    // These transitions only change fixed-width state/revision fields. Refuse a
    // future encoding change that would silently need callback storage funding.
    for (key, bytes) in prepared.batch().writes() {
        if HostStore
            .read(key)?
            .is_none_or(|old| old.len() != bytes.len())
        {
            return Err(IntegrationError::Storage);
        }
    }
    let result = if verified && env::block_timestamp() < terms.expires_at_ns {
        VerificationOutcome::Active
    } else {
        VerificationOutcome::Invalid
    };
    if let Some(managed) = life.as_mut() {
        managed.state = AdmissionState::Completed;
        let usage = env::storage_usage();
        crate::storage_accounting::meter_storage(domain, reservation, 3, false, || {
            HostStore
                .apply(prepared.batch())
                .unwrap_or_else(|_| env::panic_str("Verification write failed"));
            managed.write(&grant);
        });
        if env::storage_usage() != usage {
            env::panic_str("Callback cannot allocate storage");
        }
    } else {
        apply_reserved_storage(domain, reservation, 2, prepared.batch());
    }
    Ok(result)
}

/// Prepared exact NFT call, not a send capability, callback or receipt proof.
#[derive(Debug)]
pub struct ApprovedTransferCall {
    pub target: AccountId,
    pub method: &'static str,
    pub args: Vec<u8>,
    pub attached_yocto: u128,
    pub gas: u64,
    pub operation: OperationContext,
}

/// Reads an already persisted sale and attempt; no sale/policy/receipt fields
/// come from a public request. Purchase entry points and dispatch remain absent.
pub fn prepare_purchase_call(
    domain: &DeploymentDomain,
    sale: SaleId,
) -> Result<ApprovedTransferCall> {
    let repo = Repository::open(HostStore, domain.clone())?;
    let record = repo
        .read(&RecordKey::Sale(sale))?
        .ok_or(IntegrationError::Context)?;
    let sale = record.sale().ok_or(IntegrationError::Context)?;
    if sale.outcome() != TransferState::Pending {
        return Err(IntegrationError::Context);
    }
    let record = repo
        .read(&RecordKey::Transfer(sale.id()))?
        .ok_or(IntegrationError::Context)?;
    let op = record.operation().ok_or(IntegrationError::Context)?;
    if record.operation_terminal()
        || *op != OperationContext::for_sale(domain, sale, op.budgets())?
        || op.commitment()? != sale.transfer().operation_hash
    {
        return Err(IntegrationError::Context);
    }
    wire::approved_transfer_call(op)
}

#[cfg(test)]
mod tests;
