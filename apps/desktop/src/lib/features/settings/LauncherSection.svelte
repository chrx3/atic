<script lang="ts">
  /**
   * Favoritos del launcher (Ajustes → Launcher).
   * El alta principal es la estrella en el float; acá se listan y se quitan.
   */
  import { onMount } from "svelte";
  import { config } from "$domain/config.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import {
    launcherListFavorites,
    launcherToggleFavorite,
  } from "$ipc/search";
  import type { LauncherHit } from "$core/types";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Button from "$ui/Button.svelte";
  import LauncherIcon from "$surfaces/launcher/LauncherIcon.svelte";

  const cfg = $derived(config.current);
  let favorites = $state<LauncherHit[]>([]);
  let loading = $state(true);

  async function refresh() {
    loading = true;
    try {
      favorites = await launcherListFavorites();
      if (cfg && favorites.map((f) => f.id).join("\0") !== cfg.launcher_favorites.join("\0")) {
        // Alinea el store si hay ids huérfanos filtrados por Rust.
        await config.patch({
          launcher_favorites: favorites.map((f) => f.id),
        });
      }
    } catch (e) {
      toastError(e);
    } finally {
      loading = false;
    }
  }

  async function remove(id: string) {
    try {
      const next = await launcherToggleFavorite(id);
      await config.patch({ launcher_favorites: next });
      favorites = favorites.filter((f) => f.id !== id);
    } catch (e) {
      toastError(e);
    }
  }

  onMount(() => {
    void refresh();
  });
</script>

{#if cfg}
  <div class="flex flex-col gap-5">
    <SettingsGroup
      title="Favoritos"
      hint="Aparecen como pelotitas al final de la barra del launcher. Máximo 8."
    >
      {#if loading}
        <SettingsRow label="Cargando…">
          {#snippet control()}
            <span></span>
          {/snippet}
        </SettingsRow>
      {:else if favorites.length === 0}
        <SettingsRow
          label="Sin favoritos"
          hint="Abrí el launcher (Ctrl+Space), buscá una app y tocá la estrella."
        >
          {#snippet control()}
            <span></span>
          {/snippet}
        </SettingsRow>
      {:else}
        {#each favorites as fav (fav.id)}
          <SettingsRow label={fav.title} hint={fav.subtitle}>
            {#snippet control()}
              <div class="fav-row">
                <span class="fav-ico" aria-hidden="true">
                  <LauncherIcon id={fav.id} kind={fav.kind} size={16} />
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  onclick={() => void remove(fav.id)}
                >
                  Quitar
                </Button>
              </div>
            {/snippet}
          </SettingsRow>
        {/each}
      {/if}
    </SettingsGroup>
  </div>
{/if}

<style>
  .fav-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .fav-ico {
    display: grid;
    place-items: center;
    width: 1.75rem;
    height: 1.75rem;
    border-radius: 999px;
    background: color-mix(in srgb, var(--text, currentColor) 8%, transparent);
    color: var(--muted, inherit);
  }
</style>
