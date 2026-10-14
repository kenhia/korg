# 091 — Expandable comment edit box

Proposal korg:3794, covering **WI 3793**. Started by hand on kai.

## Goal

Sprint 090 (WI 3761) gave the new-comment box a 5-line default and an
Expand/Collapse toggle to 50 lines, and left the in-place **edit** textarea
out on purpose. Ken edited a comment right after 090 deployed and got the old
`min-h-[3rem]` box, a little over two lines tall, with no Expand. This sprint
gives the edit box the same treatment.

## Premise check

- **WI 3793 holds.** The edit `<textarea>` in `Comments.svelte` was still
  `min-h-[3rem]`, with Save and Cancel inline beside it and no toggle.
- Cross-project plan: korg routes to `korg+/`, and nothing there covers the
  comments UI.

## Decisions

- **The same mechanism as 090.** Height comes from
  `rows={editExpanded ? 50 : 5}` and nothing else. `min-h-[3rem]` is gone and
  the element is never re-keyed, so `editBuf`, the caret and any pending
  paste-ledger entries survive the toggle.
- **Its own flag.** The edit box uses `editExpanded`, separate from the
  new-comment box's `expanded`, so expanding one leaves the other alone.
  `startEdit` resets it, which means every editor opens collapsed, including
  one opened after a save or a cancel or on a different comment.
- Save, Cancel and Expand stack in a column beside the box. At 5 lines the
  column fits inside the box's height (checked by screenshot at 1280 px and
  375 px). Expand carries `aria-expanded`, and its accessible name says what it
  will do: "Expand edit box" / "Collapse edit box". That name is different
  from the new-comment box's, so neither selector matches both buttons.

## What shipped

- `web/src/lib/components/Comments.svelte`: the expandable edit box, plus a
  `comment-edit-input` test id.
- `web/tests/e2e/comment-expand.spec.ts`: a second test. Post a comment →
  Edit (`rows=5`) → type → Expand (`rows=50`, more than 5× taller, buffer
  intact) → Collapse → Expand and save → reopen (`rows=5`).

## Verification

- `just check`: green (exit 0).
- Full Playwright suite against the 2026-10-14 nightly
  (`korg-20261014-032052`, restored into a throwaway `postgres:18-alpine`):
  **113/113, `--retries 0`**.
