// Transient user-facing messages — the one place the UI says something went
// wrong (WI #547).
//
// Before this, korg had five error styles across ten pages and, far worse, a
// long tail of mutations with no error handling at all: `await api.deleteComment(id)`
// with no try/catch is an unhandled rejection and a UI that looks like it
// worked. The review found that shape in Comments, reading-list, cards and most
// of work-items.
//
// The rule this module exists to make cheap: **every mutation reports its
// failure**. If reporting is one import and one line, it happens; if it is a
// bespoke `errorMsg` state variable plus markup per page, it doesn't.

import { ApiError, NetworkError } from "./api";

export type ToastKind = "error" | "success";

export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
  /** Rendered as an "Undo" button when present (used by archive actions). */
  undo?: () => void;
}

let nextId = 1;

export const toasts = $state<Toast[]>([]);

function push(t: Omit<Toast, "id">): number {
  const id = nextId++;
  toasts.push({ ...t, id });
  return id;
}

export function dismiss(id: number): void {
  const i = toasts.findIndex((t) => t.id === id);
  if (i !== -1) toasts.splice(i, 1);
}

/** How long a success stays up. Errors never auto-dismiss — see below. */
const SUCCESS_MS = 3000;

export function notify(text: string, undo?: () => void): void {
  const id = push({ kind: "success", text, undo });
  // Undoable toasts stay longer: the whole point is that you get a moment to
  // notice and change your mind.
  setTimeout(() => dismiss(id), undo ? SUCCESS_MS * 2 : SUCCESS_MS);
}

/**
 * Report a failed action.
 *
 * `doing` is the action in the imperative — "Delete comment", "Save card" —
 * not a finished sentence. Call sites that build their own sentences are how
 * korg ended up with five phrasings of the same thing, so this composes the
 * sentence and they don't get the chance.
 *
 * Errors are deliberately **not** auto-dismissed. A message that vanishes
 * before you look back at the screen is the silent failure this whole WI is
 * about, just slower; the user closes it when they have read it.
 */
export function reportError(e: unknown, doing: string): void {
  push({ kind: "error", text: `${doing} failed — ${describe(e)}` });
  // The console keeps the machine-facing detail the UI omits.
  console.error(`${doing} failed`, e);
}

function describe(e: unknown): string {
  if (e instanceof NetworkError) return e.message;
  if (e instanceof ApiError) {
    // `internal` means korg broke, and its message is a server-side detail
    // that reads as noise to a user who did nothing wrong.
    return e.code === "internal"
      ? "korg hit an internal error. It may be worth retrying."
      : e.detail;
  }
  return e instanceof Error ? e.message : String(e);
}

/**
 * Run a mutation, reporting failure and never throwing.
 *
 * Returns the result, or `undefined` if it failed — so a caller can update
 * local state only on success:
 *
 * ```ts
 * const c = await attempt(() => api.addComment(id, body), "Add comment");
 * if (c) comments = [...comments, c];
 * ```
 *
 * The `if` is the point. Optimistically mutating local state and then leaving
 * it wrong when the request fails is its own kind of lie.
 */
export async function attempt<T>(
  fn: () => Promise<T>,
  doing: string,
): Promise<T | undefined> {
  try {
    return await fn();
  } catch (e) {
    reportError(e, doing);
    return undefined;
  }
}
