//! Exact native-NEAR purchase settlement.

use super::*;
use crate::events::emit;
use near_sdk::{ext_contract, PromiseOrValue};

#[ext_contract(ext_nft_marketplace)]
#[allow(dead_code)]
trait NftMarketplace {
    fn marketplace_transfer_approved(
        &mut self,
        expected_seller_id: AccountId,
        token_id: String,
        approval_id: U64,
        receiver_id: AccountId,
    ) -> bool;
}

#[ext_contract(ext_self_settlement)]
#[allow(dead_code)]
trait MarketplaceSettlement {
    fn on_transfer_result(&mut self, sale_id: String) -> PromiseOrValue<()>;
}

#[near]
impl Contract {
    #[payable]
    pub fn buy_nft(&mut self, listing_id: String) -> PromiseOrValue<()> {
        require!(self.trading_enabled, "Trading is disabled");
        let payment = env::attached_deposit().as_yoctonear();
        let mut listing = self.listings.get(&listing_id).expect("Listing not found");
        let mut meta = self
            .listing_meta
            .get(&listing_id)
            .expect("Listing metadata not found");
        require!(
            listing.active && meta.status == ListingStatus::Active,
            "Listing is not active"
        );
        require!(
            !meta
                .expires_at
                .is_some_and(|value| value <= env::block_timestamp()),
            "Listing expired"
        );
        require!(
            self.allowed_nft_contracts
                .contains(&listing.nft_contract_id),
            "NFT contract is not allowed"
        );
        require!(
            payment == listing.price,
            "Payment does not match listing price"
        );
        let buyer = env::predecessor_account_id();
        require!(
            buyer != listing.seller && buyer != env::current_account_id(),
            "Invalid buyer"
        );

        let asset_key = Self::asset_key(&listing.nft_contract_id, &listing.token_id);
        let mut asset = self
            .asset_states
            .get(&asset_key)
            .expect("Asset state missing");
        require!(
            asset.current_listing_id.as_ref() == Some(&listing_id)
                && asset.generation == meta.generation
                && asset.last_approval_id == listing.approval_id,
            "Listing generation is stale"
        );
        let sale_id = Self::sale_id(&listing_id, &buyer, meta.generation);
        require!(
            !self.sales.contains_key(&sale_id) && !self.settlements.contains_key(&sale_id),
            "Duplicate purchase detected"
        );

        let (seller_share, fee_share) = Self::split(payment, listing.fee_bps);
        let fee_recipient = listing
            .fee_account
            .clone()
            .unwrap_or_else(|| self.config.owner.clone());
        let before = env::storage_usage();
        listing.active = false;
        meta.status = ListingStatus::PendingSale;
        asset.current_listing_id = None;
        let sale = Sale {
            listing_id: listing_id.clone(),
            listing: listing.clone(),
            buyer: buyer.clone(),
            purchase_generation: meta.generation,
            created_at: env::block_timestamp(),
            settled: false,
            nft_delivered: false,
        };
        let settlement = SettlementRecord {
            sale_id: sale_id.clone(),
            listing_id: listing_id.clone(),
            nft_contract_id: listing.nft_contract_id.clone(),
            token_id: listing.token_id.clone(),
            seller: listing.seller.clone(),
            buyer: buyer.clone(),
            approval_id: U64(listing.approval_id),
            generation: U64(meta.generation),
            price: U128(payment),
            seller_share: U128(seller_share),
            fee_share: U128(fee_share),
            fee_recipient,
            status: SettlementStatus::NftTransferPending,
            created_at: U64(env::block_timestamp()),
            updated_at: U64(env::block_timestamp()),
        };
        self.listings.insert(&listing_id, &listing);
        self.listing_meta.insert(&listing_id, &meta);
        self.asset_states.insert(&asset_key, &asset);
        self.sales.insert(&sale_id, &sale);
        self.settlements.insert(&sale_id, &settlement);
        self.sale_ids.push(&sale_id);
        let actual_bytes = env::storage_usage()
            .checked_sub(before)
            .expect("Sale storage decreased");
        require!(
            actual_bytes <= meta.reserved_sale_bytes,
            "Sale storage reservation exhausted"
        );
        meta.actual_sale_bytes = actual_bytes;
        meta.reserved_sale_bytes -= actual_bytes;
        self.listing_meta.insert(&listing_id, &meta);
        self.ledger.pending_sale_liability = self
            .ledger
            .pending_sale_liability
            .checked_add(payment)
            .expect("Pending sale overflow");
        self.assert_backed();
        emit(
            "sale_pending",
            &serde_json::json!({
                "sale_id": sale_id, "listing_id": listing_id, "buyer_id": buyer,
                "seller_id": listing.seller, "price": payment.to_string(), "generation": meta.generation.to_string()
            }),
        );

        ext_nft_marketplace::ext(listing.nft_contract_id)
            .with_static_gas(NFT_TRANSFER_GAS)
            .with_attached_deposit(near_sdk::NearToken::from_yoctonear(1))
            .marketplace_transfer_approved(
                listing.seller,
                listing.token_id,
                U64(listing.approval_id),
                buyer,
            )
            .then(
                ext_self_settlement::ext(env::current_account_id())
                    .with_static_gas(SETTLEMENT_CALLBACK_GAS)
                    .on_transfer_result(sale_id),
            )
            .into()
    }

