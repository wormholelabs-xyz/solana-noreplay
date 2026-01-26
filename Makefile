.PHONY: build test test-docker bench bench-docker

build:
	cargo build-sbf --manifest-path program/Cargo.toml

test: build
	cd tests && cargo test

bench: build
	cd tests && cargo bench

test-docker:
	docker build --platform linux/amd64 --target test -f .devcontainer/Dockerfile .

bench-docker:
	docker build --platform linux/amd64 --target bench -f .devcontainer/Dockerfile .
