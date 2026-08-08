# 057 — Images UI: paste, paperclip, lightbox

Proposal `korg:1124`, work items #1120 and #1121. Slice 2 of the **Images in
korg** program (`korg:1127`). The design record is the program handoff
(`korg:1128`); slice 1 (sprint 056) built the engine this sits on and was
deployed to kubsdb before this started, as the program's sequencing requires.

## Goal

Ken pastes a screenshot into a work item or a comment and sees it inline. Then
the explicit half: a paperclip for files that belong to the item without
belonging in its prose, a list of everything attached, and the one place an
image can actually be destroyed.

## What was already decided

D2 (attachment is a node, markdown is placement only), D3 (`img-<hex>`), D5
(paste uploads immediately as `pending`, linked on save, sweeper is the whole
GC story), D6 (bytes over REST). Sprint 056's I3 — a comment is not a node, so
an image pasted into one is owned by the node the comment hangs off — decides
where a comment's images show up, which is the only question this slice had to
ask about ownership.

No Rust changed this sprint. Every route, row and edge this needed already
existed, which is the shape a correctly-sequenced slice should have.

## Implementation decisions

**I1 — Pending-then-link even when editing an existing item.** The obvious
shortcut is to upload with `?owner=` when the item already exists. It is wrong:
nothing sweeps a *linked* attachment, so a paste followed by Cancel would leave
an image attached to the item forever. Uploading pending in both cases makes
Cancel cost nothing and gives the identical result on Save. The paperclip is
the exception and uploads with the owner set, because it is not
paste-before-save — there is a node already and nothing to cancel.

**I2 — A placeholder, not a remembered offset.** The paste inserts
`![uploading image N…]()` at the caret in the same tick and swaps it for the
real token when the upload resolves. Remembering the caret offset instead would
put the token in the wrong place the moment someone kept typing — which is
exactly what D5's "never block typing" invites them to do. The swap is a string
replace, so a user who deletes the placeholder mid-upload gets what they asked
for: no token, and a pending attachment the sweeper takes.

Save awaits in-flight uploads (`PasteUploads.settled()`). A save that raced an
upload would persist the placeholder text.

**I3 — Comments render tokens, not markdown.** Comment bodies display as
literal text throughout korg — `#582` is a work-item reference, not a heading —
so turning them into markdown to get pictures would change how every comment
ever written displays. `splitImgTokens` lifts out the one token korg writes and
leaves everything else alone. That is also #1121's "cross-reference by
`img-<hex>`, no duplicated upload chrome": the comment shows the picture, the
node's attachment list owns the management.

**I4 — One renderer, one lightbox.** The markdown `image` renderer emits a
thumbnail *button* carrying `data-img-id` for korg's own URLs only (an image
hosted elsewhere is still an image and gets no chrome), and `MarkdownView`
delegates clicks from it. Replacing the seven hand-rolled
`{@html renderMarkdown(…)}` call sites with that component is what gets images
into the review page, the node preview, handoffs, programs and daily reports
without any of them knowing about images.

**I5 — The attachment list is not fetched.** `get_work_item` already inlines
`attachments`, so the panel renders what the detail read carried and asks the
page to reload after a change — the same pattern `related` has used since LB-3.
A second read would be a second copy of the same truth.

**I6 — `inline` is computed from the prose, not stored.** The indicator asks
"does any text place this id", scanning content, details and the loaded comment
bodies. It matches on the id rather than on the exact token korg writes, so a
hand-edited URL still counts as placed.

## Shipped

- **`lib/img.ts`** — the client-side mirror of `korg_img`'s vocabulary: id
  spelling, serve URLs, the token, `splitImgTokens`, and the inline-or-not
  predicate.
- **`lib/imagePaste.svelte.ts`** — `PasteUploads` (the pending ledger, one per
  editor) and `pasteImages`, a Svelte action giving any textarea paste and drop.
- **`lib/api.ts`** — `uploadImage` (its own `FormData` request, because
  multipart sets its own boundary), `linkImage`, `deleteImage`.
- **`markdown.ts` + `MarkdownView.svelte`** — the thumbnail-control renderer and
  the component that opens the lightbox, now used by every prose surface.
- **`Lightbox.svelte`** — full resolution over a `Dialog`, with a link to the
  original.
- **`AttachmentPanel.svelte`** — paperclip, list (thumbnail, id, filename,
  dimensions, size, `pending`/`inline`/`not inline`), discard behind a confirm.
