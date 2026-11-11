use super::*;
use crate::migration::{check_source_layout, prepare_binding_v2, MigrationError, SourceLayout};
use near_sdk::{
    borsh,
    json_types::U64,
    test_utils::{get_created_receipts, get_logs},
};

fn marketplace() -> AccountId {
    account("market.testnet")
}

fn bound_contract() -> Contract {
    testing_env!(context(contract_id(), NearToken::ZERO).build());
    Contract::new_with_marketplace(alice(), contract_metadata(), marketplace())
}

fn approved_contract() -> Contract {
    let mut contract = bound_contract();
    mint(&mut contract, "sale", alice());
    approve(&mut contract, "sale");
    contract
}

fn approve(contract: &mut Contract, id: &str) {
    next_receipt(alice(), NearToken::from_near(1));
    contract.nft_approve(id.into(), marketplace(), None);
}

fn transfer(contract: &mut Contract, id: u64) -> bool {
    contract.marketplace_transfer_approved(alice(), "sale".into(), U64(id), bob())
}

// Failed authorization must not depend on simulated receipt rollback. The SDK
// mock retains writes on panic, so compare token/root/storage/logs directly.
fn rejected(contract: &mut Contract, message: &str, action: impl FnOnce(&mut Contract)) {
    let root = borsh::to_vec(contract).unwrap();
    let tokens = serde_json::to_value(contract.nft_tokens(None, Some(100))).unwrap();
    let usage = env::storage_usage();
    let logs = get_logs();
    let receipts = get_created_receipts().len();
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| action(contract)))
        .expect_err("must reject");
    let text = error
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| error.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(text.contains(message), "expected {message:?}, got {text:?}");
    assert_eq!(borsh::to_vec(contract).unwrap(), root);
    assert_eq!(
        serde_json::to_value(contract.nft_tokens(None, Some(100))).unwrap(),
        tokens
    );
    assert_eq!(env::storage_usage(), usage);
    assert_eq!(get_logs(), logs);
    assert_eq!(get_created_receipts().len(), receipts);
}

#[test]
fn wrapper_authenticates_predecessor_not_signer() {
    let mut contract = approved_contract();
    for caller in [alice(), bob(), contract_id()] {
        testing_env!(context(caller, one_yocto())
            .signer_account_id(marketplace())
            .build());
        rejected(&mut contract, "Only the configured marketplace", |c| {
            transfer(c, 1);
        });
    }
    testing_env!(context(marketplace(), one_yocto())
        .signer_account_id(bob())
        .build());
    assert!(transfer(&mut contract, 1));
}

#[test]
fn binding_initialization_is_explicit_and_immutable() {
    let mut unbound = setup();
    assert_eq!(unbound.marketplace_id(), None);
    next_receipt(marketplace(), one_yocto());
    rejected(&mut unbound, "NFT is not marketplace-bound", |c| {
        transfer(c, 1);
    });
    testing_env!(context(alice(), NearToken::ZERO).build());
    assert!(std::panic::catch_unwind(|| Contract::new_with_marketplace(
        alice(),
        contract_metadata(),
        marketplace()
    ))
    .is_err());
    testing_env!(context(contract_id(), NearToken::ZERO).build());
    assert!(std::panic::catch_unwind(|| Contract::new_with_marketplace(
        alice(),
        contract_metadata(),
        contract_id()
    ))
    .is_err());
    let bound = bound_contract();
    assert_eq!(bound.marketplace_id(), Some(marketplace()));
}

#[test]
fn wrapper_requires_exactly_one_yocto() {
    let mut contract = approved_contract();
    for deposit in [NearToken::ZERO, NearToken::from_yoctonear(2)] {
        next_receipt(marketplace(), deposit);
        rejected(&mut contract, "exactly 1 yoctoNEAR", |c| {
            transfer(c, 1);
        });
    }
}

