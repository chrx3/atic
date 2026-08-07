<script lang="ts">
  /** Historial del portapapeles: buscar, fijar, pegar. */
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { formatListWhen } from "$core/format";
  import { clipboard } from "$domain/clipboard.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import ToolPage from "$patterns/ToolPage.svelte";
  import Toolbar from "$patterns/Toolbar.svelte";
  import Chip from "$ui/Chip.svelte";
  import EmptyState from "$ui/EmptyState.svelte";
  import Icon from "$ui/Icon.svelte";
  import IconButton from "$ui/IconButton.svelte";
  import Input from "$ui/Input.svelte";
  import { Pin, Trash2 } from "$lib/icons";

  async function run(action: () => Promise<void>, done?: string) {
    try {
      await action();
      if (done) toasts.push(done);
    } catch (error) {
      toastError(error);
    }
  }
</script>

<ToolPage
  title="Clipboard"
  icon="clipboard"
  kicker="Historial"
  blurb="Todo lo que copiaste, guardado local. El atajo lo pega desde la pill."
>
  {#snippet meta()}
    <Chip>{clipboard.items.length} elementos</Chip>
  {/snippet}

  <div class="flex h-full min-h-0 flex-col">
    <Toolbar label="Buscar en el historial">
      <div class="w-full">
        <Input
          type="search"
          bind:value={clipboard.query}
          placeholder="Buscar…"
          aria-label="Buscar en el historial"
        />
      </div>
    </Toolbar>

    <div class="min-h-0 flex-1 overflow-y-auto">
      {#if clipboard.visible.length === 0}
        <EmptyState
          compact
          icon={clipboard.query ? undefined : "clipboard"}
          title={clipboard.query ? "Nada coincide" : "El historial está vacío"}
          hint={clipboard.query
            ? "Probá con menos palabras."
            : "Copiá algo y aparece acá."}
        />
      {:else}
        <ul class="flex flex-col">
          {#each clipboard.visible as item (item.id)}
            <li
              class="group flex items-start gap-2 border-b border-line px-3 py-1.5
                     transition-colors duration-(--duration-quick) hover:bg-surface-2"
            >
              <button
                type="button"
                class="flex min-w-0 flex-1 items-start gap-2.5 text-left"
                onclick={() => void run(() => clipboard.paste(item.id), "Pegado")}
                title="Pegar en la app activa"
              >
                <span
                  class="mt-0.5 grid size-10 shrink-0 place-items-center overflow-hidden
                         rounded-sm bg-surface-2"
                  aria-hidden="true"
                >
                  {#if item.kind === "image" && item.imagePath}
                    <img
                      src={convertFileSrc(item.imagePath)}
                      alt=""
                      class="size-full object-cover"
                      loading="lazy"
                      draggable="false"
                    />
                  {:else}
                    <span class="text-micro font-semibold text-muted">Aa</span>
                  {/if}
                </span>

                <span class="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span class="line-clamp-2 text-sm text-text">
                    {#if item.kind === "image"}
                      {item.preview || "Imagen"}
                    {:else}
                      {item.preview || "(vacío)"}
                    {/if}
                  </span>
                  <span class="font-mono text-xs text-faint" data-numeric>
                    {item.kind === "image" ? "imagen · " : "texto · "}{formatListWhen(
                      Math.floor(item.createdAtMs / 1000),
                    )}
                  </span>
                </span>
              </button>

              <!-- Los controles solo aparecen al pasar por encima: en una lista
                   larga, tres iconos por fila compiten con el contenido. -->
              <div
                class="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity
                       duration-(--duration-quick)
                       group-hover:opacity-100 focus-within:opacity-100
                       {item.pinned ? 'opacity-100' : ''}"
              >
                <IconButton
                  label={item.pinned ? "Dejar de fijar" : "Fijar"}
                  size="sm"
                  pressed={item.pinned}
                  onclick={() => void run(() => clipboard.pin(item.id, !item.pinned))}
                >
                  <Icon icon={Pin} size={12} />
                </IconButton>
                <IconButton
                  label="Borrar"
                  size="sm"
                  variant="danger"
                  onclick={() => void run(() => clipboard.remove(item.id))}
                >
                  <Icon icon={Trash2} size={12} />
                </IconButton>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>
</ToolPage>
