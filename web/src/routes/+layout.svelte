<script lang="ts">
  import "../app.css";
  import { page } from "$app/stores";

  let { children } = $props();

  const nav = [
    { href: "/", label: "Today" },
    { href: "/cards", label: "Cards" },
    { href: "/work-items", label: "Work Items" },
    { href: "/reading-list", label: "Reading" },
  ];

  function active(href: string, path: string): boolean {
    return href === "/" ? path === "/" : path.startsWith(href);
  }
</script>

<div class="min-h-screen">
  <header class="border-b border-[var(--color-border)] bg-[var(--color-surface)]">
    <nav class="mx-auto flex max-w-5xl items-center gap-1 px-4 py-2">
      <a href="/" class="mr-4 text-lg font-semibold tracking-tight text-[var(--color-accent)]">korg</a>
      {#each nav as item (item.href)}
        <a
          href={item.href}
          class="rounded px-3 py-1.5 text-sm transition-colors hover:bg-[var(--color-surface-hi)]"
          class:bg-[var(--color-surface-hi)]={active(item.href, $page.url.pathname)}
          class:text-[var(--color-accent)]={active(item.href, $page.url.pathname)}
          aria-current={active(item.href, $page.url.pathname) ? "page" : undefined}
        >
          {item.label}
        </a>
      {/each}
    </nav>
  </header>

  <main class="mx-auto max-w-5xl px-4 py-6">
    {@render children()}
  </main>
</div>
