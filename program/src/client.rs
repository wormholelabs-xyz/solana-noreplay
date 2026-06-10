//! Client-side helpers for building NoReplay instructions.
//!
//! This module is only available with the `client` feature enabled.
//!
//! # Example
//!
//! ```ignore
//! use solana_noreplay::client::{CreateBitmap, MarkUsed};
//!
//! // Create a bitmap PDA (permissionless)
//! let ix = CreateBitmap {
//!     payer: &payer_pubkey,
//!     authority: &authority_pubkey,
//!     namespace: b"my_namespace",
//!     sequence: 42,
//! }.instruction();
//!
//! // Mark a sequence as used (authority must sign)
//! let ix = MarkUsed {
//!     payer: &payer_pubkey,
//!     authority: &authority_pubkey,
//!     namespace: b"my_namespace",
//!     sequence: 42,
//! }.instruction();
//! ```

use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    system_program,
};

/// Program ID for the NoReplay program (set via `NOREPLAY_PROGRAM_ID` env var at compile time).
pub const PROGRAM_ID: Pubkey = Pubkey::from_str_const(env!("NOREPLAY_PROGRAM_ID"));

/// Size of each seed component for namespace chunking.
const SEED_CHUNK_SIZE: usize = 32;

/// Derive the bitmap PDA for a given authority, namespace, and sequence.
///
/// Seeds are always: `[authority, ns_chunk_0, ns_chunk_1, bucket_index]`.
/// Delegates to [`derive_bitmap_pda_for_bucket`] so the seed layout has a
/// single source of truth.
pub fn derive_bitmap_pda(authority: &Pubkey, namespace: &[u8], sequence: u64) -> (Pubkey, u8) {
    derive_bitmap_pda_for_bucket(
        authority,
        namespace,
        sequence / crate::state::BITS_PER_BUCKET,
    )
}

/// Build instruction data for namespace + sequence.
pub fn build_instruction_data(discriminator: u8, namespace: &[u8], sequence: u64) -> Vec<u8> {
    let namespace_len = namespace.len() as u16;
    let mut data = Vec::with_capacity(1 + 2 + namespace.len() + 8);
    data.push(discriminator);
    data.extend_from_slice(&namespace_len.to_le_bytes());
    data.extend_from_slice(namespace);
    data.extend_from_slice(&sequence.to_le_bytes());
    data
}

/// Builder for CreateBitmap instruction.
///
/// Creates a bitmap PDA permissionlessly. Anyone can call this to pre-create
/// and fund bucket accounts, reducing compute and cost for the authority when
/// they later call MarkUsed.
///
/// # Accounts
///
/// 1. `[signer, writable]` Payer - pays for PDA creation
/// 2. `[]` Authority - goes into PDA seeds (does NOT need to sign)
/// 3. `[writable]` Bitmap PDA
/// 4. `[]` System program
///
/// # Example
///
/// ```ignore
/// let ix = CreateBitmap {
///     payer: &payer_pubkey,
///     authority: &authority_pubkey,
///     namespace: b"my_namespace",
///     sequence: 42,
/// }.instruction();
/// ```
pub struct CreateBitmap<'a> {
    /// Account that pays for PDA creation.
    pub payer: &'a Pubkey,
    /// Authority that owns the replay protection namespace (does NOT need to sign).
    pub authority: &'a Pubkey,
    /// Application-specific namespace (max 64 bytes).
    pub namespace: &'a [u8],
    /// Sequence number (determines which bucket to create).
    pub sequence: u64,
}

impl CreateBitmap<'_> {
    /// Build the CreateBitmap instruction.
    pub fn instruction(&self) -> Instruction {
        let (pda, _bump) = derive_bitmap_pda(self.authority, self.namespace, self.sequence);

        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(*self.payer, true),
                AccountMeta::new_readonly(*self.authority, false),
                AccountMeta::new(pda, false),
                AccountMeta::new_readonly(system_program::ID, false),
            ],
            data: build_instruction_data(
                crate::instruction::CREATE_BITMAP,
                self.namespace,
                self.sequence,
            ),
        }
    }

    /// Get the PDA that will be created.
    pub fn pda(&self) -> (Pubkey, u8) {
        derive_bitmap_pda(self.authority, self.namespace, self.sequence)
    }
}

/// Builder for MarkUsed instruction.
///
/// Marks a sequence number as used. Authority MUST sign to prevent DOS attacks
/// where adversaries mark sequences as used for other users.
///
/// # Accounts
///
/// 1. `[signer, writable]` Payer - pays for PDA creation if needed
/// 2. `[signer]` Authority - must sign; goes into PDA seeds
/// 3. `[writable]` Bitmap PDA
/// 4. `[]` System program
///
/// # Example
///
/// ```ignore
/// let ix = MarkUsed {
///     payer: &payer_pubkey,
///     authority: &authority_pubkey,
///     namespace: b"my_namespace",
///     sequence: 42,
/// }.instruction();
/// ```
pub struct MarkUsed<'a> {
    /// Account that pays for PDA creation (if needed).
    pub payer: &'a Pubkey,
    /// Authority that owns the replay protection namespace (MUST sign).
    pub authority: &'a Pubkey,
    /// Application-specific namespace (max 64 bytes).
    pub namespace: &'a [u8],
    /// Sequence number to mark as used.
    pub sequence: u64,
}

