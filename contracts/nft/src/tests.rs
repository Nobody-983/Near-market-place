use super::*;
#[cfg(feature = "testnet")]
#[path = "testnet_mint_tests.rs"]
mod testnet_mint_tests;
use near_contract_standards::non_fungible_token::{
    core::NonFungibleTokenCore,
    metadata::{NonFungibleTokenMetadataProvider, NFT_METADATA_SPEC},
    NonFungibleTokenApproval, NonFungibleTokenEnumeration,
};
use near_sdk::{
    json_types::{Base64VecU8, U128},
    test_utils::VMContextBuilder,
    testing_env, Gas, NearToken,
};

fn account(name: &str) -> AccountId {
    name.parse().expect("valid test account id")
}

fn contract_id() -> AccountId {
    account("nft.testnet")
}

fn alice() -> AccountId {
    account("alice.testnet")
}

fn bob() -> AccountId {
    account("bob.testnet")
}

fn one_yocto() -> NearToken {
    NearToken::from_yoctonear(1)
}

fn context(predecessor: AccountId, deposit: NearToken) -> VMContextBuilder {
    let mut builder = VMContextBuilder::new();
    builder
        .current_account_id(contract_id())
        .signer_account_id(predecessor.clone())
        .predecessor_account_id(predecessor)
        .attached_deposit(deposit)
        .prepaid_gas(Gas::from_tgas(300));
    builder
}

fn contract_metadata() -> NFTContractMetadata {
    NFTContractMetadata {
        spec: NFT_METADATA_SPEC.to_string(),
        name: "NEAR NFT Collection".to_string(),
        symbol: "NEARNFT".to_string(),
        icon: Some("data:image/svg+xml;base64,PHN2Zy8+".to_string()),
        base_uri: None,
        reference: None,
        reference_hash: None,
    }
}

fn token_metadata(title: &str) -> TokenMetadata {
    TokenMetadata {
        title: Some(title.to_string()),
        description: Some(format!("Metadata for {title}")),
        media: Some(format!("https://example.test/{title}.png")),
        media_hash: Some(Base64VecU8::from(vec![7u8; 32])),
        copies: Some(1),
        issued_at: Some("2026-10-29T00:00:00Z".to_string()),
        expires_at: None,
        starts_at: None,
        updated_at: None,
        extra: None,
        reference: None,
        reference_hash: None,
    }
}

fn setup() -> Contract {
    testing_env!(context(contract_id(), NearToken::ZERO).build());
    Contract::new(alice(), contract_metadata())
}

#[test]
fn frozen_registry_v1_matches_captured_previous_contract_bytes() {
    testing_env!(context(alice(), NearToken::from_near(10)).build());
    let previous = near_sdk::borsh::to_vec(&previous_layout_fixture()).unwrap();
    let captured = include_str!("../tests/fixtures/registry_v1_root.hex").trim();
    let hex: String = previous.iter().map(|b| format!("{b:02x}")).collect();
    // Captured and checked against the actual old Contract BEFORE adding binding.
    assert_eq!(hex, captured);
    let frozen: crate::migration::RegistryV1State = near_sdk::borsh::from_slice(&previous).unwrap();
    assert_eq!(previous, near_sdk::borsh::to_vec(&frozen).unwrap());
}

fn previous_layout_fixture() -> crate::migration::RegistryV1State {
    crate::migration::RegistryV1State {
        tokens: NonFungibleToken::new(b"o", alice(), Some(b"m"), Some(b"e"), Some(b"a")),
        metadata: LazyOption::new(b"c", Some(&contract_metadata())),
        ever_minted: LookupMap::new(b"used".to_vec()),
    }
}

fn mint(contract: &mut Contract, token_id: &str, owner: AccountId) {
    testing_env!(context(owner.clone(), NearToken::from_near(1)).build());
    // Exercise the storage primitive without enabling any public mint policy.
    contract.mint_once(token_id.to_string(), owner, token_metadata(token_id));
}

