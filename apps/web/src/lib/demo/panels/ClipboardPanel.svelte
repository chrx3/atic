<script lang="ts">
  /**
   * El historial de portapapeles, tal como lo abre la pill en la app: lista
   * con búsqueda fuzzy, fijados arriba, filtro por tipo y acciones por fila.
   *
   * La búsqueda es la real (`fuzzyMatch` del core): substring, todos los
   * tokens, o letras en orden. Y clic copia de verdad — en el navegador es el
   * equivalente honesto del "pegar" de la app: el texto queda listo para
   * pegarse en cualquier parte.
   */
  import Float from "../Float.svelte";
  import Icon from "$lib/atic/Icon.svelte";
  import { FileText, Image, Pin, Search, Trash2 } from "$lib/atic/icons";
  import type { IconNode } from "$lib/atic/icons";
  import { CLIPBOARD, type ClipEntry, type ClipKind } from "../data";
  import { fuzzyMatch } from "$atic/lib/core/clipboardSearch";
  import { copyText } from "../state.svelte";

  let { onClose }: { onClose?: () => void } = $props();

  let query = $state("");
  let input = $state<HTMLInputElement | null>(null);
  let items = $state<ClipEntry[]>(CLIPBOARD);
  let kindFilter = $state<"all" | ClipKind>("all");
  let onlyPinned = $state(false);

  const kindIcon: Record<ClipKind, IconNode> = { text: FileText, image: Image };

  /**
   * Fecha corta como la app (`formatListWhen`): hoy solo hora, ayer
   * etiquetado, esta semana el día, más atrás fecha media. Versión local
   * porque la del core arrastra el estado de idioma.
   */
  function formatWhen(epochMs: number): string {
    const value = new Date(epochMs);
    if (Number.isNaN(value.getTime())) return "";
    const now = new Date();
    const startToday = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    const startThat = new Date(value.getFullYear(), value.getMonth(), value.getDate());
    const dayDiff = Math.round((startToday.getTime() - startThat.getTime()) / 86_400_000);
    const time = new Intl.DateTimeFormat("es-CL", {
      hour: "2-digit",
      minute: "2-digit",
    }).format(value);
    if (dayDiff <= 0) return time;
    if (dayDiff === 1) return `Ayer · ${time}`;
    if (dayDiff < 7) {
      const weekday = new Intl.DateTimeFormat("es-CL", { weekday: "short" }).format(value);
      return `${weekday} · ${time}`;
    }
    return new Intl.DateTimeFormat("es-CL", {
      day: "numeric",
      month: "short",
      hour: "2-digit",
      minute: "2-digit",
    }).format(value);
  }

  const entries = $derived.by(() => {
    const q = query.trim();
    const list = items.filter((entry) => {
      if (kindFilter !== "all" && entry.kind !== kindFilter) return false;
      if (onlyPinned && !entry.pinned) return false;
      return fuzzyMatch(entry.text, q);
    });
    return [...list].sort((a, b) => Number(b.pinned ?? false) - Number(a.pinned ?? false));
  });

  async function pick(entry: ClipEntry) {
    await copyText(entry.text, "Copiado — listo para pegar");
  }

  function togglePin(entry: ClipEntry) {
    items = items.map((item) =>
      item.id === entry.id ? { ...item, pinned: !item.pinned } : item,
    );
  }

  function remove(entry: ClipEntry) {
    items = items.filter((item) => item.id !== entry.id);
  }

  $effect(() => {
    input?.focus();
  });
</script>

