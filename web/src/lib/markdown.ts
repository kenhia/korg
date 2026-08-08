import { marked, type Tokens } from "marked";
import DOMPurify from "dompurify";
import { imgIdFromUrl, imgUrl, THUMB_CLASS } from "./img";

marked.setOptions({ breaks: true, gfm: true });

// An `![img-<hex>](…)` token renders as a thumbnail *control*, not a bare image
// (#1120): a button, so it is focusable and answers Enter as well as a click,
// carrying the id in `data-img-id` for the lightbox to pick up.
//
// Only korg's own attachments get this treatment. A markdown image pointing
// anywhere else is still a perfectly good image — it just is not an attachment,
// has no full-resolution variant to open, and gets rendered plainly.
//
// The rendered `src` is always the `thumb` variant regardless of what the token
// says, so a hand-typed token pointing at the original still costs a thumbnail
// to display. The lightbox is what fetches full resolution, on demand.
marked.use({
  renderer: {
    image({ href, title, text }: Tokens.Image): string {
      const id = imgIdFromUrl(href);
      const alt = escapeAttr(text || id || "");
      if (!id) {
        const t = title ? ` title="${escapeAttr(title)}"` : "";
        return `<img src="${escapeAttr(href)}" alt="${alt}"${t}>`;
      }
      const label = title ? escapeAttr(title) : `${id} — open full size`;
      return (
        `<button type="button" class="${THUMB_CLASS}" data-img-id="${id}" title="${label}">` +
        `<img src="${escapeAttr(imgUrl(id, "thumb"))}" alt="${alt}" loading="lazy">` +
        `</button>`
      );
    },
  },
});

function escapeAttr(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

// Render trusted, single-user markdown to sanitized HTML.
export function renderMarkdown(src: string | null | undefined): string {
  if (!src) return "";
  const html = marked.parse(src, { async: false });
  return DOMPurify.sanitize(html);
}
