# 085 — Guard unsaved edits against navigation in the web UI

**Proposal:** korg:2846 · **Covers:** #2845 · **Branch:** `085-unsaved-edit-guard`

## Goal

Ken lost a half-written work item: he was typing into the create form, wanted
to check something, navigated away, and the text was gone. He asked for the
thing every other site does — "unsaved changes, continue?".

## Premise check

Holds, flatly. `grep -rn "beforeNavigate\|beforeunload"` over `web/src`
returned nothing: korg had no guard of any kind, on any surface.

The cards editor turned out to be the near-miss. It already has a `dirty`
derived and its own in-modal Discard prompt — but only on *its own close
button*. A nav click or a closed tab walked straight past it.

## Cross-project plan

korg routes to `korg+/`. Nothing in the register constrains this. The nearest
decision is GP-1's side-store line (a consumer may keep display chrome locally,
never korg's data), and it stays untouched: the guard stores no content
anywhere — the registry holds predicates, not text. Draft autosave, which
*would* have to argue with GP-1, is deliberately out of scope. No amendment.

## What shipped

`web/src/lib/unsavedGuard.ts` — a registry of "is there unsaved work in me?"
predicates, plus the two halves that consult it.

**Three exits, no two sharing a mechanism.** This is the finding that shaped
the design:

1. **Leaving the page** — SvelteKit client-side navigation. `beforeNavigate`.
2. **Leaving the browser** — tab close, reload, external link. SvelteKit
   surfaces this through the *same* hook as `type: "leave"` (confirmed in
   `client.js`: it registers its own `beforeunload` and turns a `cancel()` into
   `preventDefault` + `returnValue`), so there is no second listener.
3. **Neither.** korg's forms are destroyed by **state**, not by routing:
   Escape, Cancel, ← Back, picking another project, clicking another row. The
   router never hears about any of these, and on the reported surface this is
   the likelier loss — Escape on the work-items page discards the create form
   with one keystroke. These call `okToDiscard()` themselves.

### Decisions

- **`window.confirm`, not korg's `Dialog`.** `beforeNavigate` must decide
  synchronously whether to cancel; a modal answers later. The alternative is to
  cancel every navigation, ask, then re-issue it on discard — which turns Back
  into a forward push and wraps a state machine around a yes/no question. The
  unload prompt is the browser's own whatever we do, so the native one is also
  the consistent one. `okToDiscard()` is the seam if this ever becomes styled.
- **Dirty means "differs from the pristine snapshot"**, not a keystroke flag.
  Typing a word and deleting it again has cost nothing, and a form that asks
  anyway teaches people to click through the prompt.
- **`okToDiscard()` guards the whole screen by default, one surface on
  request.** Most callers (Escape, ← Back) really do destroy everything on
  screen. A form's own Cancel button does not, and passes its own predicate —
  otherwise cancelling an untouched form while a comment sat half-written below
  it would ask about the comment, which that click was never going to discard.
- **Prose, not quick-adds.** Guarded: the work-item form, both comment drafts
  (new and in-place edit), the card editor, the Commander's Call decision
  draft. Not guarded: the card quick-add and reading-list URL/title boxes — a
  one-line field is a small loss, and this repo already has the argument
  written down in `ConfirmButton.svelte` ("putting a confirm on a reversible
  action trains people to click through confirms, which is exactly how confirms
  stop working on the one that mattered").
- link-up's `cardText`/`wiText`/`linkText` read like drafts and are **filter
  boxes**. Checked, not assumed.
- The card editor **lends its existing `dirty`** to the registry rather than
  growing a second notion of dirty, and keeps its own Discard prompt on its own
  close path.

### Touched

`unsavedGuard.ts` (new), `+layout.svelte` (installs the router half),
`WorkItemForm.svelte`, `Comments.svelte`, `work-items/+page.svelte`,
`cards/+page.svelte`, `AwaitingLane.svelte`, and `tests/e2e/unsaved-guard.spec.ts`
(new).

## Verification

`just check` green. The Playwright spec covers: dirty form + nav link asks and
staying keeps the text; an untouched form does not ask; accepting navigates;
Cancel and Escape ask; a saved form stops being dirty (the crying-wolf case
that gets a guard switched off); an unsent comment draft counts.

## Follow-ups

None filed. Draft autosave/restore stays unbuilt and unfiled — it is a larger
design with storage questions that GP-1 has an opinion about, and it is not
wanted until this guard proves insufficient.

## Deployed

kubsdb, 2026-09-19, by the `deploy-kubsdb` skill from merged `main`.

- **Image** `dede8b9f1463` — pushed to the homelab registry as that tag and as
  `latest`. The in-deploy revision assertion passed: the running container's
  commit label is the commit this build came from.
- **Rollback target** `1404555d02b5` (sprint 084), confirmed present in the
  registry before building.
- **`post-deploy-check.sh --compare`** OK. Every row count identical to the
  pre-deploy baseline (cards 30, links 22, projects 59, proposals 495, reports
  83, work items 1643), 35 migrations before and after — this sprint carried no
  migration.
- **Verified live, and specifically this sprint's work**: a real browser against
  the deployed instance typed into the New Work Item form and clicked away. The
  prompt appeared ("You have unsaved changes. Leave and discard them?"),
  dismissing it kept the page *and* the typed text. Read-only — Save was never
  pressed, so nothing was created in production, and the row counts above
  confirm it.
- `/plan` returns 200; the REST work-items read answers.
