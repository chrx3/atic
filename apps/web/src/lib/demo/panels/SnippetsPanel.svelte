<script lang="ts">
  /**
   * Textos y notas: los que guardas a propósito, más el bloc de notas sueltas.
   * Distinto del historial — esto no se llena solo.
   *
   * Como en la app: tabs Textos|Notas, búsqueda por nombre o palabra, y el
   * bloc que se guarda solo. El clic copia (en el navegador es el equivalente
   * honesto del "pegar en la app activa"). Lo creado acá persiste en
   * `localStorage`.
   */
  import Float from "../Float.svelte";
  import Icon from "$lib/atic/Icon.svelte";
  import { Copy, FileText, Plus, Trash2 } from "$lib/atic/icons";
  import { SNIPPETS, type Snippet } from "../data";
  import { copyText, demo, loadJSON, saveJSON } from "../state.svelte";

  let { onClose }: { onClose?: () => void } = $props();

  const KEY = "atic-demo-snippets";
  const PAD_KEY = "atic-demo-scratchpad";

  let items = $state<Snippet[]>(loadJSON(KEY, SNIPPETS));
  let pad = $state(loadJSON(PAD_KEY, ""));
  let tab = $state<"list" | "pad">("list");
  let query = $state("");
  let adding = $state(false);
  let name = $state("");
  let body = $state("");
  let confirming: string | null = $state(null);
  let padTimer = 0;

  function persist() {
    saveJSON(KEY, items);
  }

  const entries = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter(
      (item) =>
        item.name.toLowerCase().includes(q) ||
        item.body.toLowerCase().includes(q) ||
        item.aliases.some((alias) => alias.toLowerCase().includes(q)),
    );
  });

  async function copy(snippet: Snippet) {
    await copyText(snippet.body, `“${snippet.name}” copiado`);
  }

  function add() {
    const cleanName = name.trim();
    const cleanBody = body.trim();
    if (!cleanName || !cleanBody) {
      demo.toast("Nombre y texto, por favor", "info");
      return;
    }
    items = [
      {
        id: `s${Date.now()}`,
        name: cleanName,
        body: cleanBody,
        aliases: [],
        updatedAtMs: Date.now(),
      },
      ...items,
    ];
    persist();
    name = "";
    body = "";
    adding = false;
    demo.toast("Texto guardado", "ok");
  }

  function remove(id: string) {
    if (confirming !== id) {
      confirming = id;
      window.setTimeout(() => {
        if (confirming === id) confirming = null;
      }, 3000);
      return;
    }
    confirming = null;
    items = items.filter((item) => item.id !== id);
    persist();
  }

  function onPadInput(value: string) {
    pad = value;
    window.clearTimeout(padTimer);
    // Como en la app (`editScratchpad` con debounce): se guarda solo.
    padTimer = window.setTimeout(() => saveJSON(PAD_KEY, pad), 600);
  }
</script>

