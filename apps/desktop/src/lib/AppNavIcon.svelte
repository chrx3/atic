<script lang="ts">
  import ToolIcon from "$lib/ToolIcon.svelte";
  import type { AppSectionId } from "$lib/tools";

  let { id, size = 20 }: { id: AppSectionId; size?: number } = $props();
</script>

{#if id === "meetings" || id === "captures" || id === "agents"}
  <ToolIcon id={id} {size} />
{:else}
  <svg
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.35"
    stroke-linecap="butt"
    stroke-linejoin="miter"
    aria-hidden="true"
  >
    {#if id === "home"}
      <path d="M4 10.5 12 4l8 6.5V20h-5v-6H9v6H4Z" />
    {:else if id === "texts"}
      <path d="M5 6h14M5 11h10M5 16h13" />
      <path d="M5 20h7" />
    {:else}
      <circle cx="12" cy="12" r="3" />
      <path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1" />
    {/if}
  </svg>
{/if}