#[test]
fn wrong_seller_receiver_token_and_approval_reject_without_writes() {
    let mut contract = approved_contract();
    mint(&mut contract, "other", alice());
    next_receipt(marketplace(), one_yocto());
    rejected(&mut contract, "Expected seller", |c| {
        c.marketplace_transfer_approved(bob(), "sale".into(), U64(1), alice());
    });
    for receiver in [alice(), marketplace()] {
        rejected(&mut contract, "Invalid sale receiver", |c| {
            c.marketplace_transfer_approved(alice(), "sale".into(), U64(1), receiver);
        });
    }
    rejected(&mut contract, "Token not found", |c| {
        c.marketplace_transfer_approved(alice(), "absent".into(), U64(1), bob());
    });
    rejected(&mut contract, "Exact active", |c| {
        c.marketplace_transfer_approved(alice(), "other".into(), U64(1), bob());
    });
    for id in [0, 2, u64::MAX] {
        rejected(&mut contract, "Exact active", |c| {
            transfer(c, id);
        });
    }
}

#[test]
fn missing_approval_support_fails_closed() {
    let mut contract = approved_contract();
    contract.tokens.approvals_by_id = None;
    next_receipt(marketplace(), one_yocto());
    rejected(&mut contract, "Approval extension required", |c| {
        transfer(c, 1);
    });
}

#[test]
fn revoked_and_reapproved_ids_never_substitute_for_an_old_operation() {
    let mut contract = approved_contract();
    next_receipt(alice(), one_yocto());
    contract.nft_revoke("sale".into(), marketplace());
    next_receipt(marketplace(), one_yocto());
    rejected(&mut contract, "Exact active", |c| {
        transfer(c, 1);
    });
    approve(&mut contract, "sale");
    next_receipt(marketplace(), one_yocto());
    rejected(&mut contract, "Exact active", |c| {
        transfer(c, 1);
    });
    assert!(transfer(&mut contract, 2));
}

#[test]
fn reapproval_without_revoke_and_revoke_all_invalidate_old_ids() {
    let mut contract = approved_contract();
    approve(&mut contract, "sale");
    next_receipt(marketplace(), one_yocto());
    rejected(&mut contract, "Exact active", |c| {
        transfer(c, 1);
    });
    next_receipt(alice(), one_yocto());
    contract.nft_revoke_all("sale".into());
    next_receipt(marketplace(), one_yocto());
    rejected(&mut contract, "Exact active", |c| {
        transfer(c, 2);
    });
    approve(&mut contract, "sale");
    next_receipt(marketplace(), one_yocto());
    assert!(transfer(&mut contract, 3));
}

#[test]
fn direct_deposit_cannot_exploit_sdk_owner_approval_bypass() {
    let mut contract = approved_contract();
    next_receipt(alice(), one_yocto());
    contract.nft_transfer(marketplace(), "sale".into(), None, None);
    next_receipt(marketplace(), one_yocto());
    rejected(&mut contract, "Marketplace custody", |c| {
        c.marketplace_transfer_approved(marketplace(), "sale".into(), U64(1), bob());
    });
    rejected(&mut contract, "Expected seller", |c| {
        transfer(c, 1);
    });
    // Even a marketplace self-approval does not turn custody into sale authority.
    next_receipt(marketplace(), NearToken::from_near(1));
    contract.nft_approve("sale".into(), marketplace(), None);
    next_receipt(marketplace(), one_yocto());
    rejected(&mut contract, "Marketplace custody", |c| {
        c.marketplace_transfer_approved(marketplace(), "sale".into(), U64(2), bob());
    });
}

#[test]
fn success_is_synchronous_true_clears_approvals_and_preserves_identity() {
    let mut contract = approved_contract();
    next_receipt(alice(), NearToken::from_near(1));
    contract.nft_approve("sale".into(), account("another.testnet"), None);
    next_receipt(marketplace(), one_yocto());
    let result = transfer(&mut contract, 1);
    assert_eq!(serde_json::to_vec(&result).unwrap(), b"true");
    assert!(get_created_receipts().is_empty());
    assert_eq!(contract.nft_supply_for_owner(alice()), U128(0));
    assert_eq!(contract.nft_supply_for_owner(bob()), U128(1));
    assert_eq!(contract.nft_total_supply(), U128(1));
    assert_eq!(
        contract
            .nft_token("sale".into())
            .unwrap()
            .approved_account_ids,
        Some(HashMap::new())
    );
    assert_eq!(
        contract.token_id_status("sale".into()),
        Some(TokenIdStatus::Live)
    );
    assert_eq!(
        contract
            .tokens
            .next_approval_id_by_id
            .as_ref()
            .unwrap()
            .get(&"sale".into()),
        Some(3)
    );
    let log = get_logs();
    assert_eq!(log.len(), 1);
    let event: serde_json::Value =
        serde_json::from_str(log[0].strip_prefix("EVENT_JSON:").unwrap()).unwrap();
    assert_eq!(event["standard"], "nep171");
    assert_eq!(event["event"], "nft_transfer");
    assert_eq!(event["data"][0]["old_owner_id"], "alice.testnet");
    assert_eq!(event["data"][0]["new_owner_id"], "bob.testnet");
    rejected(&mut contract, "Expected seller", |c| {
        transfer(c, 1);
    });
    next_receipt(bob(), one_yocto());
    contract.nft_transfer(alice(), "sale".into(), None, None);
    next_receipt(marketplace(), one_yocto());
    rejected(&mut contract, "Exact active", |c| {
        transfer(c, 1);
    });
}

