use super::*;
use near_marketplace_foundation::{
    persistence::{Budgets, MemoryStore, OfflineCommand, Repository, MAX_RECORD_BYTES},
    state::ListingTerms,
    types::{FeeTerms, TokenKey},
};
use near_sdk::{test_utils::VMContextBuilder, testing_env, NearToken};

fn domain() -> DeploymentDomain {
    DeploymentDomain {
        environment: if cfg!(feature = "production") {
            ArtifactDomain::Production
        } else {
            ArtifactDomain::Testnet
        },
        genesis: [1; 32],
        manifest: [2; 32],
        marketplace_code: [3; 32],
        nft_code: [4; 32],
        marketplace: "market.testnet".parse().unwrap(),
        nft: "nft.testnet".parse().unwrap(),
    }
}

fn setup() {
    let d = domain();
    let mut context = VMContextBuilder::new();
    context
        .current_account_id(d.marketplace.clone())
        .signer_account_id(d.marketplace.clone())
        .predecessor_account_id(d.marketplace)
        .account_contract(AccountContract::Local(d.marketplace_code).into())
        .account_balance(NearToken::from_near(100));
    // Native tests are isolated. No production code may erase storage this way.
    near_sdk::mock::with_mocked_blockchain(|b| {
        b.take_storage();
    });
    testing_env!(context.build());
}

#[test]
fn bootstrap_is_byte_identical_to_offline_v1_and_status_preserves_root() {
    setup();
    let d = domain();
    let offline = Repository::<MemoryStore>::new_offline(d.clone(), None)
        .unwrap()
        .into_store();
    let expected = offline.read(b"STATE").unwrap().unwrap();
    Runtime::bootstrap(Base64VecU8(borsh::to_vec(&d).unwrap()));
    assert_eq!(env::storage_read(b"STATE").unwrap(), expected);
    assert_eq!(
        decode_record(&expected, &d, &RecordKey::Root)
            .unwrap()
            .revision(),
        1
    );
    assert!(!Runtime::storage_status().trading_enabled);
    assert_eq!(env::storage_read(b"STATE").unwrap(), expected);
    assert_eq!(host_store::read_bounded(b"absent"), None);
}

#[test]
fn raw_store_preserves_exact_keys_and_applies_offline_compare_and_swap() {
    setup();
    let d = domain();
    let batch = bootstrap_batch(&d).unwrap();
    HostStore.apply(&batch).unwrap();
    let (key, value) = batch.writes().next().unwrap();
    assert_eq!(key, b"STATE");
    assert_eq!(HostStore.read(key).unwrap().as_deref(), Some(value));
    // There is no SDK marker serialization or automatic initialization here.
    assert!(!env::storage_has_key(BOOTSTRAP_KEY));
}

#[test]
fn maximum_bounded_value_roundtrips_exactly() {
    setup();
    let bytes = vec![0xA5; MAX_RECORD_BYTES];
    env::storage_write(b"bounded", &bytes);
    assert_eq!(HostStore.read(b"bounded").unwrap(), Some(bytes));
}

#[test]
fn bounded_offline_batch_preserves_non_root_keys_and_reopens_through_host_store() {
    setup();
    let d = domain();
    Runtime::bootstrap(Base64VecU8(borsh::to_vec(&d).unwrap()));
    // Synthetic fixture only. No listing ABI, promise or authorization exists.
    let repo = Repository::<MemoryStore>::new_offline(d.clone(), None).unwrap();
    let token = TokenKey::new(d.nft.clone(), "é".repeat(512)).unwrap();
    let prepared = repo
        .prepare(OfflineCommand::Listing {
            terms: ListingTerms {
                token: token.clone(),
                seller: "seller.testnet".parse().unwrap(),
                approval_id: u64::MAX,
                price: 1_000,
                expires_at_ns: 10_000,
                fees: FeeTerms::new(Some(0), Some(0), Some("fees.testnet".parse().unwrap()))
                    .unwrap(),
                policy_version: 1,
                policy_hash: [5; 32],
                correlation: [6; 32],
            },
            now_ns: 1,
            budgets: Budgets {
                first_call_gas: 101,
                second_call_gas: 102,
                callback_gas: 103,
                max_result_bytes: 4096,
                call_abi: [7; 32],
                callback_abi: [8; 32],
            },
        })
        .unwrap();
    assert!(prepared.batch().record_count() > 1);
    HostStore.apply(prepared.batch()).unwrap();
    for (key, bytes) in prepared.batch().writes() {
        assert_eq!(HostStore.read(key).unwrap().as_deref(), Some(bytes));
    }
    let reopened = Repository::open(HostStore, d.clone()).unwrap();
    assert!(reopened.read(&RecordKey::Token(token)).unwrap().is_some());
    assert!(reopened
        .read(&RecordKey::Listing(prepared.generation().unwrap()))
        .unwrap()
        .is_some());
    assert_eq!(Runtime::storage_status().root_revision.0, 2);
    assert!(!Runtime::storage_status().trading_enabled);
}

#[test]
fn manifest_roundtrip_and_maximum_account_lengths() {
    setup();
    let mut d = domain();
    d.marketplace = "a".repeat(64).parse().unwrap();
    d.nft = "b".repeat(64).parse().unwrap();
    let bytes = borsh::to_vec(&d).unwrap();
    assert_eq!(bytes.len(), MAX_MANIFEST_BYTES);
    assert_eq!(decode_manifest(&bytes), d);
}

#[test]
fn runtime_charges_actual_long_id_storage_growth_to_one_existing_reservation() {
    setup();
    Runtime::bootstrap(Base64VecU8(borsh::to_vec(&domain()).unwrap()));
    let charged = crate::test_storage_fixture::exercise(&domain(), false);
    assert!(charged > 0);
    crate::storage_accounting::assert_storage_backing(&domain());
    assert!(!Runtime::storage_status().trading_enabled);
}

// env::panic_str uses a host ABI. Rejections and post-write traps are exercised
// through compiled Wasm, not catch_unwind around native FFI.
