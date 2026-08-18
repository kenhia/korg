<script lang="ts">
  import "../app.css";
  import { dev } from "$app/environment";
  import { goto } from "$app/navigation";
  import { page } from "$app/stores";
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
      <form class="ml-auto flex items-center" onsubmit={submitSearch}>
        <label class="sr-only" for="korg-search">Search korg</label>
        <input
          id="korg-search"
          type="search"
          bind:value={term}
          placeholder="Search korg…"
          class="w-40 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-sm focus:w-64 focus:outline-none focus:ring-1 focus:ring-[var(--color-accent)] sm:w-56"
        />
      </form>
    </nav>
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
