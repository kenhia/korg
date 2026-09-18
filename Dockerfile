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
# korg-api embeds the published read-shape contract with `include_str!`, so it is
# a BUILD input and not just a repo artefact — without it the release build fails
# at compile time with "couldn't read … contract/read-shapes.json". Nothing else
# catches that: `just check` and CI compile in the full tree, and only this stage
# sees a restricted copy set. `embedded_files_are_copied_into_the_image` in
# korg-mcp's docs_drift suite is the gate that does.
COPY contract/ ./contract/
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
# Image attachments (sprint 056). Set here rather than in the compose file for
# the same reason as the two above: the image declares its own in-container
# paths, and the compose file bind-mounts the host directory onto this one.
# korg-api's default matches this value, so the two cannot drift apart.
ENV KORG_IMG_ROOT=/data/images
# KORG_TIMEZONE is intentionally required at runtime; no geographic default is guessed.
EXPOSE 5674
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -fsS http://localhost:5674/api/health || exit 1
ENTRYPOINT ["/app/korg-api"]
