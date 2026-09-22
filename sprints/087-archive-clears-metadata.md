# 087 — archiving a project clears its location metadata

Proposal korg:3054, covering **WI 3003**. Slice 10 of program korg:3062
("Low-hanging fruit, run 2"), run as an overseen karc leg on kai.

## Goal

Decide, and implement, who clears an archived project's `src_path`,
`machines` and `deploy_to`: korg at archive time, or kmuster's weekly
`check-projects --fix`.

**Answer: korg, at archive time, inside the transaction that archives.**

## The decision, and why it went this way

Ken ruled on 2026-09-11 (korg:2250) that an archived project carries none of
the three: they answer *where is the source?*, and for an archived project the
answer is "nowhere". The ruling settled the opposite reading `kwi`'s row had
been kept under — that `src_path` records where a tree *used to be*. It records
where it **is**, so it is true or absent and never stale.

That has been *data* since the 2026-09-11 hand sweep of eleven rows, and never
a *rule*. kmuster sprint 003 (korg:2983) landed the detection half as the
`archived_metadata_retained` finding; the auto-fix half did not land, because
it would need kmuster's `ProjectPatch` widened along two axes whose narrowness
is deliberate and stated in two places:

- `machines` — "korg's declaration of record… kmuster never writes it"
  (kmuster's code *and* its `CLAUDE.md` under *Boundaries — hold these*);
- `notes` — "a mechanical checker must not rewrite prose", which is exactly
  what the non-lossy append is.

Doing it korg-side needs neither boundary dissolved. It also makes the field
never wrong, rather than wrong for up to a week, and turns kmuster's new
assertion into what a backstop should be: something that normally finds
nothing and means something when it does.

**The cost, accepted:** korg grows behaviour on archive, and it does not
retroactively fix a row archived by a direct DB edit. That second case is
precisely what kmuster's backstop now covers, so the two halves are
complementary rather than redundant.

## One premise in the brief was wrong, and the ruling survives it

The proposal's `notes` justified doing the clear in-transaction like this:

> korg+ PLAN.md line ~890 records that archived projects refuse writes — a
> later write would be refused by korg's own rule.

It does not. `korg+/PLAN.md:890` reads `(#884 — archived projects refuse
writes)`, a compression written in a sentence about the `eval` project needing
to stay active. WI #884's actual rule lives in `repo/selectors.rs` and refuses
new **work** targeted at an archived project — `resolve_project` and
`resolve_project_patch`, which every create funnels through.
`update_project` is deliberately outside it, and the code says so:

> `update_project` is deliberately *not* one of them: setting `status` is how a
> project comes back.

So a follow-up write would have succeeded. The conclusion is unchanged, for
two better reasons, and they are the ones recorded in the code:

- **Atomicity** — no reader, kmuster's weekly check included, can observe a row
  that is archived and still carries a path, because that state never commits.
- **It cannot be forgotten** — a follow-up write is a caller's responsibility;
  an in-transaction clear is korg's.

Raised to the overseer on proposal korg:3054 rather than inherited silently,
because the next person reasoning about archived-project writes would otherwise
pick up a rule korg does not have.

## What shipped

`clear_location_metadata` in `crates/korg-core/src/repo/projects.rs`, called
from `update_project` when a patch sets `status` to `archived`:

- **Last in the transaction**, after every other field update. A patch that
  sets a path *and* archives in one call comes out archived and unlocated —
  archiving is the later intent — and the `notes` read sees a `notes` written
  by the same patch, so the record appends to what the caller just wrote
  rather than to what it replaced.
- **Non-lossy.** The cleared values are appended to `notes` first, in the
  wording the 2026-09-11 sweep used on eleven rows, so the corpus reads as one
  convention rather than two. One deviation: fields are separated by `;`
  rather than `,`, because the sweep only ever had a single machine to render
  and `machines kai, kubs0` would otherwise read as two fields.
- **Guarded on "is there anything to clear"**, not "is this a transition". So
  re-archiving is a no-op, no paragraph is invented for a project that was
  never located, and a row that went stale by some other route gets swept the
  next time somebody archives it.
- **Scoped to the patch setting `archived`.** Editing an archived project
  without restating `status` clears nothing: korg clears on archive, it is not
  a sweeper. That gap is deliberate and is what kmuster's assertion remains the
  backstop for.
- **`gh_repo` untouched**, as WI 2382 specified. It names the surviving copy,
  and for `kwi` the GitHub repo is the only copy of that code left anywhere.

`PROJECT_STATUS_ARCHIVED` joins `PROJECT_STATUS_ACTIVE` in `vocab.rs` — the
transition is now load-bearing on the write side, not only in the reads'
filters.

### Tests

`crates/korg-core/tests/sprint087.rs`, ten cases. The ones that matter most
are the ones asserting what is *not* touched: coming back to `active` clears
nothing and re-locates cleanly, re-archiving writes no second record, an
unlocated project gets no paragraph, and an archived project edited without
restating `status` keeps what it was given.

Two existing tests used `status: "archived"` as an incidental value while
asserting that the patch surface stores what it is given
(`sprint010::project_metadata_roundtrip`, `sweep::update_project_patches_metadata_by_name`).
That value is no longer neutral. Both now use `active` for the roundtrip and
assert the status roundtrip separately — every assertion they made is still
made — and the MCP one gained coverage that the clear reaches through the tool
surface an agent actually calls, not only through korg-core.

### Docs

`docs/api.md` gains an **"Archiving is not just a status"** section under
`update_project`: the three consequences a caller needs before calling it
(same-call location writes lose, the trigger is the patch and not the row,
coming back is unaffected), and the atomicity-not-refusal correction above.
The MCP tool description says the same in the place an agent reads it.

## Repaired in passing

- **The `update_project` MCP tool description advertised four project statuses
  that do not exist** — `status (active|maintenance|inactive|archived)`. korg
  has had exactly two since `PROJECT_STATUSES` was closed; `maintenance` and
  `inactive` were removed as unused vocabulary, and `validate_status` rejects
  both. An agent reading the tool catalogue would have been told to write a
  value the write path refuses. Corrected in the same description this sprint
  had to edit anyway, and the `docs_drift` / `tools_schema.json` gates prove it.

## Follow-ups

None filed. The kmuster side needs nothing: the whole point of going korg-side
is that `ProjectPatch` stays narrow and both stated boundaries hold. kmuster
WI 2382 is `closed` (terminal, Ken's) and got a comment recording that its
auto-fix half is now answered here rather than left pending in kmuster.
