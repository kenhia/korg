# Work-item attachments plan

Date: 2026-07-24
Status: design agreed with Ken 2026-07-24 in session; not yet filed as
proposals. Ready to slice.
Source: korg:582 — "Ability to attach screen captures to WI's" (areas: engine, ui)

## Context

For a UI bug, a screenshot is worth a couple dozen words. Today there is no way
to get one into korg, so visual context lives in chat scrollback and dies with
the session. The ask is: paste a screenshot into a work item, have it render,
and have it be reapable later so the store does not grow without bound.

The interesting half is not storage. It is that **an agent working the item can
see the picture** — the same invisible-context failure sprint 012 fixed for
comments applies here, and this plan inherits that contract rather than
reinventing it.

## What the numbers say

Measured on the live instance, 2026-07-24:

| | |
|---|---|
| `pg_database_size('korg')` | **10 MB** |
| Last nightly dump | **302 KB** gzipped (`korg-20260724-032056.sql.gz`) |
| korg container volumes | **none** — the container is stateless today |
| `/datastore` free on kubsdb | 864 GB (local disk, **not backed up**) |
| `/gratch` free | 12 TB (NAS-backed; the only thing kubsdb backs up to it is the korg dump) |

One 800 KB screenshot is 8% of the entire database. Fifty of them double it. A
20 GB blob store inside Postgres would mean 20 GB nightly dumps held 14 deep,
a restore drill measured in tens of minutes instead of seconds, and — worse —
it would break the health heuristic operations.md tells an operator to trust:
*"the newest dump should be larger than its predecessor."* Under a retention
policy, dumps would legitimately shrink, and the one cheap signal that
distinguishes a healthy backup from a broken one would start lying.

So the bytes live outside the database. The honest cost of that choice is
**split-brain between rows and disk**, and this plan pays for it explicitly
with a blob state machine plus a reconciler, rather than pretending it cannot
happen.

## Decisions

### D-1. Attachments are a side table, not a node kind

The 0008/0010/0017 pattern (new kind + detail table + relationship edge) is
korg's default for new entities and it is **wrong here**, for a specific
reason: migration 0009 made `node.id == wi_number` for work items. Every
non-workitem node consumes a work-item number. Attachments would be the
highest-volume kind in the system by an order of magnitude — a few hundred
screenshots and work-item numbers jump from #582 to #900 with nothing in
between. Numbers are referenced in docs, commits and conversation; making them
sparse is a permanent, user-visible cost paid for an abstraction nobody asked
for.

The right precedent is `comment`: a side table keyed by `node_id` with
`ON DELETE CASCADE`, deliberately *not* a node. An attachment is subordinate
content — a visual comment — not an entity with independent identity and edges.
Keying on `node_id` also means cards, reports and handoffs get attachments for
free.

Accepted consequence: attachments do not appear in `neighbors` /
`related_context`, and one attachment row cannot span two work items. The
second is handled by D-2 (two rows, one blob) and the first is not wanted.

### D-2. Content-addressed; the SHA-256 *is* the identity

The blob's SHA-256 is its id, its filename on disk, and how it is referenced in
markdown. This buys, in order of importance:

1. **Idempotent upload.** An agent retry with the same bytes is a no-op that
   returns the existing reference. This matters precisely because agents will
   be uploading over a network.
2. **Free, permanent HTTP caching.** The bytes at a URL can never change, so
   `ETag: "<sha>"` + `Cache-Control: immutable` is correct with no
   invalidation logic, ever. This is also what makes a pull-through transform
   service (D-11) trivial later.
3. **Integrity verification.** The reconciler can re-hash on demand.

Space dedupe is a rounding error and is *not* the reason — two screenshots of
the same screen taken seconds apart differ in bytes and will not dedupe.

**Two work items referencing the same blob is normal, not a bug.** It is the
expected result of pasting the same image into two items, and of an idempotent
re-upload. Every reference check must be a `count`, never an `exists`.

### D-3. Two tables: `blob` (the ledger) and `attachment` (the reference)

D-2 forces the split, and it lands cleanly: **availability is a property of the
bytes, not of any one reference.** Ken's "no longer available" tombstone
therefore lives on `blob`, shared by every work item that referenced it, so all
of them render the same placeholder and stay consistent by construction.

