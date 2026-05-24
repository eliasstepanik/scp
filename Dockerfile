# Multi-stage Dockerfile for SCP Hub and CLI
# Stage 1: Chef (dependency caching)
FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

# Stage 2: Planner (prepare recipe)
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Builder (compile dependencies and application)
FROM chef AS builder
WORKDIR /app
COPY --from=planner /app/recipe.json recipe.json

# Build dependencies (cached layer)
RUN cargo chef cook --release --recipe-path recipe.json

# Copy source code
COPY . .

# Build the release binaries
RUN cargo build --release -p scp-hub -p scp-cli

# Stage 4: Runtime (minimal image)
# Ubuntu 24.04 ships glibc 2.39, matching the GitHub Actions ubuntu-24.04 build runner.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

# Install runtime dependencies + Node.js for stdio MCP backends
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && npm install -g \
        @modelcontextprotocol/server-sequential-thinking \
        @upstash/context7-mcp \
    && apt-get clean && rm -rf /var/lib/apt/lists/*

# Create non-root user (Ubuntu 24.04 reserves UID 1000 for the 'ubuntu' user, so use 1001)
RUN useradd -m -u 1001 scp

# Copy binaries from builder
COPY --from=builder /app/target/release/scp-hub /usr/local/bin/scp-hub
COPY --from=builder /app/target/release/scp-cli /usr/local/bin/scp-cli

# Create config directory
RUN mkdir -p /etc/scp && chown -R scp:scp /etc/scp /usr/local/bin/scp-hub /usr/local/bin/scp-cli

# Switch to non-root user
USER scp

# Expose ports
EXPOSE 3100 3101

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
    CMD curl -f http://localhost:3101/health || exit 1

# Default entrypoint and command
ENTRYPOINT ["scp-hub"]
CMD ["--config", "/etc/scp/scp.toml"]
