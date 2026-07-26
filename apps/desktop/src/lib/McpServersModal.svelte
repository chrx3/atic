<script lang="ts">
  /**
   * Servidores MCP que Atic le suma al agente.
   *
   * Son herramientas **para el agente**, no para Atic: al arrancar la sesión se
   * pasan con `--mcp-config` y el agente los carga junto a los suyos. Atic solo
   * los guarda y decide cuáles van.
   *
   * El JSON se edita crudo a propósito. Cada servidor documenta su bloque y lo
   * normal es pegarlo tal cual; un formulario con campos fijos (comando, args,
   * env) obligaría a traducir a mano y se quedaría corto con cada variante
   * nueva —HTTP, SSE, headers— que aparezca.
   */
  import { untrack } from "svelte";
  import ModalShell from "$lib/ModalShell.svelte";
  import { getConfig, setConfig } from "$lib/api";
  import type { McpServerConfig } from "$lib/types";

  let {
    servers,
    onSave,
    onClose,
  }: {
    servers: McpServerConfig[];
    onSave: (next: McpServerConfig[]) => void;
    onClose: () => void;
  } = $props();

  const EXAMPLE = `{
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:/ruta"]
}`;

  // Copia editable: se toma el valor de entrada una vez y a partir de ahí manda
  // el borrador. Seguir la prop haría que un guardado externo pisara lo que
  // estás escribiendo.
  let draft = $state<McpServerConfig[]>(
    untrack(() => servers.map((s) => ({ ...s }))),
  );
  let saving = $state(false);
  let error = $state<string | null>(null);

  /** Qué entradas tienen el JSON roto. Se avisa al editar, no al arrancar. */
  const broken = $derived(
    draft
      .map((s, i) => ({ i, ok: isValid(s.json) }))
      .filter((x) => !x.ok)
      .map((x) => x.i),
  );

  function isValid(json: string): boolean {
    if (!json.trim()) return false;
    try {
      const parsed = JSON.parse(json);
      return typeof parsed === "object" && parsed !== null;
    } catch {
      return false;
    }
  }

  function add() {
    draft = [...draft, { name: "", json: EXAMPLE, enabled: true }];
  }

  function remove(index: number) {
    draft = draft.filter((_, i) => i !== index);
  }

  async function save() {
    saving = true;
    error = null;
    try {
      // Se guardan también los que están rotos: perder lo que alguien estaba
      // escribiendo por un JSON a medias sería peor que arrancar sin ellos.
      // Al iniciar sesión se saltan solos.
      const cfg = await getConfig();
      await setConfig({ ...cfg, agent_mcp_servers: JSON.stringify(draft) });
      onSave(draft);
    } catch (err) {
      error = String(err);
    } finally {
      saving = false;
    }
  }
</script>

<ModalShell title="Servidores MCP" size="lg" {onClose}>
  <div class="mcp">
    <p class="rb-hint">
      Le suman herramientas al agente. Pega aquí el bloque JSON que documenta
      cada servidor; se cargan al iniciar una sesión nueva.
    </p>

    {#each draft as server, i (i)}
      <div class="mcp-item" class:is-broken={broken.includes(i)}>
        <div class="mcp-head">
          <input
            class="rb-field mcp-name"
            bind:value={server.name}
            placeholder="nombre"
            aria-label="Nombre del servidor"
          />
          <label class="rb-check mcp-on">
            <input type="checkbox" bind:checked={server.enabled} />
            <span>Activo</span>
          </label>
          <button
            type="button"
            class="rb-btn rb-btn-ghost"
            onclick={() => remove(i)}>Quitar</button
          >
        </div>
        <textarea
          class="rb-field mcp-json"
          bind:value={server.json}
          rows="5"
          spellcheck="false"
          aria-label="Definición JSON"
        ></textarea>
        {#if broken.includes(i)}
          <p class="mcp-warn">
            JSON inválido: este servidor se va a saltar al arrancar.
          </p>
        {/if}
      </div>
    {/each}

    {#if draft.length === 0}
      <p class="rb-hint">
        Todavía no hay ninguno. El agente igual usa los que ya tengas
        configurados en su propio CLI.
      </p>
    {/if}

    <button type="button" class="rb-btn rb-btn-ghost" onclick={add}>
      Añadir servidor
    </button>

    {#if error}
      <p class="mcp-warn" role="alert">{error}</p>
    {/if}
  </div>

  {#snippet actions()}
    <button type="button" class="rb-btn rb-btn-ghost" onclick={onClose}>
      Cancelar
    </button>
    <button
      type="button"
      class="rb-btn rb-btn-primary"
      onclick={() => void save()}
      disabled={saving}
    >
      {saving ? "Guardando…" : "Guardar"}
    </button>
  {/snippet}
</ModalShell>

<style>
  .mcp {
    display: flex;
    flex-direction: column;
    gap: 0.85rem;
  }

  .mcp-item {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    border: 1px solid var(--rb-border);
    border-radius: 0.6rem;
    padding: 0.7rem;
  }

  .mcp-item.is-broken {
    border-color: var(--rb-warn);
  }

  .mcp-head {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }

  .mcp-name {
    min-width: 0;
    flex: 1;
  }

  .mcp-on {
    flex-shrink: 0;
    white-space: nowrap;
  }

  .mcp-json {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.75rem;
    resize: vertical;
  }

  .mcp-warn {
    margin: 0;
    color: var(--rb-warn);
    font-size: 0.75rem;
  }
</style>
