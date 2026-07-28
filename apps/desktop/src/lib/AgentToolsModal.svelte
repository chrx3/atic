<script lang="ts">
  /**
   * MCP y skills del agente, solo lectura.
   *
   * Atic no edita ni sustituye lo que viene del CLI / proyecto: acá se mira
   * qué reportó la sesión y qué hay en disco.
   */
  import ModalShell from "$lib/ModalShell.svelte";

  let {
    mcpServers,
    skills,
    hasSession,
    onClose,
  }: {
    mcpServers: { name: string; status: string }[];
    skills: {
      name: string;
      description: string;
      scope: string;
      path?: string;
    }[];
    hasSession: boolean;
    onClose: () => void;
  } = $props();

  function scopeLabel(scope: string): string {
    if (scope === "user") return "usuario";
    if (scope === "project") return "proyecto";
    return scope;
  }
</script>

<ModalShell title="Herramientas del agente" size="lg" {onClose}>
  <div class="tools">
    <section class="sec">
      <h3 class="sec-h">MCP</h3>
      {#if !hasSession}
        <p class="hint">
          Se listan al abrir una sesión con este agente.
        </p>
      {:else if mcpServers.length === 0}
        <p class="hint">Este agente no reportó servidores MCP.</p>
      {:else}
        <ul class="list">
          {#each mcpServers as s (s.name)}
            <li class="row">
              <span class="name">{s.name}</span>
              <span class="status">{s.status}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <section class="sec">
      <h3 class="sec-h">Skills</h3>
      {#if skills.length === 0}
        <p class="hint">No se encontraron skills en disco.</p>
      {:else}
        <ul class="list">
          {#each skills as sk (sk.path ?? sk.name)}
            <li class="skill">
              <div class="skill-top">
                <span class="name">{sk.name}</span>
                <span class="badge">{scopeLabel(sk.scope)}</span>
              </div>
              {#if sk.description}
                <p class="desc">{sk.description}</p>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <p class="foot-hint">
      Los MCP y skills vienen de tu CLI / proyecto; Atic no los reemplaza.
    </p>
  </div>

  {#snippet actions()}
    <button type="button" class="rb-btn rb-btn-ghost" onclick={onClose}>
      Cerrar
    </button>
  {/snippet}
</ModalShell>

<style>
  .tools {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .sec {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
  }

  .sec-h {
    margin: 0;
    color: var(--rb-muted, #8d827a);
    font-size: 0.6875rem;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .hint {
    margin: 0;
    color: var(--rb-muted, #8d827a);
    font-size: 0.8125rem;
    line-height: 1.45;
  }

  .list {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .row {
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
    border: 1px solid var(--rb-border, #332e2b);
    border-radius: 0.5rem;
    padding: 0.45rem 0.6rem;
  }

  .name {
    min-width: 0;
    flex: 1;
    color: var(--rb-text, #e7e2dd);
    font-size: 0.8125rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .status {
    flex-shrink: 0;
    color: var(--rb-muted, #8d827a);
    font-size: 0.6875rem;
  }

  .skill {
    border: 1px solid var(--rb-border, #332e2b);
    border-radius: 0.5rem;
    padding: 0.5rem 0.6rem;
  }

  .skill-top {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .badge {
    flex-shrink: 0;
    border: 1px solid var(--rb-border, #332e2b);
    border-radius: 999px;
    padding: 0.05rem 0.4rem;
    color: var(--rb-muted, #8d827a);
    font-size: 0.625rem;
  }

  .desc {
    margin: 0.25rem 0 0;
    color: var(--rb-muted, #8d827a);
    font-size: 0.75rem;
    line-height: 1.4;
  }

  .foot-hint {
    margin: 0;
    color: var(--rb-muted, #8d827a);
    font-size: 0.75rem;
    line-height: 1.4;
  }
</style>
