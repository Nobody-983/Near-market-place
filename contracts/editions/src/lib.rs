//! NEP-245 Multi-Token Edition Contract for the NEAR NFT Marketplace.
//! Supports fixed-supply limited editions, quantity balances, batch transfers,
//! and native quantity-based marketplace trading.

use near_contract_standards::non_fungible_token::metadata::TokenMetadata;
use near_sdk::collections::{LookupMap, UnorderedMap, Vector};
use near_sdk::json_types::U128;
use near_sdk::{env, near, require, AccountId, NearToken, PanicOnDefault, Promise};

pub type TokenId = String;

#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct MultiToken {
    pub token_id: TokenId,
    pub creator_id: AccountId,
    pub metadata: Option<TokenMetadata>,
    pub supply: u128,
}

#[near(serializers = [json])]
#[derive(Clone)]
pub struct MultiTokenView {
    pub token_id: TokenId,
    pub owner_id: AccountId,
    pub metadata: Option<TokenMetadata>,
    pub supply: U128,
}

#[near(serializers = [borsh, json])]
#[derive(Clone)]
pub struct EditionListing {
    pub listing_id: String,
    pub token_id: TokenId,
    pub seller: AccountId,
    pub price_per_copy: u128,
    pub initial_quantity: u128,
    pub available_quantity: u128,
    pub fee_bps: u64,
    pub fee_account: AccountId,
    pub created_at: u64,
    pub active: bool,
}

#[near(serializers = [json])]
#[derive(Clone)]
pub struct EditionListingView {
    pub listing_id: String,
    pub token_id: TokenId,
    pub seller: AccountId,
    pub price_per_copy: U128,
    pub initial_quantity: U128,
    pub available_quantity: U128,
    pub fee_bps: u64,
    pub fee_account: AccountId,
    pub created_at: u64,
    pub active: bool,
}

impl From<EditionListing> for EditionListingView {
    fn from(l: EditionListing) -> Self {
        Self {
            listing_id: l.listing_id,
            token_id: l.token_id,
            seller: l.seller,
            price_per_copy: U128(l.price_per_copy),
            initial_quantity: U128(l.initial_quantity),
            available_quantity: U128(l.available_quantity),
            fee_bps: l.fee_bps,
            fee_account: l.fee_account,
            created_at: l.created_at,
            active: l.active,
        }
    }
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    pub owner_id: AccountId,
    pub operator_id: AccountId,
    pub tokens: UnorderedMap<TokenId, MultiToken>,
    pub balances: LookupMap<(AccountId, TokenId), u128>,
    pub tokens_per_owner: LookupMap<AccountId, Vector<TokenId>>,
    pub listings: UnorderedMap<String, EditionListing>,
    pub seller_proceeds: LookupMap<AccountId, u128>,
    pub next_listing_nonce: u64,
}

#[near]
impl Contract {
    #[init]
    pub fn new(owner_id: AccountId, operator_id: AccountId) -> Self {
        require!(!env::state_exists(), "Already initialized");
        Self {
            owner_id,
            operator_id,
            tokens: UnorderedMap::new(b"t"),
            balances: LookupMap::new(b"b"),
            tokens_per_owner: LookupMap::new(b"o"),
            listings: UnorderedMap::new(b"l"),
            seller_proceeds: LookupMap::new(b"p"),
            next_listing_nonce: 1,
        }
    }

    // =========================================================
    // NEP-245 MINTING (EDITION CREATION)
    // =========================================================

