use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    rent::Rent,
};

pub const PROGRAM_ID: Pubkey = solana_sdk::pubkey!("rep1ayXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX");

/// Instruction discriminators (must match program)
const IX_CREATE_BITMAP: u8 = 0;
const IX_MARK_USED: u8 = 1;

/// Bits per bitmap bucket (must match program)
pub const BITS_PER_BUCKET: u64 = 256;

/// Maximum namespace length (2 chunks * 32 bytes = 64 bytes)
pub const MAX_NAMESPACE_LEN: usize = 64;

/// Size of each seed component for namespace chunking
const SEED_CHUNK_SIZE: usize = 32;

pub fn load_program() -> Vec<u8> {
    std::fs::read("../target/deploy/solana_noreplay.so")
        .expect("Program not built. Run `cargo build-sbf` first.")
}

/// Split a namespace into two chunks for PDA seed derivation.
/// Each chunk is up to 32 bytes. Second chunk may be empty.
fn split_namespace(namespace: &[u8]) -> [&[u8]; 2] {
    let mid = namespace.len().min(SEED_CHUNK_SIZE);
    [&namespace[..mid], &namespace[mid..]]
}

/// Derive the bitmap PDA for a given authority, namespace, and sequence number.
/// Seeds are always: [authority, ns_chunk_0, ns_chunk_1, bucket_index]
pub fn derive_bitmap_pda(authority: &Pubkey, namespace: &[u8], sequence: u64) -> (Pubkey, u8) {
    let bucket_index = sequence / BITS_PER_BUCKET;
    let bucket_bytes = bucket_index.to_le_bytes();
    let [ns0, ns1] = split_namespace(namespace);

    let seeds: [&[u8]; 4] = [authority.as_ref(), ns0, ns1, &bucket_bytes];

    Pubkey::find_program_address(&seeds, &PROGRAM_ID)
}

/// Build instruction data for namespace + sequence (shared by both instructions)
fn build_instruction_data(discriminator: u8, namespace: &[u8], sequence: u64) -> Vec<u8> {
    let namespace_len = namespace.len() as u16;
    let mut data = Vec::with_capacity(1 + 2 + namespace.len() + 8);
    data.push(discriminator);
    data.extend_from_slice(&namespace_len.to_le_bytes());
    data.extend_from_slice(namespace);
    data.extend_from_slice(&sequence.to_le_bytes());
    data
}

/// Build instruction to create a bitmap PDA permissionlessly
pub fn create_bitmap_instruction(
    payer: &Pubkey,
    authority: &Pubkey,
    namespace: &[u8],
    sequence: u64,
) -> Instruction {
    let (pda, _bump) = derive_bitmap_pda(authority, namespace, sequence);

    Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*payer, true),               // payer, signer
            AccountMeta::new_readonly(*authority, false), // authority, NOT a signer
            AccountMeta::new(pda, false),                 // bitmap PDA
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: build_instruction_data(IX_CREATE_BITMAP, namespace, sequence),
    }
}

/// Build instruction to mark a sequence number as used
pub fn mark_used_instruction(
    payer: &Pubkey,
    authority: &Pubkey,
    namespace: &[u8],
    sequence: u64,
) -> Instruction {
    let (pda, _bump) = derive_bitmap_pda(authority, namespace, sequence);

    Instruction {
        program_id: PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*payer, true),              // payer, signer
            AccountMeta::new_readonly(*authority, true), // authority, signer
            AccountMeta::new(pda, false),                // bitmap PDA
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: build_instruction_data(IX_MARK_USED, namespace, sequence),
    }
}

/// Account size: 1 byte bump + 32 bytes bitmap = 33 bytes
const ACCOUNT_SIZE: usize = 33;

