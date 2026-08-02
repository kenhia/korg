# T3 — the korg MCP surface, re-reviewed fresh (2026-08-02)

Sprint 037, proposal korg:868 — the agent-surface program's exit gate.
Method: T2's (#634) — call the tools against production (kubsdb :5674),
measure, never reason from schemas or docs. The reviewer session read only
plan §5 and #634's record before exploring; `docs/api.md`, `repo.rs` and
`tools.rs` stayed unread until every finding below was fixed in writing.
REVIEW.md (T1) was read *after* exploration, for cross-references only.

Coverage: **47 of 48 tools called** — every read, every mutation, every
deliberately-provoked failure path. The one exception is `create_area`,
skipped on purpose: areas have no delete or archive (finding 7), so the
probe row would have been immortal.

Every entry cites the call that produced it. Probe artifacts were archived
or deleted afterwards; the residue is declared at the end.

---

## Gaps found (schema-vs-behaviour) — one WI each

### 1. Paginated `total` collapses to 0 past the last row — **korg #883**

- `list_work_items {project:"korg"}` → `total:14`
- `… offset:10, limit:2` → `total:14` ✓
- `… offset:100` → `{items:[], total:0}` ✗ — while `omitted` stays correct,
  which makes the lie easy to miss.

### 2. Archived projects silently accept new work — **korg #884**

- `create_work_item {project:"kris"}` (kris is archived) → **succeeded**,
  filed #874. The server instructions call active projects "the only valid
  targets for new work" and the param docs promise unknown names fail
  `not_found` "rather than mis-filing" — a known-but-archived name mis-files
  silently. Adjacent: default lists show kris rows with no archived-project
  marker.

### 3. `updated` frozen on ordinary edits of WIs, cards, proposals — **korg #885**

- WI #872: title, `wi_status`, `details`, `sprint` edits across four calls —
  `updated` == `created` every time.
- Card #877: status+rank move — frozen. Proposal #868: `proposed → active`
  — frozen.