impl MarkUsed<'_> {
    /// Build the MarkUsed instruction.
    pub fn instruction(&self) -> Instruction {
        self.instruction_with_discriminator(crate::instruction::MARK_USED)
    }

    /// Build the UnmarkUsed instruction (same accounts/data, different discriminator).
    ///
    /// Clears a sequence number's replay protection bit. Returns a bool via
    /// return data indicating whether the bit was actually modified.
    pub fn unmark_instruction(&self) -> Instruction {
        self.instruction_with_discriminator(crate::instruction::UNMARK_USED)
    }

    fn instruction_with_discriminator(&self, discriminator: u8) -> Instruction {
        let (pda, _bump) = derive_bitmap_pda(self.authority, self.namespace, self.sequence);

        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(*self.payer, true),
                AccountMeta::new_readonly(*self.authority, true),
                AccountMeta::new(pda, false),
                AccountMeta::new_readonly(system_program::ID, false),
            ],
            data: build_instruction_data(discriminator, self.namespace, self.sequence),
        }
    }

    /// Get the PDA that will be used/created.
    pub fn pda(&self) -> (Pubkey, u8) {
        derive_bitmap_pda(self.authority, self.namespace, self.sequence)
    }
}

/// Derive the bitmap PDA for a given authority, namespace, and bucket index.
///
/// Equivalent to `derive_bitmap_pda(authority, namespace, bucket_index * BITS_PER_BUCKET)`
/// but accepts bucket indices that would overflow `u64` if expressed as a sequence.
pub fn derive_bitmap_pda_for_bucket(
    authority: &Pubkey,
    namespace: &[u8],
    bucket_index: u64,
) -> (Pubkey, u8) {
    let bucket_bytes = bucket_index.to_le_bytes();
    let mid = namespace.len().min(SEED_CHUNK_SIZE);

    let seeds: [&[u8]; 4] = [
        authority.as_ref(),
        &namespace[..mid],
        &namespace[mid..],
        &bucket_bytes,
    ];

    Pubkey::find_program_address(&seeds, &PROGRAM_ID)
}

/// Build instruction data for `MarkUsedBulk`.
///
/// Wire format: `[disc][ns_len: u16 LE][ns][bucket_index: u64 LE][or_mask: 128 B]`.
pub fn build_mark_used_bulk_data(
    namespace: &[u8],
    bucket_index: u64,
    or_mask: &[u8; crate::instruction::MARK_USED_BULK_MASK_LEN],
) -> Vec<u8> {
    // The prefix is identical to the single-sequence layout, with bucket_index
    // occupying the u64 slot; only the trailing OR-mask is bulk-specific.
    let mut data =
        build_instruction_data(crate::instruction::MARK_USED_BULK, namespace, bucket_index);
    data.reserve(crate::instruction::MARK_USED_BULK_MASK_LEN);
    data.extend_from_slice(or_mask);
    data
}

/// Builder for the MarkUsedBulk instruction.
///
/// OR-merges a 128-byte mask into a single bucket. Account layout, signer
/// requirements, and PDA derivation match `MarkUsed`; the data carries an
/// explicit `bucket_index` and a 128-byte mask instead of a single sequence.
/// Bits are OR'd in and never cleared.
///
/// # Accounts
///
/// 1. `[signer, writable]` Payer - pays for PDA creation if needed
/// 2. `[signer]` Authority - must sign; goes into PDA seeds
/// 3. `[writable]` Bitmap PDA
/// 4. `[]` System program
pub struct MarkUsedBulk<'a> {
    /// Account that pays for PDA creation (if needed).
    pub payer: &'a Pubkey,
    /// Authority that owns the replay protection namespace (MUST sign).
    pub authority: &'a Pubkey,
    /// Application-specific namespace (max 64 bytes).
    pub namespace: &'a [u8],
    /// Bucket index (i.e. `sequence / BITS_PER_BUCKET`).
    pub bucket_index: u64,
    /// 128-byte mask to OR into the bucket bitmap.
    pub or_mask: &'a [u8; crate::instruction::MARK_USED_BULK_MASK_LEN],
}

impl MarkUsedBulk<'_> {
    /// Build the MarkUsedBulk instruction.
    pub fn instruction(&self) -> Instruction {
        let (pda, _bump) =
            derive_bitmap_pda_for_bucket(self.authority, self.namespace, self.bucket_index);

        Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(*self.payer, true),
                AccountMeta::new_readonly(*self.authority, true),
                AccountMeta::new(pda, false),
                AccountMeta::new_readonly(system_program::ID, false),
            ],
            data: build_mark_used_bulk_data(self.namespace, self.bucket_index, self.or_mask),
        }
    }

    /// Get the PDA that will be used/created.
    pub fn pda(&self) -> (Pubkey, u8) {
        derive_bitmap_pda_for_bucket(self.authority, self.namespace, self.bucket_index)
    }
}

// Re-export useful constants for clients
pub use crate::instruction::{
    CREATE_BITMAP, MARK_USED, MARK_USED_BULK, MARK_USED_BULK_MASK_LEN, UNMARK_USED,
};
pub use crate::state::{BITMAP_ACCOUNT_SIZE, BITMAP_BYTES, BITS_PER_BUCKET};
pub use crate::MAX_NAMESPACE_LEN;
