#!/usr/bin/env bash
# cold-start.sh — prepare a Postgres cluster and korg.env for a korg container
# that has never run on this host. Runs ON the database host (kubsdb).
#
#   ssh kubs0 'cd ~/k-homelab && bin/secret get kubsdb-korg-db-password' \
#     | ssh kubsdb 'bash /datastore/korg/cold-start.sh'
#
# Everything above this in the deploy path assumes a container to inherit
# environment from. After a host rebuild there is none, and three things must
# exist before `docker compose up -d` can work: the `korg` role, the `korg`
# database, and korg.env. This script creates all three, idempotently, and is
# the only korg procedure that reads the age store — a routine deploy never
# moves the password off this host.
#
# THE PASSWORD ARRIVES ON STDIN AND NOWHERE ELSE. Not argv (world-readable in
# `ps`), not the environment (readable in /proc and `docker inspect`), not a
# temporary file, not shell history. The only thing this script prints is a
# truncated sha256 fingerprint — krot's convention — which is enough to prove a
# match against the store or the running container without revealing anything.
#
# Idempotent by construction: re-running it with the same password is a no-op
# against the cluster and rewrites korg.env with identical bytes. Re-running it
# with a NEW password is the rotation path.
set -euo pipefail

PG_CONTAINER="${KORG_PG_CONTAINER:-postgresql}"
PG_SUPERUSER="${KORG_PG_SUPERUSER:-postgres}"
ROLE="${KORG_ROLE:-korg}"
DB="${KORG_DB:-korg}"
ENV_PATH="${KORG_ENV_PATH:-/datastore/korg/korg.env}"
TIMEZONE="${KORG_TIMEZONE:-America/Los_Angeles}"

fingerprint() { printf '%s' "$1" | sha256sum | cut -c1-12; }
note() { printf '%s\n' "$*" >&2; }

# --- the password ------------------------------------------------------------

if [ -t 0 ]; then
  note "error: expected the password on stdin, got a terminal."
  note "       ssh kubs0 'cd ~/k-homelab && bin/secret get kubsdb-korg-db-password' \\"
  note "         | ssh kubsdb 'bash $0'"
  exit 2
fi

# `bin/secret get` may or may not emit a trailing newline; read succeeds either
# way but returns non-zero at EOF without one, hence the `|| true`.
IFS= read -r pw || true
[ -n "${pw:-}" ] || { note "error: no password on stdin"; exit 2; }

# The password is embedded in a URL, so a literal @ or / would silently corrupt
# the authority section and produce a connection failure that looks like a wrong
# password. Refuse rather than percent-encode: encoding here would mean the
# stored value and the deployed value differ, which is the drift this whole
# exercise exists to prevent.
case "$pw" in
  *@* | */*)
    note "error: password contains '@' or '/', which cannot appear unencoded in"
    note "       a postgres:// URL. Rotate to a value without them."
    exit 2
    ;;
esac

url="postgres://${ROLE}:${pw}@${PG_CONTAINER}:5432/${DB}"
note "password  sha256[:12] = $(fingerprint "$pw")"
note "DATABASE_URL sha256[:12] = $(fingerprint "$url")"

# --- role and database -------------------------------------------------------

docker inspect "$PG_CONTAINER" >/dev/null 2>&1 || {
  note "error: no container named '$PG_CONTAINER' on this host"
  exit 3
}

# Escape for a SQL string literal: double every single quote. Safe because
# standard_conforming_strings has been on by default since Postgres 9.1, so a
# backslash is not an escape character inside ''.
esc=${pw//\'/\'\'}

# CREATE ROLE / CREATE DATABASE have no IF NOT EXISTS, hence the guards. The
# ALTER runs unconditionally so that re-running with a new password rotates it.
docker exec -i "$PG_CONTAINER" psql -U "$PG_SUPERUSER" -d postgres \
  -v ON_ERROR_STOP=1 --quiet <<SQL
DO \$\$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '${ROLE}') THEN
    CREATE ROLE ${ROLE} LOGIN;
    RAISE NOTICE 'created role ${ROLE}';
  END IF;
END
\$\$;

ALTER ROLE ${ROLE} WITH LOGIN PASSWORD '${esc}';

SELECT format('CREATE DATABASE %I OWNER %I', '${DB}', '${ROLE}')
  WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = '${DB}')
\gexec
SQL

note "role '${ROLE}' and database '${DB}' present"

# --- korg.env ----------------------------------------------------------------

mkdir -p "$(dirname "$ENV_PATH")"
umask 077
tmp="$(mktemp "${ENV_PATH}.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
printf 'DATABASE_URL=%s\nKORG_TIMEZONE=%s\n' "$url" "$TIMEZONE" > "$tmp"
chmod 600 "$tmp"
mv "$tmp" "$ENV_PATH"
trap - EXIT
note "wrote $ENV_PATH (mode 600)"

# --- prove it, don't assume it -----------------------------------------------

# Storing a credential is not the same as having a working one — that is the
# lesson k-homelab sprint 016 paid for. Log in as the role, for real.
#
# Connect via the container's NETWORK NAME, not 127.0.0.1. kubsdb's pg_hba.conf
# begins:
#
#     local  all all                    trust
#     host   all all 127.0.0.1/32       trust
#     host   all all all                scram-sha-256
#
# so a loopback check inside the postgres container matches `trust` and succeeds
# no matter what the password is — a verification that verifies nothing, which is
# worse than none because it reports success. Resolving "$PG_CONTAINER" over the
# docker network gives a non-loopback source address, so the connection falls
# through to scram-sha-256: the same rule korg's own container authenticates
# under.
if docker exec -i -e PGPASSWORD="$pw" "$PG_CONTAINER" \
     psql -U "$ROLE" -d "$DB" -h "$PG_CONTAINER" -tAc 'select 1' >/dev/null 2>&1; then
  note "verified: role '${ROLE}' authenticates against database '${DB}'"
else
  note "error: role '${ROLE}' could NOT authenticate against '${DB}' after setup."
  note "       korg.env is written but the container will not start. Check"
  note "       pg_hba.conf in the ${PG_CONTAINER} container before retrying."
  exit 4
fi

note ""
note "next: docker compose -f $(dirname "$ENV_PATH")/docker-compose.yml up -d"
