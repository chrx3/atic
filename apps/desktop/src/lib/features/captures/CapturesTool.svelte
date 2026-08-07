<script lang="ts">
  /** Capturas recientes: mirarlas, copiarlas, leerles el texto. */
  import { formatListWhen, formatShortcut } from "$core/format";
  import { captures } from "$domain/captures.svelte";
  import { config } from "$domain/config.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import { captureSrc } from "$ipc/captures";
  import ToolPage from "$patterns/ToolPage.svelte";
  import Toolbar from "$patterns/Toolbar.svelte";
  import Button from "$ui/Button.svelte";
  import Chip from "$ui/Chip.svelte";
  import EmptyState from "$ui/EmptyState.svelte";
  import Icon from "$ui/Icon.svelte";
  import IconButton from "$ui/IconButton.svelte";
  import Kbd from "$ui/Kbd.svelte";
  import Modal from "$ui/Modal.svelte";
  import { Copy, ScanText, Trash2 } from "$lib/icons";
  import type { CaptureItem } from "$core/types";

  const shortcut = $derived(config.current?.screenshot_shortcut ?? "");

  let preview = $state<CaptureItem | null>(null);

  async function run(action: () => Promise<unknown>, done?: string) {
    try {
      const result = await action();
      if (done) toasts.push(done);
      return result;
    } catch (error) {
      toastError(error);
    }
  }

  async function ocr(path: string) {
    const text = await run(() => captures.ocr(path));
    if (typeof text === "string") {
      toasts.push(text.trim() ? "Texto copiado" : "No se encontró texto");
    }
  }
</script>

<ToolPage
  title="Capturas"
  icon="captures"
  kicker="Pantalla"
  blurb="Recortes rápidos al portapapeles y al shelf flotante."
>
  {#snippet meta()}
    <Chip>{captures.items.length} recientes</Chip>
    {#if shortcut}
      <Kbd combo={formatShortcut(shortcut)} separator="+" />
    {/if}
  {/snippet}

  <div class="flex h-full min-h-0 flex-col">
    <Toolbar label="Acciones de capturas">
      {#snippet end()}
        <Button
          variant="soft"
          size="sm"
          onclick={() =>
            void run(async () => {
              const removed = await captures.cleanup();
              toasts.push(
                removed > 0 ? `Se borraron ${removed}` : "No había nada que borrar",
              );
            })}
        >
          Limpiar vencidas
        </Button>
      {/snippet}
      <span class="text-xs text-muted">
        Se borran solas según la retención de Ajustes.
      </span>
    </Toolbar>

    <div class="min-h-0 flex-1 overflow-y-auto p-3">
      {#if captures.items.length === 0}
        <EmptyState
          compact
          icon="captures"
          title="No hay capturas recientes"
          hint={shortcut
            ? `Sacá una con ${formatShortcut(shortcut)}.`
            : "Configurá el atajo en Ajustes."}
        />
      {:else}
        <div
          class="@container/grid grid grid-cols-3 gap-2 @md/grid:grid-cols-4
                 @lg/grid:grid-cols-5"
        >
          {#each captures.items as item (item.id)}
            <figure
              class="group flex flex-col overflow-hidden rounded-sm border border-line
                     bg-surface"
            >
              <button
                type="button"
                class="block aspect-video w-full overflow-hidden bg-surface-2"
                title="Ampliar"
                onclick={() => (preview = item)}
              >
                <!-- `object-contain`: se ve la captura entera, no un recorte zoom. -->
                <img
                  src={captureSrc(item.path)}
                  alt="Captura de {item.label}"
                  loading="lazy"
                  class="size-full object-contain"
                />
              </button>

              <figcaption
                class="flex items-center gap-1 border-t border-line px-2 py-1"
              >
                <span
                  class="min-w-0 flex-1 truncate font-mono text-xs text-faint"
                  data-numeric
                >
                  {formatListWhen(Math.floor(item.createdAtMs / 1000))}
                </span>

                <div
                  class="flex shrink-0 items-center gap-0.5 opacity-0
                         transition-opacity duration-(--duration-quick)
                         group-hover:opacity-100 focus-within:opacity-100"
                >
                  <IconButton
                    label="Copiar imagen"
                    size="sm"
                    onclick={() => void run(() => captures.copy(item.path), "Copiada")}
                  >
                    <Icon icon={Copy} size={12} />
                  </IconButton>
                  <IconButton
                    label="Copiar el texto (OCR)"
                    size="sm"
                    onclick={() => void ocr(item.path)}
                  >
                    <Icon icon={ScanText} size={12} />
                  </IconButton>
                  <IconButton
                    label="Borrar"
                    size="sm"
                    variant="danger"
                    onclick={() => void run(() => captures.remove(item.path))}
                  >
                    <Icon icon={Trash2} size={12} />
                  </IconButton>
                </div>
              </figcaption>
            </figure>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</ToolPage>

{#if preview}
  {@const item = preview}
  <Modal
    title="Captura"
    subtitle={formatListWhen(Math.floor(item.createdAtMs / 1000))}
    size="lg"
    panelMax="min(90dvh, 880px)"
    onClose={() => (preview = null)}
  >
    {#snippet actions()}
      <Button
        variant="soft"
        size="sm"
        onclick={() => void run(() => captures.copy(item.path), "Copiada")}
      >
        Copiar
      </Button>
      <Button
        variant="soft"
        size="sm"
        onclick={() => void run(() => captures.open(item.path))}
      >
        Abrir
      </Button>
      <Button variant="primary" size="sm" onclick={() => (preview = null)}>
        Cerrar
      </Button>
    {/snippet}

    <div
      class="flex max-h-[min(70dvh,720px)] items-center justify-center
             overflow-auto rounded-sm bg-surface-2 p-2"
    >
      <img
        src={captureSrc(item.path)}
        alt="Captura de {item.label}"
        class="max-h-[min(66dvh,680px)] max-w-full object-contain"
      />
    </div>
  </Modal>
{/if}
