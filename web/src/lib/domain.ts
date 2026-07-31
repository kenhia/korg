// Presentation rules for korg's domain vocabulary — the single place the UI
// answers "is this finished?", "what do we call this?", "where does it sort?"
// (WI #542).
//
// These rules used to be re-derived as literals on every page: `"closed"` in
// six places (and a *different* terminal set on /plan), `"Cut"` in three,
// `kindLabel` copy-pasted twice, `midRank` duplicated in cards and planning,
// proposal statuses hardcoded beside an exported const nobody imported, and the
// relationship label `"related-to"` in two files plus a free-text input. Every
// vocabulary change was an N-file hunt, and the pages had already disagreed.
//
// The vocabularies themselves are generated from korg-core (`just gen`); this
// file is the thin layer of *judgement* on top of them.

import {
  CARD_STATUSES,
  PROJECT_CATEGORIES,
  RELATIONSHIP_LABELS,
  WI_STATUSES,
  type CardStatus,
  type ProjectCategory,
  type RelationshipLabel,
  type WiStatus,
} from "./generated/vocab";

// --- work-item lifecycle ----------------------------------------------------

/**
 * The UI asks two different questions about a finished work item, and the
 * review (F-15) read them as one rule applied inconsistently: /plan treated
 * `{done, closed, resolved}` as terminal while every other page checked only
 * `"closed"`.
 *
 * They are not the same question. "Should a default listing show this?" and
 * "does this still block the things that depend on it?" have genuinely
 * different answers, and each page was already asking the right one. So both
 * survive here, named for what they answer — collapsing them into a single
 * `isTerminal()` would have silently changed one page's meaning.
 */

/** The one status hidden from default listings. `closed` is Ken's, and
 *  reserved — agents stop at `done`. */
export const HIDDEN_BY_DEFAULT: WiStatus = "closed";

/** True when a listing should hide this item unless the user asks for it. */
export function isHiddenByDefault(status: string): boolean {
  return status === HIDDEN_BY_DEFAULT;
}

/** Work-item statuses a default listing shows. */
export function defaultVisibleStatuses(): WiStatus[] {
  return WI_STATUSES.filter((s) => s !== HIDDEN_BY_DEFAULT);
}

/**
 * Statuses under which a dependency no longer blocks its dependents — /plan's
 * question. Broader than "hidden by default": `resolved` means implemented, so
 * downstream work can proceed even though the item itself is not closed out.
 */
const SATISFIED_STATUSES: ReadonlySet<string> = new Set<WiStatus>([
  "resolved",
  "done",
  "closed",
]);

/** True when a work item no longer blocks the items that depend on it. */
export function isSatisfied(status: string): boolean {
  return SATISFIED_STATUSES.has(status);
}

// --- cards ------------------------------------------------------------------

/** The card column meaning "dropped", collapsed by default on the board. */
export const CUT: CardStatus = "Cut";

/** True when a card has been dropped rather than finished. */
export function isCut(status: string): boolean {
  return status === CUT;
}

/** Board columns in order, excluding the collapsed `Cut` strip. */
export function activeCardStatuses(): CardStatus[] {
  return CARD_STATUSES.filter((s) => s !== CUT);
}

// --- project categories -----------------------------------------------------

/**
 * The hue each project category paints in the Work Items rail (WI #678).
 *
 * This replaces `projColor(name)`, which hashed the project *name* to
 * `hsl(hash % 360, 55%, 62%)`. Deterministic, but arbitrary and non-scaling:
 * run over the live projects it put 20 of 31 within 12 degrees of another
 * (klams 326 / feedhub 327 were one degree apart) while leaving whole arcs
 * empty — nothing in 3-42, 211-246 or 250-292, and nine piled into 316-355.
 * "It feels random" because it was.
 *
 * Two things follow from *why* Ken wants this, and both invert the hash's
 * goals. The rail is the one place a long list of all active projects appears
 * at once, and it is scanned vertically — so the colors have to separate
 * against their *neighbours* in a list, not merely in isolation. And
 * same-category projects sharing one color is the feature, not a collision to
 * design away: the grouping is the thing being scanned for.
 *
 * The hues are "whatever project X is colored today", resolved against the old
 * hash, so several projects visibly change color (korg, kyac, kvllm,
 * hv-simulator, gh-kenhia) — expected, not a bug. `EVAL` is the one new color,
 * placed at the midpoint of the widest remaining gap (250-345).
 *
 * Typed `Record<ProjectCategory, number>` on purpose: adding a category to
 * korg-core's `PROJECT_CATEGORIES` and running `just gen` breaks this file
 * until its hue is chosen, which is the whole "one registry edit plus its
 * color" contract.
 */
