use pinocchio::Address;

use crate::state::BITS_PER_BUCKET;
use crate::MAX_NAMESPACE_LEN;

/// Size of each seed component for namespace chunking
const SEED_CHUNK_SIZE: usize = 32;

/// Error returned when PDA derivation fails
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivePdaError {
    NamespaceTooLong,
}

/// Seed components for bitmap PDA derivation.
///
/// Seeds are always: `[authority, ns_chunk_0, ns_chunk_1, bucket_index]`
/// where ns_chunk_0 and ns_chunk_1 may be empty slices.
///
/// Namespace is split into two chunks (max 32 bytes each) to avoid heap
/// allocation while staying within Solana's per-seed size limit.
pub struct BitmapPdaSeeds<'a> {
    pub ns_chunks: [&'a [u8]; 2],
    pub bucket_bytes: [u8; 8],
}

impl<'a> BitmapPdaSeeds<'a> {
    /// Compute seed components from namespace and sequence.
    pub fn new(namespace: &'a [u8], sequence: u64) -> Self {
        let mid = namespace.len().min(SEED_CHUNK_SIZE);
        Self {
            ns_chunks: [&namespace[..mid], &namespace[mid..]],
            bucket_bytes: (sequence / BITS_PER_BUCKET).to_le_bytes(),
        }
    }

    /// Build the seeds array for PDA derivation (without bump).
    pub fn as_seeds(&self, authority: &'a [u8]) -> [&[u8]; 4] {
        [
            authority,
            self.ns_chunks[0],
            self.ns_chunks[1],
            &self.bucket_bytes,
        ]
    }

    /// Build the seeds array with bump for verification or signing.
    pub fn as_seeds_with_bump<'b>(&'b self, authority: &'b [u8], bump: &'b [u8]) -> [&'b [u8]; 5]
    where
        'a: 'b,
    {
        [
            authority,
            self.ns_chunks[0],
            self.ns_chunks[1],
            &self.bucket_bytes,
            bump,
        ]
    }

    /// Derive the PDA address and bump.
    pub fn find_pda(&self, authority: &Address, program_id: &Address) -> (Address, u8) {
        let seeds = self.as_seeds(authority.as_ref());
        Address::find_program_address(&seeds, program_id)
    }
}

/// Derive the bitmap PDA for a given authority, namespace, and sequence.
///
/// Seeds are always: `[authority, ns_chunk_0, ns_chunk_1, bucket_index]`
/// where ns_chunk_0 and ns_chunk_1 may be empty slices.
pub fn derive_bitmap_pda(
    authority: &Address,
    namespace: &[u8],
    sequence: u64,
    program_id: &Address,
) -> Result<(Address, u8), DerivePdaError> {
    if namespace.len() > MAX_NAMESPACE_LEN {
        return Err(DerivePdaError::NamespaceTooLong);
    }

    Ok(BitmapPdaSeeds::new(namespace, sequence).find_pda(authority, program_id))
}
