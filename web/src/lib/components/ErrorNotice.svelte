<script lang="ts">
  // "This failed to load", as distinct from "there is nothing here" (WI #547).
  //
  // The failure this fixes, in its purest form, was link-up: four collections
  // each loaded with `.catch(() => [])`, so an API outage rendered a page that
  // confidently said there was nothing to link. The page did not merely lose
  // data — it asserted something false, and looked completely normal doing it.
  //
  // Empty and failed must never render the same. This component is the "failed"
  // half, and it always offers the retry, because the most common cause is
  // transient.
  import { ApiError, NetworkError } from "$lib/api";

  let {
    error,
    what,
    retry,
  }: { error: unknown; what: string; retry?: () => void } = $props();

  const detail = $derived(
    error instanceof NetworkError
      ? error.message
      : error instanceof ApiError
        ? error.code === "internal"
          ? "korg hit an internal error."
          : error.detail
        : error instanceof Error
          ? error.message
          : String(error),
  );
</script>

<div
  role="alert"
  class="rounded border border-red-800 bg-red-950/60 px-3 py-2 text-sm text-red-200"
  data-testid="error-notice"
>
  <p class="font-medium">Couldn't load {what}.</p>
  <p class="mt-0.5 text-red-300/90">{detail}</p>
  {#if retry}
    <button
      class="mt-2 rounded border border-red-700 px-2 py-0.5 text-xs text-red-200 hover:bg-red-900"
      onclick={retry}>Retry</button
    >
  {/if}
</div>
