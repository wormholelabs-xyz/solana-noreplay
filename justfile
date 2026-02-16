set shell := ["bash", "-euo", "pipefail", "-c"]

# Build the solana-noreplay program
build: check-version
    cargo build-sbf --manifest-path program/Cargo.toml --no-default-features

# Install Solana CLI from .solana-version
setup:
    sh -c "$(curl -sSfL https://release.anza.xyz/v$(cat .solana-version)/install)"

# Validate installed Solana CLI version matches .solana-version
[no-exit-message]
check-version:
    #!/usr/bin/env bash
    set -euo pipefail
    required=$(cat .solana-version)
    installed=$(solana --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || echo "none")
    if [ "$installed" != "$required" ]; then
        echo "Error: Solana version mismatch"
        echo "  Required: $required"
        echo "  Installed: $installed"
        echo "Run 'just setup' to install the correct version"
        exit 1
    fi

# Run tests
test: build
    NOREPLAY_PROGRAM_ID=repMHgR5BEpGLeZvM5iGoNNDPw4eu2BS6sXJzaC8K4t \
    cargo test --manifest-path tests/Cargo.toml

# Run benchmarks
bench: build
    NOREPLAY_PROGRAM_ID=repMHgR5BEpGLeZvM5iGoNNDPw4eu2BS6sXJzaC8K4t \
    cargo bench --manifest-path tests/Cargo.toml

# Run tests in Docker
test-docker:
    docker build --platform linux/amd64 --target test -f .devcontainer/Dockerfile .

# Run benchmarks in Docker
bench-docker:
    docker build --platform linux/amd64 --target bench -f .devcontainer/Dockerfile .

# Deterministic Docker build for deployment
build-verifiable: check-version
    solana-verify build --library-name solana_noreplay \
        --base-image solanafoundation/solana-verifiable-build:3.0.7

# Deploy to Solana cluster
deploy nw:
    SOLANA_CLUSTER={{ nw }} ./scripts/deploy.sh

# Show on-chain program info
program-info program_id:
    solana program show {{ program_id }} -u ${SOLANA_RPC_URL:-https://api.devnet.solana.com}