#[test]
fn constructor_exposes_contract_metadata() {
    let contract = setup();
    let metadata = contract.nft_metadata();

    assert_eq!(metadata.spec, NFT_METADATA_SPEC);
    assert_eq!(metadata.name, "NEAR NFT Collection");
    assert_eq!(metadata.symbol, "NEARNFT");
    assert_eq!(contract.contract_owner_id(), alice());
}

#[test]
fn mint_creates_token_with_owner_metadata_and_enumeration() {
    let mut contract = setup();
    mint(&mut contract, "token-1", alice());

    let token = contract
        .nft_token("token-1".to_string())
        .expect("minted token");
    assert_eq!(token.owner_id, alice());
    assert_eq!(
        token.metadata.as_ref().unwrap().title.as_deref(),
        Some("token-1")
    );
    assert_eq!(token.approved_account_ids, Some(HashMap::new()));

    assert_eq!(contract.nft_total_supply(), U128(1));
    assert_eq!(contract.nft_supply_for_owner(alice()), U128(1));
    assert_eq!(contract.nft_supply_for_owner(bob()), U128(0));

    let tokens = contract.nft_tokens(None, Some(10));
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].token_id, "token-1");

    let owner_tokens = contract.nft_tokens_for_owner(alice(), None, Some(10));
    assert_eq!(owner_tokens.len(), 1);
    assert_eq!(owner_tokens[0].token_id, "token-1");
}

#[test]
#[should_panic(expected = "token_id must be unique")]
fn mint_rejects_duplicate_token_id() {
    let mut contract = setup();
    mint(&mut contract, "token-1", alice());
    mint(&mut contract, "token-1", alice());
}

#[test]
fn owner_can_transfer_token() {
    let mut contract = setup();
    mint(&mut contract, "token-1", alice());

    testing_env!(context(alice(), one_yocto()).build());
    contract.nft_transfer(bob(), "token-1".to_string(), None, None);

    assert_eq!(
        contract.nft_token("token-1".to_string()).unwrap().owner_id,
        bob()
    );
    assert_eq!(contract.nft_supply_for_owner(alice()), U128(0));
    assert_eq!(contract.nft_supply_for_owner(bob()), U128(1));
}

#[test]
#[should_panic(expected = "Sender not approved")]
fn non_owner_cannot_transfer_token() {
    let mut contract = setup();
    mint(&mut contract, "token-1", alice());

    testing_env!(context(bob(), one_yocto()).build());
    contract.nft_transfer(bob(), "token-1".to_string(), None, None);
}

#[test]
#[should_panic(expected = "Requires attached deposit of exactly 1 yoctoNEAR")]
fn transfer_requires_one_yocto() {
    let mut contract = setup();
    mint(&mut contract, "token-1", alice());

    testing_env!(context(alice(), NearToken::ZERO).build());
    contract.nft_transfer(bob(), "token-1".to_string(), None, None);
}

#[test]
fn approved_account_can_transfer_and_approvals_are_cleared() {
    let mut contract = setup();
    mint(&mut contract, "token-1", alice());

    testing_env!(context(alice(), NearToken::from_near(1)).build());
    assert!(contract
        .nft_approve("token-1".to_string(), bob(), None)
        .is_none());
    assert!(contract.nft_is_approved("token-1".to_string(), bob(), None));

    let approval_id = contract
        .nft_token("token-1".to_string())
        .unwrap()
        .approved_account_ids
        .unwrap()
        .get(&bob())
        .copied()
        .expect("bob approval id");

    testing_env!(context(bob(), one_yocto()).build());
    contract.nft_transfer(
        bob(),
        "token-1".to_string(),
        Some(approval_id),
        Some("approved transfer".to_string()),
    );

    let token = contract.nft_token("token-1".to_string()).unwrap();
    assert_eq!(token.owner_id, bob());
    assert_eq!(token.approved_account_ids, Some(HashMap::new()));
}

