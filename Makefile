SOLANA_VERSION := $(shell cat .solana-version)
INSTALLED_VERSION := $(shell solana --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)

.PHONY: build build-verifiable test test-docker bench bench-docker setup check-version check-network deploy program-info

build: check-version
	cargo build-sbf --manifest-path program/Cargo.toml

setup:
	sh -c "$$(curl -sSfL https://release.anza.xyz/v$(SOLANA_VERSION)/install)"

check-version:
	@if [ "$(INSTALLED_VERSION)" != "$(SOLANA_VERSION)" ]; then \
		echo "Error: Solana version mismatch"; \
		echo "  Required: $(SOLANA_VERSION)"; \
		echo "  Installed: $(INSTALLED_VERSION)"; \
		echo "Run 'make setup' to install the correct version"; \
		exit 1; \
	fi

test: build
	cd tests && cargo test

bench: build
	cd tests && cargo bench

test-docker:
	docker build --platform linux/amd64 --target test -f .devcontainer/Dockerfile .

bench-docker:
	docker build --platform linux/amd64 --target bench -f .devcontainer/Dockerfile .

build-verifiable: check-version
	solana-verify build --library-name solana_noreplay \
		--base-image solanafoundation/solana-verifiable-build:3.0.7

deploy: check-network build-verifiable
	SOLANA_CLUSTER=$(NETWORK) ./scripts/deploy.sh

check-network:
	@test -n "$(NETWORK)" || { echo "ERROR: NETWORK is not set. Use NETWORK=devnet or NETWORK=mainnet"; exit 1; }

program-info:
	@test -n "$${PROGRAM_ID:-}" || { echo "ERROR: PROGRAM_ID env var is required"; exit 1; }
	solana program show $${PROGRAM_ID} -u $${SOLANA_RPC_URL:-https://api.devnet.solana.com}
