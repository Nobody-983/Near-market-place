use std::collections::HashMap;

use near_contract_standards::non_fungible_token::{
    approval::ext_nft_approval_receiver,
    core::NonFungibleTokenCore,
    events::NftMint,
    metadata::{
        NFTContractMetadata, NonFungibleTokenMetadataProvider, TokenMetadata, NFT_METADATA_SPEC,
    },
    NonFungibleToken, NonFungibleTokenApproval, NonFungibleTokenEnumeration,
    NonFungibleTokenResolver, Token, TokenId,
};
use near_sdk::{
    collections::{LazyOption, LookupMap},
    env,
    json_types::{U128, U64},
    near, require, AccountId, Gas, PanicOnDefault, Promise, PromiseOrValue,
};

#[cfg(all(feature = "testnet", feature = "production"))]
compile_error!("testnet and production NFT artifacts must be built separately");
#[cfg(feature = "testnet")]
mod testnet_mint;
#[cfg(not(feature = "testnet"))]
const MINTING_ENABLED: bool = false;
const MAX_TOKEN_ID_BYTES: usize = 1_024;
// Execution bounds, not marketplace economic policy. Legacy oversized records
// retain standard transfer/revocation access, but cannot create new approvals or
// use the marketplace wrapper until the owner clears the approval map.
const MAX_APPROVALS_PER_TOKEN: usize = 32;
const MAX_APPROVAL_MSG_BYTES: usize = 4_096;

mod approval_storage;
pub mod migration;
mod mutation;
mod transfer_resolution;

#[near(serializers = [json])]
#[derive(Debug, PartialEq, Eq)]
pub enum TokenIdStatus {
    Live,
    Burned,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    tokens: NonFungibleToken,
    metadata: LazyOption<NFTContractMetadata>,
    // Independent of live ownership: never remove entries, including after burn.
    ever_minted: LookupMap<TokenId, bool>,
    // BindingV2 appends this field to frozen RegistryV1. No setter or migration
    // entry point. Old constructor ABIs initialize an unbound, fail-closed NFT.
    marketplace_id: Option<AccountId>,
}

#[near]
impl Contract {
    #[init]
    pub fn new(owner_id: AccountId, metadata: NFTContractMetadata) -> Self {
        require_self_initialization();
        metadata.assert_valid();

        Self {
            tokens: NonFungibleToken::new(b"o", owner_id, Some(b"m"), Some(b"e"), Some(b"a")),
            metadata: LazyOption::new(b"c", Some(&metadata)),
            ever_minted: LookupMap::new(b"used".to_vec()),
            marketplace_id: None,
        }
    }

    /// Fresh-account initialization only; deployment/initialization must be one
    /// reviewed atomic self-call. The generated #[init] guard rejects STATE.
    #[init]
    pub fn new_with_marketplace(
        owner_id: AccountId,
        metadata: NFTContractMetadata,
        marketplace_id: AccountId,
    ) -> Self {
        require!(
            env::predecessor_account_id() == env::current_account_id(),
            "Marketplace binding requires NFT self-initialization"
        );
        require!(
            marketplace_id != env::current_account_id(),
            "Marketplace must differ from NFT contract"
        );
        let mut contract = Self::new(owner_id, metadata);
        contract.marketplace_id = Some(marketplace_id);
        contract
    }

    pub fn marketplace_id(&self) -> Option<AccountId> {
        self.marketplace_id.clone()
    }

    /// Success is exactly JSON `true` (four bytes) from this synchronous receipt.
    /// Failure panics; no false value, promise, payout or storage-refund promise.
    #[payable]
    pub fn marketplace_transfer_approved(
        &mut self,
        expected_seller_id: AccountId,
        token_id: TokenId,
        approval_id: U64,
        receiver_id: AccountId,
    ) -> bool {
        near_sdk::assert_one_yocto();
        let marketplace = self
            .marketplace_id
            .as_ref()
            .expect("NFT is not marketplace-bound");
        require!(
            env::predecessor_account_id() == *marketplace,
            "Only the configured marketplace may call"
        );
        assert_bounded_token_id(&token_id);
        require!(
            expected_seller_id != *marketplace,
            "Marketplace custody is not sale authorization"
        );
        require!(
            receiver_id != expected_seller_id && receiver_id != *marketplace,
            "Invalid sale receiver"
        );
        let owner = self
            .tokens
            .owner_by_id
            .get(&token_id)
            .expect("Token not found");
        require!(
            owner == expected_seller_id,
            "Expected seller is not token owner"
        );
        let approvals = approval_storage::bounded_approvals(&self.tokens, &token_id);
        require!(
            approvals.get(marketplace) == Some(&approval_id.0),
            "Exact active marketplace approval required"
        );
        let version = mutation::Version::load(&token_id).map(|v| v.transfer_next());
        // All authorization checks above precede the SDK's approval removal.
        // Its owner-caller exception is never a substitute for our preflight.
        self.tokens.internal_transfer(
            marketplace,
            &receiver_id,
            &token_id,
            Some(approval_id.0),
            None,
        );
        if let Some(version) = version {
            version.write(&token_id);
        }
        true
    }

