use super::*;
use crate::mutation::Version;
use near_sdk::{borsh, json_types::U64, test_utils::get_created_receipts, PromiseResult};

fn callback() {
    testing_env!(
        context(contract_id(), NearToken::ZERO)
            .storage_usage(env::storage_usage())
            .build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        vec![PromiseResult::Successful(b"true".to_vec())]
    );
}
fn resolve(
    c: &mut Contract,
    token: &str,
    v: Version,
    approvals: Option<HashMap<AccountId, u64>>,
) -> bool {
    callback();
    c.nft_resolve_transfer_versioned(
        alice(),
        bob(),
        token.into(),
        approvals,
        U64(v.ownership),
        U64(v.approval),
    )
}
fn send(c: &mut Contract, token: &str) -> (Version, Option<HashMap<AccountId, u64>>) {
    let approvals = c
        .tokens
        .approvals_by_id
        .as_ref()
        .unwrap()
        .get(&token.to_string());
    next_receipt(alice(), one_yocto());
    drop(c.nft_transfer_call(bob(), token.into(), None, None, "return".into()));
    (Version::load(token).unwrap(), approvals)
}

#[test]
fn repeated_absent_and_empty_map_cycles_never_create_refunds_even_for_long_ids() {
    for id in ["short".to_string(), "L".repeat(MAX_TOKEN_ID_BYTES)] {
        let mut c = setup();
        mint(&mut c, &id, alice());
        for empty in [false, true] {
            if empty {
                c.tokens
                    .approvals_by_id
                    .as_mut()
                    .unwrap()
                    .insert(&id, &HashMap::new());
            }
            for _ in 0..12 {
                let (v, map) = send(&mut c, &id);
                assert!(!resolve(&mut c, &id, v, map));
                assert!(get_created_receipts().is_empty());
                assert!(!c.tokens.approvals_by_id.as_ref().unwrap().contains_key(&id));
                next_receipt(alice(), one_yocto());
                c.nft_revoke_all(id.clone());
                assert!(get_created_receipts().is_empty());
            }
        }
        assert!(c.ever_minted.contains_key(&id));
    }
}

#[test]
fn funded_snapshot_restores_once_and_refunds_only_once() {
    let mut c = setup();
    mint(&mut c, "funded", alice());
    next_receipt(alice(), NearToken::from_near(1));
    c.nft_approve("funded".into(), bob(), None);
    let (v, map) = send(&mut c, "funded");
    assert!(!resolve(&mut c, "funded", v, map.clone()));
    assert!(c.nft_is_approved("funded".into(), bob(), Some(1)));
    next_receipt(alice(), one_yocto());
    c.nft_revoke_all("funded".into());
    assert_eq!(get_created_receipts().len(), 1);
    let root = borsh::to_vec(&c).unwrap();
    let usage = env::storage_usage();
    assert!(resolve(&mut c, "funded", v, map));
    assert_eq!(borsh::to_vec(&c).unwrap(), root);
    assert_eq!(env::storage_usage(), usage);
    assert!(get_created_receipts().is_empty());
    next_receipt(alice(), one_yocto());
    c.nft_revoke_all("funded".into());
    assert!(get_created_receipts().is_empty());
}

#[test]
fn approving_over_legacy_empty_map_charges_its_full_storage_again() {
    let mut c = setup();
    let id = "E".repeat(MAX_TOKEN_ID_BYTES);
    mint(&mut c, &id, alice());
    c.tokens
        .approvals_by_id
        .as_mut()
        .unwrap()
        .insert(&id, &HashMap::new());
    let before = env::storage_usage();
    c.tokens.approvals_by_id.as_mut().unwrap().remove(&id);
    let empty_bytes = before - env::storage_usage();
    c.tokens
        .approvals_by_id
        .as_mut()
        .unwrap()
        .insert(&id, &HashMap::new());
    next_receipt(alice(), NearToken::from_near(1));
    let usage = env::storage_usage();
    c.nft_approve(id.clone(), bob(), None);
    let charged_bytes = env::storage_usage() - usage + empty_bytes;
    let receipts = get_created_receipts();
    let refunded = receipts
        .iter()
        .flat_map(|r| &r.actions)
        .find_map(|a| match a {
            near_sdk::mock::MockAction::Transfer { deposit, .. } => Some(deposit.as_yoctonear()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        NearToken::from_near(1).as_yoctonear() - refunded,
        u128::from(charged_bytes) * env::storage_byte_cost().as_yoctonear()
    );
}

#[test]
fn delayed_resolver_cannot_override_intervening_transfer_revoke_or_reapproval() {
    let mut c = setup();
    mint(&mut c, "aba", alice());
    next_receipt(alice(), NearToken::from_near(1));
    c.nft_approve("aba".into(), bob(), None);
    let (v, map) = send(&mut c, "aba");
    next_receipt(bob(), one_yocto());
    c.nft_transfer(alice(), "aba".into(), None, None);
    next_receipt(alice(), one_yocto());
    c.nft_revoke_all("aba".into());
    next_receipt(alice(), NearToken::from_near(1));
    c.nft_approve("aba".into(), bob(), None);
    next_receipt(alice(), one_yocto());
    c.nft_transfer(bob(), "aba".into(), None, None);
    let new_version = Version::load("aba");
    assert!(resolve(&mut c, "aba", v, map));
    assert_eq!(c.nft_token("aba".into()).unwrap().owner_id, bob());
    assert!(!c.nft_is_approved("aba".into(), bob(), Some(1)));
    assert_eq!(Version::load("aba"), new_version);
    assert!(get_created_receipts().is_empty());
}

#[test]
fn receiver_revoke_without_an_entry_invalidates_snapshot_authorization() {
    let mut c = setup();
    mint(&mut c, "revoked", alice());
    next_receipt(alice(), NearToken::from_near(1));
    c.nft_approve("revoked".into(), bob(), None);
    let (v, map) = send(&mut c, "revoked");
    next_receipt(bob(), one_yocto());
    c.nft_revoke("revoked".into(), alice());
    assert!(!resolve(&mut c, "revoked", v, map));
    assert_eq!(c.nft_token("revoked".into()).unwrap().owner_id, alice());
    assert!(!c.nft_is_approved("revoked".into(), bob(), Some(1)));
}

#[test]
fn mutation_overflow_and_legacy_callback_fail_closed() {
    let mut c = setup();
    mint(&mut c, "overflow", alice());
    Version {
        ownership: u64::MAX,
        approval: 1,
    }
    .write("overflow");
    next_receipt(alice(), one_yocto());
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.nft_transfer(
            bob(),
            "overflow".into(),
            None,
            None
        )))
        .is_err()
    );
    assert_eq!(c.nft_token("overflow".into()).unwrap().owner_id, alice());
    callback();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.nft_resolve_transfer(
            alice(),
            bob(),
            "overflow".into(),
            None
        )))
        .is_err()
    );
}