#[test]
fn owner_can_revoke_approval() {
    let mut contract = setup();
    mint(&mut contract, "token-1", alice());

    testing_env!(context(alice(), NearToken::from_near(1)).build());
    contract.nft_approve("token-1".to_string(), bob(), None);
    assert!(contract.nft_is_approved("token-1".to_string(), bob(), None));

    testing_env!(context(alice(), one_yocto()).build());
    contract.nft_revoke("token-1".to_string(), bob());
    assert!(!contract.nft_is_approved("token-1".to_string(), bob(), None));
}

#[test]
fn transfer_call_moves_token_and_returns_promise() {
    let mut contract = setup();
    mint(&mut contract, "token-1", alice());

    testing_env!(context(alice(), one_yocto()).build());
    let result = contract.nft_transfer_call(
        bob(),
        "token-1".to_string(),
        None,
        None,
        "escrow-listing".to_string(),
    );

    assert!(matches!(result, PromiseOrValue::Promise(_)));
    assert_eq!(
        contract.nft_token("token-1".to_string()).unwrap().owner_id,
        bob()
    );
}

#[test]
#[cfg_attr(
    not(feature = "testnet"),
    should_panic(expected = "Minting is disabled pending policy approval")
)]
#[cfg_attr(
    feature = "testnet",
    should_panic(expected = "Testnet mint authority not configured")
)]
fn public_mint_is_disabled_even_for_contract_owner() {
    let mut contract = setup();
    contract.nft_mint("disabled".into(), alice(), token_metadata("disabled"));
}

#[test]
fn permanent_registry_survives_transfer_and_simulated_burn() {
    let mut contract = setup();
    mint(&mut contract, "permanent", alice());
    assert_eq!(
        contract.token_id_status("permanent".into()),
        Some(TokenIdStatus::Live)
    );
    testing_env!(context(alice(), one_yocto()).build());
    contract.nft_transfer(bob(), "permanent".into(), None, None);
    let id = "permanent".to_string();
    // There is no public burn method. Model its removal of live data, keeping
    // the independent registry and next-approval counter untouched.
    contract
        .tokens
        .next_approval_id_by_id
        .as_mut()
        .unwrap()
        .insert(&id, &42);
    contract.tokens.owner_by_id.remove(&id);
    contract
        .tokens
        .token_metadata_by_id
        .as_mut()
        .unwrap()
        .remove(&id);
    let owners = contract.tokens.tokens_per_owner.as_mut().unwrap();
    let mut owned = owners.get(&bob()).unwrap();
    owned.remove(&id);
    owners.insert(&bob(), &owned);
    assert_eq!(
        contract.token_id_status(id.clone()),
        Some(TokenIdStatus::Burned)
    );
    assert_eq!(
        contract
            .tokens
            .next_approval_id_by_id
            .as_ref()
            .unwrap()
            .get(&id),
        Some(42)
    );
    assert!(contract.nft_token(id.clone()).is_none());
    assert_eq!(contract.nft_total_supply(), U128(0));

    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        mint(&mut contract, &id, alice());
    }));
    assert!(failed.is_err());
    assert_eq!(contract.token_id_status(id), Some(TokenIdStatus::Burned));
}

#[test]
fn resolver_cannot_resurrect_a_burned_id_or_its_approvals() {
    let mut contract = setup();
    mint(&mut contract, "burned", alice());
    contract.tokens.owner_by_id.remove(&"burned".into());
    testing_env!(
        context(contract_id(), NearToken::ZERO).build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        vec![near_sdk::PromiseResult::Successful(b"true".to_vec())]
    );
    assert!(contract.nft_resolve_transfer_versioned(
        alice(),
        bob(),
        "burned".into(),
        None,
        near_sdk::json_types::U64(0),
        near_sdk::json_types::U64(0)
    ));
    assert_eq!(
        contract.token_id_status("burned".into()),
        Some(TokenIdStatus::Burned)
    );
    assert!(contract
        .tokens
        .approvals_by_id
        .as_ref()
        .unwrap()
        .get(&"burned".into())
        .is_none());
}

