<script lang="ts">
  /**
   * La ventana principal.
   *
   * Reemplaza a un `routes/+page.svelte` de 1.135 líneas que tenía 25
   * variables de estado, 22 suscripciones en un solo `onMount` y toda la
   * lógica de negocio mezclada con el markup. Acá no hay nada de eso: el
   * estado vive en `domain/`, las suscripciones las monta `startSession`, y
   * esto solo decide qué se dibuja.
   */
  import { toolById, type ToolId } from "$core/tools";
  import { sessionEffect } from "$domain/session";
  import { toasts } from "$domain/toasts.svelte";
  import CapturesTool from "$features/captures/CapturesTool.svelte";
  import ClipboardTool from "$features/clipboard/ClipboardTool.svelte";
  import DictationTool from "$features/dictation/DictationTool.svelte";
  import MeetingsTool from "$features/meetings/MeetingsTool.svelte";
  import SnippetsTool from "$features/snippets/SnippetsTool.svelte";
  import { closeWindow, minimizeWindow, toggleMaximizeWindow } from "$ipc/windows";
  import WindowFrame from "$patterns/WindowFrame.svelte";
  import Button from "$ui/Button.svelte";
  import EmptyState from "$ui/EmptyState.svelte";
  import IconButton from "$ui/IconButton.svelte";
  import ToastStack from "$ui/ToastStack.svelte";
  import HubView from "./HubView.svelte";
  import { provideMainUi } from "./mainUi.svelte";

  const ui = provideMainUi();

  /** Lo que ya está reescrito. Falta agentes, que depende de la fase 7. */
  const READY: ToolId[] = [
    "meetings",
    "dictation",
    "clipboard",
    "snippets",
    "captures",
  ];

  // Una sola declaración de qué necesita esta ventana. Sin esto, cada vista
  // volvía a suscribirse por su cuenta y el estado quedaba duplicado.
  $effect(() =>
    sessionEffect([
      "config",
      "recordings",
      "models",
      "capture",
      "dictation",
      "clipboard",
      "snippets",
      "captures",
    ]),
  );

  const tool = $derived(toolById(ui.activeTool));

  function onKeydown(event: KeyboardEvent) {
    // Esc vuelve al hub, salvo que haya un diálogo abierto —el nativo lo cierra
    // él— o que el foco esté en un campo, donde Esc suele significar otra cosa.
    if (event.key !== "Escape" || ui.view !== "tool") return;
    if (document.querySelector("dialog[open]")) return;
    const el = document.activeElement;
    if (el instanceof HTMLElement && el.closest("input, textarea, [contenteditable]")) {
      return;
    }
    ui.backToHub();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<WindowFrame
  title={ui.view === "hub" ? "Atic" : tool.label}
  onMinimize={() => void minimizeWindow()}
  onMaximize={() => void toggleMaximizeWindow()}
  onClose={() => void closeWindow()}
>
  {#snippet start()}
    {#if ui.view === "tool"}
      <IconButton label="Volver al inicio" size="sm" onclick={() => ui.backToHub()}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <path
            d="M15 6l-6 6 6 6"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </IconButton>
    {/if}
  {/snippet}

  {#if ui.view === "hub"}
    <HubView ready={READY} onOpen={(id) => ui.openTool(id)} />
  {:else if ui.activeTool === "meetings"}
    <MeetingsTool />
  {:else if ui.activeTool === "dictation"}
    <DictationTool />
  {:else if ui.activeTool === "clipboard"}
    <ClipboardTool />
  {:else if ui.activeTool === "snippets"}
    <SnippetsTool initialTab={ui.snippetsTab} />
  {:else if ui.activeTool === "captures"}
    <CapturesTool />
  {:else}
    <EmptyState title="{tool.label} todavía no está reescrita" hint={tool.blurb}>
      {#snippet action()}
        <Button variant="soft" size="sm" onclick={() => ui.backToHub()}>
          Volver al inicio
        </Button>
      {/snippet}
    </EmptyState>
  {/if}
</WindowFrame>

<ToastStack items={toasts.items} onDismiss={(id) => toasts.dismiss(id)} />
