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

# Everything CI enforces, in the order that fails fastest. Mirrors
# .github/workflows/ci.yml — keep the two in step.
#
# The korg-migrate snapshot suites run only if snapshots/*.dump are present;
# see KORG_SNAPSHOT_TESTS in docs/setup.md.
check: fmt-check gen-check web-check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

fmt-check:
    cargo fmt --all --check

# svelte-check + eslint over the web app. Runs after gen-check because
# svelte-check type-checks the TypeScript `just gen` writes.
web-check:
    pnpm --dir web install --frozen-lockfile
    pnpm --dir web check
    pnpm --dir web lint

# Assert the checked-in generated files are what the generator produces right
# now — i.e. that regenerating changes nothing.
#
# This compares the files against *themselves* before and after, not against
# git. Comparing against git (`just gen && git diff --exit-code`) is the usual
# CI idiom and works on a clean checkout, but it also fails on a working tree
# that has legitimately-regenerated-but-not-yet-committed output, which is the
# normal state halfway through a sprint. Hashing sidesteps the question of what
# git happens to know.
gen-check:
    #!/usr/bin/env bash
    set -euo pipefail
    paths=(web/src/lib/generated crates/korg-mcp/tests/tools_schema.json)
    fingerprint() { find "${paths[@]}" -type f | sort | xargs sha256sum; }
    before=$(fingerprint)
    just gen
    if [ "$before" != "$(fingerprint)" ]; then
        echo "error: generated files are stale — 'just gen' changed them." >&2
        echo "       review the diff (every schema line is a change agents see), then commit." >&2
        exit 1
    fi

# Snapshot the frozen kwi + kcard source databases (read-only). [S3]
# Requires KCARD_DATABASE_URL (and optionally KWI_DATABASE_URL) in the env.
snapshot:
    bash scripts/snapshot.sh

# Run the kwi+kcard -> korg import and verify fidelity invariants F1-F7. [S6]
# Prerequisite: `just snapshot` (produces snapshots/*.dump). KORG_SNAPSHOT_TESTS=1
# because asking for this recipe is asking for the suite: missing snapshots must
# fail here, not skip.
verify-import:
    KORG_SNAPSHOT_TESTS=1 cargo test -p korg-migrate --test fidelity

# Import kwi+kcard from snapshots into korg (set KORG_DATABASE_URL). Pass
# --reset to TRUNCATE *every* node kind (work items, cards, links, topics,
# daily plans, proposals, reports) plus projects and areas first — it refuses
# to run without KORG_RESET_CONFIRM=yes.
#   just snapshot        # refresh snapshots from the live sources (read-only)
#   KORG_DATABASE_URL=... just import --reset
import *ARGS:
    cargo run -q -p korg-migrate -- {{ARGS}}
