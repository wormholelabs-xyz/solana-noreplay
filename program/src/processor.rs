use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    AccountView, Address, ProgramResult,
};
use pinocchio_system::instructions::{Allocate, Assign, CreateAccount, Transfer};

use crate::instruction::Instruction;
use crate::pda::BitmapPdaSeeds;
use crate::state::{BitmapAccount, BITMAP_ACCOUNT_SIZE};

/// Process program instructions.
pub fn process_instruction(
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
