//! Callback-safe seller, fee, and buyer-refund withdrawals.

use super::*;
use crate::events::emit;
use near_sdk::{ext_contract, NearToken, Promise};

#[ext_contract(ext_self_withdraw)]
#[allow(dead_code)]
trait MarketplaceWithdrawal {
    fn on_withdrawal_result(&mut self, account_id: AccountId, nonce: U64) -> bool;
}

#[near]
impl Contract {
    #[payable]
    pub fn withdraw_proceeds(&mut self, amount: U128) -> Promise {
        near_sdk::assert_one_yocto();
        let caller = env::predecessor_account_id();
        self.begin_withdrawal(
            caller.clone(),
            caller,
            amount.0,
            WithdrawalKind::SellerProceeds,
            None,
        )
    }

    #[payable]
    pub fn withdraw_fees(&mut self, amount: U128) -> Promise {
        near_sdk::assert_one_yocto();
        let caller = env::predecessor_account_id();
        self.begin_withdrawal(
            caller.clone(),
            caller,
            amount.0,
            WithdrawalKind::MarketplaceFees,
            None,
        )
    }

    #[payable]
    pub fn withdraw_refund(&mut self, sale_id: String) -> Promise {
        near_sdk::assert_one_yocto();
        let settlement = self
            .settlements
            .get(&sale_id)
            .expect("Settlement not found");
        require!(
            settlement.status == SettlementStatus::RefundAvailable,
            "Refund is not available"
        );
        let caller = env::predecessor_account_id();
        require!(
            caller == settlement.buyer,
            "Only the buyer may withdraw this refund"
        );
        self.begin_withdrawal(
            caller.clone(),
            caller,
            settlement.price.0,
            WithdrawalKind::BuyerRefund,
            Some(sale_id),
        )
    }

    #[private]
    pub fn on_withdrawal_result(&mut self, account_id: AccountId, nonce: U64) -> bool {
        require!(
            env::attached_deposit().is_zero(),
            "Callback rejects deposits"
        );
        require!(
            env::promise_results_count() == 1,
            "Callback requires exactly one result"
        );
        let Some(pending) = self.pending_withdrawals.get(&account_id) else {
            return false;
        };
        if pending.nonce != nonce.0 {
            return false;
        }
        self.pending_withdrawals.remove(&account_id);
        if pending.temporary_storage_cost > 0 {
            let is_storage_kind = matches!(
                pending.kind,
                WithdrawalKind::StorageBalance | WithdrawalKind::RegistrationExcess
            );
            if is_storage_kind {
                self.release_protocol_storage(pending.temporary_storage_cost);
            } else {
                self.release_storage(&pending.account_id, pending.temporary_storage_cost);
            }
        }
        self.ledger.withdrawal_in_flight = self
            .ledger
            .withdrawal_in_flight
            .checked_sub(pending.amount)
            .expect("In-flight underflow");
        let success = env::promise_result_checked(0, 0).is_ok();
        if success {
            match pending.kind {
                WithdrawalKind::SellerProceeds => {
                    self.ledger.seller_withdrawn = self
                        .ledger
                        .seller_withdrawn
                        .checked_add(pending.amount)
                        .expect("Withdrawn overflow")
                }
                WithdrawalKind::MarketplaceFees => {
                    self.ledger.fee_withdrawn = self
                        .ledger
                        .fee_withdrawn
                        .checked_add(pending.amount)
                        .expect("Withdrawn overflow")
                }
                WithdrawalKind::BuyerRefund => {
                    self.ledger.refunds_withdrawn = self
                        .ledger
                        .refunds_withdrawn
                        .checked_add(pending.amount)
                        .expect("Withdrawn overflow");
                    if let Some(sale_id) = pending.reference_id.as_ref() {
                        if let Some(mut settlement) = self.settlements.get(sale_id) {
                            if settlement.status == SettlementStatus::RefundAvailable {
                                settlement.status = SettlementStatus::Refunded;
                                settlement.updated_at = U64(env::block_timestamp());
                                self.settlements.insert(sale_id, &settlement);
                            }
                        }
                    }
                }
                WithdrawalKind::StorageBalance | WithdrawalKind::RegistrationExcess => {}
            }
            emit(
                "withdrawal_succeeded",
                &serde_json::json!({
                    "account_id": account_id, "recipient_id": pending.recipient_id,
                    "amount": pending.amount.to_string(), "nonce": pending.nonce.to_string(), "kind": pending.kind
                }),
            );
        } else {
            self.restore_withdrawal(&pending);
            emit(
                "withdrawal_failed",
                &serde_json::json!({
                    "account_id": account_id, "amount": pending.amount.to_string(),
                    "nonce": pending.nonce.to_string(), "kind": pending.kind
                }),
            );
        }
        self.assert_backed();
        success
    }
}

