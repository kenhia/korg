// The `img-<hex>` display id, as the browser handles it (#1120, #1121).
//
// The server owns this vocabulary — `korg_img::ImgId` and `ROUTE_PREFIX` — and
// this file is its client-side mirror, kept deliberately tiny: id spelling, the
// serve URLs, and the markdown token that places an image in prose. Everything
// else about an attachment arrives as an `AttachmentRow` from the API.
//
// Placement is markdown; existence is not (handoff D2). These helpers write and
// recognise the token, and nothing here ever decides whether an attachment
// exists — that is the `has_attachment` edge's job, server-side.

/** Where the serve routes live. Mirrors `korg_img::ROUTE_PREFIX`. */
export const IMG_ROUTE_PREFIX = "/api/img";

/** The class every inline thumbnail control carries, whether it was generated
 *  by the markdown renderer or written as markup in a component. `app.css`
 *  sizes it; `MarkdownView` delegates clicks from it. */
export const THUMB_CLASS = "korg-thumb";

/** The generated sizes. `thumb` displays, `agent` is for agent reads. */
export type ImgVariant = "thumb" | "agent";

// Parsed case-insensitively, like the server: the `IMG-C2A` spelling from the
// design record resolves to the same image as `img-c2a`.
const IMG_ID = /img-[0-9a-f]+/i;

/** The markdown token that places an image: `![img-c2a](/api/img/img-c2a/thumb)`.
 *
 *  Anchored on the *alt text* rather than the URL, because the alt text is what
 *  korg writes and what survives a hand-edit — someone who retypes the URL, or
 *  points it at the original instead of the thumb, still has a token korg
 *  recognises. Global, so `matchAll` finds every image in a body. */
export const IMG_TOKEN_RE = /!\[(img-[0-9a-f]+)\]\(([^)\s]*)\)/gi;

/** The canonical spelling of an id: lowercase, `img-` prefixed. Returns null for
 *  anything that is not one, so callers never have to pre-validate. */
export function normalizeImgId(raw: string | null | undefined): string | null {
  if (!raw) return null;
  const m = raw.match(new RegExp(`^${IMG_ID.source}$`, "i"));
  return m ? m[0].toLowerCase() : null;
}

/** The display id of the attachment with this node id (#1467).
 *
 *  Mirrors `korg_img::ImgId`, which is the node id itself rendered as hex —
 *  derived rather than a second sequence, so the two can never disagree. Added
 *  when attachments got a page of their own: the route is addressed by node id
 *  like every other kind's, and the image behind it is addressed by display id.
 *
 *  Returns null for a non-positive id, matching `ImgId::from_node_id`'s refusal
 *  rather than inventing `img-0`. */
export function imgIdFromNodeId(node_id: number): string | null {
  return node_id > 0 ? `img-${node_id.toString(16)}` : null;
}

/** Where to fetch an image: the original with no variant, `thumb` or `agent`. */
export function imgUrl(id: string, variant?: ImgVariant): string {
  const base = `${IMG_ROUTE_PREFIX}/${id.toLowerCase()}`;
  return variant ? `${base}/${variant}` : base;
}

/** The id a serve URL addresses, or null if it addresses something else.
 *
 *  Used to tell korg's own images apart from any other `<img>` in a body — an
 *  externally-hosted screenshot pasted as a plain markdown link is still a
 *  perfectly good image, it just is not an attachment and gets no chrome. */
export function imgIdFromUrl(url: string | null | undefined): string | null {
  if (!url) return null;
  const m = url.match(
    new RegExp(`^${IMG_ROUTE_PREFIX}/(${IMG_ID.source})(?:/(?:thumb|agent))?$`, "i"),
  );
  return m ? m[1].toLowerCase() : null;
}

/** The token to insert at the cursor when an image is pasted.
 *
 *  Points at the `thumb` variant: the inline rendering is a thumbnail control
 *  and the lightbox fetches the original on demand, so a body with a dozen
 *  images costs a dozen thumbnails rather than a dozen 4K screenshots. */
export function imgToken(id: string): string {
  return `![${id.toLowerCase()}](${imgUrl(id, "thumb")})`;
}

/** Whether a body places this image. What the attachment list's
 *  inline-or-not indicator reports (#1121).
 *
 *  Matches on the id, not on the exact token korg would write, so an image
 *  referenced by a hand-typed `![img-c2a](/api/img/img-c2a)` still counts as
 *  inline — the question the indicator answers is "does this appear in the
 *  prose", not "is this token byte-identical to ours". */
export function bodyPlacesImg(
  id: string,
  ...bodies: (string | null | undefined)[]
): boolean {
  const wanted = id.toLowerCase();
  for (const body of bodies) {
    if (!body) continue;
    for (const [, found] of body.matchAll(IMG_TOKEN_RE)) {
      if (found.toLowerCase() === wanted) return true;
    }
  }
  return false;
}

/** Every attachment id a body places, first-seen order, deduplicated. */
export function imgIdsIn(...bodies: (string | null | undefined)[]): string[] {
  const seen = new Set<string>();
  for (const body of bodies) {
    if (!body) continue;
    for (const [, id] of body.matchAll(IMG_TOKEN_RE)) seen.add(id.toLowerCase());
  }
  return [...seen];
}

/** Human-readable byte size for the attachment list. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
