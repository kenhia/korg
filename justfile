# korg — task runner

# Build the whole workspace.
build:
    cargo build --workspace

# Run the full test suite.
test:
    cargo test --workspace

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