#[test]
fn invalid_metadata_does_not_consume_an_id() {
    let mut contract = setup();
    let mut invalid = token_metadata("retry");
    invalid.media_hash = Some(Base64VecU8(vec![1]));
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        contract.mint_once("retry".into(), alice(), invalid);
    }))
    .is_err());
    assert_eq!(contract.token_id_status("retry".into()), None);
    mint(&mut contract, "retry", alice());
    assert_eq!(
        contract.token_id_status("retry".into()),
        Some(TokenIdStatus::Live)
    );
}

#[test]
fn registry_storage_is_included_and_id_bytes_are_not_normalized() {
    let mut contract = setup();
    testing_env!(context(alice(), NearToken::from_near(1)).build());
    let before = env::storage_usage();
    contract.mint_once("Token".into(), alice(), token_metadata("Token"));
    let total = env::storage_usage() - before;
    let expected_refund = NearToken::from_near(1).as_yoctonear()
        - u128::from(total) * env::storage_byte_cost().as_yoctonear();
    let receipts = near_sdk::test_utils::get_created_receipts();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].receiver_id, alice());
    assert!(
        matches!(&receipts[0].actions[..], [near_sdk::mock::MockAction::Transfer { deposit, .. }]
        if deposit.as_yoctonear() == expected_refund)
    );
    let id = "Token".to_string();
    // Remove only inside the fixture to measure the permanent allocation.
    let with_registry = env::storage_usage();
    contract.ever_minted.remove(&id);
    let registry_bytes = with_registry - env::storage_usage();
    contract.ever_minted.insert(&id, &true);
    assert!(registry_bytes > 0 && total > registry_bytes);
    mint(&mut contract, "token", alice());
    assert_eq!(contract.nft_total_supply(), U128(2));
}

#[test]
#[should_panic(expected = "Insufficient mint storage deposit")]
fn mint_primitive_rejects_unfunded_storage() {
    let mut contract = setup();
    testing_env!(context(alice(), NearToken::ZERO).build());
    contract.mint_once("unfunded".into(), alice(), token_metadata("unfunded"));
}

fn receipt_refunds() -> u128 {
    near_sdk::test_utils::get_created_receipts()
        .iter()
        .flat_map(|r| &r.actions)
        .filter_map(|a| match a {
            near_sdk::mock::MockAction::Transfer { deposit, .. } => Some(deposit.as_yoctonear()),
            _ => None,
        })
        .sum()
}

fn next_receipt(caller: AccountId, deposit: NearToken) {
    let usage = env::storage_usage();
    testing_env!(context(caller, deposit).storage_usage(usage).build());
}

#[test]
fn long_id_approval_and_revocation_charge_actual_bytes_and_retain_counter_backing() {
    let mut contract = setup();
    let id = "x".repeat(MAX_TOKEN_ID_BYTES);
    mint(&mut contract, &id, alice());
    let byte_cost = env::storage_byte_cost().as_yoctonear();
    let deposit = NearToken::from_near(1);
    next_receipt(alice(), deposit);
    let before = env::storage_usage();
    contract.nft_approve(id.clone(), bob(), None);
    let added = env::storage_usage() - before;
    // Both storage keys contain this long ID, not only the approved account.
    assert!(added > (2 * MAX_TOKEN_ID_BYTES) as u64);
    assert_eq!(
        receipt_refunds(),
        deposit.as_yoctonear() - u128::from(added) * byte_cost
    );
    assert_eq!(
        contract
            .tokens
            .next_approval_id_by_id
            .as_ref()
            .unwrap()
            .get(&id),
        Some(2)
    );

    next_receipt(alice(), one_yocto());
    let before_revoke = env::storage_usage();
    contract.nft_revoke(id.clone(), bob());
    let released = before_revoke - env::storage_usage();
    assert_eq!(receipt_refunds(), u128::from(released) * byte_cost);
    assert!(added > released);
    assert_eq!(
        contract.token_id_status(id.clone()),
        Some(TokenIdStatus::Live)
    );
    let counters = contract.tokens.next_approval_id_by_id.as_mut().unwrap();
    let before_counter = env::storage_usage();
    let value = counters.remove(&id).unwrap(); // fixture measurement only
    let counter_bytes = before_counter - env::storage_usage();
    counters.insert(&id, &value);
    assert_eq!(added - released, counter_bytes);

    // The retained counter is not charged again. Exact map funding is sufficient.
    next_receipt(
        alice(),
        NearToken::from_yoctonear(u128::from(released) * byte_cost),
    );
    let before = env::storage_usage();
    contract.nft_approve(id.clone(), bob(), None);
    assert_eq!(env::storage_usage() - before, released);
    assert_eq!(receipt_refunds(), 0);
    assert_eq!(
        contract
            .tokens
            .next_approval_id_by_id
            .as_ref()
            .unwrap()
            .get(&id),
        Some(3)
    );
}

