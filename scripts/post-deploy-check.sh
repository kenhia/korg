#!/usr/bin/env bash
# post-deploy-check.sh — prove a korg deployment actually works, not merely that
# a process is listening.
#
#   bash scripts/post-deploy-check.sh --baseline /tmp/korg-baseline.json   # before
#   bash scripts/post-deploy-check.sh --compare  /tmp/korg-baseline.json   # after
#   bash scripts/post-deploy-check.sh                                      # checks only
#
#   KORG_URL      default https://kubsdb.encke-wahoo.ts.net:5674
#   KORG_DB_SSH   host holding the postgres container, default kubsdb;
#                 set empty to skip the schema section entirely
#
# Exit 0 == healthy.
#
# Why this exists, when /api/health already returns {"status":"ok"}: health
# proves the process is up. A container still running last week's image passes
# it. What a deploy needs proven is that the *shipped code* answers correctly on
# both transports, that writes still land, and that nothing vanished — which is
# what the three sections below do.
#
# Two deliberate choices:
#
#   * The write is IDEMPOTENT — a project's status re-PATCHed to the value it
#     already has. A create/delete pair would prove more, but it adds rows to
#     production and needs cleanup that can itself fail at the worst moment. A
#     project is the target rather than a work item because projects are not
#     nodes: nothing but the project row's own `updated` moves, so no triage
#     view reorders because a deploy was verified.
#
#   * The count diff is REPORTED, not asserted. korg is live; humans and agents
#     add rows while an image builds. During the sprint 015 deploy this diff
#     explained a +1 that would otherwise have looked like a new archived filter
#     dropping data — the real cause was a work item created in the UI mid-build.
#     A decrease is the direction worth stopping for, and that is called out.
#
#   * The schema section (WI #584) is gathered over SSH, not HTTP, and is
#     OPTIONAL — it skips with a note if the host is unreachable, so this script
#     still runs against a local instance. korg applies migrations automatically
#     at container start, so a deploy can move schema state while every REST row
#     count stays identical; sprint 020 shipped a migration whose entire contract
#     was to leave the node id sequence alone, and verifying that meant psql over
#     SSH by hand. A rewound sequence is the case that matters: it is invisible
#     until the next write collides with an id already in use.

set -euo pipefail

U="${KORG_URL:-https://kubsdb.encke-wahoo.ts.net:5674}"
DB_SSH="${KORG_DB_SSH-kubsdb}"
MODE=none
FILE=

while [[ $# -gt 0 ]]; do
  case "$1" in
    --baseline) MODE=baseline; FILE="${2:?--baseline needs a file}"; shift 2 ;;
    --compare)  MODE=compare;  FILE="${2:?--compare needs a file}";  shift 2 ;;
    -h|--help)  sed -n '2,14p' "$0"; exit 0 ;;
    *)          U="$1"; shift ;;
  esac
done

command -v jq >/dev/null || { echo "FAIL: jq is required" >&2; exit 1; }

fail() { echo "FAIL: $*" >&2; exit 1; }
ok()   { echo "  ok   $*"; }

# ---------------------------------------------------------------------------
# Row counts, via the API's own `total` — which also exercises the collection
# envelope every list endpoint gained in sprint 015.
# ---------------------------------------------------------------------------

# A page size big enough that no bare-array read can reach it, and small enough
# that hitting it is a bug rather than a plausible corpus. See `counts`.
ARRAY_CEILING=100000

counts() {
  local wi card link proposal report project
  # Enveloped collections carry a filtered total; ask for archived=all so the
  # number is the whole table and not "what is live today".
  wi=$(curl -fsS "$U/api/work-items?archived=all&limit=1" | jq '.total')
  card=$(curl -fsS "$U/api/cards?archived=all&limit=1"    | jq '.total')
  link=$(curl -fsS "$U/api/links?archived=all&limit=1"    | jq '.total')
  # These three answer with bare arrays by design (small, hand-ordered), so the
  # count IS the array length — which is a corpus count only if nothing
  # truncated it, and one of them was being truncated (#1101).
  #
  # `/api/proposals` and `/api/projects` take no limit at all: `list_proposals`
  # and `list_projects` are unconditional `fetch_all`, so their lengths are
  # honest and stay honest. `/api/reports` DOES page, defaulting to 30 — and
  # production passed 30 reports long enough ago that this number had been
  # pinned at exactly 30, so one of the seven was a constant. A count that
  # cannot fall cannot detect the loss this whole compare exists to catch: the
  # sprint 052 deploy filed reports from two new sources and still printed
  # `reports: 30 -> 30 (0)`.
  #
  # Asking for a ceiling fixes today; asserting we stayed under it is what stops
  # the fix from rotting into the same silent saturation the day the corpus
  # outgrows the number.
  proposal=$(curl -fsS "$U/api/proposals" | jq 'length')
  report=$(curl -fsS "$U/api/reports?limit=$ARRAY_CEILING" | jq 'length')
  project=$(curl -fsS "$U/api/projects"   | jq 'length')
  [[ "$report" -lt "$ARRAY_CEILING" ]] || fail \
    "the report count hit the page ceiling ($ARRAY_CEILING) — it is a page size now, \
not a corpus count. Raise ARRAY_CEILING or give /api/reports a total envelope."
  jq -n --argjson w "$wi" --argjson c "$card" --argjson l "$link" \
        --argjson p "$proposal" --argjson r "$report" \
        --argjson j "$project" \
    '{work_items:$w, cards:$c, links:$l, proposals:$p, reports:$r, projects:$j}'
}