<Float title="Textos y notas" icon="snippets" onClose={onClose}>
  <div class="snips">
    <div class="tabs" role="tablist" aria-label="Textos y notas">
      <button
        type="button"
        role="tab"
        class="tab"
        class:is-active={tab === "list"}
        aria-selected={tab === "list"}
        onclick={() => (tab = "list")}
      >
        Textos
      </button>
      <button
        type="button"
        role="tab"
        class="tab"
        class:is-active={tab === "pad"}
        aria-selected={tab === "pad"}
        onclick={() => (tab = "pad")}
      >
        Notas
      </button>
    </div>

    {#if tab === "list"}
      <label class="search">
        <input
          bind:value={query}
          placeholder="Buscar textos…"
          aria-label="Buscar textos guardados"
          spellcheck="false"
          autocomplete="off"
        />
      </label>

      {#if adding}
        <form
          class="new"
          onsubmit={(event) => {
            event.preventDefault();
            add();
          }}
        >
          <input
            bind:value={name}
            placeholder="Nombre (p. ej. Datos de facturación)"
            aria-label="Nombre del texto"
            autocomplete="off"
          />
          <textarea
            bind:value={body}
            placeholder="El texto que quieres tener a mano…"
            aria-label="Contenido del texto"
            rows="4"
          ></textarea>
          <div class="new-actions">
            <button type="submit" class="btn is-primary">Guardar</button>
            <button
              type="button"
              class="btn"
              onclick={() => {
                adding = false;
                name = "";
                body = "";
              }}
            >
              Cancelar
            </button>
          </div>
        </form>
      {/if}

      {#each entries as snippet (snippet.id)}
        <div class="row">
          <button type="button" class="item" onclick={() => copy(snippet)}>
            <span class="icon"><Icon icon={FileText} size={14} /></span>
            <span class="item-text">
              <span class="item-name">{snippet.name}</span>
              <span class="item-body">{snippet.body.split("\n")[0]}</span>
              {#if snippet.aliases.length > 0}
                <span class="item-aliases">{snippet.aliases.join(" · ")}</span>
              {/if}
            </span>
            <span class="copy"><Icon icon={Copy} size={13} /></span>
          </button>
          <button
            type="button"
            class="remove"
            class:is-confirm={confirming === snippet.id}
            aria-label="Borrar «{snippet.name}»"
            title="Borrar «{snippet.name}»"
            onclick={() => remove(snippet.id)}
          >
            {#if confirming === snippet.id}
              <span class="confirm-label">¿Borrar?</span>
            {:else}
              <Icon icon={Trash2} size={13} />
            {/if}
          </button>
        </div>
      {:else}
        <p class="empty">
          {query ? `Sin textos con “${query}”.` : "Todavía no hay textos guardados"}
        </p>
      {/each}
    {:else}
      <textarea
        class="pad"
        value={pad}
        oninput={(e) => onPadInput(e.currentTarget.value)}
        placeholder="Notas temporales…"
        aria-label="Bloc de notas"
      ></textarea>
    {/if}
  </div>

  {#snippet footer()}
    <div class="foot">
      <span>{tab === "list" ? "Clic copia el texto completo" : "El bloc se guarda solo"}</span>
      {#if tab === "list"}
        <button type="button" class="btn" onclick={() => (adding = !adding)}>
          {#if adding}Cancelar{:else}<Icon icon={Plus} size={12} /> Nuevo{/if}
        </button>
      {/if}
    </div>
  {/snippet}
</Float>

<style>
  .tabs {
    display: flex;
    gap: 2px;
    border-bottom: 1px solid var(--line);
    padding: 6px 8px 0;
  }

  .tab {
    border: 0;
    border-bottom: 2px solid transparent;
    padding: 7px 10px 8px;
    background: transparent;
    color: var(--faint);
    font: inherit;
    font-size: var(--text-xs);
    cursor: pointer;
  }

  .tab:hover {
    color: var(--muted);
  }

  .tab.is-active {
    border-bottom-color: var(--accent);
    color: var(--text);
  }

  .search {
    display: block;
    border-bottom: 1px solid var(--line);
    padding: 8px 12px;
  }

  .search input {
    width: 100%;
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--text-base);
    outline: none;
  }

  .search input::placeholder {
    color: var(--faint);
  }

  .row {
    position: relative;
    display: flex;
    align-items: center;
    padding: 0 6px;
  }

  .item {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
    gap: 9px;
    border: 0;
    border-radius: var(--radius-sm);
    padding: 7px 8px;
    background: transparent;
    color: var(--text);
    cursor: pointer;
    font: inherit;
    text-align: left;
  }

  .item:hover {
    background: color-mix(in srgb, var(--text) 7%, transparent);
  }

  .icon {
    display: grid;
    width: 26px;
    height: 26px;
    flex-shrink: 0;
    place-items: center;
    border-radius: 7px;
    background: color-mix(in srgb, var(--text) 7%, transparent);
    color: var(--muted);
    font-size: 0;
  }

  .item-text {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    gap: 1px;
  }

  .item-name {
    font-size: var(--text-sm);
  }

  .item-body {
    overflow: hidden;
    color: var(--faint);
    font-size: var(--text-micro);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .item-aliases {
    color: var(--faint);
    font-family: var(--font-mono);
    font-size: 0.59375rem;
    opacity: 0.8;
  }

  .copy {
    display: grid;
    width: 22px;
    height: 22px;
    flex-shrink: 0;
    place-items: center;
    color: var(--faint);
    opacity: 0;
    font-size: 0;
  }

  .item:hover .copy {
    opacity: 1;
  }

  .remove {
    display: flex;
    height: 24px;
    flex-shrink: 0;
    align-items: center;
    border: 0;
    border-radius: var(--radius-xs);
    padding: 0 6px;
    background: transparent;
    color: var(--faint);
    cursor: pointer;
    opacity: 0;
  }

  .row:hover .remove {
    opacity: 1;
  }

  .remove:hover {
    background: color-mix(in srgb, var(--danger) 14%, transparent);
    color: var(--danger);
  }

  .remove.is-confirm {
    background: color-mix(in srgb, var(--danger) 14%, transparent);
    color: var(--danger);
    opacity: 1;
  }

  .confirm-label {
    font-size: var(--text-micro);
    white-space: nowrap;
  }

  .empty {
    margin: 0;
    padding: 18px 12px;
    color: var(--faint);
    font-size: var(--text-sm);
    text-align: center;
  }

  .pad {
    display: block;
    width: 100%;
    min-height: 240px;
    border: 0;
    padding: 12px 14px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--text-sm);
    line-height: 1.55;
    outline: none;
    resize: vertical;
  }

  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    padding: 4px 9px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--text-micro);
    cursor: pointer;
  }

  .btn:hover {
    border-color: var(--line-strong);
    color: var(--text);
  }

  .btn.is-primary {
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    background: var(--accent);
    color: var(--on-accent);
  }

  .new {
    display: flex;
    flex-direction: column;
    gap: 8px;
    border-bottom: 1px solid var(--line);
    padding: 12px;
  }

  .new input,
  .new textarea {
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    padding: 7px 9px;
    background: var(--surface-2);
    color: var(--text);
    font: inherit;
    font-size: var(--text-sm);
    outline: none;
    resize: vertical;
  }

  .new input:focus,
  .new textarea:focus {
    border-color: var(--line-strong);
  }

  .new-actions {
    display: flex;
    gap: 6px;
  }
</style>
