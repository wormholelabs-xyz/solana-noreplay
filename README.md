# Solana NoReplay

```
devnet address: repMHgR5BEpGLeZvM5iGoNNDPw4eu2BS6sXJzaC8K4t
```

A generic on-chain replay-protection primitive for Solana that amortises account overhead by tracking many "already processed" items in a single PDA-backed bitmap.

## Why?

Many Solana programs require _non-repeatability_: once an action has been processed, it must not be accepted again (for example consuming a cross-chain message, redeeming a voucher, or executing a claim).

The most common pattern is _one PDA per message_: derive a deterministic address from some stable identifier of the action, then create an account at that address to mark it as consumed.

This approach is simple but expensive. Creating an account incurs roughly **128 bytes of storage overhead** even if you only need to represent a single bit. At around $100/SOL, the rent-exempt deposit for a minimal account is on the order of ~$0.09 per message.

See: [ACCOUNT_STORAGE_OVERHEAD][account-overhead].

[account-overhead]: https://docs.rs/solana-rent/latest/solana_rent/constant.ACCOUNT_STORAGE_OVERHEAD.html

## How?

Instead of one account per message, this program packs many consumption bits into a single account.

A message is identified by two components:

- **Namespace**: an arbitrary, deterministic byte prefix supplied by the caller program
- **Sequence**: a monotonically increasing `u64` within that namespace

The sequence space is partitioned into fixed-size buckets of `BITS_PER_BUCKET` bits (currently 1024, derived from `BITMAP_BYTES = 128`).

```
bucket_index = sequence / BITS_PER_BUCKET
bit_index    = sequence % BITS_PER_BUCKET
```

`BITS_PER_BUCKET` is a power of two so this compiles to a shift and mask.

Each bucket is represented by a PDA seeded by:

- the **authority** (must be a signer; in CPI contexts, typically a PDA of the calling program)
- the caller-supplied `namespace` (split into 32-byte chunks if longer than 32 bytes)
- `bucket_index` (little-endian `u64`)

The PDA stores a bitmap of `BITMAP_BYTES` (128 bytes = 1024 bits), plus a 1-byte bump seed for efficient PDA verification. Total account size is 129 bytes. A sequence is considered *already processed* if and only if its corresponding bit is set.

### Requirements

For a given namespace, `sequence` must come from a dense or at least *locally dense* monotonic space. If identifiers are uniformly random (for example cryptographic hashes), bucketing provides no benefit because consecutive messages will never share a bucket.

If your protocol naturally produces random identifiers, you must introduce a monotonic sequence number (or equivalent) to make this scheme effective.

## Example: Wormhole VAAs

Wormhole VAAs provide a concrete example, but the scheme is not Wormhole-specific.

A natural namespace for VAAs is:

```
namespace = chain_id (u16, LE) || emitter_address (32 bytes)
```

This 34-byte namespace is automatically split into two seed components:
- chunk 0: bytes 0–31 (32 bytes)
- chunk 1: bytes 32–33 (2 bytes)

The VAA `sequence` field is monotonic per emitter. Using this scheme:

- `bucket_index = sequence / BITS_PER_BUCKET`
- PDA seeds: `[authority, ns_chunk_0, ns_chunk_1, bucket_index_le]`

Consecutive VAAs from the same emitter therefore share bucket accounts, amortising storage costs across many messages.

## Storage and cost intuition

- _Standard approach_: ~128 bytes of overhead per consumed message (one PDA per message).
- _Bitmap approach_: 1 bit per message, plus amortised account overhead across `BITS_PER_BUCKET` messages.

With `BITS_PER_BUCKET = 1024` and a 129-byte account (1 bump + 128 bitmap), the marginal cost per message approaches a single bit when buckets are well-utilised.

## CPI usage model

The NoReplay program is intended to be invoked via CPI by other programs.

### Accounts

1. `[signer, writable]` **Payer** — pays for PDA creation if needed
2. `[signer]` **Authority** — goes into PDA seeds; must sign for `MarkUsed` (not required for `CreateBitmap`)
3. `[writable]` **Bitmap PDA** — the bucket account (derived from authority, namespace, bucket_index)
4. `[]` **System program**