#[test]
fn approval_ids_use_lossless_decimal_strings_in_generated_wrapper_calls() {
    testing_env!(context(marketplace(), one_yocto()).build());
    for id in [0, (1u64 << 53) - 1, 1u64 << 53, (1u64 << 53) + 1, u64::MAX] {
        let bytes = serde_json::to_vec(&U64(id)).unwrap();
        assert_eq!(bytes, format!("\"{id}\"").as_bytes());
        assert_eq!(serde_json::from_slice::<U64>(&bytes).unwrap().0, id);
        assert!(serde_json::from_str::<U64>(&id.to_string()).is_err());
        next_receipt(marketplace(), one_yocto());
        Contract::ext(contract_id())
            .with_attached_deposit(one_yocto())
            .marketplace_transfer_approved(alice(), "sale".into(), U64(id), bob())
            .detach();
        let receipts = get_created_receipts();
        let args = receipts
            .iter()
            .flat_map(|r| &r.actions)
            .find_map(|a| match a {
                near_sdk::mock::MockAction::FunctionCallWeight {
                    method_name, args, ..
                } => {
                    assert_eq!(method_name, b"marketplace_transfer_approved");
                    Some(args)
                }
                _ => None,
            })
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(args).unwrap();
        assert_eq!(json["approval_id"], id.to_string());
        assert_eq!(json["expected_seller_id"], "alice.testnet");
        assert_eq!(json["receiver_id"], "bob.testnet");
    }
    for invalid in [
        "null",
        "-1",
        "1.5",
        "\"18446744073709551616\"",
        "\"-1\"",
        "\"1.0\"",
    ] {
        assert!(serde_json::from_str::<U64>(invalid).is_err());
    }
}

#[test]
fn full_width_active_id_is_compared_exactly_and_exhaustion_is_preserved() {
    let mut contract = approved_contract();
    // Audited legacy fixture can contain an active MAX. The current issuer stops
    // at MAX-1 because it must retain a representable next counter.
    contract
        .tokens
        .approvals_by_id
        .as_mut()
        .unwrap()
        .insert(&"sale".into(), &HashMap::from([(marketplace(), u64::MAX)]));
    contract
        .tokens
        .next_approval_id_by_id
        .as_mut()
        .unwrap()
        .insert(&"sale".into(), &u64::MAX);
    next_receipt(marketplace(), one_yocto());
    rejected(&mut contract, "Exact active", |c| {
        transfer(c, u64::MAX - 1);
    });
    assert!(transfer(&mut contract, u64::MAX));
    next_receipt(bob(), NearToken::from_near(1));
    rejected(&mut contract, "Approval ID exhausted", |c| {
        c.nft_approve("sale".into(), marketplace(), None);
    });
}

#[test]
fn standard_approval_hook_keeps_its_lossless_numeric_u64_abi() {
    let mut contract = approved_contract();
    let next = (1u64 << 53) + 1;
    contract
        .tokens
        .next_approval_id_by_id
        .as_mut()
        .unwrap()
        .insert(&"sale".into(), &next);
    next_receipt(alice(), NearToken::from_near(1));
    let hook = contract.nft_approve("sale".into(), marketplace(), Some("terms".into()));
    drop(hook);
    let receipts = get_created_receipts();
    let args = receipts
        .iter()
        .flat_map(|r| &r.actions)
        .find_map(|a| match a {
            near_sdk::mock::MockAction::FunctionCallWeight {
                method_name, args, ..
            } => {
                assert_eq!(method_name, b"nft_on_approve");
                Some(args)
            }
            _ => None,
        })
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(args).unwrap();
    assert_eq!(json["approval_id"].as_u64(), Some(next));
    assert_eq!(json["owner_id"], "alice.testnet");
    next_receipt(marketplace(), one_yocto());
    rejected(&mut contract, "Exact active", |c| {
        transfer(c, next - 1);
    });
    assert!(transfer(&mut contract, next));
}

