//! Disabled NEAR runtime with authenticated NFT verification internals.
//! Owner-only prepaid admission lifecycle; no public intake, trading or payments.
#![deny(unsafe_code)]

#[cfg(all(target_arch = "wasm32", not(near)))]
compile_error!("NEAR Wasm requires --cfg near; use runtime/build.sh");

#[cfg(not(any(feature = "testnet", feature = "production")))]
compile_error!("select exactly one runtime artifact: testnet or production");
#[cfg(all(feature = "testnet", feature = "production"))]
compile_error!("testnet and production artifacts must be built separately");
#[cfg(all(feature = "test-hooks", feature = "production"))]
compile_error!("test hooks must never be compiled into production");
#[cfg(all(feature = "vm-tests", target_arch = "wasm32"))]
compile_error!("VM harness is native-only");

mod host_store;
pub mod nft_integration;
pub mod storage_accounting;
#[cfg(any(test, feature = "test-hooks"))]
mod test_storage_fixture;
#[cfg(feature = "testnet")]
mod testnet_funding;
pub use host_store::HostStore;

use near_marketplace_foundation::persistence::{
    bootstrap_batch, bootstrap_storage_batch, decode_record, ArtifactDomain, AtomicStore,
    DeploymentDomain, RecordKey,
};
use near_sdk::{
    env,
    json_types::{Base64VecU8, U128, U64},
    near, AccountContract, AccountId,
};

pub const BOOTSTRAP_KEY: &[u8] = b"nmp:runtime:bootstrap:v1";
const BOOTSTRAP_MAGIC: &[u8] = b"NMPBOOT1";
const MAX_MANIFEST_BYTES: usize = 265;

// A marker only: no method receives self and no method uses #[init]. Therefore
// the SDK never reads/writes its Borsh representation over our STATE envelope.
#[near(contract_state)]
pub struct Runtime;

#[near(serializers = [json])]
pub struct StorageStatus {
    pub initialized: bool,
    pub trading_enabled: bool,
    pub root_revision: U64,
    pub root_bytes: u32,
    pub root_hash: Base64VecU8,
    pub deployment_commitment: Base64VecU8,
}

#[near]
impl Runtime {
    /// An admission locator only. The owner cannot supply or override call terms
    /// or callback evidence. Repeated pending dispatch is a read-only status check.
    pub fn dispatch_nft_admission(admission_id: Base64VecU8) -> nft_integration::AdmissionStatus {
        let domain = runtime_domain();
        nft_integration::dispatch(&domain, &admission_id.0)
            .unwrap_or_else(|e| env::panic_str(&format!("Admission dispatch: {e:?}")))
    }
    /// Intent bytes are owner authorization, never runtime receipt evidence.
    pub fn create_nft_admission(
        operation: Base64VecU8,
        storage_owner_id: AccountId,
        reserve_amount: U128,
        creation_bytes: U64,
        local_gas: U64,
    ) -> nft_integration::AdmissionStatus {
        let domain = runtime_domain();
        nft_integration::create_admission(
            &domain,
            &operation.0,
            storage_owner_id,
            reserve_amount.0,
            creation_bytes.0,
            local_gas.0,
        )
        .unwrap_or_else(|e| env::panic_str(&format!("Admission: {e:?}")))
    }

    /// Releases only unused future credit, never retained stake or native NEAR.
    pub fn release_nft_admission(
        token_id: String,
        owner_id: AccountId,
        approval_id: U64,
        generation: U64,
    ) -> nft_integration::AdmissionStatus {
        let domain = runtime_domain();
        nft_integration::release_admission(
            &domain,
            &token_id,
            &owner_id,
            approval_id.0,
            generation.0,
        )
        .unwrap_or_else(|e| env::panic_str(&format!("Admission release: {e:?}")))
    }
    /// NEP-178 hook: approval_id remains a numeric u64, matching our NFT ABI.
    /// Both public trading gates stay closed, including for admitted candidates.
    pub fn nft_on_approve(
        token_id: String,
        owner_id: AccountId,
        approval_id: u64,
        msg: String,
    ) -> nft_integration::PendingVerification {
        let domain = runtime_domain();
        nft_integration::authenticate_nft(&domain)
            .unwrap_or_else(|e| env::panic_str(&format!("NFT integration: {e:?}")));
        near_marketplace_foundation::authorize_public_action(
            near_marketplace_foundation::PublicAction::List,
        )
        .unwrap_or_else(|_| env::panic_str("Listing actions remain disabled"));
        nft_integration::begin(&domain, token_id, owner_id, approval_id, &msg)
            .unwrap_or_else(|e| env::panic_str(&format!("NFT integration: {e:?}")))
    }

