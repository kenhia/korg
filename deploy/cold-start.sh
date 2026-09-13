#!/usr/bin/env bash
# cold-start.sh — prepare a Postgres cluster and korg.env for a korg container
# that has never run on this host. Runs ON the database host (kubsdb).
#
#   ssh kubs0 'cd ~/k-homelab && bin/secret get kubsdb-korg-db-password' \
#     | ssh kubsdb 'bash /datastore/korg/cold-start.sh'
#
# Everything above this in the deploy path assumes a container to inherit
# environment from. After a host rebuild there is none, and four things must
# exist before `docker compose up -d` can work: the `korg` role, the `korg`
# database, korg.env, and KORG_DB_PASSWORD in /etc/khomelab/secrets.env.
#
# This script creates the first three, idempotently. **The fourth is not its
# job** (korg #2547, program korg:2440): the per-host secrets file is rendered
# by k-homelab from the same age store, with
#
#     bin/apply kubsdb khomelab-secrets
#
# and korg must not write a second producer of it. This script still needs the
# password because it is what sets the role's password on the cluster — but it
# no longer writes the value anywhere on disk, which is the whole change.
#
# So the order after a rebuild is: run this, then render the secrets file, then
# `docker compose up -d`. Both steps take their value from the one age-store
# entry, so they cannot disagree.
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

# This script used to refuse any password containing '@' or '/', because it
# embedded the value in a postgres:// URL where either character silently
# corrupts the authority section and fails as if the password were wrong.
#
# That refusal is GONE (korg #2547). korg now receives the password as its own
# variable and applies it to parsed connection options rather than splicing it
# into a string, so no character in it is special to anything here. That matters
# beyond tidiness: the constraint was a string format dictating what values the
# fleet's rotation tooling was allowed to generate, and korg:2439's proof pass
# rotates exactly this password.
note "password sha256[:12] = $(fingerprint "$pw")"

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

# KORG_TIMEZONE and nothing else. DATABASE_URL moved to docker-compose.yml when
# it stopped carrying a credential, and the credential itself is only ever in
# the age store and the file k-homelab renders from it (korg #2547).
#
# Writing this file at all is now a small thing — it exists because korg-core
# rejects a missing IANA zone at startup rather than guessing one, so something
# has to put it on the host, and this is the script that runs on a bare one.
mkdir -p "$(dirname "$ENV_PATH")"
umask 077
tmp="$(mktemp "${ENV_PATH}.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
printf 'KORG_TIMEZONE=%s\n' "$TIMEZONE" > "$tmp"
chmod 600 "$tmp"
mv "$tmp" "$ENV_PATH"
trap - EXIT
note "wrote $ENV_PATH (mode 600, no secret)"

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
  note "       Check pg_hba.conf in the ${PG_CONTAINER} container before retrying."
  exit 4
fi

# The control. The paragraph above explains why a loopback check would accept
# any password; this is what stops that explanation from being the only thing
# standing between us and a check that passes for the wrong reason. If a WRONG
# password is also accepted, the check above proved nothing and saying so is the
# only honest outcome.
if docker exec -i -e PGPASSWORD="${pw}-wrong" "$PG_CONTAINER" \
     psql -U "$ROLE" -d "$DB" -h "$PG_CONTAINER" -tAc 'select 1' >/dev/null 2>&1; then
  note "error: a DELIBERATELY WRONG password was also accepted, so the check"
  note "       above verified nothing. Something is matching a 'trust' rule in"
  note "       ${PG_CONTAINER}'s pg_hba.conf — fix that before trusting any of"
  note "       this. Nothing about the stored password is proven either way."
  exit 5
fi
note "control:  a wrong password is refused"

note ""
note "next: render the per-host secrets file, from kubs0:"
note "        cd ~/k-homelab && bin/apply kubsdb khomelab-secrets"
note "      then start korg:"
note "        docker compose -f $(dirname "$ENV_PATH")/docker-compose.yml up -d"
note ""
note "korg will not start until KORG_DB_PASSWORD is in /etc/khomelab/secrets.env"
note "— this script deliberately does not write it (korg #2547)."
