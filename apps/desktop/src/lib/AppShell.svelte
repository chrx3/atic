<script lang="ts">
  import type { Snippet } from "svelte";
  import { fade } from "svelte/transition";
  import Titlebar from "$lib/Titlebar.svelte";
  import ParticleWheel from "$lib/ParticleWheel.svelte";
  import { toolById, type ToolId } from "$lib/tools";
  import { formatShortcut } from "$lib/format";
  import type { UiTheme } from "$lib/theme";
  import Icon from "$ui/Icon.svelte";
  import { ArrowLeft } from "$lib/icons";

  let {
    activeTool = $bindable("meetings"),
    view = $bindable<"hub" | "tool">("hub"),
    /** Atajo de la rueda, para enseñarlo en el pie del hub. */
    radialShortcut = "",
    theme = "system",
    onToggleTheme,
    onOpenSettings,
    children,
  }: {
    activeTool?: ToolId;
    view?: "hub" | "tool";
    radialShortcut?: string;
    theme?: UiTheme;
    onToggleTheme?: () => void;
    onOpenSettings?: () => void;
    children: Snippet;
  } = $props();

  let hubActive = $state<ToolId | null>(null);

  const tool = $derived(toolById(activeTool));
  const titleTool = $derived(
    view === "hub" ? { ...tool, label: "Inicio" } : tool,
  );

  const reduceMotion =
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const hubFade = reduceMotion ? 0 : 180;

  function openTool(id: ToolId) {
    activeTool = id;
    view = "tool";
    hubActive = null;
  }

  // Esc vuelve al inicio: es lo primero que intenta cualquiera. No se aplica
  // si hay un modal abierto (el <dialog> nativo consume su propio Escape) ni
  // mientras se escribe en un campo.
  $effect(() => {
    if (view !== "tool") return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (document.querySelector("dialog[open]")) return;
      const el = document.activeElement as HTMLElement | null;
      if (
        el &&
        (el.tagName === "INPUT" ||
          el.tagName === "TEXTAREA" ||
          el.isContentEditable)
      ) {
        return;
      }
      view = "hub";
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
</script>

<div class="atic-shell">
  <Titlebar tool={titleTool} {theme} {onToggleTheme} {onOpenSettings} />

  <div class="atic-shell-body">
    <!-- El contenido vive siempre montado: los modales y toasts cuelgan de él
         y deben poder abrirse también con el hub delante (el <dialog> nativo
         va al top-layer, por encima de cualquier overlay). -->
    <div class="atic-shell-main">
      {#if view === "tool"}
        <button
          type="button"
          class="atic-back"
          onclick={() => (view = "hub")}
          title="Volver al inicio (Esc)"
        >
          <Icon icon={ArrowLeft} size={11} />
          Inicio
        </button>
      {/if}
      {@render children()}
    </div>

    {#if view === "hub"}
      <div class="atic-hub" transition:fade={{ duration: hubFade }}>
        <ParticleWheel
          bind:activeId={hubActive}
          hint={radialShortcut
            ? `Mantén ${formatShortcut(radialShortcut)} para esta rueda sobre cualquier app`
            : ""}
          onSelect={openTool}
        />
      </div>
    {/if}
  </div>
</div>

<style>
  .atic-shell {
    container-type: inline-size;
    container-name: atic-shell;
    display: flex;
    height: 100dvh;
    flex-direction: column;
    overflow: hidden;
    color: var(--rb-text);
    background: var(--rb-bg0);
  }

  .atic-shell-body {
    position: relative;
    display: flex;
    min-height: 0;
    flex: 1 1 auto;
  }

  .atic-shell-main {
    container-type: inline-size;
    container-name: atic-main;
    display: flex;
    min-width: 0;
    min-height: 0;
    flex: 1 1 auto;
    flex-direction: column;
    overflow: auto;
    overscroll-behavior: contain;
  }

  /* Miga de vuelta al inicio, visible en toda herramienta. */
  .atic-back {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    align-self: flex-start;
    flex: 0 0 auto;
    margin: 0.65rem 0 -0.25rem 1rem;
    padding: 0.25rem 0.5rem;
    border: 0;
    border-radius: 999px;
    background: transparent;
    color: var(--rb-faint);
    font-size: 0.625rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    cursor: pointer;
    transition: color 0.14s ease;
  }

  .atic-back:hover {
    color: var(--rb-text);
  }

  .atic-back:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .atic-hub {
    position: absolute;
    inset: 0;
    z-index: 5;
    background: var(--rb-bg0);
  }
</style>