    /// No result/hash arguments: only the NEAR host supplies dependency results.
    #[private]
    pub fn on_nft_verified(
        generation: U64,
        attempt_nonce: U64,
    ) -> nft_integration::VerificationOutcome {
        let domain = runtime_domain();
        nft_integration::complete(&domain, generation.0, attempt_nonce.0)
            .unwrap_or_else(|e| env::panic_str(&format!("NFT integration: {e:?}")))
    }
    /// Self-bootstrap only, from an explicitly supplied deployment manifest.
    /// No governance identities or financial policy are selected by bootstrap.
    #[private]
    pub fn bootstrap(manifest: Base64VecU8) {
        initialize(&manifest.0, 0, None);
    }

    /// Fresh bootstrap only. This immutable role has no treasury, recovery,
    /// withdrawal, activation, policy-change or NFT-transfer authority.
    #[private]
    pub fn bootstrap_with_admission_owner(manifest: Base64VecU8, admission_owner_id: AccountId) {
        initialize(&manifest.0, 0, Some(admission_owner_id));
    }

    pub fn storage_status() -> StorageStatus {
        require_zero_deposit();
        let stored = host_store::read_bounded(BOOTSTRAP_KEY)
            .unwrap_or_else(|| env::panic_str("Bootstrap missing"));
        let manifest = stored
            .strip_prefix(BOOTSTRAP_MAGIC)
            .unwrap_or_else(|| env::panic_str("Invalid bootstrap schema"));
        let domain = decode_manifest(manifest);
        check_domain(&domain);
        let root =
            host_store::read_bounded(b"STATE").unwrap_or_else(|| env::panic_str("Root missing"));
        let decoded = decode_record(&root, &domain, &RecordKey::Root)
            .unwrap_or_else(|_| env::panic_str("Invalid root envelope"));
        StorageStatus {
            initialized: true,
            trading_enabled: false,
            root_revision: U64(decoded.revision()),
            root_bytes: root.len() as u32,
            root_hash: Base64VecU8(env::sha256(&root)),
            deployment_commitment: Base64VecU8(domain.commitment().unwrap().to_vec()),
        }
    }
}

fn runtime_domain() -> DeploymentDomain {
    require_zero_deposit();
    checked_runtime_domain()
}

fn checked_runtime_domain() -> DeploymentDomain {
    let stored = host_store::read_bounded(BOOTSTRAP_KEY)
        .unwrap_or_else(|| env::panic_str("Bootstrap missing"));
    let domain = decode_manifest(
        stored
            .strip_prefix(BOOTSTRAP_MAGIC)
            .unwrap_or_else(|| env::panic_str("Invalid bootstrap schema")),
    );
    check_domain(&domain);
    // Opening validates the existing versioned root, never the SDK marker type.
    near_marketplace_foundation::persistence::Repository::open(HostStore, domain.clone())
        .unwrap_or_else(|_| env::panic_str("Invalid runtime root"));
    domain
}

// Separate impl ensures the entire generated fault-export wrapper is cfg-gated.
#[cfg(feature = "test-hooks")]
#[near]
impl Runtime {
    /// Only in the explicitly named test artifact. No arbitrary key/value input.
    #[private]
    pub fn test_abort_bootstrap(manifest: Base64VecU8, stage: u8) {
        if !(1..=3).contains(&stage) {
            env::panic_str("Invalid fault stage");
        }
        initialize(&manifest.0, stage, None);
    }

    /// Test a post-write receipt abort using a real, unchanged v1 root write set.
    #[private]
    pub fn test_abort_after_existing_write() {
        require_bootstrap_caller();
        let status = Self::storage_status();
        if !status.initialized {
            env::panic_str("Bootstrap missing");
        }
        let root = host_store::read_bounded(b"STATE").unwrap();
        env::storage_write(b"STATE", &root);
        env::storage_write(b"nmp:runtime:test:rollback", b"must not persist");
        env::panic_str("Injected post-write abort");
    }

    /// Exercise backend precondition rejection, bypassing only the outer
    /// bootstrap existence guard. No external write-set input is accepted.
    #[private]
    pub fn test_conflicting_write_set() {
        require_bootstrap_caller();
        Self::storage_status();
        let stored = host_store::read_bounded(BOOTSTRAP_KEY).unwrap();
        let domain = decode_manifest(stored.strip_prefix(BOOTSTRAP_MAGIC).unwrap());
        let batch = bootstrap_batch(&domain).unwrap();
        HostStore.apply(&batch).unwrap();
        env::panic_str("Conflict incorrectly accepted");
    }

    #[private]
    pub fn test_metered_storage(underfunded: bool) -> near_sdk::json_types::U128 {
        require_bootstrap_caller();
        Self::storage_status();
        let stored = host_store::read_bounded(BOOTSTRAP_KEY).unwrap();
        let domain = decode_manifest(stored.strip_prefix(BOOTSTRAP_MAGIC).unwrap());
        near_sdk::json_types::U128(test_storage_fixture::exercise(&domain, underfunded))
    }
}

fn require_zero_deposit() {
    if !env::attached_deposit().is_zero() {
        env::panic_str("Attached deposit is not accepted");
    }
}