#[test]
fn token_and_message_limits_use_bytes_and_reject_before_counter_writes() {
    let mut contract = approved_contract();
    next_receipt(marketplace(), one_yocto());
    for id in ["".to_string(), " ".to_string(), "é".repeat(513)] {
        rejected(&mut contract, "token_id", |c| {
            c.marketplace_transfer_approved(alice(), id, U64(1), bob());
        });
    }
    next_receipt(alice(), NearToken::from_near(1));
    rejected(&mut contract, "Approval message", |c| {
        c.nft_approve(
            "sale".into(),
            marketplace(),
            Some("é".repeat(MAX_APPROVAL_MSG_BYTES / 2 + 1)),
        );
    });
    assert_eq!(
        contract
            .tokens
            .next_approval_id_by_id
            .as_ref()
            .unwrap()
            .get(&"sale".into()),
        Some(2)
    );
    let promise = contract.nft_approve(
        "sale".into(),
        marketplace(),
        Some("x".repeat(MAX_APPROVAL_MSG_BYTES)),
    );
    assert!(promise.is_some());
    drop(promise);
    let long = "é".repeat(MAX_TOKEN_ID_BYTES / 2);
    mint(&mut contract, &long, alice());
    approve(&mut contract, &long);
    next_receipt(marketplace(), one_yocto());
    assert!(contract.marketplace_transfer_approved(alice(), long, U64(1), bob()));
}

#[test]
fn approval_limit_allows_replacement_and_revocation_but_not_growth() {
    let mut contract = approved_contract();
    for i in 1..MAX_APPROVALS_PER_TOKEN {
        next_receipt(alice(), NearToken::from_near(1));
        contract.nft_approve(
            "sale".into(),
            account(&format!("approved{i}.testnet")),
            None,
        );
    }
    next_receipt(alice(), NearToken::from_near(1));
    rejected(&mut contract, "Approval count limit", |c| {
        c.nft_approve("sale".into(), bob(), None);
    });
    contract.nft_approve("sale".into(), marketplace(), None);
    next_receipt(marketplace(), one_yocto());
    rejected(&mut contract, "Exact active", |c| {
        transfer(c, 1);
    });
    assert!(transfer(&mut contract, MAX_APPROVALS_PER_TOKEN as u64 + 1));
}

#[test]
fn oversized_legacy_map_fails_wrapper_but_owner_can_clear_it() {
    let mut contract = approved_contract();
    let mut map = HashMap::from([(marketplace(), 1)]);
    for i in 0..MAX_APPROVALS_PER_TOKEN {
        map.insert(account(&format!("legacy{i}.testnet")), i as u64 + 2);
    }
    contract
        .tokens
        .approvals_by_id
        .as_mut()
        .unwrap()
        .insert(&"sale".into(), &map);
    contract
        .tokens
        .next_approval_id_by_id
        .as_mut()
        .unwrap()
        .insert(&"sale".into(), &100);
    next_receipt(marketplace(), one_yocto());
    rejected(&mut contract, "Approval count limit", |c| {
        transfer(c, 1);
    });
    next_receipt(alice(), NearToken::from_near(1));
    rejected(&mut contract, "Approval count limit", |c| {
        c.nft_approve("sale".into(), marketplace(), None);
    });
    next_receipt(alice(), one_yocto());
    contract.nft_revoke_all("sale".into());
    approve(&mut contract, "sale");
    next_receipt(marketplace(), one_yocto());
    assert!(transfer(&mut contract, 100));
}

