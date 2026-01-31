//! Anchor Interface for Solana NoReplay Program.
//!
//! This crate provides Anchor-compatible account structs and CPI helpers
//! for interacting with the solana-noreplay program.
//!
//! The underlying program uses raw instruction data (not Anchor serialization),
//! so this interface provides manual CPI functions that build the correct format.
//!
//! ## Usage
//!
//! ```ignore
//! use solana_noreplay_interface::cpi;
//!
//! // Mark a sequence as used (authority must sign via PDA seeds)
//! cpi::mark_used(
//!     CpiContext::new_with_signer(
//!         ctx.accounts.noreplay_program.to_account_info(),
//!         cpi::MarkUsed {
//!             payer: ctx.accounts.payer.to_account_info(),
//!             authority: ctx.accounts.emitter.to_account_info(),
//!             bitmap: ctx.accounts.replay_bitmap.to_account_info(),
//!             system_program: ctx.accounts.system_program.to_account_info(),
//!         },
//!         &[&authority_seeds],
//!     ),
//!     namespace,
//!     sequence,
//! )?;
//! ```

use anchor_lang::prelude::*;
use anchor_lang::solana_program::{
    instruction::{AccountMeta, Instruction},
    program::invoke_signed,
};

// Program ID placeholder - update after deployment
declare_id!("rep1ayXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX");

/// Instruction discriminators (must match the Pinocchio program).
pub const CREATE_BITMAP: u8 = 0;
pub const MARK_USED: u8 = 1;

/// Maximum namespace length (64 bytes = 2 chunks of 32 bytes).
pub const MAX_NAMESPACE_LEN: usize = 64;

/// Bits per bitmap bucket (1024 bits = 128 bytes).
pub const BITS_PER_BUCKET: u64 = 1024;

/// Size of bitmap account data (1 byte bump + 128 bytes bitmap).
pub const BITMAP_ACCOUNT_SIZE: usize = 129;

/// Derive the bitmap PDA for a given authority, namespace, and sequence.
///
/// Seeds: `[authority, ns_chunk_0 (0-32 bytes), ns_chunk_1 (0-32 bytes), bucket_index (8 bytes LE)]`
pub fn derive_bitmap_pda(authority: &Pubkey, namespace: &[u8], sequence: u64) -> (Pubkey, u8) {
    let bucket_index = sequence / BITS_PER_BUCKET;
    let bucket_bytes = bucket_index.to_le_bytes();

    // Split namespace into chunks (max 32 bytes each)
    let mid = namespace.len().min(32);
    let ns_chunk_0 = &namespace[..mid];
    let ns_chunk_1 = &namespace[mid..];

    Pubkey::find_program_address(
        &[authority.as_ref(), ns_chunk_0, ns_chunk_1, &bucket_bytes],
        &ID,
    )
}

/// Build instruction data for CreateBitmap or MarkUsed.
///
/// Format: `[discriminator (1)][namespace_len (2 LE)][namespace (0-64)][sequence (8 LE)]`
fn build_instruction_data(discriminator: u8, namespace: &[u8], sequence: u64) -> Vec<u8> {
    let namespace_len = namespace.len() as u16;
    let mut data = Vec::with_capacity(1 + 2 + namespace.len() + 8);
    data.push(discriminator);
    data.extend_from_slice(&namespace_len.to_le_bytes());
    data.extend_from_slice(namespace);
    data.extend_from_slice(&sequence.to_le_bytes());
    data
}

/// CPI module for invoking solana-noreplay instructions.
pub mod cpi {
    use super::*;

    /// Accounts for the CreateBitmap instruction.
    ///
    /// Creates a bitmap PDA permissionlessly. Anyone can call this to pre-create
    /// and fund bucket accounts, reducing compute for the authority later.
    pub struct CreateBitmap<'info> {
        /// Pays for PDA creation.
        pub payer: AccountInfo<'info>,

        /// Authority for the replay protection namespace (does NOT need to sign).
        pub authority: AccountInfo<'info>,

