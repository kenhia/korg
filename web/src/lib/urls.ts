// Extract unique http(s) URLs from free text (markdown or plain), preserving
// first-seen order. Used to surface clickable launch links in edit panels.
const URL_RE = /https?:\/\/[^\s<>()\[\]"'`]+/gi;

// Trailing punctuation that is almost never part of the URL itself.
const TRAILING = /[.,;:!?)>\]}'"]+$/;

export function extractUrls(...texts: (string | null | undefined)[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const text of texts) {
    if (!text) continue;
    const matches = text.match(URL_RE);
    if (!matches) continue;
    for (const raw of matches) {
      const url = raw.replace(TRAILING, "");
      if (!url || seen.has(url)) continue;
      seen.add(url);
      out.push(url);
    }
  }
  return out;
}
