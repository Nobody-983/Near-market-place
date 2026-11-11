//! Seller-approved listing lifecycle and owner policy controls.

use super::*;
use crate::events::emit;
use near_sdk::PromiseOrValue;

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct ListingTerms {
    standard: String,
    version: String,
    marketplace_id: AccountId,
    nft_contract_id: AccountId,
    token_id: String,
    seller_id: AccountId,
    price: String,
    fee_bps: u16,
    fee_account: AccountId,
    expires_at: Option<String>,
}

#[near]
impl Contract {
    /// NEP-178 approval receiver. The original signer must be the NFT owner,
    /// and the signed message must bind every economic and identity term.
    pub fn nft_on_approve(
        &mut self,
        token_id: String,
        owner_id: AccountId,
        approval_id: u64,
        msg: String,
    ) -> PromiseOrValue<String> {
        require!(self.trading_enabled, "Trading is disabled");
        require!(
            env::attached_deposit().is_zero(),
            "Approval callback rejects deposits"
        );
        let nft_contract_id = env::predecessor_account_id();
        require!(
            self.allowed_nft_contracts.contains(&nft_contract_id),
            "NFT contract is not allowed"
        );
        require!(
            env::signer_account_id() == owner_id,
            "Approval signer must be the seller"
        );
        require!(
            !token_id.trim().is_empty() && token_id.len() <= MAX_TOKEN_ID_BYTES,
            "Invalid token_id"
        );
        require!(approval_id > 0, "approval_id must be positive");
        require!(
            msg.len() <= MAX_APPROVAL_MSG_BYTES,
            "Approval message exceeds byte limit"
        );

        let terms: ListingTerms = serde_json::from_str(&msg).expect("Invalid listing terms");
        require!(
            terms.standard == "near_marketplace" && terms.version == "1.0.0",
            "Unsupported listing terms"
        );
        require!(
            terms.marketplace_id == env::current_account_id(),
            "Marketplace term mismatch"
        );
        require!(
            terms.nft_contract_id == nft_contract_id,
            "NFT contract term mismatch"
        );
        require!(terms.token_id == token_id, "Token term mismatch");
        require!(terms.seller_id == owner_id, "Seller term mismatch");
        require!(
            u64::from(terms.fee_bps) == self.config.fee_bps,
            "Fee term mismatch"
        );
        require!(
            terms.fee_account == self.fee_recipient(),
            "Fee recipient term mismatch"
        );
        let price = terms.price.parse::<u128>().expect("Invalid price");
        require!(price > 0, "Price must be positive");
        let expires_at = terms
            .expires_at
            .map(|v| v.parse::<u64>().expect("Invalid expiration"));
        if let Some(expires_at) = expires_at {
            require!(
                expires_at > env::block_timestamp(),
                "Listing already expired"
            );
        }

        let asset_key = Self::asset_key(&nft_contract_id, &token_id);
        let mut asset = self.asset_states.get(&asset_key).unwrap_or_default();
        require!(
            approval_id > asset.last_approval_id,
            "Stale or repeated approval_id"
        );
        let generation = asset
            .generation
            .checked_add(1)
            .expect("Sale generation exhausted");

        if let Some(previous_id) = asset.current_listing_id.take() {
            if let (Some(mut old), Some(mut old_meta)) = (
                self.listings.get(&previous_id),
                self.listing_meta.get(&previous_id),
            ) {
                if old.active && old_meta.status == ListingStatus::Active {
                    old.active = false;
                    old.cancelled_at = Some(env::block_timestamp());
                    old_meta.status = ListingStatus::Replaced;
                    if old_meta.reserved_sale_bytes > 0 {
                        let released = Self::storage_cost(old_meta.reserved_sale_bytes);
                        self.release_storage(&old_meta.storage_payer, released);
                        old_meta.reserved_sale_bytes = 0;
                    }
                    self.listings.insert(&previous_id, &old);
                    self.listing_meta.insert(&previous_id, &old_meta);
                    emit(
                        "listing_replaced",
                        &serde_json::json!({ "listing_id": previous_id, "generation": old_meta.generation.to_string() }),
                    );
                }
            }
        }

        let listing_id = Self::listing_id(&nft_contract_id, &token_id, &owner_id, generation);
        require!(
            !self.listings.contains_key(&listing_id),
            "Listing generation already exists"
        );
        let before = env::storage_usage();
        let listing = Listing {
            nft_contract_id: nft_contract_id.clone(),
            token_id: token_id.clone(),
            seller: owner_id.clone(),
            approval_id,
            price,
            fee_bps: self.config.fee_bps,
            fee_account: Some(self.fee_recipient()),
            created_at: env::block_timestamp(),
            cancelled_at: None,
            active: true,
        };
        let meta = ListingMeta {
            generation,
            expires_at,
            status: ListingStatus::Active,
            storage_payer: owner_id.clone(),
            reserved_sale_bytes: SALE_STORAGE_RESERVE_BYTES,
            actual_sale_bytes: 0,
            terms_hash: env::sha256(msg.as_bytes()),
        };
        asset.generation = generation;
        asset.last_approval_id = approval_id;
        asset.current_listing_id = Some(listing_id.clone());
        self.listings.insert(&listing_id, &listing);
        self.listing_meta.insert(&listing_id, &meta);
        self.asset_states.insert(&asset_key, &asset);
        self.listing_ids.push(&listing_id);
        let actual_bytes = env::storage_usage()
            .checked_sub(before)
            .expect("Listing storage decreased");
        let charged_bytes = actual_bytes
            .checked_add(SALE_STORAGE_RESERVE_BYTES)
            .expect("Listing storage overflow");
        self.charge_storage(&owner_id, Self::storage_cost(charged_bytes));
        self.assert_backed();
        emit(
            "listing_created",
            &serde_json::json!({
                "listing_id": listing_id, "nft_contract_id": nft_contract_id, "token_id": token_id,
                "seller_id": owner_id, "approval_id": approval_id.to_string(), "generation": generation.to_string(),
                "price": price.to_string(), "expires_at": expires_at.map(|v| v.to_string())
            }),
        );
        PromiseOrValue::Value(listing_id)
    }

