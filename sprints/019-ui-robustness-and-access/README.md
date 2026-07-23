# Sprint 019 — UI robustness and access

Proposal `korg:558` ("Cleanup B6"). Covers WI #547, #548, #549, #571.

The 2026-07 review's sixth bundle, and the first aimed at the browser. The
previous five made the *server* honest — typed errors, honest mutations,
enveloped reads, one contract source, docs that match the code. This one makes
the **client** honest, and the theme is the same: korg's UI could fail without
telling you.

This document began as the plan agreed before implementation and has been
updated in place, so what follows is what was built — including the four places
where measuring something changed the plan.

## Prerequisites — all satisfied

B1–B5 are `done`, so nothing needed a workaround. B3 gave the response
envelopes, B4 gave `domain.ts`'s `chip` map for #571 to land on, and B1 gave
REST errors a machine-readable `code` — which the client was throwing away.

## Front 1 — the error surface (#547)

### What was broken

Verified in the source, not inferred:

| Site | Behaviour |
|---|---|
| `Comments.svelte` | load `.catch(() => {})` — a failed fetch rendered "No comments." |
| `Comments.svelte` | add / delete / edit were bare `await` — failure was an unhandled rejection, zero feedback |
| `link-up/+page.svelte` | **all four** collections `.catch(() => [])` — an outage rendered a confidently empty page |
| `cards`, `reading-list`, `work-items` | the same shape across add/save/archive |

`link-up` was the sharpest: it did not merely lose data, it asserted something
false. "There is nothing to link" and "korg is down" rendered identically.

### `ApiError` — the `code` field now survives

`api.ts` caught the server's `{error, code}` body and kept only the message
string, so sprint 013's whole reason for adding `code` died in the last five
lines before it reached the UI. There is now an `ApiError` carrying `status`,
`code`, `detail`, `method` and `path`, plus a `NetworkError` for "the request
never got an answer" — a genuinely different situation from "korg replied and
said no".

The distinction earns its keep immediately: `invalid_input` is the caller's
problem, so the server's own sentence is shown ("no project named 'KORG' — did
you mean 'korg'?"); `internal` is korg's problem, so the user gets an apology
and a retry suggestion instead of `sqlx: connection pool timed out`.

`ErrorCode` is **generated**, not hand-mirrored — `ERROR_CODES` was added to
`korg-core` next to the enum, exported through the existing `just gen`
vocabulary path, and fenced by a test asserting it matches `ErrorCode::ALL`.
Hand-writing the union in `api.ts` would have been the exact drift B4 removed.

### The rest of the surface

- `toast.svelte.ts` — `notify`, `reportError`, and `attempt(fn, doing)`, which
  reports failure and returns `undefined` so callers update local state only on
  success. Optimistically mutating and then leaving it wrong is its own lie.
- `Toaster.svelte` — one instance in the layout, two live regions:
  `role="alert"` for errors (assertive), `role="status"` for successes (polite).
  Errors **do not** auto-dismiss; a message that vanishes before you look back
  is the silent failure again, just slower.
- `ErrorNotice.svelte` — the "failed to load" state, always with a retry.
- **`.catch(() => [])` is now a lint error.** An eslint `no-restricted-syntax`
  rule catches the array, object and empty-block forms, with a message naming
  `attempt()`. Verified against a probe file containing all three shapes.

### Not done, deliberately

No retry/backoff, no request dedupe, no cache. Tempting while touching every
fetch, and out of scope: this sprint is about *surfacing* failure, and a retry
layer changes when failures surface.

## Front 2 — the dialog primitive (#548)

`NodePreview` had no focus trap, no restore and no Escape; the cards modal had
Escape only. Both now use one `Dialog` built on native `<dialog>` +
`showModal()`, which supplies the trap, the restore, Escape and `aria-modal`
from the browser. A hand-rolled trap is ~60 lines that has to enumerate
focusable selectors, and every implementation gets something subtly wrong.

Two things the platform did *not* cover, both found by testing:

- **Focus restore needed help.** `<dialog>` restores focus on `close()`, but
  korg's callers wrap the dialog in `{#if open}`, so closing destroys the
  component in the same tick and the browser never gets the chance. `Dialog`
  now remembers the opener and restores it on teardown.
- **NodePreview's backdrop was a full-screen `<button aria-label="Close
  preview">`** — a viewport-sized control in the tab order. `::backdrop` plus
  the header close button does the same job without the phantom button.

Labelled inputs across reading-list, cards, the card editor, `Comments`, the
`/plan` project select (a **critical** axe violation: a `<select>` with no
accessible name) and the find-by-ID box. The in-repo standard is now a real
`<label for>` — `TopicPicker` had *both* an `sr-only` label and an `aria-label`,
where `aria-label` wins and the label element is dead markup.

### Table rows: the fix the plan got wrong

