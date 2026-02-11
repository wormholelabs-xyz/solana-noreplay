FROM node:20 AS solana-dev

ARG TZ
ENV TZ="$TZ"

# Install build dependencies for Rust/Solana
RUN apt-get update && apt-get install -y --no-install-recommends \
  build-essential \
  pkg-config \
  libssl-dev \
  libudev-dev \
  llvm \
  clang \
  cmake \
  curl \
  && apt-get clean && rm -rf /var/lib/apt/lists/*

# Create directories that will be volume-mounted (with correct ownership)
RUN mkdir -p /home/node/.cargo/registry /home/node/.cache/solana && \
  chown -R node:node /home/node/.cargo /home/node/.cache

USER node

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/home/node/.cargo/bin:$PATH"

# Install Solana CLI (agave) - version from .solana-version file
COPY .solana-version /tmp/.solana-version
RUN sh -c "$(curl -sSfL https://release.anza.xyz/v$(cat /tmp/.solana-version)/install)"
ENV PATH="/home/node/.local/share/solana/install/active_release/bin:$PATH"

# Pre-warm Solana SBF build cache
RUN mkdir -p /tmp/sbf-warmup/src

RUN cat >/tmp/sbf-warmup/Cargo.toml <<'EOF'
[package]
name = "sbf-warmup"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
EOF

RUN echo > /tmp/sbf-warmup/src/lib.rs

# Pre-warm Solana SBF toolchain
RUN --mount=type=cache,target=/home/node/.cache/solana,uid=1000,gid=1000 \
    cd /tmp/sbf-warmup && \
    cargo build-sbf && \
    rm -rf /tmp/sbf-warmup

# =============================================================================
# Build stage - builds the Solana program
# =============================================================================
FROM solana-dev AS solana-build

WORKDIR /workspace

COPY --chown=node:node Cargo.toml Cargo.lock* ./
COPY --chown=node:node program program
COPY --chown=node:node tests/Cargo.toml tests/Cargo.toml

# Dummy tests source to satisfy workspace
RUN mkdir -p tests/src tests/benches && \
    echo > tests/src/lib.rs && \
    echo 'fn main() {}' > tests/benches/compute_units.rs

RUN --mount=type=cache,target=/home/node/.cargo/registry,uid=1000,gid=1000 \
    --mount=type=cache,target=/home/node/.cache/solana,uid=1000,gid=1000 \
    --mount=type=cache,target=/workspace/target,uid=1000,gid=1000 \
    cargo build-sbf --manifest-path program/Cargo.toml && \
    mkdir -p /workspace/deploy && \
    cp /workspace/target/deploy/*.so /workspace/deploy/

# =============================================================================
# Test stage - runs tests against the built program
# =============================================================================
FROM solana-dev AS test

WORKDIR /workspace

ENV NOREPLAY_PROGRAM_ID=repMHgR5BEpGLeZvM5iGoNNDPw4eu2BS6sXJzaC8K4t

# Copy manifests and test code
COPY --chown=node:node Cargo.toml Cargo.lock* ./
COPY --chown=node:node program program
COPY --chown=node:node tests tests

# Copy built program from build stage
COPY --from=solana-build --chown=node:node /workspace/deploy target/deploy

# Run tests (clean cached solana-noreplay to ensure client feature is compiled)
RUN --mount=type=cache,target=/home/node/.cargo/registry,uid=1000,gid=1000 \
    --mount=type=cache,target=/workspace/target/debug,uid=1000,gid=1000 \
    cargo clean -p solana-noreplay --manifest-path tests/Cargo.toml 2>/dev/null || true && \
    cargo test --manifest-path tests/Cargo.toml

# =============================================================================
# Benchmark stage - runs compute unit benchmarks
# =============================================================================
FROM solana-dev AS bench

WORKDIR /workspace

ENV NOREPLAY_PROGRAM_ID=repMHgR5BEpGLeZvM5iGoNNDPw4eu2BS6sXJzaC8K4t

# Copy manifests and test/bench code
COPY --chown=node:node Cargo.toml Cargo.lock* ./
COPY --chown=node:node program program
COPY --chown=node:node tests tests

# Copy built program from build stage
COPY --from=solana-build --chown=node:node /workspace/deploy target/deploy

# Run benchmarks (clean cached solana-noreplay to ensure client feature is compiled)
RUN --mount=type=cache,target=/home/node/.cargo/registry,uid=1000,gid=1000 \
    --mount=type=cache,target=/workspace/target/release,uid=1000,gid=1000 \
    cargo clean -p solana-noreplay --manifest-path tests/Cargo.toml --release 2>/dev/null || true && \
    cargo bench --manifest-path tests/Cargo.toml && \
    cat target/benches/compute_units.md

# =============================================================================
# Development stage - adds Claude and other dev tools
# =============================================================================
FROM solana-dev AS devcontainer

USER root

ARG CLAUDE_CODE_VERSION=latest

# Install development tools
RUN apt-get update && apt-get install -y --no-install-recommends \
  less \
  git \
  procps \
  sudo \
  fzf \
  zsh \
  man-db \
  unzip \
  gnupg2 \
  gh \
  iptables \
  ipset \
  iproute2 \
  dnsutils \
  aggregate \
  jq \
  nano \
  vim \
  wget \
  && apt-get clean && rm -rf /var/lib/apt/lists/*

# Ensure default node user has access to /usr/local/share
RUN mkdir -p /usr/local/share/npm-global && \
  chown -R node:node /usr/local/share

ARG USERNAME=node

# Persist bash history
RUN SNIPPET="export PROMPT_COMMAND='history -a' && export HISTFILE=/commandhistory/.bash_history" \
  && mkdir /commandhistory \
  && touch /commandhistory/.bash_history \
  && chown -R $USERNAME /commandhistory

# Set `DEVCONTAINER` environment variable
ENV DEVCONTAINER=true

# Create workspace and config directories
RUN mkdir -p /workspace /home/node/.claude && \
  chown -R node:node /workspace /home/node/.claude

WORKDIR /workspace

ARG GIT_DELTA_VERSION=0.18.2
RUN ARCH=$(dpkg --print-architecture) && \
  wget "https://github.com/dandavison/delta/releases/download/${GIT_DELTA_VERSION}/git-delta_${GIT_DELTA_VERSION}_${ARCH}.deb" && \
  dpkg -i "git-delta_${GIT_DELTA_VERSION}_${ARCH}.deb" && \
  rm "git-delta_${GIT_DELTA_VERSION}_${ARCH}.deb"

# Copy and set up firewall script
COPY init-firewall.sh allowed-domains.txt /usr/local/bin/
RUN chmod +x /usr/local/bin/init-firewall.sh && \
  echo "node ALL=(root) NOPASSWD: /usr/local/bin/init-firewall.sh" > /etc/sudoers.d/node-firewall && \
  chmod 0440 /etc/sudoers.d/node-firewall

USER node

# Install global packages
ENV NPM_CONFIG_PREFIX=/usr/local/share/npm-global
ENV PATH=$PATH:/usr/local/share/npm-global/bin

# Set the default shell to zsh
ENV SHELL=/bin/zsh

# Set the default editor
ENV EDITOR nano
ENV VISUAL nano

# Setup zsh
ARG ZSH_IN_DOCKER_VERSION=1.2.0
RUN sh -c "$(wget -O- https://github.com/deluan/zsh-in-docker/releases/download/v${ZSH_IN_DOCKER_VERSION}/zsh-in-docker.sh)" -- \
  -p git \
  -p fzf \
  -a "source /usr/share/doc/fzf/examples/key-bindings.zsh" \
  -a "source /usr/share/doc/fzf/examples/completion.zsh" \
  -a "export PROMPT_COMMAND='history -a' && export HISTFILE=/commandhistory/.bash_history" \
  -a 'export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"' \
  -x

# Install Claude
RUN npm install -g @anthropic-ai/claude-code@${CLAUDE_CODE_VERSION}