#[test]
fn raw_oversized_truncated_and_noncanonical_approval_maps_fail_closed() {
    let mut contract = approved_contract();
    let mut corrupt_maps = vec![vec![0; 10_000], vec![1, 0], u32::MAX.to_le_bytes().to_vec()];
    let mut duplicate = 2u32.to_le_bytes().to_vec();
    for _ in 0..2 {
        duplicate.extend(borsh::to_vec(&marketplace()).unwrap());
        duplicate.extend(1u64.to_le_bytes());
    }
    corrupt_maps.push(duplicate);
    for bytes in corrupt_maps {
        contract
            .tokens
            .approvals_by_id
            .as_mut()
            .unwrap()
            .insert_raw(&borsh::to_vec(&"sale").unwrap(), &bytes);
        next_receipt(marketplace(), one_yocto());
        let usage = env::storage_usage();
        let before = env::storage_read(b"a\x04\0\0\0sale").unwrap();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| transfer(&mut contract, 1)))
                .is_err()
        );
        assert_eq!(env::storage_read(b"a\x04\0\0\0sale").unwrap(), before);
        assert_eq!(env::storage_usage(), usage);
        assert_eq!(
            contract.tokens.owner_by_id.get(&"sale".into()),
            Some(alice())
        );
        assert!(get_logs().is_empty());
    }
}

#[test]
fn frozen_previous_layout_conversion_preserves_live_and_burned_records() {
    testing_env!(context(alice(), NearToken::from_near(10)).build());
    let mut previous = previous_layout_fixture();
    let live = "live".to_string();
    let burned = "historical-burn".to_string();
    let legacy_long = "L".repeat(MAX_TOKEN_ID_BYTES + 1);
    previous.tokens.internal_mint_with_refund(
        live.clone(),
        alice(),
        Some(token_metadata("live")),
        None,
    );
    for id in [&live, &burned, &legacy_long] {
        previous.ever_minted.insert(id, &true);
    }
    previous
        .tokens
        .approvals_by_id
        .as_mut()
        .unwrap()
        .insert(&live, &HashMap::from([(marketplace(), (1u64 << 53) + 1)]));
    previous
        .tokens
        .next_approval_id_by_id
        .as_mut()
        .unwrap()
        .insert(&live, &((1u64 << 53) + 2));
    previous
        .tokens
        .next_approval_id_by_id
        .as_mut()
        .unwrap()
        .insert(&burned, &u64::MAX);
    let source = borsh::to_vec(&previous).unwrap();
    let before = env::storage_usage();
    let mut converted = prepare_binding_v2(&source, &contract_id(), marketplace()).unwrap();
    assert_eq!(env::storage_usage(), before); // conversion writes no storage
    let target = borsh::to_vec(&converted).unwrap();
    assert_eq!(&target[..source.len()], &source);
    assert_eq!(
        &target[source.len()..],
        borsh::to_vec(&Some(marketplace())).unwrap()
    );
    assert_eq!(
        check_source_layout(&source, SourceLayout::RegistryV1),
        Ok(())
    );
    assert_eq!(
        check_source_layout(&target, SourceLayout::BindingV2),
        Ok(())
    );
    assert!(prepare_binding_v2(&target, &contract_id(), marketplace()).is_err());
    assert_eq!(
        check_source_layout(&source, SourceLayout::BindingV2),
        Err(MigrationError::LayoutMismatch)
    );
    assert_eq!(converted.marketplace_id(), Some(marketplace()));
    assert_eq!(converted.nft_metadata().name, contract_metadata().name);
    assert_eq!(converted.nft_token(live.clone()).unwrap().owner_id, alice());
    assert_eq!(
        converted
            .nft_token(live.clone())
            .unwrap()
            .metadata
            .unwrap()
            .title,
        Some("live".into())
    );
    assert_eq!(converted.nft_supply_for_owner(alice()), U128(1));
    assert_eq!(converted.nft_total_supply(), U128(1));
    for id in [&burned, &legacy_long] {
        assert_eq!(
            converted.token_id_status(id.clone()),
            Some(TokenIdStatus::Burned)
        );
        assert!(converted.nft_token(id.clone()).is_none());
    }
    assert_eq!(
        converted
            .tokens
            .next_approval_id_by_id
            .as_ref()
            .unwrap()
            .get(&burned),
        Some(u64::MAX)
    );
    next_receipt(alice(), NearToken::from_near(1));
    rejected(&mut converted, "never reused", |c| {
        c.mint_once(burned, alice(), token_metadata("retry"));
    });
    next_receipt(marketplace(), one_yocto());
    assert!(converted.marketplace_transfer_approved(
        alice(),
        live.clone(),
        U64((1u64 << 53) + 1),
        bob()
    ));
    assert_eq!(
        converted
            .tokens
            .next_approval_id_by_id
            .as_ref()
            .unwrap()
            .get(&live),
        Some((1u64 << 53) + 2)
    );
    let roundtrip: Contract = borsh::from_slice(&borsh::to_vec(&converted).unwrap()).unwrap();
    assert_eq!(roundtrip.nft_token(live).unwrap().owner_id, bob());
}

