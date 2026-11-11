use near_marketplace_foundation::{
    isolation::{
        artifact_environment, verify_deployment, verify_test_isolation, DeploymentIdentity,
        Environment, ResourceInventory,
    },
    Error,
};

fn identity(environment: Environment) -> DeploymentIdentity {
    DeploymentIdentity {
        environment,
        network_id: match environment {
            Environment::Testnet => "testnet",
            Environment::Production => "mainnet",
        }
        .into(),
        genesis_hash: [1; 32],
        nft: "nft.example".parse().unwrap(),
        marketplace: "market.example".parse().unwrap(),
        nft_code_hash: [2; 32],
        market_code_hash: [3; 32],
        abi_hash: [4; 32],
    }
}

fn inventory(offset: u8) -> ResourceInventory {
    ResourceInventory {
        signing_keys: vec![[offset; 32]],
        accounts: vec![format!("account{offset}.example").parse().unwrap()],
        rpc_services: vec![[offset + 1; 32]],
        rpc_credentials: vec![[offset + 2; 32]],
        databases: vec![[offset + 3; 32]],
        storage: vec![[offset + 4; 32]],
        signer_processes: vec![[offset + 5; 32]],
    }
}

#[test]
fn environment_is_selected_by_artifact_not_runtime_config() {
    #[cfg(not(any(feature = "testnet", feature = "production")))]
    assert_eq!(artifact_environment(), Err(Error::MissingConfiguration));
    #[cfg(feature = "testnet")]
    assert_eq!(artifact_environment(), Ok(Environment::Testnet));
    #[cfg(feature = "production")]
    assert_eq!(artifact_environment(), Ok(Environment::Production));
}

#[test]
fn identity_verification_fails_closed_on_missing_mode_or_mismatch() {
    let Ok(environment) = artifact_environment() else {
        let expected = identity(Environment::Testnet);
        assert_eq!(
            verify_deployment(&expected, &expected),
            Err(Error::MissingConfiguration)
        );
        return;
    };
    let expected = identity(environment);
    assert_eq!(verify_deployment(&expected, &expected), Ok(()));
    for field in 0..8 {
        let mut observed = expected.clone();
        match field {
            0 => observed.genesis_hash = [9; 32],
            1 => observed.nft_code_hash = [9; 32],
            2 => observed.market_code_hash = [9; 32],
            3 => observed.abi_hash = [9; 32],
            4 => observed.nft = "different.example".parse().unwrap(),
            5 => observed.marketplace = "different.example".parse().unwrap(),
            6 => observed.network_id = "another-chain".into(),
            _ => {
                observed.environment = match environment {
                    Environment::Testnet => Environment::Production,
                    Environment::Production => Environment::Testnet,
                }
            }
        }
        assert!(verify_deployment(&expected, &observed).is_err());
    }
    let mut empty = expected.clone();
    empty.genesis_hash = [0; 32];
    assert_eq!(
        verify_deployment(&empty, &empty),
        Err(Error::InvalidIdentity)
    );
    let mut aliased = expected;
    aliased.nft = aliased.marketplace.clone();
    assert_eq!(
        verify_deployment(&aliased, &aliased),
        Err(Error::InvalidIdentity)
    );
}

#[test]
fn shared_production_resources_reject_in_every_category() {
    let production = inventory(20);
    assert_eq!(verify_test_isolation(&inventory(1), &production), Ok(()));
    for category in 0..7 {
        let mut test = inventory(1);
        match category {
            0 => test.signing_keys = production.signing_keys.clone(),
            1 => test.accounts = production.accounts.clone(),
            2 => test.rpc_services = production.rpc_services.clone(),
            3 => test.rpc_credentials = production.rpc_credentials.clone(),
            4 => test.databases = production.databases.clone(),
            5 => test.storage = production.storage.clone(),
            _ => test.signer_processes = production.signer_processes.clone(),
        }
        assert_eq!(
            verify_test_isolation(&test, &production),
            Err(Error::InvalidIdentity)
        );
    }
}

#[test]
fn missing_inventory_category_or_zero_fingerprint_rejects() {
    let production = inventory(20);
    for category in 0..7 {
        let mut test = inventory(1);
        match category {
            0 => test.signing_keys.clear(),
            1 => test.accounts.clear(),
            2 => test.rpc_services.clear(),
            3 => test.rpc_credentials.clear(),
            4 => test.databases.clear(),
            5 => test.storage.clear(),
            _ => test.signer_processes.clear(),
        }
        assert_eq!(
            verify_test_isolation(&test, &production),
            Err(Error::MissingConfiguration)
        );
    }
    let mut test = inventory(1);
    test.signing_keys = vec![[0; 32]];
    assert_eq!(
        verify_test_isolation(&test, &production),
        Err(Error::InvalidIdentity)
    );
}
