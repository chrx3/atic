<script lang="ts">
  /**
   * Qué modelos aparecen en el selector para Cursor / OpenCode.
   *
   * El catálogo lo da el proveedor; acá solo se elige la lista corta que
   * conviene tener a mano. Vacío al guardar = mostrar todos.
   */
  import { untrack } from "svelte";
  import ModalShell from "$lib/ModalShell.svelte";
  import { visibleModelIds } from "$lib/agentModels";

  let {
    backendId,
    backendLabel,
    models,
    onSave,
    onClose,
  }: {
    backendId: string;
    backendLabel: string;
    models: { id: string; label: string; note?: string }[];
    onSave: (ids: string[]) => void;
    onClose: () => void;
  } = $props();

  let query = $state("");
  let checked = $state<Record<string, boolean>>(
    untrack(() => {
      const saved = visibleModelIds(backendId);
      const next: Record<string, boolean> = {};
      for (const m of models) {
        next[m.id] = saved === null ? true : saved.includes(m.id);
      }
      return next;
    }),
  );

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return models;
    return models.filter((m) => {
      const hay = `${m.label} ${m.note ?? ""} ${m.id}`.toLowerCase();
      return hay.includes(q);
    });
  });

  const selectedIds = $derived(
    models.filter((m) => checked[m.id]).map((m) => m.id),
  );

  const allVisibleSelected = $derived(
    models.length > 0 && selectedIds.length === models.length,
  );

  function selectAll() {
    const next = { ...checked };
    for (const m of models) next[m.id] = true;
    checked = next;
  }

  function selectNone() {
    const next = { ...checked };
    for (const m of models) next[m.id] = false;
    checked = next;
  }

  function save() {
    // Todos marcados = sin filtro (mostrar el catálogo completo).
    onSave(allVisibleSelected ? [] : selectedIds);
  }
</script>

<ModalShell
  title="Modelos visibles"
  subtitle={`Elige cuáles aparecen en el selector de ${backendLabel}.`}
  size="md"
  {onClose}
>
  <div class="amm">
    <p class="rb-hint">
      Aplica a Cursor y OpenCode. Si no eliges ninguno, o los marcas todos, se
      muestran todos los modelos del proveedor.
    </p>

    <input
      class="rb-field amm-search"
      type="search"
      placeholder="Buscar modelo…"
      bind:value={query}
      aria-label="Buscar modelo"
    />

    <div class="amm-list" role="group" aria-label="Modelos">
      {#each filtered as m (m.id)}
        <label class="rb-check amm-row">
          <input type="checkbox" bind:checked={checked[m.id]} />
          <span class="amm-meta">
            <span class="amm-label">{m.label}</span>
            {#if m.note}
              <span class="amm-note">{m.note}</span>
            {:else if m.id !== m.label}
              <span class="amm-note">{m.id}</span>
            {/if}
          </span>
        </label>
      {:else}
        <p class="rb-hint amm-empty">Sin coincidencias</p>
      {/each}
    </div>
  </div>

  {#snippet actions()}
    <button type="button" class="rb-btn rb-btn-ghost" onclick={selectAll}>
      Todos
    </button>
    <button type="button" class="rb-btn rb-btn-ghost" onclick={selectNone}>
      Ninguno
    </button>
    <button type="button" class="rb-btn rb-btn-ghost" onclick={onClose}>
      Cancelar
    </button>
    <button type="button" class="rb-btn rb-btn-primary" onclick={save}>
      Guardar
    </button>
  {/snippet}
</ModalShell>

<style>
  .amm {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    min-height: 0;
  }

  .amm-search {
    flex-shrink: 0;
  }

  .amm-list {
    display: flex;
    max-height: min(22rem, 50vh);
    flex-direction: column;
    gap: 0.15rem;
    overflow-x: hidden;
    overflow-y: auto;
    border: 1px solid var(--rb-border);
    border-radius: 0.6rem;
    padding: 0.35rem;
  }

  .amm-row {
    align-items: flex-start;
    gap: 0.55rem;
    border-radius: 0.4rem;
    padding: 0.4rem 0.45rem;
  }
  .amm-row:hover {
    background: color-mix(in srgb, var(--rb-border) 55%, transparent);
  }

  .amm-meta {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.1rem;
  }

  .amm-label {
    color: var(--rb-text, inherit);
    font-size: 0.8125rem;
    line-height: 1.3;
    word-break: break-word;
  }

  .amm-note {
    color: var(--rb-muted, #8d827a);
    font-size: 0.6875rem;
    line-height: 1.3;
    word-break: break-word;
  }

  .amm-empty {
    margin: 0;
    padding: 0.6rem 0.4rem;
  }
</style>
