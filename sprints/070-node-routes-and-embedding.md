# 070 — korg addressable and embeddable

Proposal korg:1469. Covers #1467 (M, per-node routes) and #1468 (S,
`frame-ancestors`).
Branch `070-node-routes-and-embedding`.

Slice 1 of program korg:1471, which spans korg and kfdc. Slice 2 is
kfdc:1470 (#1203, the iframe pane) and is blocked on **both** halves of
this one: a missing route leaves nothing to point at, and a missing CSP
means the browser refuses the frame even when the route is right.

## Goal

Two things kfdc's expanded-mode pane needs, neither of which is kfdc's
errand.

The routes half had been paid for three times before kfdc asked. korg
#981's awaiting lane could not link kinds without a URL and settled for
the kind's *list* page; kfdc #993's Net Log degrades to plain text under
the standing instruction *"the per-node deep-link gap is korg's, don't
fake URLs"*; korg-vs's resolve-by-ID — paste `korg:1203`, land on the
node — had nothing to resolve to for six of nine kinds.

**Nothing was broken, which is why this was invisible on the board.** Two
consumers were degrading correctly rather than fabricating, which is the
behaviour you want and also the behaviour that produces no signal. The
audit #1467 asked for is what turned it up.

## The audit

Own page before this sprint: `handoff` (`/handoffs/:node_id`, #621),
`program` (`/programs/:node_id`, sprint 044), `sprint_proposal`
(`/planning/:node_id`, #1162). Three of nine.

`card`, `link`, `report` and `schedule` fell back to their list page —
a real destination, as #981 argued, but not the node. `attachment` had
no destination at all.

And **`workitem` had a URL that did not exist**: `nodeHref` returned
`/work-items?wi=<n>` and the Work Items page reads no URL parameters at
all. Every link built from it — the proposal detail page's "open in Work
Items", the awaiting lane, search's "open in context" — landed on the
unfiltered list and did nothing with the parameter. That is the one place
the gap was not degrading gracefully; it was quietly wrong, and had been
since the fallback was written.

## What shipped — #1467

**One route table, in korg-core.** `NODE_ROUTES` sits beside `NODE_KINDS`
in `vocab.rs`, and `every_node_kind_has_a_route` iterates `NODE_KINDS` to
fence it. A tenth node kind fails the build until it has a page. That
fence is written the way it is because of `schedule`: sprint 051 added it
to the `node.kind` constraint and to nothing else, and the fence meant to
catch exactly that (`every_node_kind_resolves_to_a_real_title`) iterated
its own hardcoded list of seven and passed for three sprints.

| kind | route |
| --- | --- |
| `workitem` | `/work-items/{wi_number}` |
| `card` | `/cards/{node_id}` |
| `link` | `/reading-list/{node_id}` |
| `sprint_proposal` | `/planning/{node_id}` |
| `report` | `/daily-reports/{node_id}` |
| `handoff` | `/handoffs/{node_id}` |
| `program` | `/programs/{node_id}` |
| `schedule` | `/schedules/{node_id}` |
| `attachment` | `/attachments/{node_id}` |

**Six new pages, one renderer.** `NodeDetail.svelte` renders
`get_node_preview` full width; each new route is a `<BackTo>` and a
`<NodeDetail>`. This is what kept #1467 an M rather than six page builds,
and it is honest rather than a shortcut: the payload has been uniform and
kind-agnostic since #260 and has had an arm for every kind since #870's
audit. korg was never missing the *rendering* — only a URL to render it
at.

#621's per-kind decision is upheld by the **URL**, not by the renderer.
`/cards/812` says what it points at; the day a card wants a board-aware
page it replaces the component behind the same address. A kind that
outgrows the uniform view graduates; it does not get a new URL.

**`GET /n/:node_id`** resolves the kind server-side and 307s to the
canonical page. This is the call a consumer cannot make for itself:
`korg:1395` and `WI-836` carry an id and **no kind**. A redirect rather
than a rendered page for three reasons — the address bar ends on the real
URL, korg grows no second renderer to argue with #621, and it works under
`curl -I` and with JS off. The fragment needs no server support:
fragments are never sent to servers, and a browser re-applies the
original to a redirect target that carries none.

**korg hands over the URL where it already hands over a node.**
`NodePreview.url` and `SearchHit.url`. Paths, not absolute URLs — korg
answers on several hostnames and the caller holds the one it asked on —
and nullable, so "korg cannot say" stays representable.

**The comment anchor is the case that proves the rule.** A search hit on
a comment carries `kind: "comment"` and the *owning node's* id, whose
kind the response never carried. `korg:1395#comment-777` is a locator
korg has printed since #1177, and nothing but korg could turn it into a
URL. `SearchHit` gained a private `owner_kind` column for exactly this;
every node page mounts the same `Comments` component, so `#comment-<id>`
is one convention across every kind, with a scroll-and-flash that waits
for the fetch (the browser applies the fragment while the list is still
one "Loading…" row).

**korg's own UI gave up its copy.** `domain.ts` had `OWN_PAGE` (three
kinds) beside `LIST_PAGE` (four) and a `nodeHref` cascade over both.
`nodeHref` is gone — with every tier below the first removed it was a
second name for `nodePage` plus a `wi_number` argument that could not
matter — and `nodePage` reads the generated table. The preview panel's
"Open full page" uses `node.url` and the search page uses `hit.url`, so
korg's own app exercises the fields kfdc and korg-vs read.

## What shipped — #1468

`KORG_FRAME_ANCESTORS`, comma-separated (matching `KORG_CORS_ORIGINS`
beside it), served as `Content-Security-Policy: frame-ancestors` on
**every** response — the pane loads a document and everything that
document pulls in, and a policy covering only the first has a hole in it.

**Default closed, and spelled out.** Unset serves `frame-ancestors
'none'` rather than omitting the header, because an absent directive
means *anyone* may frame korg: silence and "nobody" are opposite answers.

**No `X-Frame-Options`.** The older header cannot express an allowlist,
and where the two disagree browsers honour the stricter — so one added
later, for the best of reasons, would silently undo this and leave the
pane blank with a correct-looking CSP in the response.
`no_x_frame_options_header` fences it.

**Embedding is not authentication.** Said at the setting, in
`setup.md`, in `api.md`, and asserted: `the_allowlist_grants_no_access`
writes a work item through a korg that permits no embedding. The
plausible mistake is the other direction — somebody reads the allowlist
as an access-control list and later "tightens" korg by trusting it.

**The allowlist ships in `deploy/docker-compose.yml`, not `korg.env`.** It is
not a secret, and it is a decision about how korg runs — which is what that
file is the source of truth for. Putting it in the gitignored env file would
make "who may frame korg" unreviewable, the failure #842 fixed for the rest of
the run config. Without this the code would deploy inert.

No `postMessage`, on either account. v1 is one-way by decision.

## Verified

`just check` green. New suites: `korg-core/tests/sprint070.rs` (5 —
every kind resolves, the preview and search `url`s agree with the table,
a comment hit routes to its anchor) and `korg-api/tests/sprint070.rs`
(9 — the redirect, the 404, that `/n/` is a real route and not the SPA
fallback swallowing it, and the five CSP properties).

Ran live against a throwaway Postgres with
`KORG_FRAME_ANCESTORS=https://kubsdb.encke-wahoo.ts.net:8100`:
`/n/1` → `307` + `location: /work-items/1`, `/n/999999` → `404`,
`content-security-policy: frame-ancestors https://kubsdb.encke-wahoo.ts.net:8100`
on the shell, on the redirect and on the 404, and no `x-frame-options`
anywhere.

Full Playwright suite on a fresh database: **86 passed**, including five
new specs in `node-routes.spec.ts`. One existing spec changed —
`awaiting-decide`'s "every kind on the lane is clickable" asserted a card
links to `/cards`, and now asserts `/cards/<id>`. That assertion was
#981's fallback, and this sprint is what retires it.

## Plan obligation — discharged

`cross-project-planning/korg+/PLAN.md` amended in this ship (commit
`34e14bd`), per the amend rule:

- **GP-16** — a per-node URL is korg's to emit, not the consumer's to
  reconstruct. GP-13 in the URL register, and the fourth time the shape
  has been paid for. Names the two supported answers (`/n/:node_id`, or
  the `url` on a ref korg already returns) and the forbidden third — a
  consumer-side kind → path map, which is GP-14's mistake in a new
  register. korg's own web app giving up its copy is the evidence, so the
  rule has no exception to point at.
- **GP-17** — embedding is not authentication, and the embed allowlist is
  not an access-control list. Recorded in the plan rather than only in
  korg's docs because the misreading is cross-project: the next consumer
  added to that list will be tempted to read membership as permission.

## Follow-ups

- **`AwaitingRow` and `RelatedRef` carry no `url`.** Both name a node and
  both are read by consumers; GP-16 says korg should hand the path over
  there too. Left out because both are `FromRow` structs built in SQL and
  neither had a consumer blocked on it this sprint — korg's own UI reads
  them through `nodePage`, and `/n/:node_id` covers anyone else. Worth a
  follow-up before the second external consumer of either lands.
- **`NodeDetail` renders neighbours as `kind #id`, not titles.**
  `neighbors` returns the edge, not the neighbour's content, and
  resolving 25 titles would be 25 reads to decorate a list most nodes do
  not have. Fine today; a `title` on `Neighbor` would be the fix if it
  starts mattering.
- **The e2e suite is order-dependent under an accumulating database.**
  `today-due-schedules` seeds a project named `e2e-due-pill-gone-…`, and
  Playwright's `getByRole(name:)` is substring-matched, so a second run
  against the same database makes `programs-progress`'s
  `getByRole("button", {name: "Go"})` ambiguous. Pre-existing, unrelated
  to this sprint, and only visible because a fresh database is the normal
  case. Worth an `exact: true` on that locator.

## Deployed 2026-08-20

- **Image** `kubsdb.encke-wahoo.ts.net:5000/korg:64c3aa01bb77`
  (digest `sha256:7b2984a79b6a…`), from merge commit `64c3aa0`.
- **Rollback target** `korg:d5fc8a0181f7` (sprint 069), confirmed present in
  the registry before building. Clean image-only rollback this time: **no
  schema migration** — 30 before and after — so a re-tag is a complete revert.
- **Backup** before deploy: `korg-20260820-031846.sql.gz`, 1,695,362 bytes —
  from this morning and larger than the night before.

Verified live:

| check | result |
|---|---|
| running revision == commit built | `64c3aa01bb77793b…`, asserted in the deploy script |
| `post-deploy-check.sh --compare` | OK, exit 0 |
| migrations | 30 → 30 (none pending) |
| every row count vs baseline | **unchanged** — cards 30, links 4, projects 50, proposals 267, reports 45, work items 976, nodes 1369 |
| `frame-ancestors` on the shell | `frame-ancestors https://kubsdb.encke-wahoo.ts.net:8100` |
| `X-Frame-Options` | **absent**, as required — it would override the allowlist |
| the header on non-200s | present on the 307 **and** the 404, not only the shell |
| `GET /n/1469` | `307` → `location: /planning/1469` |
| `GET /n/1467` | `307` → `location: /work-items/1467` |
| `GET /n/999999` | `404` (stale locator, no redirect into nowhere) |
| all nine per-kind routes | `200` — `/work-items/1467`, `/cards/1`, `/reading-list/1`, `/daily-reports/1`, `/schedules/1`, `/attachments/1`, `/planning/1469`, and the three that already existed |
| `NodePreview.url` | `/api/nodes/1469` → `/planning/1469`; `/api/nodes/1471` → `/programs/1471` |
| `SearchHit.url` | live `frame-ancestors` search returned `WI-1468` → `/work-items/1468`, `korg:1469#comment-907` → `/planning/1469#comment-907`, and `korg:1189#comment-584` → `/work-items/1189#comment-584` |

That last row is the one worth keeping: two comment hits whose owners are
*different kinds*, both resolving to the right page with the right anchor. It is
the case a consumer could not have computed — the response carries the owning
node's id and never its kind — and it is now answered from production.

Program korg:1471 moved `active` → `holding`: slice 1 is done, slice 2
(kfdc:1470, #1203) is `proposed` with one open item and nothing in flight. It is
unblocked as of this deploy — both prerequisites are live at
`https://kubsdb.encke-wahoo.ts.net:5674`.
