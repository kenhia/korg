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
  NODE_ROUTES,
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

/**
 * The status for work that is real and wanted but waiting on a condition rather
 * than on a person (WI #810) — #406 and #780 were the examples Ken gave.
 *
 * It is a *third* question about a work item, alongside the two above, and the
 * reason it gets its own name here: parked items are neither hidden nor
 * satisfied. They stay in the listing (hiding them is what `closed` is for) and
 * they still block their dependents (deferred is not done) — they simply must
 * not sit among the rows you are choosing your next task from.
 */
export const PARKED: WiStatus = "parked";

/** True when an item is deferred until some condition fires. */
export function isParked(status: string): boolean {
  return status === PARKED;
}

/**
 * Split a listing into what is actionable and what is parked, preserving the
 * incoming order within each half.
 *
 * A `sort()` comparator would have been the smaller change and the wrong one:
 * the caller's order is not always `wi_number` (it is whatever the page sorted
 * by), and a comparator that returns 0 for two same-half rows is only
 * order-preserving because `Array.prototype.sort` happens to be stable — a
 * property to rely on deliberately, not by accident. Partitioning says what is
 * meant, and gives the caller the two lists it needs to draw a divider between.
 */
export function partitionParked<T extends { wi_status: string }>(
  items: readonly T[],
): { active: T[]; parked: T[] } {
  const active: T[] = [];
  const parked: T[] = [];
  for (const it of items) (isParked(it.wi_status) ? parked : active).push(it);
  return { active, parked };
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
 * hv-simulator, gh-kenhia) — expected, not a bug. `EVAL` was the one new color,
 * placed at the midpoint of the widest remaining gap (250-345); `Tools`
 * (2026-08-21, WI #1505) follows the same rule at the midpoint of what was then
 * the widest gap, 64-157. Picking that way keeps every pair of categories as
 * far apart as the wheel still allows, which is what a rail scanned by color
 * needs.
 *
 * Typed `Record<ProjectCategory, number>` on purpose: adding a category to
 * korg-core's `PROJECT_CATEGORIES` and running `just gen` breaks this file
 * until its hue is chosen, which is the whole "one registry edit plus its
 * color" contract.
 */
/**
 * "A live proposal already covers this" (WI #824), as one exported constant so
 * the row title, the Prop column and the Planning rail cannot drift apart.
 *
 * It is the Ops hue, which is what Ken meant by "the colour `claude-cleo` is
 * in": a name-hash accident when he first described it, and the Ops category
 * colour since #678 — so reusing it here is stable rather than arbitrary. The
 * Planning page already paints an *active* proposal's card border the same
 * hue (#671), which makes warm consistently mean "this is spoken for" across
 * both pages rather than two unrelated warm things.
 *
 * Written out rather than `CATEGORY_HUE.Ops` interpolated, because this is not
 * a claim about the Ops *category* — a work item in an Ops project is not what
 * is being marked. If the two ever need to diverge, they can.
 */
export const IN_PROPOSAL_COLOR = "hsl(42 55% 62%)";

const CATEGORY_HUE: Record<ProjectCategory, number> = {
  Ops: 42,
  Infrastructure: 64,
  Tools: 110,
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
 * navigated to. `Tools` sits third (Ken, 2026-08-21): the homelab's own
 * instruments are reached for about as often as the infrastructure they run on,
 * so they belong beside `Infrastructure` and `Ops` rather than out past `AI`.
 *
 * This list drives BOTH grouped lists — the Work Items rail and the Planning
 * page's — through `CATEGORY_ORDER`, so a placement is decided once here and
 * never per page.
 *
 * Reorder by editing this list. `satisfies` catches a typo'd name; anything in
 * the vocabulary but missing here is appended below rather than dropped, so a
 * category added later shows up in the rail even before someone places it.
 */
const RAIL_ORDER = [
  "Infrastructure",
  "Ops",
  "Tools",
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

// --- IDs, everywhere an agent might cite one (WI #980) -----------------------

/**
 * How an id renders. One constant, because the point of the sweep is that an id
 * looks the same everywhere — Ken reads agent output constantly, and matching
 * "korg:979" against a screen is a scanning task, not a reading one.
 *
 * The style is the Work Items list's, which is the surface that already had it
 * right: mono so digits line up column-wise, muted so a screenful of ids never
 * competes with the titles they annotate.
 */
export const ID_CLASS = "font-mono text-xs text-[var(--color-muted)]";

// --- node timestamps (WI #1106) ----------------------------------------------

/**
 * `created`/`updated` as a reader wants them. Lifted out of the handoff page,
 * which had the only copy, when the generic preview grew the same need: every
 * node kind carries these two columns and until #1106 only three surfaces
 * rendered them, so "when was this queued?" meant an MCP call.
 *
 * Locale format rather than the `YYYY-MM-DD` the schedules page slices to —
 * these answer "how current is this", where the time of day is part of the
 * answer, whereas a schedule's anchor is a date and never a moment.
 *
 * Returns the input unchanged if it does not parse: a visible raw timestamp is
 * a better failure than `Invalid Date`.
 */
export function stamp(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
}

/**
 * `stamp` without the time of day, as `YYYY-MM-DD` in the *reader's* zone.
 *
 * Deliberately not `iso.slice(0, 10)`, which is what the schedules page does to
 * `due_at`. That is correct there and wrong here: `due_at`/`anchor_at` arrive as
 * `to_char(…, 'YYYY-MM-DD')` date text with no zone to get wrong, whereas
 * `created`/`updated`/`materialized` are RFC3339 instants in UTC, and slicing
 * one renders the UTC day — off by one for anybody west of Greenwich for part
 * of every day. The shape stays `YYYY-MM-DD` so these read as the same kind of
 * thing as the dates beside them.
 */
export function stampDay(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${mm}-${dd}`;
}

// --- program slice progress (WI #980) ----------------------------------------

/** The per-slice work-item rollup `get_board`/`get_program` already carries. */
export type SliceCounts = {
  covered_count: number;
  open: number;
  resolved: number;
  done: number;
  closed: number;
};

/**
 * The three states a slice's work can be in, and the two percentages that draw
 * them.
 *
 * Ken misread program 979 — everything implemented, nothing closed — as "still
 * has implementation to do", because the old two-part count put his *closed*
 * count on the left where a "how much is finished" figure goes. `0/3` is a true
 * statement about verification and a false one about work, and the display gave
 * no way to tell which question was being answered.
 *
 * So the figure answers both, in D-7's own vocabulary:
 *
 * - **work-complete** = `resolved + done + closed` — everything agents consider
 *   finished. `resolved` counts here because that is exactly what it means:
 *   implemented, awaiting Ken.
 * - **Ken-verified** = `closed` — the status reserved for him.
 *
 * `N / <N / N` is the state worth spotting across the room: all the work is
 * done and it is waiting on Ken. Both the triplet and the bar are computed
 * here so they cannot disagree — the bug being fixed is a display that
 * contradicted itself, and two call sites doing their own arithmetic is how it
 * would come back.
 *
 * Display-layer only, deliberately: `get_board`'s slices already carry these
 * four counters and kfdc's Operations panel is a second consumer of the same
 * fields, so nothing new goes on the read.
 */
export function sliceProgress(s: SliceCounts): {
  workComplete: number;
  verified: number;
  total: number;
  /** Width of the whole finished run, verified part included. */
  workPct: number;
  /** Width of the verified run alone, drawn over the finished one. */
  verifiedPct: number;
} {
  const total = s.covered_count;
  const workComplete = s.resolved + s.done + s.closed;
  const verified = s.closed;
  // An empty slice is not "complete", it is empty, and the bar says so by
  // staying blank rather than reading 100%.
  const pct = (n: number) => (total === 0 ? 0 : Math.round((n / total) * 100));
  return {
    workComplete,
    verified,
    total,
    workPct: pct(workComplete),
    verifiedPct: pct(verified),
  };
}

/** A proposal that is closed out, whichever way it went. Declined counts:
 *  nothing is waiting on it. */
const SLICE_FINISHED_STATUSES = ["done", "declined"];

/**
 * Why this slice is not finished, or null when it is (WI #1168).
 *
 * Ken's ask is a program close-out that says "not all the slices are complete,
 * you sure?", so something has to decide what complete means, and the two
 * halves of the answer are different vocabularies:
 *
 * - the **slice** is a sprint proposal, and its terminal statuses are `done`
 *   and `declined`;
 * - its **work items** are complete at `resolved + done + closed` — #978's
 *   *completion* split, not the visibility one. `resolved` means implemented
 *   and awaiting Ken, and a program he is about to close by hand is precisely
 *   the case where that counts as done. `covered_count` includes `parked`
 *   (#1386), so parked work lands in the remainder here rather than vanishing
 *   — deferred indefinitely is still not finished.
 *
 * A reason string rather than a boolean because the dialog has to *say* which
 * slices and why: "are you sure" with no evidence is the kind of confirm people
 * learn to click through.
 */
export function sliceUnfinished(s: SliceCounts & { status: string }): string | null {
  const reasons: string[] = [];
  if (!SLICE_FINISHED_STATUSES.includes(s.status)) reasons.push(`still ${s.status}`);
  const remaining = s.covered_count - (s.resolved + s.done + s.closed);
  if (remaining > 0) {
    reasons.push(`${remaining} of ${s.covered_count} work items unfinished`);
  }
  return reasons.length > 0 ? reasons.join(", ") : null;
}

/**
 * The bar's two hues, reusing meanings the rest of the UI already carries so
 * the colour is readable before the legend is: emerald is a finished status
 * pill, amber is the awaiting-Ken lane. A fully amber bar therefore *means*
 * "all of this is waiting on you", which is the signal #980 is about.
 */
export const PROGRESS_VERIFIED_CLASS = "bg-emerald-500";
export const PROGRESS_WORK_CLASS = "bg-amber-500";

// --- where a node lives (WI #981, #982, #1467) -------------------------------

/**
 * The kinds with a page of their own, and how to build it.
 *
 * **Nine entries, and there is no second set.** This used to be a
 * hand-maintained `OWN_PAGE` of three kinds beside a `LIST_PAGE` of four,
 * because most kinds had no route to send anyone to — the gap #981's awaiting
 * lane routed around, kfdc #993 degraded to plain text rather than fake, and
 * korg-vs's resolve-by-ID had nothing to resolve to. #1467 gave every kind a
 * page, so the fallback tier is gone and the table is generated: korg-core owns
 * it, `/n/:node_id` and every `url` korg returns are built from the same rows,
 * and this file cannot drift from them.
 */
export function nodePage(kind: string, node_id: number): string | null {
  const template = (NODE_ROUTES as Record<string, string | undefined>)[kind];
  return template ? template.replace("{id}", String(node_id)) : null;
}

// `nodeHref(kind, node_id, wi_number)` used to sit here as the "own page, or
// the work-items list, or the kind's list page" cascade. #1467 removed every
// tier below the first — each kind has its own page now — which left it as a
// second name for `nodePage` plus a `wi_number` argument that could not matter,
// since a work item's node id **is** its `wi_number` (0009). Callers ask
// `nodePage` directly.
//
// Prefer the `url` korg already put on the payload where there is one
// (`NodePreview.url`, `SearchHit.url`): a comment hit's URL carries a
// `#comment-<id>` anchor this cannot reconstruct, because the response gives a
// comment's owning node id without its kind.

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

/**
 * Source-freshness colours (#950).
 *
 * `stale` is the only alarming one, and it is deliberately the *same* red a
 * `problem` report gets: the whole lesson of the July 2026 outage is that a
 * dead monitor and a monitor reporting a fire deserve the same amount of
 * attention. `retired` and `unrated` are muted — neither is a problem, and
 * colouring them would train the eye to skip the panel.
 */
const SOURCE_FRESHNESS_STYLE: Record<string, string> = {
  fresh: "bg-emerald-900/40 text-emerald-300 border-emerald-700",
  stale: "bg-red-900/40 text-red-300 border-red-700",
  retired: "bg-[var(--color-surface-hi)] text-[var(--color-muted)] border-[var(--color-border)]",
  unrated: "bg-[var(--color-surface-hi)] text-[var(--color-muted)] border-[var(--color-border)]",
};

/** The complete class list for a source-freshness pill. */
export function sourceFreshnessPill(freshness: string): string {
  const style =
    SOURCE_FRESHNESS_STYLE[freshness] ??
    "bg-[var(--color-surface-hi)] text-[var(--color-muted)] border-[var(--color-border)]";
  return `rounded-full border px-2 py-0.5 text-xs font-medium uppercase tracking-wide ${style}`;
}

/**
 * How a schedule's due-ness reads.
 *
 * Three states, not two: `due` wants doing, `outstanding` means it already
 * produced a work item that is still open (so the *work item* is the surface,
 * not the schedule), and everything else is simply waiting.
 */
export function scheduleDuePill(due: boolean, outstanding: boolean): string {
  const style = outstanding
    ? "bg-sky-900/40 text-sky-300 border-sky-700"
    : due
      ? "bg-amber-900/40 text-amber-300 border-amber-700"
      : "bg-[var(--color-surface-hi)] text-[var(--color-muted)] border-[var(--color-border)]";
  return `rounded-full border px-2 py-0.5 text-xs font-medium uppercase tracking-wide ${style}`;
}

/** The word that goes in the pill above. */
export function scheduleDueLabel(due: boolean, outstanding: boolean): string {
  if (outstanding) return "in flight";
  return due ? "due" : "waiting";
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
