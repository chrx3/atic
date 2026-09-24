<script lang="ts">
  /**
   * La ventana de agentes: una ventana OS normal (marco, Alt+Tab) donde se
   * trabaja con ellos. El contenido es la pizarra de consolas
   * (`AgentsBoard`); acá solo va lo que es de la ventana, como los avisos.
   *
   * Perfil webview propio: tema e idioma llegan por eventos (el `+layout` los
   * aplica), igual que en el overlay.
   */
  import AgentsBoard from "$features/agents/AgentsBoard.svelte";
  import ToastStack from "$ui/ToastStack.svelte";
  import { toasts } from "$domain/toasts.svelte";
</script>

<div class="agents-window">
  <AgentsBoard />
  <ToastStack items={toasts.items} onDismiss={(id) => toasts.dismiss(id)} />
</div>

<style>
  .agents-window {
    display: flex;
    height: 100dvh;
    min-height: 0;
    flex-direction: column;
    background: var(--bg);
    color: var(--text);
  }
</style>
