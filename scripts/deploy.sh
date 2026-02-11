#!/usr/bin/env bash
set -euo pipefail

# Required: PROGRAM_ID (base58 pubkey) — looks for PROGRAM_ID.json as keypair
#           PAYER (path to fee-payer keypair)
# Optional: PROGRAM_KEYPAIR (override program keypair path)
#           SOLANA_CLUSTER (default: devnet)
#           SOLANA_RPC_URL (override RPC)

if [ -z "${PROGRAM_ID:-}" ]; then
    echo "ERROR: PROGRAM_ID env var is required (base58 program address)"
    exit 1
fi

if [ -z "${PAYER:-}" ]; then
    echo "ERROR: PAYER env var is required (path to fee-payer keypair)"
    exit 1
fi

if [ ! -f "$PAYER" ]; then
    echo "ERROR: Payer keypair not found at $PAYER"
    exit 1
fi

PROGRAM_SO="target/deploy/solana_noreplay.so"
PROGRAM_KEYPAIR="${PROGRAM_KEYPAIR:-${PROGRAM_ID}.json}"
CLUSTER="${SOLANA_CLUSTER:-devnet}"

# Resolve RPC URL
if [ -n "${SOLANA_RPC_URL:-}" ]; then
    RPC_URL="$SOLANA_RPC_URL"
else
    case "$CLUSTER" in
        devnet)     RPC_URL="https://api.devnet.solana.com" ;;
        testnet)    RPC_URL="https://api.testnet.solana.com" ;;
        mainnet*)   RPC_URL="https://api.mainnet-beta.solana.com" ;;
        localhost)  RPC_URL="http://localhost:8899" ;;
        *)          echo "Unknown cluster: $CLUSTER"; exit 1 ;;
    esac
fi

SOLANA_FLAGS=(-u "$RPC_URL" --use-rpc --keypair "$PAYER")

# Pre-flight checks
if [ ! -f "$PROGRAM_SO" ]; then
    echo "ERROR: $PROGRAM_SO not found. Run 'make build' first."
    exit 1
fi

DEPLOYER=$(solana-keygen pubkey "$PAYER")
BALANCE=$(solana balance "$DEPLOYER" "${SOLANA_FLAGS[@]}" 2>/dev/null || echo "unknown")

echo "Cluster:    $CLUSTER ($RPC_URL)"
echo "Program ID: $PROGRAM_ID"
echo "Deployer:   $DEPLOYER"
echo "Balance:    $BALANCE"
echo ""

# Mainnet guard
if [[ "$CLUSTER" == mainnet* ]]; then
    read -r -p "WARNING: deploying to MAINNET. Type 'yes' to continue: " confirm
    if [ "$confirm" != "yes" ]; then
        echo "Aborted."
        exit 1
    fi
fi

# Detect initial deploy vs upgrade
if solana program show "$PROGRAM_ID" "${SOLANA_FLAGS[@]}" &>/dev/null; then
    echo "Program exists on-chain, upgrading..."
    solana program deploy \
        "${SOLANA_FLAGS[@]}" \
        --program-id "$PROGRAM_ID" \
        "$PROGRAM_SO"
else
    if [ ! -f "$PROGRAM_KEYPAIR" ]; then
        echo "ERROR: Initial deploy requires keypair at $PROGRAM_KEYPAIR"
        echo "       (set PROGRAM_KEYPAIR to override)"
        exit 1
    fi
    echo "Initial deployment..."
    solana program deploy \
        "${SOLANA_FLAGS[@]}" \
        --program-id "$PROGRAM_KEYPAIR" \
        "$PROGRAM_SO"
fi

echo ""
echo "Program ID: $PROGRAM_ID"
echo ""
echo "Set this when building consumers:"
echo "  NOREPLAY_PROGRAM_ID=$PROGRAM_ID"
