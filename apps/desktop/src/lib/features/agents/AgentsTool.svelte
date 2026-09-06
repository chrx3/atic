<script lang="ts">
  /**
   * Agentes en la ventana principal: lanzador de consolas CLI.
   * El wrapper de chat (AgentsDemo) queda fuera del camino mientras la
   * feature se reactiva; este componente es la puerta.
   */
  import AgentLauncher from "./AgentLauncher.svelte";
  import HubSessions from "./HubSessions.svelte";
  import { agents } from "$lib/agentSessions.svelte";

  /**
   * Escuchar los deltas en ESTA ventana.
   *
   * El store es por ventana: que la pill escuche no le sirve a la principal.
   * Sin esto, `agents.sessions` está vacío acá y las delegaciones no se ven.
   * `init` es idempotente, así que llamarlo al montar no pisa a nadie.
   */
  void agents.init();

  /**
   * Las sesiones que un agente le pidió a otro. Solo esas: las que abre el
   * usuario ya tienen su consola, y una lista con todo repetiría lo que el
   * lanzador muestra al lado.
   */
  const delegadas = $derived(agents.sessions.filter((s) => !!s.parent));
</script>

<div class="host">
  <AgentLauncher />
  {#if delegadas.length > 0}
    <div class="delegaciones">
      <HubSessions />
    </div>
  {/if}
</div>

<style>
  .host {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  /* Mitad de la altura como techo: el lanzador no se puede quedar sin sitio
     porque alguien delegó cinco veces. */
  .delegaciones {
    flex: 1 1 auto;
    min-height: 0;
    max-height: 50%;
    padding-top: 10px;
    border-top: 1px solid color-mix(in sRGB, var(--rb-text) 10%, transparent);
  }
</style>
