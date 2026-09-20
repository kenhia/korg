# 086 — korg's half of the project connector

**Proposal:** korg:2915 · **Covers:** #2874 · **Branch:** `086-project-connector`
· **Program:** korg:2916 (slice 1 of 2) · **Overseen**

## Goal

kctrldeck's Code tab (WI 2853) wants every live korg project as a closed,
launchable row: add a project to korg and it appears on the panel, archive one
and it goes, with no hand-kept favorites. It consumes a source-neutral JSON
listing; korg is the first source.

Two pieces, and the order is load-bearing — without the first, the endpoint
cannot describe cleo at all, including kctrldeck's own repo.

## Premise check

Every claim in WI 2874 and the proposal's notes checked out, with one
correction worth carrying.

- **`src_path` has a CHECK constraint, `~/`-relative.** Holds.
  `project_src_path_canonical` (0019), `^~/[^[:space:]()]*[^[:space:]()/]$`,
  added `NOT VALID`.
- **`GET /api/connectors/projects` does not exist.** Holds — no occurrence of
  `connectors` anywhere in `crates/` or `web/src`.
- **`/api/projects` returns everything the endpoint needs.** Holds:
  `repo::list_projects` selects the full `ProjectRow`, `src_path`, `machines`,
  `starred` and `category` included.
- **cleo's projects carry NULL.** *Drifted, and the drift matters.* The
  proposal names **five**; it is **four** — `kbrickshoot`, `kctrldeck`,
  `kpidashclient-win`, `krcmd`. The fifth, `agent-skills`, has a perfectly good
  `src_path` (`~/src/ai-agents/agent-skills`) and `machines: [kubs0, cleo]`;
  what it has is a *second* clone korg cannot describe, which is a different
  problem and the reason `locations` is a list. v1 emits its `machines[0]`
  (kubs0) and is correct to. Six further projects carry NULL `src_path` and no
  machines at all — they are not repos, and the endpoint's filter excludes
  them. WI 2874 also named `kxeneon` and `korg-vs`; the first is archived and
  the second has no project row, exactly as the notes said.
