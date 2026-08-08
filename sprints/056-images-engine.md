# 056 — Images engine: the `korg-img` crate

Proposal `korg:1081`, work items #582 and #1119. Slice 1 of the **Images in
korg** program (`korg:1127`); the design is settled and lives in the program
handoff (`korg:1128`), written from the 2026-08-08 Ken + Fable brainstorm on
cleo. That handoff is the specification — this record only carries what the
implementation decided on top of it.

## Goal

Ken can paste a screenshot into korg and it survives; an agent can `curl -F` an
image in and read a vision-sized variant back out. Everything downstream (UI,
infra, kmon) waits on this slice, and Ken wants the program's slices
**sequential**, so this one has to be whole rather than fast.

## What the handoff already decided (not relitigated)

D1 crate-not-microservice · D2 attachment-is-a-typed-node, markdown is placement
only · D3 `img-<hex>` display id · D4 eager variants, EXIF stripped, no dedup ·
D5 pending-sweeper is the entire GC story, no retention policy · D6 bytes over
REST / metadata over MCP · D7 store at `/datastore/korg/images` on kubsdb.

## Implementation decisions (the handoff's "open items", answered)

**I1 — What `korg-img` actually owns.** The crate is the *image and blob
engine*: id formatting/parsing, decode, variant generation, the on-disk layout,
hashing, and the store's own stats and sweep-by-path operations. It has no
Postgres dependency and no axum dependency.

The node model stays in korg-core, the routes in korg-api, the tools in
korg-mcp — because that layering is korg's, and a fourth crate owning its own
migration + router + tool list would be a second architecture inside one binary.
D1's point was "a crate boundary gives the same clean API without a second
deploy surface", and it does: `korg-img` is bytes-in / variants-and-paths-out,
which is exactly the part a future microservice would take if a second consumer
ever appears.

**I2 — `state` is derived from the edge, not stored.** D5 describes a `pending`
state that becomes `linked` on owner save. Storing that as a column gives two
sources of truth for one fact, and the failure mode is the bad one: a
`has_attachment` edge written through the generic `relate` path would leave the
column saying `pending`, and the sweeper would delete a **linked** image.

So there is no `state` column. `pending` means *no `has_attachment` edge*, and
the sweeper's predicate is "no edge AND older than 24h". The observable
lifecycle is exactly D5's, `state` is still reported on every read, and it
cannot lie. A consequence worth stating: an attachment whose owner node is
deleted loses its edge and is swept — which is correct GC, and the only path
that reaches it, because discarding an attachment deletes it outright and
deleting a *markdown token* never touches the edge (D2).

