<script lang="ts">
  /**
   * El launcher (Ctrl+Space): buscador con favoritos, recientes y acciones.
   * Teclado completo — ↑↓ mueven, ↵ ejecuta — como el de la app.
   *
   * Las filas de Atic salen del catálogo real (`LAUNCHER_ITEMS` se arma desde
   * `TOOLS`); los favoritos y recientes persisten en `localStorage`, como en
   * la app persisten en su config.
   */
  import Float from "../Float.svelte";
  import Icon from "$lib/atic/Icon.svelte";
  import { ArrowRight, Search, Star } from "$lib/atic/icons";
  import { LAUNCHER_ITEMS, type LauncherItem } from "../data";
  import { demo, loadJSON, saveJSON } from "../state.svelte";

  let { onClose }: { onClose?: () => void } = $props();

  const FAVS_KEY = "atic-demo-launcher-favs";
  const RECENTS_KEY = "atic-demo-launcher-recents";

  let query = $state("");
  let index = $state(0);
  let input = $state<HTMLInputElement | null>(null);
  let favs = $state<string[]>(loadJSON(FAVS_KEY, ["meetings", "clipboard"]));
  let recents = $state<string[]>(loadJSON(RECENTS_KEY, []));

  const byId = new Map(LAUNCHER_ITEMS.map((item) => [item.id, item]));

  type Section = { title: string | null; items: LauncherItem[] };

  const sections = $derived.by((): Section[] => {
    const q = query.trim().toLowerCase();
    if (q) {
      const hits = LAUNCHER_ITEMS.filter(
        (item) =>
          item.label.toLowerCase().includes(q) || item.hint.toLowerCase().includes(q),
      ).slice(0, 9);
      return [{ title: null, items: hits }];
    }
    const favItems = favs.map((id) => byId.get(id)).filter((item) => item !== undefined);
    const recentItems = recents
      .filter((id) => !favs.includes(id))
      .map((id) => byId.get(id))
      .filter((item) => item !== undefined)
      .slice(0, 4);
    const rest = LAUNCHER_ITEMS.filter(
      (item) => !favs.includes(item.id) && !recents.includes(item.id),
    );
    const out: Section[] = [];
    if (favItems.length > 0) out.push({ title: "Favoritos", items: favItems });
    if (recentItems.length > 0) out.push({ title: "Recientes", items: recentItems });
    out.push({ title: favItems.length + recentItems.length > 0 ? "Todo" : null, items: rest });
    return out;
  });

  /** La lista plana que recorren ↑↓, en el orden en que se ve. */
  const flat = $derived(sections.flatMap((section) => section.items));
  const position = $derived(new Map(flat.map((item, i) => [item.id, i] as const)));

  $effect(() => {
    // Al cambiar la búsqueda, la selección vuelve arriba.
    void query;
    index = 0;
  });

  function run(item: LauncherItem | undefined) {
    if (!item) return;
    recents = [item.id, ...recents.filter((id) => id !== item.id)].slice(0, 6);
    saveJSON(RECENTS_KEY, recents);
    demo.toast(`Abrir ${item.label}`);
    onClose?.();
  }

  function toggleFav(item: LauncherItem) {
    favs = favs.includes(item.id)
      ? favs.filter((id) => id !== item.id)
      : [...favs, item.id];
    saveJSON(FAVS_KEY, favs);
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      index = (index + 1) % Math.max(1, flat.length);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      index = (index - 1 + flat.length) % Math.max(1, flat.length);
    } else if (event.key === "Enter") {
      event.preventDefault();
      run(flat[index]);
    }
  }

  $effect(() => {
    input?.focus();
  });
</script>

