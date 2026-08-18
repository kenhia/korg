# 065 — korg quick wins

**Proposal:** korg:1396 · **Items:** #466, #1146, #1398 · all XS, all project `korg`.

Three backlog-tail items bundled because none justified a sprint alone. They
turned out to share a shape anyway: each is a surface that reads worse than the
data behind it, and two of the three had a *stated* fix that was wrong.

The korg+ guiding plan (`cross-project-planning/korg+/PLAN.md`) had already
swept 1396 on 2026-08-17: **proceed, plan-independent**. Nothing here touches a
contract surface the plan names, and no `GP-n` decision needed amending —
re-checked at close-out, not assumed from the sweep.

## #1398 — report sources: the panel led with its own noise

Seven sources on the Daily Reports page, one of them a real reporter, and it
rendered last.

**Ordering.** The key was `is_stale DESC, last_report_date ASC NULLS FIRST`:
right about stale, and then it sorted unrated against fresh *by date*. An
unrated source is unrated precisely because it filed once and stopped — so its
date is always old, and it always won. Six one-off probes therefore sat above
kmon, the only source still filing. Now ranked by freshness bucket:
**stale → fresh → unrated → retired**, oldest-first inside a bucket.

The fix collapsed a duplicated CASE on the way past. Freshness was computed
three times in one query — the SELECT, the `asserts` guard, the ORDER BY — and
ORDER BY couldn't reference an output alias inside an expression, which is what
had forced the third copy. A `rated` CTE resolves it once. Worth noting for its
own sake: `asserts` now reads *"fresh sources assert their status, everything
else asserts unknown"*, which is the #950 rule as written. The old spelling
re-derived freshness from its four parts and happened to agree.

**Data.** Four one-off probes retired with notes (`t3-probe-037`,
`kaed-journal-pass`, `kaed-live-test`, `live-test-014`). `kyac` stays unrated
per sprint 052's warning — interactive, not ended — and now carries a note
saying so, which needed a small UI change: notes only rendered on `retired`
rows, so a deliberately-unrated row could not explain itself. An operator note
now *replaces* the generic "declare a cadence to rate it" advice on unrated
rows, because for kyac that advice is wrong.

`kmon:vmlab-01` was the one genuine decision and went to Ken: two reports 37
days apart, no inferable cadence. He chose **retire**, told that its last
report was `PROBLEM — "the nightly backup service has failed"` and that
retiring silences that permanently. The sprint believed no work item existed
and said so in the source's note; the overseer pass found otherwise. The
failure was kmon sprint 08's **staged drill** (a planted 203/EXEC on the drill
VM), and kmon's investigation loop filed **#1180** in `homelab-health` when it
found the plant — the WI just lived outside the korg project, where the sweep
looked. #1180 is closed out as drill residue and the source note corrected;
nothing real was ever broken, so retiring silences nothing.

## #1146 — the agent variant that was a bigger copy

`img-46b` measured 43,911 bytes of `:agent` variant against a 24,598-byte
original at identical 1019×311 dimensions. Faithful re-encode, no benefit:
`image`'s PNG encoder is simply worse than whatever produced the paste.

**The item's prescribed condition was "already ≤1568px **and same mime
family**". Implemented, that is wrong in the direction that matters.** The
variant encoder picks its format from the pixels, so an *opaque* PNG encodes to
JPEG — a different family — and the mime rule would keep the variant. JPEG at
q82 is bad at flat synthetic colour, which is what UI screenshots are: a
measured 800×600 opaque PNG came out **42,193 bytes of JPEG against 9,878 of
PNG**. Predicting by type keeps the larger file exactly where the bug bites
hardest.

So the rule measures instead: encode the variant, then drop it if the original
fits the ceiling **and** the encode came out no smaller. Costs one throwaway
encode of an image already under 1568px; cannot be wrong about the thing it is
measuring. A test found this, not review — the first version's assertion that
the JPEG conversion "earns its place by being smaller" simply failed, and the
failure was the finding. Both directions are now fenced: the opaque case that
killed the mime rule, and a noisy photographic PNG whose JPEG variant genuinely
shrinks and is kept.

**Read side, no migration.** `attachment_variants` reconstructs a skipped size
from the original's own dimensions via `Variant::fits_original` — the same
predicate `prepare` used — so `/api/img/:id/agent` still resolves and still
serves. *An agent told to fetch the agent variant never learns how korg stored
it*, which was the constraint that ruled out simply dropping the row. The
surface gained `is_original` for the two callers that must know: byte
accounting (don't add it to the original's) and anyone already holding the
original. Variant order is now stable, smallest ceiling first, regardless of
storage.