    #[init]
    pub fn new_default_meta(
        owner_id: AccountId,
        name: String,
        symbol: String,
        icon: Option<String>,
    ) -> Self {
        Self::new(
            owner_id,
            NFTContractMetadata {
                spec: NFT_METADATA_SPEC.to_string(),
                name,
                symbol,
                icon,
                base_uri: None,
                reference: None,
                reference_hash: None,
            },
        )
    }

    #[payable]
    pub fn nft_mint(
        &mut self,
        token_id: TokenId,
        token_owner_id: AccountId,
        token_metadata: TokenMetadata,
    ) -> Token {
        #[cfg(feature = "testnet")]
        self.authorize_testnet_mint(&token_owner_id);
        #[cfg(not(feature = "testnet"))]
        require!(
            MINTING_ENABLED,
            "Minting is disabled pending policy approval"
        );
        self.mint_once(token_id, token_owner_id, token_metadata)
    }

    pub fn contract_owner_id(&self) -> AccountId {
        self.tokens.owner_id.clone()
    }

    pub fn token_id_status(&self, token_id: TokenId) -> Option<TokenIdStatus> {
        self.ever_minted.contains_key(&token_id).then(|| {
            if self.tokens.owner_by_id.get(&token_id).is_some() {
                TokenIdStatus::Live
            } else {
                TokenIdStatus::Burned
            }
        })
    }
}

// Internal storage primitive, not a NEAR entry point or an authorization policy.
impl Contract {
    fn mint_once(
        &mut self,
        token_id: TokenId,
        token_owner_id: AccountId,
        token_metadata: TokenMetadata,
    ) -> Token {
        require!(!token_id.trim().is_empty(), "token_id must not be empty");
        require!(
            token_id.len() <= MAX_TOKEN_ID_BYTES,
            "token_id exceeds byte limit"
        );
        token_metadata.assert_valid();
        require!(
            !self.ever_minted.contains_key(&token_id)
                && self.tokens.owner_by_id.get(&token_id).is_none(),
            "token_id must be unique and never reused"
        );

        let before = env::storage_usage();
        require!(
            mutation::Version::load(&token_id).is_none(),
            "Mutation tombstone exists"
        );
        mutation::Version::default().write(&token_id);
        self.ever_minted.insert(&token_id, &true);
        let token = self.tokens.internal_mint_with_refund(
            token_id,
            token_owner_id,
            Some(token_metadata),
            None,
        );
        let bytes = env::storage_usage()
            .checked_sub(before)
            .expect("mint storage increased");
        let cost = env::storage_byte_cost()
            .as_yoctonear()
            .checked_mul(u128::from(bytes))
            .expect("mint storage cost overflow");
        require!(
            env::attached_deposit().as_yoctonear() >= cost,
            "Insufficient mint storage deposit"
        );
        // Includes permanent ID storage, unlike measuring only the SDK mint.
        // NEAR atomically rolls back both registry and mint on receipt failure.
        let refund = env::attached_deposit().as_yoctonear() - cost;
        if refund > 1 {
            Promise::new(env::predecessor_account_id())
                .transfer(near_sdk::NearToken::from_yoctonear(refund))
                .detach();
        }
        NftMint {
            owner_id: &token.owner_id,
            token_ids: &[&token.token_id],
            memo: None,
        }
        .emit();
        token
    }
}

