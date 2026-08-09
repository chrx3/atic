<script lang="ts">
  /**
   * Hosts SSH para agentes remotos (Ajustes → Agentes).
   *
   * El CRUD vive en SshHostsPanel; acá lo enganchamos al store de config de la
   * ventana principal — el Settings que el usuario abre hoy (no el modal legacy).
   */
  import { config } from "$domain/config.svelte";
  import { toasts } from "$domain/toasts.svelte";
  import SshHostsPanel from "$features/agents/SshHostsPanel.svelte";

  function onToast(msg: string) {
    toasts.push(msg);
  }
</script>

{#if config.current}
  <div class="flex flex-col gap-5">
    <SshHostsPanel bind:config={config.current} {onToast} />
  </div>
{/if}
