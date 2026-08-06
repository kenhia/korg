# 049 — IDs everywhere, honest progress, decisions in one gesture

**Proposal:** korg:1042 · **Work items:** #980 (tweak, S), #981 (tweak, S),
#982 (tweak, S), #966 (task, S), #846 (chore, XS)
**Branch:** `049-ui-polish-ids-and-decisions`

## Goal

Five small items, four of them from Ken's 2026-08-05 user test of `/programs`
and the awaiting lane. They arrived as separate findings but say one thing from
different angles: **the UI does not show or resolve the things agent
conversations are made of** — and a decision Ken has to make costs three
surfaces instead of one.

## What shipped

### #982 — find-by-ID resolves every kind

The cause was smaller than the report suggested and the fix is broader.
`get_node_preview` dispatches on `node.kind` and had an arm for eight of the
nine kinds; `program` was the only one missing, so it fell through `_ => {}` and
kept the default `format!("{kind} #{id}")` title. That is the entire bug: chip
`PROGRAM`, title literally `program #979`.

The program arm mirrors the proposal arm's shape, including its aim/notes rule
(the routing contract is a field when there is a long form to show instead, and
the body when there is not — never both), and adds the derived `span` and slice
count as fields since a program carries no project of its own (D-6).

The answer to the WI's open question — *"which other kinds hit this
fallback?"* — is now **none**, and it is a test rather than a claim.
`every_node_kind_resolves_to_a_real_title` builds one node of each kind through
the real write paths and asserts none of them previews as `{kind} #{id}`. Add a
tenth kind without an arm and it fails. The fallback is silent by construction,
so a test had to be the thing that notices.

The preview's "Open full page ↗" was hardcoded to `kind === "handoff"` because
handoffs were the only detail route when it was written. Programs have had one
since 044, so it asks `nodePage(kind)` now.

### #980 — IDs everywhere, and an honest three-part progress figure

**Progress.** The old display put Ken's *closed* count on the left, where a "how
much is finished" figure goes. `0/3` was a true statement about verification and
a false one about work, with no way to tell which question was being answered —
which is exactly how program 979 (everything implemented, nothing closed) got
read as "still has implementation to do".

It now reads `work-complete / verified-by-you / total`, in D-7's own vocabulary:
work-complete is `resolved + done + closed` (everything agents consider
finished — `resolved` counts because that is precisely what it means), verified
is `closed`. `N / <N / N` is the state worth spotting across the room.

Both the triplet and the bar come from one `sliceProgress()` call in
`domain.ts`. That is deliberate: the bug being fixed is a display that
contradicted itself, and two call sites doing their own arithmetic is how it
comes back. The bar's hues reuse meanings the UI already carries — emerald is a
finished status pill, amber is the awaiting-Ken lane — so a fully amber bar
*means* "all of this is waiting on you" before you read the legend. The legend
ships above the list rather than in a tooltip, because three bare numbers with
no key are what produced this bug in the first place.

Display-layer only, as the WI required: `get_board`'s slices already carry all
four counters and kfdc's Operations panel is a second consumer, so nothing was
added to the read.

**IDs.** Ken's follow-up comment widened this to a UI-wide sweep. One
`ID_CLASS` constant, in the Work Items list's existing mono-and-muted style, now
appears on: the programs list row, the program detail header, each slice row and
each related ref; the cards board tile and a new ID column in the cards table;
topic rows; reading-list rows; daily-report rows; the awaiting lane; the Today
page's plan items (the *source's* id — the source is what an agent names), its
proposal tray and its card tray; and both link-up pickers. One constant, because
matching `korg:979` against a screen is a scanning task, not a reading one.

The Planning card already showed its proposal id, so it was left alone.

### #980's third finding — the program status control

Filed as a comment rather than its own WI: the detail page rendered status as a
read-only pill while `PATCH /api/programs/:node_id` sat unused, so marking 979
done needed a REST call on Ken's behalf. It is now an active/holding/done
control on the header, same shape as the proposal status buttons on Planning,
with the current status keeping the pill styling so the header still reads at a
glance.

### #981 — register a decision in one gesture

`reply` on a lane row opens an inline box; one action posts the comment **and**
clears the marker. The ordering is load-bearing: comment first, then clear. If
the comment fails the marker stays up and the ask survives; if the clear fails
the decision is already recorded where agents look. The failure to design out is
a cleared marker with the answer nowhere.

The comment goes to the node via the same `POST /api/nodes/:id/comments` an
agent reads back — the lane stays a flag and never grows a store of its own
(D-3/D-7). The e2e spec's last assertion is exactly this, and it is the one that
would fail if a future change ever made the reply lane-local.

