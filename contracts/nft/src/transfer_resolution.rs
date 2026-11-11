//! Versioned NEP-171 resolution. No old callback can change a newer ownership
//! epoch, recreate an absent map, or resurrect superseded approval authority.
use super::*;
use near_contract_standards::non_fungible_token::{core::ext_nft_receiver, events::NftTransfer};
use near_sdk::ext_contract;

#[ext_contract(ext_versioned_resolver)]
#[allow(dead_code)]
trait VersionedResolver {
    fn nft_resolve_transfer_versioned(
        &mut self,
        previous_owner_id: AccountId,
        receiver_id: AccountId,
        token_id: TokenId,
        approved_account_ids: Option<HashMap<AccountId, u64>>,
        ownership_version: U64,
        approval_version: U64,
    ) -> bool;
}

impl Contract {
    pub(crate) fn transfer_call_versioned(
        &mut self,
        receiver_id: AccountId,
        token_id: TokenId,
        approval_id: Option<u64>,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<bool> {
        near_sdk::assert_one_yocto();
        assert_bounded_token_id(&token_id);
        require!(
            msg.len() <= MAX_APPROVAL_MSG_BYTES,
            "Transfer message exceeds byte limit"
        );
        let version = mutation::Version::load(&token_id)
            .expect("Funded mutation version required before transfer-call")
            .transfer_next();
        let accounts = approval_storage::bounded_approvals(&self.tokens, &token_id);
        let existed = self
            .tokens
            .approvals_by_id
            .as_ref()
            .unwrap()
            .contains_key(&token_id);
        let sender = env::predecessor_account_id();
        let (owner, _) =
            self.tokens
                .internal_transfer(&sender, &receiver_id, &token_id, approval_id, memo);
        version.write(&token_id);
        ext_nft_receiver::ext(receiver_id.clone())
            .with_unused_gas_weight(1)
            .nft_on_transfer(sender, owner.clone(), token_id.clone(), msg)
            .then(
                ext_versioned_resolver::ext(env::current_account_id())
                    // Technical allocation, not measured node-level gas assurance.
                    .with_static_gas(Gas::from_tgas(20))
                    .with_unused_gas_weight(0)
                    .nft_resolve_transfer_versioned(
                        owner,
                        receiver_id,
                        token_id,
                        existed.then_some(accounts),
                        U64(version.ownership),
                        U64(version.approval),
                    ),
            )
            .into()
    }
}

#[near]
impl Contract {
    #[private]
    pub fn nft_resolve_transfer_versioned(
        &mut self,
        previous_owner_id: AccountId,
        receiver_id: AccountId,
        token_id: TokenId,
        approved_account_ids: Option<HashMap<AccountId, u64>>,
        ownership_version: U64,
        approval_version: U64,
    ) -> bool {
        require!(
            env::predecessor_account_id() == env::current_account_id(),
            "Private resolver"
        );
        require!(
            env::attached_deposit().is_zero(),
            "Resolver rejects deposit"
        );
        require!(
            env::promise_results_count() == 1,
            "Resolver requires one result"
        );
        assert_bounded_token_id(&token_id);
        require!(
            approved_account_ids
                .as_ref()
                .is_none_or(|a| a.len() <= MAX_APPROVALS_PER_TOKEN),
            "Approval count limit reached"
        );
        let version = mutation::Version::load(&token_id).expect("Missing mutation version");
        if version.ownership != ownership_version.0
            || self.tokens.owner_by_id.get(&token_id).as_ref() != Some(&receiver_id)
        {
            return true; // stale/burned/forwarded: no writes, approvals or refunds
        }
        let next = version.transfer_next();
        let must_return = env::promise_result_checked(0, 5)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<bool>(&bytes).ok())
            .unwrap_or(true);
        if !must_return {
            next.write(&token_id); // consume this resolver even if owner did not change
            return true;
        }
        // Only receiver approvals actually removed here can be refunded. Empty
        // legacy entries have no demonstrated funding and are never refundable.
        let before = env::storage_usage();
        let receiver_approvals = self
            .tokens
            .approvals_by_id
            .as_mut()
            .unwrap()
            .remove(&token_id);
        if receiver_approvals.as_ref().is_some_and(|a| !a.is_empty()) {
            refund_released_approval_storage(before, receiver_id.clone());
        }
        self.tokens
            .internal_transfer_unguarded(&token_id, &receiver_id, &previous_owner_id);
        if version.approval == approval_version.0 {
            // The original map was removed without a refund at dispatch. Restore
            // only nonempty funded bytes; never materialize an absent/empty map.
            if let Some(accounts) = approved_account_ids.filter(|a| !a.is_empty()) {
                self.tokens
                    .approvals_by_id
                    .as_mut()
                    .unwrap()
                    .insert(&token_id, &accounts);
            }
        }
        next.write(&token_id);
        NftTransfer {
            old_owner_id: &receiver_id,
            new_owner_id: &previous_owner_id,
            token_ids: &[&token_id],
            authorized_id: None,
            memo: None,
        }
        .emit();
        false
    }
}
