use super::{ApprovedTransferCall, IntegrationError, Result};
use near_marketplace_foundation::{
    persistence::{DeploymentDomain, OperationContext},
    state::ListingTerms,
};
use near_sdk::{
    env,
    json_types::{U128, U64},
    serde::{
        de::{Error, MapAccess, Visitor},
        Deserialize, Deserializer,
    },
    serde_json, AccountId, Gas, GasWeight, NearToken, Promise,
};
use std::collections::BTreeMap;

pub(super) const VERIFY_ABI: &[u8] = b"near-nft-5.30:verify:v1:nft_token(token_id:string)->Token|null;nft_is_approved(token_id:string,approved_account_id:AccountId,approval_id:u64-number)->bool;ordered-join";
pub(super) const CALLBACK_ABI: &[u8] = b"nmp:on_nft_verified:v1(generation:U64-string,attempt_nonce:U64-string)->active|invalid|ignored;results=2";
pub(super) const TRANSFER_ABI: &[u8] = b"nft:marketplace_transfer_approved:v1(expected_seller_id:AccountId,token_id:string,approval_id:U64-string,receiver_id:AccountId);deposit=1;sync-json-true";

pub(super) fn hash(bytes: &[u8]) -> [u8; 32] {
    env::sha256(bytes).try_into().unwrap()
}

#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde", deny_unknown_fields)]
pub(super) struct Message {
    version: u16,
    pub price: U128,
    pub expires_at_ns: U64,
    pub policy_version: U64,
}
pub(super) fn message(msg: &str, max: usize) -> Result<Message> {
    if msg.len() > max {
        return Err(IntegrationError::Message);
    }
    let msg: Message = serde_json::from_str(msg).map_err(|_| IntegrationError::Message)?;
    if msg.version != 1
        || msg.price.0 == 0
        || msg.policy_version.0 == 0
        || msg.expires_at_ns.0 <= env::block_timestamp()
    {
        return Err(IntegrationError::Message);
    }
    Ok(msg)
}

pub(super) struct ViewCall {
    pub target: AccountId,
    pub method: &'static str,
    pub args: Vec<u8>,
    pub gas: u64,
}
impl ViewCall {
    pub fn schedule(self) -> Promise {
        Promise::new(self.target).function_call_weight(
            self.method,
            self.args,
            NearToken::ZERO,
            Gas::from_gas(self.gas),
            GasWeight(0),
        )
    }
}
pub(super) fn verification_calls(op: &OperationContext) -> Result<[ViewCall; 2]> {
    let (_, terms) = op.verification().ok_or(IntegrationError::Context)?;
    let b = op.budgets();
    if b.call_abi != hash(VERIFY_ABI)
        || b.callback_abi != hash(CALLBACK_ABI)
        || b.max_result_bytes > 16 * 1024
        || b.max_result_bytes < 5
    {
        return Err(IntegrationError::Context);
    }
    op.commitment()?;
    Ok([
        ViewCall { target: op.domain().nft.clone(), method: "nft_token", gas: b.first_call_gas,
            args: serde_json::to_vec(&serde_json::json!({"token_id": terms.token.token()})).map_err(|_| IntegrationError::Context)? },
        ViewCall { target: op.domain().nft.clone(), method: "nft_is_approved", gas: b.second_call_gas,
            args: serde_json::to_vec(&serde_json::json!({"token_id": terms.token.token(), "approved_account_id": op.domain().marketplace, "approval_id": terms.approval_id})).map_err(|_| IntegrationError::Context)? },
    ])
}
pub(super) fn callback_args(generation: u64, nonce: u64) -> Result<Vec<u8>> {
    serde_json::to_vec(
        &serde_json::json!({"generation": U64(generation), "attempt_nonce": U64(nonce)}),
    )
    .map_err(|_| IntegrationError::Context)
}

#[derive(Deserialize)]
#[serde(crate = "near_sdk::serde", deny_unknown_fields)]
struct TokenView {
    token_id: String,
    owner_id: AccountId,
    #[serde(rename = "metadata")]
    _metadata: near_sdk::serde::de::IgnoredAny,
    #[serde(deserialize_with = "approvals")]
    approved_account_ids: BTreeMap<AccountId, u64>,
}
fn approvals<'de, D: Deserializer<'de>>(
    d: D,
) -> std::result::Result<BTreeMap<AccountId, u64>, D::Error> {
    struct ApprovalVisitor;
    impl<'de> Visitor<'de> for ApprovalVisitor {
        type Value = BTreeMap<AccountId, u64>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("at most 32 unique numeric approvals")
        }
        fn visit_map<A: MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut entries = BTreeMap::new();
            while let Some((account, id)) = map.next_entry::<AccountId, u64>()? {
                if entries.len() == 32 || entries.insert(account, id).is_some() {
                    return Err(A::Error::custom("duplicate or oversized approval map"));
                }
            }
            Ok(entries)
        }
    }
    d.deserialize_map(ApprovalVisitor)
}
pub(super) fn verify_views(
    token: &[u8],
    approval: &[u8],
    terms: &ListingTerms,
    domain: &DeploymentDomain,
) -> bool {
    let Ok(token) = serde_json::from_slice::<TokenView>(token) else {
        return false;
    };
    token.token_id == terms.token.token()
        && token.owner_id == terms.seller
        && token.approved_account_ids.get(&domain.marketplace) == Some(&terms.approval_id)
        && serde_json::from_slice::<bool>(approval).ok() == Some(true)
}

pub(super) fn approved_transfer_call(op: &OperationContext) -> Result<ApprovedTransferCall> {
    let (_, _, terms, receiver) = op.approved_transfer().ok_or(IntegrationError::Context)?;
    op.commitment()?;
    if op.budgets().call_abi != hash(TRANSFER_ABI) {
        return Err(IntegrationError::Context);
    }
    Ok(ApprovedTransferCall { target: op.domain().nft.clone(), method: "marketplace_transfer_approved",
        args: serde_json::to_vec(&serde_json::json!({ "expected_seller_id": terms.seller,
            "token_id": terms.token.token(), "approval_id": U64(terms.approval_id), "receiver_id": receiver }))
            .map_err(|_| IntegrationError::Context)?,
        attached_yocto: 1, gas: op.budgets().first_call_gas, operation: op.clone(),
    })
}
