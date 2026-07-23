#!/usr/bin/env bash
# post-deploy-check.sh — prove a korg deployment actually works, not merely that
# a process is listening.
#
#   bash scripts/post-deploy-check.sh --baseline /tmp/korg-baseline.json   # before
#   bash scripts/post-deploy-check.sh --compare  /tmp/korg-baseline.json   # after
#   bash scripts/post-deploy-check.sh                                      # checks only
#
#   KORG_URL   default https://kubsdb.encke-wahoo.ts.net:5674
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

set -euo pipefail

U="${KORG_URL:-https://kubsdb.encke-wahoo.ts.net:5674}"
MODE=none
FILE=

while [[ $# -gt 0 ]]; do
  case "$1" in
    --baseline) MODE=baseline; FILE="${2:?--baseline needs a file}"; shift 2 ;;
    --compare)  MODE=compare;  FILE="${2:?--compare needs a file}";  shift 2 ;;
    -h|--help)  sed -n '2,12p' "$0"; exit 0 ;;
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

counts() {
  local wi card link topic proposal report project
  # Enveloped collections carry a filtered total; ask for archived=all so the
  # number is the whole table and not "what is live today".
  wi=$(curl -fsS "$U/api/work-items?archived=all&limit=1" | jq '.total')
  card=$(curl -fsS "$U/api/cards?archived=all&limit=1"    | jq '.total')
  link=$(curl -fsS "$U/api/links?archived=all&limit=1"    | jq '.total')
  topic=$(curl -fsS "$U/api/topics?archived=all&limit=1"  | jq '.total')
  # These three answer with bare arrays by design (small, hand-ordered).
  proposal=$(curl -fsS "$U/api/proposals" | jq 'length')
  report=$(curl -fsS "$U/api/reports"     | jq 'length')
  project=$(curl -fsS "$U/api/projects"   | jq 'length')
  jq -n --argjson w "$wi" --argjson c "$card" --argjson l "$link" \
        --argjson t "$topic" --argjson p "$proposal" --argjson r "$report" \
        --argjson j "$project" \
    '{work_items:$w, cards:$c, links:$l, topics:$t, proposals:$p, reports:$r, projects:$j}'
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
# 4. Baseline / compare
# ---------------------------------------------------------------------------
case "$MODE" in
  baseline)
    echo "$SNAP" > "$FILE"
    echo "-- baseline written to $FILE"
    ;;
  compare)
    [[ -f "$FILE" ]] || fail "no baseline at $FILE — run with --baseline before deploying"
    echo "-- counts vs baseline ($FILE)"
    DIFF=$(jq -n --slurpfile b "$FILE" --argjson a "$SNAP" '
      $b[0] as $before
      | [ $a | keys[] as $k
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
    ;;
esac

echo "== OK"
