//! NEP-145 compatible prefunded storage accounting.

use super::*;
use near_contract_standards::storage_management::{
    StorageBalance, StorageBalanceBounds, StorageManagement,
};
use near_sdk::NearToken;

impl Contract {
    fn storage_view(account: StorageAccount) -> StorageBalance {
        StorageBalance {
            total: NearToken::from_yoctonear(account.total),
            available: NearToken::from_yoctonear(
                account
                    .total
                    .checked_sub(account.used)
                    .expect("Invalid storage balance"),
            ),
        }
    }
}

#[near]
impl StorageManagement for Contract {
    #[payable]
    fn storage_deposit(
        &mut self,
        account_id: Option<AccountId>,
        registration_only: Option<bool>,
    ) -> StorageBalance {
        let account_id = account_id.unwrap_or_else(env::predecessor_account_id);
        let recipient = env::predecessor_account_id();
        let deposit = env::attached_deposit().as_yoctonear();
        require!(deposit > 0, "Storage deposit must be positive");
        let existed = self.storage_accounts.get(&account_id);
        let mut balance = existed.clone().unwrap_or_default();
        let before = env::storage_usage();
        balance.total = balance
            .total
            .checked_add(deposit)
            .expect("Storage total overflow");
        self.storage_accounts.insert(&account_id, &balance);
        let registration_cost = if existed.is_none() {
            Self::storage_cost(
                env::storage_usage()
                    .checked_sub(before)
                    .expect("Registration storage decreased"),
            )
        } else {
            0
        };
        if registration_cost > 0 {
            require!(
                deposit >= registration_cost,
                "Insufficient storage registration deposit"
            );
            balance.used = registration_cost;
            self.storage_accounts.insert(&account_id, &balance);
        }
        let refundable = deposit
            .checked_sub(registration_cost)
            .expect("Registration underflow");
        self.ledger.storage_liability = self
            .ledger
            .storage_liability
            .checked_add(refundable)
            .expect("Storage liability overflow");

        let registration_only = registration_only.unwrap_or(false);
        if registration_only && refundable > 0 {
            let _ = self.schedule_withdrawal(
                account_id.clone(),
                recipient,
                refundable,
                WithdrawalKind::RegistrationExcess,
                None,
            );
        }
        self.assert_backed();
        Self::storage_view(
            self.storage_accounts
                .get(&account_id)
                .expect("Storage account missing"),
        )
    }

    #[payable]
    fn storage_withdraw(&mut self, amount: Option<NearToken>) -> StorageBalance {
        near_sdk::assert_one_yocto();
        let account_id = env::predecessor_account_id();
        let balance = self
            .storage_accounts
            .get(&account_id)
            .expect("Storage account not registered");
        let available = balance
            .total
            .checked_sub(balance.used)
            .expect("Invalid storage balance");
        let amount = amount
            .map(|value| value.as_yoctonear())
            .unwrap_or(available);
        require!(
            amount <= available,
            "Storage withdrawal exceeds available balance"
        );
        if amount > 0 {
            let _ = self.schedule_withdrawal(
                account_id.clone(),
                account_id.clone(),
                amount,
                WithdrawalKind::StorageBalance,
                None,
            );
        }
        self.assert_backed();
        Self::storage_view(
            self.storage_accounts
                .get(&account_id)
                .expect("Storage account missing"),
        )
    }

    #[payable]
    fn storage_unregister(&mut self, _force: Option<bool>) -> bool {
        near_sdk::assert_one_yocto();
        let account_id = env::predecessor_account_id();
        if self.storage_accounts.get(&account_id).is_none() {
            return false;
        }
        env::panic_str(
            "Storage unregister is disabled because replay barriers and history are permanent",
        )
    }

    fn storage_balance_bounds(&self) -> StorageBalanceBounds {
        StorageBalanceBounds {
            min: NearToken::from_yoctonear(Self::storage_cost(128)),
            max: None,
        }
    }

    fn storage_balance_of(&self, account_id: AccountId) -> Option<StorageBalance> {
        self.storage_accounts
            .get(&account_id)
            .map(Self::storage_view)
    }
}
