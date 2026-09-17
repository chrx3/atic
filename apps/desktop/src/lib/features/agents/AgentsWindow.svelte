<script lang="ts">
  /**
   * Ventana dedicada de consolas de agentes: solo terminales.
   *
   * Es una ventana OS normal (marco, Mission Control, ⌘Tab), no un float del
   * overlay: sirve para trabajar largo con varias consolas sin la pill de por
   * medio. El lanzador es el mismo componente del float; el traspaso entre
   * ventanas mueve las PTY vivas con su scrollback.
   *
   * Perfil webview propio: tema e idioma llegan por eventos (el `+layout` los
   * aplica), igual que en el overlay.
   */
  import { onMount } from "svelte";
  import AgentLauncher from "$features/agents/AgentLauncher.svelte";
  import ToastStack from "$ui/ToastStack.svelte";
  import { toasts } from "$domain/toasts.svelte";
  import { onAgentsTransfer } from "$ipc/agents";
  import {
    transferInbox,
    AGENTS_WINDOW_LABEL,
    type TransferPayload,
  } from "$features/agents/consoleTransfer.svelte";

  /** Baja del oyente de mudanzas (se arma en el onMount). */
  let transferUnlisten: (() => void) | null = null;

  onMount(() => {
    // La ventana vive aunque no tenga consolas: escucha siempre para adoptar
    // lo que el float mude, aunque nazca por el propio traspaso.
    void onAgentsTransfer((raw) => {
      let payload: TransferPayload;
      try {
        payload = JSON.parse(raw) as TransferPayload;
      } catch {
        return;
      }
      if (!payload || payload.to !== AGENTS_WINDOW_LABEL) return;
      transferInbox.offer(payload);
    }).then((unlisten) => {
      transferUnlisten = unlisten;
    });
    return () => {
      transferUnlisten?.();
    };
  });
</script>

<div class="agents-window">
  <!-- `shown` siempre: acá no hay float que abrir; es la ventana la vista. -->
  <AgentLauncher shown />
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