#[test]
fn all_initializers_require_direct_self_identity_before_any_storage_write() {
    for caller in [alice(), bob()] {
        for constructor in 0..3 {
            testing_env!(context(caller.clone(), NearToken::ZERO).build());
            let before = env::storage_usage();
            assert!(std::panic::catch_unwind(|| match constructor {
                0 => Contract::new(alice(), contract_metadata()),
                1 => Contract::new_default_meta(alice(), "x".into(), "X".into(), None),
                _ => Contract::new_with_marketplace(alice(), contract_metadata(), bob()),
            })
            .is_err());
            assert_eq!(env::storage_usage(), before);
        }
    }
    testing_env!(context(contract_id(), NearToken::ZERO)
        .signer_account_id(alice())
        .build());
    assert!(std::panic::catch_unwind(|| Contract::new(alice(), contract_metadata())).is_err());
}

#[test]
fn previous_binding_root_is_unchanged_and_epoch_survives_restart() {
    let mut c = setup();
    mint(&mut c, "persist", alice());
    let root = borsh::to_vec(&c).unwrap();
    let old: crate::migration::BindingV2State = borsh::from_slice(&root).unwrap();
    assert_eq!(root, borsh::to_vec(&old).unwrap());
    let (v, map) = send(&mut c, "persist");
    let mut reopened: Contract = borsh::from_slice(&borsh::to_vec(&c).unwrap()).unwrap();
    assert!(!resolve(&mut reopened, "persist", v, map));
    assert!(Version::load("persist").unwrap().ownership > v.ownership);
}

#[test]
fn legacy_transfer_call_requires_funded_version_and_corruption_cannot_reset_it() {
    testing_env!(context(contract_id(), NearToken::ZERO).build());
    let mut old = previous_layout_fixture();
    old.ever_minted.insert(&"legacy".into(), &true);
    old.tokens.internal_mint_with_refund(
        "legacy".into(),
        alice(),
        Some(token_metadata("legacy")),
        None,
    );
    let mut c =
        crate::migration::prepare_binding_v2(&borsh::to_vec(&old).unwrap(), &contract_id(), bob())
            .unwrap();
    next_receipt(alice(), one_yocto());
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(c.nft_transfer_call(
            bob(),
            "legacy".into(),
            None,
            None,
            String::new()
        ))))
        .is_err()
    );
    assert!(Version::load("legacy").is_none());
    assert_eq!(c.nft_token("legacy".into()).unwrap().owner_id, alice());
    next_receipt(alice(), NearToken::from_near(1));
    c.nft_approve("legacy".into(), bob(), None);
    assert_eq!(
        Version::load("legacy"),
        Some(Version {
            ownership: 0,
            approval: 1
        })
    );
    let mut key = b"nft:mutation:v1:".to_vec();
    key.extend(env::sha256(b"legacy"));
    for corrupt in [vec![], vec![0; 15], vec![0; 17]] {
        env::storage_write(&key, &corrupt);
        next_receipt(alice(), one_yocto());
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.nft_transfer(
                bob(),
                "legacy".into(),
                None,
                None
            )))
            .is_err()
        );
        assert_eq!(env::storage_read(&key), Some(corrupt));
        assert_eq!(c.nft_token("legacy".into()).unwrap().owner_id, alice());
    }
}