```sql
CREATE TABLE blob (
    sha256        TEXT        PRIMARY KEY CHECK (sha256 ~ '^[0-9a-f]{64}$'),
    byte_size     BIGINT      NOT NULL CHECK (byte_size > 0),
    mime          TEXT        NOT NULL,
    state         TEXT        NOT NULL DEFAULT 'available'
                              CHECK (state IN ('available','pruned','missing')),
    state_changed TIMESTAMPTZ NOT NULL DEFAULT now(),
    state_reason  TEXT,
    created       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE attachment (
    id          BIGSERIAL   PRIMARY KEY,
    node_id     BIGINT      NOT NULL REFERENCES node(id) ON DELETE CASCADE,
    sha256      TEXT        NOT NULL REFERENCES blob(sha256) ON DELETE RESTRICT,
    filename    TEXT        NOT NULL,
    caption     TEXT,
    retain      BOOLEAN     NOT NULL DEFAULT FALSE,
    uploaded_by TEXT,
    created     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX attachment_node_idx ON attachment (node_id);
CREATE INDEX attachment_sha_idx  ON attachment (sha256);
CREATE UNIQUE INDEX attachment_node_sha_uq ON attachment (node_id, sha256);
```

`attachment_node_sha_uq` is what makes re-upload idempotent: the same bytes on
the same node resolve to the existing row and return `created: false`.

**Blob rows are never deleted — only their files are.** The table is an
append-only ledger. At the 20 GB quota and ~500 KB average that is ~40k rows,
about 8 MB, and it means a markdown reference written a year ago always
resolves to a truthful answer instead of a bare 404.

`uploaded_by` is free-text (`web`, `agent:claude`, `korghelper@kai`) — korg has
no actor model to hook into, and this plan does not add one.

### D-4. Storage root: local primary, nightly mirror

Bytes are written to **`/datastore/korg/attachments`** on kubsdb (local disk,
864 GB free) via a bind mount, and mirrored to **`/gratch/korg/attachments`**
by the existing nightly `korg-backup.sh`, whose recipe already lives in
k-homelab.

Considered and rejected: writing to `/gratch` directly on every save. It puts
NFS in the request path — if gratch is unavailable, pasting a screenshot fails
even though the rest of korg is fine — in exchange for saving one rsync line.
Considered and deferred: dual-write to both on save. Marginal gain over a
nightly mirror; the exposure window is one day of screenshots.

The mirror runs **without `--delete`** in v1. A prune propagating instantly to
the backup would defeat the point of having one. `/gratch` has 12 TB free, so
an accumulating mirror is not a problem worth solving yet; the follow-up, if it
ever matters, is a lagged reaper that removes mirror files absent from the
source for more than 30 days.

Layout: `<root>/<aa>/<bb>/<sha256>` with 2-byte fanout, plus `<root>/incoming/`
for in-flight uploads.

Config: `KORG_ATTACHMENT_ROOT`, `KORG_ATTACHMENT_QUOTA_BYTES`,
`KORG_ATTACHMENT_MAX_BYTES`.

**Note for the deploy skill:** this makes the korg container stateful for the
first time. `docker compose` gains a volume, and "recreate the container
freely" stops being unconditionally true.

### D-5. States, and the 410 contract

- `available` — file is on disk.
- `pruned` — reclaimed deliberately by policy. Expected, not a fault.
- `missing` — a row expects a file that is not there. Disk loss, or a restore
  from a dump older than the prune that removed the file.

Splitting `pruned` from `missing` is the point. Collapsing them into one "gone"
hides the only new failure mode this architecture introduces.

