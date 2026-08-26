# 074 — Starred projects, and the last two UI papercuts

Proposal korg:1630. Covers #1603 (XS), #1625 (S), #1629 (M).
Branch `074-starred-projects`.

## Goal

The whole open korg queue, and all three land in `web/src` — only #1629
reaches past it into the engine. Sequenced XS → S → M so the small ones
are done before the M eats the day.

- **#1603** — a parked proposal's status pill renders unstyled on the
  detail page, so the page shows a parked proposal as though *no* status
  were selected.
- **#1625** — agent comments are written in Markdown and rendered as
  literal text, which is the wall of prose Ken reads every day.
- **#1629** — a handful of projects are hot for a week at a time; put
  them at the top of both project rails, and let the set be a fact korg
  holds rather than a per-browser preference.

Every design decision was settled before the sprint (proposal notes
D-1/D-2/D-3, Ken 2026-08-26). Two calls were deliberately left to sprint
start and both were made then: lift the duplicated rail grouping (yes),
and swap #1625's renderer before verifying rather than after.

## Cross-project plan

korg is listed in `cross-project-planning/index.md` → `korg+/`. Three
decisions bear on this scope and none contradict it:

- **GP-13's consumer half** is #1603 exactly — *where korg emits a literal
  the consumer has no treatment for, render neutral, never another
  state's*. Here the consumer is korg's own UI.
- **GP-14** governs how the fix is gated, below.
- **GP-19** is the contract `parked` carries: live, below the line,
  unfinished. #1603 is the decoration gap GP-19 predicted consumers would
  meet — korg's own UI turned out to be one of them.

## #1603 — the pill, and the gate that was missing

Sprint 072 added `parked` to `PROPOSAL_STATUSES`. The proposal detail page
held a local four-entry map typed `Record<string, string>`, so the new
literal indexed to `undefined` and the *selected* button rendered
`rounded px-2 py-0.5 text-xs undefined` — no background, no ink. The row
of statuses is a control *and* a readout, and the selected one is
distinguished by its pill; parked rendered identically to the four states
the proposal was not in. `aria-pressed`/`disabled` still said so, which is
why nothing failed.

**The programs pages carry the same vocabulary and did not have the bug**,
for one reason: their maps were keyed `Record<ProgramStatus, string>`, so
widening the vocabulary without them fails `svelte-check`. That is the
actual finding — not "a map was missing an entry" but *a map keyed by
`string` is a map with its gate switched off*.

So `proposalStatusPill()` lifts into `$lib/domain` beside
`reportStatusPill` / `sourceFreshnessPill` / `scheduleDuePill`, with
**both** mechanisms, deliberately not one:

- the exhaustive `Record<ProposalStatus, …>` is a **compile-time** gate,
  catching the vocabulary being extended without a treatment;
- the `?? UNKNOWN_STATUS_STYLE` is the **runtime** one, catching a korg
  deploy emitting a literal this bundle has never heard of — which no gate
  compiled into this bundle can see.

GP-13 is explicit that these are different failures and that a
vocabulary-mirroring gate "is honest only about what it reaches". Hence
`string` in, neutral out.

The gate was verified by breaking it: removing `parked` from the map fails
`svelte-check` with *"Property 'parked' is missing in type … but required
in type `Record<"done" | "parked" | …>`"*, and passes again restored. The
web app has no unit-test runner, so this type **is** the test, and an
unverified gate would have been GP-14's mirror-image mistake in a new
spelling.

`parked` takes the slate the programs pages chose in 072 (`bg-slate-800
text-slate-400`) and the reasoning with it: dormant is not finished, so it
keeps a hue where `done` has none, and the tint holds it apart from
`declined`, which asserts the opposite of parking.

### The two program maps came along

Not scope creep — the same defect one level up. The two programs routes
held **byte-identical** copies of their status map, and the list page fell
back to `?? ""`, which is #1603's bug in its other spelling: an unstyled
pill is not a neutral pill. Both now call `programStatusStyle()`.

That one is **colour only**, and it is the odd one out in the section. The
list renders it in a row of cards (`px-1.5`), the detail page renders it as
a status control you click (`px-2`) — two honest densities, so lifting the
shape would silently restyle a page to fix a duplication that was only ever
in the colours. The file's shape-included rule exists to stop *accidental*
divergence, and this is not that. The hue reasoning written in 072 moved
into `domain.ts` with the map rather than being stranded in two route files.

## #1625 — comments render markdown, reversing #1120/#1121

The old rule was written into `CommentBody.svelte`'s header: *"Comments render
as literal text in korg and keep doing so: a `#582` is a work-item reference
and a `*` is an asterisk."* That was raised during planning as grounds to cut
this item, and it does not survive contact with korg's own renderer config,
which already defends both halves:

- **`#582` is safe.** An ATX heading requires whitespace after the hashes, in
  CommonMark and in marked's heading rule (`(?=\s|$)`). `#582` is paragraph
  text; `# 582` would be a heading, and nobody writes that.
- **Line breaks are safe.** `markdown.ts` sets `breaks: true`, so a single
  newline is a `<br>` — the same shape `whitespace-pre-wrap` gave. This is the
  one that would otherwise have reflowed every agent comment into a wall.

What genuinely changes is narrow and wanted: `*` italicises, `-`/`1.` start
lists. `_` does not, so `snake_case` — which agents write constantly — is fine.

**That is an argument, so it got a check rather than a benefit of the doubt.**
`tests/e2e/comment-markdown.spec.ts` posts one comment carrying all four cases
at once (they interact — the list and the emphasis have to survive every
newline becoming a `<br>`) and asserts: no heading element anywhere in the
thread, exactly one `<em>` and it is not `snake_case`, two `<li>`, at least one
`<br>`. Every one of those assertions would have been a reason not to ship.

The swap itself is one call site: `MarkdownView` already exists, already
sanitizes, and already turns korg's `![img-<hex>]` tokens into the same
thumbnail buttons `CommentBody` hand-rolled. So `CommentBody.svelte` and
`Comments.svelte`'s own `lightbox` state are gone — `MarkdownView` owns its
lightbox — and `splitImgTokens`/`ImgSegment` went with them, having existed
solely to render comments as literal text with pictures in.

`CommentBody.svelte` is **deleted rather than kept with a rewritten header**,
which is a deliberate departure from the proposal's note. The reversal had to
stay findable, but a dead component retained to carry a comment is the kind of
file a later cleanup removes without reading it. The record now lives at the
render site in `Comments.svelte`, where the next agent to ask "why is this
markdown?" is actually standing.

The real work was the layout, as predicted: the comment sits in a flex row, and
a `prose` block needs `min-w-0` there. Without it a flex item will not shrink
below its longest unbreakable token, so a single pasted URL pushes the edit and
delete controls off the row. Verified with a fixture comment carrying a
120-character URL — it wraps, and the controls hold their place.

`images-ui.spec.ts` carried a comment asserting "the surrounding text stays
literal", which this change makes untrue; it now says what the test actually
still pins. The test itself passes unchanged, thumbnails and all.
