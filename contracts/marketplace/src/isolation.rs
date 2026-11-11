//! Identity comparisons only: a trusted RPC/deployment verifier must establish
//! observations. Account suffixes and caller-supplied network strings are not proof.
use crate::{AccountId, Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Environment {
    Testnet,
    Production,
}

pub fn artifact_environment() -> Result<Environment> {
    #[cfg(feature = "production")]
    {
        Ok(Environment::Production)
    }
    #[cfg(all(feature = "testnet", not(feature = "production")))]
    {
        Ok(Environment::Testnet)
    }
    #[cfg(not(any(feature = "testnet", feature = "production")))]
    {
        Err(Error::MissingConfiguration)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeploymentIdentity {
    pub environment: Environment,
    pub network_id: String,
    pub genesis_hash: [u8; 32],
    pub nft: AccountId,
    pub marketplace: AccountId,
    pub nft_code_hash: [u8; 32],
    pub market_code_hash: [u8; 32],
    pub abi_hash: [u8; 32],
}

pub fn verify_deployment(
    expected: &DeploymentIdentity,
    observed: &DeploymentIdentity,
) -> Result<()> {
    let artifact = artifact_environment()?;
    for identity in [expected, observed] {
        let network = match identity.environment {
            Environment::Testnet => "testnet",
            Environment::Production => "mainnet",
        };
        if identity.environment != artifact || identity.network_id != network {
            return Err(Error::WrongEnvironment);
        }
        if identity.nft == identity.marketplace
            || [
                identity.genesis_hash,
                identity.nft_code_hash,
                identity.market_code_hash,
                identity.abi_hash,
            ]
            .contains(&[0; 32])
        {
            return Err(Error::InvalidIdentity);
        }
    }
    if expected != observed {
        return Err(Error::InvalidIdentity);
    }
    Ok(())
}

/// Fingerprints of actual, canonical resources, not user-selected labels or URL
/// spellings. The future verifier must discover aliases and validate completeness.
/// Digests are public identifiers; never pass or log secret credential material.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceInventory {
    pub signing_keys: Vec<[u8; 32]>,
    pub accounts: Vec<AccountId>,
    pub rpc_services: Vec<[u8; 32]>,
    pub rpc_credentials: Vec<[u8; 32]>,
    pub databases: Vec<[u8; 32]>,
    pub storage: Vec<[u8; 32]>,
    pub signer_processes: Vec<[u8; 32]>,
}

impl ResourceInventory {
    fn fingerprints(&self) -> Result<Vec<[u8; 32]>> {
        let groups = [
            &self.signing_keys,
            &self.rpc_services,
            &self.rpc_credentials,
            &self.databases,
            &self.storage,
            &self.signer_processes,
        ];
        if self.accounts.is_empty() || groups.iter().any(|group| group.is_empty()) {
            return Err(Error::MissingConfiguration);
        }
        let all: Vec<_> = groups.into_iter().flatten().copied().collect();
        if all.contains(&[0; 32]) {
            return Err(Error::InvalidIdentity);
        }
        Ok(all)
    }
}

/// Inventory comparison, not filesystem isolation or proof of RPC identity.
/// No network/credential/environment files are read or changed here.
pub fn verify_test_isolation(
    test: &ResourceInventory,
    production: &ResourceInventory,
) -> Result<()> {
    let test_ids = test.fingerprints()?;
    let production_ids = production.fingerprints()?;
    if test
        .accounts
        .iter()
        .any(|account| production.accounts.contains(account))
        || test_ids.iter().any(|id| production_ids.contains(id))
    {
        return Err(Error::InvalidIdentity);
    }
    Ok(())
}
