// Ctrl-V a screenshot into an editor and it lands inline (#1120).
//
// The flow, as the handoff settles it (D5): paste uploads **immediately** and
// never blocks typing. A placeholder goes in at the caret in the same tick, the
// upload runs in the background, and the placeholder is swapped for the real
// `![img-<hex>](…)` token when it resolves. Typing on top of a half-finished
// upload is therefore fine — the swap is a string replace of a unique
// placeholder, not an offset remembered from three keystrokes ago.
//
// Uploads are created **pending** — no owner — and claimed by `linkAll` when
// the thing being edited is saved. That is what makes Cancel free: an
// attachment nothing ever claims is swept 24 hours later by the server's
// sweeper, and korg never has to distinguish "abandoned" from "still typing".

import { api } from "./api";
import { imgToken } from "./img";
import { reportError } from "./toast.svelte";

/** Placeholders are per-tab unique, which is all the uniqueness a string
 *  replace inside one textarea needs. */
let seq = 0;

/** What goes in at the caret while the bytes are in flight. Deliberately looks
 *  like nothing else a person would type. */
function placeholder(n: number): string {
  return `![uploading image ${n}…]()`;
}

/**
 * The pending-attachment ledger for one editor.
 *
 * One per form, shared by every textarea in it (a work item's content and
 * details both feed the same list), because "what did this edit upload?" is a
 * property of the edit, not of the field.
 */
export class PasteUploads {
  /** Attachments uploaded by this editor that nothing owns yet. */
  pending = $state<string[]>([]);
  /** How many uploads are in flight, for a "still uploading…" affordance. */
  inFlight = $state(0);

  /** Paste-to-token round trips still running, so a save can wait for them. */
  #flight: Promise<unknown>[] = [];

  /** Register an in-flight paste. Tracks the *whole* round trip — upload and
   *  the placeholder swap that follows it — rather than the upload alone, so
   *  `settled()` cannot resolve between the two. */
  track<T>(p: Promise<T>): Promise<T> {
    this.#flight.push(p);
    return p;
  }

  /** Resolve once nothing is in flight. Save calls this first: an upload that
   *  lands after the save would leave a placeholder in the saved text and an
   *  attachment nothing claims.
   *
   *  Loops rather than awaiting once, because a paste that arrives while we are
   *  waiting is exactly the case this exists for. */
  async settled(): Promise<void> {
    while (this.#flight.length > 0) {
      const batch = this.#flight;
      this.#flight = [];
      await Promise.allSettled(batch);
    }
  }

  /** Upload one image, returning its `img-<hex>` id (null if it failed).
   *
   *  Reports its own failure: a paste is a fire-and-forget gesture with no
   *  return value to check, so a silent failure would leave a placeholder
   *  sitting in the text with no explanation. */
  async upload(file: File | Blob): Promise<string | null> {
    this.inFlight += 1;
    try {
      const row = await api.uploadImage(file);
      this.pending = [...this.pending, row.img_id];
      return row.img_id;
    } catch (e) {
      reportError(e, "Upload image");
      return null;
    } finally {
      this.inFlight -= 1;
    }
  }

  /**
   * The save step: claim every pending upload for the node that now owns it.
   *
   * Failures are reported but never rethrown — the save they follow has already
   * succeeded, and an unclaimed attachment is a recoverable state (the image is
   * still in the store for 24 hours, and re-saving re-links it) whereas an
   * error thrown here would read as "your edit did not save".
   */
  async linkAll(ownerNodeId: number): Promise<void> {
    const ids = this.pending;
    if (ids.length === 0) return;
    this.pending = [];
    const results = await Promise.allSettled(
      ids.map((id) => api.linkImage(id, ownerNodeId)),
    );
    for (const [i, r] of results.entries()) {
      if (r.status === "rejected") {
        // Put it back: the next save retries it, and the attachment panel
        // still shows it as pending in the meantime.
        this.pending = [...this.pending, ids[i]];
        reportError(r.reason, `Attach ${ids[i]}`);
      }
    }
  }

