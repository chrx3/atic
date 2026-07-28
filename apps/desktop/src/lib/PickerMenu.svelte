<script lang="ts">
  /**
   * Pastilla con menú, al estilo de los compositores de agentes.
   *
   * Existe en vez de un `<select>` por dos motivos: el nativo pinta con el
   * tema del sistema y rompe el tono de la consola, y sobre todo abre hacia
   * abajo — dentro de una burbuja anclada al borde de la pantalla eso queda
   * fuera de la ventana. Este abre hacia ARRIBA, que es donde hay sitio cuando
   * el control vive en el compositor.
   */
  interface Option {
    id: string;
    label: string;
    /** Qué cambia si eliges esto. Sin ella, elegir es adivinar. */
    note?: string;
    disabled?: boolean;
  }

  import type { Snippet } from "svelte";
  import type { Attachment } from "svelte/attachments";

  let {
    label,
    options,
    value,
    open,
    onToggle,
    onPick,
    icon,
    loading = false,
    loadingMessage = "Cargando modelos…",
    footer,
    searchable,
    iconOnly = false,
    title,
    ariaLabel,
  }: {
    label: string;
    /** Ícono a la izquierda de la etiqueta. Opcional: no todas lo necesitan. */
    icon?: Snippet;
    options: Option[];
    value: string;
    open: boolean;
    onToggle: () => void;
    onPick: (id: string) => void;
    loading?: boolean;
    loadingMessage?: string;
    footer?: Snippet;
    /** Forzar búsqueda; por defecto se activa con 6+ opciones. */
    searchable?: boolean;
    /** Solo ícono + caret; la etiqueta sigue en title / aria-label. */
    iconOnly?: boolean;
    title?: string;
    ariaLabel?: string;
  } = $props();

  let query = $state("");

  const showSearch = $derived(
    searchable === true || (searchable !== false && options.length >= 6),
  );

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return options;
    return options.filter((o) => {
      const hay = `${o.label} ${o.note ?? ""} ${o.id}`.toLowerCase();
      return hay.includes(q);
    });
  });

  /** Con catálogo vacío, el popover se muestra solo para el mensaje de carga. */
  const popOpen = $derived(open || (loading && options.length === 0));
  const showLoading = $derived(loading && (open || options.length === 0));

  const chipTitle = $derived(title ?? label);
  const chipAria = $derived(ariaLabel ?? label);

  const focusSearch: Attachment<HTMLInputElement> = (node) => {
    node.focus();
  };

  function onSearchKeydown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    if (query) {
      query = "";
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    onToggle();
  }

  function toggle() {
    if (!open) query = "";
    onToggle();
  }

  let root = $state<HTMLDivElement | null>(null);

  // Cierra al pulsar fuera. En capture + timeout 0 para no comerse el clic
  // que acaba de abrirlo.
  $effect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      const el = root;
      const target = event.target;
      if (!(target instanceof Node) || !el) return;
      if (el.contains(target)) return;
      onToggle();
    };
    const id = window.setTimeout(() => {
      window.addEventListener("pointerdown", onPointerDown, true);
    }, 0);
    return () => {
      window.clearTimeout(id);
      window.removeEventListener("pointerdown", onPointerDown, true);
    };
  });
</script>

