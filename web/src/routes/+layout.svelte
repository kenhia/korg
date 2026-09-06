<script lang="ts">
  import "../app.css";
  import { dev } from "$app/environment";
  import { goto } from "$app/navigation";
  import { page } from "$app/stores";
  import { api } from "$lib/api";
  import Toaster from "$lib/components/Toaster.svelte";

  let { children } = $props();

  // Ordered by how often Ken starts there, not by when each page was built
  // (sprint 029). `Plan` is the dependency graph — a thing you consult, not a
  // place you begin — so it sits at the end. `Link Up` left the bar entirely:
  // linking nodes is an agent request, not something he drives by hand. The
  // route still exists for anyone holding the URL.
  const nav = [
    { href: "/", label: "Today" },
    { href: "/cards", label: "Cards" },
    { href: "/work-items", label: "Work Items" },
    { href: "/planning", label: "Planning" },
    // Programs sit next to Planning because they are the layer directly above
    // it — a program bundles proposals the way a proposal bundles work items
    // (sprint 044).
    { href: "/programs", label: "Programs" },
    { href: "/daily-reports", label: "Reports" },
    // Schedules sit beside Reports because both are korg's time-derived state
    // (sprint 051): one is a date arriving, the other a date passing with no
    // write. Nothing on either page runs on its own.
    { href: "/schedules", label: "Schedules" },
    { href: "/reading-list", label: "Reading" },
    { href: "/plan", label: "Plan" },
  ];

  // Match on full path segments so /plan doesn't also light up on /planning
  // (WI #290) — active only on an exact match or a real subpath (/plan/…).
  function active(href: string, path: string): boolean {
    if (href === "/") return path === "/";
    return path === href || path.startsWith(href + "/");
  }

  // Kanban and Link Up need the full width; other detail/list pages stay
  // narrow — including Today, since sprint 050 slimmed it to lanes and trays.
  const wide = $derived(
    $page.url.pathname.startsWith("/cards") ||
      $page.url.pathname.startsWith("/link-up"),
  );

  // Work Items wants room for its table but not edge-to-edge — ~10% gutters
  // each side (80% width), keeping a little breathing space.
  const roomy = $derived($page.url.pathname.startsWith("/work-items"));

  // Search (#1177) is in the header rather than the nav on purpose: it is
  // something you do *from* a page, not a page you go to. A tab would make it a
  // destination and put the box one click further from everywhere that is not
  // it. The field keeps whatever /search is currently showing, so editing the
  // query from the results page works without a second input.
  let term = $state("");
  $effect(() => {
    term = $page.url.pathname === "/search"
      ? ($page.url.searchParams.get("q") ?? "")
      : "";
  });

  function submitSearch(e: SubmitEvent) {
    e.preventDefault();
    const q = term.trim();
    if (!q) return;
    // Carry the scope and kind already in play so refining a query does not
    // silently reset "search everything" back to the live-only default.
    const params = new URLSearchParams({ q });
    for (const key of ["scope", "kind"]) {
      const v = $page.url.pathname === "/search" ? $page.url.searchParams.get(key) : null;
      if (v) params.set(key, v);
    }
    goto(`/search?${params}`);
  }

  // Find by ID (#1809) sits beside Search for the reason Search sits in the
  // header at all: it is something you do *from* a page. It lived inside Work
  // Items, so reaching it meant navigating to Work Items first — and it is the
  // box Ken reaches for more often than full-text search, which made that the
  // wrong page to gate it behind.
  //
  // The move is also what let it become kind-agnostic. The page-local version
  // had two branches — jump to the row for a work item, open the slide-over
  // preview for anything else — because when #260 built it there was nowhere
  // else to go. Sprint 070 gave every kind a page and put the path on the node
  // itself (`NodePreview.url`, GP-13), so this resolves the id and goes where
  // korg says it lives. One branch, every kind, and no kind -> path table here.
  let findId = $state("");
  let findError = $state<string | null>(null);

  async function submitFind(e: SubmitEvent) {
    e.preventDefault();
    const id = parseInt(findId.trim(), 10);
    if (!Number.isFinite(id)) return;
    findError = null;
    let node;
    try {
      node = await api.node(id);
    } catch (err) {
      findError = err instanceof Error ? err.message : String(err);
      return;
    }
    // `url` is nullable so korg keeps a way to say it cannot answer, and the
    // vocabulary fence makes that unreachable for a real node. Handling it
    // anyway costs one branch and beats navigating to `/null`.
    if (!node) {
      findError = `No node with id ${id}.`;
      return;
    }
    if (!node.url) {
      findError = `korg has no page for node ${id} (${node.kind}).`;
      return;
    }
    findId = "";
    goto(node.url);
  }