In CPI contexts, the **authority** is typically a PDA of the calling program (which the calling program can sign for). This ensures that only the calling program can mark sequences as used within its namespace.

### Instructions

The program supports two instructions:

#### CreateBitmap (discriminator = 0)

Permissionlessly creates a bitmap PDA. Anyone can call this to pre-create and fund bucket accounts, reducing compute and cost for the authority when they later call `MarkUsed`. Authority does **not** need to sign.

#### MarkUsed (discriminator = 1)

Marks a sequence number as used. Authority **must** sign to prevent DOS attacks where adversaries mark sequences as used for other users.

### Instruction data format

```
[discriminator: u8][namespace_len: u16 LE][namespace: 0-64 bytes][sequence: u64 LE]
```

- `discriminator`: 0 for CreateBitmap, 1 for MarkUsed
- `namespace`: deterministic, application-specific identifier (max 64 bytes)
- `sequence`: the sequence number to mark/create bucket for

### MarkUsed behaviour

1. Verifies the authority is a signer
2. Computes `(bucket_index, bit_index)` from `sequence`
3. Derives PDA from `[authority, ns_chunk_0, ns_chunk_1, bucket_index_le]`
4. Initialises the bucket PDA if it does not yet exist (or takes ownership of a system-owned pre-funded account)
5. Checks the bitmap at `bit_index`
   - if the bit is set: reject as a replay
   - otherwise: set the bit and succeed

## Notes on seed and parameter design

- `BITS_PER_BUCKET` is a power of two (256) so bit arithmetic is cheap.
- Only `bucket_index` is included in the PDA derivation; _never_ include `bit_index`.
- The **authority must be a signer for MarkUsed** to prevent DOS attacks where adversaries mark sequences as used for other users. CreateBitmap is permissionless.
- The bump seed is stored in the account (first byte) to avoid re-derivation on subsequent calls.
- `namespace` should be collision-resistant for your application:
  - include domain separators, chain IDs, contract addresses, emitter IDs, etc. as appropriate
  - namespaces longer than 32 bytes are automatically split into 32-byte chunks (max 64 bytes total = 2 chunks)

This design deliberately separates *how replay protection is implemented* from *how messages are identified*, allowing different protocols to reuse the same NoReplay primitive with their own namespace and sequencing schemes.

## When not to use this

This approach is not universally appropriate. You should _not_ use this scheme if:

- _Identifiers are uniformly random_ (e.g. hashes) and you cannot introduce a monotonic sequence.
- _Message volume is extremely low_, where the simplicity of one-PDA-per-message outweighs the fixed overhead of bitmap buckets.
- _Strict total ordering with permanent gaps is unacceptable_, unless your application defines a clear policy for missing or skipped sequences.
- _Unbounded worst-case sparsity_ is expected and storage growth must be strictly proportional to the number of messages; in that case an interval/RLE-based scheme may be more appropriate.
- _Account count must be minimised at all costs_ and you are willing to accept realloc complexity instead of multiple bucket PDAs.

## Notes on sparsity

In practice, message streams are often sparse even when sequence numbers are incremental.

For example, in Wormhole a given emitter may publish messages destined for many chains. A Solana consumer contract will only ever observe the subset of messages routed to Solana, leaving permanent gaps in the observed sequence space.

This scheme tolerates such sparsity, but sparsity affects the *amortised* cost per processed message.

For example, assume `BITS_PER_BUCKET = 1024` and `$100/SOL`.
Then the cost of a single PDA per message is ~$0.089 (128 bytes for the account overhead).
The cost of a bucket (129 bytes data + 128 bytes overhead = 257 bytes) is ~$0.179.

- **100% hit rate**: ~509x cheaper than one-PDA-per-message (≈ $0.000175 / msg)
- **10% hit rate**: ~51x cheaper (≈ $0.00175 / msg)
- **1% hit rate**: ~5x cheaper (≈ $0.0175 / msg)
- **0.2% hit rate**: break-even with one-PDA-per-message