Purge/discard needed nothing — `Store::remove` is `remove_dir_all` on the
attachment directory, so "agent == original" cannot double-delete or miss. The
existing discard test uploads a 200×200 image, which under this change has no
agent blob, so it already exercises the new shape.

Two consequences recorded in `docs/api.md` rather than left to be discovered: a
skipped variant carries the original's EXIF (that stripping was always
incidental, and the bare-id route has served EXIF-intact originals since the
feature shipped), and only `agent` is ever affected.

## #466 — the EVAL convention (korg's oldest open item)

No code. The 2026-08-15 decision chose option 2 — a designated project with
`category: EVAL` — and explicitly rejected the scratch korg instance, with a
costing, so that it stays rejected.

Created project `eval` (id 57) and wrote `docs/eval-convention.md`: short,
self-contained, and written to be *handed to* an eval agent, which is what Ken
asked for on 2026-08-16. Pointers from `docs/api.md`'s disposal doctrine and
`docs/usage.md`'s category paragraph — the two places that already say `EVAL`.
A standalone page can't be wired into `docs_drift` (that suite reads structured
tables), so discoverability comes from those pointers instead.

**The decision's status half was stale.** Both the option-2 sketch and the
decision said eval work would land in a project at `inactive`/`archived` so the
project rail would hide it. Since #884 that is impossible: `project_id_for_name`
*and* `require_project` refuse an archived project as a write target, name path
and id path both. An archived eval project cannot receive the items it exists to
hold. So `eval` is **active**, containment comes from the category and the
description, and the reason is written into the project's notes and the
convention page — specifically so nobody later "fixes" it by archiving it.

## Follow-ups

- Nothing enforces the eval convention, by design. If residue keeps appearing
  outside `eval`, the next lever is the `origin` field from #466's option 4,
  not a scratch instance.
- `docs/eval-convention.md` has no drift check. Cheap options exist (assert it
  names a live `PROJECT_CATEGORIES` value) but none that would catch the way it
  is most likely to rot — the `eval` project being renamed or archived.

## Deployed

**2026-08-18** · image `kubsdb.encke-wahoo.ts.net:5000/korg:e292e855fed4`
(digest `sha256:8d1c9f4b0de6…`) · rollback target `5b130a7187ad` (sprint 064),
confirmed present in the registry before building.

No migration this sprint, and the check says so: `migrations 28 → 28`, every row
count identical across the deploy (1311 nodes, 944 work items, 250 proposals, 50
projects, 43 reports, 30 cards, 4 links; `node_max` and `seq_last` both 1412).
The revision gate ran inside the deploy rather than after it — the running
container's `org.opencontainers.image.revision` was asserted equal to the commit
built, which is the one check a silently-cached `compose pull` cannot pass.

### Verified live

| Item | Check | Result |
|---|---|---|
| deploy gate | container revision label vs the commit built | `e292e855fed4…` — asserted inside the deploy |
| #1398 | `GET /api/report-sources` | **`kmon` (fresh) leads**, `kyac` (unrated) below it, five retired rows tail — monotonic in stale → fresh → unrated → retired. The same data under the old key put kyac first and kmon last |
| #1398 | deployed bundle, `nodes/4.*.js` | carries `.note??"no cadence korg will believe yet…"` — the branch that lets kyac's row render *why* it is deliberately unrated instead of the generic "declare a cadence" advice |
| #1146 | `POST /api/img`, 1019×311 opaque screenshot (the bug's own dimensions) | `agent` variant returns **`is_original: true`**, 1,914 bytes, `image/png` |
| #1146 | `GET /api/img/img-585/agent` | 200, `image/png`, **byte-identical** to the uploaded original |
| #1146 | blobs on disk | `original.png` + `thumb.jpg` only — **no `agent.jpg`** |
| #1146 | `DELETE /api/img/img-585` | `{"deleted":true}`, directory gone, record 404s |
| — | `GET /plan` deep link | 200 |

**The live upload is the measured-versus-predicted case, by luck of being a
realistic screenshot.** It is opaque, so korg would encode its `agent` variant
as JPEG — a *different* mime family. Under this item's originally-specified rule
("already ≤1568px **and same mime family**") that variant would therefore have
been kept, and at q82 it measures **12,288 bytes against a 1,914-byte original**:
six times the file, stored beside it, for the same pixels. The shipped rule
measures instead and wrote nothing. The thumb tells the same story from the
other side — 4,877 bytes of JPEG from a 1,914-byte PNG source — and is left
alone deliberately, since it is a real downscale the UI renders everywhere.

The test attachment was discarded, so nothing was left in production; that
discard doubles as the live check that a skipped variant cannot double-delete
or miss, since `Store::remove` takes the whole directory.
