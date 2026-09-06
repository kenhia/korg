# 076 — find-by-ID in the nav, in-flight schedules, comment origin

Proposal [korg:1944](https://korg.kubsdb/planning/1944). Three independent
slices, one per layer, nothing coupling them — a fill-a-sprint bundle rather
than a themed one. They landed in the order backend, backend, UI.

## Premise check

All three held, checked before branching:

- **#1809** — the find-by-ID box existed only inside `work-items/+page.svelte`,
  with a page-local `findById()`. Nothing in the header.
- **#1644** — `BoardRollup` carried `due_schedules` and nothing else; no
  `in_flight` anywhere in the engine, the MCP surface, the bindings or the docs.
- **#1879** — `Comment` was `{id, node_id, body, created, updated}`;
  `relationship` already had `origin` from 0016 §4.

## Cross-project plan

korg is listed in `cross-project-planning/index.md` → `korg+/`. Two decisions
touched, neither contradicted:

- **GP-1** (agents curate korg; the board renders korg) settles #1644's surface
  question the same way the 2026-08-17 queue sweep settled proposal 1393's
  `due_schedules`: it goes on `get_board`, and kfdc and korg-dash inherit it.
- The plan's kfdc section describes the curator writing "one `⟦curator⟧`
  synopsis comment per live proposal, **everything `origin`-stamped**". Comments
  carried no origin, so the curator distinguishes its comment by a prose marker
  instead. #1879 makes that sentence literally true — the plan described the
  end state before the substrate existed. Worth an amend at ship time rather
  than a correction: nothing in it was wrong, it was early.

## #1879 — self-reported `origin` on comments

Migration `0033_comment_origin.sql`: one nullable `TEXT` column, no backfill, no
index. The decision worth recording is what it *isn't* — not an author column
(korg has no authenticated writer, which is the same reason 0026 declined one
for the transition log) and not a control (an unverified string cannot stop a
leg stamping `overseer` on its own clearance; karc closes that structurally with
PD-9). It is the audit aid beside those, so a human reading a thread sees
`overseer` and `overseen-sprint` at a glance.

Deliberately D-17's column, spelling and semantics rather than a second
convention — the same reasoning 0032 used to refuse `pinned` for `starred`.

Two write-path calls that could have gone either way:

- **Absent origin stores NULL**, never a transport-derived guess. "It arrived
  over MCP, call it `mcp`" would manufacture provenance the writer never
  claimed.
- **`update_comment` preserves an existing stamp when the caller sends none**
  (`COALESCE($3, origin)`), matching relate's ON CONFLICT no-op. Clearing was
  the worse of the two available bugs: a comment silently losing provenance
  during an unrelated body fix is the exact hole #1879 exists to close. An
  editor that *does* identify itself takes the stamp, because it wrote the body
  that is now there.

The web client sends `origin: "web"`, the same string it already stamps on
`relate`. The UI renders it as a small muted tag beside the row controls, only
when non-null — a placeholder on a NULL would dress an absence up as a fact.

## #1644 — in-flight schedules on the board

`in_flight_schedules`, a sibling field, chosen over folding due and in-flight
into one `schedules` list with a `state` discriminator (Ken, 2026-09-06). The
discriminator is tidier on paper and breaks a shipped contract: kfdc and
korg-dash already render `due_schedules` and would both have to change to keep
showing what they already show. Additive costs them nothing, and kfdc #1645 opts
in.

The non-obvious part is the predicate. It is **not** filtered on
`s.status = 'active'`, where it parts company with `due_schedules`: a `once`
schedule marks itself `done` as it fires, and the open item it produced is
precisely what must not vanish — filtering on an active schedule would have
re-created the bug for the case that reported it. Unfinished-ness is a fact
about the *item*, so the item is what the query filters on, reading
`WI_UNFINISHED_STATUSES` so `parked` counts (#810).

`materialized_at` is the work item's own `node.created` rather than the
`materializes` edge's `created`, which is nullable by design (0016 §4) — same
instant, always present.

## #1809 — find-by-ID in the global nav

The box moved to the header beside "Search korg…", and Ken's M estimate
("suspect this will require some re-plumbing") was right about the shape but the
re-plumbing turned out to be **deletion**, not addition.

The page-local version had two branches: jump to the row and flash it for a work
item, open the slide-over preview for anything else. Both existed because when
#260 built it there was nowhere else to go. Sprint 070 changed that — every kind
has a page, and `NodePreview.url` carries the path (GP-13) — so the global box
resolves the id and goes where korg says it lives. One branch, every kind, no
kind → path table in the layout.

**Consequence worth naming:** the in-table navigate-and-flash jump is gone.
Find-by-ID on a work item now lands on `/work-items/<wi>` rather than
highlighting the row in the list. That took `flashWi`, `forceShow` (and its
filter bypass, WI #762 D-3), `gotoWorkItem` and the page's field-level `error`
banner with it — all of them existed only to serve that box, and the `error`
state had no writers left once it went, so it left rather than becoming a block
that can never render. The Work Items page keeps `NodePreview` for the
related-nodes list (WI #611).

Errors render in the header under the box that produced them, cleared on the
next keystroke: the page below is whatever you were already looking at and has
no reason to host an error about a nav control.

## Shipped

- `crates/korg-core/migrations/0033_comment_origin.sql` — new
- `comment.origin` through the repo layer, `ops::CommentBody`, both transports,
  and all four focused reads' inlined comments
- `BoardRollup::in_flight_schedules` + `InFlightSchedule`, and the `get_board`
  tool description
- global find-by-ID in `+layout.svelte`; the box and its jump machinery removed
  from `work-items/+page.svelte`
- `docs/api.md`: board field table, a new "In-flight schedules (#1644)" section,
  and comment provenance folded into the D-17 section
- `crates/korg-core/tests/sprint076.rs` — 7 tests
- regenerated bindings and the MCP schema snapshot

`just check` green.
