# Sprint 005 — Daily Reports: kmon's reports as first-class korg nodes

_2026-07-04. Proposed by Ken; ties together kmon v2 (investigation) and v3 (write-back) by
giving both the infrastructure they need: reports live in korg with history, findings
become work items linked to reports, and the UI gets a Daily Report page._

## Why this order (before kmon v2/v3)

- Investigation needs "what's already known" — findings-as-WIs with stable identity gives
  recurring problems ONE work item accumulating evidence, not daily duplicates.
- Reports move from files nobody rereads into the surface Ken already works in, with
  history and comments for free (comments become correction/feedback data).

## korg changes

### Migration `0009_report.sql` (pattern: 0008_sprint_proposal)

- Extend `node_kind_check`: + `'report'`.
- New detail table:
  ```sql
  CREATE TABLE report (
      node_id     BIGINT PRIMARY KEY REFERENCES node(id) ON DELETE CASCADE,
      source      TEXT   NOT NULL,              -- 'kmon' (future: other reporters)
      report_date DATE   NOT NULL,
      status      TEXT   NOT NULL CHECK (status IN ('ok', 'attention', 'problem')),
      summary     TEXT   NOT NULL,              -- the one-liner for the list view
      body        TEXT   NOT NULL,              -- full markdown
      model       TEXT,                         -- which model wrote it
      escalated   BOOLEAN NOT NULL DEFAULT FALSE,
      UNIQUE (source, report_date)              -- one kmon report per day; re-run replaces
  );
  ```
- Findings link via the existing `relationship` table, label **`finding`**
  (report → workitem). No new join table (0008 precedent with `covers`).
- Comments already generalized to nodes (0007) — reports get them for free.

### repo.rs + MCP tools (korg-mcp)

- `create_report {source, report_date, status, summary, body, model?, escalated?}` —
  upsert on (source, report_date): same-day re-run replaces body/status/summary,
  keeps node_id (so links/comments survive).
- `list_reports {source?, limit?}` — newest first, summary fields only.
- `get_report {report_date, source?}` — full body.
- Linking/comments reuse existing `relate` / `add_comment` (add_comment takes node_id —
  works for reports as-is; verify `relate` accepts the new kind).

### Web: `/daily-reports` route (SvelteKit, mirror the planning page's shape)

- List of reports, newest first: date · status pill (ok/attention/problem) · one-line
  summary · source/model chip.
- Click row → expand full markdown body inline (Tailwind typography), latest report
  **pre-expanded** (leaderboard UX per Ken).
- Expanded view shows: linked finding WIs (relationship label `finding`, clickable to the
  work-items page) + the node's comments with the existing add-comment control.
- Nav entry alongside planning/cards/reading-list.

## kmon changes (after korg deploys)

- `kmon/korg_sink.py`: after verification passes —
  1. `create_report` (status from the kmon-status footer; summary = the `## status`
     paragraph's first sentence; body = full markdown).
  2. Per finding: **dedup by identity tag** `kmon:<kind>:<key>` (e.g.
     `kmon:unit:kcard-backup.service`, `kmon:disk:/data`). Search open WIs with that tag:
     exists → `add_comment` ("still failing on YYYY-MM-DD, report attached") + `relate`
     to today's report; absent → `create_work_item` (project `kmon`, area `system` by
     default; overridable per finding) + tag + `relate`.
  3. Resolved findings (tagged WI open but condition gone): comment "not observed today"
     — **closing stays manual** (v1 is read-only-ish; kmon never closes WIs).
- Report file in `reports/` stays (offline copy + works when korg is down).

## Non-goals (this sprint)

- No auto-closing of finding WIs; no scheduling changes; no report editing UI; no
  retention/pruning (reports are small text rows).

## Deploy

Image-over-SSH per [`../004-sprint-proposal/DEPLOY.md`](../004-sprint-proposal/DEPLOY.md):
build → `docker save | ssh kubsdb docker load` → `docker compose up -d` from
`/datastore/korg` (compose + env are authoritative on the host). Migration runs on boot
(`korg_core::connect` migrates). Take a `just snapshot` first.

## Acceptance

- `just test` green incl. new repo/tool tests; migration applies cleanly to a snapshot copy.
- kmon `just report` end-to-end: report visible on /daily-reports with latest expanded,
  finding WIs created project=kmon/area=system, second run same day replaces (no dupe),
  recurring finding comments instead of duplicating.