/// Rent cost for a bitmap PDA (33 bytes)
pub fn rent_for_bitmap() -> u64 {
    Rent::default().minimum_balance(ACCOUNT_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use litesvm::LiteSVM;
    use proptest::prelude::*;
    use solana_sdk::{
        native_token::LAMPORTS_PER_SOL, signature::Keypair, signer::Signer,
        transaction::Transaction,
    };

    #[test]
    fn lamports_cost_is_rent_exempt_minimum() {
        let mut svm = LiteSVM::new();
        svm.add_program(PROGRAM_ID, &load_program());

        let authority = Keypair::new();
        let initial_balance = 10 * LAMPORTS_PER_SOL;
        svm.airdrop(&authority.pubkey(), initial_balance).unwrap();

        let namespace = b"test";
        let sequence = 42u64;
        let (pda, _) = derive_bitmap_pda(&authority.pubkey(), namespace, sequence);

        // Record balance before
        let balance_before = svm.get_balance(&authority.pubkey()).unwrap();

        // Execute transaction (authority is both payer and authority)
        let ix = mark_used_instruction(
            &authority.pubkey(),
            &authority.pubkey(),
            namespace,
            sequence,
        );
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[&authority],
            blockhash,
        );
        let result = svm.send_transaction(tx);
        assert!(result.is_ok(), "Transaction should succeed: {:?}", result);

        // Check balances after
        let balance_after = svm.get_balance(&authority.pubkey()).unwrap();
        let pda_balance = svm.get_balance(&pda).unwrap();

        // PDA should have exactly rent-exempt minimum for 32-byte bitmap
        let expected_rent = rent_for_bitmap();
        assert_eq!(
            pda_balance, expected_rent,
            "PDA should have rent-exempt minimum: expected {}, got {}",
            expected_rent, pda_balance
        );

        // Payer should have paid rent + transaction fee
        let lamports_spent = balance_before - balance_after;
        assert!(
            lamports_spent >= expected_rent,
            "Payer should spend at least rent: spent {}, rent {}",
            lamports_spent,
            expected_rent
        );

        // The difference should be the transaction fee (5000 lamports by default)
        let tx_fee = lamports_spent - expected_rent;
        assert_eq!(
            tx_fee, 5000,
            "Transaction fee should be 5000 lamports: got {}",
            tx_fee
        );
    }

    #[test]
    fn works_when_pda_prefunded() {
        let mut svm = LiteSVM::new();
        svm.add_program(PROGRAM_ID, &load_program());

        let authority = Keypair::new();
        let attacker = Keypair::new();
        svm.airdrop(&authority.pubkey(), 10 * LAMPORTS_PER_SOL)
            .unwrap();
        svm.airdrop(&attacker.pubkey(), 10 * LAMPORTS_PER_SOL)
            .unwrap();

        let namespace = b"test";
        let sequence = 123u64;
        let (pda, _) = derive_bitmap_pda(&authority.pubkey(), namespace, sequence);

        // Attacker sends lamports to the PDA before it's used
        let transfer_ix = solana_sdk::system_instruction::transfer(
            &attacker.pubkey(),
            &pda,
            1_000_000, // 0.001 SOL
        );
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[transfer_ix],
            Some(&attacker.pubkey()),
            &[&attacker],
            blockhash,
        );
        svm.send_transaction(tx).unwrap();

        // Verify PDA now has lamports
        let pda_balance = svm.get_balance(&pda).unwrap();
        assert!(pda_balance > 0, "PDA should have lamports from attacker");

        svm.expire_blockhash();

        // Authority should still be able to claim this sequence
        let ix = mark_used_instruction(
            &authority.pubkey(),
            &authority.pubkey(),
            namespace,
            sequence,
        );
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[&authority],
            blockhash,
        );
        let result = svm.send_transaction(tx);
        assert!(
            result.is_ok(),
            "Should succeed even with pre-funded PDA: {:?}",
            result
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(20))]

        /// Property: Once a sequence number is replay-protected, it cannot be used again
        #[test]
        fn replay_protection_prevents_reuse(sequence: u64) {
            let mut svm = LiteSVM::new();
            svm.add_program(PROGRAM_ID, &load_program());

            let authority = Keypair::new();
            let namespace = b"test";
            svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

            // First use should succeed
            let ix = mark_used_instruction(&authority.pubkey(), &authority.pubkey(), namespace, sequence);
            let blockhash = svm.latest_blockhash();
            let tx = Transaction::new_signed_with_payer(
                &[ix],
                Some(&authority.pubkey()),
                &[&authority],
                blockhash,
            );
            let result = svm.send_transaction(tx);
            prop_assert!(result.is_ok(), "First use of sequence {} should succeed: {:?}", sequence, result);

            // Advance slot to get new blockhash (so tx signature differs)
            svm.expire_blockhash();

            // Second use with same sequence should fail
            let ix = mark_used_instruction(&authority.pubkey(), &authority.pubkey(), namespace, sequence);
            let blockhash = svm.latest_blockhash();
            let tx = Transaction::new_signed_with_payer(
                &[ix],
                Some(&authority.pubkey()),
                &[&authority],
                blockhash,
            );
            let result = svm.send_transaction(tx);
            prop_assert!(result.is_err(), "Second use of sequence {} should fail (replay protection): {:?}", sequence, result);
        }

        /// Property: Different sequence numbers are independent
        #[test]
        fn different_sequences_are_independent(seq1: u64, seq2: u64) {
            prop_assume!(seq1 != seq2);

            let mut svm = LiteSVM::new();
            svm.add_program(PROGRAM_ID, &load_program());

            let authority = Keypair::new();
            let namespace = b"test";
            svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

            // Use seq1
            let ix = mark_used_instruction(&authority.pubkey(), &authority.pubkey(), namespace, seq1);
            let blockhash = svm.latest_blockhash();
            let tx = Transaction::new_signed_with_payer(
                &[ix],
                Some(&authority.pubkey()),
                &[&authority],
                blockhash,
            );
            let result = svm.send_transaction(tx);
            prop_assert!(result.is_ok(), "First sequence {} should succeed: {:?}", seq1, result);

            // Advance slot to get new blockhash
            svm.expire_blockhash();

            // Using seq2 should still work (independent)
            let ix = mark_used_instruction(&authority.pubkey(), &authority.pubkey(), namespace, seq2);
            let blockhash = svm.latest_blockhash();
            let tx = Transaction::new_signed_with_payer(
                &[ix],
                Some(&authority.pubkey()),
                &[&authority],
                blockhash,
            );
            let result = svm.send_transaction(tx);
            prop_assert!(result.is_ok(), "Different sequence {} should succeed: {:?}", seq2, result);
        }

        /// Property: Incremental sequence numbers all work correctly
        #[test]
        fn incremental_sequences_all_work(base in 0u64..u64::MAX - 10) {
            let mut svm = LiteSVM::new();
            svm.add_program(PROGRAM_ID, &load_program());

            let authority = Keypair::new();
            let namespace = b"test";
            svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();

            // Use 10 consecutive sequence numbers
            for i in 0..10u64 {
                let sequence = base.saturating_add(i);

                let ix = mark_used_instruction(&authority.pubkey(), &authority.pubkey(), namespace, sequence);
                let blockhash = svm.latest_blockhash();
                let tx = Transaction::new_signed_with_payer(
                    &[ix],
                    Some(&authority.pubkey()),
                    &[&authority],
                    blockhash,
                );
                let result = svm.send_transaction(tx);
                prop_assert!(result.is_ok(), "Sequence {} (base {} + {}) should succeed: {:?}", sequence, base, i, result);

                svm.expire_blockhash();
            }

            // Verify all 10 are now protected (can't be reused)
            for i in 0..10u64 {
                let sequence = base.saturating_add(i);

                let ix = mark_used_instruction(&authority.pubkey(), &authority.pubkey(), namespace, sequence);
                let blockhash = svm.latest_blockhash();
                let tx = Transaction::new_signed_with_payer(
                    &[ix],
                    Some(&authority.pubkey()),
                    &[&authority],
                    blockhash,
                );
                let result = svm.send_transaction(tx);
                prop_assert!(result.is_err(), "Replay of sequence {} should fail: {:?}", sequence, result);

                svm.expire_blockhash();
            }
        }

        /// Property: Different authorities have independent sequence spaces
        #[test]
        fn different_authorities_are_independent(sequence: u64) {
            let mut svm = LiteSVM::new();
            svm.add_program(PROGRAM_ID, &load_program());

            let authority1 = Keypair::new();
            let authority2 = Keypair::new();
            let namespace = b"test";
            svm.airdrop(&authority1.pubkey(), 10_000_000_000).unwrap();
            svm.airdrop(&authority2.pubkey(), 10_000_000_000).unwrap();

            // Authority 1 uses sequence
            let ix = mark_used_instruction(&authority1.pubkey(), &authority1.pubkey(), namespace, sequence);
            let blockhash = svm.latest_blockhash();
            let tx = Transaction::new_signed_with_payer(
                &[ix],
                Some(&authority1.pubkey()),
                &[&authority1],
                blockhash,
            );
            let result = svm.send_transaction(tx);
            prop_assert!(result.is_ok(), "Authority 1 should succeed with sequence {}: {:?}", sequence, result);

            // Advance slot to get new blockhash
            svm.expire_blockhash();

            // Authority 2 should still be able to use same sequence
            let ix = mark_used_instruction(&authority2.pubkey(), &authority2.pubkey(), namespace, sequence);
            let blockhash = svm.latest_blockhash();
            let tx = Transaction::new_signed_with_payer(
                &[ix],
                Some(&authority2.pubkey()),
                &[&authority2],
                blockhash,
            );
            let result = svm.send_transaction(tx);
            prop_assert!(result.is_ok(), "Authority 2 should succeed with same sequence {}: {:?}", sequence, result);
        }
    }

    // ============================================================================
    // Namespace-specific tests
    // ============================================================================

    #[test]
    fn different_namespaces_are_independent() {
        let mut svm = LiteSVM::new();
        svm.add_program(PROGRAM_ID, &load_program());

        let authority = Keypair::new();
        svm.airdrop(&authority.pubkey(), 10 * LAMPORTS_PER_SOL)
            .unwrap();

        let namespace1 = b"namespace_a";
        let namespace2 = b"namespace_b";
        let sequence = 42u64;

        // Use sequence in namespace1
        let ix = mark_used_instruction(
            &authority.pubkey(),
            &authority.pubkey(),
            namespace1,
            sequence,
        );
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[&authority],
            blockhash,
        );
        assert!(svm.send_transaction(tx).is_ok());

        svm.expire_blockhash();

        // Same sequence in namespace2 should succeed (independent)
        let ix = mark_used_instruction(
            &authority.pubkey(),
            &authority.pubkey(),
            namespace2,
            sequence,
        );
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[&authority],
            blockhash,
        );
        let result = svm.send_transaction(tx);
        assert!(
            result.is_ok(),
            "Same sequence in different namespace should succeed: {:?}",
            result
        );
    }

    #[test]
    fn empty_namespace_works() {
        let mut svm = LiteSVM::new();
        svm.add_program(PROGRAM_ID, &load_program());

        let authority = Keypair::new();
        svm.airdrop(&authority.pubkey(), 10 * LAMPORTS_PER_SOL)
            .unwrap();

        let namespace: &[u8] = b"";
        let sequence = 1u64;

        let ix = mark_used_instruction(
            &authority.pubkey(),
            &authority.pubkey(),
            namespace,
            sequence,
        );
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[&authority],
            blockhash,
        );
        let result = svm.send_transaction(tx);
        assert!(result.is_ok(), "Empty namespace should work: {:?}", result);
    }

    #[test]
    fn short_namespace_works() {
        let mut svm = LiteSVM::new();
        svm.add_program(PROGRAM_ID, &load_program());

        let authority = Keypair::new();
        svm.airdrop(&authority.pubkey(), 10 * LAMPORTS_PER_SOL)
            .unwrap();

        // 10-byte namespace (less than 32)
        let namespace = b"short_ns!!";
        let sequence = 1u64;

        let ix = mark_used_instruction(
            &authority.pubkey(),
            &authority.pubkey(),
            namespace,
            sequence,
        );
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[&authority],
            blockhash,
        );
        let result = svm.send_transaction(tx);
        assert!(result.is_ok(), "Short namespace should work: {:?}", result);
    }

    #[test]
    fn long_namespace_works() {
        let mut svm = LiteSVM::new();
        svm.add_program(PROGRAM_ID, &load_program());

        let authority = Keypair::new();
        svm.airdrop(&authority.pubkey(), 10 * LAMPORTS_PER_SOL)
            .unwrap();

        // 64-byte namespace (spans 2 chunks)
        let namespace = [0xABu8; 64];
        let sequence = 1u64;

        let ix = mark_used_instruction(
            &authority.pubkey(),
            &authority.pubkey(),
            &namespace,
            sequence,
        );
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[&authority],
            blockhash,
        );
        let result = svm.send_transaction(tx);
        assert!(
            result.is_ok(),
            "64-byte namespace should work: {:?}",
            result
        );
    }

    #[test]
    fn max_namespace_length_works() {
        let mut svm = LiteSVM::new();
        svm.add_program(PROGRAM_ID, &load_program());

        let authority = Keypair::new();
        svm.airdrop(&authority.pubkey(), 10 * LAMPORTS_PER_SOL)
            .unwrap();

        // 64-byte namespace (maximum allowed = 2 chunks * 32 bytes)
        let namespace = [0xCDu8; MAX_NAMESPACE_LEN];
        let sequence = 1u64;

        let ix = mark_used_instruction(
            &authority.pubkey(),
            &authority.pubkey(),
            &namespace,
            sequence,
        );
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[&authority],
            blockhash,
        );
        let result = svm.send_transaction(tx);
        assert!(
            result.is_ok(),
            "64-byte namespace should work: {:?}",
            result
        );
    }

    #[test]
    fn namespace_too_long_fails() {
        let mut svm = LiteSVM::new();
        svm.add_program(PROGRAM_ID, &load_program());

        let authority = Keypair::new();
        svm.airdrop(&authority.pubkey(), 10 * LAMPORTS_PER_SOL)
            .unwrap();

        // 65-byte namespace (one byte over maximum)
        let namespace = [0xEFu8; MAX_NAMESPACE_LEN + 1];
        let sequence = 1u64;

        // Manually build instruction data without deriving PDA (since oversized namespace would panic)
        let namespace_len = namespace.len() as u16;
        let mut data = Vec::with_capacity(1 + 2 + namespace.len() + 8);
        data.push(IX_MARK_USED); // discriminator
        data.extend_from_slice(&namespace_len.to_le_bytes());
        data.extend_from_slice(&namespace);
        data.extend_from_slice(&sequence.to_le_bytes());

        // Use a dummy PDA - the program will reject before PDA validation due to namespace length
        let dummy_pda = Pubkey::new_unique();

        let ix = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new_readonly(authority.pubkey(), true),
                AccountMeta::new(dummy_pda, false),
                AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            ],
            data,
        };

        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[&authority],
            blockhash,
        );
        let result = svm.send_transaction(tx);
        assert!(result.is_err(), "65-byte namespace should fail");
    }

    #[test]
    fn authority_must_be_signer() {
        let mut svm = LiteSVM::new();
        svm.add_program(PROGRAM_ID, &load_program());

        let payer = Keypair::new();
        let authority = Keypair::new(); // Authority that won't sign
        svm.airdrop(&payer.pubkey(), 10 * LAMPORTS_PER_SOL).unwrap();

        let namespace = b"test";
        let sequence = 1u64;

        // Create instruction but mark authority as non-signer
        let (pda, _bump) = derive_bitmap_pda(&authority.pubkey(), namespace, sequence);

        let namespace_len = namespace.len() as u16;
        let mut data = Vec::with_capacity(1 + 2 + namespace.len() + 8);
        data.push(IX_MARK_USED); // discriminator
        data.extend_from_slice(&namespace_len.to_le_bytes());
        data.extend_from_slice(namespace);
        data.extend_from_slice(&sequence.to_le_bytes());

        let ix = Instruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(payer.pubkey(), true), // payer, signer
                AccountMeta::new_readonly(authority.pubkey(), false), // authority, NOT a signer!
                AccountMeta::new(pda, false),           // bitmap PDA
                AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            ],
            data,
        };

        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer.pubkey()),
            &[&payer], // Only payer signs, not authority
            blockhash,
        );
        let result = svm.send_transaction(tx);
        assert!(result.is_err(), "Should fail when authority doesn't sign");
    }

    #[test]
    fn separate_payer_and_authority_works() {
        let mut svm = LiteSVM::new();
        svm.add_program(PROGRAM_ID, &load_program());

        let payer = Keypair::new();
        let authority = Keypair::new();
        svm.airdrop(&payer.pubkey(), 10 * LAMPORTS_PER_SOL).unwrap();
        // Authority doesn't need SOL since payer pays

        let namespace = b"test";
        let sequence = 1u64;

        let ix = mark_used_instruction(&payer.pubkey(), &authority.pubkey(), namespace, sequence);

        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer.pubkey()),
            &[&payer, &authority], // Both sign
            blockhash,
        );
        let result = svm.send_transaction(tx);
        assert!(
            result.is_ok(),
            "Should work with separate payer and authority: {:?}",
            result
        );
    }
}
