//! Offline migration planning only: no state writes, initializer, or NEAR ABI.
//! Evidence completeness is an external audit/operator assertion, not proved by
//! an enum value or digest. See ../MIGRATION.md before using any generated plan.
use std::collections::{BTreeMap, BTreeSet};

use near_contract_standards::non_fungible_token::{
    metadata::NFTContractMetadata, NonFungibleToken,
};
use near_sdk::{
    borsh::{self, BorshDeserialize, BorshSerialize},
    collections::{LazyOption, LookupMap},
    AccountId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceLayout {
    LegacyWithoutRegistry,
    RegistryV1,
    BindingV2,
}

#[derive(BorshDeserialize)]
#[borsh(crate = "near_sdk::borsh")]
struct LegacyState {
    _tokens: NonFungibleToken,
    _metadata: LazyOption<NFTContractMetadata>,
}

/// Frozen pre-marketplace-binding root. Do not add fields or replace this with
/// `Contract`: collection serialization is pinned to SDK/standards =5.30.0.
/// Field order and existing prefixes are part of the migration ABI.
#[derive(BorshDeserialize, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
pub(crate) struct RegistryV1State {
    pub(crate) tokens: NonFungibleToken,
    pub(crate) metadata: LazyOption<NFTContractMetadata>,
    pub(crate) ever_minted: LookupMap<String, bool>,
}

/// Exact root before resolver hardening. New mutation records are sidecars;
/// this layout and every existing collection handle remain unchanged.
#[derive(BorshDeserialize, BorshSerialize)]
#[borsh(crate = "near_sdk::borsh")]
pub(crate) struct BindingV2State {
    pub(crate) tokens: NonFungibleToken,
    pub(crate) metadata: LazyOption<NFTContractMetadata>,
    pub(crate) ever_minted: LookupMap<String, bool>,
    pub(crate) marketplace_id: Option<AccountId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationError {
    LayoutMismatch,
    IncompleteHistory,
    InvalidHistory,
    MissingMint,
    RegistryConflict,
    InvalidBinding,
}

/// Decode only the exact layout pinned by the approved source-code/state manifest.
/// No fallback/default initializer and no trailing bytes are accepted.
pub fn check_source_layout(bytes: &[u8], expected: SourceLayout) -> Result<(), MigrationError> {
    match expected {
        SourceLayout::LegacyWithoutRegistry => borsh::from_slice::<LegacyState>(bytes).map(|_| ()),
        SourceLayout::RegistryV1 => borsh::from_slice::<RegistryV1State>(bytes).map(|_| ()),
        SourceLayout::BindingV2 => borsh::from_slice::<BindingV2State>(bytes).map(|_| ()),
    }
    .map_err(|_| MigrationError::LayoutMismatch)
}

/// Offline, write-free root conversion, NOT a migration entry point or authority
/// check. Source completeness, authenticity, funding and receipt closure must be
/// independently established. All collection handles move unchanged; no live-ID
/// enumeration can reconstruct absent burned IDs. Rejects legacy and V2 roots.
pub fn prepare_binding_v2(
    registry_v1_root: &[u8],
    nft_account_id: &AccountId,
    marketplace_id: AccountId,
) -> Result<crate::Contract, MigrationError> {
    if nft_account_id == &marketplace_id {
        return Err(MigrationError::InvalidBinding);
    }
    let old: RegistryV1State =
        borsh::from_slice(registry_v1_root).map_err(|_| MigrationError::LayoutMismatch)?;
    Ok(crate::Contract {
        tokens: old.tokens,
        metadata: old.metadata,
        ever_minted: old.ever_minted,
        marketplace_id: Some(marketplace_id),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryCoverage {
    LiveEnumerationOnly,
    Incomplete,
    /// An externally verified, complete successful-mint/approval history across
    /// every deployment and upgrade, including burns. A digest alone is not proof.
    ExternallyVerifiedLifetime {
        evidence_digest: [u8; 32],
    },
}

#[derive(Clone, Debug)]
pub struct HistoricalMint {
    pub token_id: String,
    /// Historical next counter, including the last counter before any burn.
    pub next_approval_id: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedIdentity {
    pub token_id: String,
    pub burned: bool,
    pub next_approval_id: Option<u64>,
}

/// Pure, offline inventory preparation; does not authorize migration or claim
/// that the supplied history/snapshot is authentic. Preserve live ownership and
/// metadata separately; this plan never recreates a live NFT from a burned ID.
pub fn plan_id_import(
    coverage: HistoryCoverage,
    successful_mints: &[HistoricalMint],
    live_ids: &[String],
    existing_registry: &[String],
    retained_counters: &BTreeMap<String, u64>,
) -> Result<Vec<ImportedIdentity>, MigrationError> {
    if !matches!(coverage, HistoryCoverage::ExternallyVerifiedLifetime { evidence_digest } if evidence_digest != [0; 32])
    {
        return Err(MigrationError::IncompleteHistory);
    }
    let live: BTreeSet<_> = live_ids.iter().cloned().collect();
    if live.len() != live_ids.len() {
        return Err(MigrationError::InvalidHistory);
    }
    let mut identities = BTreeMap::new();
    for mint in successful_mints {
        if mint.token_id.is_empty() || mint.next_approval_id == Some(0) {
            return Err(MigrationError::InvalidHistory);
        }
        let retained = retained_counters.get(&mint.token_id).copied();
        if retained == Some(0) {
            return Err(MigrationError::InvalidHistory);
        }
        let identity = ImportedIdentity {
            token_id: mint.token_id.clone(),
            burned: !live.contains(&mint.token_id),
            next_approval_id: mint.next_approval_id.max(retained),
        };
        // Two successful mints of one ID are a historical identity conflict, not
        // an excuse to discard a burn or overwrite the earlier provenance.
        if identities.insert(mint.token_id.clone(), identity).is_some() {
            return Err(MigrationError::InvalidHistory);
        }
    }
    if live
        .iter()
        .chain(retained_counters.keys())
        .any(|id| !identities.contains_key(id))
    {
        return Err(MigrationError::MissingMint);
    }
    if existing_registry
        .iter()
        .any(|id| !identities.contains_key(id))
    {
        return Err(MigrationError::RegistryConflict);
    }
    Ok(identities.into_values().collect())
}
