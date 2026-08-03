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
  import { recordings } from "$domain/recordings.svelte";
  import { sessionEffect } from "$domain/session";
  import { toasts } from "$domain/toasts.svelte";
  import CapturesTool from "$features/captures/CapturesTool.svelte";
  import ClipboardTool from "$features/clipboard/ClipboardTool.svelte";
  import DictationTool from "$features/dictation/DictationTool.svelte";
  import MeetingsTool from "$features/meetings/MeetingsTool.svelte";
  import SearchModal from "$features/search/SearchModal.svelte";
  import SettingsPanel from "$features/settings/SettingsPanel.svelte";
  import SnippetsTool from "$features/snippets/SnippetsTool.svelte";
  import Modal from "$ui/Modal.svelte";
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

  let settingsOpen = $state(false);
  let searchOpen = $state(false);

  function onKeydown(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
      event.preventDefault();
      searchOpen = true;
      return;
    }

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

  {#snippet actions()}
    <IconButton label="Buscar (Ctrl+K)" size="sm" onclick={() => (searchOpen = true)}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
        <circle cx="11" cy="11" r="6" stroke="currentColor" stroke-width="1.8" />
        <path
          d="m20 20-3.5-3.5"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
        />
      </svg>
    </IconButton>

    <IconButton label="Ajustes" size="sm" onclick={() => (settingsOpen = true)}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
        <circle cx="12" cy="12" r="3" stroke="currentColor" stroke-width="1.8" />
        <path
          d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M18.4 5.6L17 7M7 17l-1.4 1.4"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
        />
      </svg>
    </IconButton>
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

{#if searchOpen}
  <SearchModal
    onClose={() => (searchOpen = false)}
    onNavigate={(hit) => {
      // Los resultados que son un sitio y no una acción: se abre la
      // herramienta que los contiene y se selecciona lo elegido.
      if (hit.kind === "recording") {
        recordings.select(hit.id);
        ui.openTool("meetings");
      } else if (hit.kind === "scratchpad") {
        ui.snippetsTab = "scratchpad";
        ui.openTool("snippets");
      }
    }}
  />
{/if}

{#if settingsOpen}
  <Modal title="Ajustes" size="lg" onClose={() => (settingsOpen = false)}>
    <!-- Sin padding propio: el panel maneja el suyo, y la navegación va pegada
         al borde. -->
    <div class="-mx-4 -my-3 h-[60vh]">
      <SettingsPanel />
    </div>
  </Modal>
{/if}

<ToastStack items={toasts.items} onDismiss={(id) => toasts.dismiss(id)} />
