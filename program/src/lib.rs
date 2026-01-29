use pinocchio::{
    cpi::{Seed, Signer},
    default_panic_handler,
    error::ProgramError,
    no_allocator, program_entrypoint, AccountView, Address, ProgramResult,
};
use pinocchio_system::instructions::{Allocate, Assign, CreateAccount, Transfer};

program_entrypoint!(process_instruction);
no_allocator!();
default_panic_handler!();

/// Zero-copy instruction parser.
/// Borrows namespace directly from instruction data.
pub enum Instruction<'a> {
    /// Create a bitmap PDA permissionlessly (discriminator = 0)
    CreateBitmap { namespace: &'a [u8], sequence: u64 },
    /// Mark a sequence number as used (discriminator = 1)
    MarkUsed { namespace: &'a [u8], sequence: u64 },
}

impl<'a> Instruction<'a> {
    const CREATE_BITMAP: u8 = 0;
    const MARK_USED: u8 = 1;

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

/// Bits per bitmap bucket (256 bits = 32 bytes)
pub const BITS_PER_BUCKET: u64 = 256;
/// Size of the bitmap in bytes
pub const BITMAP_BYTES: usize = (BITS_PER_BUCKET / 8) as usize;
/// Total account size: [bump: u8][bitmap: 32 bytes] = 33 bytes
pub const BITMAP_ACCOUNT_SIZE: usize = 1 + BITMAP_BYTES;

/// Zero-copy wrapper for bitmap account data.
/// Layout: [bump: u8][bitmap: 32 bytes]
pub struct BitmapAccount<'a> {
    pub bump: &'a mut u8,
    pub bitmap: &'a mut [u8; BITMAP_BYTES],
}

impl<'a> BitmapAccount<'a> {
    /// Wrap account data. Returns None if data is too small.
    #[inline]
    pub fn from_slice(data: &'a mut [u8]) -> Option<Self> {
        if data.len() < BITMAP_ACCOUNT_SIZE {
            return None;
        }
        let (bump, rest) = data.split_at_mut(1);
        let bitmap = <&mut [u8; BITMAP_BYTES]>::try_from(&mut rest[..BITMAP_BYTES]).ok()?;
        Some(Self {
            bump: &mut bump[0],
            bitmap,
        })
    }

    /// Check if a sequence number is marked as used.
    #[inline]
    pub fn is_used(&self, sequence: u64) -> bool {
        let bit_index = (sequence % BITS_PER_BUCKET) as usize;
        let byte_index = bit_index / 8;
        let bit_offset = bit_index % 8;
        self.bitmap[byte_index] & (1 << bit_offset) != 0
    }

    /// Mark a sequence number as used. Returns true if it was already used.
    #[inline]
    pub fn mark_used(&mut self, sequence: u64) -> bool {
        let bit_index = (sequence % BITS_PER_BUCKET) as usize;
        let byte_index = bit_index / 8;
        let bit_offset = bit_index % 8;
        let was_used = self.bitmap[byte_index] & (1 << bit_offset) != 0;
        self.bitmap[byte_index] |= 1 << bit_offset;
        was_used
    }
}

/// Maximum namespace length (2 chunks * 32 bytes = 64 bytes)
/// Seeds: [authority (32), ns_chunk_0, ns_chunk_1, bucket_index (8)]
pub const MAX_NAMESPACE_LEN: usize = 64;

/// Size of each seed component for namespace chunking
const SEED_CHUNK_SIZE: usize = 32;

/// Create and assign a PDA with the given space.
/// Uses single CPI for new accounts, 3 CPIs for pre-funded accounts.
fn create_pda<'a>(
    payer: &'a AccountView,
    pda: &'a AccountView,
    owner: &Address,
    space: u64,
    signers: &[Signer],
) -> ProgramResult {
    let current_lamports = pda.lamports();

    let create_account = CreateAccount::with_minimum_balance(payer, pda, space, owner, None)?;

    if current_lamports == 0 {
        create_account.invoke_signed(signers)?;
    } else {
        let required_lamports = create_account.lamports;
        // Pre-funded account: need 3 separate CPIs

        // Transfer additional lamports if needed
        if current_lamports < required_lamports {
            Transfer {
                from: payer,
                to: pda,
                lamports: required_lamports - current_lamports,
            }
            .invoke()?;
        }

        // Allocate space
        Allocate {
            account: pda,
            space,
        }
        .invoke_signed(signers)?;

        // Assign to owner
        Assign {
            account: pda,
            owner,
        }
        .invoke_signed(signers)?;
    }

    Ok(())
}

/// Build signer seeds for PDA.
fn build_signer<'a>(
    authority: &'a [u8],
    pda_seeds: &'a BitmapPdaSeeds<'a>,
    bump_seed: &'a [u8],
) -> [Seed<'a>; 5] {
    let seeds = pda_seeds.as_seeds_with_bump(authority, bump_seed);
    [
        Seed::from(seeds[0]),
        Seed::from(seeds[1]),
        Seed::from(seeds[2]),
        Seed::from(seeds[3]),
        Seed::from(seeds[4]),
    ]
}

