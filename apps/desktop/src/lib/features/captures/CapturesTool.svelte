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
  import IconButton from "$ui/IconButton.svelte";
  import Kbd from "$ui/Kbd.svelte";

  const shortcut = $derived(config.current?.screenshot_shortcut ?? "");

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
  kicker="Pantalla"
  blurb="Recortes rápidos al portapapeles y al shelf flotante."
>
  {#snippet meta()}
    <Chip>{captures.items.length} recientes</Chip>
    {#if shortcut}
      <Kbd combo={formatShortcut(shortcut)} separator="+" />
    {/if}
  {/snippet}

  <div class="flex h-full flex-col">
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

    <div class="min-h-0 flex-1 overflow-y-auto p-4">
      {#if captures.items.length === 0}
        <EmptyState
          title="No hay capturas recientes"
          hint={shortcut
            ? `Sacá una con ${formatShortcut(shortcut)}.`
            : "Configurá el atajo en Ajustes."}
        />
      {:else}
        <div
          class="@container/grid grid grid-cols-2 gap-2 @md/grid:grid-cols-3
                 @lg/grid:grid-cols-4"
        >
          {#each captures.items as item (item.id)}
            <figure
              class="group flex flex-col overflow-hidden rounded-sm border border-line
                     bg-surface"
            >
              <button
                type="button"
                class="block aspect-video w-full overflow-hidden bg-surface-2"
                title="Abrir"
                onclick={() => void run(() => captures.open(item.path))}
              >
                <!-- `loading="lazy"`: el shelf guarda cientos y pintarlas todas
                     al entrar congela la ventana. -->
                <img
                  src={captureSrc(item.path)}
                  alt="Captura de {item.label}"
                  loading="lazy"
                  class="size-full object-cover"
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
                    <svg
                      width="12"
                      height="12"
                      viewBox="0 0 24 24"
                      fill="none"
                      aria-hidden="true"
                    >
                      <rect
                        x="8"
                        y="8"
                        width="12"
                        height="12"
                        rx="2"
                        stroke="currentColor"
                        stroke-width="1.8"
                      />
                      <path
                        d="M4 16V6a2 2 0 012-2h10"
                        stroke="currentColor"
                        stroke-width="1.8"
                      />
                    </svg>
                  </IconButton>
                  <IconButton
                    label="Copiar el texto (OCR)"
                    size="sm"
                    onclick={() => void ocr(item.path)}
                  >
                    <svg
                      width="12"
                      height="12"
                      viewBox="0 0 24 24"
                      fill="none"
                      aria-hidden="true"
                    >
                      <path
                        d="M5 7V5h14v2M12 5v14M9 19h6"
                        stroke="currentColor"
                        stroke-width="1.8"
                        stroke-linecap="round"
                      />
                    </svg>
                  </IconButton>
                  <IconButton
                    label="Borrar"
                    size="sm"
                    variant="danger"
                    onclick={() => void run(() => captures.remove(item.path))}
                  >
                    <svg
                      width="12"
                      height="12"
                      viewBox="0 0 24 24"
                      fill="none"
                      aria-hidden="true"
                    >
                      <path
                        d="M6 6l12 12M18 6L6 18"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                      />
                    </svg>
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