</script>

<div class="min-h-screen">
  <header
    class="sticky top-0 z-40 border-b border-[var(--color-border)] bg-[var(--color-surface)]"
  >
    <!-- Wraps rather than scrolls (WI #549). It was `overflow-x-auto`, which at
         390px put the last item at x≈797 of a 390-wide viewport with no
         affordance whatsoever — the items were not hidden behind a control you
         could learn to use, they were simply gone. Ten short labels wrap to two
         rows on a phone and one row from `sm` up, which costs a little sticky
         header height in exchange for every destination being reachable. -->
    <nav
      class="mx-auto flex max-w-[120rem] flex-wrap items-center gap-1 px-4 py-2"
    >
      <!-- The dev loop and production look identical, and the dev server can be
           pointed at the production API (KORG_API), so "am I about to write to
           the real thing?" is a question the UI should answer rather than leave
           to whichever tab you clicked last. `dev` is true only under
           `vite dev`; the built bundle korg-api serves is always plain korg. -->
      <a
        href="/"
        class="mr-4 text-lg font-semibold tracking-tight"
        class:text-[var(--color-accent)]={!dev}
        class:text-red-500={dev}
        title={dev ? "Development server — not production" : undefined}
        >{dev ? "korg-dev" : "korg"}</a
      >
      {#each nav as item (item.href)}
        <a
          href={item.href}
          class="rounded px-3 py-1.5 text-sm transition-colors hover:bg-[var(--color-surface-hi)]"
          class:bg-[var(--color-surface-hi)]={active(
            item.href,
            $page.url.pathname,
          )}
          class:text-[var(--color-accent)]={active(
            item.href,
            $page.url.pathname,
          )}
          aria-current={active(item.href, $page.url.pathname)
            ? "page"
            : undefined}
        >
          {item.label}
        </a>
      {/each}

      <!-- The placeholder names korg deliberately. Page-level filter boxes are
           also called "search", and this one searches the whole corpus rather
           than the table in front of you — a distinction the Work Items filter
           and this box were briefly indistinguishable on. -->
      <div class="ml-auto flex items-center gap-2">
        <form class="flex items-center" onsubmit={submitSearch}>
          <label class="sr-only" for="korg-search">Search korg</label>
          <input
            id="korg-search"
            type="search"
            bind:value={term}
            placeholder="Search korg…"
            class="w-40 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-sm focus:w-64 focus:outline-none focus:ring-1 focus:ring-[var(--color-accent)] sm:w-56"
          />
        </form>

        <!-- Narrower than Search and it does not grow on focus: an id is a
             handful of digits, so the width it needs is fixed, and taking room
             from the search box beside it would be a cost with no use. -->
        <form class="flex items-center gap-1" onsubmit={submitFind}>
          <label class="sr-only" for="korg-find-by-id"
            >Find any node by its id</label
          >
          <input
            id="korg-find-by-id"
            bind:value={findId}
            placeholder="find by ID…"
            inputmode="numeric"
            title="Go to any node — work item, card, proposal, program, schedule, report, link or handoff — by its id"
            class="w-28 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-sm focus:outline-none focus:ring-1 focus:ring-[var(--color-accent)]"
            oninput={() => (findError = null)}
          />
          <button
            type="submit"
            class="rounded bg-[var(--color-accent-soft)] px-2 py-1 text-sm hover:bg-[var(--color-accent)]"
            >Go</button
          >
        </form>
      </div>
    </nav>

    <!-- The failure belongs in the header, under the box that produced it: the
         page below is whatever you were already looking at and has no reason to
         host an error about a nav control. Cleared on the next keystroke. -->
    {#if findError}
      <p
        role="alert"
        class="mx-auto max-w-[120rem] px-4 pb-2 text-sm text-red-400"
      >
        {findError}
      </p>
    {/if}
  </header>

  <main
    class="mx-auto w-full px-4 py-6"
    class:max-w-5xl={!wide && !roomy}
    class:max-w-[120rem]={wide}
    class:max-w-[80%]={roomy}
  >
    {@render children()}
  </main>

  <Toaster />
</div>