const CATEGORY_HUE: Record<ProjectCategory, number> = {
  Ops: 42,
  Infrastructure: 64,
  AI: 157,
  Dashboard: 192,
  Fun: 250,
  EVAL: 297,
  Other: 345,
};

/**
 * The order categories appear in when the rail is grouped (Ken, 2026-07-29).
 *
 * Deliberately *not* alphabetical, and not hue order. It runs roughly from the
 * infrastructure Ken works in outward to the things he visits least, so the
 * groups he reaches for are at the top of the rail where the eye lands — the
 * same "scan by position" argument that motivated colouring at all. `EVAL` sits
 * last because it exists to make an eval-sandbox leak findable, not to be
 * navigated to.
 *
 * Reorder by editing this list. `satisfies` catches a typo'd name; anything in
 * the vocabulary but missing here is appended below rather than dropped, so a
 * category added later shows up in the rail even before someone places it.
 */
const RAIL_ORDER = [
  "Infrastructure",
  "Ops",
  "AI",
  "Dashboard",
  "Fun",
  "Other",
  "EVAL",
] as const satisfies readonly ProjectCategory[];

export const CATEGORY_ORDER: readonly ProjectCategory[] = [
  ...RAIL_ORDER,
  ...PROJECT_CATEGORIES.filter(
    (c) => !(RAIL_ORDER as readonly string[]).includes(c),
  ),
];

/**
 * The rail color for a project, or `undefined` for "render with no color".
 *
 * Archived projects get no color at all — not the `Other` color (WI #678,
 * decided 2026-07-26; `inactive` and `maintenance` left the vocabulary in
 * #828, so `archived` is now the whole of "not live"). Ken's taxonomy covers
 * the *active* set because
 * that is the set he scans, so colourlessness doubles as a signal that a
 * project is not live. A project with no category is likewise uncoloured: it is
 * genuinely uncategorised — `create_project` takes only a name — rather than
 * belonging to a catch-all. `Other` exists for ACTIVE projects outside the
 * taxonomy.
 *
 * An off-vocabulary category (a database that predates migration 0018) falls
 * through to `undefined` rather than throwing.
 */
export function projectRailColor(p: {
  status: string;
  category: string | null;
}): string | undefined {
  if (p.status !== "active") return undefined;
  if (p.category === null) return undefined;
  const hue = CATEGORY_HUE[p.category as ProjectCategory];
  return hue === undefined ? undefined : `hsl(${hue} 55% 62%)`;
}

// --- daily plan sources -----------------------------------------------------

const KIND_LABELS: Record<string, string> = {
  workitem: "WI",
  card: "Card",
  topic: "Topic",
  // Plannable since sprint 029 — "which sprint am I pushing on today" is the
  // same kind of answer as "which card".
  sprint_proposal: "Sprint",
};

/** The short chip label for a planned item's source kind. */
export function kindLabel(kind: string): string {
  return KIND_LABELS[kind] ?? kind;
}

// --- fractional ranking -----------------------------------------------------

/**
 * A rank that sorts strictly between `prev` and `next`.
 *
 * Ranks are fractional so a drag only rewrites the row that moved. With no
 * neighbour below, step past the last one; with none above, halve towards zero;
 * with neither, start at 0. Ranks arrive as strings because the server stores
 * them as an exact `Decimal`.
 */
export function midRank(prev?: string, next?: string): number {
  const before = prev === undefined ? undefined : Number(prev);
  const after = next === undefined ? undefined : Number(next);
  if (before !== undefined && after !== undefined) return (before + after) / 2;
  if (after !== undefined) return after - 1;
  if (before !== undefined) return before + 1;
  return 1;
}

// --- relationship labels ----------------------------------------------------

/** The labels korg itself writes or interprets, for a picker. Any other label
 *  is legal — the server stores a caller's order faithfully — so the UI offers
 *  these and a custom escape hatch rather than a bare free-text box. */