        /// Bitmap PDA to create.
        pub bitmap: AccountInfo<'info>,

        /// System program for account creation.
        pub system_program: AccountInfo<'info>,
    }

    impl<'info> ToAccountMetas for CreateBitmap<'info> {
        fn to_account_metas(&self, _is_signer: Option<bool>) -> Vec<AccountMeta> {
            vec![
                AccountMeta::new(*self.payer.key, true),
                AccountMeta::new_readonly(*self.authority.key, false),
                AccountMeta::new(*self.bitmap.key, false),
                AccountMeta::new_readonly(*self.system_program.key, false),
            ]
        }
    }

    impl<'info> ToAccountInfos<'info> for CreateBitmap<'info> {
        fn to_account_infos(&self) -> Vec<AccountInfo<'info>> {
            vec![
                self.payer.clone(),
                self.authority.clone(),
                self.bitmap.clone(),
                self.system_program.clone(),
            ]
        }
    }

    /// Accounts for the MarkUsed instruction.
    ///
    /// Marks a sequence number as used. Authority MUST sign to prevent DOS attacks.
    pub struct MarkUsed<'info> {
        /// Pays for PDA creation if the bitmap doesn't exist yet.
        pub payer: AccountInfo<'info>,

        /// Authority for the replay protection namespace (MUST sign).
        pub authority: AccountInfo<'info>,

        /// Bitmap PDA storing the used bits.
        pub bitmap: AccountInfo<'info>,

        /// System program for account creation if needed.
        pub system_program: AccountInfo<'info>,
    }

    impl<'info> ToAccountMetas for MarkUsed<'info> {
        fn to_account_metas(&self, _is_signer: Option<bool>) -> Vec<AccountMeta> {
            vec![
                AccountMeta::new(*self.payer.key, true),
                AccountMeta::new_readonly(*self.authority.key, true), // Authority must sign
                AccountMeta::new(*self.bitmap.key, false),
                AccountMeta::new_readonly(*self.system_program.key, false),
            ]
        }
    }

    impl<'info> ToAccountInfos<'info> for MarkUsed<'info> {
        fn to_account_infos(&self) -> Vec<AccountInfo<'info>> {
            vec![
                self.payer.clone(),
                self.authority.clone(),
                self.bitmap.clone(),
                self.system_program.clone(),
            ]
        }
    }

    /// Create a bitmap PDA permissionlessly.
    ///
    /// This allows anyone to pre-fund bitmap accounts, reducing cost for the
    /// authority when they later call `mark_used`.
    pub fn create_bitmap<'info>(
        ctx: CpiContext<'_, '_, '_, 'info, CreateBitmap<'info>>,
        namespace: &[u8],
        sequence: u64,
    ) -> Result<()> {
        let ix = Instruction {
            program_id: crate::ID,
            accounts: ctx.accounts.to_account_metas(None),
            data: build_instruction_data(CREATE_BITMAP, namespace, sequence),
        };

        invoke_signed(&ix, &ctx.accounts.to_account_infos(), ctx.signer_seeds)?;

        Ok(())
    }

    /// Mark a sequence number as used for replay protection.
    ///
    /// The authority MUST sign to prevent adversaries from marking sequences
    /// as used for other users. In CPI contexts, the authority is typically
    /// a PDA that the calling program signs for.
    ///
    /// Returns an error if the sequence was already marked as used (replay detected).
    pub fn mark_used<'info>(
        ctx: CpiContext<'_, '_, '_, 'info, MarkUsed<'info>>,
        namespace: &[u8],
        sequence: u64,
    ) -> Result<()> {
        let ix = Instruction {
            program_id: crate::ID,
            accounts: ctx.accounts.to_account_metas(None),
            data: build_instruction_data(MARK_USED, namespace, sequence),
        };

        invoke_signed(&ix, &ctx.accounts.to_account_infos(), ctx.signer_seeds)?;

        Ok(())
    }
}
