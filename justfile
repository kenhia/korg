# korg — task runner

# Build the whole workspace.
build:
    cargo build --workspace

# Run the full test suite.
test:
    cargo test --workspace

# Regenerate every derived artefact from korg-core: the TypeScript the web app
# imports (ts-rs + the vocabularies), and the MCP tool-schema snapshot. Run this
# after changing any shared operation struct, response row, or vocabulary — CI
# and `just check` fail if the checked-in output is stale (WI #540/#541).
gen:
    # The ts-rs export directory and integer mapping live in .cargo/config.toml
    # so that a plain `cargo test` writes to the same place this does.
    cargo test -p korg-core --lib export_bindings
    UPDATE_SCHEMA_SNAPSHOT=1 cargo test -p korg-mcp --test schema

# Everything CI enforces, in the order that fails fastest.
check: fmt-check
    just gen
    git diff --exit-code -- web/src/lib/generated crates/korg-mcp/tests/tools_schema.json
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

fmt-check:
    cargo fmt --all --check

# Snapshot the frozen kwi + kcard source databases (read-only). [S3]
# Requires KCARD_DATABASE_URL (and optionally KWI_DATABASE_URL) in the env.
snapshot:
    bash scripts/snapshot.sh

# Run the kwi+kcard -> korg import and verify fidelity invariants F1-F7. [S6]
# Prerequisite: `just snapshot` (produces snapshots/*.dump).
verify-import:
    cargo test -p korg-migrate --test fidelity

# Import kwi+kcard from snapshots into korg (set KORG_DATABASE_URL). Pass
# --reset to TRUNCATE *every* node kind (work items, cards, links, topics,
# daily plans, proposals, reports) plus projects and areas first — it refuses
# to run without KORG_RESET_CONFIRM=yes.
#   just snapshot        # refresh snapshots from the live sources (read-only)
#   KORG_DATABASE_URL=... just import --reset
import *ARGS:
    cargo run -q -p korg-migrate -- {{ARGS}}
