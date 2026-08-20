<script lang="ts">
  /**
   * Buscar en todo lo guardado: grabaciones, textos, portapapeles, capturas.
   *
   * La búsqueda la hace Rust, que es quien tiene los índices. Acá solo se
   * escribe, se espera un momento y se elige.
   */
  import type { SearchHit } from "$core/types";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import { t } from "$domain/i18n.svelte";
  import { pasteClipboardItem } from "$ipc/clipboard";
  import { activateCapture } from "$ipc/captures";
  import { searchLocal } from "$ipc/search";
  import { pasteSnippet } from "$ipc/snippets";
  import Modal from "$ui/Modal.svelte";
  import EmptyState from "$ui/EmptyState.svelte";

  let {
    onClose,
    onNavigate,
  }: {
    onClose: () => void;
    /** Para los resultados que no son una acción sino un sitio de la app. */
    onNavigate: (hit: SearchHit) => void;
  } = $props();

  const KIND_LABEL = $derived({
    recording: t("page.search.recording"),
    snippet: t("page.search.snippet"),
    clipboard: t("page.search.clipboard"),
    capture: t("page.search.capture"),
    scratchpad: t("page.search.scratchpad"),
  } as const);

  let query = $state("");
  let hits = $state<SearchHit[]>([]);
  let active = $state(0);
  let input = $state<HTMLInputElement | null>(null);

  /**
   * Se consulta con retardo.
   *
   * Sin él, escribir «reunión» dispara siete búsquedas y las respuestas pueden
   * llegar desordenadas, dejando en pantalla los resultados de «reuni».
   */
  $effect(() => {
    const q = query.trim();
    if (!q) {
      hits = [];
      return;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      void searchLocal(q)
        .then((found) => {
          if (cancelled) return;
          hits = found;
          active = 0;
        })
        .catch(toastError);
    }, 140);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  });

  $effect(() => {
    input?.focus();
  });

  /**
   * Elegir hace lo obvio para cada tipo.
   *
   * Un texto o algo del portapapeles se pegan —es para lo que existen—; una
   * grabación o el bloc son sitios adonde ir.
   */
  async function choose(hit: SearchHit) {
    try {
      switch (hit.kind) {
        case "snippet":
          await pasteSnippet(hit.id);
          toasts.push(t("toast.pastedNamed", { title: hit.title }));
          break;
        case "clipboard":
          await pasteClipboardItem(hit.id);
          toasts.push(t("toast.pastedClipboard"));
          break;
        case "capture":
          await activateCapture(hit.id);
          break;
        default:
          onNavigate(hit);
      }
      onClose();
    } catch (error) {
      toastError(error);
    }
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      active = Math.min(active + 1, hits.length - 1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      active = Math.max(active - 1, 0);
    } else if (event.key === "Enter" && hits[active]) {
      event.preventDefault();
      void choose(hits[active]);
    }
  }
</script>

<Modal title={t("page.search.title")} size="md" {onClose}>
  {#snippet header()}
    <!-- El campo ES el encabezado: un título encima solo aleja el cursor de lo
         único que hay que hacer acá. -->
    <input
      bind:this={input}
      bind:value={query}
      onkeydown={onKey}
      type="search"
      placeholder={t("page.search.placeholder")}
      aria-label={t("page.search.title")}
      class="h-8 w-full bg-transparent text-md text-text outline-none
             placeholder:text-faint"
    />
  {/snippet}

  {#if !query.trim()}
    <EmptyState title={t("page.search.typeToSearch")} hint={t("page.search.typeHint")} />
  {:else if hits.length === 0}
    <EmptyState title={t("page.common.nothing")} hint={t("page.common.fewerWords")} />
  {:else}
    <ul class="-mx-2 flex flex-col">
      {#each hits as hit, i (hit.kind + hit.id)}
        <li>
          <button
            type="button"
            class="flex w-full items-baseline gap-2 rounded-sm px-2 py-1.5 text-left
                   {i === active ? 'bg-surface-2' : ''}"
            onmouseenter={() => (active = i)}
            onclick={() => void choose(hit)}
          >
            <span class="w-20 shrink-0 text-micro text-faint uppercase">
              {KIND_LABEL[hit.kind]}
            </span>
            <span class="min-w-0 flex-1">
              <span class="block truncate text-sm text-text">{hit.title}</span>
              {#if hit.preview}
                <span class="block truncate text-xs text-faint">{hit.preview}</span>
              {/if}
            </span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</Modal>