/// Initialize a bitmap PDA if it doesn't exist yet, and verify the PDA is correct.
/// Returns the bump seed (either from creation or from existing account).
fn init_bitmap_pda<'a>(
    payer: &'a AccountView,
    authority: &'a AccountView,
    bitmap_pda: &'a AccountView,
    pda_seeds: &BitmapPdaSeeds,
    program_id: &Address,
) -> Result<u8, ProgramError> {
    let pda_owner = unsafe { bitmap_pda.owner() };

    if pda_owner != program_id {
        // Need to create - derive PDA to get bump
        let (expected_pda, bump) = pda_seeds.find_pda(authority.address(), program_id);

        if bitmap_pda.address() != &expected_pda {
            return Err(ProgramError::InvalidSeeds);
        }

        let bump_seed = [bump];
        let signer_seeds = build_signer(authority.address().as_ref(), pda_seeds, &bump_seed);
        let signers = [Signer::from(signer_seeds.as_ref())];

        create_pda(
            payer,
            bitmap_pda,
            program_id,
            BITMAP_ACCOUNT_SIZE as u64,
            &signers,
        )?;

        // Store bump in the account
        // SAFETY: We have exclusive write access to the PDA data after creation/validation.
        // No other references exist.
        let account_data = unsafe { bitmap_pda.borrow_unchecked_mut() };
        let bitmap =
            BitmapAccount::from_slice(account_data).ok_or(ProgramError::AccountDataTooSmall)?;
        *bitmap.bump = bump;

        Ok(bump)
    } else {
        // Account exists - read bump and verify PDA
        // SAFETY: We have exclusive write access to the PDA data after creation/validation.
        // No other references exist.
        let account_data = unsafe { bitmap_pda.borrow_unchecked_mut() };
        let bitmap =
            BitmapAccount::from_slice(account_data).ok_or(ProgramError::AccountDataTooSmall)?;
        let bump = *bitmap.bump;

        let bump_slice = [bump];
        let seeds = pda_seeds.as_seeds_with_bump(authority.address().as_ref(), &bump_slice);
        let expected_pda = Address::create_program_address(&seeds, program_id)
            .map_err(|_| ProgramError::InvalidSeeds)?;

        if bitmap_pda.address() != &expected_pda {
            return Err(ProgramError::InvalidSeeds);
        }

        Ok(bump)
    }
}

fn process_instruction(
    program_id: &Address,
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match Instruction::parse(instruction_data)? {
        Instruction::CreateBitmap {
            namespace,
            sequence,
        } => process_create_bitmap(program_id, accounts, namespace, sequence),
        Instruction::MarkUsed {
            namespace,
            sequence,
        } => process_mark_used(program_id, accounts, namespace, sequence),
    }
}

/// Creates a bitmap PDA permissionlessly.
///
/// This allows anyone to pre-create and fund bitmap accounts, reducing
/// compute and cost for the authority when they later call MarkUsed.
///
/// # Accounts
/// 0. `[writable, signer]` payer - Pays for PDA creation
/// 1. `[]` authority - Used for PDA derivation (does NOT need to sign)
/// 2. `[writable]` bitmap_pda - PDA to create
/// 3. `[]` system_program - System program
///
/// # Instruction Data
/// | Offset | Size | Description |
/// |--------|------|-------------|
/// | 0      | 1    | discriminator (0) |
/// | 1      | 2    | namespace_len (u16 LE) |
/// | 3      | var  | namespace (0-64 bytes) |
/// | 3+len  | 8    | sequence (u64 LE) - used to derive bucket |
fn process_create_bitmap(
    program_id: &Address,
    accounts: &[AccountView],
    namespace: &[u8],
    sequence: u64,
) -> ProgramResult {
    let [payer, authority, bitmap_pda, _system_program] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !payer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Authority does NOT need to sign - this is permissionless

    let pda_seeds = BitmapPdaSeeds::new(namespace, sequence);

    init_bitmap_pda(payer, authority, bitmap_pda, &pda_seeds, program_id)?;

    Ok(())
}

/// Marks a sequence number as used for replay protection.
///
/// # Accounts
/// 0. `[writable, signer]` payer - Pays for PDA creation if needed
/// 1. `[signer]` authority - Owner of the sequence space (included in PDA seeds)
/// 2. `[writable]` bitmap_pda - PDA storing the bitmap for this bucket
/// 3. `[]` system_program - System program for PDA creation
///
/// # Instruction Data
/// | Offset | Size | Description |
/// |--------|------|-------------|
/// | 0      | 1    | discriminator (1) |
/// | 1      | 2    | namespace_len (u16 LE) |
/// | 3      | var  | namespace (0-64 bytes) |
/// | 3+len  | 8    | sequence (u64 LE) |
fn process_mark_used(
    program_id: &Address,
    accounts: &[AccountView],
    namespace: &[u8],
    sequence: u64,
) -> ProgramResult {
    let [payer, authority, bitmap_pda, _system_program] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !payer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Authority MUST sign to prevent DOS attacks
    if !authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let pda_seeds = BitmapPdaSeeds::new(namespace, sequence);

    // Initialize PDA if needed (also verifies PDA is correct)
    init_bitmap_pda(payer, authority, bitmap_pda, &pda_seeds, program_id)?;

    // Get mutable access to bitmap data
    // SAFETY: We have exclusive write access to the PDA data after creation/validation.
    // No other references exist.
    let account_data = unsafe { bitmap_pda.borrow_unchecked_mut() };
    let mut bitmap =
        BitmapAccount::from_slice(account_data).ok_or(ProgramError::AccountDataTooSmall)?;

    // Mark sequence as used, fail if already used (replay protection)
    if bitmap.mark_used(sequence) {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    Ok(())
}

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