    #[private]
    pub fn on_transfer_result(&mut self, sale_id: String) -> PromiseOrValue<()> {
        require!(
            env::attached_deposit().is_zero(),
            "Callback rejects deposits"
        );
        require!(
            env::promise_results_count() == 1,
            "Callback requires exactly one result"
        );
        let Some(mut settlement) = self.settlements.get(&sale_id) else {
            return PromiseOrValue::Value(());
        };
        if settlement.status != SettlementStatus::NftTransferPending {
            return PromiseOrValue::Value(());
        }
        let mut sale = self.sales.get(&sale_id).expect("Sale record missing");
        let mut meta = self
            .listing_meta
            .get(&settlement.listing_id)
            .expect("Listing metadata missing");
        let storage_before = env::storage_usage();
        require!(
            sale.purchase_generation == settlement.generation.0
                && sale.listing.nft_contract_id == settlement.nft_contract_id
                && sale.listing.token_id == settlement.token_id
                && sale.listing.seller == settlement.seller
                && sale.listing.approval_id == settlement.approval_id.0
                && sale.listing.price == settlement.price.0,
            "Settlement record mismatch"
        );
        let (seller_share, fee_share) = Self::split(settlement.price.0, sale.listing.fee_bps);
        require!(
            seller_share == settlement.seller_share.0 && fee_share == settlement.fee_share.0,
            "Settlement accounting mismatch"
        );

        self.ledger.pending_sale_liability = self
            .ledger
            .pending_sale_liability
            .checked_sub(settlement.price.0)
            .expect("Pending sale underflow");
        let delivered = env::promise_result_checked(0, 5)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<bool>(&bytes).ok())
            .unwrap_or(false);
        sale.settled = true;
        settlement.updated_at = U64(env::block_timestamp());
        if delivered {
            sale.nft_delivered = true;
            settlement.status = SettlementStatus::Settled;
            meta.status = ListingStatus::Sold;
            Self::checked_add_map(&mut self.seller_proceeds, &settlement.seller, seller_share);
            Self::checked_add_map(&mut self.fee_proceeds, &settlement.fee_recipient, fee_share);
            self.ledger.seller_liability = self
                .ledger
                .seller_liability
                .checked_add(seller_share)
                .expect("Seller liability overflow");
            self.ledger.fee_liability = self
                .ledger
                .fee_liability
                .checked_add(fee_share)
                .expect("Fee liability overflow");
            emit(
                "sale_settled",
                &serde_json::json!({
                    "sale_id": sale_id, "seller_share": seller_share.to_string(), "fee_share": fee_share.to_string()
                }),
            );
        } else {
            settlement.status = SettlementStatus::RefundAvailable;
            meta.status = ListingStatus::TransferFailed;
            Self::checked_add_map(
                &mut self.buyer_refunds,
                &settlement.buyer,
                settlement.price.0,
            );
            self.ledger.refund_liability = self
                .ledger
                .refund_liability
                .checked_add(settlement.price.0)
                .expect("Refund liability overflow");
            emit(
                "sale_failed",
                &serde_json::json!({
                    "sale_id": sale_id, "buyer_id": settlement.buyer, "refund": settlement.price.0.to_string()
                }),
            );
        }
        self.sales.insert(&sale_id, &sale);
        self.settlements.insert(&sale_id, &settlement);
        self.listing_meta.insert(&settlement.listing_id, &meta);
        let callback_bytes = env::storage_usage()
            .checked_sub(storage_before)
            .expect("Settlement storage decreased");
        require!(
            callback_bytes <= meta.reserved_sale_bytes,
            "Settlement storage reservation exhausted"
        );
        let unused = meta.reserved_sale_bytes - callback_bytes;
        if unused > 0 {
            self.release_storage(&meta.storage_payer, Self::storage_cost(unused));
        }
        meta.actual_sale_bytes = meta
            .actual_sale_bytes
            .checked_add(callback_bytes)
            .expect("Sale storage overflow");
        meta.reserved_sale_bytes = 0;
        self.listing_meta.insert(&settlement.listing_id, &meta);
        self.assert_backed();
        PromiseOrValue::Value(())
    }
}