#[near]
impl NonFungibleTokenCore for Contract {
    #[payable]
    fn nft_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
    ) {
        let version = mutation::Version::load(&token_id).map(|v| v.transfer_next());
        self.tokens
            .nft_transfer(receiver_id, token_id.clone(), approval_id, memo);
        if let Some(version) = version {
            version.write(&token_id);
        }
    }

    #[payable]
    fn nft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<bool> {
        self.transfer_call_versioned(receiver_id, token_id, approval_id, memo, msg)
    }

    fn nft_token(&self, token_id: TokenId) -> Option<Token> {
        self.tokens.nft_token(token_id)
    }
}

#[near]
impl NonFungibleTokenResolver for Contract {
    #[private]
    fn nft_resolve_transfer(
        &mut self,
        previous_owner_id: AccountId,
        receiver_id: AccountId,
        token_id: TokenId,
        approved_account_ids: Option<HashMap<AccountId, u64>>,
    ) -> bool {
        let _ = (
            previous_owner_id,
            receiver_id,
            token_id,
            approved_account_ids,
        );
        // Retain the ABI, but old callbacks cannot authenticate ownership epochs.
        // Outstanding legacy receipts MUST be drained before an authorized upgrade.
        env::panic_str("Legacy resolver disabled: unversioned outcome cannot mutate state")
    }
}

#[near]
impl NonFungibleTokenApproval for Contract {
    #[payable]
    fn nft_approve(
        &mut self,
        token_id: TokenId,
        account_id: AccountId,
        msg: Option<String>,
    ) -> Option<Promise> {
        require!(
            env::attached_deposit().as_yoctonear() >= 1,
            "Requires attached deposit of at least 1 yoctoNEAR"
        );
        assert_bounded_token_id(&token_id);
        require!(
            msg.as_ref()
                .is_none_or(|msg| msg.len() <= MAX_APPROVAL_MSG_BYTES),
            "Approval message exceeds byte limit"
        );
        let owner = self
            .tokens
            .owner_by_id
            .get(&token_id)
            .expect("Token not found");
        require!(
            env::predecessor_account_id() == owner,
            "Predecessor must be token owner."
        );
        let mut accounts = approval_storage::bounded_approvals(&self.tokens, &token_id);
        require!(
            accounts.contains_key(&account_id) || accounts.len() < MAX_APPROVALS_PER_TOKEN,
            "Approval count limit reached"
        );
        let version = mutation::Version::load(&token_id)
            .unwrap_or_default()
            .approval_next();
        let counters = self
            .tokens
            .next_approval_id_by_id
            .as_mut()
            .expect("Approval extension required");
        let approval_id = counters.get(&token_id).unwrap_or(1);
        let next = approval_id.checked_add(1).expect("Approval ID exhausted");
        // Legacy resolvers could leave an unfunded empty map. Discard it without
        // refund BEFORE measuring, so approving again funds the complete entry.
        if accounts.is_empty() {
            self.tokens
                .approvals_by_id
                .as_mut()
                .unwrap()
                .remove(&token_id);
        }
        let before = env::storage_usage();
        version.write(&token_id); // first allocation is funded by this approval deposit
        counters.insert(&token_id, &next);
        let approvals = self
            .tokens
            .approvals_by_id
            .as_mut()
            .expect("Approval extension required");
        accounts.insert(account_id.clone(), approval_id);
        approvals.insert(&token_id, &accounts);
        // Include complete trie keys/map overhead and the first permanent counter.
        // Re-approval replaces a fixed-size ID and consumes no additional bytes.
        let bytes = env::storage_usage()
            .checked_sub(before)
            .expect("Approval storage decreased");
        let cost = env::storage_byte_cost()
            .as_yoctonear()
            .checked_mul(u128::from(bytes))
            .expect("Approval storage cost overflow");
        require!(
            env::attached_deposit().as_yoctonear() >= cost,
            "Insufficient approval storage deposit"
        );
        let refund = env::attached_deposit().as_yoctonear() - cost;
        if refund > 1 {
            Promise::new(owner.clone())
                .transfer(near_sdk::NearToken::from_yoctonear(refund))
                .detach();
        }
        // Preserve the pinned SDK's hook and 10 Tgas local budget. Receipt failure
        // rolls back writes/refunds; a later hook failure does not revoke approval.
        msg.map(|msg| {
            ext_nft_approval_receiver::ext(account_id)
                .with_static_gas(env::prepaid_gas().saturating_sub(Gas::from_tgas(10)))
                .nft_on_approve(token_id, owner, approval_id, msg)
        })
    }

