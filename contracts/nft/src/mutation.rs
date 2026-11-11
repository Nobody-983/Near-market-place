//! Permanent, fixed-size sidecars. The historical Contract root is unchanged.
//! Allocated only by funded mint/approve; never delete or reset after burn.
use near_sdk::{env, require};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Version {
    pub ownership: u64,
    pub approval: u64,
}

fn key(token: &str) -> Vec<u8> {
    let mut key = b"nft:mutation:v1:".to_vec();
    key.extend(env::sha256(token.as_bytes()));
    key
}

impl Version {
    pub fn load(token: &str) -> Option<Self> {
        let key = key(token);
        // Bound the guest allocation even for a corrupt stored value.
        let register = 0x4e46_5456;
        // SAFETY: initialized slice remains alive for the synchronous host read.
        let status =
            unsafe { near_sys::storage_read(key.len() as u64, key.as_ptr() as u64, register) };
        if status == 0 {
            return None;
        }
        require!(status == 1, "Invalid mutation storage result");
        let bytes = env::read_register_bounded(register, 16)
            .expect("Missing mutation register")
            .expect("Oversized mutation version");
        require!(bytes.len() == 16, "Corrupt mutation version");
        Some(Self {
            ownership: u64::from_le_bytes(bytes[..8].try_into().unwrap()),
            approval: u64::from_le_bytes(bytes[8..].try_into().unwrap()),
        })
    }

    pub fn approval_next(self) -> Self {
        Self {
            approval: self
                .approval
                .checked_add(1)
                .expect("Mutation version exhausted"),
            ..self
        }
    }

    pub fn transfer_next(self) -> Self {
        Self {
            ownership: self
                .ownership
                .checked_add(1)
                .expect("Mutation version exhausted"),
            ..self.approval_next()
        }
    }

    pub fn write(self, token: &str) {
        let mut bytes = self.ownership.to_le_bytes().to_vec();
        bytes.extend(self.approval.to_le_bytes());
        env::storage_write(&key(token), &bytes);
    }
}
