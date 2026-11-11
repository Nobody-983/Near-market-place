use borsh::{BorshDeserialize, BorshSerialize};
use sha2::{Digest, Sha256};

use crate::{
    ledger::Claim,
    state::{Listing, ListingTerms, Sale},
    types::{ClaimId, Generation, SaleId},
    AccountId,
};

use super::{codec, StorageError, StorageResult};

pub(crate) const OP_TAG: &[u8] = b"near-marketplace:operation:v1\0";
const DOMAIN_TAG: &[u8] = b"near-marketplace:deployment:v1\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[repr(u8)]
#[borsh(use_discriminant = true)]
pub enum ArtifactDomain {
    Testnet = 0,
    Production = 1,
}

/// Explicit commitments to an independently reviewed manifest. These fields do
/// not verify a chain, deployed code, key list or ABI. No environment defaults.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct DeploymentDomain {
    pub environment: ArtifactDomain,
    pub genesis: [u8; 32],
    pub manifest: [u8; 32],
    pub marketplace_code: [u8; 32],
    pub nft_code: [u8; 32],
    pub marketplace: AccountId,
    pub nft: AccountId,
}

impl DeploymentDomain {
    pub(crate) fn validate(&self) -> StorageResult<()> {
        if self.marketplace == self.nft
            || [
                self.genesis,
                self.manifest,
                self.marketplace_code,
                self.nft_code,
            ]
            .contains(&[0; 32])
        {
            return Err(StorageError::Domain);
        }
        Ok(())
    }

    pub fn commitment(&self) -> StorageResult<[u8; 32]> {
        self.validate()?;
        hash(DOMAIN_TAG, self)
    }
}

/// Explicit measured budgets and reviewed ABI commitments, not defaults or an
/// invented NFT wire interface. No JSON/promise generation consumes this type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Budgets {
    pub first_call_gas: u64,
    pub second_call_gas: u64,
    pub callback_gas: u64,
    pub max_result_bytes: u32,
    pub call_abi: [u8; 32],
    pub callback_abi: [u8; 32],
}

/// Discriminants and field order are part of the v1 intent codec. Verification
/// dependency order is NFT token view, then exact-approval view. Transfer is the
/// specified approval-only intent; payment is one native Transfer action.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[repr(u8)]
#[borsh(use_discriminant = true)]
pub(crate) enum Descriptor {
    Verification {
        generation: Generation,
        terms: ListingTerms,
    } = 0,
    ApprovedTransfer {
        sale: SaleId,
        generation: Generation,
        terms: ListingTerms,
        receiver: AccountId,
    } = 1,
    Payment {
        claim: ClaimId,
        sale: SaleId,
        generation: Generation,
        recipient: AccountId,
        amount: u128,
        seller_amount: u128,
        fee_amount: u128,
        refund_amount: u128,
    } = 2,
}

/// A canonical intent commitment, never a receipt identifier or execution proof.
/// There is no public constructor from arbitrary targets, arguments or a hash.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct OperationContext {
    pub(crate) schema: u16,
    pub(crate) domain: DeploymentDomain,
    pub(crate) descriptor: Descriptor,
    pub(crate) nonce: u64,
    pub(crate) budgets: Budgets,
}

impl OperationContext {
    pub fn for_listing(
        domain: &DeploymentDomain,
        listing: &Listing,
        budgets: Budgets,
    ) -> StorageResult<Self> {
        Self::new(
            domain,
            Descriptor::Verification {
                generation: listing.generation(),
                terms: listing.terms().clone(),
            },
            listing.verification().nonce,
            budgets,
        )
    }

    pub fn for_sale(
        domain: &DeploymentDomain,
        sale: &Sale,
        budgets: Budgets,
    ) -> StorageResult<Self> {
        Self::new(
            domain,
            Descriptor::ApprovedTransfer {
                sale: sale.id(),
                generation: sale.generation(),
                terms: sale.terms().clone(),
                receiver: sale.buyer().clone(),
            },
            sale.transfer().nonce,
            budgets,
        )
    }

    pub fn for_payment(
        domain: &DeploymentDomain,
        claim: &Claim,
        nonce: u64,
        budgets: Budgets,
    ) -> StorageResult<Self> {
        let (seller_amount, fee_amount, refund_amount) = claim.role_amounts();
        Self::new(
            domain,
            Descriptor::Payment {
                claim: claim.id(),
                sale: claim.sale(),
                generation: claim.generation(),
                recipient: claim.recipient().clone(),
                amount: claim.amount(),
                seller_amount,
                fee_amount,
                refund_amount,
            },
            nonce,
            budgets,
        )
    }

    pub(super) fn new(
        domain: &DeploymentDomain,
        descriptor: Descriptor,
        nonce: u64,
        budgets: Budgets,
    ) -> StorageResult<Self> {
        let value = Self {
            schema: 1,
            domain: domain.clone(),
            descriptor,
            nonce,
            budgets,
        };
        value.validate()?;
        Ok(value)
    }

