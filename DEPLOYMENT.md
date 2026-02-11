# Deploying solana-noreplay

## Prerequisites

- Solana CLI (`make setup` to install the correct version)
- `solana-verify` (`cargo install solana-verify`) for verifiable builds
- Docker (required by `solana-verify`)

## 1. Generate keys

Program keypair (determines the on-chain address, only needed once):

```bash
solana-keygen grind --starts-with rep:1
mkdir -p keys && mv rep<...>.json keys/
```

Note the pubkey (filename without `.json`) — this is your `PROGRAM_ID`.

Deployer/payer keypair:

```bash
solana-keygen new -o keys/payer.json
```

Fund it on https://faucet.solana.com

## 2. Build

Standard build:

```bash
make build
```

Verifiable build (deterministic, recommended for shared deployments):

```bash
make build-verifiable
```

## 3. Deploy

Initial deploy (needs program keypair):

```bash
PROGRAM_ID=<pubkey> PROGRAM_KEYPAIR=keys/<pubkey>.json PAYER=keys/payer.json make deploy
```

Upgrade (program keypair not needed):

```bash
PROGRAM_ID=<pubkey> PAYER=keys/payer.json make deploy
```

## 4. Verify

```bash
PROGRAM_ID=<pubkey> make program-info
```

## 5. Building consumers

Programs that CPI into solana-noreplay (like xrpl-sequencer) need the
program address at compile time:

```bash
NOREPLAY_PROGRAM_ID=<pubkey> cargo build-sbf
```