    #[payable]
    pub fn mt_mint(
        &mut self,
        token_id: TokenId,
        amount: U128,
        token_metadata: TokenMetadata,
    ) -> MultiTokenView {
        let caller = env::predecessor_account_id();
        let supply = amount.0;
        require!(supply > 0, "Edition supply must be greater than zero");
        require!(
            supply <= 1_000_000_000,
            "Edition supply exceeds maximum limit"
        );
        require!(!token_id.trim().is_empty(), "Token ID cannot be empty");
        require!(token_id.len() <= 128, "Token ID exceeds length limit");
        require!(
            !self.tokens.get(&token_id).is_some(),
            "Token ID already exists"
        );

        let initial_storage = env::storage_usage();

        let token = MultiToken {
            token_id: token_id.clone(),
            creator_id: caller.clone(),
            metadata: Some(token_metadata),
            supply,
        };
        self.tokens.insert(&token_id, &token);
        self.internal_add_balance(&caller, &token_id, supply);

        // Calculate and verify required storage deposit
        let storage_used = env::storage_usage().saturating_sub(initial_storage);
        let required_deposit = env::storage_byte_cost()
            .as_yoctonear()
            .saturating_mul(u128::from(storage_used));
        let attached = env::attached_deposit().as_yoctonear();
        require!(
            attached >= required_deposit,
            format!(
                "Insufficient storage deposit. Required: {} yoctoNEAR",
                required_deposit
            )
        );

        // Refund any excess storage deposit
        let refund = attached.saturating_sub(required_deposit);
        if refund > 0 {
            let _ = Promise::new(caller.clone()).transfer(NearToken::from_yoctonear(refund));
        }

        // Standard NEP-245 event emission
        self.emit_nep245_event("mt_mint", &caller, &[&token_id], &[supply]);

        MultiTokenView {
            token_id: token.token_id,
            owner_id: token.creator_id,
            metadata: token.metadata,
            supply: U128(token.supply),
        }
    }

    // =========================================================
    // NEP-245 CORE TRANSFERS
    // =========================================================

