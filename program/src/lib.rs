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

/// Instruction discriminators
const IX_CREATE_BITMAP: u8 = 0;
const IX_MARK_USED: u8 = 1;

/// Bits per bitmap PDA (256 bits = 32 bytes)
const BITS_PER_BUCKET: u64 = 256;
const BYTES_PER_BUCKET: usize = (BITS_PER_BUCKET / 8) as usize; // 32

/// Maximum namespace length (2 chunks * 32 bytes = 64 bytes)
/// Seeds: [authority (32), ns_chunk_0, ns_chunk_1, bucket_index (8)]
const MAX_NAMESPACE_LEN: usize = 64;

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

/// Parse instruction data after discriminator.
/// Returns (namespace, sequence) or error.
fn parse_instruction_data(data: &[u8]) -> Result<(&[u8], u64), ProgramError> {
    // Minimum: 2 (len) + 0 (empty namespace) + 8 (sequence)
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

    Ok((namespace, sequence))
}

/// Build signer seeds for PDA.
fn build_signer<'a>(
    authority: &'a [u8],
    pda_seeds: &'a BitmapPdaSeeds<'a>,
    bump_seed: &'a [u8],
) -> [Seed<'a>; 5] {
    [
        Seed::from(authority),
        Seed::from(pda_seeds.ns_chunks[0]),
        Seed::from(pda_seeds.ns_chunks[1]),
        Seed::from(pda_seeds.bucket_bytes.as_ref()),
        Seed::from(bump_seed),
    ]
}

fn process_instruction(
    program_id: &Address,
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    if instruction_data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }

    match instruction_data[0] {
        IX_CREATE_BITMAP => process_create_bitmap(program_id, accounts, &instruction_data[1..]),
        IX_MARK_USED => process_mark_used(program_id, accounts, &instruction_data[1..]),
        _ => Err(ProgramError::InvalidInstructionData),
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
    data: &[u8],
) -> ProgramResult {
    let [payer, authority, bitmap_pda, _system_program] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !payer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }

    // Authority does NOT need to sign - this is permissionless

    let (namespace, sequence) = parse_instruction_data(data)?;

    let pda_seeds = BitmapPdaSeeds::new(namespace, sequence);
    let (expected_pda, bump) = pda_seeds.find_pda(authority.address(), program_id);

    if bitmap_pda.address() != &expected_pda {
        return Err(ProgramError::InvalidSeeds);
    }

    let pda_owner = unsafe { bitmap_pda.owner() };

    // Only create if not already owned by this program
    if pda_owner != program_id {
        let bump_seed = [bump];
        let signer_seeds = build_signer(authority.address().as_ref(), &pda_seeds, &bump_seed);
        let signers = [Signer::from(signer_seeds.as_ref())];

        create_pda(
            payer,
            bitmap_pda,
            program_id,
            BYTES_PER_BUCKET as u64,
            &signers,
        )?;
    }

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
fn process_mark_used(program_id: &Address, accounts: &[AccountView], data: &[u8]) -> ProgramResult {
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

    let (namespace, sequence) = parse_instruction_data(data)?;

    // Calculate bit position within bucket
    let bit_index = (sequence % BITS_PER_BUCKET) as usize;
    let byte_index = bit_index / 8;
    let bit_offset = bit_index % 8;

    let pda_seeds = BitmapPdaSeeds::new(namespace, sequence);
    let (expected_pda, bump) = pda_seeds.find_pda(authority.address(), program_id);

    if bitmap_pda.address() != &expected_pda {
        return Err(ProgramError::InvalidSeeds);
    }

    let pda_owner = unsafe { bitmap_pda.owner() };

    // Create bitmap PDA if it doesn't exist yet
    if pda_owner != program_id {
        let bump_seed = [bump];
        let signer_seeds = build_signer(authority.address().as_ref(), &pda_seeds, &bump_seed);
        let signers = [Signer::from(signer_seeds.as_ref())];

        create_pda(
            payer,
            bitmap_pda,
            program_id,
            BYTES_PER_BUCKET as u64,
            &signers,
        )?;
    }

    // Get mutable access to bitmap data
    let data = unsafe { bitmap_pda.borrow_unchecked_mut() };

    // Check if bit is already set (replay protection)
    if data[byte_index] & (1 << bit_offset) != 0 {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    // Set the bit to mark sequence as used
    data[byte_index] |= 1 << bit_offset;

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

    /// Build the seeds array for PDA derivation.
    pub fn as_seeds(&self, authority: &'a [u8]) -> [&[u8]; 4] {
        [
            authority,
            self.ns_chunks[0],
            self.ns_chunks[1],
            &self.bucket_bytes,
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
