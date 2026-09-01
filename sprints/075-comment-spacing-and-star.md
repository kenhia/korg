# 075 — Comment spacing, and the star that had one colour

Proposal korg:1758. Covers #1651 (S), #1666 (S).
Branch `075-comment-spacing-and-star`.

## Goal

Two S-sized reading papercuts, both in `web/src`, both fallout from
sprint 074 landing. #1625 turned comments into rendered Markdown and Ken
started actually reading them, which is when a thread's lack of edges
became a problem. #1629 put a ★/☆ on the Project Details bar, and a week
of using it showed the two states were one colour.

No backend, no data model, no MCP surface. The proposal called it a
quick polish pass and it was.

## Cross-project plan

korg is listed in `cross-project-planning/index.md` → `korg+/`, so the
plan was read. **Nothing in the decision register bears on this scope.**
GP-13 and GP-14 are the two that touch presentation, and both are about
*vocabularies* — rendering neutral for a status literal the bundle has
never heard of, and not keying a treatment map by `string`. Neither has
anything to say about a gutter or a hue. No amend.

## Premise check

Both premises held, and checking sharpened one of them — see #1651.

## #1651 — the row that was painted the same colour as the page

The report was "multiple comments blur together". Two separate things
caused that, and only one of them was the obvious one.

The obvious one: `<ul class="space-y-1">` is a 4px gutter, *smaller than
the line-height inside a comment*. Two comments separated by less space
than two lines of one comment read as one comment with a paragraph
break. That is the whole illusion.

The one worth writing down: the row's background was
`bg-[var(--color-bg)]` — which is the **page** background. `Comments` is
mounted on four surfaces and only two of them set a background:

| host | container background |
| --- | --- |
| work-items detail `<article>` | `--color-surface` |
| `NodePreview` (side `<dialog>`) | `--color-surface` |
| planning proposal detail `<section>` | none → `--color-bg` |
| `NodeDetail` `<article>` (every `/<kind>/<node_id>` route) | none → `--color-bg` |

So on the two surfaces where Ken actually reads threads — the proposal
detail page and the node routes — the row was tinted **exactly** the
colour behind it. Not subtly; identically. On the other two it showed at
a 0.04 lightness delta, which is why the bug read as "too subtle"
rather than "absent". A fix aimed only at the gutter would have left
half the surfaces with no card at all.

Ken pointed at the reference himself: *"the size and background of the
`tags` entry box in the create work item form looks about the right size
and colour"*. That box is `bg-[var(--color-surface-hi)] px-2 py-1` —
one step **lighter** than either host. Taking it makes a comment a card
on every surface instead of on two of them, and keeps the padding he
called right.

Shipped, all three parts of "I'm leaning toward both":

- `space-y-1` → `space-y-2`;
- row background `--color-bg` → `--color-surface-hi`;
- `border border-[var(--color-border)]` on the row. With multi-line
  Markdown inside, a gutter alone cannot say where one comment stops —
  the border is the separator half of the ask.

One consequence to fix in the same change: the in-place **edit**
textarea was `--color-surface-hi`, which is now the row's own colour, so
the editor would have vanished into the card. It takes `--color-bg`
instead and reads as inset rather than raised.