    #[payable]
    pub fn cancel_listing(&mut self, listing_id: String) {
        near_sdk::assert_one_yocto();
        let listing = self.listings.get(&listing_id).expect("Listing not found");
        require!(
            env::predecessor_account_id() == listing.seller,
            "Only the seller may cancel"
        );
        self.close_listing(&listing_id, ListingStatus::Cancelled);
    }

    pub fn expire_listing(&mut self, listing_id: String) {
        require!(
            env::attached_deposit().is_zero(),
            "Expiration rejects deposits"
        );
        let meta = self
            .listing_meta
            .get(&listing_id)
            .expect("Listing metadata not found");
        require!(
            meta.expires_at
                .is_some_and(|value| value <= env::block_timestamp()),
            "Listing has not expired"
        );
        self.close_listing(&listing_id, ListingStatus::Expired);
    }

    #[payable]
    pub fn set_trading_enabled(&mut self, enabled: bool) {
        near_sdk::assert_one_yocto();
        self.assert_owner();
        self.trading_enabled = enabled;
        emit(
            "trading_status_changed",
            &serde_json::json!({ "enabled": enabled }),
        );
    }

    #[payable]
    pub fn set_fee_policy(&mut self, fee_bps: u64, fee_account: Option<AccountId>) {
        near_sdk::assert_one_yocto();
        self.assert_owner();
        Self::validate_fee_policy(fee_bps, fee_account.as_ref());
        self.config.fee_bps = fee_bps;
        self.config.fee_account = fee_account;
        emit(
            "fee_policy_changed",
            &serde_json::json!({ "fee_bps": fee_bps, "fee_account": self.fee_recipient() }),
        );
    }

    #[payable]
    pub fn set_nft_contract_allowed(&mut self, nft_contract_id: AccountId, allowed: bool) {
        near_sdk::assert_one_yocto();
        self.assert_owner();
        require!(
            nft_contract_id != env::current_account_id(),
            "Marketplace cannot be allowlisted as an NFT"
        );
        let owner = self.config.owner.clone();
        let paid_before = self
            .allowlist_storage_paid
            .get(&nft_contract_id)
            .unwrap_or(0);
        let before = env::storage_usage();
        let changed = if allowed {
            let changed = self.allowed_nft_contracts.insert(&nft_contract_id);
            if changed {
                self.allowlist_storage_paid.insert(&nft_contract_id, &0);
            }
            changed
        } else {
            let changed = self.allowed_nft_contracts.remove(&nft_contract_id);
            if changed {
                self.allowlist_storage_paid.remove(&nft_contract_id);
            }
            changed
        };
        require!(changed, "Allowlist already has requested state");
        let after = env::storage_usage();
        if after > before {
            let cost = Self::storage_cost(after - before);
            self.charge_storage(&owner, cost);
            self.allowlist_storage_paid.insert(&nft_contract_id, &cost);
        } else if before > after {
            // The primary allowlist entry was created at initialization and has
            // no user-paid record. Only release storage previously charged here.
            if paid_before > 0 {
                self.release_storage(&owner, paid_before);
            }
        }
        emit(
            "nft_allowlist_changed",
            &serde_json::json!({ "nft_contract_id": nft_contract_id, "allowed": allowed }),
        );
    }
}

impl Contract {
    fn close_listing(&mut self, listing_id: &String, status: ListingStatus) {
        let mut listing = self.listings.get(listing_id).expect("Listing not found");
        let mut meta = self
            .listing_meta
            .get(listing_id)
            .expect("Listing metadata not found");
        require!(
            listing.active && meta.status == ListingStatus::Active,
            "Listing is not active"
        );
        listing.active = false;
        listing.cancelled_at = Some(env::block_timestamp());
        meta.status = status;
        if meta.reserved_sale_bytes > 0 {
            self.release_storage(
                &meta.storage_payer,
                Self::storage_cost(meta.reserved_sale_bytes),
            );
            meta.reserved_sale_bytes = 0;
        }
        let key = Self::asset_key(&listing.nft_contract_id, &listing.token_id);
        if let Some(mut asset) = self.asset_states.get(&key) {
            if asset.current_listing_id.as_ref() == Some(listing_id) {
                asset.current_listing_id = None;
                self.asset_states.insert(&key, &asset);
            }
        }
        self.listings.insert(listing_id, &listing);
        self.listing_meta.insert(listing_id, &meta);
        emit(
            match status {
                ListingStatus::Expired => "listing_expired",
                _ => "listing_cancelled",
            },
            &serde_json::json!({ "listing_id": listing_id, "generation": meta.generation.to_string() }),
        );
    }
}