#[test]
#[should_panic(expected = "Insufficient approval storage deposit")]
fn long_id_rejects_the_old_sdk_account_only_storage_quote() {
    let mut contract = setup();
    let id = "y".repeat(MAX_TOKEN_ID_BYTES);
    mint(&mut contract, &id, alice());
    let old_quote = (bob().as_str().len() as u128 + 12) * env::storage_byte_cost().as_yoctonear();
    next_receipt(alice(), NearToken::from_yoctonear(old_quote));
    contract.nft_approve(id, bob(), None);
}

#[test]
fn replacement_and_partial_revocation_do_not_refund_retained_storage() {
    let mut contract = setup();
    let id = "z".repeat(MAX_TOKEN_ID_BYTES);
    mint(&mut contract, &id, alice());
    next_receipt(alice(), NearToken::from_near(1));
    contract.nft_approve(id.clone(), bob(), None);
    next_receipt(alice(), NearToken::from_near(1));
    contract.nft_approve(id.clone(), account("third.testnet"), None);
    next_receipt(alice(), one_yocto());
    let before = env::storage_usage();
    contract.nft_approve(id.clone(), bob(), None);
    assert_eq!(env::storage_usage(), before);
    assert_eq!(receipt_refunds(), 0);
    assert!(contract.nft_is_approved(id.clone(), bob(), Some(3)));
    next_receipt(alice(), one_yocto());
    let before = env::storage_usage();
    contract.nft_revoke(id.clone(), bob());
    assert_eq!(
        receipt_refunds(),
        u128::from(before - env::storage_usage()) * env::storage_byte_cost().as_yoctonear()
    );
    assert!(contract.nft_is_approved(id.clone(), account("third.testnet"), Some(2)));
    next_receipt(alice(), one_yocto());
    let before = env::storage_usage();
    contract.nft_revoke_all(id.clone());
    assert_eq!(
        receipt_refunds(),
        u128::from(before - env::storage_usage()) * env::storage_byte_cost().as_yoctonear()
    );
    assert_eq!(
        contract
            .tokens
            .next_approval_id_by_id
            .as_ref()
            .unwrap()
            .get(&id),
        Some(4)
    );
    assert!(contract.ever_minted.contains_key(&id));
}

#[test]
fn approval_counter_exhaustion_rejects_before_writes() {
    let mut contract = setup();
    let id = "exhausted".to_string();
    mint(&mut contract, &id, alice());
    contract
        .tokens
        .next_approval_id_by_id
        .as_mut()
        .unwrap()
        .insert(&id, &u64::MAX);
    next_receipt(alice(), NearToken::from_near(1));
    let before = env::storage_usage();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        contract.nft_approve(id.clone(), bob(), None);
    }))
    .is_err());
    assert_eq!(env::storage_usage(), before);
    assert_eq!(
        contract
            .tokens
            .next_approval_id_by_id
            .as_ref()
            .unwrap()
            .get(&id),
        Some(u64::MAX)
    );
    assert!(!contract.nft_is_approved(id, bob(), None));
}

