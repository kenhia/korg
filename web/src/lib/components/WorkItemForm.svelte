<script lang="ts">
  import { untrack } from "svelte";
  import { api, type WorkItemRow } from "$lib/api";
  import { PasteUploads, pasteImages } from "$lib/imagePaste.svelte";
  import { WI_STATUSES, WI_TSHIRTS, WI_TYPES } from "$lib/generated/vocab";

  let {
    projectId,
    areas,
    editItem = null,
    onSaved,
    onCancel,
  }: {
    projectId: number | undefined;
    areas: { id: number; name: string }[];
    editItem?: WorkItemRow | null;
    onSaved: () => void;
    onCancel: () => void;
  } = $props();

  // Snapshot for one-time field init (the form remounts per item, so it never
  // needs to react to editItem changing in place). untrack signals that the
  // single read is intentional.
  const seed = untrack(() => editItem);
  const isEdit = seed !== null;

  let title = $state(seed?.title ?? "");
  let content = $state(seed?.content ?? "");
  let details = $state(seed?.details ?? "");
  let wiType = $state(seed?.wi_type ?? "task");
  let wiStatus = $state(seed?.wi_status ?? "open");
  let wiTshirt = $state(seed?.wi_tshirt ?? "S");
  let area = $state(seed?.area ?? "");
  let sprint = $state(seed?.sprint ?? "");
  let parent = $state(seed?.parent != null ? String(seed.parent) : "");
  let tags = $state(seed ? seed.tags.join(", ") : "");

  let saving = $state(false);
  let err = $state<string | null>(null);

  // Images pasted into either editor, uploaded as they were pasted and claimed
  // by the item on save (handoff D5). One ledger for both fields: the images
  // belong to the work item, not to the textarea they were dropped in.
  //
  // Uploading with no owner even when editing an *existing* item is deliberate.
  // The alternative — upload straight onto the item being edited — would leave
  // an attachment behind on Cancel, permanently, because nothing sweeps a
  // linked image. Pending-then-link makes Cancel cost nothing and gives the
  // same result on Save.
  const uploads = new PasteUploads();

  function tagList(): string[] {
    return tags
      .split(",")
      .map((t) => t.trim())
      .filter((t) => t !== "");
  }

  async function save() {
    if (title.trim() === "") {
      err = "Title is required";
      return;
    }
    if (content.trim() === "") {
      err = "Content is required";
      return;
    }
    saving = true;
    err = null;
    // An upload that lands after the save would leave `![uploading image 1…]()`
    // in the saved text. Waiting is the whole cost of not blocking typing
    // earlier, and it is paid at the one moment the user is already waiting.
    await uploads.settled();
    const areaId = area === "" ? null : (areas.find((a) => a.name === area)?.id ?? null);
    const parentNum = parent.trim() === "" ? null : parseInt(parent, 10);
    // Which node claims the pasted images: the item being edited, or the one
    // the create is about to return.
    let ownerNodeId = editItem?.node_id ?? null;
    try {
      if (editItem) {
        await api.updateWorkItem(editItem.wi_number, {
          title: title.trim(),
          content: content.trim(),
          details: details.trim() === "" ? null : details,
          wi_type: wiType,
          wi_status: wiStatus,
          wi_tshirt: wiTshirt,
          sprint: sprint.trim() === "" ? null : sprint.trim(),
          area_id: areaId,
          parent: parentNum,
          tags: tagList(),
        });
      } else {
        const r = await api.createWorkItem({
          title: title.trim(),
          content: content.trim(),
          wi_type: wiType,
          wi_status: wiStatus,
          wi_tshirt: wiTshirt,
          sprint: sprint.trim() || undefined,
          details: details.trim() || undefined,
          area_id: areaId ?? undefined,
          project_id: projectId,
        });
        if (parentNum) await api.updateWorkItem(r.wi_number, { parent: parentNum });
        ownerNodeId = r.node_id;
      }
      // After the item exists, and before the parent reloads it — so the
      // attachment list it renders already shows them as linked.
      if (ownerNodeId != null) await uploads.linkAll(ownerNodeId);
      onSaved();
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="flex h-full flex-col gap-2 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
  <div class="flex items-center justify-between">
    <span class="text-sm font-semibold">{isEdit ? `Edit #${editItem?.wi_number}` : "New work item"}</span>
    <div class="flex gap-2">
      <button class="rounded px-3 py-1 text-sm hover:bg-[var(--color-surface-hi)]" onclick={onCancel}>Cancel</button>
      <button class="rounded bg-[var(--color-accent-soft)] px-3 py-1 text-sm hover:bg-[var(--color-accent)] disabled:opacity-40" disabled={saving} onclick={save}>Save</button>
    </div>
  </div>

  {#if err}<p role="alert" class="rounded bg-red-950 px-2 py-1 text-xs text-red-300">{err}</p>{/if}

  <input class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none" placeholder="Title" bind:value={title} />

  <div class="flex flex-wrap gap-2 text-xs text-[var(--color-muted)]">
    <span class="flex items-center gap-1">Type
      <select class="rounded bg-[var(--color-surface-hi)] px-2 py-1 text-[var(--color-text)] outline-none" bind:value={wiType}>{#each WI_TYPES as t (t)}<option value={t}>{t}</option>{/each}</select>
    </span>
    <span class="flex items-center gap-1">Status
      <select class="rounded bg-[var(--color-surface-hi)] px-2 py-1 text-[var(--color-text)] outline-none" bind:value={wiStatus}>{#each WI_STATUSES as s (s)}<option value={s}>{s}</option>{/each}</select>
    </span>
    <span class="flex items-center gap-1">Size
      <select class="rounded bg-[var(--color-surface-hi)] px-2 py-1 text-[var(--color-text)] outline-none" bind:value={wiTshirt}>{#each WI_TSHIRTS as ts (ts)}<option value={ts}>{ts}</option>{/each}</select>
    </span>
    <span class="flex items-center gap-1">Area
      <select class="rounded bg-[var(--color-surface-hi)] px-2 py-1 text-[var(--color-text)] outline-none" bind:value={area}>
        <option value="">—</option>
        {#each areas as a (a.id)}<option value={a.name}>{a.name}</option>{/each}
      </select>
    </span>
    <span class="flex items-center gap-1">Sprint
      <input class="w-24 rounded bg-[var(--color-surface-hi)] px-2 py-1 text-[var(--color-text)] outline-none" bind:value={sprint} />
    </span>
    <span class="flex items-center gap-1">Parent&nbsp;#
      <input class="w-16 rounded bg-[var(--color-surface-hi)] px-2 py-1 text-[var(--color-text)] outline-none" inputmode="numeric" bind:value={parent} />
    </span>
  </div>

  <span class="block text-xs text-[var(--color-muted)]">
    Content (markdown)
    <!-- Said once, next to the field it applies to: paste is invisible until
         someone knows it is there. `inFlight` then reports the one thing that
         is genuinely in progress. -->
    {#if uploads.inFlight > 0}
      <span class="text-[var(--color-accent)]">· uploading {uploads.inFlight} image{uploads.inFlight === 1 ? "" : "s"}…</span>
    {:else}
      <span class="opacity-60">· Ctrl-V an image to attach it</span>
    {/if}
  </span>
  <textarea class="min-h-[12rem] flex-1 w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none" placeholder="Content (markdown)" data-testid="wi-content" bind:value={content} use:pasteImages={uploads}></textarea>

  <span class="block text-xs text-[var(--color-muted)]">Details (markdown)</span>
  <textarea
    class="min-h-[6rem] w-full rounded px-2 py-1.5 text-sm outline-none"
    style="background: color-mix(in oklch, var(--color-surface-hi) 80%, var(--color-accent) 20%)"
    placeholder="Details (markdown)"
    data-testid="wi-details"
    bind:value={details}
    use:pasteImages={uploads}
  ></textarea>

  <input class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 text-xs outline-none" placeholder="tags, comma, separated" bind:value={tags} />

  <div class="flex justify-end gap-2">
    <button class="rounded px-3 py-1.5 text-sm hover:bg-[var(--color-surface-hi)]" onclick={onCancel}>Cancel</button>
    <button class="rounded bg-[var(--color-accent-soft)] px-3 py-1.5 text-sm hover:bg-[var(--color-accent)] disabled:opacity-40" disabled={saving} onclick={save}>Save</button>
  </div>
</div>
