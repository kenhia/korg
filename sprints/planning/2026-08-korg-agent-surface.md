# korg agent surface — program plan (2026-08)

**Written:** 2026-08-01, planning session on cleo, after T2 (`claude-cleo` #634).
**Goal:** make korg's MCP surface the best it can be for agent use — cheap to read, honest about what it returns, and shaped so the common call is the small one. We own korg; nothing here is blocked on anyone else.
**Canonical location:** this file. It is the plan of record for the program; korg sprint records live in `sprints/NNN-*/` as usual.

**Related plans:** the infra-cleanup weekend plan (`k-homelab/sprints/planning/2026-08-infra-cleanup.md`) is a *separate, closing* program. This one does not extend it — see §6.

---

## 1. The finding that started it, and why it is bigger than two bugs

T2 explored the klams and korg MCP surfaces hands-on (#634, `resolved`; findings doc `agent-skills/docs/2026-08-01-klams-korg-surface.md`). Two korg traps, both silent, both cases where **the server's own instructions mislead**, so neither can be left to the schemas:

- **#851 — `survey_work_items` includes archived rows by default.** Project `klams`: `archived` omitted → **105**; `archived:false` → 104. korg's instructions say *"All exclude archived rows unless you ask for them"*; the tool's schema says the opposite and is correct. This is the tool the instructions **recommend for cross-project sweeps**, so a survey used to size a backlog over-counts with nothing in the response saying so.
- **#852 — `list_proposals` has no `limit`** and returns every summary in full: **110 rows / ~185,500 chars ≈ 46k tokens**, **71% of it `done`**. `survey_work_items` exists precisely because `list_work_items` *"can exceed tool-output limits"*; proposals are heavier per row and have no equivalent. `start-sprint:34` and `refill-queue:46` both call it unfiltered today (**agent-skills #854**) — the latter five lines after correctly warning about this exact hazard for work items.

**These are not two isolated bugs. They are the same defect the 816 program just finished fixing for projects**, and 816's own verdict names it:

> *"the schema is mostly innocent; the read surface is guilty"*
> — #757 field review, quoted in `.scratch/816-result-summary.md`

816 measured where the cost actually lives, and the answer was not what the proposal assumed:

| | 816 proposal claimed | Measured |
|---|---|---|
| Lean vs full `list_projects` | ~2,050 tokens saved | **1,910 chars vs 10,584 — 5.5×** |
| Where the saving comes from | the row filter | **the field projection** — dropping 6 of 10 columns dwarfs hiding 8 of 36 rows |

**That measurement is the single most useful input to this program.** It says: tier the *fields* first, filter the *rows* second. A `limit` on `list_proposals` helps; returning a 200-char summary instead of a 1,700-char one helps far more.

## 2. The shape to generalize — 816 already built it, for one collection

816 shipped a three-tier read surface for projects. **It is the template**, and it is proven: Phase C's behavioural acceptance was 3/3 correct routing from korg alone, with no klams recall.

| Tier | Projects (shipped) | Cost |
|---|---|---|
| 0 — always-on roster | 28 names rendered into MCP `instructions` at `initialize` | ~112 tokens, per-session so it cannot go stale |
| 1 — lean list | `list_projects(status?, detail?)` → `{items, omitted:{archived:N}}`, defaults active-only + lean | 1,910 chars |
| 2 — full row | `get_project(name)` → every column incl. unbounded `notes` | on demand, one row |

Two design decisions inside it are worth carrying verbatim:

- **The `omitted` envelope.** A filtered list that looks bare is how *"there is no such project"* gets concluded from *"you didn't ask for archived ones."* Any defaulted-narrow read must say what it hid.
- **Short contract field + unbounded overflow field.** `description` is a ≤160-char routing contract (CHECK-enforced); long prose moved to `notes`. **Capping the small field destroyed nothing because the overflow field was added in the same change.**

**Ken's intuition matches this exactly, arrived at independently** — summary should be a brief summary, with new field(s) for the bulk; default to the limited set; non-open on request. So the program is not "fix `list_proposals`". It is:

> **Generalize the 816 read-surface contract across every collection korg exposes.**

That framing matters because it turns a pile of tweaks into one contract that new tools must satisfy — and it gives `docs_drift` something assertable to enforce (§5).

### The specific shape, as a starting proposal

Ken's sketch, refined against what 816 actually shipped:

- **`list_*`** — summary projection, **excludes terminal states by default** (`done`/`archived`/`declined`/`closed`), a flag to include them, and an `omitted` envelope stating what was hidden. No detail flag: if you want detail, you want one row.
- **`get_*`** — full row by id/name, detail by default.
- **Object shape** — a short, bounded contract field (what `description` is for projects) plus an unbounded overflow field (what `notes` is). For proposals this means `summary` becomes an actual summary and today's content moves to a new field.

**The proposal-shape change is the one with real blast radius**, because existing `summary` values are 1,500–4,000 chars of genuinely good analysis that must be preserved, not truncated. That is a migration, and it is why it is sized separately below.

## 3. Coverage — what the existing queue already handles, and what it doesn't

Open korg WIs: 16. Live korg proposals: 817, 818, 822, 825.

**Not covered by any proposal — this is the gap this program fills:**

| WI | What | Note |
|---|---|---|
| **#851** | `survey_work_items` archived default | T2. Uncovered. |
| **#852** | `list_proposals` unbounded | T2. Uncovered. The biggest single token win available. |
| **#842** | korg's production DB credential exists only in the running container | Uncovered, and it is a **security** item, not surface work — see §6. |
| **#846** | CI cancels the merge commit's run | Uncovered, XS. |
| **#855** | true-delete `Agent-Plan`/`feedhub`/`loglens` | Uncovered. 816 suggests testing the cheap hypothesis first (`EVAL` + `archived` may already suffice, reducing it to one row). |
| *(unfiled)* | **Proposal object shape** — `summary` holds the whole analysis | **Filed 2026-08-01 as korg #860** (§9). |
| *(unfiled)* | **Work-item read tiering** — `list_work_items` / `survey_work_items` / `get_work_item` against the §2 contract | **Filed 2026-08-01 as korg #861** (§9). |
| *(unfiled)* | **816 §6 staleness work** — `docs_drift` extensions + a `start-sprint` premise check | **Filed 2026-08-01 as korg #862 + agent-skills #863** (§9). 816 explicitly says file them and do *not* fold either into #758. |

**Covered, but with agent value buried inside UI framing — worth surfacing rather than re-filing:**

- **#813** (in proposal 825) adds a `has_handoff` marker and a details flag to list rows. Its own note says it outright: *"agents probably want `has_handoff` and proposal membership on list reads more than the UI does"* — because `survey_work_items` cannot answer *"which of these already has durable context waiting"* today without N follow-up calls. **That is agent-surface work sitting in a UI sprint.** Don't move it; make sure 825's contract change is designed against §2's contract so it is paid for once.
- **#762** (in proposal 817) rewrites how work items load and page. `LIST_LIMIT_MAX` is 500 and the list is truncating **right now** (554 items, 500 returned, oldest-first so the **newest 54 are hidden**). This is the same read surface, from the UI side.

**Covered and out of scope here:** 818 (eval traffic — #466 is korg's slice), 822 (LLM sidecar weekend project).

## 4. Sequence

**Step 0 — retire Phoenix, adopt kprojects.** *(Also commits the `.gitignore` line
that §7's overseer protocol depends on — see the prerequisite note there.)* Ken's call, and better motivated than tidiness: **korg has no `CLAUDE.md` and no `.github/copilot-instructions.md` at all.** We are about to run several agent sprints in this repo. `.phoenix/` is 196K of dead `trace.jsonl` + snapshots, referenced only by `.gitignore` and `.dockerignore`. Delete it, strip the two references, run `/kproject-init` to inject the managed harness block into both instruction files. Also commit the pending `.gitignore` change (`.scratch/`) or drop it deliberately. One small PR, and it makes every later session in this program cheaper.

**Then, ranked. This is the recommendation; the starter session may reorder with reason.**

| Rank | Proposal | Scope | Why here |
|---|---|---|---|
| **1** | **Stop the bleeding** | #852 + #851 | Both S, both pure MCP surface, no migration. #852 is the largest single token win in the store and two skills are hitting it every run. #851 is a one-line default plus an instructions correction. Ship this first because it costs least and pays most. |
| **2** | **817 — the instrument** (existing) | #762, #812, #701 | Its own framing is *"fix the instrument before making more measurements with it."* We are about to change read contracts; a suite that only passes on a toy database cannot tell us whether we broke something. #762 is *also* read-surface work, so this is not a detour. |
| **3** | **The collection read contract** | new WIs: proposal object shape, work-item tiering | The real work. Includes the `summary` → short-contract + overflow-field migration, which must **preserve** existing analysis text. Do it after 2 so the e2e suite is trustworthy. |
| **4** | **The consumers** | agent-skills #854, then the korg block in `agent-skills/claude-md/CLAUDE.md` | Fix `start-sprint` and `refill-queue` **against the new surface**, not the old one — doing this before 3 means doing it twice. The CLAUDE.md korg section (currently 9 lines, added by T2) shrinks or disappears as the traps stop being traps: **the goal is that the global instructions need to say nothing about korg.** |
| **5** | **Re-review** | fresh session | New surface exploration, same method as T2 — call the tools, measure, don't reason from schemas. Feeds the next loop. |

**Opportunistic, unranked, pick up when convenient:** #846 (XS, one line), #855 (test the cheap hypothesis first), and the two 816 §6 staleness WIs once filed.

## 5. The loop, and how it ends

Steps 3→5 repeat: **review → plan → fix**. Exit when a fresh re-review finds nothing that changes agent behaviour — measured, not felt. Two concrete exit signals:

- **The korg block in the global `CLAUDE.md` is empty**, because every trap it documents has been fixed at the source. T2's own rule is the test: *behaviour of a tool goes in its description; only what no schema can say goes in `CLAUDE.md`.*
- **A re-review's findings are all "works as documented"** rather than schema-vs-behaviour gaps.

When it does, **don't stop — drop the rank.** Continual improvement at low priority, revisited when a new tool lands or a session hits friction.

**Make it self-enforcing rather than remembered.** 816 §6 found the durable answer already: `docs_drift` fired four times during that program and was right every time. Once §2's contract is written down, extend `docs_drift` to assert it — every collection read has a documented default, every defaulted-narrow read carries an `omitted` envelope, every bounded field has an overflow sibling. **A test that fails CI beats a convention a session might skim**, and it is what stops this program from needing a third pass.

## 6. Boundaries — what this program is not

- **It does not extend infra-cleanup.** That program is closing; its post-review runs next and its S6/observability package drops to opportunistic (proposal 819 already exists and simply stays in the queue at low rank — nothing to create).
- **#842 is security, not surface.** korg's production DB credential existing only in the running container belongs with the secrets work the infra plan's S4 handled for other hosts. It is listed here because it is an uncovered korg WI, but it should be triaged as infra, not folded into a read-surface sprint.
- **Not a UI program.** 825 and 817 own the UI. This one touches the web surface only where a shared contract forces it.
- **Not a schema rewrite.** 816's verdict stands: the schema is mostly innocent. The proposal object shape is the one place a field change is warranted, and it is additive plus a migration, not a redesign.

## 7. How this program is driven — two panes, one working tree

Ken runs this from **VS Code with the Claude extension, Remote-SSH'd into kai**, so
both panes execute *on kai* with native access to `~/src/tools/korg`. Two sessions
in the same workspace:

- **Overseer** — holds the program, decides what happens next, writes the korg data.
- **Worker** — executes one sprint at a time.

**They share a git working tree, a `target/` dir, and this file. That is the hazard,
and the division below is not optional.**

| | Overseer | Worker |
|---|---|---|
| Git branch / commit / stash | **never** | **owns it exclusively** |
| Source, tests, web, migrations | reads freely, **never writes** | writes |
| **`.scratch/overseer/`** | **writes freely** — its own scratch space | leaves it alone |
| **`sprints/review/`** | **writes freely** — its checked-in output | commits it (see below) |
| `cargo` / `just check` | **does not run them** — a second build blocks on the same `target/` lock and stalls both panes | runs them |
| korg MCP (WIs, proposals, comments) | **primary author** | updates only the WIs its own sprint covers |
| This plan file | **decides** what should change | **makes and commits** the change, in its sprint PR |
| Deploys to kubsdb | never | only via `sprint-ship` Phase 7 |

**The rule that prevents the collision:** the overseer's output is **korg data,
instructions, and files under those two paths** — never a commit. An overseer editing
source in a tree the worker has mid-branch is how you get a sprint PR carrying changes
nobody meant to ship.

### The two overseer write paths

**`.scratch/overseer/`** — anything transient: things Ken may want to copy out, notes,
intermediate output. It must never reach a commit.

> **Prerequisite: `.scratch/` must be gitignored, and today it is not.** korg's
> `.gitignore` carries an **uncommitted** line adding it. Until that is committed,
> anything the overseer writes there shows up as untracked in the worker's tree.
> **Step 0 must commit that line, not drop it** — this protocol depends on it.

**`sprints/review/`** — checked-in output: reviews, findings, anything with a reader
after this program ends. The overseer writes the file; **committing it happens one of
two ways**, and both are safe:

- The **worker** adds it explicitly in its next sprint PR, or
- The **overseer** commits *only that path* — but **only when the tree is clean and on
  `main`**, i.e. no sprint is in flight. One working tree means one branch, so the
  overseer cannot have its own branch while the worker holds one.

### The rule that makes all of this safe

**Workers must never `git add -A`, `git add .`, or `git commit -a`. Add explicit
paths.** Every collision above degenerates to this one: a blanket add sweeps whatever
the other pane happened to be writing into a sprint PR. Explicit paths make the two
panes independent even when they write simultaneously.

**Before starting a worker sprint, the tree must be clean and on `main`** — modulo the
overseer's two paths, which are expected to be dirty or ignored. If source files are
dirty, the previous sprint didn't finish; resolve that rather than branching over it.

Reads never conflict, so the overseer should read code freely. It is only writes to
**shared** paths — source, git state, and builds — that are exclusive.

## 8. Open questions for the starter session

1. **Terminal-state vocabulary differs per collection** — work items are `open`/`resolved`/`done`/`closed`; proposals are `proposed`/`active`/`done`/`declined`; projects are now `active`/`archived`. "Exclude terminal by default" needs a per-collection answer, and `resolved` is genuinely ambiguous (implemented, but Ken may still want to see it).
2. **Does `survey_*` survive?** If `list_work_items` becomes lean by default, `survey_work_items` may be redundant — or may become the canonical lean read with `list_*` retired. Decide before building, and note #851 is a bug in whichever survives.
3. **Naming for the proposal overflow field.** `notes` matches projects and is the boring, consistent choice.
4. **Does the roster idea generalize?** Projects got a Tier-0 always-on roster because 28 names are ~112 tokens. Nothing else is that small — but "active proposals" might be, and it is the thing `start-sprint` reaches for.

## 9. Program start log — 2026-08-01 (starter session)

### Shipped

- **Step 0 done** — PR #37, squash-merged `c5f756f`, sprint record
  `sprints/033-step0-kproject-harness.md`. Phoenix gone, harness block +
  Project section in both agent files, roadmap seeded, `.scratch/` line
  **committed** (§7's prerequisite). Deployed to kubsdb (procedural — no code
  changed; counts identical to baseline).
- **agent-skills #854 pulled forward from rank 4 and shipped** (Ken's call) —
  PR agent-skills#10, both skills now pass `status` (two calls each), deployed
  to all three hosts the k-homelab way, md5-verified, WI `done`. Rationale:
  every worker sprint in this program starts with `start-sprint`, so each
  pickup was paying the ~46k-token call. The deeper consumer rework stays at
  rank 4 (proposal 867 / WI #864) — these filters are forward-compatible with
  any #852 outcome.

### Filed

korg **#860** (proposal object shape — bounded `summary` + `notes`, migration
preserves every char), korg **#861** (work-item read tiering — merged lean
list, terminal-excluded default, `omitted` envelope), korg **#862**
(`docs_drift` extensions), agent-skills **#863** (`start-sprint` premise
check). Plus agent-skills **#864** (consumer rework / CLAUDE.md korg block to
empty) — not among §3's four, but rank 4 had no coverable WI without it.

### Ranked — §4's order kept, made real in the queue

865 (**4.1**, #851+#852) → 817 (**4.2**, moved from 5, premise re-verified) →
866 (**4.3**, #860+#861) → 867 (**4.4**, agent-skills, #864) → 868 (**4.5**,
re-review, covers nothing by design). Contiguous block above korg's remaining
queue (818 at 6, 825 at 6.5), below the three non-korg proposals at ranks 1–3
— "above korg's existing queue" read as within-korg precedence, not as jumping
other projects' work. No reordering of §4's recommendation was warranted: the
starter session's measurements only strengthened it. All five summaries are
written in the shape #860 will enforce (≤500-char routing contract, analysis
in the covered WIs).

### §8 answers

1. **Terminal vocabulary, per collection:** work items exclude `closed` +
   archived by default; `resolved` and `done` stay visible (`done`'s
   visibility is already a schema promise, `resolved` is Ken's
   may-want-to-see, and together they are 7 rows of 574). Proposals exclude
   `done` + `declined` (71% of the payload). Projects: shipped in 032.
2. **`survey_work_items` does not survive.** `list_work_items` becomes the
   lean read (survey's projection + terminal/archived defaults + `omitted`
   envelope); survey retires after a deprecation window (#861). #851 is
   therefore a bug in the *transitional* tool — fixed cheaply at rank 1, made
   unreachable at rank 3.
3. **Overflow field name: `notes`**, matching projects (#860).
4. **Roster: deferred to the rank-5 re-review, with reason.** Proposed+active
   titles today ≈ 1.3k chars ≈ 330 tokens *per session*, paid by every
   session including the majority that never pick a sprint; a lean filtered
   list costs about the same *on demand*. Re-measure after 865 ships the lean
   read — the roster only wins if pickup frequency rises a lot.

### Found on contact with the code and data (the 816 pattern, again)

- **A new gap in T2's class:** `update_work_item`'s schema says `closed` is
  "hidden by default", but nothing on the MCP surface hides terminal items —
  `list_work_items` returns all 445 closed rows (78% of the corpus). Folded
  into #861 rather than filed separately.
- **§2's "1,500–4,000 chars" undersold proposal summaries:** measured p90 of
  `proposed` is 4,864, max 5,710 (production DB; full distribution in #860).
- **#762's truncation grew as predicted:** 574 non-archived items, newest ~74
  hidden (was 554/54 when 817 was written).
- **#846 is half-verified live:** this program's own Step-0 ship was the first
  since agent-skills' Phase 7.3 change — the `[skip ci]` deploy record started
  no run and the merge commit completed CI with `success`. Remaining scope is
  korg's one-line `cancel-in-progress` scoping (still unscoped, ci.yml:47).
- **#842 flagged for infra triage** by comment (§6 boundary holds; not folded
  into any surface sprint).

## 10. Ranks 4.1–4.4 landed — 2026-08-01/02

- korg sprints **034** (865: #851+#852), **035** (817: #762/#701/#812), **036**
  (866: #860+#861, migration 0021 — 96 summaries moved to `notes`, every
  character verified preserved) each shipped and deployed to kubsdb same-day.
  865/866 `done`; all six covered WIs `closed`.
- **867** (agent-skills #864) merged as `b322715`: the `## korg` CLAUDE.md
  section deleted **whole**, start-sprint/refill-queue/plan-status reworked
  against the new reads. plan-status's fallback was a live defect — the lean
  default hid 26 of homelab-ai's 31 items from the frontier computation.
  Record: `agent-skills/docs/2026-08-01-korg-consumers-sweep.md` (kubs0).
- **Fleet deploy 2026-08-02** (overseer, via k-homelab routing): the three
  skills md5-identical to the repo on kai/kubs0/cleo; the composed CLAUDE.md on
  all three hosts carries no `## korg` section. **§5's block exit signal is
  live on the fleet**, pending 868's independent confirmation.
- **korg #871** filed (delete the `survey_work_items` deprecated alias — 036
  D-7's follow-up, which had no WI) and un-gated by the deploy. Recommended
  before 868 so the re-review sees the end-state surface.
- **Next: 868** (rank 4.5) — fresh session, not one that built any of this.
  Remaining opportunistic: #846 (korg half only), #855, #862, #863; 825 carries
  the row-contract markers (036 D-8).