- **The four paths.** Verified on cleo rather than assumed — `ssh cleo` (rc 0,
  a real listing, not an inferred reachability) shows `D:\ClaudeWorks\` holding
  `kbrickshoot`, `kctrldeck`, `kpidashclient-win` and `krcmd`, plus `korg-vs`
  which has no korg row.
- **kmuster reconciles against `src_path`.** Holds; see below.

## Cross-project plan

korg routes to `korg+/`. Grepped: the register says nothing about `src_path`,
connectors or project listings, and no `GP-n` constrains either piece.

**Decided: this does not enter the register, and that was the question the
proposal left open.** The korg+ cluster is korg / kfdc / kfo, and GP-1's rule
("agents curate korg; the board renders korg") governs *consumers of korg's
board data*. kctrldeck is neither a cluster member nor a board — it is a deck
that opens folders, and the thing it consumes is a source-neutral protocol korg
happens to implement, not a korg contract. Registering it would put a decision
in the cluster's register that binds nothing in the cluster. **No amendment.**

Worth noting for whoever revisits it: the endpoint *is* a published contract,
and it says so — via its own `version` field, which is the mechanism WI 2874
specified.

## What shipped

### 1. `src_path` learns the drive-root form

`/d/ClaudeWorks/kctrldeck` <-> `D:\ClaudeWorks\kctrldeck`. Ken's proposal, and
the right one: it is the MSYS/git-bash spelling an agent recognises, it cannot
collide with anything a previous write could have stored (a POSIX absolute path
was never valid here), and it converts mechanically both ways — which is what
lets the endpoint emit a native path without korg storing one.

- **`crates/korg-core/migrations/0036_src_path_drive_form.sql`** — the widened
  CHECK, `^(~|/[a-z])/[^[:space:]()]*[^[:space:]()/]$`. The separator after the
  drive letter is the load-bearing character: without it `/home/ken/x` parses
  as drive `h` and every absolute POSIX path 0019 normalised away comes back.
  DDL only — see "Not in this change" on why the backfill is not here.
- **`repo::check_src_path`** — mirrors the constraint app-side so a correctable
  input stays `invalid_input` (#887's rule). A **native Windows path gets its
  own remedy**, computed from the value sent: `D:\ClaudeWorks\kctrldeck` is
  answered with "write `/d/ClaudeWorks/kctrldeck`", not with "write it relative
  to home", which would point at a home directory that does not exist on that
  host. That is the input an agent holding a cleo checkout will try first.
- Field docs on `ProjectRow` and `ProjectPatch` (so the MCP schema and the
  TypeScript carry it), plus `docs/api.md`.

### 2. `GET /api/connectors/projects`

`crates/korg-api/src/connectors.rs`. Active projects with a `src_path` and at
least one machine, emitted as `{connector, version, generated, projects}`.

**The endpoint's whole added value is that `path` is absolute and native to its
host.** korg expands `~/` and converts the drive root back to `D:\…`, so no
consumer learns korg's storage conventions to open a folder. Everything else is
a projection of `GET /api/projects`.

Three decisions inside it:

- **`machines[0]` is the host.** `machines` is a list, `src_path` is singular
  and documented (#675) as the *development* machine's copy — so `machines[0]`
  is the one host the path is defined against. `locations` stays a list anyway,
  so a v1 consumer needs no shape change when korg learns to describe the
  second clone `agent-skills` already has.
- **Omissions are silent.** A project with no path or no machine is one whose
  metadata nobody filled in; failing the listing over a blank field would take
  the panel down. `category` is omitted rather than null, so a consumer
  grouping by it gets no group rather than a group called `null`.
- **Not in `contract/read-shapes.json`.** That document is generated from
  korg's advertised *MCP collection reads* and asserts it describes those and
  nothing else — adding a REST-only projection would fail its own `phantom`
  check. `version` is the contract mechanism here, as WI 2874 specified.

`KorgConfig::now()` is new and public: `generated` is a response field, and a
surface reading `OffsetDateTime::now_utc` directly is one the harness's pinned
clock cannot reach. `local_today` now goes through it.

### Tests

`crates/korg-core/tests/sprint086.rs` (5) and `crates/korg-api/tests/sprint086.rs`
(7), plus unit tests beside the conversions. The ones that earn their keep are
the negative cases: `/home/ken/...` and `/usr/local/...` still refused,
uppercase drives refused, every 0019 rule (trailing slash, whitespace,
parentheses) shared by the new form rather than waived for it.

## Repaired in passing

- **Four cleo projects' `notes` asserted the opposite of what now ships.**
  Exposed by the backfill itself: `kctrldeck` said "src_path stays null",
  `kbrickshoot` "src_path can't hold a Windows path", `kpidashclient-win` and
  `krcmd` "the `src_path` field takes only `~/`-relative POSIX paths, so it is
  recorded here". Each now names its drive-root value and the migration that
  allows it. Not cosmetic — an agent reading the old line would conclude the
  field must be NULL and undo the backfill. Proved by re-reading all five cleo
  rows off the deployed instance: `src_path` set, no stale claim left.
- **`project_src_path_canonical` promoted from `NOT VALID` to validated.** 0019
  added it unvalidated for one row — `kcard`, holding a sentence of archive
  history — and said to promote it "when the pass that owns it lands". That
  pass landed (kmuster sprint 002); `kcard` is archived with a NULL `src_path`,
  and all 62 rows on kubsdb conform (measured over `GET /api/projects`, which
  filters nothing). 0036 had to drop and re-add the constraint regardless, so
  `NOT VALID` or validated was a choice that could not be avoided, and the
  evidence removed it. A precheck `DO` block names any offending row before the
  promotion, so a database this migration has not seen fails legibly instead of
  with a bare constraint violation.

## Cross-repo changes made

None. See below — kmuster was checked, not changed.

## Follow-ups filed

Both name the decision that made them items rather than repairs.

- **kmuster #2918** — teach the reconciler the `/d/...` form. Checked before
  shipping and **nothing breaks today**: `declared.rs`'s matcher runs
  `expand_tilde`, which only rewrites a `~` prefix, so a drive path falls
  through unchanged — and there are no observed cleo paths to compare it
  against, because kmuster has no cleo scanner yet (its roadmap's "Later").
  The decision is *where the conversion lives* once that scanner lands, which
  is kmuster's architecture and is worth taking with the scanner in front of
  you. The item also carries a stale doc line to fix in the same change
  (`declared.rs` still says korg's field cannot hold a Windows path).
- **korg #2919** — where korg records a machine's user. The endpoint must
  expand `~/` and korg has nowhere to record a home directory: `machines` is a
  `TEXT[]` of names (0011), not rows. Shipped as a documented `POSIX_HOME`
  constant — verified `/home/ken` on both hosts the endpoint can currently emit
  — because the alternative is a `machine` table, which is a schema addition
  and a question about whether korg should model hosts at all. Not a decision
  to take while adding an endpoint.

## Not in this change

**The cleo backfill is data, and data is written after deploy.** Migration
0035's header states the rule and the reason: a migration must apply to a fresh
install with an empty table, and one naming production rows means different
things on different databases. So the four `update_project` calls run against
kubsdb once the deploy has relaxed the constraint there — before then
production would refuse the values.

That makes the backfill this sprint's **acceptance trigger**, not a soak: the
evidence is not elapsed time, it is a deploy that happens in this same session.
Fired at ship, recorded under `## Deployed`.

