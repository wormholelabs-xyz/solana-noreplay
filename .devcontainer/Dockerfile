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

# Install Solana CLI (agave)
ARG SOLANA_VERSION=3.1.7
RUN sh -c "$(curl -sSfL https://release.anza.xyz/v${SOLANA_VERSION}/install)"
ENV PATH="/home/node/.local/share/solana/install/active_release/bin:$PATH"

# =============================================================================
# Test stage - builds and tests the program
# =============================================================================
FROM solana-dev AS test

WORKDIR /workspace

COPY --chown=node:node program/ program/
COPY --chown=node:node Cargo.toml Cargo.lock* ./

RUN cargo build-sbf

# Run tests
CMD ["cargo", "test-sbf"]

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
