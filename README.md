# Solana NoReplay

This program implements replay protection using a chunked bitmap.

account structure: PDA ( caller program, <arbitrary bytes>)

use case: emitter chain (u16), emitter address (32 bytes), sequence number (u64) -> bitmaps of N bits.

TODO:
- litesvm proptest
