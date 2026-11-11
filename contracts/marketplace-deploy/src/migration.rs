//! Fail-closed migration from the initial marketplace state layout.

use super::*;

#[near(serializers = [borsh])]
#[derive(Clone)]
pub(crate) struct OldLedger {
    pub(crate) seller_liability: u128,
    pub(crate) fee_liability: u128,
    pub(crate) seller_withdrawn: u128,
    pub(crate) fee_withdrawn: u128,
}

#[near(serializers = [borsh])]
pub(crate) struct OldContract {
    pub(crate) config: Config,
    pub(crate) listings: LookupMap<String, Listing>,
    pub(crate) sales: LookupMap<String, Sale>,
    pub(crate) seller_proceeds: LookupMap<AccountId, u128>,
    pub(crate) ledger: OldLedger,
}

#[near]
impl Contract {
    /// Upgrade entry point. Trading always remains disabled after migration so
    /// operators must first prove that all pre-upgrade callbacks were drained.
    #[init(ignore_state)]
    pub fn migrate() -> Self {
        Self::assert_direct_self_call();
        let old: OldContract = env::state_read().expect("Legacy state is missing or incompatible");
        let seller_liability = old
            .ledger
            .seller_liability
            .checked_sub(old.ledger.seller_withdrawn)
            .expect("Legacy seller ledger is inconsistent");
        let fee_liability = old
            .ledger
            .fee_liability
            .checked_sub(old.ledger.fee_withdrawn)
            .expect("Legacy fee ledger is inconsistent");
        let mut allowed_nft_contracts = UnorderedSet::new(b"an".to_vec());
        allowed_nft_contracts.insert(&old.config.nft_contract_id);
        let mut fee_proceeds = LookupMap::new(b"fp".to_vec());
        let fee_recipient = old
            .config
            .fee_account
            .clone()
            .unwrap_or_else(|| old.config.owner.clone());
        if fee_liability > 0 {
            fee_proceeds.insert(&fee_recipient, &fee_liability);
        }
        Self {
            config: old.config,
            listings: old.listings,
            sales: old.sales,
            seller_proceeds: old.seller_proceeds,
            ledger: Ledger {
                seller_liability,
                fee_liability,
                seller_withdrawn: old.ledger.seller_withdrawn,
                fee_withdrawn: old.ledger.fee_withdrawn,
                control_storage_reserve: 0,
                ..Ledger::default()
            },
            state_version: 2,
            trading_enabled: false,
            allowed_nft_contracts,
            allowlist_storage_paid: LookupMap::new(b"ap".to_vec()),
            listing_meta: LookupMap::new(b"lm".to_vec()),
            asset_states: LookupMap::new(b"as".to_vec()),
            settlements: LookupMap::new(b"st".to_vec()),
            fee_proceeds,
            buyer_refunds: LookupMap::new(b"br".to_vec()),
            storage_accounts: LookupMap::new(b"sa".to_vec()),
            pending_withdrawals: LookupMap::new(b"pw".to_vec()),
            withdrawal_nonces: LookupMap::new(b"wn".to_vec()),
            listing_ids: Vector::new(b"li".to_vec()),
            sale_ids: Vector::new(b"si".to_vec()),
        }
    }
}
