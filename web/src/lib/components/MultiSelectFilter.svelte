<script lang="ts">
  let {
    label,
    options,
    selected,
    onchange,
    modified = false,
  }: {
    label: string;
    options: string[];
    selected: Set<string>;
    onchange: (s: Set<string>) => void;
    modified?: boolean;
  } = $props();

  function toggle(opt: string) {
    const s = new Set(selected);
    if (s.has(opt)) s.delete(opt);
    else s.add(opt);
    onchange(s);
  }
</script>

<details class="relative">
  <summary
    class="flex cursor-pointer list-none items-center gap-1 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-xs"
    class:text-[var(--color-accent)]={modified}
  >
    <span>{label}</span>
    <span class="text-[var(--color-muted)]">{selected.size}/{options.length}</span>
  </summary>
  <div
    class="absolute left-0 z-30 mt-1 max-h-64 w-44 overflow-auto rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-2 text-xs shadow-xl"
  >
    <div class="mb-1 flex gap-2 border-b border-[var(--color-border)] pb-1">
      <button class="hover:text-[var(--color-accent)]" onclick={() => onchange(new Set(options))}>All</button>
      <button class="hover:text-[var(--color-accent)]" onclick={() => onchange(new Set())}>None</button>
    </div>
    {#each options as opt (opt)}
      <label class="flex items-center gap-2 py-0.5">
        <input type="checkbox" checked={selected.has(opt)} onchange={() => toggle(opt)} />
        <span>{opt}</span>
      </label>
    {:else}
      <p class="text-[var(--color-muted)]">none</p>
    {/each}
  </div>
</details>
