use pinocchio::error::ProgramError;

use crate::MAX_NAMESPACE_LEN;

/// Zero-copy instruction parser.
/// Borrows namespace directly from instruction data.
pub enum Instruction<'a> {
    /// Create a bitmap PDA permissionlessly (discriminator = 0)
    CreateBitmap { namespace: &'a [u8], sequence: u64 },
    /// Mark a sequence number as used (discriminator = 1)
    MarkUsed { namespace: &'a [u8], sequence: u64 },
}

impl<'a> Instruction<'a> {
    pub const CREATE_BITMAP: u8 = 0;
    pub const MARK_USED: u8 = 1;

    /// Parse instruction data into an Instruction enum.
    /// Zero-copy: namespace is a slice into the original data.
    pub fn parse(data: &'a [u8]) -> Result<Self, ProgramError> {
        // Minimum: 1 (discriminator) + 2 (namespace_len) + 0 (empty namespace) + 8 (sequence)
        if data.len() < 11 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let discriminator = data[0];
        let payload = &data[1..];

        let namespace_len = u16::from_le_bytes(payload[0..2].try_into().unwrap()) as usize;

        if namespace_len > MAX_NAMESPACE_LEN {
            return Err(ProgramError::InvalidInstructionData);
        }

        if payload.len() != 2 + namespace_len + 8 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let namespace = &payload[2..2 + namespace_len];
        let sequence = u64::from_le_bytes(payload[2 + namespace_len..].try_into().unwrap());

        match discriminator {
            Self::CREATE_BITMAP => Ok(Instruction::CreateBitmap {
                namespace,
                sequence,
            }),
            Self::MARK_USED => Ok(Instruction::MarkUsed {
                namespace,
                sequence,
            }),
            _ => Err(ProgramError::InvalidInstructionData),
        }
    }
}
