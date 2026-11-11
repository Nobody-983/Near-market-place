//! Bound approval deserialization before allocating a legacy or corrupt map.
use std::collections::HashMap;

use near_contract_standards::non_fungible_token::{NonFungibleToken, TokenId};
use near_sdk::{borsh, env, require, AccountId};

use crate::MAX_APPROVALS_PER_TOKEN;

// Borsh map length + at most 32 (string length + 64-byte AccountId + u64).
const MAX_APPROVAL_MAP_BYTES: usize = 4 + MAX_APPROVALS_PER_TOKEN * (4 + 64 + 8);
const APPROVAL_READ_REGISTER: u64 = 0x4e46_5441;

pub(crate) fn bounded_approvals(
    tokens: &NonFungibleToken,
    token_id: &TokenId,
) -> HashMap<AccountId, u64> {
    let map = tokens
        .approvals_by_id
        .as_ref()
        .expect("Approval extension required");
    // In pinned SDK 5.30.0, LookupMap serializes only its Vec<u8> prefix;
    // get/insert append Borsh(key). Preserve the source handle's actual prefix.
    let mut key: Vec<u8> = borsh::from_slice(&borsh::to_vec(map).expect("Map handle encoding"))
        .expect("Map handle decoding");
    key.extend(borsh::to_vec(token_id).expect("Token key encoding"));
    // SAFETY: the initialized key slice stays alive for the synchronous host
    // call, which writes only into a host register, not a guest output pointer.
    let status = unsafe {
        near_sys::storage_read(
            key.len() as u64,
            key.as_ptr() as u64,
            APPROVAL_READ_REGISTER,
        )
    };
    let bytes = match status {
        0 => return HashMap::new(),
        1 => env::read_register_bounded(APPROVAL_READ_REGISTER, MAX_APPROVAL_MAP_BYTES)
            .expect("Missing approval register")
            .expect("Approval map exceeds byte limit"),
        _ => env::panic_str("Invalid storage host status"),
    };
    let count = u32::from_le_bytes(
        bytes
            .get(..4)
            .expect("Truncated approval map")
            .try_into()
            .unwrap(),
    );
    require!(
        count as usize <= MAX_APPROVALS_PER_TOKEN,
        "Approval count limit reached"
    );
    let accounts: HashMap<AccountId, u64> =
        borsh::from_slice(&bytes).expect("Corrupt approval map");
    require!(
        borsh::to_vec(&accounts).expect("Approval map encoding") == bytes,
        "Noncanonical approval map"
    );
    accounts
}