Also folded in, as the WI asked: `href(row)` no longer returns null. Every kind
the lane renders is clickable, via a shared `nodeHref` that knows the two kinds
with a page of their own and the list page for the rest. A list page is a real
destination — the argument the original `sprint_proposal` → `/planning` case
already made.

`clear` stays for asks that need no answer, and the box says out loud that
closing or finishing the item clears the ask on its own (D-7 already does this),
so Ken does not type "closing this" into a comment box.

### #966 — `get_work_item` takes `node_id` too

Fable passed `{node_id: 965}` and got `missing field wi_number`. The call was
right in every way that matters: since the 0009 identity migration a work item's
node id **equals** its `wi_number`, and every neighbouring read (`related`,
`covered`, `list_awaiting`) hands out `node_id` — so the agent was holding the
right number and korg refused it on spelling.

`WorkItemSelector` takes either. Both together is `invalid_input`, matching the
project/area selectors (#575): korg does not choose between two ids the caller
passed on purpose. Equal values are not an exception — an unequal pair is a real
contradiction, and a rule with an exception is a rule nobody remembers. The
error names the identity so a retry is obvious. REST keeps its single path
segment, which is unambiguous by construction.

### #846 — scope the CI cancellation off main

The one line, plus the *why* above it, as klams did:

```yaml
cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}
```

Re-verified before landing: korg still has **no main-only jobs** — no `if:`
conditions and no `github.ref ==` guard anywhere in `ci.yml`, the concurrency
group being the only `github.ref` reference at all. klams' tradeoff (scoping the
cancellation made its main-only `perf` job run twice per ship) does not apply
here, so there was nothing to weigh.

The comment says explicitly that the symptom is currently **masked, not gone**
— the shared sprint-ship `[skip ci]` half means the deploy record starts no run,
so no two pushes currently overlap on main — precisely so the next reader does
not "simplify" the line back out.

## Running the e2e suite found three things

This sprint is the first time the Playwright suite has been run since sprints
043 and 044 landed. It is deliberately out of CI (`docs/setup.md`: it needs a
built bundle, a running `korg-api` and a database), and the cost of that shows
up as drift noticed in batches. Two specs had gone stale in ways unrelated to
this sprint, and both are fixed here rather than left red:

- `handoff.spec.ts` created a proposal with no project. Sprint **043** made a
  proposal single-project and required; an untargeted create is now
  `invalid_input`, so the seed silently produced no proposal.
- `work-item-edit.spec.ts` asserted the relationship picker offers exactly five
  labels. Sprint **044** added `includes` and `collides-with`. This list has now
  gone stale twice for the same structural reason — `has_handoff` in 025, caught
  in 028; these two in 044, caught here — and the comment says so.

The third was mine: putting the id beside a topic's name as a bare text node
made the smallest element containing the name the whole `<h2>`, whose text is
now `#12 the name`. Nothing matched the name exactly — by locator *or* by sight.
The name got an element of its own. Worth noting as a sweep-wide hazard: every
other site already had the title in its own element, which is why only topics
broke.

Full suite: **68 passed**, including `a11y.spec.ts` and `theme-contrast.spec.ts`
over every swept surface.

## Verification

The proposal's acceptance was the user test that produced the findings, and it
is now four specs in `programs-progress.spec.ts` and `awaiting-decide.spec.ts`
rather than a manual pass:

- a program with everything implemented and nothing closed reads `3 / 0 / 3`;
- closing one item moves it to `2 / 1 / 3` (a closed item counts in *both* the
  left and middle figures — it is work that is complete and verified);
- the status control marks a program done from the browser, and the server
  agrees afterwards, not just the button;
- typing a program's id into find-by-ID shows its real title and lands on
  `/programs/:id`;
- answering on the lane leaves a comment on the node and clears the marker;
- a card — a kind with no page of its own — is clickable on the lane.

Six core tests in `crates/korg-core/tests/sprint049.rs` and one MCP-level test
in `korg-mcp/tests/server.rs` cover the two items that reach past `web/`.

## Follow-ups

- **#870** stays next, as the proposal intended: this sprint's sweep produced
  the list of surfaces its UI-coverage review needs.
- The e2e suite's out-of-CI status cost three failures this sprint that nobody
  could have seen coming. Worth its own item if the pattern repeats a third
  time; not folded in here, because giving it a CI job is a real piece of work
  (bundle, API, database) and not a polish sprint's business.
