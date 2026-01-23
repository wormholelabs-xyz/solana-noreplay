# Solana NoReplay

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

The sequence space is partitioned into fixed-size buckets of `BUCKET_SIZE` bits.

```
bucket_index = sequence / BUCKET_SIZE
bit_index    = sequence % BUCKET_SIZE
```

In implementations, `BUCKET_SIZE` is typically chosen as a power of two so this compiles to a shift and mask.

Each bucket is represented by a PDA seeded by:

- the **calling program’s address**
- the caller-supplied `namespace`
- `bucket_index` (little-endian `u64`)

The PDA stores a bitmap of `BUCKET_SIZE / 8` bytes. A sequence is considered *already processed* if and only if its corresponding bit is set.

### Requirements

For a given namespace, `sequence` must come from a dense or at least *locally dense* monotonic space. If identifiers are uniformly random (for example cryptographic hashes), bucketing provides no benefit because consecutive messages will never share a bucket.

If your protocol naturally produces random identifiers, you must introduce a monotonic sequence number (or equivalent) to make this scheme effective.

## Example: Wormhole VAAs

Wormhole VAAs provide a concrete example, but the scheme is not Wormhole-specific.

A natural namespace for VAAs is:

```
namespace = chain_id (u16, LE) || emitter_address (32 bytes)
```

The VAA `sequence` field is monotonic per emitter. Using this scheme:

- `bucket_index = sequence / BUCKET_SIZE`
- PDA seeds: `[calling_program_id, namespace, bucket_index_le]`

Consecutive VAAs from the same emitter therefore share bucket accounts, amortising storage costs across many messages.

## Storage and cost intuition

- _Standard approach_: ~128 bytes of overhead per consumed message (one PDA per message).
- _Bitmap approach_: 1 bit per message, plus amortised account overhead across `BUCKET_SIZE` messages.

For sufficiently large buckets, the marginal cost per message approaches a single bit, yielding roughly _100×–1000× storage savings_ compared to the one-PDA-per-message pattern (depending on bucket size and utilisation).

## CPI usage model

The NoReplay program is intended to be invoked via CPI by other programs.

The caller provides:

- `namespace: &[u8]` — deterministic, application-specific identifier
- `sequence: u64`
- the expected bucket PDA (derived from the same seeds)

The NoReplay program:

1. Computes `(bucket_index, bit_index)` from `sequence`
2. Initialises the bucket PDA if it does not yet exist (or takes ownership of a system-owned pre-funded account)
3. Checks the bitmap at `bit_index`
   - if the bit is set: reject as a replay
   - otherwise: set the bit and succeed

## Notes on seed and parameter design

- `BUCKET_SIZE` should be a power of two so bit arithmetic is cheap.
- Only `bucket_index` is included in the PDA derivation; _never_ include `bit_index`.
- `namespace` should be collision-resistant for your application:
  - include domain separators, chain IDs, contract addresses, emitter IDs, etc. as appropriate
  - respect Solana’s seed length limits (split into multiple seed components if needed)

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

For example, assume `BUCKET_SIZE = 1024 bits` and `$100/SOL`.
Then the cost of a single PDA per message is ~$0.089 (128 bytes for the account overhead).
The cost of a bucket (128 bytes bitmap + 128 bytes overhead) is ~$0.178.

- **100% hit rate**: ~512x cheaper than one-PDA-per-message (≈ $0.000174 / msg)
- **10% hit rate**: ~51x cheaper (≈ $0.00174 / msg)
- **1% hit rate**: ~5.1x cheaper (≈ $0.0174 / msg)
- **0.1% hit rate**: ~0.51x (≈ $0.174 / msg; worse than one-PDA-per-message)