export const KNOWN_RELATIONSHIP_LABELS = RELATIONSHIP_LABELS;

/** The label to reach for when the user hasn't chosen one. It is the registry's
 *  only undirected label, which is what "just link these" means. */
export const DEFAULT_RELATIONSHIP_LABEL: RelationshipLabel = "related-to";

/** How a label reads left-to-right, for a tooltip or hint. */
export function relationshipReads(label: string): string | undefined {
  return RELATIONSHIP_LABELS.find((s) => s.label === label)?.reads;
}

/** False when the stored orientation of an edge carries no meaning, so the UI
 *  must present it symmetrically. Unknown labels are caller-defined and keep
 *  their direction. */
export function directionIsMeaningful(label: string): boolean {
  return RELATIONSHIP_LABELS.find((s) => s.label === label)?.directed ?? true;
}

// --- report status ----------------------------------------------------------

/**
 * The colour half of a report-status pill: emerald / amber / red for
 * `ok` / `attention` / `problem`.
 *
 * Lived on the Daily Reports page alone until Today grew a "latest report"
 * indicator (sprint 029). A second copy is precisely the drift this module
 * exists to stop — the two would eventually disagree about what "attention"
 * looks like, and a status colour that means different things on different
 * pages is worse than no colour.
 */
const REPORT_STATUS_STYLE: Record<string, string> = {
  ok: "bg-emerald-900/40 text-emerald-300 border-emerald-700",
  attention: "bg-amber-900/40 text-amber-300 border-amber-700",
  problem: "bg-red-900/40 text-red-300 border-red-700",
};

/** The complete class list for a report-status pill, shape included, so two
 *  call sites cannot render the same status at different sizes. */
export function reportStatusPill(status: string): string {
  const style =
    REPORT_STATUS_STYLE[status] ??
    "bg-[var(--color-surface-hi)] text-[var(--color-muted)] border-[var(--color-border)]";
  return `rounded-full border px-2 py-0.5 text-xs font-medium uppercase tracking-wide ${style}`;
}

// --- chips ------------------------------------------------------------------

/**
 * Chip classes, one definition per chip kind.
 *
 * Project, tag and category chips had two or three visual variants each
 * depending on the page, so the same value looked like a different kind of
 * thing in two places. These are the canonical ones — the kanban board's, which
 * were the only set that colour-coded the three kinds distinctly.
 *
 * **Tags carry a hue (WI #571).** They used to be
 * `bg-[var(--color-surface-hi)] text-[var(--color-muted)]`, and "tags fade into
 * the background" turned out not to be a text-contrast problem at all — the
 * muted-on-surface-hi text measured 5.23:1, comfortably past WCAG AA, so an
 * axe-core pass looked straight at this bug and reported nothing. The problem
 * was that the chip painted its *container* in the same token as the thing
 * behind it: 1.00:1 against a `surface-hi` kanban tile, 1.15:1 on a `surface`
 * panel. A chip reads as a chip because it is a container, and this one had no
 * edge.
 *
 * So the fix is a third hue, not a darker grey — measured against the two
 * chips that already worked rather than picked by eye. Contrast of the chip
 * background against its container, and of the label against the chip
 * (`surface` / `surface-hi` / page `bg`):
 *
 * | chip | edge | label |
 * |---|---|---|
 * | project (teal, unchanged) | 1.50 / 1.36 / 1.59 | 7.86 / 7.56 / 8.03 |
 * | category (violet, unchanged) | 1.40 / 1.26 / 1.49 | 8.91 / 8.63 / 9.06 |
 * | tag **before** | 1.15 / **1.00** / 1.25 | 5.23 (flat, no hue) |
 * | tag **after** | 1.63 / 1.46 / 1.75 | 8.51 / 8.28 / 8.63 |
 *
 * The new tag clears both existing chips on edge separation and sits between
 * them on label contrast. korg has a single dark theme (`app.css`), so "both
 * themes" in the sprint proposal is read as both *surface levels* — which is
 * the distinction that actually produced the bug.
 */
export const chip = {
  project: "rounded bg-teal-900/60 px-1.5 py-0.5 text-xs text-teal-300",
  category: "rounded bg-violet-900/70 px-1.5 py-0.5 text-xs text-violet-200",
  tag: "rounded bg-amber-900/70 px-1.5 py-0.5 text-xs text-amber-200",
} as const;