<div class="pm" bind:this={root}>
  {#if popOpen}
    <div class="pm-pop" role="listbox" aria-busy={showLoading}>
      {#if showLoading && options.length === 0}
        <div class="pm-loading" role="status">
          <span class="pm-spinner" aria-hidden="true"></span>
          <span>{loadingMessage}</span>
        </div>
      {:else}
        {#if showSearch}
          <div class="pm-search">
            <input
              class="pm-search-input"
              type="search"
              placeholder="Buscar…"
              bind:value={query}
              onkeydown={onSearchKeydown}
              aria-label="Buscar opciones"
              {@attach focusSearch}
            />
          </div>
        {/if}
        {#if showLoading}
          <div class="pm-loading" role="status">
            <span class="pm-spinner" aria-hidden="true"></span>
            <span>{loadingMessage}</span>
          </div>
        {:else}
          <ul class="pm-list">
            {#each filtered as o (o.id)}
              <li>
                <button
                  type="button"
                  role="option"
                  aria-selected={o.id === value}
                  class="pm-opt"
                  class:active={o.id === value}
                  disabled={o.disabled}
                  onclick={() => onPick(o.id)}
                >
                  <span class="pm-opt-l">{o.label}</span>
                  {#if o.note}
                    <span class="pm-opt-n">{o.note}</span>
                  {/if}
                </button>
              </li>
            {:else}
              <li class="pm-empty">Sin coincidencias</li>
            {/each}
          </ul>
        {/if}
      {/if}
      {#if footer}
        <div class="pm-footer">{@render footer()}</div>
      {/if}
    </div>
  {/if}

  <button
    type="button"
    class="pm-chip"
    class:is-open={popOpen}
    class:is-icon={iconOnly}
    aria-haspopup="listbox"
    aria-expanded={popOpen}
    aria-label={chipAria}
    title={chipTitle}
    onclick={toggle}
  >
    {#if icon}<span class="pm-icon">{@render icon()}</span>{/if}
    {#if !iconOnly}
      <span class="pm-label">{label}</span>
    {/if}
    <span class="pm-caret" aria-hidden="true">⌄</span>
  </button>
</div>

<style>
  .pm {
    position: relative;
  }

  .pm-chip {
    display: inline-flex;
    max-width: 10rem;
    align-items: center;
    gap: 0.25rem;
    border: 1px solid var(--line, #332e2b);
    border-radius: 999px;
    padding: 0.2rem 0.55rem;
    background: transparent;
    color: var(--dim, #8d827a);
    font-family: inherit;
    font-size: 0.6875rem;
    cursor: pointer;
  }
  .pm-chip.is-icon {
    max-width: none;
    padding: 0.2rem 0.35rem;
  }
  .pm-chip:hover,
  .pm-chip.is-open {
    color: var(--text, #e7e2dd);
  }

  .pm-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pm-icon {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    color: var(--dim);
  }

  .pm-caret {
    flex-shrink: 0;
    font-size: 0.625rem;
    line-height: 1;
  }

  .pm-pop {
    position: absolute;
    bottom: calc(100% + 0.35rem);
    left: 0;
    z-index: 20;
    display: flex;
    min-width: max(14rem, 100%);
    max-width: 20rem;
    flex-direction: column;
    border: 1px solid var(--line, #332e2b);
    border-radius: 0.6rem;
    background: #262120;
    box-shadow: 0 10px 26px rgb(0 0 0 / 45%);
    opacity: 1;
    transform: translateY(0);
    transition:
      opacity 120ms ease,
      transform 120ms ease;
  }

  .pm-search {
    position: sticky;
    top: 0;
    z-index: 1;
    flex-shrink: 0;
    border-bottom: 1px solid var(--line, #332e2b);
    padding: 0.35rem;
    background: #262120;
  }

  .pm-search-input {
    box-sizing: border-box;
    width: 100%;
    border: 1px solid var(--line, #332e2b);
    border-radius: 0.4rem;
    padding: 0.3rem 0.45rem;
    background: #1c1918;
    color: var(--text, #e7e2dd);
    font-family: inherit;
    font-size: 0.6875rem;
    outline: none;
  }
  .pm-search-input::placeholder {
    color: var(--faint, #6b615a);
  }
  .pm-search-input:focus-visible {
    border-color: var(--coral, #d97757);
  }

  .pm-list {
    max-height: min(18rem, 50vh);
    margin: 0;
    overflow-x: hidden;
    overflow-y: auto;
    padding: 0.2rem;
    list-style: none;
  }

  .pm-opt {
    display: block;
    width: 100%;
    border: 0;
    border-radius: 0.4rem;
    padding: 0.3rem 0.55rem;
    background: transparent;
    color: var(--dim, #8d827a);
    font-family: inherit;
    font-size: 0.6875rem;
    text-align: left;
    cursor: pointer;
  }
  .pm-opt:hover:not(:disabled) {
    background: #332e2b;
    color: var(--text, #e7e2dd);
  }
  .pm-opt.active .pm-opt-l {
    color: var(--coral, #d97757);
  }

  .pm-opt-l {
    display: block;
    overflow: hidden;
    color: var(--text, #e7e2dd);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* El rasgo, más pequeño y apagado: se lee al dudar, no compite con el
     nombre cuando ya sabes cuál quieres. */
  .pm-opt-n {
    display: block;
    margin-top: 0.05rem;
    overflow: hidden;
    color: var(--faint, #6b615a);
    font-size: 0.625rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pm-opt:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .pm-empty {
    padding: 0.55rem 0.55rem;
    color: var(--faint, #6b615a);
    font-size: 0.6875rem;
  }

  .pm-loading {
    display: flex;
    align-items: center;
    gap: 0.55rem;
    padding: 0.85rem 0.8rem;
    color: var(--dim, #8d827a);
    font-size: 0.6875rem;
    line-height: 1.35;
  }

  .pm-spinner {
    flex-shrink: 0;
    width: 0.75rem;
    height: 0.75rem;
    border: 1.5px solid var(--line, #332e2b);
    border-top-color: var(--coral, #d97757);
    border-radius: 50%;
    animation: pm-spin 0.7s linear infinite;
  }

  @keyframes pm-spin {
    to {
      transform: rotate(360deg);
    }
  }

  .pm-footer {
    flex-shrink: 0;
    border-top: 1px solid var(--line, #332e2b);
    padding: 0.25rem;
  }

  .pm-footer :global(button) {
    display: block;
    width: 100%;
    border: 0;
    border-radius: 0.4rem;
    padding: 0.35rem 0.55rem;
    background: transparent;
    color: var(--dim, #8d827a);
    font-family: inherit;
    font-size: 0.6875rem;
    text-align: left;
    cursor: pointer;
  }
  .pm-footer :global(button:hover) {
    background: #332e2b;
    color: var(--text, #e7e2dd);
  }
</style>