**I3 — The owner is always a node.** D2 says "owner (WI or comment)", but a korg
comment is not a node — it is a row in `comment` with a `node_id` pointing at
the node it hangs off (0001/0007). The `relationship` table can only reference
nodes, so an image pasted into a comment is owned by the node that comment is
on, and the comment body carries the placement token. That keeps GC edge-driven
(D2's actual requirement) and puts every image on one attachment list per node.

**I4 — URL shape**: `/api/img/{img-id}` for the original,
`/api/img/{img-id}/{variant}` for `thumb` and `agent`. Path segments rather than
the brainstorm's `IMG-C2A:thumb` colon spelling — same addressing, but nothing
downstream (proxy, `curl`, a markdown renderer) has to have an opinion about a
colon inside a path segment. The handoff left the exact shape to the
implementer.

**I5 — Disk layout**: `<root>/<node_id / 1000>/<img-id>/{original,thumb,agent}.<ext>`.
One directory per attachment (so slice 3's purge runbook is one `rm -rf`),
bucketed a thousand at a time so no single directory grows to 30K entries.

**I6 — Variant encoding follows the pixels, not the source format**: an image
with an alpha channel encodes to PNG, everything else to JPEG q82. Screenshots
land on PNG through that rule and photographs on JPEG, without either being
special-cased — and nothing with transparency gets silently flattened onto
whatever JPEG decides is white. Re-encoding from decoded pixels is what strips
EXIF, so that is free rather than a step. `thumb` is 400px on the long edge,
`agent` 1568px; neither upscales.

**I7 — Variant metadata is a table**, `attachment_variant`, not JSONB and not
eight columns on `attachment`. `/api/img/stats` is two `sum()`s, and a third
variant is a row rather than a migration.

**I8 — Sweeper cadence**: hourly, from a background task in korg-api, plus
`POST /api/img/sweep` so it is testable and runnable on demand. korg's "no
scheduler" rule (#581/#950) is about *surfacing work* — a job that decides
something is due, and lies when it dies. This decides nothing: it deletes rows
that provably have no owner. It is also logged, so a dead sweeper is visible.

## Shipped

- **`crates/korg-img`** — the engine: `ImgId` (`img-<hex>`, case-insensitive),
  `Variant`, `prepare()` (sniff → decode → both variants → SHA-256), and
  `Store` (bucketed, one directory per attachment). No Postgres, no axum, no
  clock. 12 unit tests.
- **Migration 0027** — `attachment` joins the kind vocabulary; `attachment` and
  `attachment_variant` detail tables. DDL only, clean re-tag.
- **`has_attachment`** in the relationship registry — the ninth label, and the
  only one whose *absence* destroys its right endpoint.
- **korg-core** — the attachment repo module (create / get / list / attach /
  delete / sweep / stats), a `NodePreview` arm, attachment titles in
  `related_context`, and `attachments` inlined on `get_work_item` with
  `has_attachment` excluded from that read's `related` block.
- **korg-api** — `POST /api/img`, `GET /api/img/:id[/:variant]`,
  `POST /api/img/:id/link`, `DELETE /api/img/:id`, `GET /api/img/stats`,
  `POST /api/img/sweep`, plus the hourly background sweeper. Decode and disk
  I/O run in `spawn_blocking`; the upload route carries its own 32 MB body
  limit because axum's default 2 MB would refuse most real screenshots.
- **korg-mcp** — `get_attachment`, `list_attachments`. Deliberately no
  `delete_attachment`: see below.
- **Deploy** — `KORG_IMG_ROOT` in the Dockerfile, `/datastore/korg/images`
  bind-mounted in the compose file.
- **Docs** — a normative `## Images` section in `api.md`, the REST table and
  vocabulary bullet in `usage.md`, the env table in `setup.md`, the
  `attachment` row in `node-shapes.md`, the catalogue and label registry.
- **Tests** — 12 in korg-img, 14 in `korg-core/tests/sprint056.rs`, 8 in
  `korg-api/tests/images.rs` driving real PNG bytes through the router.

### Two decisions made during the build, not before it

**No `delete_attachment` MCP tool.** It was written, then removed. korg-mcp has
no store handle, so the tool could only have deleted the row — and the sweeper
finds orphans *through* the row, so a row deleted without its bytes is bytes
nothing will ever collect. `DELETE /api/img/:id` owns both. This is the same
REST-for-bytes split D6 already drew; the tool would have quietly crossed it.

**The tool count was written down five times.** Adding two tools failed
`mcp_http.rs` and `server.rs`, both of which held a hand-maintained literal
long after `docs_drift` had checked the three real inventories and agreed.
`server.rs`'s copy sat under a comment claiming to assert that "tool
descriptors are stable", which a count does not. Both are now properties:
mcp_http compares the served *set* against the library's, and server.rs checks
that every descriptor has a name, a description and an object schema. This is
the F-12 lesson (inventories are generated or drift-tested, never hand-counted)
applied to the two files `docs_drift` does not read.

## Follow-ups

- **The store has no second copy.** korg's `pg_dump` already lands on gratch;
  the blobs do not, so a restored database can currently reference images that
  exist nowhere. Slice 3 (k-homelab #1122) adds the nightly rsync. This is the
  one real gap this slice ships with, and it is sequenced deliberately.
- **Focused reads other than `get_work_item` do not inline attachments.** A
  proposal or program with an image surfaces it as a `has_attachment` edge in
  `related` (resolving to the filename) and through `list_attachments`, not as
  an `attachments` block. #582 is about work items and the generic path is
  honest; widening it is cheap when something wants it.
- **Nothing writes or renders a markdown token yet** — that is slice 2
  (`korg:1124`, #1120/#1121), and until it lands the attachment list is the
  only surface. WI #1115's misaligned reading-list edit links is the natural
  first real-world use.

## Deployed

**2026-08-08** — `kubsdb.encke-wahoo.ts.net:5000/korg:e06c77842de3`
(digest `sha256:cedac9b4…49e1f`), built from the squash-merge `e06c778`.
Rollback target: `korg:01de7bd78c96` (sprint 055), confirmed present in the
registry before building.

**Rolling back across 0027 is a clean re-tag.** The migration is DDL-only and
purely additive, and an image that predates it never selects from `attachment`
or `attachment_variant` — so the previous image runs correctly against the
new schema. What a re-tag does *not* undo is the store: blobs written by this
image stay on disk with rows the old image ignores. Nothing breaks; the space
is reclaimed by the sweeper or by hand.

### Verified live

`post-deploy-check.sh --compare` clean: every row count identical (767 work
items, 30 cards, 4 links, 175 proposals, 33 reports, 40 projects), node count
1029 and `node_id_seq` at 1128 both unmoved, migrations 26 → 27 — the shape a
DDL-only migration should have.

Then the sprint's own work, which no fixed script covers. A 2000×1000 PNG
uploaded with `curl -F` against the deployed instance:

- **Identity** — `img-469`, node 1129 (0x469), as D3 specifies.
- **Original byte-exact** on the way back out, `content-type: image/png`,
  `cache-control: public, max-age=31536000, immutable`.
- **Variants** — `thumb` 400×200, `agent` 1568×784: both caps applied, aspect
  ratio preserved, both decode as real PNGs.
- **Disk layout** — `/datastore/korg/images/0001/img-469/{original,thumb,agent}.png`,
  through the new bind mount.
- **Lifecycle** — uploaded `pending`, `POST /link` to #582 moved it to `linked`
  and it inherited that item's project.
- **MCP** — `get_work_item(582)` carried the attachment with a URL per size,
  and `has_attachment` was correctly *absent* from that read's `related` block;
  `get_attachment` resolved it by `img_id`.
- **Accounting** — `/api/img/stats` reported `total_bytes: 192852` against a
  `find`-summed 192852 on the volume. The reconciliation the runbook describes
  agrees exactly on a fresh store, which is the only time it can be checked
  without interpretation.
- **Discard** — `DELETE` removed the record and all three blobs; store back to
  zero, serve 404. Production carries no residue from this test.

Web deep links (`/plan`, `/planning`, `/work-items`) and `/api/health` all 200.
