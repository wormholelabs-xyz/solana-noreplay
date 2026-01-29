/// Size of the bitmap in bytes (128 bytes = 1024 bits)
pub const BITMAP_BYTES: usize = 128;
/// Bits per bitmap bucket (derived from BITMAP_BYTES)
pub const BITS_PER_BUCKET: u64 = (BITMAP_BYTES * 8) as u64;
/// Total account size: [bump: u8][bitmap: 128 bytes] = 129 bytes
pub const BITMAP_ACCOUNT_SIZE: usize = 1 + BITMAP_BYTES;

/// Zero-copy wrapper for bitmap account data.
/// Layout: [bump: u8][bitmap: 128 bytes]
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
        let was_used = self.is_used(sequence);
        let bit_index = (sequence % BITS_PER_BUCKET) as usize;
        let byte_index = bit_index / 8;
        let bit_offset = bit_index % 8;
        self.bitmap[byte_index] |= 1 << bit_offset;
        was_used
    }
}