#[test]
fn migration_rejects_truncation_trailing_bytes_and_self_binding() {
    testing_env!(context(alice(), NearToken::ZERO).build());
    let bytes = borsh::to_vec(&previous_layout_fixture()).unwrap();
    for end in 0..bytes.len() {
        assert!(prepare_binding_v2(&bytes[..end], &contract_id(), marketplace()).is_err());
    }
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(prepare_binding_v2(&extra, &contract_id(), marketplace()).is_err());
    assert!(matches!(
        prepare_binding_v2(&bytes, &contract_id(), contract_id()),
        Err(MigrationError::InvalidBinding)
    ));
}

#[test]
fn migrated_oversized_live_id_preserves_standard_transfer_access() {
    testing_env!(context(alice(), NearToken::from_near(10)).build());
    let mut old = previous_layout_fixture();
    let id = "L".repeat(MAX_TOKEN_ID_BYTES + 1);
    old.ever_minted.insert(&id, &true);
    old.tokens
        .internal_mint_with_refund(id.clone(), alice(), Some(token_metadata("legacy")), None);
    let mut converted =
        prepare_binding_v2(&borsh::to_vec(&old).unwrap(), &contract_id(), marketplace()).unwrap();
    assert_eq!(converted.nft_token(id.clone()).unwrap().owner_id, alice());
    next_receipt(alice(), NearToken::from_near(1));
    rejected(&mut converted, "token_id exceeds", |c| {
        c.nft_approve(id.clone(), marketplace(), None);
    });
    next_receipt(marketplace(), one_yocto());
    rejected(&mut converted, "token_id exceeds", |c| {
        c.marketplace_transfer_approved(alice(), id.clone(), U64(1), bob());
    });
    next_receipt(alice(), one_yocto());
    converted.nft_transfer(bob(), id.clone(), None, None);
    assert_eq!(converted.nft_token(id.clone()).unwrap().owner_id, bob());
    assert_eq!(converted.token_id_status(id), Some(TokenIdStatus::Live));
}

#[test]
fn resolver_restoration_does_not_lower_counter_or_override_later_reapproval() {
    let mut contract = approved_contract();
    let old_approvals = contract
        .tokens
        .approvals_by_id
        .as_ref()
        .unwrap()
        .get(&"sale".into());
    next_receipt(alice(), one_yocto());
    let promise = contract.nft_transfer_call(bob(), "sale".into(), None, None, "return".into());
    drop(promise);
    let version = crate::mutation::Version::load("sale").unwrap();
    next_receipt(bob(), NearToken::from_near(1));
    contract.nft_approve("sale".into(), marketplace(), None);
    testing_env!(
        context(contract_id(), NearToken::ZERO).build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        vec![near_sdk::PromiseResult::Successful(b"true".to_vec())]
    );
    assert!(!contract.nft_resolve_transfer_versioned(
        alice(),
        bob(),
        "sale".into(),
        old_approvals,
        U64(version.ownership),
        U64(version.approval)
    ));
    assert_eq!(
        contract
            .tokens
            .next_approval_id_by_id
            .as_ref()
            .unwrap()
            .get(&"sale".into()),
        Some(3)
    );
    // Receiver mutations supersede the old snapshot; neither ID is resurrected.
    assert!(!contract.nft_is_approved("sale".into(), marketplace(), Some(1)));
    approve(&mut contract, "sale");
    next_receipt(marketplace(), one_yocto());
    rejected(&mut contract, "Exact active", |c| {
        transfer(c, 1);
    });
    rejected(&mut contract, "Exact active", |c| {
        transfer(c, 2);
    });
    assert!(transfer(&mut contract, 3));
}

#[cfg(feature = "vm-tests")]
#[path = "wasm_binding_tests.rs"]
mod wasm;
