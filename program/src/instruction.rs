//! Instruction definitions and parsing for the NoReplay program.

use pinocchio::{error::ProgramError, AccountView};

use crate::MAX_NAMESPACE_LEN;

/// Instruction discriminators.
pub const CREATE_BITMAP: u8 = 0;
pub const MARK_USED: u8 = 1;
pub const UNMARK_USED: u8 = 2;
pub const MARK_USED_BULK: u8 = 3;

// =============================================================================
// CreateBitmap
// =============================================================================

/// Accounts for CreateBitmap instruction.
///
/// # Accounts
/// 0. `[writable, signer]` payer - Pays for PDA creation
/// 1. `[]` authority - Used for PDA derivation (does NOT need to sign)
/// 2. `[writable]` bitmap_pda - PDA to create
/// 3. `[]` system_program - System program (implicit, not stored)
pub struct CreateBitmapAccounts<'a> {
    pub payer: &'a AccountView,
    pub authority: &'a AccountView,
    pub bitmap_pda: &'a AccountView,
}

impl<'a> TryFrom<&'a [AccountView]> for CreateBitmapAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountView]) -> Result<Self, Self::Error> {
        let [payer, authority, bitmap_pda, _system_program, ..] = accounts else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        // Payer must sign
        if !payer.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }

        // Authority does NOT need to sign - CreateBitmap is permissionless

        Ok(Self {
            payer,
            authority,
            bitmap_pda,
        })
    }
}

/// Data for CreateBitmap and MarkUsed instructions.
///
/// Format: `[namespace_len: u16 LE][namespace: 0-64 bytes][sequence: u64 LE]`
pub struct InstructionData<'a> {
    pub namespace: &'a [u8],
    pub sequence: u64,
}

impl<'a> TryFrom<&'a [u8]> for InstructionData<'a> {
    type Error = ProgramError;

    fn try_from(data: &'a [u8]) -> Result<Self, Self::Error> {
        // Minimum: 2 (namespace_len) + 0 (empty namespace) + 8 (sequence) = 10 bytes
        if data.len() < 10 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let namespace_len = u16::from_le_bytes(data[0..2].try_into().unwrap()) as usize;

        if namespace_len > MAX_NAMESPACE_LEN {
            return Err(ProgramError::InvalidInstructionData);
        }

        if data.len() != 2 + namespace_len + 8 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let namespace = &data[2..2 + namespace_len];
        let sequence = u64::from_le_bytes(data[2 + namespace_len..].try_into().unwrap());

        Ok(Self {
            namespace,
            sequence,
        })
    }
}

/// CreateBitmap instruction - creates a bitmap PDA permissionlessly.
///
/// This allows anyone to pre-create and fund bitmap accounts, reducing
/// compute and cost for the authority when they later call MarkUsed.
pub struct CreateBitmap<'a> {
    pub accounts: CreateBitmapAccounts<'a>,
    pub data: InstructionData<'a>,
}

impl<'a> TryFrom<(&'a [u8], &'a [AccountView])> for CreateBitmap<'a> {
    type Error = ProgramError;

    fn try_from((data, accounts): (&'a [u8], &'a [AccountView])) -> Result<Self, Self::Error> {
        Ok(Self {
            accounts: CreateBitmapAccounts::try_from(accounts)?,
            data: InstructionData::try_from(data)?,
        })
    }
}

// =============================================================================
// MarkUsed
// =============================================================================

/// Accounts for MarkUsed instruction.
///
/// # Accounts
/// 0. `[writable, signer]` payer - Pays for PDA creation if needed
/// 1. `[signer]` authority - Owner of the sequence space (included in PDA seeds)
/// 2. `[writable]` bitmap_pda - PDA storing the bitmap for this bucket
/// 3. `[]` system_program - System program (implicit, not stored)
pub struct MarkUsedAccounts<'a> {
    pub payer: &'a AccountView,
    pub authority: &'a AccountView,
    pub bitmap_pda: &'a AccountView,
}

