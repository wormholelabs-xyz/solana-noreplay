//! Benchmarks comparing compute unit usage for different account creation paths.
//!
//! Run with: cargo bench --package solana-noreplay-tests

use mollusk_svm::Mollusk;
use mollusk_svm_bencher::MolluskComputeUnitBencher;
use solana_account::Account;
use solana_instruction::{AccountMeta, Instruction};
use solana_pubkey::Pubkey;

const SYSTEM_PROGRAM_ID: Pubkey = solana_pubkey::pubkey!("11111111111111111111111111111111");
const BITS_PER_BUCKET: u64 = 256;
const SEED_CHUNK_SIZE: usize = 32;

/// Instruction discriminators (must match program)
const IX_CREATE_BITMAP: u8 = 0;
const IX_MARK_USED: u8 = 1;

fn program_id() -> Pubkey {
    "rep1ayXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"
        .parse()
        .unwrap()
}

fn split_namespace(namespace: &[u8]) -> [&[u8]; 2] {
    let mid = namespace.len().min(SEED_CHUNK_SIZE);
    [&namespace[..mid], &namespace[mid..]]
}

fn derive_bitmap_pda(authority: &Pubkey, namespace: &[u8], sequence: u64) -> (Pubkey, u8) {
    let program_id = program_id();
    let bucket_index = sequence / BITS_PER_BUCKET;
    let bucket_bytes = bucket_index.to_le_bytes();
    let [ns0, ns1] = split_namespace(namespace);
    let seeds: [&[u8]; 4] = [authority.as_ref(), ns0, ns1, &bucket_bytes];
    Pubkey::find_program_address(&seeds, &program_id)
}

fn build_instruction_data(discriminator: u8, namespace: &[u8], sequence: u64) -> Vec<u8> {
    let namespace_len = namespace.len() as u16;
    let mut data = Vec::with_capacity(1 + 2 + namespace.len() + 8);
    data.push(discriminator);
    data.extend_from_slice(&namespace_len.to_le_bytes());
    data.extend_from_slice(namespace);
    data.extend_from_slice(&sequence.to_le_bytes());
    data
}

fn build_create_bitmap_instruction(
    payer: &Pubkey,
    authority: &Pubkey,
    namespace: &[u8],
    sequence: u64,
) -> Instruction {
    let program_id = program_id();
    let (pda, _bump) = derive_bitmap_pda(authority, namespace, sequence);

    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*authority, false), // NOT a signer for CreateBitmap
            AccountMeta::new(pda, false),
            AccountMeta::new_readonly(SYSTEM_PROGRAM_ID, false),
        ],
        data: build_instruction_data(IX_CREATE_BITMAP, namespace, sequence),
    }
}

fn build_mark_used_instruction(
    payer: &Pubkey,
    authority: &Pubkey,
    namespace: &[u8],
    sequence: u64,
) -> Instruction {
    let program_id = program_id();
    let (pda, _bump) = derive_bitmap_pda(authority, namespace, sequence);

    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*authority, true), // signer for MarkUsed
            AccountMeta::new(pda, false),
            AccountMeta::new_readonly(SYSTEM_PROGRAM_ID, false),
        ],
        data: build_instruction_data(IX_MARK_USED, namespace, sequence),
    }
}

/// Account size: 1 byte bump + 32 bytes bitmap = 33 bytes
const ACCOUNT_SIZE: usize = 33;

fn rent_for_bitmap() -> u64 {
    // Higher than rent-exempt minimum to ensure prefunded_full skips Transfer CPI
    1_200_000
}

/// Create an account with bump stored at offset 0
fn account_with_bump(lamports: u64, bump: u8, owner: &Pubkey) -> Account {
    let mut data = vec![0u8; ACCOUNT_SIZE];
    data[0] = bump; // Store bump at offset 0
    Account {
        lamports,
        data,
        owner: *owner,
        executable: false,
        rent_epoch: 0,
    }
}

