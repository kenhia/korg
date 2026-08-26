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

## #1629 — starred projects

Two halves, as Ken said. The engine landed first and separately (`367609f`);
this is the rail.

### The grouping was lifted (the call left to sprint start)

The proposal flagged this as a decision to make at the start rather than
halfway through, because it is the M growing an L. Ken said lift it, and the
tree agreed: the two rails were independent implementations of the same
construction, down to the same escaped-NUL key for the "Uncategorised" bucket,
and Planning's copy carried a comment reading *"Same construction as the Work
Items rail (WI #678)"*. Two copies of a shared idea is how #1603 happened one
level down, and the starred band would have been the third.

`railGroups()` in `$lib/domain` now returns the bands: the starred set, then
`CATEGORY_ORDER`, then Uncategorised. It is generic over the row type so both
rails pass their own, and a band carries `label: string | null` — `null` means
render no header, which only the starred band uses.

Planning's copy also carried the #1310 lesson: that bucket key must be the
two-character escape sequence and never a raw NUL byte, one of which had made
the whole file binary to git (`Bin 26176 bytes` instead of a reviewable diff)
and invisible to `grep -rn` across the repo. That note moved into `domain.ts`
with the code, and the lifted version was checked for a raw NUL before it was
committed.

### The band

A starred project renders **twice**: in the band and again in its normal
category position. That is Ken's mock and it is the feature — the band is a
shortcut, not a move, so the rail does not reshuffle under the click.

The band is bare, no glyph and no header. It does carry the rule Ken's mock
draws between "All Projects" and it — the category headers bring their own
`border-t`, so a headerless band would otherwise run straight on from the
All-projects row as though it were part of it. Verified against a screenshot of
the rendered rail, not just the DOM.

The band renders only when something is starred, so a rail with nothing hot
looks exactly as it did before this existed.

### The control (D-2, D-3)

One ★/☆ button, at the right of the `Project Details <proj>` summary — the bar
that renders only on Work Items and only with a single project selected, which
is exactly the moment "I'm working on this one a lot right now" is true.
Planning reflects the band and cannot set it.

Two implementation notes that are not obvious and would each have been a bug:

- **`float-right`, not a flex summary.** `display: flex` on a `<summary>` drops
  its disclosure marker, and that triangle is the affordance saying the section
  opens. The float keeps `list-item` intact.
- **The click is stopped as well as prevented.** A `<button>` inside a
  `<summary>` toggles the disclosure on its way past, so starring would have
  opened the Project Details panel every single time.

The glyphs take the project's own `projectRailColor(...)` hue, which is D-3's
whole argument for text glyphs over an emoji pin: no pin can be recoloured or
has a true outline counterpart for the off state. 📌 stays Planning's
pinned-*proposals* mark and the two never meet. The words follow the glyph —
"Star korg" / "Unstar korg", never "Pin" — even though the work item is titled
"Pinned projects", which predates D-3.

### Verification

`tests/e2e/starred-projects.spec.ts` pins the two things a reasonable
implementation gets wrong: the project appears **twice** after starring, and
Planning shows the band while offering no control to set it — which is only
possible because the star is a column rather than this browser's storage.

One assertion was deliberately **removed** rather than fixed: that the band is
gone after unstarring. These suites run against a shared database where another
project may legitimately be starred, so the band's absence is not this test's
to claim — only that *this* project left it. An earlier version of the ordering
assertion had passed for the wrong reason — `indexOf` returning -1 against a
CSS-uppercased header reads as "the band is above it" — which is what prompted
checking what each assertion actually proves.

## Follow-ups

- **The band in alphabetical mode.** Ken's mock is explicitly "with category
  view enabled", and unchecking *by category* still gives the flat rail with no
  band. The navigation need is identical in both modes, but the band's grammar
  (a rule above the first category header) only exists in grouped mode, and
  adding dividers to a deliberately flat list is a change to a mode nobody
  asked about. Left alone on purpose — worth asking Ken.

## Deployed

**2026-08-26 to kubsdb** (`https://kubsdb.encke-wahoo.ts.net:5674/`), image
`kubsdb.encke-wahoo.ts.net:5000/korg:80f1c89093d9`, revision
`80f1c89093d991b5eaeae220db982758cb439ee5`. Rollback target
`korg:bd1d95c055e3` (sprint 073), confirmed present in the registry *before*
building rather than assumed.

Migration **0032** applied on startup: 31 → 32, the only schema change.

### Verified live

| Check | Result |
|---|---|
| Revision assertion in the deploy itself | running label == built commit |
| `post-deploy-check.sh --compare` | OK — every row count unchanged |
| Projects across the migration | 53 → 53, category distribution byte-identical |
| `starred` on `GET /api/projects` | present, `false` on all 53 |
| Star write round-trip (`PATCH /api/projects/korg`) | `true`, then restored to `false` |
| **An unrelated patch does not clear it** | `{"status":"active"}` left `starred: true` |
| `starred` absent from the lean `list_projects` | confirmed over MCP |
| `/`, `/work-items`, `/planning`, `/planning/1630` | 200 |

The unrelated-patch row is the one worth having run against production rather
than only against a test container. It is the failure this feature could
plausibly have shipped — an absent `starred` key deserialising to `Some(false)`
would make every ordinary project edit quietly unstar the project, and nothing
would look wrong until the band emptied a week later. The engine tests assert
it, and so now does the deployed instance.

All 53 projects came out of the migration unstarred, which is the honest
starting value: nothing had ever been starred, so the default states a fact
rather than guessing one. The band is therefore invisible until Ken stars
something, and the rails look exactly as they did before this shipped.
