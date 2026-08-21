<script lang="ts">
  /**
   * Agentes en la ventana principal: lanzador de consolas CLI.
   * El wrapper de chat (AgentsDemo) queda fuera del camino mientras la
   * feature se reactiva; este componente es la puerta.
   */
  import AgentLauncher from "./AgentLauncher.svelte";
</script>

<div class="host">
  <AgentLauncher />
</div>

<style>
  .host {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
</style>