The plan said to give interactive `<tr>`s `role="button"`. Doing that broke the
table: a row that claims to be a button stops being a row, and
`getByRole("row")` — along with any real assistive-tech grid navigation — stops
finding it. Four pre-existing specs caught this immediately.

The rows now stay rows, and the **title cell contains a real `<button>`**. That
keeps the table a table and gets Enter/Space for free because it is an actual
button, rather than by re-implementing the button contract on a `<tr>`.

### Board tiles: a bug bigger than the one reported

The WI says the board tile "ignores Space". Measuring it showed the tile
**ignored Enter too**, despite having had an Enter handler since the board was
written. `svelte-dnd-action` registers its own keydown listener for keyboard
dragging and stops propagation, and Svelte 5 delegates `onkeydown` to the root —
so the handler never ran. `onkeydowncapture` fires first and fixes both keys.

The same library also **rewrites the tile's `role` to `listitem`** (the zone
becomes `role="list"`), so a `role="button"` set in markup does not reach the
DOM. Reorderable-list semantics are arguably right; what matters for this WI is
that the tile is focusable and activates from the keyboard, which it now is and
does. Both facts are recorded at the call site and in the spec.

## Front 3 — destructive actions and mobile (#549)

**Confirm for delete, undo for archive** — split by risk rather than treating
four actions alike. A deleted comment is unrecoverable through the UI, so it
confirms (`ConfirmButton`: arms on first press, disarms on blur, Escape and a
timeout, and announces the armed state via `role="status"` because the button
keeps focus while only its accessible name changes). Archive is reversible by
design, so it gets an undo toast. Confirming reversible actions is how confirms
stop working on the one that mattered.

Confirms: comment delete, relationship remove, daily-plan item remove.
Undo: work-item archive, card archive, topic archive.

### Mobile, measured at 390×844

| Claim | Measured |
|---|---|
| nav overflows to x≈797 with no affordance | **reproduced**, and fixed by wrapping instead of scrolling — all 11 items reachable, 0px body overflow on all 10 routes |
| cards Cut strip clips off-screen | **not reproducible** against current code, collapsed or expanded: the strip ends at x=374 of 390 and the board reports no overflow. Left alone rather than "fixed" |
| planner labels over-truncate at desktop widths | **reproduced and worse than described**: at 1440px a label showed **81px of the 628px it needed — 87% of the title gone**. Now wraps to two lines with the full text in `title` |
| reading-list title hijacks activation | **reproduced**: the `<a>` called `preventDefault()` and opened the editor, reserving ctrl/⌘-click for actually following it. The link now navigates; editing has its own button |

One finding beyond the WI: `/planning`'s header pushed **41px past** a 390px
viewport, scrolling the whole page sideways. Same class of failure as the nav,
less obvious because the overflowing part is a filter rather than a link. Fixed
by wrapping. All ten routes now measure 0px horizontal overflow.

## Front 4 — tag chips (#571), and why axe would not have caught it

Ken reported tags "fade into the background". Measuring it changed the fix.

`chip.tag` was `bg-[var(--color-surface-hi)] text-[var(--color-muted)]`:

| Pair | Ratio |
|---|---|
| chip **text** on chip background | **5.23:1** — passes AA |
| chip **background** vs a `surface-hi` tile | **1.00:1** |
| chip background vs a `surface` panel | 1.15:1 |

**The text contrast was fine. The chip had no edge.** A chip reads as a chip
because it is a container, and this one painted its container in the same token
as the thing behind it — on the kanban board, *identically*. So the fix is a
hue, not a darker grey. Tags are now amber, joining project (teal) and category
(violet), and measure best-of-three on separation:

| chip | edge (bg / surface / surface-hi) | label |
|---|---|---|
| project (teal, unchanged) | 1.41 / 1.38 / 1.29 | 8.0–9.1 |
| **tag (amber, new)** | **1.58 / 1.50 / 1.38** | 8.8–9.6 |
| category (violet, unchanged) | 1.37 / 1.31 / 1.19 | 9.2–9.9 |

`NodePreview` also hardcoded project and tag chip styles — the seventh variant
F-15 counted, and the one site B4's consolidation missed. It uses the map now.

### axe-core cannot read korg's colours

Wiring `@axe-core/playwright` produced `color-contrast` failures across four
routes. They were **all false positives**: axe reported `--color-muted` as
`#6e7076` (3.61:1), while the colour Chromium actually paints for
`oklch(0.68 0.01 270)` is `#96989f` (6.21:1). Ground truth came from a canvas
round-trip in the browser, which also confirmed the converter used for the chip
table above.

korg's entire palette is oklch, so the rule is unusable here and is disabled
with that measurement recorded next to it. Contrast is still checked — by
`theme-contrast.spec.ts`, which reads painted pixels rather than parsing CSS,
and which also asserts the thing axe structurally cannot: that a chip separates
from the surface behind it.