<Float title="Clipboard" icon="clipboard" onClose={onClose}>
  <div class="clip">
    <label class="search">
      <Icon icon={Search} size={15} />
      <input
        bind:this={input}
        bind:value={query}
        placeholder="Buscar en el historial…"
        aria-label="Buscar en el historial"
        spellcheck="false"
        autocomplete="off"
      />
    </label>

    <div class="toolbar" role="group" aria-label="Filtrar por tipo">
      {#each [["all", "Todo"], ["text", "Texto"], ["image", "Imágenes"]] as [value, label] (value)}
        <button
          type="button"
          class="chip"
          class:is-on={kindFilter === value}
          aria-pressed={kindFilter === value}
          onclick={() => (kindFilter = value as typeof kindFilter)}
        >
          {label}
        </button>
      {/each}
      <button
        type="button"
        class="chip"
        class:is-on={onlyPinned}
        aria-pressed={onlyPinned}
        onclick={() => (onlyPinned = !onlyPinned)}
      >
        <Icon icon={Pin} size={11} /> Fijados
      </button>
      <span class="count">{entries.length}</span>
    </div>

    <ul class="list">
      {#each entries as entry (entry.id)}
        <li class="row">
          <button type="button" class="item" onclick={() => pick(entry)}>
            <span class="kind"><Icon icon={kindIcon[entry.kind]} size={13} /></span>
            <span class="text">{entry.text}</span>
            <span class="when">{formatWhen(entry.createdAtMs)}</span>
          </button>
          <span class="actions">
            <button
              type="button"
              class="mini"
              class:is-pinned={entry.pinned}
              aria-label={entry.pinned ? "Desfijar" : "Fijar arriba"}
              title={entry.pinned ? "Desfijar" : "Fijar arriba"}
              onclick={() => togglePin(entry)}
            >
              <Icon icon={Pin} size={12} />
            </button>
            <button
              type="button"
              class="mini is-danger"
              aria-label="Borrar del historial"
              title="Borrar del historial"
              onclick={() => remove(entry)}
            >
              <Icon icon={Trash2} size={12} />
            </button>
          </span>
        </li>
      {:else}
        <li class="empty">Nada en el historial con “{query}”.</li>
      {/each}
    </ul>
  </div>

  {#snippet footer()}
    <div class="foot">
      <span>Clic copia al portapapeles — listo para pegar donde sea</span>
    </div>
  {/snippet}
</Float>

<style>
  .search {
    display: flex;
    height: 42px;
    flex-shrink: 0;
    align-items: center;
    gap: 8px;
    border-bottom: 1px solid var(--line);
    padding: 0 12px;
    color: var(--faint);
  }

  .search input {
    min-width: 0;
    flex: 1;
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--text-base);
    outline: none;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 5px;
    border-bottom: 1px solid var(--line);
    padding: 7px 10px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: 1px solid var(--line);
    border-radius: var(--radius-pill);
    padding: 3px 9px;
    background: transparent;
    color: var(--faint);
    font: inherit;
    font-size: var(--text-micro);
    cursor: pointer;
  }

  .chip:hover {
    border-color: var(--line-strong);
    color: var(--muted);
  }

  .chip.is-on {
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
    color: var(--text);
  }

  .count {
    margin-left: auto;
    color: var(--faint);
    font-size: var(--text-micro);
    font-variant-numeric: tabular-nums;
  }

  .list {
    margin: 0;
    padding: 6px;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: center;
    border-radius: var(--radius-sm);
  }

  .row:hover {
    background: color-mix(in srgb, var(--text) 7%, transparent);
  }

  .item {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
    gap: 9px;
    border: 0;
    padding: 7px 4px 7px 8px;
    background: transparent;
    color: var(--text);
    cursor: pointer;
    font: inherit;
    text-align: left;
  }

  .kind {
    display: grid;
    width: 24px;
    height: 24px;
    flex-shrink: 0;
    place-items: center;
    border-radius: 7px;
    background: color-mix(in srgb, var(--text) 7%, transparent);
    color: var(--muted);
    font-size: 0;
  }

  .text {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    font-size: var(--text-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .when {
    flex-shrink: 0;
    color: var(--faint);
    font-size: var(--text-micro);
    font-variant-numeric: tabular-nums;
  }

  .actions {
    display: flex;
    flex-shrink: 0;
    gap: 1px;
    opacity: 0;
  }

  .row:hover .actions,
  .row:focus-within .actions {
    opacity: 1;
  }

  .mini {
    display: grid;
    width: 24px;
    height: 24px;
    place-items: center;
    border: 0;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--faint);
    cursor: pointer;
  }

  .mini:hover {
    background: color-mix(in srgb, var(--text) 10%, transparent);
    color: var(--text);
  }

  .mini.is-pinned {
    color: var(--warn);
    opacity: 1;
  }

  .row .actions:has(.is-pinned) {
    opacity: 1;
  }

  .mini.is-danger:hover {
    background: color-mix(in srgb, var(--danger) 14%, transparent);
    color: var(--danger);
  }

  .empty {
    padding: 18px 12px;
    color: var(--faint);
    font-size: var(--text-sm);
    text-align: center;
  }

  .foot {
    display: flex;
    align-items: center;
  }
</style>
