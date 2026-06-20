<script lang="ts">
  import { api, WI_TYPES, WI_STATUSES, TSHIRTS, type WorkItem } from "$lib/api";

  let {
    projectId,
    areas,
    editItem = null,
    onSaved,
    onCancel,
  }: {
    projectId: number | undefined;
    areas: { id: number; name: string }[];
    editItem?: WorkItem | null;
    onSaved: () => void;
    onCancel: () => void;
  } = $props();

  const seed = editItem; // snapshot for one-time field init (form remounts per item)
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
    const areaId = area === "" ? null : (areas.find((a) => a.name === area)?.id ?? null);
    const parentNum = parent.trim() === "" ? null : parseInt(parent, 10);
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
      }
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

  {#if err}<p class="rounded bg-red-950 px-2 py-1 text-xs text-red-300">{err}</p>{/if}

  <input class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none" placeholder="Title" bind:value={title} />

  <div class="flex flex-wrap gap-2 text-xs text-[var(--color-muted)]">
    <span class="flex items-center gap-1">Type
      <select class="rounded bg-[var(--color-surface-hi)] px-2 py-1 text-[var(--color-text)] outline-none" bind:value={wiType}>{#each WI_TYPES as t (t)}<option value={t}>{t}</option>{/each}</select>
    </span>
    <span class="flex items-center gap-1">Status
      <select class="rounded bg-[var(--color-surface-hi)] px-2 py-1 text-[var(--color-text)] outline-none" bind:value={wiStatus}>{#each WI_STATUSES as s (s)}<option value={s}>{s}</option>{/each}</select>
    </span>
    <span class="flex items-center gap-1">Size
      <select class="rounded bg-[var(--color-surface-hi)] px-2 py-1 text-[var(--color-text)] outline-none" bind:value={wiTshirt}>{#each TSHIRTS as ts (ts)}<option value={ts}>{ts}</option>{/each}</select>
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

  <span class="block text-xs text-[var(--color-muted)]">Content (markdown)</span>
  <textarea class="min-h-[12rem] flex-1 w-full rounded bg-[var(--color-surface-hi)] px-2 py-1.5 text-sm outline-none" placeholder="Content (markdown)" bind:value={content}></textarea>

  <span class="block text-xs text-[var(--color-muted)]">Details (markdown)</span>
  <textarea
    class="min-h-[6rem] w-full rounded px-2 py-1.5 text-sm outline-none"
    style="background: color-mix(in oklch, var(--color-surface-hi) 80%, var(--color-accent) 20%)"
    placeholder="Details (markdown)"
    bind:value={details}
  ></textarea>

  <input class="w-full rounded bg-[var(--color-surface-hi)] px-2 py-1 text-xs outline-none" placeholder="tags, comma, separated" bind:value={tags} />

  <div class="flex justify-end gap-2">
    <button class="rounded px-3 py-1.5 text-sm hover:bg-[var(--color-surface-hi)]" onclick={onCancel}>Cancel</button>
    <button class="rounded bg-[var(--color-accent-soft)] px-3 py-1.5 text-sm hover:bg-[var(--color-accent)] disabled:opacity-40" disabled={saving} onclick={save}>Save</button>
  </div>
</div>