    pub(crate) fn validate(&self) -> StorageResult<()> {
        self.domain.validate()?;
        let b = self.budgets;
        if self.schema != 1 {
            return Err(StorageError::Schema);
        }
        if self.nonce == 0
            || b.callback_gas == 0
            || b.max_result_bytes == 0
            || b.max_result_bytes as usize > codec::MAX_RECORD_BYTES
            || b.call_abi == [0; 32]
            || b.callback_abi == [0; 32]
            || b.first_call_gas
                .checked_add(b.second_call_gas)
                .and_then(|v| v.checked_add(b.callback_gas))
                .is_none()
        {
            return Err(StorageError::Invariant);
        }
        match &self.descriptor {
            Descriptor::Verification { generation, terms } => {
                codec::validate_terms(terms, &self.domain)?;
                if generation.value() == 0 || b.first_call_gas == 0 || b.second_call_gas == 0 {
                    return Err(StorageError::Invariant);
                }
            }
            Descriptor::ApprovedTransfer {
                sale,
                generation,
                terms,
                receiver,
            } => {
                codec::validate_terms(terms, &self.domain)?;
                if sale.value() == 0
                    || generation.value() == 0
                    || receiver == &terms.seller
                    || receiver == &self.domain.marketplace
                    || b.first_call_gas == 0
                    || b.second_call_gas != 0
                {
                    return Err(StorageError::Invariant);
                }
            }
            Descriptor::Payment {
                claim,
                sale,
                generation,
                recipient,
                amount,
                seller_amount,
                fee_amount,
                refund_amount,
            } => {
                if claim.value() == 0
                    || sale.value() == 0
                    || generation.value() == 0
                    || recipient == &self.domain.marketplace
                    || *amount == 0
                    || seller_amount
                        .checked_add(*fee_amount)
                        .and_then(|v| v.checked_add(*refund_amount))
                        != Some(*amount)
                    || (*refund_amount != 0 && (*seller_amount != 0 || *fee_amount != 0))
                    || b.first_call_gas != 0
                    || b.second_call_gas != 0
                {
                    return Err(StorageError::Invariant);
                }
            }
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> StorageResult<Vec<u8>> {
        self.validate()?;
        let mut bytes = OP_TAG.to_vec();
        bytes.extend(borsh::to_vec(self).map_err(|_| StorageError::Encoding)?);
        Ok(bytes)
    }

    pub fn from_canonical_bytes(bytes: &[u8], domain: &DeploymentDomain) -> StorageResult<Self> {
        if bytes.len() > codec::MAX_RECORD_BYTES {
            return Err(StorageError::Bounds);
        }
        let payload = bytes.strip_prefix(OP_TAG).ok_or(StorageError::Schema)?;
        let mut reader = codec::WireReader(payload);
        reader.operation()?;
        reader.end()?;
        let context: Self = borsh::from_slice(payload).map_err(|_| StorageError::Encoding)?;
        context.validate()?;
        if &context.domain != domain {
            return Err(StorageError::Domain);
        }
        Ok(context)
    }

    pub fn commitment(&self) -> StorageResult<[u8; 32]> {
        Ok(Sha256::digest(self.canonical_bytes()?).into())
    }

    pub fn nonce(&self) -> u64 {
        self.nonce
    }
    pub fn domain(&self) -> &DeploymentDomain {
        &self.domain
    }
    pub fn budgets(&self) -> Budgets {
        self.budgets
    }
    pub fn verification(&self) -> Option<(Generation, &ListingTerms)> {
        match &self.descriptor {
            Descriptor::Verification { generation, terms } => Some((*generation, terms)),
            _ => None,
        }
    }
    pub fn approved_transfer(&self) -> Option<(SaleId, Generation, &ListingTerms, &AccountId)> {
        match &self.descriptor {
            Descriptor::ApprovedTransfer {
                sale,
                generation,
                terms,
                receiver,
            } => Some((*sale, *generation, terms, receiver)),
            _ => None,
        }
    }
    pub fn result_count(&self) -> u64 {
        if matches!(self.descriptor, Descriptor::Verification { .. }) {
            2
        } else {
            1
        }
    }
    pub fn attached_yocto(&self) -> u128 {
        match &self.descriptor {
            Descriptor::Verification { .. } => 0,
            Descriptor::ApprovedTransfer { .. } => 1,
            Descriptor::Payment { amount, .. } => *amount,
        }
    }
}

pub(crate) fn hash<T: BorshSerialize>(tag: &[u8], value: &T) -> StorageResult<[u8; 32]> {
    let mut digest = Sha256::new();
    digest.update(tag);
    digest.update(borsh::to_vec(value).map_err(|_| StorageError::Encoding)?);
    Ok(digest.finalize().into())
}