# ---------------------------------------------------------------------------
# Schema state, over SSH — the half no REST count can see (WI #584).
#
# Prints a JSON object, or nothing (non-zero) when the DB host is unreachable,
# `ssh` is missing, or KORG_DB_SSH is empty. Callers treat that as "skip", never
# as failure: this script has to keep working against a local instance.
#
# kubsdb's login shell is fish, which mis-parses $() and $$-quoting — hence
# `ssh … bash -s` with a quoted heredoc, per docs/operations.md. The container is
# `postgresql` and the read-only role is `korg`.
# ---------------------------------------------------------------------------
schema() {
  [[ -n "$DB_SSH" ]] || return 1
  command -v ssh >/dev/null || return 1
  ssh -o ConnectTimeout=5 -o BatchMode=yes "$DB_SSH" bash -s <<'REMOTE' 2>/dev/null
docker exec postgresql psql -U korg -d korg -tAc "
  select json_build_object(
    'migrations',    (select count(*)    from _sqlx_migrations),
    'migration_max', (select max(version) from _sqlx_migrations),
    'node_count',    (select count(*)    from node),
    'node_min',      (select min(id)     from node),
    'node_max',      (select max(id)     from node),
    'seq_last',      (select last_value  from node_id_seq),
    'seq_called',    (select is_called   from node_id_seq))"
REMOTE
}

echo "== korg post-deploy check: $U"

# ---------------------------------------------------------------------------
# 1. Reads
# ---------------------------------------------------------------------------
echo "-- reads"

curl -fsS "$U/api/health" | grep -q '"status":"ok"' || fail "/api/health did not report ok"
ok "health"

SNAP=$(counts)
echo "$SNAP" | jq -r 'to_entries[] | "  ok   \(.key): \(.value)"'

# A focused read: the two-level contract (sprint 015) says this inlines comments
# and carries an exact comment_count. A list working while a single-item read
# 500s is a real failure mode and one a bare health check misses entirely.
FIRST_WI=$(curl -fsS "$U/api/work-items?limit=1" | jq -r '.items[0].wi_number // empty')
[[ -n "$FIRST_WI" ]] || fail "no work items returned — the database looks empty"
curl -fsS "$U/api/work-items/$FIRST_WI" | jq -e 'has("comment_count") and has("comments")' >/dev/null \
  || fail "GET /api/work-items/$FIRST_WI is missing the inlined-comment fields"
ok "focused read (work item #$FIRST_WI)"

# The error contract, which is code and therefore deployable — and was wrong in
# production for longer than anyone realised before sprint 013.
MISS=$(curl -sS -o /tmp/korg-404.$$ -w '%{http_code}' "$U/api/work-items/999999999")
[[ "$MISS" == "404" ]] || { rm -f /tmp/korg-404.$$; fail "a missing work item answered $MISS, expected 404"; }
jq -e '.code == "not_found"' /tmp/korg-404.$$ >/dev/null \
  || { rm -f /tmp/korg-404.$$; fail "the 404 body is missing code=not_found"; }
rm -f /tmp/korg-404.$$
ok "error contract (404 + code:not_found)"

# ---------------------------------------------------------------------------
# 2. MCP
# ---------------------------------------------------------------------------
echo "-- mcp"
KORG_MCP_URL="$U/mcp" bash "$(dirname "$0")/mcp-roundtrip-check.sh" >/dev/null \
  || fail "MCP roundtrip check failed — run scripts/mcp-roundtrip-check.sh for detail"
ok "initialize + tools/list + tools/call"

# ---------------------------------------------------------------------------
# 3. Write path
# ---------------------------------------------------------------------------
echo "-- write"
PROJ=$(curl -fsS "$U/api/projects" | jq -r '[.[] | select(.name == "korg")][0] // .[0]')
NAME=$(jq -r '.name' <<<"$PROJ")
STATUS=$(jq -r '.status' <<<"$PROJ")
[[ -n "$NAME" && "$STATUS" != "null" ]] || fail "could not read a project to re-PATCH"

