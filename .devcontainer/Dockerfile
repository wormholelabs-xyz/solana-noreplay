# Common base stage for downloading tools
FROM debian:bookworm-slim@sha256:4724b8cc51e33e398f0e2e15e18d5ec2851ff0c2280647e1310bc1642182655d AS downloader
RUN apt-get update && apt-get install -y --no-install-recommends \
  curl \
  unzip \
  bzip2 \
  git \
  ca-certificates \
  && rm -rf /var/lib/apt/lists/*

# Stage: Download and extract git-delta
FROM downloader AS git-delta
ARG GIT_DELTA_VERSION=0.18.2
ARG GIT_DELTA_CHECKSUM_AMD64=1658c7b61825d411b50734f34016101309e4b6e7f5799944cf8e4ac542cebd7f
ARG GIT_DELTA_CHECKSUM_ARM64=937781aa7788e1510858743fff6c9a8b4a69fe0a22a7c8a69493e633227939a9
RUN ARCH=$(dpkg --print-architecture) && \
  if [ "${ARCH}" = "amd64" ]; then \
    EXPECTED_CHECKSUM="${GIT_DELTA_CHECKSUM_AMD64}"; \
  else \
    EXPECTED_CHECKSUM="${GIT_DELTA_CHECKSUM_ARM64}"; \
  fi && \
  curl -fsSLo /tmp/git-delta.deb "https://github.com/dandavison/delta/releases/download/${GIT_DELTA_VERSION}/git-delta_${GIT_DELTA_VERSION}_${ARCH}.deb" && \
  echo "${EXPECTED_CHECKSUM}  /tmp/git-delta.deb" | sha256sum -c - && \
  dpkg-deb -x /tmp/git-delta.deb /opt/delta

# Stage: Download oh-my-zsh at pinned commit
FROM downloader AS oh-my-zsh
ARG OHMYZSH_COMMIT=c954bbb168fc645592c50017de0d0e138db8df5f
RUN git init /opt/ohmyzsh \
    && git -C /opt/ohmyzsh remote add origin https://github.com/ohmyzsh/ohmyzsh.git \
    && git -C /opt/ohmyzsh fetch --depth 1 origin ${OHMYZSH_COMMIT} \
    && git -C /opt/ohmyzsh checkout FETCH_HEAD \
    && rm -rf /opt/ohmyzsh/.git

# Stage: Download powerlevel10k at pinned commit (v1.20.0)
FROM downloader AS powerlevel10k
ARG POWERLEVEL10K_COMMIT=35833ea15f14b71dbcebc7e54c104d8d56ca5268
RUN git init /opt/powerlevel10k \
    && git -C /opt/powerlevel10k remote add origin https://github.com/romkatv/powerlevel10k.git \
    && git -C /opt/powerlevel10k fetch --depth 1 origin ${POWERLEVEL10K_COMMIT} \
    && git -C /opt/powerlevel10k checkout FETCH_HEAD \
    && rm -rf /opt/powerlevel10k/.git

# Stage: download and verify the castellan firewall binary
# Pre-built release from https://github.com/wormholelabs-xyz/castellan/releases.
# Get the checksums by downloading each tarball and running sha256sum on it.
FROM downloader AS castellan-download
ARG CASTELLAN_VERSION=0.2.0
ARG CASTELLAN_CHECKSUM_AMD64=85b112642d56d3faa1aba48905a48b4e842e74741580a1d716e7a7b185ec9156
ARG CASTELLAN_CHECKSUM_ARM64=ca1cce6df1adc128140cfab0a0fd3446b152ee5a533ee5d5ca3c9c90da0b0660
RUN ARCH=$(dpkg --print-architecture) && \
  if [ "${ARCH}" = "amd64" ]; then \
    TARBALL="castellan-linux-x86_64.tar.gz"; EXPECTED_CHECKSUM="${CASTELLAN_CHECKSUM_AMD64}"; \
  else \
    TARBALL="castellan-linux-aarch64.tar.gz"; EXPECTED_CHECKSUM="${CASTELLAN_CHECKSUM_ARM64}"; \
  fi && \
  curl -fsSLo /tmp/castellan.tar.gz \
    "https://github.com/wormholelabs-xyz/castellan/releases/download/v${CASTELLAN_VERSION}/${TARBALL}" && \
  echo "${EXPECTED_CHECKSUM}  /tmp/castellan.tar.gz" | sha256sum -c - && \
  mkdir -p /opt/castellan && \
  tar -xzf /tmp/castellan.tar.gz -C /opt/castellan castellan && \
  chmod +x /opt/castellan/castellan

# Stage: Download and verify the Claude Code native binary
# This mirrors what https://downloads.claude.ai/claude-code-releases/bootstrap.sh does,
# but pins the version and per-arch checksum instead of following "latest". The base
# image is glibc (Debian), so we use the linux-x64 / linux-arm64 (non-musl) builds.
# Refresh the checksums from the release manifest when bumping the version:
#   curl -fsSL https://downloads.claude.ai/claude-code-releases/${CLAUDE_CODE_VERSION}/manifest.json
FROM downloader AS claude-code
ARG CLAUDE_CODE_VERSION
ARG CLAUDE_CODE_CHECKSUM_AMD64
ARG CLAUDE_CODE_CHECKSUM_ARM64
RUN ARCH=$(dpkg --print-architecture) && \
  if [ "${ARCH}" = "amd64" ]; then \
    PLATFORM="linux-x64"; EXPECTED_CHECKSUM="${CLAUDE_CODE_CHECKSUM_AMD64}"; \
  else \
    PLATFORM="linux-arm64"; EXPECTED_CHECKSUM="${CLAUDE_CODE_CHECKSUM_ARM64}"; \
  fi && \
  curl -fsSLo /tmp/claude \
    "https://downloads.claude.ai/claude-code-releases/${CLAUDE_CODE_VERSION}/${PLATFORM}/claude" && \
  echo "${EXPECTED_CHECKSUM}  /tmp/claude" | sha256sum -c - && \
  mkdir -p /opt/claude && \
  mv /tmp/claude /opt/claude/claude && \
  chmod +x /opt/claude/claude

# Base stage for VS Code extension downloads — provides the download-vsix.sh helper.
FROM downloader AS vscode-downloader
COPY .devcontainer/download-vsix.sh /usr/local/bin/download-vsix.sh
RUN chmod +x /usr/local/bin/download-vsix.sh

# VS Code extension stages — one per extension so each version bump only busts its own cache layer.

# Stage: anthropic.claude-code
FROM vscode-downloader AS vscode-ext-claude-code
ARG CLAUDE_CODE_EXT_VERSION
ARG CLAUDE_CODE_EXT_SHA256
RUN download-vsix.sh anthropic claude-code "${CLAUDE_CODE_EXT_VERSION}" "${CLAUDE_CODE_EXT_SHA256}"

# Stage: dbaeumer.vscode-eslint
FROM vscode-downloader AS vscode-ext-eslint
ARG ESLINT_EXT_VERSION
ARG ESLINT_EXT_SHA256
RUN download-vsix.sh dbaeumer vscode-eslint "${ESLINT_EXT_VERSION}" "${ESLINT_EXT_SHA256}"

# Stage: esbenp.prettier-vscode
FROM vscode-downloader AS vscode-ext-prettier
ARG PRETTIER_EXT_VERSION
ARG PRETTIER_EXT_SHA256
RUN download-vsix.sh esbenp prettier-vscode "${PRETTIER_EXT_VERSION}" "${PRETTIER_EXT_SHA256}"

# Stage: eamodio.gitlens
FROM vscode-downloader AS vscode-ext-gitlens
ARG GITLENS_EXT_VERSION
ARG GITLENS_EXT_SHA256
RUN download-vsix.sh eamodio gitlens "${GITLENS_EXT_VERSION}" "${GITLENS_EXT_SHA256}"

# =============================================================================
# Stage: Download and verify the Solana CLI (agave)
# amd64 uses the official anza-xyz/agave release; anza doesn't publish a
# linux-aarch64 build, so arm64 uses the wormholelabs-xyz/agave fork's build for
# that target instead. Both are checksum-verified. The version comes from
# .solana-version (single source of truth). Regenerate the checksums when bumping
# it by downloading each tarball and running sha256sum on it.
# =============================================================================
FROM downloader AS solana
ARG SOLANA_FALLBACK_REPO=wormholelabs-xyz/agave
ARG SOLANA_CHECKSUM_AMD64=06f97c065cc977cbec2f13ffc9bc9d3b92fef485431fcb370a269de69532ef51
ARG SOLANA_CHECKSUM_ARM64=1c226ddf2066d5e19c5ca1370b168269ea7ee9923a4c759093be63b7699d4e8c
COPY .solana-version /tmp/.solana-version
RUN SOLANA_VERSION="$(cat /tmp/.solana-version)" && \
  ARCH=$(dpkg --print-architecture) && \
  if [ "${ARCH}" = "amd64" ]; then \
    URL="https://release.anza.xyz/v${SOLANA_VERSION}/solana-release-x86_64-unknown-linux-gnu.tar.bz2"; \
    CHECKSUM="${SOLANA_CHECKSUM_AMD64}"; \
  else \
    URL="https://github.com/${SOLANA_FALLBACK_REPO}/releases/download/v${SOLANA_VERSION}/solana-release-aarch64-unknown-linux-gnu.tar.bz2"; \
    CHECKSUM="${SOLANA_CHECKSUM_ARM64}"; \
  fi && \
  curl -fsSLo /tmp/solana-release.tar.bz2 "${URL}" && \
  echo "${CHECKSUM}  /tmp/solana-release.tar.bz2" | sha256sum -c - && \
  mkdir -p /opt/solana && \
  tar -xjf /tmp/solana-release.tar.bz2 -C /opt/solana

# =============================================================================
# solana-dev: base stage with the Rust + Solana (agave) toolchain
# =============================================================================
FROM node:24@sha256:bb20cf73b3ad7212834ec48e2174cdcb5775f6550510a5336b842ae32741ce6c AS solana-dev

ARG TZ
ENV TZ="$TZ"

# Set the locale
ENV LC_ALL=C.UTF-8 LANG=C.UTF-8

# Install build dependencies for Rust/Solana
RUN DEBIAN_FRONTEND=noninteractive apt-get update && apt-get install -y --no-install-recommends \
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

# Install Solana CLI (agave) from the checksum-verified `solana` download stage.
# Lay it out in the release layout the anza installer uses (versioned release dir
# + active_release symlink), with the version read from .solana-version.
COPY --from=solana --chown=node:node /opt/solana/solana-release /tmp/solana-release
COPY --chown=node:node .solana-version /tmp/.solana-version
RUN SOLANA_VERSION="$(cat /tmp/.solana-version)" && \
  mkdir -p "/home/node/.local/share/solana/install/releases/${SOLANA_VERSION}" && \
  mv /tmp/solana-release "/home/node/.local/share/solana/install/releases/${SOLANA_VERSION}/solana-release" && \
  ln -sf "/home/node/.local/share/solana/install/releases/${SOLANA_VERSION}/solana-release" \
    /home/node/.local/share/solana/install/active_release
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
# Development stage - adds Claude Code and other interactive dev tools
# =============================================================================
FROM solana-dev AS devcontainer

USER root

ARG CLAUDE_CODE_VERSION

# Install development tools and nftables (used by the castellan firewall)
RUN DEBIAN_FRONTEND=noninteractive apt-get update && apt-get install -y --no-install-recommends \
  less \
  procps \
  sudo \
  fzf \
  zsh \
  man-db \
  unzip \
  gnupg2 \
  gh \
  iptables \
  nftables \
  iproute2 \
  dnsutils \
  jq \
  nano \
  vim \
  && apt-get clean && rm -rf /var/lib/apt/lists/*

# Copy git-delta binary from build stage
COPY --from=git-delta /opt/delta/usr/bin/delta /usr/local/bin/delta

# Ensure default node user has access to /usr/local/share/npm-global
RUN mkdir -p /usr/local/share/npm-global && \
  chown -R node:node /usr/local/share/npm-global

ARG USERNAME=node

# Persist bash history
RUN SNIPPET="export PROMPT_COMMAND='history -a' && export HISTFILE=/commandhistory/.bash_history" \
  && mkdir /commandhistory \
  && touch /commandhistory/.bash_history \
  && chown -R $USERNAME /commandhistory

# Set `DEVCONTAINER` environment variable to help with orientation
ENV DEVCONTAINER=true

# Create workspace and config directories and set permissions
RUN mkdir -p /workspace /home/node/.claude && \
  chown -R node:node /workspace /home/node/.claude

WORKDIR /workspace

# Set up non-root user
USER node

# Install global packages
ENV NPM_CONFIG_PREFIX=/usr/local/share/npm-global
ENV PATH=$PATH:/usr/local/share/npm-global/bin

# Set the default shell to zsh rather than sh
ENV SHELL=/bin/zsh

# Set the default editor and visual
ENV EDITOR=nano
ENV VISUAL=nano

# Install oh-my-zsh and powerlevel10k theme from pinned stages
COPY --chown=node:node --from=oh-my-zsh /opt/ohmyzsh /home/node/.oh-my-zsh
COPY --chown=node:node --from=powerlevel10k /opt/powerlevel10k /home/node/.oh-my-zsh/custom/themes/powerlevel10k
COPY --chown=node:node .devcontainer/node-zshrc /home/node/.zshrc

# Install Claude Code from the SHA-verified native binary. It is a single self-contained
# executable, so we drop it straight onto PATH (root-owned, non-writable by node) rather
# than running `claude install`, which would set up a user-writable auto-updating launcher.
COPY --from=claude-code /opt/claude/claude /usr/local/bin/claude

# Install the castellan firewall — binary from the download stage, scripts and allow-list
# from .devcontainer. The allow-list is baked in at the path castellan reads by default
# (/usr/local/share/castellan/allowed-domains.txt); edits require a devcontainer rebuild.
#
# Security: the sudoers grant below gives effective root to anyone who can write
# init-firewall.sh, so it (and the supervisor and the full /usr/local/share/castellan
# chain) must stay root-owned and non-writable. The root COPYs already ensure this.
USER root
COPY --from=castellan-download /opt/castellan/castellan /usr/local/bin/castellan
COPY .devcontainer/init-firewall.sh .devcontainer/castellan-supervisor.sh /usr/local/bin/
RUN mkdir -p /usr/local/share/castellan
COPY .devcontainer/allowed-domains.txt /usr/local/share/castellan/allowed-domains.txt
RUN chmod +x /usr/local/bin/init-firewall.sh /usr/local/bin/castellan-supervisor.sh && \
  echo "node ALL=(root) NOPASSWD: /usr/local/bin/init-firewall.sh" > /etc/sudoers.d/node-firewall && \
  chmod 0440 /etc/sudoers.d/node-firewall

# VS Code extensions (baked in for version pinning; installed at folderOpen via .vscode/tasks.json)
RUN mkdir -p /opt/vscode-extensions
COPY --from=vscode-ext-claude-code /opt/ext /opt/vscode-extensions/
COPY --from=vscode-ext-eslint      /opt/ext /opt/vscode-extensions/
COPY --from=vscode-ext-prettier    /opt/ext /opt/vscode-extensions/
COPY --from=vscode-ext-gitlens     /opt/ext /opt/vscode-extensions/

USER node