fn require_bootstrap_caller() {
    require_zero_deposit();
    let current = env::current_account_id();
    if env::predecessor_account_id() != current
        || env::signer_account_id() != current
        || env::promise_results_count() != 0
    {
        env::panic_str("Bootstrap requires a direct self call");
    }
}

fn check_domain(domain: &DeploymentDomain) {
    domain
        .commitment()
        .unwrap_or_else(|_| env::panic_str("Invalid deployment domain"));
    let expected = if cfg!(feature = "production") {
        ArtifactDomain::Production
    } else {
        ArtifactDomain::Testnet
    };
    if domain.environment != expected || domain.marketplace != env::current_account_id() {
        env::panic_str("Deployment identity mismatch");
    }
    // This SDK host API is feature-gated. Supported protocol execution must be
    // established separately before deployment; missing support fails closed.
    match env::current_contract_code() {
        AccountContract::Local(hash) if hash == domain.marketplace_code => {}
        _ => env::panic_str("Executing code hash mismatch"),
    }
    // NFT code, genesis and manifest provenance still require an independent
    // deployment verifier. Their supplied hashes are NOT evidence of truth.
}

fn decode_manifest(bytes: &[u8]) -> DeploymentDomain {
    if bytes.len() > MAX_MANIFEST_BYTES || bytes.len() < 137 {
        env::panic_str("Invalid manifest size");
    }
    // Borsh: environment byte, four hashes, then two AccountId strings.
    // Inspect both lengths before allowing derived Borsh to allocate them.
    let mut offset = 129;
    for _ in 0..2 {
        let length_bytes = bytes
            .get(offset..offset + 4)
            .unwrap_or_else(|| env::panic_str("Truncated manifest"));
        let length = u32::from_le_bytes(length_bytes.try_into().unwrap()) as usize;
        if !(2..=64).contains(&length) {
            env::panic_str("Invalid manifest account length");
        }
        offset += 4 + length;
        if offset > bytes.len() {
            env::panic_str("Truncated manifest account");
        }
    }
    if offset != bytes.len() {
        env::panic_str("Trailing manifest bytes");
    }
    borsh::from_slice(bytes).unwrap_or_else(|_| env::panic_str("Invalid manifest encoding"))
}

fn initialize(manifest: &[u8], fault_stage: u8, owner: Option<AccountId>) {
    require_bootstrap_caller();
    // Existence, not successful decoding: corrupt/oversized/legacy roots may
    // never be treated as absent, and a partial bootstrap may never be reset.
    if env::storage_has_key(b"STATE")
        || env::storage_has_key(BOOTSTRAP_KEY)
        || env::storage_has_key(near_marketplace_foundation::persistence::STORAGE_ROOT_KEY)
        || env::storage_has_key(nft_integration::authority::OWNER_KEY)
    {
        env::panic_str("Already initialized or existing state");
    }
    let domain = decode_manifest(manifest);
    check_domain(&domain);
    let owner = owner.map(|owner| {
        nft_integration::authority::encode(&domain, &owner)
            .unwrap_or_else(|_| env::panic_str("Invalid admission owner"))
    });
    let batch =
        bootstrap_batch(&domain).unwrap_or_else(|_| env::panic_str("Invalid bootstrap root"));
    HostStore
        .apply(&batch)
        .unwrap_or_else(|_| env::panic_str("Bootstrap write failed"));
    inject_fault(fault_stage, 1);
    let mut stored = BOOTSTRAP_MAGIC.to_vec();
    stored.extend_from_slice(manifest);
    env::storage_write(BOOTSTRAP_KEY, &stored);
    inject_fault(fault_stage, 2);
    HostStore
        .apply(&bootstrap_storage_batch(&domain).unwrap())
        .unwrap_or_else(|_| env::panic_str("Storage accounting bootstrap failed"));
    inject_fault(fault_stage, 3);
    if let Some(owner) = owner {
        env::storage_write(nft_integration::authority::OWNER_KEY, &owner);
    }
    // Uses the installed SDK's constant byte price, not a live protocol query.
    // No customer deposits/liabilities exist at this fresh bootstrap. Pre-fund
    // the account externally; surplus is not attributed as spendable capital.
    let required = u128::from(env::storage_usage())
        .checked_mul(env::storage_byte_cost().as_yoctonear())
        .unwrap_or_else(|| env::panic_str("Storage cost overflow"));
    if env::account_balance().as_yoctonear() < required {
        env::panic_str("Unfunded bootstrap storage");
    }
    storage_accounting::assert_storage_backing(&domain);
}

fn inject_fault(stage: u8, current: u8) {
    #[cfg(feature = "test-hooks")]
    if stage == current {
        env::panic_str("Injected bootstrap abort");
    }
    #[cfg(not(feature = "test-hooks"))]
    {
        let _ = (stage, current);
    }
}

#[cfg(test)]
mod tests;
