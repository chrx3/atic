<script lang="ts">
  /**
   * Icono de un resultado del launcher: bitmap real de la app (Windows) o
   * Lucide para acciones / fallback.
   */
  import Icon from "$ui/Icon.svelte";
  import { LAUNCHER_ICONS, Search } from "$lib/icons";
  import { loadLauncherAppIcon } from "./launcherIconCache";

  let {
    id,
    kind,
    size = 16,
  }: {
    id: string;
    kind: string;
    size?: number;
  } = $props();

  const isApp = $derived(kind === "app" || id.startsWith("app:"));

  const which = $derived(
    id.startsWith("action:")
      ? id.slice("action:".length)
      : isApp
        ? "app"
        : "search",
  );

  const fallback = $derived(LAUNCHER_ICONS[which] ?? Search);
  const appIcon = $derived(isApp && id ? loadLauncherAppIcon(id) : null);
</script>

{#snippet lucide()}
  <Icon icon={fallback} {size} strokeWidth={1.7} />
{/snippet}

{#if appIcon}
  {#await appIcon}
    {@render lucide()}
  {:then url}
    {#if url}
      <img
        class="li-img"
        src={url}
        width={size}
        height={size}
        alt=""
        draggable="false"
      />
    {:else}
      {@render lucide()}
    {/if}
  {:catch}
    {@render lucide()}
  {/await}
{:else}
  {@render lucide()}
{/if}

<style>
  .li-img {
    display: block;
    object-fit: contain;
    border-radius: 0.2rem;
    flex-shrink: 0;
  }
</style>
