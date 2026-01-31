//! Instruction definitions and parsing for the NoReplay program.

use pinocchio::{error::ProgramError, AccountView};

use crate::MAX_NAMESPACE_LEN;

/// Instruction discriminators.
pub const CREATE_BITMAP: u8 = 0;
pub const MARK_USED: u8 = 1;

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

        // Authority MUST sign to prevent DOS attacks where adversaries
        // mark sequences as used for other users
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