## Verification

- **`just check` green** — fmt, generated-file freshness, `pnpm check` (0
  warnings), `pnpm lint`, clippy `-D warnings`, 34 Rust suites.
- **Playwright: 48 tests, 0 failures** on a fresh database (2 pre-existing
  daily-planner specs are flaky and pass on retry).
- **axe-core: no serious or critical violations on any of the 10 routes.**
- New suites: `a11y`, `theme-contrast`, `error-surfacing`, `destructive-confirm`,
  `dialog-focus`.
- The eslint swallow-ban and the drift between plan and reality were each
  verified by deliberately breaking something and watching it fail.

Four pre-existing specs were updated, each for a real behaviour change: the two
confirm flows now take two presses, `modal-close` became `dialog-close` (the
primitive owns that button), and link-up's outcome message moved to the toaster.

### A methodology note worth keeping

Two conclusions in this sprint were wrong until controlled:

- Three drag specs appeared to be regressions. They passed on a *main* worktree
  and failed on this branch — but main was running against an empty database
  and this branch against a seeded one. Against a fresh database, this branch
  passes them too. The failures were my test data, not the code.
- `slot-schedule` was cited in the B7 proposal as failing on main. It passes on
  main here. That note is stale or environment-specific and should not be
  trusted when B7 runs.

## Not done

- The **category chip** is the weakest of the three (1.19:1 on `surface-hi`).
  The contrast floor is set just below it rather than redesigning a chip nobody
  reported. Worth revisiting when F-15's chip work is next open.
- **Board tiles keep `role="listitem"`** from the dnd library. Keyboard
  activation works; the role is the library's.
- No light theme — confirmed out of scope with Ken. korg has one dark theme, so
  "both themes" in the proposal is read as both *surface levels*.

## Deployed 2026-07-23

Shipped to kubsdb from merged `main` (`ae202ca0`).

| | |
|---|---|
| Image | `korg:latest` + `korg:ae202ca0780c` |
| Revision label | `org.opencontainers.image.revision=ae202ca0780cb01b4a9b1129f3a2f6d6a38a7cf8` |
| Rollback target | `sha256:61bdfd51…` / `korg:c78468fa1912` (sprint 018) |

Preflight passed every gate: clean tree, kubsdb reachable, backups current
(`korg-20260723-032356.sql.gz`, 283 KB, larger than each predecessor), rollback
target noted, baseline captured. Sprint 018's SHA stamping paid off immediately —
the rollback target reported `rev=c78468fa…` where the previous deploy had to
record an empty label.

`scripts/post-deploy-check.sh --compare` passed with **zero delta** on every row
count (380 work items, 27 cards, 4 links, 0 topics, 57 proposals, 23 reports, 29
projects), plus health, the enveloped reads, the focused read, the 404 +
`code: not_found` contract, the MCP roundtrip and the idempotent write.

Verified live, specific to what this sprint changed:

- **Tag chips carry the hue.** 40 amber chips render on the cards board, and the
  one measured separates from its tile at **1.65:1** — the same tile background
  that made the old chip exactly 1.00:1 and invisible. The bug that started
  WI #571, fixed and measured in production.
- **The card editor is a real modal.** `dialog.matches(":modal")` is true after
  opening and Escape closes it, so it is `showModal()` rather than a styled div.
- **The toaster is mounted** on every page, and find-by-ID has its `<label for>`.
- **No horizontal overflow at 390px** across `/`, `/cards`, `/work-items`,
  `/planning` and `/reading-list`.

### CI note

GitHub dropped `pull_request` events for this repo for roughly an hour. PR #20
got zero check suites, and neither reopening the PR nor an empty `synchronize`
push produced a run — while the workflow stayed `active`, Actions `enabled`, the
repo public, and GitHub reported all systems operational. Merged on the local
gate (the same set CI runs) with Ken's agreement; the `push` to `main` fired
immediately and went **green on both jobs**, so the outage was
`pull_request`-specific and has passed. Worth reporting to GitHub rather than
re-diagnosing if it recurs.

### Two housekeeping items

**PR #9 closed as superseded** ("Plan view: resolved/closed are terminal
statuses", open since 2026-07-08). It wanted `/plan`'s `isDone` to treat
`{done, closed, resolved}` as terminal; `main` already does exactly that via
`isSatisfied()`, which landed in sprint 016 and went further by separating the
two questions the UI had been conflating.

**A stray commit was corrected.** The first attempt at this deploy record ran
`git add -A` from the repo root while another session was writing B7 in the same
working tree, and swept two of its in-progress files onto `main`. They were
un-tracked with `git rm --cached` (so that session kept them on disk) in the
commit following `d577836`. Worth remembering: `git add -A` is not safe in a
shared working tree, and `sprint-ship`'s Phase 3 says to run exactly that.
