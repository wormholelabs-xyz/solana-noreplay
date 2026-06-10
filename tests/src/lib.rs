use solana_sdk::rent::Rent;

// Re-export from the program's client module
pub use solana_noreplay::client::{
    build_instruction_data, derive_bitmap_pda, derive_bitmap_pda_for_bucket, CreateBitmap,
    MarkUsed, MarkUsedBulk, BITMAP_ACCOUNT_SIZE, BITMAP_BYTES, BITS_PER_BUCKET, CREATE_BITMAP,
    MARK_USED, MARK_USED_BULK, MARK_USED_BULK_MASK_LEN, MAX_NAMESPACE_LEN, PROGRAM_ID, UNMARK_USED,
};

pub fn load_program() -> Vec<u8> {
    std::fs::read("../target/deploy/solana_noreplay.so")
        .expect("Program not built. Run `cargo build-sbf` first.")
}

/// Rent cost for a bitmap PDA
pub fn rent_for_bitmap() -> u64 {
    Rent::default().minimum_balance(BITMAP_ACCOUNT_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use litesvm::LiteSVM;
    use proptest::prelude::*;
    use solana_sdk::{
        instruction::{AccountMeta, Instruction as SdkInstruction},
        native_token::LAMPORTS_PER_SOL,
        pubkey::Pubkey,
        signature::Keypair,
        signer::Signer,
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
        let ix = MarkUsed {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace,
            sequence,
        }
        .instruction();
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
        let ix = MarkUsed {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace,
            sequence,
        }
        .instruction();
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
            let ix = MarkUsed {
                payer: &authority.pubkey(),
                authority: &authority.pubkey(),
                namespace,
                sequence,
            }.instruction();
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
            let ix = MarkUsed {
                payer: &authority.pubkey(),
                authority: &authority.pubkey(),
                namespace,
                sequence,
            }.instruction();
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
            let ix = MarkUsed {
                payer: &authority.pubkey(),
                authority: &authority.pubkey(),
                namespace,
                sequence: seq1,
            }.instruction();
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
            let ix = MarkUsed {
                payer: &authority.pubkey(),
                authority: &authority.pubkey(),
                namespace,
                sequence: seq2,
            }.instruction();
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

                let ix = MarkUsed {
                    payer: &authority.pubkey(),
                    authority: &authority.pubkey(),
                    namespace,
                    sequence,
                }.instruction();
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

                let ix = MarkUsed {
                    payer: &authority.pubkey(),
                    authority: &authority.pubkey(),
                    namespace,
                    sequence,
                }.instruction();
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
            let ix = MarkUsed {
                payer: &authority1.pubkey(),
                authority: &authority1.pubkey(),
                namespace,
                sequence,
            }.instruction();
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
            let ix = MarkUsed {
                payer: &authority2.pubkey(),
                authority: &authority2.pubkey(),
                namespace,
                sequence,
            }.instruction();
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
        let ix = MarkUsed {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace: namespace1,
            sequence,
        }
        .instruction();
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
        let ix = MarkUsed {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace: namespace2,
            sequence,
        }
        .instruction();
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

        let ix = MarkUsed {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace,
            sequence,
        }
        .instruction();
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

        let ix = MarkUsed {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace,
            sequence,
        }
        .instruction();
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

        let ix = MarkUsed {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace: &namespace,
            sequence,
        }
        .instruction();
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

        let ix = MarkUsed {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace: &namespace,
            sequence,
        }
        .instruction();
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
        // Can't use MarkUsed helper because PDA derivation panics with oversized namespace
        // (chunk 1 would be 33 bytes, exceeding Solana's 32-byte seed limit)
        let namespace = [0xEFu8; MAX_NAMESPACE_LEN + 1];
        let sequence = 1u64;
        let data = build_instruction_data(MARK_USED, &namespace, sequence);
        let dummy_pda = Pubkey::new_unique();

        let ix = SdkInstruction {
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
        let data = build_instruction_data(MARK_USED, namespace, sequence);

        let ix = SdkInstruction {
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

    // ============================================================================
    // UnmarkUsed tests
    // ============================================================================

    #[test]
    fn unmark_clears_previously_marked_bit() {
        let mut svm = LiteSVM::new();
        svm.add_program(PROGRAM_ID, &load_program());

        let authority = Keypair::new();
        svm.airdrop(&authority.pubkey(), 10 * LAMPORTS_PER_SOL)
            .unwrap();

        let namespace = b"test";
        let sequence = 42u64;

        // Mark it first
        let ix = MarkUsed {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace,
            sequence,
        }
        .instruction();
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[&authority],
            blockhash,
        );
        assert!(svm.send_transaction(tx).is_ok());

        svm.expire_blockhash();

        // Verify it's marked (replay should fail)
        let ix = MarkUsed {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace,
            sequence,
        }
        .instruction();
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[&authority],
            blockhash,
        );
        assert!(svm.send_transaction(tx).is_err(), "Should be marked");

        svm.expire_blockhash();

        // Unmark it
        let ix = MarkUsed {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace,
            sequence,
        }
        .unmark_instruction();
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[&authority],
            blockhash,
        );
        let result = svm.send_transaction(tx);
        assert!(result.is_ok(), "Unmark should succeed: {:?}", result);

        // Check return data: should be 1 (was modified)
        let return_data = result.unwrap().return_data;
        assert_eq!(
            return_data.data.as_slice(),
            &[1u8],
            "Return data should indicate bit was modified"
        );

        svm.expire_blockhash();

        // Now marking again should succeed (bit was cleared)
        let ix = MarkUsed {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace,
            sequence,
        }
        .instruction();
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
            "Mark should succeed after unmark: {:?}",
            result
        );
    }

    #[test]
    fn unmark_on_already_clear_bit_returns_not_modified() {
        let mut svm = LiteSVM::new();
        svm.add_program(PROGRAM_ID, &load_program());

        let authority = Keypair::new();
        svm.airdrop(&authority.pubkey(), 10 * LAMPORTS_PER_SOL)
            .unwrap();

        let namespace = b"test";
        let sequence = 99u64;

        // Unmark without ever marking - should succeed with was_modified=false
        let ix = MarkUsed {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace,
            sequence,
        }
        .unmark_instruction();
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
            "Unmark on clear bit should succeed: {:?}",
            result
        );

        let return_data = result.unwrap().return_data;
        assert_eq!(
            return_data.data.as_slice(),
            &[0u8],
            "Return data should indicate bit was NOT modified"
        );
    }

    #[test]
    fn unmark_requires_authority_signature() {
        let mut svm = LiteSVM::new();
        svm.add_program(PROGRAM_ID, &load_program());

        let payer = Keypair::new();
        let authority = Keypair::new();
        svm.airdrop(&payer.pubkey(), 10 * LAMPORTS_PER_SOL).unwrap();

        let namespace = b"test";
        let sequence = 1u64;

        // Create instruction but mark authority as non-signer
        let (pda, _bump) = derive_bitmap_pda(&authority.pubkey(), namespace, sequence);
        let data = build_instruction_data(UNMARK_USED, namespace, sequence);

        let ix = SdkInstruction {
            program_id: PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(payer.pubkey(), true),
                AccountMeta::new_readonly(authority.pubkey(), false), // NOT a signer!
                AccountMeta::new(pda, false),
                AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            ],
            data,
        };

        let blockhash = svm.latest_blockhash();
        let tx =
            Transaction::new_signed_with_payer(&[ix], Some(&payer.pubkey()), &[&payer], blockhash);
        let result = svm.send_transaction(tx);
        assert!(result.is_err(), "Should fail when authority doesn't sign");
    }

    #[test]
    fn unmark_idempotent() {
        let mut svm = LiteSVM::new();
        svm.add_program(PROGRAM_ID, &load_program());

        let authority = Keypair::new();
        svm.airdrop(&authority.pubkey(), 10 * LAMPORTS_PER_SOL)
            .unwrap();

        let namespace = b"test";
        let sequence = 7u64;

        // Mark it
        let ix = MarkUsed {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace,
            sequence,
        }
        .instruction();
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[&authority],
            blockhash,
        );
        assert!(svm.send_transaction(tx).is_ok());

        svm.expire_blockhash();

        // Unmark it twice - both should succeed
        for expected_modified in [1u8, 0u8] {
            let ix = MarkUsed {
                payer: &authority.pubkey(),
                authority: &authority.pubkey(),
                namespace,
                sequence,
            }
            .unmark_instruction();
            let blockhash = svm.latest_blockhash();
            let tx = Transaction::new_signed_with_payer(
                &[ix],
                Some(&authority.pubkey()),
                &[&authority],
                blockhash,
            );
            let result = svm.send_transaction(tx);
            assert!(result.is_ok(), "Unmark should always succeed");

            let return_data = result.unwrap().return_data;
            assert_eq!(
                return_data.data.as_slice(),
                &[expected_modified],
                "Expected modified={} on iteration",
                expected_modified
            );

            svm.expire_blockhash();
        }
    }

    #[test]
    fn unmark_does_not_affect_other_bits() {
        let mut svm = LiteSVM::new();
        svm.add_program(PROGRAM_ID, &load_program());

        let authority = Keypair::new();
        svm.airdrop(&authority.pubkey(), 10 * LAMPORTS_PER_SOL)
            .unwrap();

        let namespace = b"test";

        // Mark sequences 10 and 11 (same bucket)
        for seq in [10u64, 11u64] {
            let ix = MarkUsed {
                payer: &authority.pubkey(),
                authority: &authority.pubkey(),
                namespace,
                sequence: seq,
            }
            .instruction();
            let blockhash = svm.latest_blockhash();
            let tx = Transaction::new_signed_with_payer(
                &[ix],
                Some(&authority.pubkey()),
                &[&authority],
                blockhash,
            );
            assert!(svm.send_transaction(tx).is_ok());
            svm.expire_blockhash();
        }

        // Unmark only sequence 10
        let ix = MarkUsed {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace,
            sequence: 10,
        }
        .unmark_instruction();
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[&authority],
            blockhash,
        );
        assert!(svm.send_transaction(tx).is_ok());

        svm.expire_blockhash();

        // Sequence 10 should be re-markable
        let ix = MarkUsed {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace,
            sequence: 10,
        }
        .instruction();
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[&authority],
            blockhash,
        );
        assert!(
            svm.send_transaction(tx).is_ok(),
            "Sequence 10 should be re-markable after unmark"
        );

        svm.expire_blockhash();

        // Sequence 11 should still be marked (replay fails)
        let ix = MarkUsed {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace,
            sequence: 11,
        }
        .instruction();
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[&authority],
            blockhash,
        );
        assert!(
            svm.send_transaction(tx).is_err(),
            "Sequence 11 should still be marked"
        );
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

        let ix = MarkUsed {
            payer: &payer.pubkey(),
            authority: &authority.pubkey(),
            namespace,
            sequence,
        }
        .instruction();

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

    // ============================================================================
    // MarkUsedBulk tests
    // ============================================================================

    /// Helper: send a `MarkUsedBulk` ix in a single-payer/authority transaction.
    #[allow(clippy::result_large_err)]
    fn send_mark_used_bulk(
        svm: &mut LiteSVM,
        authority: &Keypair,
        namespace: &[u8],
        bucket_index: u64,
        or_mask: &[u8; MARK_USED_BULK_MASK_LEN],
    ) -> Result<litesvm::types::TransactionMetadata, litesvm::types::FailedTransactionMetadata>
    {
        let ix = MarkUsedBulk {
            payer: &authority.pubkey(),
            authority: &authority.pubkey(),
            namespace,
            bucket_index,
            or_mask,
        }
        .instruction();
        let blockhash = svm.latest_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&authority.pubkey()),
            &[authority],
            blockhash,
        );
        svm.send_transaction(tx)
    }

    /// Build a 128-byte mask with the given bit indices set.
    fn mask_with_bits(bits: &[usize]) -> [u8; MARK_USED_BULK_MASK_LEN] {
        let mut mask = [0u8; MARK_USED_BULK_MASK_LEN];
        for &bit in bits {
            mask[bit / 8] |= 1 << (bit % 8);
        }
        mask
    }

    #[test]
    fn mark_used_bulk_or_masks_into_bucket() {
        let mut svm = LiteSVM::new();
        svm.add_program(PROGRAM_ID, &load_program());

        let authority = Keypair::new();
        svm.airdrop(&authority.pubkey(), 10 * LAMPORTS_PER_SOL)
            .unwrap();

        let namespace = b"test";
        let bucket_index = 7u64;
        let (pda, expected_bump) =
            derive_bitmap_pda_for_bucket(&authority.pubkey(), namespace, bucket_index);

        // First call: scattered bits, allocates the bucket.
        let mask1 = mask_with_bits(&[0, 5, 17, 200, 1023]);
        send_mark_used_bulk(&mut svm, &authority, namespace, bucket_index, &mask1)
            .expect("first MarkUsedBulk should succeed");

        let account = svm.get_account(&pda).expect("bucket PDA should exist");
        assert_eq!(
            account.data.len(),
            BITMAP_ACCOUNT_SIZE,
            "PDA must be allocated to canonical size"
        );
        assert_eq!(account.data[0], expected_bump, "bump byte mismatch");
        assert_eq!(&account.data[1..], &mask1, "bitmap must equal initial mask");

        svm.expire_blockhash();

        // Second call: overlapping and new bits.
        let mask2 = mask_with_bits(&[5, 6, 17, 18, 512]);
        send_mark_used_bulk(&mut svm, &authority, namespace, bucket_index, &mask2)
            .expect("second MarkUsedBulk should succeed");

        let account = svm.get_account(&pda).expect("bucket PDA should exist");
        let mut expected = [0u8; BITMAP_BYTES];
        for i in 0..BITMAP_BYTES {
            expected[i] = mask1[i] | mask2[i];
        }
        assert_eq!(account.data[0], expected_bump, "bump byte must persist");
        assert_eq!(
            &account.data[1..],
            &expected,
            "bitmap must be the OR of all masks applied"
        );
    }

    #[test]
    fn mark_used_bulk_cannot_clear_set_bits() {
        let mut svm = LiteSVM::new();
        svm.add_program(PROGRAM_ID, &load_program());

        let authority = Keypair::new();
        svm.airdrop(&authority.pubkey(), 10 * LAMPORTS_PER_SOL)
            .unwrap();

        let namespace = b"test";
        let bucket_index = 0u64;
        let (pda, _bump) =
            derive_bitmap_pda_for_bucket(&authority.pubkey(), namespace, bucket_index);

        // Set bit 5 only.
        let first_mask = mask_with_bits(&[5]);
        send_mark_used_bulk(&mut svm, &authority, namespace, bucket_index, &first_mask)
            .expect("first MarkUsedBulk should succeed");

        let account = svm.get_account(&pda).expect("bucket PDA should exist");
        assert_eq!(
            account.data[1] & 0b0010_0000,
            0b0010_0000,
            "bit 5 should be set after first call"
        );

        svm.expire_blockhash();

        // All-zero mask must not clear bit 5.
        let zero_mask = [0u8; MARK_USED_BULK_MASK_LEN];
        send_mark_used_bulk(&mut svm, &authority, namespace, bucket_index, &zero_mask)
            .expect("zero-mask MarkUsedBulk should succeed");

        let account = svm.get_account(&pda).expect("bucket PDA should exist");
        assert_eq!(
            account.data[1] & 0b0010_0000,
            0b0010_0000,
            "bit 5 must remain set after zero-mask call (OR-only invariant)"
        );
        // All other bits should still be zero.
        let mut expected = [0u8; BITMAP_BYTES];
        expected[0] = 0b0010_0000;
        assert_eq!(
            &account.data[1..],
            &expected,
            "no other bits should have flipped"
        );
    }
}