impl Contract {
    pub(crate) fn begin_withdrawal(
        &mut self,
        account_id: AccountId,
        recipient_id: AccountId,
        amount: u128,
        kind: WithdrawalKind,
        reference_id: Option<String>,
    ) -> Promise {
        self.schedule_withdrawal(account_id, recipient_id, amount, kind, reference_id)
    }

    pub(crate) fn schedule_withdrawal(
        &mut self,
        account_id: AccountId,
        recipient_id: AccountId,
        amount: u128,
        kind: WithdrawalKind,
        reference_id: Option<String>,
    ) -> Promise {
        require!(amount > 0, "Amount must be positive");
        require!(
            !self.pending_withdrawals.contains_key(&account_id),
            "Withdrawal already pending"
        );
        require!(
            self.storage_accounts.contains_key(&account_id),
            "Storage deposit required for withdrawals"
        );

        let is_storage_kind = matches!(
            kind,
            WithdrawalKind::StorageBalance | WithdrawalKind::RegistrationExcess
        );

        let current_nonce = self.withdrawal_nonces.get(&account_id).unwrap_or(0);
        let nonce = current_nonce
            .checked_add(1)
            .expect("Withdrawal nonce exhausted");
        let nonce_before = env::storage_usage();
        self.withdrawal_nonces.insert(&account_id, &nonce);
        let mut protocol_reserve_delta = 0u128;
        if current_nonce == 0 {
            let nonce_cost = Self::storage_cost(
                env::storage_usage()
                    .checked_sub(nonce_before)
                    .expect("Nonce storage decreased"),
            );
            if is_storage_kind {
                protocol_reserve_delta = protocol_reserve_delta
                    .checked_add(nonce_cost)
                    .expect("Protocol reserve overflow");
            } else {
                self.charge_storage(&account_id, nonce_cost);
            }
        }
        let mut pending = PendingWithdrawal {
            account_id: account_id.clone(),
            recipient_id: recipient_id.clone(),
            amount,
            nonce,
            kind,
            reference_id,
            temporary_storage_cost: 0,
            created_at: env::block_timestamp(),
        };
        let pending_before = env::storage_usage();
        self.pending_withdrawals.insert(&account_id, &pending);
        let temporary_storage_cost = Self::storage_cost(
            env::storage_usage()
                .checked_sub(pending_before)
                .expect("Pending withdrawal storage decreased"),
        );
        if is_storage_kind {
            protocol_reserve_delta = protocol_reserve_delta
                .checked_add(temporary_storage_cost)
                .expect("Protocol reserve overflow");
        } else {
            self.charge_storage(&account_id, temporary_storage_cost);
        }
        self.reserve_protocol_storage(protocol_reserve_delta);
        pending.temporary_storage_cost = temporary_storage_cost;
        self.pending_withdrawals.insert(&account_id, &pending);

        match kind {
            WithdrawalKind::SellerProceeds => {
                Self::checked_sub_map(&mut self.seller_proceeds, &account_id, amount);
                self.ledger.seller_liability = self
                    .ledger
                    .seller_liability
                    .checked_sub(amount)
                    .expect("Seller liability underflow");
            }
            WithdrawalKind::MarketplaceFees => {
                Self::checked_sub_map(&mut self.fee_proceeds, &account_id, amount);
                self.ledger.fee_liability = self
                    .ledger
                    .fee_liability
                    .checked_sub(amount)
                    .expect("Fee liability underflow");
            }
            WithdrawalKind::BuyerRefund => {
                Self::checked_sub_map(&mut self.buyer_refunds, &account_id, amount);
                self.ledger.refund_liability = self
                    .ledger
                    .refund_liability
                    .checked_sub(amount)
                    .expect("Refund liability underflow");
            }
            WithdrawalKind::StorageBalance | WithdrawalKind::RegistrationExcess => {
                let mut storage = self
                    .storage_accounts
                    .get(&account_id)
                    .expect("Storage account not registered");
                storage.total = storage
                    .total
                    .checked_sub(amount)
                    .expect("Storage total underflow");
                self.storage_accounts.insert(&account_id, &storage);
                self.ledger.storage_liability = self
                    .ledger
                    .storage_liability
                    .checked_sub(amount)
                    .expect("Storage liability underflow");
            }
        }
        self.ledger.withdrawal_in_flight = self
            .ledger
            .withdrawal_in_flight
            .checked_add(amount)
            .expect("In-flight overflow");
        self.assert_backed();
        Promise::new(recipient_id)
            .transfer(NearToken::from_yoctonear(amount))
            .then(
                ext_self_withdraw::ext(env::current_account_id())
                    .with_static_gas(WITHDRAW_CALLBACK_GAS)
                    .on_withdrawal_result(account_id, U64(nonce)),
            )
    }

