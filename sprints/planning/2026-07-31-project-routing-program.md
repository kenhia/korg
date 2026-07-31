# Project routing metadata — program plan

Proposal `korg:816`, marked `active` 2026-07-31. This is **not one sprint.** It
is a program spanning at least two korg branches and two external sessions, and
this document exists so the context survives the gaps between them.

Written during the 031 session, before any engine work, because the planning
itself turned up structure the WIs do not record.

## The thesis, unchanged

Routing a work item to the right project is done from agent memory, not from
korg. Measured 2026-07-28 routing "install Go on kai and kubs0" out of kprojects
#741 → k-homelab #750: `list_projects` returned 34 rows, ~4.5k tokens, and most
`description` fields were null. The call contributed nothing; the routing came
from klams recall. An agent without that context routes on name-guessing.

## Two findings that restructure the WIs

### 1. There are not three data passes. There is one.

Three WIs each carry a "go look at the projects and write correct values" half,
and they are the same operation:

| WI | Its data half | Rows |
|---|---|---|
| #675 | backfill null `src_path` — "needs Ken's knowledge of where each lives" | 11 null |
| #757 | backfill `description` on active projects | 11 null of 28 active |
| #758 | visit each project's source, verify path/repo/description still true | all |

Each requires the same act: **visit the project's actual source and read it.**
Doing that three times is the waste, not the schedule. So the backfill halves of
#675 and #757 are discharged by **#758's first run in fix mode** — which is what
#758 already asks for ("prefer auto-fix over reporting where the correct value is
unambiguous").

### 2. `#758 depends_on #757` is too coarse and hides the real order.

#758 depends on #757's **convention decision**, not on #757's **backfill**. The
backfill depends on #758's **checker**. Not circular — interleaved:

```
757.1 decide the semantics
  └→ 757.2 korg engine supports them
       └→ 758 build the checker (external, claude-cleo)
            └→ 757.3 = 758's first run, in fix mode
```

The current single edge cannot express this, which is why the work looked like it
needed coordination that it does not.

## What is korg-engine work, and what is not

**Engine** means a change to this repo — Rust, migrations, tool schemas, web.

| Work | Engine? | Where it runs |
|---|---|---|
| #675 rename `cn_path` → `src_path` | **yes** | this repo |
| #675 mechanical value fixes (4 rows) | **yes** — rides the migration | this repo |
| `category` casing normalization | **yes** — one migration | this repo |
| #757.1 field review / lean-shape analysis | no | fresh external session |
| #757.2 tier the read surface | **yes** | this repo |
| #673 reword `project` param | **yes** | this repo |
| #674 roster into MCP `instructions` | **yes** | this repo |
| #675 null `src_path` backfill | no — data | 758's first run |
| #757.3 `description` backfill | no — data | 758's first run |
| #758 build the checker | no | claude-cleo |
| Behavioural acceptance test | no | fresh session, no context |

Note what this table says about #758: **none of it is korg-engine work.** Its
mechanical half shells out to hosts (does `src_path` exist on the declared
machine, does `gh_repo` resolve); its judgment half reads repo READMEs. It writes
findings back through the MCP surface like any other client. It was only ever "a
korg WI" because korg's data is its *subject*. It moves to `claude-cleo`.

## Phases and gates

### Phase A — unblocked engine work (branch 031)

Nothing here waits on anything. The analysis in gate 1 explicitly excludes all of
it: #757's review says the `src_path` rename and the `category` exclusion signal
are "already decided, do not re-litigate."

- **#675 part 1 — the rename.** `cn_path` → `src_path`: Postgres column +
  migration, MCP tool schemas, `list_projects`/`create_project`/`update_project`
  shapes, web client, and any skill or doc naming the field. Grep before assuming
  the blast radius is small.
