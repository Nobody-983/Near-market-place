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