- **`CommentBody.svelte`** + `Comments.svelte` — paste in the new-comment and
  edit boxes, tokens rendered, uploads claimed on post, and an
  `onAttachmentsChanged` callback so the owning page can refresh its list.
- **`WorkItemForm.svelte`** — paste on content and details, a hint that says so,
  in-flight count, uploads claimed after save.
- **Docs** — a sixth UI rule in `usage.md`, the token an agent should write in
  `api.md`'s images section, and a `KORG_IMG_ROOT` warning on both places that
  tell you to start `korg-api` by hand (`setup.md`'s e2e instructions and the
  README quickstart): its default is the container path, so a dev box that
  follows them and then pastes an image gets a permission error naming a
  directory nobody asked for.
- **Tests** — `web/tests/e2e/images-ui.spec.ts`: paste-to-token-to-thumbnail-to-
  lightbox-to-discard, paste into a comment, and the paperclip's "not inline"
  case. Real PNG bytes painted in the page and pushed through a synthetic
  `ClipboardEvent`, against a real store — the interesting failures in this
  feature all live in the seam between the clipboard and the server.

### Two things found by running it

**Escape closed two things at once.** `showModal()` makes the rest of the page
inert for pointer and focus, but a `keydown` still bubbles to `window` — so
Escape out of the lightbox also ran the work-items page's own Escape handler
and closed the detail panel underneath. The e2e spec caught it on the first
run. `Dialog` now counts its open modals and exports `modalOpen()`, and the
three pages with window-level key handlers stand down while one is up. This was
never lightbox-specific: the node preview had the same behaviour, unnoticed.

**`work-item-edit.spec.ts` had drifted.** It asserts the relationship-label
picker offers exactly the registry, and sprint 056 added `has_attachment`
without the suite running afterwards. One line — and the fourth time this exact
list has gone stale, for the reason its own comment predicts: e2e is out of CI,
so a registry addition is only noticed the next time somebody runs the suite.

## Follow-ups

- **The attachment list is on work items only.** A card or a proposal with a
  pasted image renders it inline (the token is enough) but has no panel, because
  no focused read but `get_work_item` inlines `attachments` — sprint 056's own
  follow-up. Widening it is a repo change plus this component, unchanged.
- **A cancelled edit leaves a pending upload.** By design (D5) — the sweeper
  takes it in 24 hours. Deleting on Cancel would be tidier and is also how you
  delete an image someone pasted, cancelled, and then wanted back.
- **Paste in the card editor.** The card editor's textareas do not use
  `pasteImages` yet; the action is per-textarea, so it is a two-line change when
  something wants it.

## Deployed

**2026-08-08** — `kubsdb.encke-wahoo.ts.net:5000/korg:7cf580b88991`
(digest `sha256:00d5f30b…86637`), built from the squash-merge `7cf580b`.
Rollback target: `korg:e06c77842de3` (sprint 056), confirmed present in the
registry before building.

**Rollback is a plain re-tag.** This sprint added no migration — `migrations`
is still 27 on both sides of the deploy — so the previous image runs correctly
against the current schema, and nothing this sprint wrote to disk is a format
the old image cannot read.

### Verified live

`post-deploy-check.sh --compare` clean: every row count identical (767 work
items, 30 cards, 4 links, 175 proposals, 33 reports, 40 projects), node count
1029 and `node_id_seq` both unmoved, migrations 27 → 27 — the shape a
no-migration deploy should have.

Then this sprint's own work, which is a *bundle*, so the check is that the
bundle kubsdb serves is this one:

- **Stylesheet** — the served `assets/0.*.css` carries the `.korg-thumb` rules.
- **Route bundles** — pulling every `nodes/*.js` chunk and its shared chunks
  finds `uploading image` (the paste placeholder), `Ctrl-V an image` (the
  editor hint, twice — content and comment), `korg-thumb`, `open full size`,
  `not inline` and `lightbox-image`. That is each of the sprint's four surfaces
  — paste, inline thumbnail, attachment list, lightbox — present in the code
  actually being served, rather than inferred from the image tag.
- **Deep links** — `/`, `/work-items`, `/planning`, `/api/health` all 200.

And one live round trip, because the container was **recreated** and the image
store is a bind mount — the one thing a compose recreate could plausibly get
wrong. A 1200×600 PNG uploaded to the deployed instance: `img-46a`, `pending`,
`thumb` 400×200 and `agent` 1200×600 (no upscale, as specified), all three
served with the right byte counts, then `DELETE` → 404 and
`/api/img/stats` back to `total_bytes: 0`. Production carries no residue from
the test.
