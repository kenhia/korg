# Setup

korg builds into a single `korg-api` binary that serves the web UI, the REST
API, and the MCP endpoint (`POST /mcp`) from one process backed by PostgreSQL.

## Prerequisites

- **Rust** 1.85+ (the workspace pins a toolchain via `rust-toolchain.toml`).
- **PostgreSQL** 14+ reachable via a connection URL.
- **Node.js** 24+ and **pnpm** 10+ (only needed to build the web UI).
- **Docker** — required for the test suite (tests spin up throwaway Postgres
  containers via `testcontainers`) and for container deployment.
- **[`just`](https://github.com/casey/just)** — optional, runs the task recipes
  in `justfile`.

## 1. Clone and build

```bash
git clone https://github.com/kenhia/korg.git
cd korg
cargo build --workspace        # or: just build
```

## 2. Provision a database

Point korg at any PostgreSQL instance. Schema migrations are embedded in the
binary and applied automatically on startup (`sqlx::migrate!`), so an empty
database is all you need:

```bash
createdb korg                  # or use an existing Postgres role/db
export DATABASE_URL=postgres://korg:korg@localhost:5432/korg
```

## 3. Build the web UI

```bash
cd web
pnpm install
pnpm build                     # emits web/build/
cd ..
```

## 4. Run korg-api

```bash
DATABASE_URL=postgres://korg:korg@localhost:5432/korg \
KORG_TIMEZONE=Etc/UTC \
KORG_WEB_DIR=$PWD/web/build \
KORG_LISTEN_ADDR=0.0.0.0:8090 \
  cargo run -p korg-api
```

Open `http://<host>:8090` for the UI, hit `http://<host>:8090/api/health` for a
health check, and point an MCP client at `http://<host>:8090/mcp`.

## Environment variables

| Variable           | Required | Default          | Purpose                                                   |
| ------------------ | -------- | ---------------- | --------------------------------------------------------- |
| `DATABASE_URL`     | yes      | —                | PostgreSQL connection string.                             |
| `KORG_TIMEZONE`    | yes      | —                | DST-aware IANA timezone used for daily lifecycle boundaries (for example `Etc/UTC`). Startup rejects missing/invalid values. |
| `KORG_LISTEN_ADDR` | no       | `0.0.0.0:8080`   | Address/port the server binds to.                         |
| `KORG_WEB_DIR`     | no       | —                | Path to the built SvelteKit bundle; UI is served when set.|
| `KORG_LOG`         | no       | `info`           | `tracing` env-filter (e.g. `korg_api=debug`).             |

## Running with Docker

The repository ships a multi-stage `Dockerfile` that builds the web bundle and
the release binary, then assembles a slim runtime image listening on `5674`:

```bash
docker build -t korg .
docker run --rm -p 5674:5674 \
  -e DATABASE_URL=postgres://korg:korg@host.docker.internal:5432/korg \
  -e KORG_TIMEZONE=Etc/UTC \
  korg
```

The image runs a `HEALTHCHECK` against `/api/health`.

## Development workflow

Hot-reload the UI against a running API instead of rebuilding the bundle:

```bash
# Terminal 1 — API
DATABASE_URL=... KORG_TIMEZONE=Etc/UTC cargo run -p korg-api

# Terminal 2 — UI dev server (proxies to the API)
cd web && KORG_API=http://localhost:8090 pnpm dev   # http://localhost:5173
```

### Generated files

`web/src/lib/generated/` and `crates/korg-mcp/tests/tools_schema.json` are
derived from korg-core and must never be hand-edited. After changing a shared
operation struct (`korg_core::ops`, or any `New*`/`*Patch` in `repo`/`topics`),
a response row, or a vocabulary:

```bash
just gen        # rewrites the TypeScript and the MCP tool-schema snapshot
```

The ts-rs export directory and its `i64 -> number` mapping live in
`.cargo/config.toml`, not in the recipe, so a plain `cargo test --workspace`
writes to the same place `just gen` does instead of leaving a stale copy in
`crates/korg-core/bindings/`.

Then read the diff — every line of the snapshot is a change agents see. `just
check` fails if the generated files are not what the generator currently
produces (it hashes them before and after regenerating, so it is indifferent to
whether you have committed yet):

```bash
just check      # fmt, gen freshness, clippy -D warnings, full test suite
```

## Tests

```bash
cargo test --workspace         # or: just test  (requires Docker)
```

The Rust integration tests provision disposable PostgreSQL containers, so Docker
must be running and the daemon reachable.

End-to-end UI tests (Playwright/Chromium) run against a live `korg-api`:

```bash
cd web
npx playwright install chromium                       # once
KORG_E2E_URL=http://127.0.0.1:8090 npx playwright test
```
