<script lang="ts">
  /**
   * Los ajustes de agentes, como modal de la ventana de agentes.
   *
   * Esta ventana tiene su propio perfil de webview y no carga la config al
   * arrancar: se relee al abrir. Así además no se guarda encima de lo que se
   * haya cambiado desde la ventana principal mientras esta estaba abierta.
   */
  import { onMount } from "svelte";
  import Modal from "$ui/Modal.svelte";
  import { config } from "$domain/config.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import { t } from "$domain/i18n.svelte";
  import AgentSettingsPanel from "$features/settings/agents/AgentSettingsPanel.svelte";

  let { onClose }: { onClose: () => void } = $props();

  let ready = $state(false);

  onMount(() => {
    void config
      .hydrate()
      .then(() => (ready = true))
      .catch((err) => {
        toastError(err);
        onClose();
      });
  });
</script>

<Modal
  title={t("settings.agents.modalTitle")}
  size="lg"
  fill
  scrollBody={false}
  {onClose}
>
  <div class="-mx-4 -my-3 flex h-full min-h-0 flex-1 flex-col overflow-hidden">
    {#if ready}
      <AgentSettingsPanel layout="nav" />
    {:else}
      <div class="flex h-full w-full items-center justify-center text-sm text-muted">
        {t("settings.loading")}
      </div>
    {/if}
  </div>
</Modal>