#[test]
fn migration_layout_checks_never_reinitialize_or_guess_a_codec() {
    use crate::migration::{check_source_layout, MigrationError, SourceLayout};
    #[derive(near_sdk::borsh::BorshSerialize)]
    #[borsh(crate = "near_sdk::borsh")]
    struct LegacyFixture<'a> {
        tokens: &'a NonFungibleToken,
        metadata: &'a LazyOption<NFTContractMetadata>,
    }
    testing_env!(context(alice(), NearToken::from_near(10)).build());
    let contract = previous_layout_fixture();
    let old = near_sdk::borsh::to_vec(&LegacyFixture {
        tokens: &contract.tokens,
        metadata: &contract.metadata,
    })
    .unwrap();
    let current = near_sdk::borsh::to_vec(&contract).unwrap();
    assert_eq!(
        check_source_layout(&old, SourceLayout::LegacyWithoutRegistry),
        Ok(())
    );
    assert_eq!(
        check_source_layout(&current, SourceLayout::RegistryV1),
        Ok(())
    );
    assert_eq!(
        check_source_layout(&old, SourceLayout::RegistryV1),
        Err(MigrationError::LayoutMismatch)
    );
    assert_eq!(
        check_source_layout(&current, SourceLayout::LegacyWithoutRegistry),
        Err(MigrationError::LayoutMismatch)
    );
    let mut extra = current.clone();
    extra.push(0);
    assert!(check_source_layout(&extra, SourceLayout::RegistryV1).is_err());
    assert!(check_source_layout(&current[..current.len() - 1], SourceLayout::RegistryV1).is_err());
}

#[path = "binding_tests.rs"]
mod binding;

#[test]
fn migration_requires_complete_history_and_preserves_burns_and_counters() {
    use crate::migration::*;
    use std::collections::BTreeMap;
    let mints = vec![
        HistoricalMint {
            token_id: "live".into(),
            next_approval_id: Some(5),
        },
        HistoricalMint {
            token_id: "burned".into(),
            next_approval_id: Some(u64::MAX),
        },
    ];
    let live = vec!["live".to_string()];
    let counters = BTreeMap::from([("live".into(), 8)]);
    for coverage in [
        HistoryCoverage::LiveEnumerationOnly,
        HistoryCoverage::Incomplete,
        HistoryCoverage::ExternallyVerifiedLifetime {
            evidence_digest: [0; 32],
        },
    ] {
        assert_eq!(
            plan_id_import(coverage, &mints, &live, &[], &counters),
            Err(MigrationError::IncompleteHistory)
        );
    }
    let coverage = HistoryCoverage::ExternallyVerifiedLifetime {
        evidence_digest: [1; 32],
    };
    let plan = plan_id_import(coverage, &mints, &live, &["burned".into()], &counters).unwrap();
    assert_eq!(
        plan[0],
        ImportedIdentity {
            token_id: "burned".into(),
            burned: true,
            next_approval_id: Some(u64::MAX)
        }
    );
    assert_eq!(plan[1].next_approval_id, Some(8));
    assert!(!plan[1].burned);
    assert_eq!(
        plan_id_import(coverage, &mints, &["unknown".into()], &[], &counters),
        Err(MigrationError::MissingMint)
    );
    assert_eq!(
        plan_id_import(coverage, &mints[..1], &live, &["burned".into()], &counters),
        Err(MigrationError::RegistryConflict)
    );
    let duplicate = vec![mints[0].clone(), mints[0].clone()];
    assert_eq!(
        plan_id_import(coverage, &duplicate, &live, &[], &counters),
        Err(MigrationError::InvalidHistory)
    );
    let legacy_long = vec![HistoricalMint {
        token_id: "L".repeat(2_048),
        next_approval_id: None,
    }];
    assert_eq!(
        plan_id_import(coverage, &legacy_long, &[], &[], &BTreeMap::new()).unwrap()[0].token_id,
        legacy_long[0].token_id
    );
}

#[path = "security_tests.rs"]
mod security;