- **#675 part 2 — semantics into the field description.** `src_path` = the
  working copy on the project's **development** machine (the `machines` entry),
  not the deploy target. korg's own row is the example: `machines: ["kai"]` vs
  `deploy_to: ["kubsdb"]`. The absence of this statement is what let the field
  drift; say it in the schema so the next agent to populate a row does not guess.
- **#675 part 3 — the 4 mechanical value fixes**, same migration, no judgment
  needed: `klams` `src/ai/klams` and `kpidash` `src/kpidash` missing `~/`, `korg`
  absolute (`/home/ken/src/tools/korg`), `gratch` trailing slash.
- **`category` casing normalization.** Live values are `Ops`, `Infrastructure`,
  `Dashboard`, `Other`, `EVAL`, `AI`, `Fun`. **#674's `category = 'fun'`
  exclusion matches nothing as written** — what #674 calls a "small remaining
  cleanup" is load-bearing for its own filter. Do it here, ahead of #674.
- **The invariant check.** Every non-null `src_path` starts with `~/`, no
  trailing slash, no whitespace, no parentheses — as a query or constraint, so
  the field cannot silently re-drift.

Deliberately **not** in Phase A: `kcard`'s prose-in-a-path-field
(`~/.archive-src/kcard (kai; archived 2026-07-09, was ~/src/tools/kcard)`),
`feedhub`'s eval-run path, and the 11 nulls. Those need judgment or a source
visit, so they belong to the data pass. `kcard` and `feedhub` are both archived,
so nothing routing-facing reads them in the meantime.

**Ship and merge 031 here.** Three reasons, the third being the real one:

1. It is independently correct and independently valuable.
2. It removes the question of whether a branch survives two external sessions.
3. The rename touches a migration, published tool schemas and the web client at
   once. If its blast radius has a miss, you want to find that in isolation —
   not tangled with a roster and a new read surface.

### GATE 1 — external analysis session (BLOCKING)

**Blocks:** #757.2, #674, #673 (only because it must ship with #674), #758's
assertions, and the data pass. Everything downstream, in other words.

**Does not block:** all of Phase A.

The deliverable is a decision on what the project read surface should be. Filed
as a comment on #757, or as a file-based handoff if the reasoning runs long.

Do it in a **fresh session**, not this one. Not for model reasons — this session
has absorbed the whole chain of prior reasoning and would tend to ratify it,
where the point of the review is to be able to say "this field should not exist."
Ken's call on tier: Fable or Opus, judgment task with real blast radius.

The prompt must carry the field inventory below so the session does not spend its
budget re-deriving it.

**What it must decide:**

