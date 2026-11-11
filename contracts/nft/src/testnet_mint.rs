//! Fresh testnet accounts only. BindingV2 root layout remains unchanged.
use super::*;
const AUTHORITY_KEY: &[u8] = b"nft:testnet:mint-authority:v1";

#[near]
impl Contract {
    #[init]
    pub fn new_testnet(
        mint_authority_id: AccountId,
        metadata: NFTContractMetadata,
        marketplace_id: AccountId,
    ) -> Self {
        require_self_initialization();
        for account in [
            &env::current_account_id(),
            &mint_authority_id,
            &marketplace_id,
        ] {
            require!(
                account.as_str().ends_with(".testnet"),
                "Testnet accounts required"
            );
        }
        require!(
            mint_authority_id != env::current_account_id() && mint_authority_id != marketplace_id,
            "Mint authority must be separate from contracts"
        );
        require!(
            !env::storage_has_key(AUTHORITY_KEY),
            "Mint authority already exists"
        );
        let contract =
            Self::new_with_marketplace(mint_authority_id.clone(), metadata, marketplace_id);
        env::storage_write(AUTHORITY_KEY, mint_authority_id.as_bytes());
        // Measure the complete fresh root, including host record overhead. The
        // SDK init wrapper subsequently writes these identical bytes, with no
        // further growth. Any failure aborts the entire initialization receipt.
        env::state_write(&contract);
        let cost = u128::from(env::storage_usage())
            .checked_mul(env::storage_byte_cost().as_yoctonear())
            .expect("Initialization cost overflow");
        require!(
            env::account_balance().as_yoctonear() >= cost,
            "Unfunded initialization"
        );
        contract
    }

    pub fn testnet_mint_authority(&self) -> Option<AccountId> {
        if !env::storage_has_key(AUTHORITY_KEY) {
            return None;
        }
        let register = 0x4e46_544d;
        // SAFETY: the static initialized key lives across this synchronous call.
        let status = unsafe {
            near_sys::storage_read(
                AUTHORITY_KEY.len() as u64,
                AUTHORITY_KEY.as_ptr() as u64,
                register,
            )
        };
        require!(status == 1, "Missing mint authority");
        let bytes = env::read_register_bounded(register, 64)
            .expect("Missing authority register")
            .expect("Oversized mint authority");
        Some(
            String::from_utf8(bytes)
                .expect("Invalid mint authority")
                .parse()
                .expect("Invalid authority account"),
        )
    }
}

impl Contract {
    pub(crate) fn authorize_testnet_mint(&self, recipient: &AccountId) {
        let authority = self
            .testnet_mint_authority()
            .expect("Testnet mint authority not configured");
        require!(
            env::current_account_id().as_str().ends_with(".testnet")
                && authority.as_str().ends_with(".testnet"),
            "Testnet identity required"
        );
        require!(
            env::predecessor_account_id() == authority && env::promise_results_count() == 0,
            "Only configured testnet mint authority may mint"
        );
        require!(
            recipient == &authority,
            "Testnet mint requires recipient consent via self-mint"
        );
        require!(
            self.tokens.owner_id == authority && self.marketplace_id.is_some(),
            "Invalid testnet binding"
        );
    }
}
