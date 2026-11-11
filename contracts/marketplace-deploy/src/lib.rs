//! Production-minded approval-based fixed-price NEAR marketplace.
//!
//! Native NEAR is the only settlement asset. Browser and database state are
//! never accepted as payment evidence; every financial transition is derived
//! from authenticated NEAR receipts and durable contract state.

use near_sdk::{
    collections::{LookupMap, UnorderedSet, Vector},
    env,
    json_types::{U128, U64},
    near, require, AccountId, Gas, PanicOnDefault,
};

mod events;
mod listing;
mod migration;
mod purchase;
mod storage;
mod withdraw;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "tests.rs"]
mod tests;

pub(crate) const MAX_TOKEN_ID_BYTES: usize = 1_024;
pub(crate) const MAX_APPROVAL_MSG_BYTES: usize = 4_096;
pub(crate) const MAX_FEE_BPS: u16 = 9_999;
pub(crate) const MAX_PAGE_SIZE: u64 = 100;
pub(crate) const SALE_STORAGE_RESERVE_BYTES: u64 = 8_192;
pub(crate) const NFT_TRANSFER_GAS: Gas = Gas::from_tgas(30);
pub(crate) const SETTLEMENT_CALLBACK_GAS: Gas = Gas::from_tgas(40);
pub(crate) const WITHDRAW_CALLBACK_GAS: Gas = Gas::from_tgas(20);
pub(crate) const EVENT_STANDARD: &str = "near_marketplace";
pub(crate) const EVENT_VERSION: &str = "1.0.0";

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listing {
    pub nft_contract_id: AccountId,
    pub token_id: String,
    pub seller: AccountId,
    pub approval_id: u64,
    pub price: u128,
    pub fee_bps: u64,
    pub fee_account: Option<AccountId>,
    pub created_at: u64,
    pub cancelled_at: Option<u64>,
    pub active: bool,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sale {
    pub listing_id: String,
    pub listing: Listing,
    pub buyer: AccountId,
    pub purchase_generation: u64,
    pub created_at: u64,
    pub settled: bool,
    pub nft_delivered: bool,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ListingStatus {
    Active,
    PendingSale,
    Sold,
    Cancelled,
    Expired,
    Replaced,
    TransferFailed,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListingMeta {
    pub generation: u64,
    pub expires_at: Option<u64>,
    pub status: ListingStatus,
    pub storage_payer: AccountId,
    pub reserved_sale_bytes: u64,
    pub actual_sale_bytes: u64,
    pub terms_hash: Vec<u8>,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug, Default)]
pub(crate) struct AssetState {
    pub generation: u64,
    pub last_approval_id: u64,
    pub current_listing_id: Option<String>,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SettlementStatus {
    NftTransferPending,
    Settled,
    RefundAvailable,
    Refunded,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettlementRecord {
    pub sale_id: String,
    pub listing_id: String,
    pub nft_contract_id: AccountId,
    pub token_id: String,
    pub seller: AccountId,
    pub buyer: AccountId,
    pub approval_id: U64,
    pub generation: U64,
    pub price: U128,
    pub seller_share: U128,
    pub fee_share: U128,
    pub fee_recipient: AccountId,
    pub status: SettlementStatus,
    pub created_at: U64,
    pub updated_at: U64,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ledger {
    pub seller_liability: u128,
    pub fee_liability: u128,
    pub refund_liability: u128,
    pub pending_sale_liability: u128,
    pub withdrawal_in_flight: u128,
    pub storage_liability: u128,
    pub seller_withdrawn: u128,
    pub fee_withdrawn: u128,
    pub refunds_withdrawn: u128,
    /// Protocol NEAR reserved to back withdrawal-control storage (nonce entries
    /// and pending records) that is not charged to NEP-145 user storage balances.
    /// Permanent for nonce entries while they exist; temporary portions are
    /// released when the corresponding pending withdrawal callback completes.
    pub control_storage_reserve: u128,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerView {
    pub seller_liability: U128,
    pub fee_liability: U128,
    pub refund_liability: U128,
    pub pending_sale_liability: U128,
    pub withdrawal_in_flight: U128,
    pub storage_liability: U128,
    pub seller_withdrawn: U128,
    pub fee_withdrawn: U128,
    pub refunds_withdrawn: U128,
    pub control_storage_reserve: U128,
}

impl From<Ledger> for LedgerView {
    fn from(value: Ledger) -> Self {
        Self {
            seller_liability: U128(value.seller_liability),
            fee_liability: U128(value.fee_liability),
            refund_liability: U128(value.refund_liability),
            pending_sale_liability: U128(value.pending_sale_liability),
            withdrawal_in_flight: U128(value.withdrawal_in_flight),
            storage_liability: U128(value.storage_liability),
            seller_withdrawn: U128(value.seller_withdrawn),
            fee_withdrawn: U128(value.fee_withdrawn),
            refunds_withdrawn: U128(value.refunds_withdrawn),
            control_storage_reserve: U128(value.control_storage_reserve),
        }
    }
}

impl Ledger {
    pub(crate) fn total_liability(&self) -> u128 {
        self.seller_liability
            .checked_add(self.fee_liability)
            .and_then(|v| v.checked_add(self.refund_liability))
            .and_then(|v| v.checked_add(self.pending_sale_liability))
            .and_then(|v| v.checked_add(self.withdrawal_in_flight))
            .and_then(|v| v.checked_add(self.storage_liability))
            .and_then(|v| v.checked_add(self.control_storage_reserve))
            .expect("Liability overflow")
    }
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub nft_contract_id: AccountId,
    pub owner: AccountId,
    pub fee_bps: u64,
    pub fee_account: Option<AccountId>,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarketplaceConfigView {
    pub owner: AccountId,
    pub fee_bps: u16,
    pub fee_account: AccountId,
    pub primary_nft_contract_id: AccountId,
    pub nft_contract_id: AccountId,
    pub allowed_nft_contracts: Vec<AccountId>,
    pub trading_enabled: bool,
    pub state_version: u8,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StorageAccount {
    pub total: u128,
    pub used: u128,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListingView {
    pub nft_contract_id: AccountId,
    pub token_id: String,
    pub seller: AccountId,
    pub approval_id: U64,
    pub price: U128,
    pub fee_bps: u64,
    pub fee_account: Option<AccountId>,
    pub created_at: U64,
    pub cancelled_at: Option<U64>,
    pub active: bool,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListingMetaView {
    pub generation: U64,
    pub expires_at: Option<U64>,
    pub status: ListingStatus,
    pub storage_payer: AccountId,
    pub reserved_sale_bytes: U64,
    pub actual_sale_bytes: U64,
    pub terms_hash: Vec<u8>,
}

impl From<ListingMeta> for ListingMetaView {
    fn from(value: ListingMeta) -> Self {
        Self {
            generation: U64(value.generation),
            expires_at: value.expires_at.map(U64),
            status: value.status,
            storage_payer: value.storage_payer,
            reserved_sale_bytes: U64(value.reserved_sale_bytes),
            actual_sale_bytes: U64(value.actual_sale_bytes),
            terms_hash: value.terms_hash,
        }
    }
}

impl From<Listing> for ListingView {
    fn from(value: Listing) -> Self {
        Self {
            nft_contract_id: value.nft_contract_id,
            token_id: value.token_id,
            seller: value.seller,
            approval_id: U64(value.approval_id),
            price: U128(value.price),
            fee_bps: value.fee_bps,
            fee_account: value.fee_account,
            created_at: U64(value.created_at),
            cancelled_at: value.cancelled_at.map(U64),
            active: value.active,
        }
    }
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaleView {
    pub listing_id: String,
    pub listing: ListingView,
    pub buyer: AccountId,
    pub purchase_generation: U64,
    pub created_at: U64,
    pub settled: bool,
    pub nft_delivered: bool,
}

impl From<Sale> for SaleView {
    fn from(value: Sale) -> Self {
        Self {
            listing_id: value.listing_id,
            listing: value.listing.into(),
            buyer: value.buyer,
            purchase_generation: U64(value.purchase_generation),
            created_at: U64(value.created_at),
            settled: value.settled,
            nft_delivered: value.nft_delivered,
        }
    }
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WithdrawalKind {
    SellerProceeds,
    MarketplaceFees,
    BuyerRefund,
    StorageBalance,
    RegistrationExcess,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingWithdrawal {
    pub account_id: AccountId,
    pub recipient_id: AccountId,
    pub amount: u128,
    pub nonce: u64,
    pub kind: WithdrawalKind,
    pub reference_id: Option<String>,
    pub temporary_storage_cost: u128,
    pub created_at: u64,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingWithdrawalView {
    pub account_id: AccountId,
    pub recipient_id: AccountId,
    pub amount: U128,
    pub nonce: U64,
    pub kind: WithdrawalKind,
    pub reference_id: Option<String>,
    pub temporary_storage_cost: U128,
    pub created_at: U64,
}

impl From<PendingWithdrawal> for PendingWithdrawalView {
    fn from(value: PendingWithdrawal) -> Self {
        Self {
            account_id: value.account_id,
            recipient_id: value.recipient_id,
            amount: U128(value.amount),
            nonce: U64(value.nonce),
            kind: value.kind,
            reference_id: value.reference_id,
            temporary_storage_cost: U128(value.temporary_storage_cost),
            created_at: U64(value.created_at),
        }
    }
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    pub(crate) config: Config,
    pub(crate) listings: LookupMap<String, Listing>,
    pub(crate) sales: LookupMap<String, Sale>,
    pub(crate) seller_proceeds: LookupMap<AccountId, u128>,
    pub(crate) ledger: Ledger,
    pub(crate) state_version: u8,
    pub(crate) trading_enabled: bool,
    pub(crate) allowed_nft_contracts: UnorderedSet<AccountId>,
    pub(crate) allowlist_storage_paid: LookupMap<AccountId, u128>,
    pub(crate) listing_meta: LookupMap<String, ListingMeta>,
    pub(crate) asset_states: LookupMap<Vec<u8>, AssetState>,
    pub(crate) settlements: LookupMap<String, SettlementRecord>,
    pub(crate) fee_proceeds: LookupMap<AccountId, u128>,
    pub(crate) buyer_refunds: LookupMap<AccountId, u128>,
    pub(crate) storage_accounts: LookupMap<AccountId, StorageAccount>,
    pub(crate) pending_withdrawals: LookupMap<AccountId, PendingWithdrawal>,
    pub(crate) withdrawal_nonces: LookupMap<AccountId, u64>,
    pub(crate) listing_ids: Vector<String>,
    pub(crate) sale_ids: Vector<String>,
}

#[near]
impl Contract {
    #[init]
    pub fn new(
        nft_contract_id: AccountId,
        owner: AccountId,
        fee_bps: u64,
        fee_account: Option<AccountId>,
    ) -> Self {
        Self::assert_direct_self_call();
        Self::validate_fee_policy(fee_bps, fee_account.as_ref());
        require!(
            nft_contract_id != env::current_account_id(),
            "Invalid NFT contract id"
        );
        require!(
            owner != env::current_account_id() && owner != nft_contract_id,
            "Invalid marketplace owner"
        );
        let mut allowed_nft_contracts = UnorderedSet::new(b"an".to_vec());
        allowed_nft_contracts.insert(&nft_contract_id);
        Self {
            config: Config {
                nft_contract_id,
                owner,
                fee_bps,
                fee_account,
            },
            listings: LookupMap::new(b"l"),
            sales: LookupMap::new(b"s"),
            seller_proceeds: LookupMap::new(b"p"),
            ledger: Ledger::default(),
            state_version: 2,
            trading_enabled: false,
            allowed_nft_contracts,
            allowlist_storage_paid: LookupMap::new(b"ap".to_vec()),
            listing_meta: LookupMap::new(b"lm".to_vec()),
            asset_states: LookupMap::new(b"as".to_vec()),
            settlements: LookupMap::new(b"st".to_vec()),
            fee_proceeds: LookupMap::new(b"fp".to_vec()),
            buyer_refunds: LookupMap::new(b"br".to_vec()),
            storage_accounts: LookupMap::new(b"sa".to_vec()),
            pending_withdrawals: LookupMap::new(b"pw".to_vec()),
            withdrawal_nonces: LookupMap::new(b"wn".to_vec()),
            listing_ids: Vector::new(b"li".to_vec()),
            sale_ids: Vector::new(b"si".to_vec()),
        }
    }

    pub fn config(&self) -> MarketplaceConfigView {
        MarketplaceConfigView {
            owner: self.config.owner.clone(),
            fee_bps: self.config.fee_bps as u16,
            fee_account: self.fee_recipient(),
            primary_nft_contract_id: self.config.nft_contract_id.clone(),
            nft_contract_id: self.config.nft_contract_id.clone(),
            allowed_nft_contracts: self.allowed_nft_contracts.to_vec(),
            trading_enabled: self.trading_enabled,
            state_version: self.state_version,
        }
    }
    pub fn ledger(&self) -> LedgerView {
        self.ledger.clone().into()
    }
    pub fn listing(&self, listing_id: String) -> Option<ListingView> {
        self.listings.get(&listing_id).map(Into::into)
    }
    pub fn listing_metadata(&self, listing_id: String) -> Option<ListingMetaView> {
        self.listing_meta.get(&listing_id).map(Into::into)
    }
    pub fn sale(&self, sale_id: String) -> Option<SaleView> {
        self.sales.get(&sale_id).map(Into::into)
    }
    pub fn settlement(&self, sale_id: String) -> Option<SettlementRecord> {
        self.settlements.get(&sale_id)
    }
    pub fn seller_proceeds(&self, seller_id: AccountId) -> U128 {
        U128(self.seller_proceeds.get(&seller_id).unwrap_or(0))
    }
    pub fn fee_proceeds(&self, account_id: AccountId) -> U128 {
        U128(self.fee_proceeds.get(&account_id).unwrap_or(0))
    }
    pub fn buyer_refund(&self, buyer_id: AccountId) -> U128 {
        U128(self.buyer_refunds.get(&buyer_id).unwrap_or(0))
    }
    pub fn pending_withdrawal(&self, account_id: AccountId) -> Option<PendingWithdrawalView> {
        self.pending_withdrawals.get(&account_id).map(Into::into)
    }

    pub fn listings(&self, from_index: Option<U64>, limit: Option<u64>) -> Vec<ListingView> {
        let start = from_index.unwrap_or(U64(0)).0;
        let limit = limit.unwrap_or(50).min(MAX_PAGE_SIZE);
        (start..start.saturating_add(limit).min(self.listing_ids.len()))
            .filter_map(|i| self.listing_ids.get(i))
            .filter_map(|id| self.listings.get(&id))
            .map(Into::into)
            .collect()
    }

    pub fn transactions(
        &self,
        from_index: Option<U64>,
        limit: Option<u64>,
    ) -> Vec<SettlementRecord> {
        let start = from_index.unwrap_or(U64(0)).0;
        let limit = limit.unwrap_or(50).min(MAX_PAGE_SIZE);
        (start..start.saturating_add(limit).min(self.sale_ids.len()))
            .filter_map(|i| self.sale_ids.get(i))
            .filter_map(|id| self.settlements.get(&id))
            .collect()
    }
}

impl Contract {
    pub(crate) fn assert_direct_self_call() {
        require!(
            env::predecessor_account_id() == env::current_account_id()
                && env::signer_account_id() == env::current_account_id()
                && env::promise_results_count() == 0
                && env::attached_deposit().is_zero(),
            "Initialization requires a direct zero-deposit self-call"
        );
    }
    pub(crate) fn assert_owner(&self) {
        require!(
            env::predecessor_account_id() == self.config.owner,
            "Owner only"
        );
    }
    pub(crate) fn validate_fee_policy(fee_bps: u64, fee_account: Option<&AccountId>) {
        require!(
            fee_bps <= u64::from(MAX_FEE_BPS),
            "Fee basis points must leave a seller share"
        );
        if let Some(account) = fee_account {
            require!(account != &env::current_account_id(), "Invalid fee account");
        }
    }
    pub(crate) fn fee_recipient(&self) -> AccountId {
        self.config
            .fee_account
            .clone()
            .unwrap_or_else(|| self.config.owner.clone())
    }
    pub(crate) fn asset_key(nft_contract_id: &AccountId, token_id: &str) -> Vec<u8> {
        let mut value = nft_contract_id.as_bytes().to_vec();
        value.push(0);
        value.extend_from_slice(token_id.as_bytes());
        env::sha256(&value)
    }
    pub(crate) fn listing_id(
        nft_contract_id: &AccountId,
        token_id: &str,
        seller: &AccountId,
        generation: u64,
    ) -> String {
        format!("{}|{}|{}|{}", nft_contract_id, token_id, seller, generation)
    }
    pub(crate) fn sale_id(listing_id: &str, buyer: &AccountId, generation: u64) -> String {
        format!("{}|{}|{}", listing_id, buyer, generation)
    }
    pub(crate) fn split(price: u128, fee_bps: u64) -> (u128, u128) {
        let bps = u128::from(fee_bps);
        let fee = (price / 10_000)
            .checked_mul(bps)
            .and_then(|base| base.checked_add((price % 10_000).checked_mul(bps)? / 10_000))
            .expect("Fee overflow");
        (price.checked_sub(fee).expect("Split underflow"), fee)
    }
    pub(crate) fn checked_add_map(
        map: &mut LookupMap<AccountId, u128>,
        account: &AccountId,
        amount: u128,
    ) {
        let value = map
            .get(account)
            .unwrap_or(0)
            .checked_add(amount)
            .expect("Balance overflow");
        map.insert(account, &value);
    }
    pub(crate) fn checked_sub_map(
        map: &mut LookupMap<AccountId, u128>,
        account: &AccountId,
        amount: u128,
    ) {
        let value = map.get(account).unwrap_or(0);
        require!(value >= amount, "Insufficient available balance");
        map.insert(
            account,
            &value.checked_sub(amount).expect("Balance underflow"),
        );
    }
    pub(crate) fn storage_cost(bytes: u64) -> u128 {
        u128::from(bytes)
            .checked_mul(env::storage_byte_cost().as_yoctonear())
            .expect("Storage cost overflow")
    }
    pub(crate) fn protocol_storage_available(&self) -> u128 {
        let storage_stake = Self::storage_cost(env::storage_usage());
        env::account_balance()
            .as_yoctonear()
            .saturating_sub(storage_stake)
            .saturating_sub(self.ledger.total_liability())
    }
    pub(crate) fn reserve_protocol_storage(&mut self, amount: u128) {
        if amount == 0 {
            return;
        }
        let available = self.protocol_storage_available();
        require!(
            available >= amount,
            "Insufficient contract protocol storage reserve"
        );
        self.ledger.control_storage_reserve = self
            .ledger
            .control_storage_reserve
            .checked_add(amount)
            .expect("Control storage reserve overflow");
    }
    pub(crate) fn release_protocol_storage(&mut self, amount: u128) {
        if amount == 0 {
            return;
        }
        self.ledger.control_storage_reserve = self
            .ledger
            .control_storage_reserve
            .checked_sub(amount)
            .expect("Control storage reserve underflow");
    }
    pub(crate) fn charge_storage(&mut self, account: &AccountId, amount: u128) {
        let mut balance = self
            .storage_accounts
            .get(account)
            .expect("Storage deposit required");
        let available = balance
            .total
            .checked_sub(balance.used)
            .expect("Invalid storage balance");
        require!(
            available >= amount,
            "Insufficient marketplace storage balance"
        );
        balance.used = balance
            .used
            .checked_add(amount)
            .expect("Storage balance overflow");
        self.ledger.storage_liability = self
            .ledger
            .storage_liability
            .checked_sub(amount)
            .expect("Storage liability underflow");
        self.storage_accounts.insert(account, &balance);
    }
    pub(crate) fn release_storage(&mut self, account: &AccountId, amount: u128) {
        let mut balance = self
            .storage_accounts
            .get(account)
            .expect("Storage account missing");
        balance.used = balance
            .used
            .checked_sub(amount)
            .expect("Storage release underflow");
        self.ledger.storage_liability = self
            .ledger
            .storage_liability
            .checked_add(amount)
            .expect("Storage liability overflow");
        self.storage_accounts.insert(account, &balance);
    }
    pub(crate) fn assert_backed(&self) {
        let storage_stake = Self::storage_cost(env::storage_usage());
        let liquid = env::account_balance()
            .as_yoctonear()
            .saturating_sub(storage_stake);
        require!(
            liquid >= self.ledger.total_liability(),
            "Contract liabilities are not fully backed"
        );
    }
}