1. **The tiering.** Three consumers ask three different questions:

   | Tier | Surface | Question |
   |---|---|---|
   | 1 | MCP `instructions` roster (#674) | "what projects exist" — always-on, ~70 tok |
   | 2 | `list_projects`, lean default | "does this belong here?" |
   | 3 | `get_project(name)` | "tell me everything about this one" |

   Tier 2 is the saving: `id`, `gh_repo`, `src_path`, `machines`, `deploy_to`,
   `category` answer *where it lives and how it builds*, not *does this belong
   here*. Dropping 6 of 9 fields across 35 rows dwarfs any row filter.

2. **The status default.** `list_projects` returns all 35 rows including 4
   archived and 3 inactive. The token saving from filtering is marginal and is
   **not** the argument. The argument is correctness: korg's own description says
   it *"replaces kwi and kcard"*, and both of those archived rows are presented
   to a routing agent identically to live projects. They are precisely the rows
   that produce a confident wrong route. Needs a param to opt them back in.

3. **Whether `description` serves two consumers or needs a sibling.** A human in
   the web UI and an agent budgeting tokens in #674's always-on roster want
   different things. Decide: one field with a hard budget, or `description`
   (one line, routing, budgeted) plus a longer `summary` (deep view, unbudgeted).
   A new column is a small migration if the answer is two.

4. **The token budget for `description`**, given #674 renders it into every
   session unconditionally.

5. **Whether "what does NOT belong here" is prose convention or its own field.**
   This is the thing that actually disambiguates. "Install Go on the hosts" looks
   plausible in both kprojects and k-homelab; what separates them is *"harness
   conventions, not machine state."* A description that only says what a project
   *is* does not route.

**Out of scope — decided, do not re-litigate:** the `src_path` rename and
canonical form (#675, shipped in Phase A); `category` as #674's exclusion signal
and its casing (Phase A).

**The escape hatch must survive the lean default.** #758's checker needs full
metadata for every project — it wants `list_projects(detail: "full", status:
"all")`, not 35 `get_project` calls. And the **REST surface should keep the full
shape**: `korg-api`'s `/projects` feeds the web client, which colors the project
rail by `category` and greys inactive/archived rows. Lean applies to MCP only.
That split is not a special case — it is the answer to decision 3.

### Phase B — post-analysis engine work (branch 032)

- **#757.2** — implement whatever gate 1 decided: lean `list_projects` + status
  filter, `get_project` exposed over MCP, any schema change the review calls for.
- **#673 + #674 together.** Never separately: #673 alone is a regression. It
  tells an agent "pass a name you're confident in"; with no roster supplying that
  name it invites a confident *wrong* guess → `not_found` → `list_projects`
  anyway, which is worse than today.
- **Write the contract handoff for #758** and attach it (`has_handoff`). It
  carries: the agreed field semantics, the canonical `src_path` form from Phase
  A, the new tool contract, and #674's warning below.
- **Move #758 to `claude-cleo`.** One call — `update_work_item` supports a
  project move and clears an area that no longer belongs to the target
  (`crates/korg-mcp/src/tools.rs:186`).

At the end of Phase B, **korg's engine work for this program is done.**

### GATE 2 — external build + data pass (NON-BLOCKING for engine)

**Blocks:** the behavioural acceptance test, and #757 closing honestly.
**Blocks no further code in this repo.**

A `claude-cleo` session builds #758's checker and runs it once in fix mode over
the 28 active projects. That single run discharges #675's 11 null `src_path`
rows, #757's 11 null `description`s, `kcard`'s prose-in-a-path, and `feedhub`'s
eval-run path.

Split #758's own build, because its scheduled home is a separate decision: the
**checker logic** is what this program needs; **where it runs on a timer** (kmon
vs. a korg unit) is tied to #581 and should not gate anything here.

Carry forward verbatim, from #674: the backup job on **kubsdb** is
`korg-backup.timer` → `korg-backup.service`. **Never append to
`korg-backup.sh`** — `Type=oneshot` means an analysis bug fails the *backup*
unit and raises an alarm for a backup that actually succeeded. A separate unit
ordered `After=korg-backup.service` if it goes there at all.

### Phase C — verification and close-out

Not more engine work. Run the acceptance test #757 specifies, which is
**behavioural, not a non-null column**: hand a fresh session with no prior
context three routing cases — a host install, a harness convention change, a
skill edit — with only `list_projects` output, and see where they land. If they
still need klams, the descriptions are not doing their job.

#673's test has the same shape: a fresh session files a WI against an inferable
project **without** a preceding `list_projects`.

## Field inventory — measured 2026-07-31, live

Carry this into the gate 1 prompt.

**Current surface.** `list_projects` is `tool::<NoArgs>`
(`crates/korg-mcp/src/tools.rs:224`) → `repo::list_projects`
(`crates/korg-core/src/repo.rs:1805`), no WHERE and no field selection.
`PROJECT_SELECT` (`repo.rs:1801`) is `id, name, gh_repo, cn_path, description,
status, machines, deploy_to, category` — 9 columns × 35 rows, always.

**`get_project` already exists in Rust** (`repo.rs:1812`) and is **not exposed
over MCP.** Only `list_projects`, `create_project` and `update_project` are. Tier
3 is a wiring job, not a new query.

**Row counts:** 35 projects — **28 active, 3 inactive, 4 archived, zero
`maintenance`.** #757's text says "~25 active/maintenance rows"; it is 28 active.

**Null `description`:** 13 overall, **11 of the 28 active** — `Agent-Plan`,
`apt-temps`, `ATV`, `gh-kenhia`, `homelab-ai`, `kagent`, `kmon`, `k-homelab`,
`korg-dash`, `kvscf`, `kwebi`. Several more are present but too thin to route on
(`"EVAL"`, `"Homelab configuration"`, `"Ken's Work Items"`).

**Null `cn_path`:** 11 — `Agent-Plan`, `apt-temps`, `ATV`, `claude-cleo`,
`gh-kenhia`, `homelab-ai`, `homelab-health`, `korg-dash`, `kvscf`, `kwebi`,
`loglens`. Some are legitimately null (`ATV`, `apt-temps` may have no checkout);
null stays null rather than being invented.

**`category` values in use:** `Ops`, `Infrastructure`, `Dashboard`, `Other`,
`EVAL`, `AI`, `Fun` — mixed case throughout, not just `trt-llm-explore`.

## Economics, for whoever questions the whole thing

`list_projects` is ~2,050 tokens, paid by every session that files a work item. A
names + descriptions roster in the MCP `instructions` block is ~70 tokens, paid
always. Break-even is **~4.4%** — one filing session in 23. With `refill-queue`,
`start-sprint` and `plan-status` all touching korg, the real rate is far above
that. Server instructions are the right home specifically because that block is
already loaded unconditionally, so the roster extends an existing always-on
budget rather than opening a new one.

## Operational notes

**`.korg-sprint-proposal` holds `korg:816` and `sprint-ship` will close the
proposal when the branch merges.** #816 is a program record spanning several
branches, so it must **not** be closed at the end of 031 or 032. Either remove
the marker before shipping those, or tell `sprint-ship` to leave the proposal
`active`.

**agent-skills #751 was dropped from #816** at sprint start (`rel_id` 524
removed) — a proposal cannot span two repos. Not descoped, re-sequenced; the
rationale is recorded as comments on both #816 and #751, and #751's `related-to`
#757 edge is intact. It is the *consumer* of this program, and #674's roster may
make part of its routing table redundant.

## WI restructure — applied 2026-07-31

- **#757 split.** It keeps only the field review and is now `research`/S, retitled
  *"Field review: what should korg's `project` record hold, and what must
  `description` say?"* Its measured field inventory is written into `details` so
  the gate-1 session does not re-derive it.
- **#828 created** (korg, feature/M) — *"Tier the project read surface: lean
  `list_projects` with status filter, expose `get_project` over MCP"*. Genuinely
  new scope that #757's text never covered. Added to #816's `covers`.
- **#675 narrowed** to the rename, the semantics statement and the 4 mechanical
  value fixes — what Phase A ships. Its null/judgment rows join the data pass.
- **#758 moved to `claude-cleo`** and removed from #816's `covers` (`rel_id` 523),
  for the same reason #751 was. Its first run in fix mode over the 28 active
  projects is now an explicit acceptance criterion, not a side effect.
- **Edges re-cut:** `#828 depends_on #757`, `#758 depends_on #828`, `#675
  related-to #758`. The pre-existing `#758 depends_on #757` became *correct* once
  #757 was narrowed to the review — the coarseness dissolved via the split rather
  than via edge surgery, which is the cleaner outcome.

**#816 now covers five korg WIs and nothing else:** #673, #674, #675, #757, #828.
The proposal is the korg-engine scope; this document is the program.

Notes were left on #674 (the `category` casing normalization is a prerequisite for
its own filter, and its *rendering rule* — not just its data — is gated on #757),
and on #758 (why it moved, the three-passes-are-one finding, and the
`korg-backup.sh` warning carried forward so a project move cannot lose it).
