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

fn build_instruction(
    payer: &Pubkey,
    authority: &Pubkey,
    namespace: &[u8],
    sequence: u64,
) -> Instruction {
    let program_id = program_id();
    let (pda, _bump) = derive_bitmap_pda(authority, namespace, sequence);

    let namespace_len = namespace.len() as u16;
    let mut data = Vec::with_capacity(2 + namespace.len() + 8);
    data.extend_from_slice(&namespace_len.to_le_bytes());
    data.extend_from_slice(namespace);
    data.extend_from_slice(&sequence.to_le_bytes());

    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*authority, true),
            AccountMeta::new(pda, false),
            AccountMeta::new_readonly(SYSTEM_PROGRAM_ID, false),
        ],
        data,
    }
}

fn rent_for_bitmap() -> u64 {
    // Higher than rent-exempt minimum to ensure prefunded_full skips Transfer CPI
    1_200_000
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

    // Scenario 1: New account (0 lamports) -> single CreateAccount CPI
    let sequence_new = 1u64;
    let (pda_new, _) = derive_bitmap_pda(&authority, namespace, sequence_new);
    let ix_new = build_instruction(&payer, &authority, namespace, sequence_new);
    let accounts_new: Vec<(Pubkey, Account)> = vec![
        (payer, Account::new(10_000_000_000, 0, &SYSTEM_PROGRAM_ID)),
        (authority, Account::new(0, 0, &SYSTEM_PROGRAM_ID)),
        (pda_new, Account::default()),
        (SYSTEM_PROGRAM_ID, system_program_account.clone()),
    ];

    // Scenario 2: Partially pre-funded -> Transfer + Allocate + Assign (3 CPIs)
    let sequence_prefunded = 2u64;
    let (pda_prefunded, _) = derive_bitmap_pda(&authority, namespace, sequence_prefunded);
    let ix_prefunded = build_instruction(&payer, &authority, namespace, sequence_prefunded);
    let accounts_prefunded: Vec<(Pubkey, Account)> = vec![
        (payer, Account::new(10_000_000_000, 0, &SYSTEM_PROGRAM_ID)),
        (authority, Account::new(0, 0, &SYSTEM_PROGRAM_ID)),
        (pda_prefunded, Account::new(rent_exempt_min / 2, 0, &SYSTEM_PROGRAM_ID)),
        (SYSTEM_PROGRAM_ID, system_program_account.clone()),
    ];

    // Scenario 3: Fully pre-funded -> Allocate + Assign (2 CPIs)
    let sequence_fully_funded = 3u64;
    let (pda_fully_funded, _) = derive_bitmap_pda(&authority, namespace, sequence_fully_funded);
    let ix_fully_funded = build_instruction(&payer, &authority, namespace, sequence_fully_funded);
    let accounts_fully_funded: Vec<(Pubkey, Account)> = vec![
        (payer, Account::new(10_000_000_000, 0, &SYSTEM_PROGRAM_ID)),
        (authority, Account::new(0, 0, &SYSTEM_PROGRAM_ID)),
        (pda_fully_funded, Account::new(rent_exempt_min, 0, &SYSTEM_PROGRAM_ID)),
        (SYSTEM_PROGRAM_ID, system_program_account.clone()),
    ];

    // Scenario 4: Account already exists (owned by program) -> 0 CPIs
    let sequence_existing = 4u64;
    let (pda_existing, _) = derive_bitmap_pda(&authority, namespace, sequence_existing);
    let ix_existing = build_instruction(&payer, &authority, namespace, sequence_existing);
    let accounts_existing: Vec<(Pubkey, Account)> = vec![
        (payer, Account::new(10_000_000_000, 0, &SYSTEM_PROGRAM_ID)),
        (authority, Account::new(0, 0, &SYSTEM_PROGRAM_ID)),
        (pda_existing, Account::new(rent_exempt_min, 32, &program_id)),
        (SYSTEM_PROGRAM_ID, system_program_account.clone()),
    ];

    MolluskComputeUnitBencher::new(mollusk)
        .bench(("new_account_single_cpi", &ix_new, &accounts_new))
        .bench(("prefunded_partial_triple_cpi", &ix_prefunded, &accounts_prefunded))
        .bench(("prefunded_full_double_cpi", &ix_fully_funded, &accounts_fully_funded))
        .bench(("existing_account_no_cpi", &ix_existing, &accounts_existing))
        .must_pass(true)
        .out_dir("../target/benches")
        .execute();
}
