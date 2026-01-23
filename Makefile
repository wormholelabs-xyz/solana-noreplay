.PHONY: build test test-docker

build:
	cargo build-sbf --manifest-path program/Cargo.toml

test: build
	cd tests && cargo test

test-docker:
	docker build --platform linux/amd64 --target test -f .devcontainer/Dockerfile .