<Float title="Apps" icon="launcher" onClose={onClose}>
  <div class="launcher">
    <label class="search">
      <Icon icon={Search} size={16} />
      <input
        bind:this={input}
        bind:value={query}
        onkeydown={onKeydown}
        placeholder="Buscar apps…"
        aria-label="Buscar apps y acciones"
        spellcheck="false"
        autocomplete="off"
      />
      <kbd>Esc</kbd>
    </label>

    <div class="results">
      {#each sections as section (section.title ?? "hits")}
        {#if section.title}
          <h4 class="section-title">{section.title}</h4>
        {/if}
        <ul class="list" role="listbox" aria-label={section.title ?? "Resultados"}>
          {#each section.items as item (item.id)}
            {@const i = position.get(item.id) ?? 0}
            <li>
              <div class="row" class:is-sel={i === index}>
                <button
                  type="button"
                  class="item"
                  role="option"
                  aria-selected={i === index}
                  onpointerenter={() => (index = i)}
                  onclick={() => run(item)}
                >
                  <span class="item-icon" class:is-action={item.kind === "action"}>
                    <Icon icon={item.icon} size={16} />
                  </span>
                  <span class="item-text">
                    <span class="item-label">{item.label}</span>
                    <span class="item-hint">{item.hint}</span>
                  </span>
                  {#if item.shortcut}
                    <span class="item-kbd">{item.shortcut.replaceAll("+", " ")}</span>
                  {/if}
                  <span class="item-go"><Icon icon={ArrowRight} size={14} /></span>
                </button>
                <button
                  type="button"
                  class="fav"
                  class:is-fav={favs.includes(item.id)}
                  aria-label={favs.includes(item.id)
                    ? `Quitar ${item.label} de favoritos`
                    : `Agregar ${item.label} a favoritos`}
                  title={favs.includes(item.id)
                    ? `Quitar ${item.label} de favoritos`
                    : `Agregar ${item.label} a favoritos`}
                  onclick={() => toggleFav(item)}
                >
                  <Icon icon={Star} size={13} />
                </button>
              </div>
            </li>
          {/each}
        </ul>
      {:else}
        <p class="empty">Sin resultados para “{query}”.</p>
      {/each}
    </div>
  </div>

  {#snippet footer()}
    <div class="foot">
      <span><kbd>↑↓</kbd> navegar</span>
      <span><kbd>↵</kbd> abrir</span>
      <span><kbd>Esc</kbd> cerrar</span>
    </div>
  {/snippet}
</Float>

<style>
  .launcher {
    display: flex;
    flex-direction: column;
  }

  .search {
    display: flex;
    height: 46px;
    flex-shrink: 0;
    align-items: center;
    gap: 9px;
    border-bottom: 1px solid var(--line);
    padding: 0 12px;
    color: var(--faint);
  }

  .search:focus-within {
    color: var(--muted);
  }

  .search input {
    min-width: 0;
    flex: 1;
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--text-md);
    outline: none;
  }

  .search input::placeholder {
    color: var(--faint);
  }

  .results {
    max-height: 340px;
    overflow: auto;
  }

  .section-title {
    margin: 0;
    padding: 8px 14px 2px;
    color: var(--faint);
    font-size: var(--text-micro);
    font-weight: var(--font-weight-semibold);
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
  }

  .list {
    margin: 0;
    padding: 4px 6px 6px;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: center;
    border-radius: var(--radius-sm);
  }

  .row.is-sel {
    background: color-mix(in srgb, var(--text) 8%, transparent);
  }

  .item {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
    gap: 10px;
    border: 0;
    padding: 7px 4px 7px 8px;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .item-icon {
    display: grid;
    width: 28px;
    height: 28px;
    flex-shrink: 0;
    place-items: center;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--text) 7%, transparent);
    color: var(--muted);
    font-size: 0;
  }

  /* Las acciones de Atic llevan el icono en verde, como en la app. */
  .item-icon.is-action {
    color: var(--ok);
  }

  .item-text {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    gap: 1px;
  }

  .item-label {
    overflow: hidden;
    font-size: var(--text-base);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .item-hint {
    overflow: hidden;
    color: var(--faint);
    font-size: var(--text-micro);
    letter-spacing: 0.02em;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .item-kbd {
    flex-shrink: 0;
    color: var(--faint);
    font-family: var(--font-mono);
    font-size: 0.625rem;
  }

  .item-go {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    color: var(--faint);
    opacity: 0;
  }

  .row.is-sel .item-go {
    opacity: 1;
  }

  .fav {
    display: grid;
    width: 26px;
    height: 26px;
    flex-shrink: 0;
    place-items: center;
    border: 0;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--faint);
    cursor: pointer;
    opacity: 0;
  }

  .row:hover .fav,
  .row.is-sel .fav,
  .fav.is-fav {
    opacity: 1;
  }

  .fav.is-fav {
    color: var(--warn);
  }

  .empty {
    margin: 0;
    padding: 18px 12px;
    color: var(--faint);
    font-size: var(--text-sm);
    text-align: center;
  }

  .foot {
    display: flex;
    gap: 12px;
  }

  kbd {
    padding: 2px 6px;
    border: 1px solid var(--line);
    border-radius: var(--radius-xs);
    color: var(--faint);
    font-family: var(--font-mono);
    font-size: 0.625rem;
  }
</style>