    #[payable]
    fn nft_revoke(&mut self, token_id: TokenId, account_id: AccountId) {
        near_sdk::assert_one_yocto();
        let owner = self
            .tokens
            .owner_by_id
            .get(&token_id)
            .expect("Token not found");
        require!(
            env::predecessor_account_id() == owner,
            "Predecessor must be token owner."
        );
        let version = mutation::Version::load(&token_id).map(|v| v.approval_next());
        let before = env::storage_usage();
        let approvals = self
            .tokens
            .approvals_by_id
            .as_mut()
            .expect("Approval extension required");
        let mut funded = false;
        if let Some(mut accounts) = approvals.get(&token_id) {
            funded = !accounts.is_empty();
            if accounts.remove(&account_id).is_some() {
                if accounts.is_empty() {
                    approvals.remove(&token_id);
                } else {
                    approvals.insert(&token_id, &accounts);
                }
            }
        }
        if let Some(version) = version {
            version.write(&token_id);
        }
        if funded {
            refund_released_approval_storage(before, owner);
        }
    }

    #[payable]
    fn nft_revoke_all(&mut self, token_id: TokenId) {
        near_sdk::assert_one_yocto();
        let owner = self
            .tokens
            .owner_by_id
            .get(&token_id)
            .expect("Token not found");
        require!(
            env::predecessor_account_id() == owner,
            "Predecessor must be token owner."
        );
        let version = mutation::Version::load(&token_id).map(|v| v.approval_next());
        let before = env::storage_usage();
        let removed = self
            .tokens
            .approvals_by_id
            .as_mut()
            .expect("Approval extension required")
            .remove(&token_id);
        if let Some(version) = version {
            version.write(&token_id);
        }
        if removed.is_some_and(|a| !a.is_empty()) {
            refund_released_approval_storage(before, owner);
        }
    }

    fn nft_is_approved(
        &self,
        token_id: TokenId,
        approved_account_id: AccountId,
        approval_id: Option<u64>,
    ) -> bool {
        self.tokens
            .nft_is_approved(token_id, approved_account_id, approval_id)
    }
}

fn require_self_initialization() {
    require!(
        env::predecessor_account_id() == env::current_account_id()
            && env::signer_account_id() == env::current_account_id()
            && env::promise_results_count() == 0
            && env::attached_deposit().is_zero(),
        "Initialization requires a direct zero-deposit self call"
    );
}

fn assert_bounded_token_id(token_id: &str) {
    require!(!token_id.trim().is_empty(), "token_id must not be empty");
    require!(
        token_id.len() <= MAX_TOKEN_ID_BYTES,
        "token_id exceeds byte limit"
    );
}

// Refund only released map bytes. Permanent counters/ID registry remain funded.
fn refund_released_approval_storage(before: u64, owner: AccountId) {
    let bytes = before
        .checked_sub(env::storage_usage())
        .expect("Revocation storage increased");
    let refund = env::storage_byte_cost()
        .as_yoctonear()
        .checked_mul(u128::from(bytes))
        .expect("Approval refund overflow");
    if refund > 0 {
        Promise::new(owner)
            .transfer(near_sdk::NearToken::from_yoctonear(refund))
            .detach();
    }
}

#[near]
impl NonFungibleTokenEnumeration for Contract {
    fn nft_total_supply(&self) -> U128 {
        self.tokens.nft_total_supply()
    }

    fn nft_tokens(&self, from_index: Option<U128>, limit: Option<u64>) -> Vec<Token> {
        self.tokens.nft_tokens(from_index, limit)
    }

    fn nft_supply_for_owner(&self, account_id: AccountId) -> U128 {
        self.tokens.nft_supply_for_owner(account_id)
    }

    fn nft_tokens_for_owner(
        &self,
        account_id: AccountId,
        from_index: Option<U128>,
        limit: Option<u64>,
    ) -> Vec<Token> {
        self.tokens
            .nft_tokens_for_owner(account_id, from_index, limit)
    }
}

#[near]
impl NonFungibleTokenMetadataProvider for Contract {
    fn nft_metadata(&self) -> NFTContractMetadata {
        self.metadata
            .get()
            .unwrap_or_else(|| env::panic_str("Contract metadata is not initialized"))
    }
}

#[cfg(test)]
mod tests;
