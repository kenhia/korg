<script lang="ts">
  import "../app.css";
  import { page } from "$app/stores";
  import Toaster from "$lib/components/Toaster.svelte";

  let { children } = $props();

  const nav = [
    { href: "/", label: "Today" },
    { href: "/history", label: "History" },
    { href: "/topics", label: "Topics" },
    { href: "/plan", label: "Plan" },
    { href: "/cards", label: "Cards" },
    { href: "/work-items", label: "Work Items" },
    { href: "/planning", label: "Planning" },
    { href: "/daily-reports", label: "Reports" },
    { href: "/reading-list", label: "Reading" },
    { href: "/link-up", label: "Link Up" },
  ];

  // Match on full path segments so /plan doesn't also light up on /planning
  // (WI #290) — active only on an exact match or a real subpath (/plan/…).
  function active(href: string, path: string): boolean {
    if (href === "/") return path === "/";
    return path === href || path.startsWith(href + "/");
  }

  // Planner, kanban, and Link Up need the full width; other detail/list pages
  // stay narrow.
  const wide = $derived(
    $page.url.pathname === "/" ||
      $page.url.pathname.startsWith("/cards") ||
      $page.url.pathname.startsWith("/link-up"),
  );

  // Work Items wants room for its table but not edge-to-edge — ~10% gutters
  // each side (80% width), keeping a little breathing space.
  const roomy = $derived($page.url.pathname.startsWith("/work-items"));
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
      <a
        href="/"
        class="mr-4 text-lg font-semibold tracking-tight text-[var(--color-accent)]"
        >korg</a
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
