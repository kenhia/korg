import { marked } from "marked";
import DOMPurify from "dompurify";

marked.setOptions({ breaks: true, gfm: true });

// Render trusted, single-user markdown to sanitized HTML.
export function renderMarkdown(src: string | null | undefined): string {
  if (!src) return "";
  const html = marked.parse(src, { async: false });
  return DOMPurify.sanitize(html);
}
