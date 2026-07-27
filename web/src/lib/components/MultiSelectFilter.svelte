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

  // A bare <details> only closes when you click its own summary again, so the
  // five filters on the Work Items toolbar could all sit open at once and each
  // needed dismissing by hand (WI #583). Tracking `open` in state lets an
  // outside pointerdown or Escape close it, which is what every other dropdown
  // does.
  //
  // Opening one filter closes the others for free: their summary is "outside"
  // this one, so the same handler fires.
  //
  // `open` is one-way (state -> DOM) and the summary's own click is
  // preventDefault'd, so this state is the only thing that ever opens or closes
  // the panel. `bind:open` was the obvious first cut and it desyncs: the
  // write-back rides the `toggle` event, and clicking filter B's summary while
  // filter A is open leaves B's DOM open with B's state still false — so B's
  // effect never registers and Escape does nothing. Caught by
  // filter-dismiss.spec.ts. Two sources of truth for one boolean, and the
  // native one wins silently.
  let open = $state(false);
  let root = $state<HTMLDetailsElement>();
  let summary = $state<HTMLElement>();

  $effect(() => {
    if (!open) return;

    // pointerdown, not click: dismissing on press matches native menus, and it
    // fires even when the press lands on something that swallows the click.
    // Capture phase so a stopPropagation() elsewhere cannot strand the panel.
    function onPointerDown(e: PointerEvent) {
      if (!root?.contains(e.target as Node)) open = false;
    }
    function onKeyDown(e: KeyboardEvent) {
      if (e.key !== "Escape") return;
      open = false;
      // Escape unwinds to where the user was, so put focus back on the summary
      // rather than dropping it on <body>.
      summary?.focus();
    }

    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onKeyDown);
    };
  });
</script>

<details class="relative" data-testid={`filter-${label}`} {open} bind:this={root}>
  <!-- Enter/Space on a focused <summary> dispatch a click, so preventing the
       default toggle here still leaves the control keyboard-operable. -->
  <summary
    bind:this={summary}
    onclick={(e) => {
      e.preventDefault();
      open = !open;
    }}
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
