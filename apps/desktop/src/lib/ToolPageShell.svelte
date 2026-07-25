<script lang="ts">
  import type { Snippet } from "svelte";
  import type { ToolDef } from "$lib/tools";
  import { openDataDir, type DataDirKind } from "$lib/api";

  let {
    tool,
    kicker = "",
    dataDir = undefined as DataDirKind | undefined,
    meta,
    actions,
    prefs,
    children,
  }: {
    tool: ToolDef;
    kicker?: string;
    dataDir?: DataDirKind;
    meta?: Snippet;
    actions?: Snippet;
    prefs?: Snippet;
    children?: Snippet;
  } = $props();

  let opening = $state(false);

  async function openFolder() {
    if (!dataDir || opening) return;
    opening = true;
    try {
      await openDataDir(dataDir);
    } catch (error) {
      console.warn("openDataDir", error);
    } finally {
      opening = false;
    }
  }
</script>

<section class="atic-tool-panel" aria-label={tool.label}>
  <!-- Título y acciones comparten fila: las acciones ganan contexto y se
       recupera una franja entera de alto para el contenido real. -->
  <header class="atic-tool-head">
    <div class="atic-tool-titles">
      {#if kicker}
        <p class="atic-tool-kicker">{kicker}</p>
      {/if}
      <div class="atic-tool-title-row">
        <h2>{tool.label}</h2>
        {#if meta}{@render meta()}{/if}
      </div>
      <p class="atic-tool-blurb">{tool.blurb}</p>
    </div>

    {#if actions || dataDir}
      <div class="atic-tool-actions">
        {#if actions}{@render actions()}{/if}
        {#if dataDir}
          <button
            type="button"
            class="rb-btn rb-btn-ghost atic-tool-folder"
            disabled={opening}
            onclick={() => void openFolder()}
          >
            {opening ? "Abriendo…" : "Abrir carpeta"}
          </button>
        {/if}
      </div>
    {/if}
  </header>

  <div class="atic-tool-body">
    {#if children}
      {@render children()}
    {/if}
  </div>

  <!-- Preferencias al final: son configuración, no contenido. -->
  {#if prefs}
    <div class="atic-tool-prefs">
      {@render prefs()}
    </div>
  {/if}
</section>

<style>
  .atic-tool-panel {
    container-type: inline-size;
    container-name: atic-tool;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    gap: 0.75rem;
    padding: 0;
  }

  .atic-tool-head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem 1rem;
  }

  .atic-tool-titles {
    min-width: 0;
  }

  .atic-tool-head h2 {
    margin: 0;
    font-family: var(--rb-display);
    font-size: 1.0625rem;
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .atic-tool-kicker {
    margin: 0 0 0.15rem;
    color: var(--rb-muted);
    font-size: 0.6875rem;
    font-weight: 650;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .atic-tool-blurb {
    margin: 0.2rem 0 0;
    max-width: 36rem;
    color: var(--rb-faint);
    font-size: 0.75rem;
    line-height: 1.45;
  }

  .atic-tool-title-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
  }

  .atic-tool-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    align-items: center;
  }

  .atic-tool-folder {
    margin-inline-start: auto;
  }

  /* Configuración: presente pero claramente secundaria. */
  .atic-tool-prefs {
    display: flex;
    flex-direction: column;
    flex: 0 0 auto;
    gap: 0.55rem;
    margin-top: 0.25rem;
    opacity: 0.72;
    transition: opacity 0.16s ease;
  }

  .atic-tool-prefs:hover,
  .atic-tool-prefs:focus-within {
    opacity: 1;
  }

  .atic-tool-body {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 0.65rem;
  }

  @container atic-main (max-width: 36.999rem) {
    .atic-tool-panel {
      gap: 0.65rem;
    }

    .atic-tool-blurb {
      font-size: 0.8125rem;
    }

    .atic-tool-folder {
      margin-inline-start: 0;
    }

    .atic-tool-actions > :global(.rb-btn) {
      flex: 1 1 8rem;
    }
  }
</style>