  /** Forget the ledger without deleting anything — the sweeper's job. Used when
   *  an editor is done with a set of uploads it has already linked. */
  clear(): void {
    this.pending = [];
  }
}

/** Every image file in a clipboard or drop payload. */
function imagesIn(dt: DataTransfer | null): File[] {
  if (!dt) return [];
  return [...dt.files].filter((f) => f.type.startsWith("image/"));
}

/** Splice text into a textarea at the caret, keeping `bind:value` in step.
 *
 *  Mutating `.value` and dispatching `input` is what tells Svelte's binding the
 *  field changed; setting the value alone would be overwritten the next time
 *  the bound state re-renders. */
function insertAtCaret(el: HTMLTextAreaElement, text: string): void {
  const start = el.selectionStart ?? el.value.length;
  const end = el.selectionEnd ?? start;
  el.value = `${el.value.slice(0, start)}${text}${el.value.slice(end)}`;
  const caret = start + text.length;
  el.setSelectionRange(caret, caret);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

/** Replace one exact substring, leaving everything else — including whatever
 *  was typed since — untouched. */
function replaceIn(el: HTMLTextAreaElement, needle: string, text: string): void {
  const at = el.value.indexOf(needle);
  // Gone means the user deleted the placeholder mid-upload, which is a decision
  // and not an error: the attachment stays pending and the sweeper takes it.
  if (at === -1) return;
  const caret = el.selectionStart ?? 0;
  el.value = el.value.slice(0, at) + text + el.value.slice(at + needle.length);
  // Keep the caret where the typing was, adjusting only if the edit happened
  // before it and changed the length.
  const shift = caret > at ? text.length - needle.length : 0;
  el.setSelectionRange(caret + shift, caret + shift);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

/**
 * Svelte action: paste (or drop) an image into this textarea to upload it.
 *
 * ```svelte
 * <textarea bind:value={content} use:pasteImages={uploads}></textarea>
 * ```
 *
 * A paste carrying no image is left entirely alone — pasting text into a
 * textarea must keep working exactly as the browser does it.
 */
export function pasteImages(el: HTMLTextAreaElement, uploads: PasteUploads) {
  async function ingest(files: File[]) {
    for (const file of files) {
      const mark = placeholder(++seq);
      insertAtCaret(el, mark);
      const id = await uploads.upload(file);
      // A failed upload takes its placeholder with it, so the text is never
      // left promising an image that does not exist.
      replaceIn(el, mark, id ? imgToken(id) : "");
    }
  }

  const onPaste = (e: ClipboardEvent) => {
    const files = imagesIn(e.clipboardData);
    if (files.length === 0) return;
    e.preventDefault();
    void uploads.track(ingest(files));
  };

  // Dropping an image file onto the editor is the same gesture as pasting one,
  // and the browser's default — navigate away to the file — is never what was
  // meant. It lands at the caret rather than under the pointer: mapping a drop
  // point to a text offset needs an API that is still spelled differently in
  // every engine, and the token is trivially moved once it is in the text.
  const onDrop = (e: DragEvent) => {
    const files = imagesIn(e.dataTransfer);
    if (files.length === 0) return;
    e.preventDefault();
    void uploads.track(ingest(files));
  };

  // `dragover` must be cancelled or the browser navigates to the dropped file.
  const onDragOver = (e: DragEvent) => {
    if (imagesIn(e.dataTransfer).length > 0 || e.dataTransfer?.types.includes("Files")) {
      e.preventDefault();
    }
  };

  el.addEventListener("paste", onPaste);
  el.addEventListener("drop", onDrop);
  el.addEventListener("dragover", onDragOver);
  return {
    destroy() {
      el.removeEventListener("paste", onPaste);
      el.removeEventListener("drop", onDrop);
      el.removeEventListener("dragover", onDragOver);
    },
  };
}
