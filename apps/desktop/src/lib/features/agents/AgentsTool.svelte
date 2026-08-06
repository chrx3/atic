<script lang="ts">
  /**
   * Agentes en la ventana principal: solo demo visual.
   * El sistema real (sesiones, IPC, backends) se reconstruye aparte.
   */
  import AgentsDemo from "./AgentsDemo.svelte";
</script>

<div class="host">
  <AgentsDemo variant="panel" />
</div>

<style>
  .host {
    height: 100%;
    min-height: 0;
  }
</style>
