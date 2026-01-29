use pinocchio::{default_panic_handler, no_allocator, program_entrypoint};

pub mod instruction;
pub mod pda;
pub mod processor;
pub mod state;

#[cfg(feature = "client")]
pub mod client;

// Re-exports for convenience
pub use instruction::{CreateBitmap, InstructionData, MarkUsed, CREATE_BITMAP, MARK_USED};
pub use pda::{derive_bitmap_pda, BitmapPdaSeeds, DerivePdaError};
pub use state::{BitmapAccount, BITMAP_ACCOUNT_SIZE, BITMAP_BYTES, BITS_PER_BUCKET};

/// Maximum namespace length (2 chunks * 32 bytes = 64 bytes)
/// Seeds: [authority (32), ns_chunk_0, ns_chunk_1, bucket_index (8)]
pub const MAX_NAMESPACE_LEN: usize = 64;

program_entrypoint!(processor::process_instruction);
no_allocator!();
default_panic_handler!();
