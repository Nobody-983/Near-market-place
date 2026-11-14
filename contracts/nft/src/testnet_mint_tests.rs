use super::*;
use std::panic::{catch_unwind, AssertUnwindSafe};

fn configured() -> Contract {
    testing_env!(context(contract_id(), NearToken::ZERO).build());
    Contract::new_testnet(alice(), contract_metadata(), account("market.testnet"))
}

#[test]
fn testnet_mint_authority_is_immutable_self_only_and_storage_funded() {
    let mut c = configured();
    assert_eq!(c.testnet_mint_authority(), Some(alice()));
    let root = near_sdk::borsh::to_vec(&c).unwrap();
    let _: crate::migration::BindingV2State = near_sdk::borsh::from_slice(&root).unwrap();
    for (caller, recipient) in [(bob(), alice()), (alice(), bob())] {
        testing_env!(context(caller, NearToken::from_near(1)).build());
        assert!(catch_unwind(AssertUnwindSafe(|| c.nft_mint(
            "bad".into(),
            recipient,
            token_metadata("bad")
        )))
        .is_err());
        assert!(c.token_id_status("bad".into()).is_none());
    }
    testing_env!(context(alice(), NearToken::from_near(1)).build());
    let token = "é".repeat(512);
    c.nft_mint(token.clone(), alice(), token_metadata("test"));
    assert_eq!(c.token_id_status(token.clone()), Some(TokenIdStatus::Live));
    let mut c: Contract =
        near_sdk::borsh::from_slice(&near_sdk::borsh::to_vec(&c).unwrap()).unwrap();
    assert!(catch_unwind(AssertUnwindSafe(|| c.nft_mint(
        token,
        alice(),
        token_metadata("test")
    )))
    .is_err());
}

#[test]
fn testnet_mint_rejects_foreign_network_and_underfunded_storage() {
    testing_env!(context(contract_id(), NearToken::ZERO).build());
    assert!(catch_unwind(|| Contract::new_testnet(
        account("alice.near"),
        contract_metadata(),
        account("market.testnet")
    ))
    .is_err());
    let mut c = configured();
    testing_env!(context(alice(), one_yocto()).build());
    assert!(catch_unwind(AssertUnwindSafe(|| c.nft_mint(
        "x".repeat(1024),
        alice(),
        token_metadata("test")
    )))
    .is_err());
    // Native SDK mock retains partial writes after panic; no rollback claim.
}

#[test]
fn permissionless_testnet_minting_allows_any_wallet_self_mint_and_enforces_caller_equals_owner() {
    let mut c = configured();

    // 1. Wallet A can mint
    testing_env!(context(alice(), NearToken::from_near(1)).build());
    let token_a = c.nft_mint("token-a".into(), alice(), token_metadata("alice-nft"));
    assert_eq!(token_a.owner_id, alice());

    // 2. Wallet B can mint
    testing_env!(context(bob(), NearToken::from_near(1)).build());
    let token_b = c.nft_mint("token-b".into(), bob(), token_metadata("bob-nft"));
    assert_eq!(token_b.owner_id, bob());

    // 3. Each wallet initially owns its own NFT
    assert_eq!(c.nft_token("token-a".into()).unwrap().owner_id, alice());
    assert_eq!(c.nft_token("token-b".into()).unwrap().owner_id, bob());

    // Unauthorized arbitrary recipient: Wallet A cannot mint to Wallet B
    testing_env!(context(alice(), NearToken::from_near(1)).build());
    assert!(catch_unwind(AssertUnwindSafe(|| c.nft_mint(
        "token-c".into(),
        bob(),
        token_metadata("unauthorized-recipient")
    )))
    .is_err());
    assert!(c.token_id_status("token-c".into()).is_none());

    // Unauthorized arbitrary recipient: Wallet B cannot mint to Wallet A
    testing_env!(context(bob(), NearToken::from_near(1)).build());
    assert!(catch_unwind(AssertUnwindSafe(|| c.nft_mint(
        "token-d".into(),
        alice(),
        token_metadata("unauthorized-recipient-2")
    )))
    .is_err());
    assert!(c.token_id_status("token-d".into()).is_none());

    // Non-testnet caller is rejected
    testing_env!(context(account("attacker.near"), NearToken::from_near(1)).build());
    assert!(catch_unwind(AssertUnwindSafe(|| c.nft_mint(
        "token-e".into(),
        account("attacker.near"),
        token_metadata("non-testnet")
    )))
    .is_err());
}
