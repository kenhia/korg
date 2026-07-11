# korg — single image serving web UI + REST API + MCP (POST /mcp).
# Multi-stage: build the SvelteKit static bundle and the korg-api release
# binary, then assemble a slim runtime. Target arch matches the build host
# (kubsdb is linux/amd64, same as the dev host — no cross-compile).

# --- Stage 1: web bundle ------------------------------------------------------
FROM node:24-bookworm-slim AS web
WORKDIR /web
RUN corepack enable
COPY web/package.json web/pnpm-lock.yaml ./
RUN corepack prepare pnpm@10.33.2 --activate && pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

# --- Stage 2: rust release ----------------------------------------------------
FROM rust:1-bookworm AS rust
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ ./crates/
RUN cargo build --release -p korg-api

# --- Stage 3: runtime ---------------------------------------------------------
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=rust /src/target/release/korg-api /app/korg-api
COPY --from=web /web/build /app/web/build
ENV KORG_WEB_DIR=/app/web/build
ENV KORG_LISTEN_ADDR=0.0.0.0:5674
# KORG_TIMEZONE is intentionally required at runtime; no geographic default is guessed.
EXPOSE 5674
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -fsS http://localhost:5674/api/health || exit 1
ENTRYPOINT ["/app/korg-api"]
