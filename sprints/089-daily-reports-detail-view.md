# 089 — Daily Reports: a detail view that shows the whole lead

Proposal korg:3310, covering **WI 3294**. Slice 2 of program korg:3314
("Low-hanging fruit, run 4"), run as an overseen karc leg on kai.

## Goal

Ken, on report #3202: the list row's blurb is cut off with no way to read the
rest. He preferred a detail view to a tooltip. Acceptance: from the Daily
Reports page he can read a report's full summary/lead text without it being
clipped, on desktop and at phone width.

## Premise check

- **The clip is real, and it has two layers** (holds). `+page.svelte:218`
  put `truncate` on the summary, and the stored summary is itself capped at
  200 characters by kmon — #3202's ends "The collector's `fa".
- **"Add a `/daily-reports/<node_id>` route" had drifted.** The route already
  existed: sprint 070 (#1467) gave every kind a URL, and reports rendered
  through the generic `NodeDetail`. But nothing linked to it, and it showed the
  clipped summary as a field, the findings only as bare `workitem #n` edges,
  and no reviewed toggle. The drift confirmed the direction and did not change
  it. NodeDetail's own header names the move: "a kind that outgrows the
  uniform view graduates; it does not get a new URL". So the work became
  *graduate the existing route*, not *add one*.

## Decisions

The overseer's calls in the proposal notes were taken as made: the lead comes
from the body, the 200-character cap is untouched (so kmon's writer and the
schema do not change), and the layout follows the proposal/program detail
pages.

- **The whole sentence is the body's first paragraph**, for all three body
  shapes in production: kmon's `## status` block, k-homelab-drift's bare
  three-line opener, and kfo-soak's prose-first body. So the page shows the
  body and puts nothing in front of it (see the ruling below).
- **Layout is the proposal page's.** Id + title (`{source} — {date}`, the
  preview's title, so nothing changes for a reader who knew the old page),
  status pill, and then the one control the kind has, *reviewed*, where a
  proposal has its status row. Then the body under a `Report` heading, findings as a list with preview + "open work item ↗",
  and comments. The toggle reconciles from the response rather than being
  optimistic: the list page's reason for optimism is a list that wants to get
  shorter, and this page has one row.
- **The wrong-kind notice is kept.** The generic view said "Node #n is a
  sprint_proposal, not a report" with a link to where it lives. The new page
  reads `reportMaybe` (a `httpMaybe` twin of `api.report`) and, on a 404,
  asks `api.node` for the kind, so it answers that case no worse.
- **The list's `#id` is now a link**, placed beside the expand button rather
  than inside it (an `<a>` inside a `<button>` is invalid HTML, the same
  reason the reviewed toggle already sat outside it). An **open row wraps its
  summary**; a shut one still clips, so the list stays a list. On a phone the
  wrapped summary takes the full width under the date and pill.
- **No standfirst: overseer ruling, round 2.** The first cut put a lead
  standfirst above the body (a `reportLead` helper took the first prose block
  of the body), as the brief asked. For every writer, that lead *is* the
  body's first paragraph, so the page showed it twice a few lines apart. The
  overseer ruled the duplicate noise (handoff korg:3326, reply on korg:3310):
  the standfirst was dropped, and `reportLead` with it, since nothing else
  called it. The spec asserts that the paragraph appears exactly once.

## What shipped

- `web/src/routes/daily-reports/[node_id]/+page.svelte`: the bespoke page.
- `web/src/routes/daily-reports/+page.svelte`: `#id` links to it, and an
  open row wraps.
- `web/src/lib/api.ts`: `reportMaybe`.
- `web/tests/e2e/report-detail.spec.ts`: four specs. The `#id` opens the page
  and it shows the whole lead sentence (past the 200-char cap) exactly once,
  and the reviewed toggle persists across a reload. An open row
  wraps and a shut one clips. At 375 px there is no sideways scroll. A
  non-report id says what it is. Written first: against the unchanged code,
  3 failed and 1 passed (the wrong-kind spec, which the generic view already
  satisfied, and which now guards against a regression).
- `docs/node-shapes.md`: the report section names both routes and says why the
  page shows `body`, not `summary`.

## Verification

- `just check`: green (exit 0).
- Full Playwright suite on kai against the 2026-09-25 nightly
  (`korg-20260925-032452`, restored into a throwaway `postgres:18-alpine`),
  `korg-api` on `:8090`: **116 passed, 1 flaky** (`search.spec.ts` "a
  relaxed answer says so", untouched by this sprint, passed on retry). The new
  spec ran 12/12 with `--repeat-each 3 --retries 0`.
- Screenshots at 375 px and 1280 px of both the list and the page, checked by
  eye.

Round 2, after the standfirst was dropped: `just check` and the
daily-reports e2e spec were re-run; results are in the round-2 handoff on
korg:3310.

## Repaired in passing

- `docs/node-shapes.md`'s report table never listed `reviewed` (#2154), which
  the list page has rendered since that sprint. Added, and the gate still
  passes.
- `NodeDetail.svelte`'s header said it backs six pages including reports. It
  now backs five, and the comment says so.

## Follow-ups

None.
