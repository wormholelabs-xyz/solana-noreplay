.PHONY: test-docker build-docker

test-docker:
	docker build --platform linux/amd64 --target test -f .devcontainer/Dockerfile -t solana-noreplay-test .
	docker run --rm solana-noreplay-test

build-docker:
	docker build --platform linux/amd64 --target test -f .devcontainer/Dockerfile .