impl<'a> TryFrom<&'a [AccountView]> for MarkUsedAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountView]) -> Result<Self, Self::Error> {
        let [payer, authority, bitmap_pda, _system_program, ..] = accounts else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        // Payer must sign
        if !payer.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }

        // Authority signer enforces authorization: the bitmap PDA is
        // derived from the authority address, so only the namespace owner
        // can mark sequences as used in their namespace.
        if !authority.is_signer() {
            return Err(ProgramError::MissingRequiredSignature);
        }

        Ok(Self {
            payer,
            authority,
            bitmap_pda,
        })
    }
}

/// MarkUsed instruction - marks a sequence number as used for replay protection.
pub struct MarkUsed<'a> {
    pub accounts: MarkUsedAccounts<'a>,
    pub data: InstructionData<'a>,
}

impl<'a> TryFrom<(&'a [u8], &'a [AccountView])> for MarkUsed<'a> {
    type Error = ProgramError;

    fn try_from((data, accounts): (&'a [u8], &'a [AccountView])) -> Result<Self, Self::Error> {
        Ok(Self {
            accounts: MarkUsedAccounts::try_from(accounts)?,
            data: InstructionData::try_from(data)?,
        })
    }
}

// =============================================================================
// MarkUsedBulk
// =============================================================================

/// Size of the OR-mask carried by `MarkUsedBulk` (one full bucket of bits).
pub const MARK_USED_BULK_MASK_LEN: usize = crate::state::BITMAP_BYTES;

/// Data for the MarkUsedBulk instruction.
///
/// Format: `[namespace_len: u16 LE][namespace: 0-64 bytes][bucket_index: u64 LE][or_mask: 128 bytes]`
///
/// `bucket_index` is `sequence / BITS_PER_BUCKET` — the same value used as a seed for
/// the bitmap PDA. The mask is OR'd into the bucket; it never clears bits.
pub struct MarkUsedBulkData<'a> {
    pub namespace: &'a [u8],
    pub bucket_index: u64,
    pub or_mask: &'a [u8; MARK_USED_BULK_MASK_LEN],
}

impl<'a> TryFrom<&'a [u8]> for MarkUsedBulkData<'a> {
    type Error = ProgramError;

    fn try_from(data: &'a [u8]) -> Result<Self, Self::Error> {
        // Minimum: 2 (namespace_len) + 0 (empty namespace) + 8 (bucket_index) + 128 (mask)
        const MIN_LEN: usize = 2 + 8 + MARK_USED_BULK_MASK_LEN;
        if data.len() < MIN_LEN {
            return Err(ProgramError::InvalidInstructionData);
        }

        let namespace_len = u16::from_le_bytes(data[0..2].try_into().unwrap()) as usize;

        if namespace_len > MAX_NAMESPACE_LEN {
            return Err(ProgramError::InvalidInstructionData);
        }

        let expected_len = 2 + namespace_len + 8 + MARK_USED_BULK_MASK_LEN;
        if data.len() != expected_len {
            return Err(ProgramError::InvalidInstructionData);
        }

        let namespace = &data[2..2 + namespace_len];
        let bucket_index = u64::from_le_bytes(
            data[2 + namespace_len..2 + namespace_len + 8]
                .try_into()
                .unwrap(),
        );
        let mask_start = 2 + namespace_len + 8;
        let or_mask: &[u8; MARK_USED_BULK_MASK_LEN] = data[mask_start..]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?;

        Ok(Self {
            namespace,
            bucket_index,
            or_mask,
        })
    }
}

/// MarkUsedBulk instruction - OR-merges a 128-byte mask into a single bucket's bitmap.
///
/// Account layout matches `MarkUsed` exactly (same slot positions, same signer
/// requirements). The PDA derivation also matches `MarkUsed` — the only difference
/// is that `bucket_index` is supplied directly instead of being derived from a
/// single sequence number.
///
/// Semantics: bits are OR'd in. The instruction never clears a set bit.
pub struct MarkUsedBulk<'a> {
    pub accounts: MarkUsedAccounts<'a>,
    pub data: MarkUsedBulkData<'a>,
}

impl<'a> TryFrom<(&'a [u8], &'a [AccountView])> for MarkUsedBulk<'a> {
    type Error = ProgramError;

    fn try_from((data, accounts): (&'a [u8], &'a [AccountView])) -> Result<Self, Self::Error> {
        Ok(Self {
            accounts: MarkUsedAccounts::try_from(accounts)?,
            data: MarkUsedBulkData::try_from(data)?,
        })
    }
}
