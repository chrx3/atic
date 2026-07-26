<script lang="ts">
  /**
   * La página de agentes en la ventana principal: un acceso, no una consola.
   *
   * La consola vive en su propia ventana porque hace falta tenerla abierta al
   * lado de lo que estés haciendo. Duplicarla acá daría dos vistas del mismo
   * proceso compitiendo por ser «la» sesión.
   */
  import { showAgentsWindow } from "$lib/api";
  import { agents } from "$lib/agentSessions.svelte";
  import { onMount } from "svelte";

  onMount(() => {
    void agents.init();
  });
</script>

<div class="at">
  <p class="at-copy">
    Atic no reemplaza a tu agente: lanza el que ya tienes instalado, con tu
    sesión, tus herramientas y tus skills. Solo le pone una interfaz con
    permisos, contexto a la vista y avisos en la pill.
  </p>

  {#if agents.sessions.length > 0}
    <ul class="at-list">
      {#each agents.sessions as s (s.id)}
        <li class="at-item">
          <span class="at-name">{s.backendName}</span>
          <span class="at-state">
            {#if s.pending.length > 0}
              espera tu permiso
            {:else if s.status === "working"}
              trabajando
            {:else if s.unread > 0}
              {s.unread} sin leer
            {:else}
              lista
            {/if}
          </span>
        </li>
      {/each}
    </ul>
  {/if}

  <button
    type="button"
    class="rb-btn rb-btn-primary"
    onclick={() => void showAgentsWindow()}
  >
    Abrir la consola
  </button>
</div>

<style>
  .at {
    display: flex;
    max-width: 32rem;
    flex-direction: column;
    align-items: flex-start;
    gap: 1rem;
  }

  .at-copy {
    margin: 0;
    color: var(--rb-muted);
    font-size: 0.875rem;
    line-height: 1.55;
  }

  .at-list {
    display: flex;
    width: 100%;
    flex-direction: column;
    gap: 0.4rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .at-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    border: 1px solid var(--rb-border);
    border-radius: 0.5rem;
    padding: 0.5rem 0.7rem;
  }

  .at-name {
    font-size: 0.875rem;
  }

  .at-state {
    color: var(--rb-muted);
    font-size: 0.75rem;
  }
</style>