`GET /api/attachments/<sha>` on a non-`available` blob returns **410 Gone**
with the metadata (filename, size, state, when, why) — not 404. The web UI
renders that as a placeholder card ("screenshot.png · 412 KB · pruned
2026-09-02") in place of a broken image. **This rendering is the whole payoff
of keeping the tombstone**; without it the record is invisible and the extra
state buys nothing.

Two distinct removals, deliberately different:

- **Explicit detach** (Ken removes an image from a work item) — the
  `attachment` row is hard-deleted. He is saying it does not belong here; a
  tombstone would be noise.
- **Policy prune** — the file goes, the blob is tombstoned, and every
  `attachment` row survives to render the placeholder.

### D-6. Upload surfaces

1. **Web paste and drag-drop** — the actual feature. A `paste` handler on the
   work-item form and comment box POSTs the clipboard image and inserts
   `![](/api/attachments/<sha>)` at the cursor. `marked` + `dompurify` are
   already in `web/package.json`, so rendering and sanitising are solved.
2. **REST multipart** — `POST /api/nodes/:id/attachments`. Documented as a
   `curl -F` one-liner; this is what agents and scripts use.
3. **MCP — read-only.** Bytes must never cross an MCP tool boundary. A 300 KB
   PNG is ~400 KB of base64, roughly 100k tokens; it would blow out any agent
   context that touched it, the same family of failure that forced
   `survey_work_items` to exist. MCP gets metadata and URLs.

The upload response returns `{sha256, filename, byte_size, mime, url,
markdown, created}` so a client has a paste-ready snippet and can tell a
dedupe hit (`created: false`) from a fresh store.

### D-7. Attachments ride the two-level read contract

This is the requirement that makes the feature useful rather than decorative,
and it is inherited, not invented: sprint 012 established it for comments after
agents kept missing decisions recorded there.

- Collection reads (`survey_work_items`, list) carry **`attachment_count`** —
  the signal that visual context exists.
- The focused read (`get_work_item`) **inlines** the attachment list (filename,
  caption, url, state), capped, with `attachments_truncated` when the cap bites.

An agent following the normal read path then knows a screenshot exists and how
to fetch it. Retrieval is `curl -o /tmp/shot.png <url>` followed by an ordinary
image read — which is the underrated half of #582: Ken pastes a picture of a
broken UI and the agent can actually look at it.

### D-8. Serving and safety

- `GET /api/attachments/<sha256>` and
  `GET /api/attachments/<sha256>/<filename>` (the filename is cosmetic, used
  for `Content-Disposition`; lookup is by sha only).
- Hard MIME allowlist: `image/png`, `image/jpeg`, `image/webp`, `image/gif`.
  **SVG is rejected** — it is script-bearing XML and korg would be serving it
  inline from its own origin. DOMPurify protects rendered markdown; it does not
  protect a direct navigation to a blob URL.
- The declared content type is never trusted. Magic bytes are sniffed
  server-side and a mismatch is rejected.
- `X-Content-Type-Options: nosniff`; `Content-Disposition: inline` for images,
  `attachment` otherwise; `ETag: "<sha>"`;
  `Cache-Control: private, max-age=31536000, immutable`.
- The endpoint is node-agnostic by design (D-2): anyone who can reach korg can
  fetch any blob by hash. korg has no auth and lives behind Tailscale; this
  changes nothing about its exposure.

### D-9. Limits and quota

- **Per-file cap** — 10 MB, applied as `DefaultBodyLimit` **on the upload route
  only**. Axum's default body limit is 2 MB; without an explicit override,
  screenshots simply 413 with no obvious cause.
- **Global quota** — `sum(byte_size) where state='available'`, a trivial
  aggregate at this scale, checked before each store. Reject at 100%, warn at
  80%.
- `axum = "0.7"` in the workspace does **not** have the `multipart` feature
  enabled; it needs adding.

### D-10. Retention, and the clock it needs

Policy (Ken, 2026-07-24):

| Trigger | Age |
|---|---|
| Work item `closed` | 30 days |
| Node `archived` | 90 days |
| Both | 30 days (the closed rule wins naturally, as an `OR`) |
| `retain = TRUE` on any referencing row | never |

An **attachment row** is eligible when `retain` is false and its node satisfies
one of the above. A **blob** is prune-eligible when it has at least one
referencing row and *every* one of them is eligible — so a single live work
item, or a single `retain` flag, keeps the bytes for all of them.

**Schema gap this exposes:** korg records no status- or archive-change
timestamp. `node.updated` is bumped by the `touch_updated()` trigger on any
write, and nothing tracks when `wi_status` became `closed`. "Closed 30 days"
is therefore not currently expressible. Two options:

- Use `node.updated` — zero migration, and the semantics are arguably nicer
  ("30 days since anyone last touched this closed item"). But any bulk write
  resets every clock in the system, which is exactly why migration 0016 had to
  disable that trigger.
- **Recommended:** add `workitem.status_changed` and `node.archived_changed`,
  maintained by triggers that fire only on a real change (`IS DISTINCT FROM`),
  backfilled from `updated`. Small, honest, and useful beyond this feature.

### D-11. Thumbnails are out of scope

korg stores and serves originals and does no image processing. v1 handles the
list-view cost with `loading="lazy"` and a CSS `max-width`.

Resizing belongs to the image-transform microservice Ken has been considering,
integrated **pull-through**: the service fetches `/api/attachments/<sha>` on a
miss, caches, and serves `/t/<sha>?w=320`. korg needs no code for this, and
content-addressing means the cache never needs invalidating. The push shape
(korg calling out on upload) is rejected: it couples the upload path to a
second service and adds a failure mode to the one operation that must not fail.

### D-12. Status page

There is no `/status` route in `web/src/routes/` today. This feature needs one,
and Ken has other candidates for it. Scope here is the attachments panel plus
the JSON endpoint behind it:

`GET /api/attachments/stats` → total bytes and count, bytes held by `retain`,
count by state, orphan count, quota headroom, last sweep time and outcome.

The `retain` number is the one that matters operationally: it is the floor the
sweeps cannot reclaim.

## Write order and sweeps

**Write order is load-bearing.** Stream to `<root>/incoming/<tmp>` hashing as
you go → `fsync` → rename into `<root>/aa/bb/<sha>` → `INSERT INTO blob … ON
CONFLICT DO NOTHING` → `INSERT INTO attachment`. File first, row second, so a
crash leaves an orphan file (always safe to collect) and never a row pointing
at nothing.

Four sweeps, one nightly in-process tokio task (no new systemd unit to deploy),
plus `POST /api/attachments/sweep` for on-demand runs, **`dry_run=true` by
default**:

1. **Reconcile** — stat every `available` blob; absent → `missing`. Run this
   after any restore.
2. **Orphan GC** — files with no blob row and mtime older than 24 h; plus
   anything in `incoming/` older than 1 h.
3. **Retention** — blobs where every referencing row is eligible (D-10):
   delete the file, set `pruned`, record the reason.
4. **Unreferenced** — blobs whose last `attachment` row was detached, older
   than 24 h: delete the file, set `pruned`, reason `unreferenced`. The blob
   row stays (D-3).

## Operational impact

- **docs/operations.md** gains an attachments section: where the root is, that
  the mirror is part of `korg-backup.sh`, and the restore order — **blobs
  first** (rsync back from `/gratch`), then the pg dump, then a reconcile pass.
  Restoring the DB from a dump older than a prune is precisely what `missing`
  exists to describe.
- **k-homelab's `korg-backup` recipe** gains the rsync. It is the source of
  truth for that script; edit it there, not on the host.
- The **"dump should be larger than yesterday's" heuristic survives intact** —
  a direct benefit of keeping blobs out of Postgres.
- **`korg-migrate --reset`** truncates every node but would strand the entire
  blob store. It must refuse, or clear the root, and operations.md's
  destructive-operations section needs the line.
- **MCP tool count is asserted as a magic number in two places** —
  `korg-api/tests/mcp_http.rs:77` and `korg-mcp/tests/server.rs:24`, currently
  47. Both need bumping when the read tools land.
- Contract regeneration: new request/response types go through `korg-core` and
  `just gen`; `just check` fails on stale output.

## Deferred: korghelper

A per-machine **stdio** MCP server (`korghelper`) that takes a local *path*, not
bytes:

- `upload_attachment(node_id, path, caption?)` — reads the file locally, POSTs
  to korg over ts.net, returns the sha and a markdown snippet. Bytes never
  enter the model's context.
- `fetch_attachment(sha, dest_path, max_width?)` — writes to local disk and
  returns the path, so an agent that needs to *look* at a screenshot can, and
  one that only needs it present on disk does not pay for it in context.

This is the right shape and it needs **no korg-side accommodation** beyond what
D-2, D-6 and D-8 already specify — stable sha-addressed REST. It should be
built after the core lands, not alongside it.

The before/after evidence workflow falls out of this: the original capture
(from kdds or wherever) is attached at file time, the fixed-state capture is
attached at close time, and the two are distinguished by `caption`.

## Implementation sequence

**A-1 — core.** Migration (blob, attachment, retention-clock columns and
triggers), storage layer, `POST /api/nodes/:id/attachments`, `GET
/api/attachments/<sha>[/<filename>]`, list, `PATCH` (caption, retain), `DELETE`
(detach), MIME allowlist and sniffing, per-file and quota limits, config, the
compose volume.

**A-2 — surfaces.** Web paste and drag-drop, gallery strip on the work-item
view, 410 placeholder rendering, the two-level read contract (D-7), MCP read
tools.

**A-3 — lifecycle.** The four sweeps plus the nightly task and manual endpoint,
`/api/attachments/stats` and the status page, the rsync mirror in the
k-homelab recipe, operations.md, the `--reset` guard.

A-2 depends on A-1. A-3 is independent of A-2 and can ship either side of it,
but should not lag far behind A-1 — until the sweeps exist the store only
grows.

## Open questions

1. **SHA in URLs: full 64 hex, or accept unambiguous prefixes?** Full is
   proposed (URLs are machine-generated), but it makes markdown noisy;
   git-style prefix resolution at 12+ chars is a cheap add if that grates.
2. **Mirror pruning** — never (proposed), or the lagged reaper?
3. **Status page** — standalone `/status` route, or folded into an existing
   page? Depends on what else Ken wants on it.
4. **Quota and per-file cap** — 20 GB and 10 MB are the working numbers; both
   are config, neither is load-bearing.
5. **Should `resolved` or `done` ever start a retention clock**, or only
   `closed` and `archived`? Only the latter two are specified, which matches
   the status semantics: `closed` means "I am done looking at this."
