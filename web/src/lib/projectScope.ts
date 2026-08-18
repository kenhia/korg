/**
 * The selected project, shared by the Work Items and Planning rails (WI #1310).
 *
 * The two pages each kept their own sticky key, so bouncing between them meant
 * bouncing between two selections: pick `k-homelab` on Work Items, switch to
 * Planning, and it is still scoped to korg. Ken's report is exactly that — "I
 * end up confusing myself as I might select `k-homelab` in one, then switch to
 * the other and it's giving me details about `korg`".
 *
 * One key, one sentinel, and deliberately only for the *project*. The pages'
 * other sticky preferences stay per-page (`korg.workitems.groupByCategory`,
 * `korg.planning.groupByCategory`) for the reason the Planning rail already
 * records: the two rails answer different questions and may well want to be
 * sorted differently. A project is a scope — where you are, not how you are
 * looking — and a scope is the thing worth carrying across a navigation.
 *
 * No migration from the two retired keys (`korg.workitems.project`,
 * `korg.planning.project`). Skipping it costs one click, once — Work Items
 * falls back to the most recently touched project when nothing is stored, so
 * the landing is sensible either way — and a legacy-read branch here would
 * outlive its usefulness by years.
 *
 * SSR-safe and failure-tolerant: `localStorage` is absent during prerender and
 * throws outright in some sandboxed contexts, and neither is a reason for a
 * page not to render.
 */

/** No project filter — "All projects" on either rail. Empty rather than a
 *  sentinel string because a project name can never be empty, so there is
 *  nothing for it to collide with. */
export const ALL_PROJECTS = "";

const KEY = "korg.project";

/** The stored scope, or null when nothing has been chosen yet. Null and
 *  `ALL_PROJECTS` are different answers: the callers treat "never chosen" as
 *  an invitation to pick a default. */
export function readProjectScope(): string | null {
  try {
    return typeof localStorage !== "undefined" ? localStorage.getItem(KEY) : null;
  } catch {
    return null;
  }
}

export function writeProjectScope(name: string): void {
  try {
    if (typeof localStorage !== "undefined") localStorage.setItem(KEY, name);
  } catch {
    /* storage unavailable — non-fatal */
  }
}