- `archived` flips bump it on every kind; topics (#879), handoffs (#875) and
  comments bump correctly on every edit. Wider sibling of T1's F-18.

### 4. Daily-plan past-day freeze unenforced — **korg #886**

- `create_daily_plan_item {plan_date:"2026-08-01"}` (yesterday) → created
  item 882 on a sealed day.
- `delete_daily_plan_item 882` → `{deleted:true}` — "past structure is
  frozen" says this should refuse.
- `reorder_daily_plan {plan_date:"2026-08-01"}` → accepted ("for an open
  day"). History whose end-before-today read validation *is* enforced can be
  silently rewritten on the write side.

### 5. `src_path` CHECK violation → `internal` + raw Postgres — **korg #887**

- `update_project {name:"korg", src_path:"~/src/tools/korg (dev copy)"}` →
  `{code:"internal", message:"error returned from database: … violates check
  constraint \"project_src_path_canonical\""}`. Rejected (nothing stored ✓)
  but at the wrong error tier, DB internals as the only guidance.

### 6. Links: no archive/delete, no timestamps — **korg #888**

- `update_link` has no `archived`; no `delete_link` exists; yet `list_links`
  documents "Archived links are EXCLUDED by default". Probe link 878 is
  stuck in the corpus as live evidence.
- Link rows carry no `created`/`updated` — the only kind without them.

### 7. Area descriptions write-only; no area lifecycle — **korg #889**

- `create_area` accepts + documents idempotent `description` update;
  `list_areas {project:"korg"}` → `[{id,name}]` ×4 and `get_project`'s
  inline areas are the same two fields. No read returns a description.
- No delete/rename/archive for areas at all. (Probe deliberately not
  created.) Belongs beside #855's project-deletion question.

### 8. Two description nits — **korg #890**

- `relate` promises the error names "the registry and the near-miss";
  `depends-on` (one char off) gets the same generic registry list as
  `blocks`. No did-you-mean exists.
- `get_work_item` says "page the tail via list_comments"; `list_comments`
  has no pagination — it returns the whole thread.

---

## Works as documented (the ledger, with the calls)

- **Envelopes & omitted math.** `list_work_items {project:"korg"}` →
  `{items:14, total:14, omitted:{archived:0, closed:112}}`;
  `wi_status:"all"` → `total:126` = 14+112 ✓. Explicit filters zero
  `omitted` (`wi_status:"closed"` → `omitted:{0,0}`) — consistent with
  "the rows the *defaults* hid". Corpus-wide default: 122 live rows,
  `omitted:{archived:17, closed:460}`.
- **`list_proposals` is fixed** (T2's #852): lean rows, no summary, 17 live,
  `omitted:{archived:5, declined:3, done:94}`. The token bomb is gone.
- **`survey_work_items`** is a true deprecated alias: same rows, same
  envelope, limit-50 default as described. (T2's #851 archived-default trap:
  fixed — default is unarchived-only, verified against list.)
- **`list_projects`** → `{items:27, omitted:{archived:9}}`, lean routing
  rows ✓. `get_project korg` → full columns + inline areas ✓.
- **Focused reads.** `get_proposal 868` — one call answered scope, notes,
  covered, comments ✓. `get_proposal 876` — `covered` carries the documented
  summary fields ✓. `get_work_item 634` — related block with titles +
  wi_numbers inline ✓. Archived nodes served with the flag visible
  (`get_work_item 874`, `get_topic 879` ✓).
- **Comment tiering.** 12 comments on #872 → oldest 10 inlined,
  `comments_truncated:true`, `comment_count:12` ✓; `list_comments` oldest
  first ✓; `update_comment` preserves `created`, advances `updated` ✓;
  `delete_comment` → `{deleted:true}` then `{deleted:false}` ✓.
- **Error taxonomy.** Every provoked error was clean `{code, message}`:
  not_found on bogus ids, wrong-kind reads ("no proposal with node_id 466"),
  bogus comment targets. `update_work_item` with both `project` and
  `project_id` → invalid_input "pass either project_id or project, not
  both" ✓. `daily_plan_history to:today` → invalid_input ✓.
- **The linking layer is airtight.** Unknown label → invalid_input naming
  the registry ✓; self-edge rejected ✓; `covers` left-kind and `has_handoff`
  right-kind validated with precise messages ✓; exact duplicate returns the
  existing id (592) ✓; directed reverse is a distinct edge (595 — a legal
  cycle) ✓; undirected reverse returns the existing edge (593) ✓;
  `neighbors` renders direction correctly from both endpoints, label filter
  works ✓; `unrelate` → true/false honestly ✓.
- **Handoffs.** Standalone create rejected naming `allow_standalone` ✓;
  unresolvable owner → not_found with **no partial insert** (872 carried no
  dangling edge) ✓; one-call create+attach ✓; both-direction `related` with
  inline titles ✓; update round-trip ✓.
- **Proposals.** `propose_sprint` covered echo drops the bogus number
  visibly (`covered:[873]` for request `[873, 999999]`) ✓; the 500-char cap
  error is exemplary — counts your actual length (632), routes to `notes` ✓.
  Same quality on `update_project`'s 160-char description cap (212 counted)
  ✓, with no partial store either time.
- **Reports.** `create_report` same-(source,date) re-run → `replaced:true`,
  node_id preserved, findings unlinked when omitted, `findings_linked` echo
  drops bogus numbers ✓ (T1's F-07 fix holds).
- **Daily plan.** Completion timestamp server-authoritative ✓; open-day move
  → `{copied:false}` transfer ✓; create/complete/move/reorder/delete
  round-trip clean ✓.
- **Projects.** `create_project {name:"korg"}` → existing id 11, no dup ✓.
- **Identity.** Created WIs return `node_id == wi_number` (872/873/874) ✓.
- **Deprecated aliases** (`survey_work_items`, `mark_link_read`,
  `search_topics`) all function while steering to their successors ✓.

Ergonomic observations, not gaps: `neighbors` rows carry refs only (no
titles — unchanged from T2; the focused reads' `related` is the fix and it
works); handoffs are reachable only via edges, so `allow_standalone` is an
orphan-maker by design (opt-in, acceptable); topics corpus is empty in
production (the kind is unused); work-item rows never expose `category`
in lists (present on focused read).

---

## Exit measurements (plan §5)

1. **Is the korg block in the global `~/.claude/CLAUDE.md` empty?**
   **YES** — read this session: the file has klams, deferred-tools, and
   remote-edit sections; no korg block exists. T2's two traps were fixed at
   source (ranks 4.1–4.4) and the block emptied rather than growing.
2. **Are the findings all "works as documented"?** **NO** — 7 behaviour gaps
   (+1 wording), #883–#890. None is a token bomb or a default that lies
   about corpus size — the 2026-08 contract work held — but `updated` being
   frozen (#885), archived projects accepting work (#884) and the unenforced
   past-day freeze (#886) all change agent behaviour.

**Verdict: the loop continues at current rank** — one measurement short of
the both-hold condition that would drop the program to low priority. The
natural next turn is a fix sprint over #883–#890 (mostly S/XS; #885 is the
M), then this re-review's successor runs the measurements again.

---

## Declared residue (production writes that outlive the session)

- **WIs #883–#890** — the deliverables.
- **WI #874** (archived) — the evidence for #884; kept because it *is* the
  finding.
- **WIs #872, #873, card 877, topic 879, proposal 876 (declined), handoff
  875** — probe nodes, all archived, tagged `probe`/`agent-surface-037`.
- **Link 878** — cannot be archived or deleted over MCP (finding 6); marked
  disposition Done, tagged `probe`. Remove it when #888 lands.
- **Report 880** (`source:"t3-probe-037"`, 2026-08-02) — reports have no
  delete or archive; body self-identifies as a probe.
- All probe comments deleted; all probe edges unrelated; daily-plan days
  left exactly as found; korg project row verified untouched.