fn main() {
    std::env::set_var("SBF_OUT_DIR", "../target/deploy");

    let program_id = program_id();
    let mollusk = Mollusk::new(&program_id, "solana_noreplay");

    let payer = Pubkey::new_unique();
    let authority = Pubkey::new_unique();
    let namespace = b"bench";
    let rent_exempt_min = rent_for_bitmap();

    let system_program_account = {
        let mut acc = Account::default();
        acc.executable = true;
        acc.owner = solana_pubkey::pubkey!("NativeLoader1111111111111111111111111111111");
        acc
    };

    // =========================================================================
    // MarkUsed benchmarks
    // =========================================================================

    // MarkUsed: New account (0 lamports) -> single CreateAccount CPI
    let sequence_new = 1u64;
    let (pda_new, _) = derive_bitmap_pda(&authority, namespace, sequence_new);
    let ix_mark_new = build_mark_used_instruction(&payer, &authority, namespace, sequence_new);
    let accounts_mark_new: Vec<(Pubkey, Account)> = vec![
        (payer, Account::new(10_000_000_000, 0, &SYSTEM_PROGRAM_ID)),
        (authority, Account::new(0, 0, &SYSTEM_PROGRAM_ID)),
        (pda_new, Account::default()),
        (SYSTEM_PROGRAM_ID, system_program_account.clone()),
    ];

    // MarkUsed: Partially pre-funded -> Transfer + Allocate + Assign (3 CPIs)
    let sequence_prefunded = 2u64;
    let (pda_prefunded, _) = derive_bitmap_pda(&authority, namespace, sequence_prefunded);
    let ix_mark_prefunded =
        build_mark_used_instruction(&payer, &authority, namespace, sequence_prefunded);
    let accounts_mark_prefunded: Vec<(Pubkey, Account)> = vec![
        (payer, Account::new(10_000_000_000, 0, &SYSTEM_PROGRAM_ID)),
        (authority, Account::new(0, 0, &SYSTEM_PROGRAM_ID)),
        (
            pda_prefunded,
            Account::new(rent_exempt_min / 2, 0, &SYSTEM_PROGRAM_ID),
        ),
        (SYSTEM_PROGRAM_ID, system_program_account.clone()),
    ];

    // MarkUsed: Fully pre-funded -> Allocate + Assign (2 CPIs)
    let sequence_fully_funded = 3u64;
    let (pda_fully_funded, _) = derive_bitmap_pda(&authority, namespace, sequence_fully_funded);
    let ix_mark_fully_funded =
        build_mark_used_instruction(&payer, &authority, namespace, sequence_fully_funded);
    let accounts_mark_fully_funded: Vec<(Pubkey, Account)> = vec![
        (payer, Account::new(10_000_000_000, 0, &SYSTEM_PROGRAM_ID)),
        (authority, Account::new(0, 0, &SYSTEM_PROGRAM_ID)),
        (
            pda_fully_funded,
            Account::new(rent_exempt_min, 0, &SYSTEM_PROGRAM_ID),
        ),
        (SYSTEM_PROGRAM_ID, system_program_account.clone()),
    ];

    // MarkUsed: Account already exists (owned by program) -> 0 CPIs
    let sequence_existing = 4u64;
    let (pda_existing, bump_existing) = derive_bitmap_pda(&authority, namespace, sequence_existing);
    let ix_mark_existing =
        build_mark_used_instruction(&payer, &authority, namespace, sequence_existing);
    let accounts_mark_existing: Vec<(Pubkey, Account)> = vec![
        (payer, Account::new(10_000_000_000, 0, &SYSTEM_PROGRAM_ID)),
        (authority, Account::new(0, 0, &SYSTEM_PROGRAM_ID)),
        (
            pda_existing,
            account_with_bump(rent_exempt_min, bump_existing, &program_id),
        ),
        (SYSTEM_PROGRAM_ID, system_program_account.clone()),
    ];

    // =========================================================================
    // CreateBitmap benchmarks
    // =========================================================================

    // CreateBitmap: New account (0 lamports) -> single CreateAccount CPI
    let sequence_create_new = 10u64;
    let (pda_create_new, _) = derive_bitmap_pda(&authority, namespace, sequence_create_new);
    let ix_create_new =
        build_create_bitmap_instruction(&payer, &authority, namespace, sequence_create_new);
    let accounts_create_new: Vec<(Pubkey, Account)> = vec![
        (payer, Account::new(10_000_000_000, 0, &SYSTEM_PROGRAM_ID)),
        (authority, Account::new(0, 0, &SYSTEM_PROGRAM_ID)),
        (pda_create_new, Account::default()),
        (SYSTEM_PROGRAM_ID, system_program_account.clone()),
    ];

    // CreateBitmap: Account already exists -> no-op
    let sequence_create_existing = 11u64;
    let (pda_create_existing, bump_create_existing) =
        derive_bitmap_pda(&authority, namespace, sequence_create_existing);
    let ix_create_existing =
        build_create_bitmap_instruction(&payer, &authority, namespace, sequence_create_existing);
    let accounts_create_existing: Vec<(Pubkey, Account)> = vec![
        (payer, Account::new(10_000_000_000, 0, &SYSTEM_PROGRAM_ID)),
        (authority, Account::new(0, 0, &SYSTEM_PROGRAM_ID)),
        (
            pda_create_existing,
            account_with_bump(rent_exempt_min, bump_create_existing, &program_id),
        ),
        (SYSTEM_PROGRAM_ID, system_program_account.clone()),
    ];

    MolluskComputeUnitBencher::new(mollusk)
        // MarkUsed scenarios
        .bench(("mark_used__new_account", &ix_mark_new, &accounts_mark_new))
        .bench((
            "mark_used__prefunded_partial",
            &ix_mark_prefunded,
            &accounts_mark_prefunded,
        ))
        .bench((
            "mark_used__prefunded_full",
            &ix_mark_fully_funded,
            &accounts_mark_fully_funded,
        ))
        .bench((
            "mark_used__existing_account",
            &ix_mark_existing,
            &accounts_mark_existing,
        ))
        // CreateBitmap scenarios
        .bench((
            "create_bitmap__new_account",
            &ix_create_new,
            &accounts_create_new,
        ))
        .bench((
            "create_bitmap__existing_account",
            &ix_create_existing,
            &accounts_create_existing,
        ))
        .must_pass(true)
        .out_dir("../target/benches")
        .execute();
}