    #[payable]
    pub fn mt_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: TokenId,
        amount: U128,
        approval: Option<()>,
        memo: Option<String>,
    ) {
        let _ = (approval, memo);
        require!(
            env::attached_deposit().as_yoctonear() >= 1,
            "Requires at least 1 yoctoNEAR attached deposit"
        );
        let sender = env::predecessor_account_id();
        self.internal_transfer(&sender, &receiver_id, &token_id, amount.0);
    }

    #[payable]
    pub fn mt_batch_transfer(
        &mut self,
        receiver_id: AccountId,
        token_ids: Vec<TokenId>,
        amounts: Vec<U128>,
        approvals: Option<Vec<()>>,
        memo: Option<String>,
    ) {
        let _ = (approvals, memo);
        require!(
            env::attached_deposit().as_yoctonear() >= 1,
            "Requires at least 1 yoctoNEAR attached deposit"
        );
        require!(
            token_ids.len() == amounts.len(),
            "Token IDs and amounts length mismatch"
        );
        let sender = env::predecessor_account_id();
        for (token_id, amount) in token_ids.iter().zip(amounts.iter()) {
            self.internal_transfer(&sender, &receiver_id, token_id, amount.0);
        }
    }

    // =========================================================
    // NEP-245 VIEWS & ENUMERATION
    // =========================================================

    pub fn mt_balance_of(&self, account_id: AccountId, token_id: TokenId) -> U128 {
        U128(self.balances.get(&(account_id, token_id)).unwrap_or(0))
    }

    pub fn mt_batch_balance_of(&self, account_id: AccountId, token_ids: Vec<TokenId>) -> Vec<U128> {
        token_ids
            .iter()
            .map(|t| self.mt_balance_of(account_id.clone(), t.clone()))
            .collect()
    }

    pub fn mt_supply(&self, token_id: TokenId) -> Option<U128> {
        self.tokens.get(&token_id).map(|t| U128(t.supply))
    }

    pub fn mt_token(&self, token_id: TokenId) -> Option<MultiTokenView> {
        self.tokens.get(&token_id).map(|t| MultiTokenView {
            token_id: t.token_id,
            owner_id: t.creator_id,
            metadata: t.metadata,
            supply: U128(t.supply),
        })
    }

    pub fn mt_tokens(&self, from_index: Option<U128>, limit: Option<u64>) -> Vec<MultiTokenView> {
        let start = from_index.map(|i| i.0 as usize).unwrap_or(0);
        let limit = limit.unwrap_or(50).min(100) as usize;
        let tokens_vec = self.tokens.values_as_vector();
        let mut results = Vec::new();
        for i in start..std::cmp::min(start + limit, tokens_vec.len() as usize) {
            if let Some(t) = tokens_vec.get(i as u64) {
                results.push(MultiTokenView {
                    token_id: t.token_id,
                    owner_id: t.creator_id,
                    metadata: t.metadata,
                    supply: U128(t.supply),
                });
            }
        }
        results
    }

    pub fn mt_tokens_for_owner(
        &self,
        account_id: AccountId,
        from_index: Option<U128>,
        limit: Option<u64>,
    ) -> Vec<MultiTokenView> {
        let token_ids = match self.tokens_per_owner.get(&account_id) {
            Some(v) => v,
            None => return vec![],
        };
        let start = from_index.map(|i| i.0 as u64).unwrap_or(0);
        let limit = limit.unwrap_or(50).min(100);
        let mut results = vec![];
        for i in start..std::cmp::min(start + limit, token_ids.len()) {
            if let Some(token_id) = token_ids.get(i) {
                if self
                    .balances
                    .get(&(account_id.clone(), token_id.clone()))
                    .unwrap_or(0)
                    > 0
                {
                    if let Some(t) = self.tokens.get(&token_id) {
                        results.push(MultiTokenView {
                            token_id: t.token_id,
                            owner_id: t.creator_id,
                            metadata: t.metadata,
                            supply: U128(t.supply),
                        });
                    }
                }
            }
        }
        results
    }

    // =========================================================
    // EDITION MARKETPLACE: LISTING, PURCHASING, CANCELLATION
    // =========================================================

    #[payable]
    pub fn mt_list(&mut self, token_id: TokenId, quantity: U128, price_per_copy: U128) -> String {
        let seller = env::predecessor_account_id();
        let qty = quantity.0;
        let price = price_per_copy.0;
        require!(qty > 0, "Listing quantity must be greater than zero");
        require!(price > 0, "Price per copy must be greater than zero");
        require!(self.tokens.get(&token_id).is_some(), "Token does not exist");

        let current_balance = self
            .balances
            .get(&(seller.clone(), token_id.clone()))
            .unwrap_or(0);
        require!(
            current_balance >= qty,
            "Insufficient edition balance to list this quantity"
        );

        // Escrow/reserve the listed quantity from seller's active balance
        self.internal_sub_balance(&seller, &token_id, qty);

        let listing_nonce = self.next_listing_nonce;
        self.next_listing_nonce += 1;
        let listing_id = format!("edition|{}|{}|{}", token_id, seller, listing_nonce);

        let listing = EditionListing {
            listing_id: listing_id.clone(),
            token_id: token_id.clone(),
            seller: seller.clone(),
            price_per_copy: price,
            initial_quantity: qty,
            available_quantity: qty,
            fee_bps: 100, // 1% marketplace fee
            fee_account: self.operator_id.clone(),
            created_at: env::block_timestamp(),
            active: true,
        };

        self.listings.insert(&listing_id, &listing);
        listing_id
    }

    #[payable]
    pub fn buy_edition(&mut self, listing_id: String, quantity: U128) {
        let buyer = env::predecessor_account_id();
        let qty_to_buy = quantity.0;
        require!(qty_to_buy > 0, "Quantity to buy must be greater than zero");

        let mut listing = self
            .listings
            .get(&listing_id)
            .expect("Edition listing not found");
        require!(listing.active, "Listing is not active");
        require!(
            listing.available_quantity >= qty_to_buy,
            "Requested quantity exceeds available listing quantity"
        );
        require!(
            buyer != listing.seller,
            "Seller cannot purchase their own edition listing"
        );

        let total_price = listing
            .price_per_copy
            .checked_mul(qty_to_buy)
            .expect("Price multiplication overflow");
        let attached = env::attached_deposit().as_yoctonear();
        require!(
            attached >= total_price,
            format!(
                "Attached deposit ({}) is less than total price ({})",
                attached, total_price
            )
        );

        // Update listing quantity
        listing.available_quantity = listing.available_quantity.saturating_sub(qty_to_buy);
        if listing.available_quantity == 0 {
            listing.active = false;
        }
        self.listings.insert(&listing_id, &listing);

        // Credit tokens from escrow to buyer
        self.internal_add_balance(&buyer, &listing.token_id, qty_to_buy);

        // Calculate and credit fee and seller proceeds
        let fee = total_price
            .checked_mul(listing.fee_bps as u128)
            .unwrap_or(0)
            / 10_000;
        let seller_share = total_price.saturating_sub(fee);

        // Payout seller proceeds
        let current_seller_proceeds = self.seller_proceeds.get(&listing.seller).unwrap_or(0);
        self.seller_proceeds.insert(
            &listing.seller,
            &(current_seller_proceeds.saturating_add(seller_share)),
        );
        let _ =
            Promise::new(listing.seller.clone()).transfer(NearToken::from_yoctonear(seller_share));

        // Payout fee
        if fee > 0 {
            let _ =
                Promise::new(listing.fee_account.clone()).transfer(NearToken::from_yoctonear(fee));
        }

        // Refund any excess payment
        let excess = attached.saturating_sub(total_price);
        if excess > 0 {
            let _ = Promise::new(buyer.clone()).transfer(NearToken::from_yoctonear(excess));
        }

        // Emit transfer event
        self.emit_nep245_event("mt_transfer", &buyer, &[&listing.token_id], &[qty_to_buy]);
    }

    #[payable]
    pub fn cancel_edition_listing(&mut self, listing_id: String) {
        let caller = env::predecessor_account_id();
        let mut listing = self
            .listings
            .get(&listing_id)
            .expect("Edition listing not found");
        require!(listing.active, "Listing is already inactive");
        require!(
            listing.seller == caller,
            "Only the listing seller can cancel this listing"
        );

        listing.active = false;
        let remaining_qty = listing.available_quantity;
        listing.available_quantity = 0;
        self.listings.insert(&listing_id, &listing);

        // Return remaining escrowed quantity back to seller
        if remaining_qty > 0 {
            self.internal_add_balance(&caller, &listing.token_id, remaining_qty);
        }
    }

    pub fn edition_listing(&self, listing_id: String) -> Option<EditionListingView> {
        self.listings.get(&listing_id).map(Into::into)
    }

    pub fn edition_listings(
        &self,
        from_index: Option<u64>,
        limit: Option<u64>,
    ) -> Vec<EditionListingView> {
        let start = from_index.unwrap_or(0) as usize;
        let limit = limit.unwrap_or(50).min(100) as usize;
        let listings_vec = self.listings.values_as_vector();
        let mut results = Vec::new();
        for i in start..std::cmp::min(start + limit, listings_vec.len() as usize) {
            if let Some(l) = listings_vec.get(i as u64) {
                results.push(l.into());
            }
        }
        results
    }

    pub fn edition_listings_for_seller(&self, seller_id: AccountId) -> Vec<EditionListingView> {
        let listings_vec = self.listings.values_as_vector();
        let mut results = Vec::new();
        for i in 0..listings_vec.len() {
            if let Some(l) = listings_vec.get(i) {
                if l.seller == seller_id {
                    results.push(l.into());
                }
            }
        }
        results
    }

    pub fn edition_listings_for_token(&self, token_id: TokenId) -> Vec<EditionListingView> {
        let listings_vec = self.listings.values_as_vector();
        let mut results = Vec::new();
        for i in 0..listings_vec.len() {
            if let Some(l) = listings_vec.get(i) {
                if l.token_id == token_id {
                    results.push(l.into());
                }
            }
        }
        results
    }

    // =========================================================
    // INTERNAL HELPERS
    // =========================================================

    fn internal_add_balance(&mut self, account_id: &AccountId, token_id: &TokenId, amount: u128) {
        let current = self
            .balances
            .get(&(account_id.clone(), token_id.clone()))
            .unwrap_or(0);
        self.balances.insert(
            &(account_id.clone(), token_id.clone()),
            &(current.saturating_add(amount)),
        );

        let mut token_ids = self
            .tokens_per_owner
            .get(account_id)
            .unwrap_or_else(|| Vector::new(env::sha256(account_id.as_bytes())));
        let mut exists = false;
        for i in 0..token_ids.len() {
            if token_ids.get(i).as_ref() == Some(token_id) {
                exists = true;
                break;
            }
        }
        if !exists {
            token_ids.push(token_id);
            self.tokens_per_owner.insert(account_id, &token_ids);
        }
    }

    fn internal_sub_balance(&mut self, account_id: &AccountId, token_id: &TokenId, amount: u128) {
        let current = self
            .balances
            .get(&(account_id.clone(), token_id.clone()))
            .expect("No balance for token");
        require!(current >= amount, "Insufficient balance");
        self.balances
            .insert(&(account_id.clone(), token_id.clone()), &(current - amount));
    }

    fn internal_transfer(
        &mut self,
        sender_id: &AccountId,
        receiver_id: &AccountId,
        token_id: &TokenId,
        amount: u128,
    ) {
        require!(
            sender_id != receiver_id,
            "Sender and receiver cannot be the same"
        );
        require!(amount > 0, "Transfer amount must be greater than zero");
        self.internal_sub_balance(sender_id, token_id, amount);
        self.internal_add_balance(receiver_id, token_id, amount);
        self.emit_nep245_event("mt_transfer", receiver_id, &[token_id], &[amount]);
    }

    fn emit_nep245_event(
        &self,
        event: &str,
        recipient: &AccountId,
        token_ids: &[&TokenId],
        amounts: &[u128],
    ) {
        let token_ids_json: Vec<String> = token_ids.iter().map(|t| format!("\"{}\"", t)).collect();
        let amounts_json: Vec<String> = amounts.iter().map(|a| format!("\"{}\"", a)).collect();
        let event_json = format!(
            "{{\"standard\":\"nep245\",\"version\":\"1.0.0\",\"event\":\"{}\",\"data\":[{{\"owner_id\":\"{}\",\"token_ids\":[{}],\"amounts\":[{}]}}]}}",
            event,
            recipient,
            token_ids_json.join(","),
            amounts_json.join(",")
        );
        env::log_str(&format!("EVENT_JSON:{}", event_json));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;

    fn contract_id() -> AccountId {
        "editions.testnet".parse().unwrap()
    }

    fn alice() -> AccountId {
        "alice.testnet".parse().unwrap()
    }

    fn bob() -> AccountId {
        "bob.testnet".parse().unwrap()
    }

    fn operator() -> AccountId {
        "operator.testnet".parse().unwrap()
    }

    fn setup() -> (Contract, VMContextBuilder) {
        let mut context = VMContextBuilder::new();
        context
            .current_account_id(contract_id())
            .predecessor_account_id(alice())
            .attached_deposit(NearToken::from_near(5));
        testing_env!(context.build());
        let contract = Contract::new(alice(), operator());
        (contract, context)
    }

    fn sample_metadata(title: &str) -> TokenMetadata {
        TokenMetadata {
            title: Some(title.to_string()),
            description: Some("Edition artwork description".to_string()),
            media: Some("https://example.com/art.png".to_string()),
            media_hash: None,
            copies: Some(100),
            issued_at: None,
            expires_at: None,
            starts_at: None,
            updated_at: None,
            extra: None,
            reference: None,
            reference_hash: None,
        }
    }

    #[test]
    fn test_create_edition_and_mint_initial_supply() {
        let (mut contract, mut context) = setup();
        context
            .predecessor_account_id(alice())
            .attached_deposit(NearToken::from_near(1));
        testing_env!(context.build());

        let token_view = contract.mt_mint(
            "cyber-dragon".to_string(),
            U128(100),
            sample_metadata("Cyber Dragon"),
        );
        assert_eq!(token_view.token_id, "cyber-dragon");
        assert_eq!(token_view.supply.0, 100);
        assert_eq!(token_view.owner_id, alice());

        assert_eq!(
            contract
                .mt_balance_of(alice(), "cyber-dragon".to_string())
                .0,
            100
        );
        assert_eq!(
            contract.mt_balance_of(bob(), "cyber-dragon".to_string()).0,
            0
        );
        assert_eq!(
            contract.mt_supply("cyber-dragon".to_string()).unwrap().0,
            100
        );
    }

    #[test]
    #[should_panic(expected = "Token ID already exists")]
    fn test_duplicate_edition_mint_rejected() {
        let (mut contract, mut context) = setup();
        context
            .predecessor_account_id(alice())
            .attached_deposit(NearToken::from_near(1));
        testing_env!(context.build());

        contract.mt_mint(
            "cyber-dragon".to_string(),
            U128(100),
            sample_metadata("Cyber Dragon"),
        );
        contract.mt_mint(
            "cyber-dragon".to_string(),
            U128(50),
            sample_metadata("Duplicate"),
        );
    }

    #[test]
    fn test_transfer_and_batch_transfer_quantities() {
        let (mut contract, mut context) = setup();
        context
            .predecessor_account_id(alice())
            .attached_deposit(NearToken::from_near(1));
        testing_env!(context.build());

        contract.mt_mint(
            "dragon-1".to_string(),
            U128(100),
            sample_metadata("Dragon 1"),
        );
        contract.mt_mint(
            "dragon-2".to_string(),
            U128(50),
            sample_metadata("Dragon 2"),
        );

        // Single transfer: Alice sends 20 copies of dragon-1 to Bob
        context
            .predecessor_account_id(alice())
            .attached_deposit(NearToken::from_yoctonear(1));
        testing_env!(context.build());
        contract.mt_transfer(bob(), "dragon-1".to_string(), U128(20), None, None);

        assert_eq!(
            contract.mt_balance_of(alice(), "dragon-1".to_string()).0,
            80
        );
        assert_eq!(contract.mt_balance_of(bob(), "dragon-1".to_string()).0, 20);

        // Batch transfer: Alice sends 10 of dragon-1 and 15 of dragon-2 to Bob
        contract.mt_batch_transfer(
            bob(),
            vec!["dragon-1".to_string(), "dragon-2".to_string()],
            vec![U128(10), U128(15)],
            None,
            None,
        );

        assert_eq!(
            contract.mt_balance_of(alice(), "dragon-1".to_string()).0,
            70
        );
        assert_eq!(contract.mt_balance_of(bob(), "dragon-1".to_string()).0, 30);
        assert_eq!(
            contract.mt_balance_of(alice(), "dragon-2".to_string()).0,
            35
        );
        assert_eq!(contract.mt_balance_of(bob(), "dragon-2".to_string()).0, 15);
    }

    #[test]
    fn test_edition_listing_and_partial_purchase_flow() {
        let (mut contract, mut context) = setup();
        context
            .predecessor_account_id(alice())
            .attached_deposit(NearToken::from_near(1));
        testing_env!(context.build());

        // 1. Alice creates Cyber Dragon edition with 100 copies
        contract.mt_mint(
            "cyber-dragon".to_string(),
            U128(100),
            sample_metadata("Cyber Dragon"),
        );

        // 2. Alice lists all 100 copies at 2 NEAR each
        let listing_id = contract.mt_list(
            "cyber-dragon".to_string(),
            U128(100),
            U128(2_000_000_000_000_000_000_000_000),
        );
        assert_eq!(
            contract
                .mt_balance_of(alice(), "cyber-dragon".to_string())
                .0,
            0
        ); // 100 in escrow

        let listing_view = contract.edition_listing(listing_id.clone()).unwrap();
        assert_eq!(listing_view.available_quantity.0, 100);
        assert_eq!(listing_view.active, true);

        // 3. Bob purchases 10 copies for 20 NEAR (2 NEAR * 10)
        let buyer_bob: AccountId = "bob.testnet".parse().unwrap();
        context
            .predecessor_account_id(buyer_bob.clone())
            .attached_deposit(NearToken::from_near(20));
        testing_env!(context.build());

        contract.buy_edition(listing_id.clone(), U128(10));

        // Verify Bob owns 10 copies
        assert_eq!(
            contract
                .mt_balance_of(buyer_bob.clone(), "cyber-dragon".to_string())
                .0,
            10
        );

        // Verify remaining listing quantity is 90
        let updated_listing = contract.edition_listing(listing_id.clone()).unwrap();
        assert_eq!(updated_listing.available_quantity.0, 90);
        assert_eq!(updated_listing.active, true);

        // 4. Buyer C (Carol) purchases 5 copies for 10 NEAR
        let buyer_carol: AccountId = "carol.testnet".parse().unwrap();
        context
            .predecessor_account_id(buyer_carol.clone())
            .attached_deposit(NearToken::from_near(10));
        testing_env!(context.build());

        contract.buy_edition(listing_id.clone(), U128(5));

        // Verify Carol owns 5 copies
        assert_eq!(
            contract
                .mt_balance_of(buyer_carol, "cyber-dragon".to_string())
                .0,
            5
        );

        // Verify remaining listing quantity is 85
        let updated_listing2 = contract.edition_listing(listing_id.clone()).unwrap();
        assert_eq!(updated_listing2.available_quantity.0, 85);
        assert_eq!(updated_listing2.active, true);
    }

    #[test]
    fn test_edition_resale_and_cancellation() {
        let (mut contract, mut context) = setup();
        context
            .predecessor_account_id(alice())
            .attached_deposit(NearToken::from_near(1));
        testing_env!(context.build());

        contract.mt_mint(
            "cyber-dragon".to_string(),
            U128(100),
            sample_metadata("Cyber Dragon"),
        );
        let listing_id = contract.mt_list(
            "cyber-dragon".to_string(),
            U128(100),
            U128(1_000_000_000_000_000_000_000_000),
        ); // 1 NEAR each

        // Bob buys 10 copies
        context
            .predecessor_account_id(bob())
            .attached_deposit(NearToken::from_near(10));
        testing_env!(context.build());
        contract.buy_edition(listing_id.clone(), U128(10));
        assert_eq!(
            contract.mt_balance_of(bob(), "cyber-dragon".to_string()).0,
            10
        );

        // Bob lists 3 of his 10 copies for resale at 3 NEAR each
        context
            .predecessor_account_id(bob())
            .attached_deposit(NearToken::ZERO);
        testing_env!(context.build());
        let bob_listing_id = contract.mt_list(
            "cyber-dragon".to_string(),
            U128(3),
            U128(3_000_000_000_000_000_000_000_000),
        );

        // Bob now has 7 available balance and 3 in escrow listing
        assert_eq!(
            contract.mt_balance_of(bob(), "cyber-dragon".to_string()).0,
            7
        );

        // Dragon buys 2 copies from Bob
        let dragon: AccountId = "dragon983.testnet".parse().unwrap();
        context
            .predecessor_account_id(dragon.clone())
            .attached_deposit(NearToken::from_near(6));
        testing_env!(context.build());
        contract.buy_edition(bob_listing_id.clone(), U128(2));

        assert_eq!(
            contract.mt_balance_of(dragon, "cyber-dragon".to_string()).0,
            2
        );
        let bob_listing = contract.edition_listing(bob_listing_id.clone()).unwrap();
        assert_eq!(bob_listing.available_quantity.0, 1);

        // Bob cancels remaining 1 copy listing
        context
            .predecessor_account_id(bob())
            .attached_deposit(NearToken::ZERO);
        testing_env!(context.build());
        contract.cancel_edition_listing(bob_listing_id.clone());

        // The remaining 1 copy returns to Bob's balance: 7 + 1 = 8
        assert_eq!(
            contract.mt_balance_of(bob(), "cyber-dragon".to_string()).0,
            8
        );
        assert_eq!(
            contract.edition_listing(bob_listing_id).unwrap().active,
            false
        );
    }
}