## Deployed

kubsdb, 2026-09-20, by the `deploy-kubsdb` skill from merged `main`.

- **Image** `b9fc57808926` — pushed to the homelab registry as that tag and as
  `latest`. The in-deploy revision assertion passed: the running container's
  commit label is the commit this build came from.
- **Rollback target** `dede8b9f1463` (sprint 085), confirmed present in the
  registry before building.
- **`post-deploy-check.sh --compare`** OK. Every row count identical to the
  pre-deploy baseline (cards 30, links 22, projects 62, proposals 513, reports
  84, work items 1679); **35 migrations before, 36 after** — this sprint's
  0036.
- **The constraint promotion held.** 0036's precheck found no offending row and
  `ADD CONSTRAINT … CHECK` validated against all 62 live rows, so
  `project_src_path_canonical` is a real invariant for the first time since
  0019 deferred it. This was the one part of the sprint whose deploy depended
  on production data rather than on the build; it is now settled, not merely
  measured.

### The acceptance trigger, fired

Not a soak — the evidence was the deploy, and the deploy happened in this
session.

1. **Backfill.** `update_project` × 4: `kbrickshoot`, `kctrldeck`,
   `kpidashclient-win`, `krcmd` → `/d/ClaudeWorks/<name>`. All accepted; before
   the deploy production would have refused every one of them.
2. **Live read** of `GET /api/connectors/projects` on the deployed instance:

   - envelope `{connector: "korg", version: 1, generated: <live>}`, **40
     projects**;
   - all four cleo rows as `D:\ClaudeWorks\<name>` with `kind: "windows"` —
     the round trip this whole sprint exists for;
   - POSIX rows expanded, e.g. `korg` → `kai` `/home/ken/src/tools/korg`
     `posix`;
   - `agent-skills` emitted as its **kubs0** location, as v1 is designed to;
   - host split kai 29, kubs0 7, cleo 4.

**Count correction.** The pre-ship handoff said ~39 emitted rows; it is **40**,
and the proposal's own note had it right. The four-not-five correction is about
the *backfill* — `agent-skills` needed none — but it was already inside the 36
baseline, so 36 + 4 = 40. The backfill figure stands; the emitted figure I
derived from it did not.