    fn restore_withdrawal(&mut self, pending: &PendingWithdrawal) {
        match pending.kind {
            WithdrawalKind::SellerProceeds => {
                Self::checked_add_map(
                    &mut self.seller_proceeds,
                    &pending.account_id,
                    pending.amount,
                );
                self.ledger.seller_liability = self
                    .ledger
                    .seller_liability
                    .checked_add(pending.amount)
                    .expect("Seller liability overflow");
            }
            WithdrawalKind::MarketplaceFees => {
                Self::checked_add_map(&mut self.fee_proceeds, &pending.account_id, pending.amount);
                self.ledger.fee_liability = self
                    .ledger
                    .fee_liability
                    .checked_add(pending.amount)
                    .expect("Fee liability overflow");
            }
            WithdrawalKind::BuyerRefund => {
                Self::checked_add_map(&mut self.buyer_refunds, &pending.account_id, pending.amount);
                self.ledger.refund_liability = self
                    .ledger
                    .refund_liability
                    .checked_add(pending.amount)
                    .expect("Refund liability overflow");
            }
            WithdrawalKind::StorageBalance => {
                let mut storage = self
                    .storage_accounts
                    .get(&pending.account_id)
                    .expect("Storage account missing");
                storage.total = storage
                    .total
                    .checked_add(pending.amount)
                    .expect("Storage total overflow");
                self.storage_accounts.insert(&pending.account_id, &storage);
                self.ledger.storage_liability = self
                    .ledger
                    .storage_liability
                    .checked_add(pending.amount)
                    .expect("Storage liability overflow");
            }
            WithdrawalKind::RegistrationExcess => {
                let mut storage = self
                    .storage_accounts
                    .get(&pending.account_id)
                    .expect("Storage account missing");
                storage.total = storage
                    .total
                    .checked_add(pending.amount)
                    .expect("Storage total overflow");
                self.storage_accounts.insert(&pending.account_id, &storage);
                self.ledger.storage_liability = self
                    .ledger
                    .storage_liability
                    .checked_add(pending.amount)
                    .expect("Storage liability overflow");
            }
        }
    }
}