AFTER=$(curl -fsS -X PATCH "$U/api/projects/$NAME" \
  -H 'content-type: application/json' \
  -d "$(jq -n --arg s "$STATUS" '{status:$s}')")
[[ "$(jq -r '.status' <<<"$AFTER")" == "$STATUS" ]] \
  || fail "the idempotent PATCH of project '$NAME' did not return the value it was given"
ok "idempotent write (project '$NAME' status=$STATUS)"

# ---------------------------------------------------------------------------
# 4. Schema state — optional, and never fatal when simply unavailable.
# ---------------------------------------------------------------------------
echo "-- schema"
if SCHEMA=$(schema) && [[ -n "$SCHEMA" ]] && jq -e . <<<"$SCHEMA" >/dev/null 2>&1; then
  jq -r 'to_entries[] | "  ok   \(.key): \(.value)"' <<<"$SCHEMA"
else
  SCHEMA=
  echo "  --   skipped (no psql over ssh${DB_SSH:+ to $DB_SSH})"
fi

# ---------------------------------------------------------------------------
# 5. Baseline / compare
# ---------------------------------------------------------------------------
case "$MODE" in
  baseline)
    jq -n --argjson c "$SNAP" --argjson s "${SCHEMA:-null}" '{counts:$c, schema:$s}' > "$FILE"
    echo "-- baseline written to $FILE"
    ;;
  compare)
    [[ -f "$FILE" ]] || fail "no baseline at $FILE — run with --baseline before deploying"
    # Pre-#584 baselines are a bare counts object; `.counts // .` reads both.
    BEFORE_COUNTS=$(jq -c '.counts // .' "$FILE")
    BEFORE_SCHEMA=$(jq -c '.schema // null' "$FILE")
    echo "-- counts vs baseline ($FILE)"
    DIFF=$(jq -n --argjson before "$BEFORE_COUNTS" --argjson a "$SNAP" '
      [ $a | keys[] as $k
        | { kind: $k, before: ($before[$k] // 0), after: $a[$k],
            delta: ($a[$k] - ($before[$k] // 0)) } ]')
    jq -r '.[] | "  \(if .delta < 0 then "LOST" elif .delta > 0 then "  +" else "  =" end) \(.kind): \(.before) -> \(.after) (\(if .delta > 0 then "+" else "" end)\(.delta))"' <<<"$DIFF"
    if jq -e 'any(.[]; .delta < 0)' <<<"$DIFF" >/dev/null; then
      echo
      echo "WARNING: a row count went DOWN across this deploy." >&2
      echo "Rows do not disappear on their own. Investigate before declaring the" >&2
      echo "deploy good — see docs/operations.md for the read-only query path and" >&2
      echo "the restore procedure." >&2
      exit 1
    fi

    if [[ -n "$SCHEMA" && "$BEFORE_SCHEMA" != "null" ]]; then
      echo "-- schema vs baseline"
      jq -rn --argjson b "$BEFORE_SCHEMA" --argjson a "$SCHEMA" '
        $a | keys[] as $k
        | "  \(if ($b[$k] == $a[$k]) then "  =" else "  ~" end) \($k): \($b[$k]) -> \($a[$k])"'
      # Reported, not asserted — except downward, matching the count diff above.
      # A migration going missing means the running image is older than the
      # database; a rewound sequence hands out ids that already exist, which
      # stays invisible until the next write collides.
      if jq -ne --argjson b "$BEFORE_SCHEMA" --argjson a "$SCHEMA" \
           '($a.migrations < $b.migrations) or ($a.seq_last < $b.seq_last)' >/dev/null; then
        echo
        echo "WARNING: schema state moved BACKWARDS across this deploy." >&2
        echo "  migrations:  $(jq -r '.migrations' <<<"$BEFORE_SCHEMA") -> $(jq -r '.migrations' <<<"$SCHEMA")" >&2
        echo "  node_id_seq: $(jq -r '.seq_last' <<<"$BEFORE_SCHEMA") -> $(jq -r '.seq_last' <<<"$SCHEMA")" >&2
        echo "A dropped migration means the deployed image is older than the schema" >&2
        echo "it is talking to. A rewound sequence will collide on the next insert." >&2
        echo "See docs/operations.md before declaring this deploy good." >&2
        exit 1
      fi
    elif [[ -n "$SCHEMA" ]]; then
      echo "-- schema vs baseline: baseline has none (taken before WI #584, or with no DB access)"
    fi
    ;;
esac

echo "== OK"