The anchored-comment flash (`--color-accent-soft`, #1467) is untouched
and still wins the ternary.

## #1666 — a two-state control with one colour

D-3 in sprint 074 chose ★/☆ over a pushpin for a good reason — text
glyphs can be recoloured and 📌 has no true outline counterpart — and
then had **both states take the project's rail hue**. That is the bug:
the colour said *which project*, and nothing said *starred or not*. All
that distinguished on from off was ★ vs ☆ at 14px.

Worse where the rail hue is undefined. `projectRailColor` returns
`undefined` for a project with no category or a non-active one, and the
inline style is then omitted entirely — so both states fell back to
plain body text. A brand-new project is exactly that case, which is
also the state the e2e suite creates.

Ken named the pair he wanted: the muted grey the "Project Details"
label already carries for off, amber for on. Shipped as a class ternary
(`text-[var(--color-muted)]` / `text-amber-300`) with the inline
rail-colour style dropped from the button.

**This revises the hue half of D-3, deliberately.** The glyph choice
stands; the rail hue stays on the project *name* beside the star, where
it says something the star does not. One mark, one meaning.

## Tests, and the one that passed for the wrong reason

The web app has no unit runner and Playwright is out of CI, so both
fixes got an e2e assertion — written to survive a palette change rather
than to pin today's hex:

- `work-item-comments.spec.ts` compares the row's computed
  `background-color` against `document.body`'s and requires them to
  differ. That is the identity bug stated exactly, and it would fail
  again on any future `--color-bg` row.
- `starred-projects.spec.ts` reads the computed colour off the "Project
  Details" summary, asserts the off-state star matches it, and asserts
  the on-state differs from the off-state.

**Both were verified by breaking them** — reverting the two components
to their pre-sprint styling and re-running — and that is the only reason
the second one is correct. Its first draft *passed against the exact
code it was written to fail.*

The reason is worth keeping. The star is a child of the `<summary>`, the
summary carries `hover:text-[var(--color-accent)]`, and before the fix
the star had no colour of its own — so it inherited whatever the summary
was. The draft read the on-state immediately after `star.click()`, with
the pointer still resting on the star, i.e. inside the hovered summary.
The star came back accent-coloured, differed from grey, and the
assertion went green while the bug it names was fully present.

`await page.mouse.move(0, 0)` before each read is the fix. The general
shape is the same one this suite already learned from the `uppercase`
category headers: **an assertion that can pass for a second reason is
not a gate**, and the only way to find the second reason is to run it
against the broken code.

With the fixes restored, the comment, markdown, node-preview and star
specs all pass.

## korg #1759 — found by running the whole suite, then pulled into scope

`web/` was touched, so the full Playwright suite was run as
`docs/setup.md` asks. **92 passed, one failed** — a *serious*
`nested-interactive` axe violation on `/work-items`.

It is not this sprint's. Checking out `main`'s
`work-items/+page.svelte` and re-running reproduces it exactly, which is
the only reason it can be dismissed with confidence rather than
assumed away.

The cause is #1629's ★/☆ being a `<button>` inside a `<summary>`. A
`<summary>` is itself an interactive control, so a focusable child is
nested interactive content. 074 already needed `preventDefault` **and**
`stopPropagation` to stop the button toggling the disclosure on its way
past — the same problem wearing a different hat, and the note in that
sprint record describes the workaround without naming what it implies.

Filed as **korg #1759** (bug, S, area:ui) with three candidate fixes,
and initially left as a follow-up. Ken added it to the proposal instead
— *"I don't like having tests fail"* — and picked **option 1**. Right
call: the suite is the only gate this UI has, and a suite with one known
failure in it is a suite nobody reads.

### The fix

The border, background and padding move off `<details>` onto a flex bar,
and the two controls become siblings inside it:

```
<div class="flex items-start gap-2 rounded border … px-3 py-1.5">
  <details class="min-w-0 flex-1"> <summary>Project Details <name></summary> <dl>…</dl> </details>
  <button data-testid="project-star">★/☆</button>
</div>
```

Nothing moves on screen. The star lands where `float-right` put it and
still reads while the section is shut, which is all D-2 ever asked for.
`items-start` keeps it on the first line when the panel opens, and
`leading-4` matches the summary's `text-xs` line box so the 14px glyph
centres against the 12px label instead of riding high.

**The old code was already telling us.** The click needed *both*
`preventDefault` and `stopPropagation` or starring would toggle the
disclosure on its way past — 074's record describes that workaround
without naming what it implies. It was the nesting complaining. Both
calls are gone now; a sibling button has nothing to propagate into.

Also settled: `float-right` was chosen in 074 because `display: flex` on
a `<summary>` drops its disclosure triangle. That constraint is gone
too — the flex container is now the bar, and the summary is left alone.

Full suite: **93 passed, 0 failed.**

The wider point is the one `docs/setup.md` already makes — Playwright is
out of CI, so a serious a11y regression merged green in 074 and sat
there until somebody ran the suite by hand. Worth considering whether
`a11y.spec.ts` at least could run in CI; not this sprint's call to make.

## Deployed

<!-- appended after the deploy -->
