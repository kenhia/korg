#!/usr/bin/env bash
# snapshot.sh — capture frozen, read-only pg_dump snapshots of the kwi and
# kcard source databases for korg's import (build step S3).
#
# Sources are NEVER mutated: pg_dump is read-only. Nothing here is committed —
# snapshots/ is gitignored and credentials come from the environment.
#
# kwi (host pg_dump):
#   KWI_DATABASE_URL    default: postgresql://ken@gratch:5432/workitems
#                       (password via ~/.pgpass)
#
# kcard — choose ONE source:
#   (a) Docker container (the running local stack):
#         KCARD_DOCKER_CONTAINER  e.g. kcard-pg
#         KCARD_ENV_FILE          optional; sourced for POSTGRES_USER/PASSWORD/DB
#                                 (e.g. kcard/deploy/local/.env)
#   (b) Reachable server:
#         KCARD_DATABASE_URL      e.g. postgres://kcard:<pw>@host:5432/kcard
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out_dir="${repo_root}/snapshots"
mkdir -p "${out_dir}"

validate() {
    local file="$1"
    if ! pg_restore --list "${file}" | grep -q "TABLE DATA"; then
        echo "ERROR: ${file} contains no table data" >&2
        return 1
    fi
}

# --- kwi ------------------------------------------------------------------
kwi_url="${KWI_DATABASE_URL:-postgresql://ken@gratch:5432/workitems}"
echo ">> dumping kwi -> ${out_dir}/kwi.dump"
pg_dump -Fc --no-owner --no-privileges "${kwi_url}" -f "${out_dir}/kwi.dump"
validate "${out_dir}/kwi.dump"

# --- kcard ----------------------------------------------------------------
kcard_container="${KCARD_DOCKER_CONTAINER:-}"
kcard_url="${KCARD_DATABASE_URL:-}"

if [[ -n "${kcard_container}" ]]; then
    if [[ -n "${KCARD_ENV_FILE:-}" ]]; then
        # shellcheck disable=SC1090
        set -a; . "${KCARD_ENV_FILE}"; set +a
    fi
    kc_user="${POSTGRES_USER:-kcard}"
    kc_db="${POSTGRES_DB:-kcard}"
    echo ">> dumping kcard (docker exec ${kcard_container}) -> ${out_dir}/kcard.dump"
    docker exec -e PGPASSWORD="${POSTGRES_PASSWORD:-}" "${kcard_container}" \
        pg_dump -Fc --no-owner --no-privileges -U "${kc_user}" "${kc_db}" \
        > "${out_dir}/kcard.dump"
    validate "${out_dir}/kcard.dump"
elif [[ -n "${kcard_url}" ]]; then
    echo ">> dumping kcard -> ${out_dir}/kcard.dump"
    pg_dump -Fc --no-owner --no-privileges "${kcard_url}" -f "${out_dir}/kcard.dump"
    validate "${out_dir}/kcard.dump"
else
    echo "ERROR: configure kcard source — set KCARD_DOCKER_CONTAINER (+KCARD_ENV_FILE)" >&2
    echo "       or KCARD_DATABASE_URL. See header of scripts/snapshot.sh." >&2
    exit 1
fi

echo "OK: snapshots written to ${out_dir}"
