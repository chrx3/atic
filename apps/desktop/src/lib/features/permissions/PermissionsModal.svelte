<script lang="ts">
  /**
   * Permisos de macOS fuera del primer uso: al arrancar si falta alguno, y
   * desde Ajustes → Acerca de.
   *
   * Se puede cerrar («Continuar»): mientras falte alguno, vuelve en el
   * próximo arranque.
   */
  import { t } from "$domain/i18n.svelte";
  import Button from "$ui/Button.svelte";
  import Modal from "$ui/Modal.svelte";
  import PermissionsList from "./PermissionsList.svelte";

  let { onClose }: { onClose: () => void } = $props();

  let list = $state<PermissionsList | null>(null);
</script>

<Modal
  title={t("permissions.title")}
  subtitle={t("permissions.subtitle")}
  size="md"
  {onClose}
>
  <div class="flex flex-col gap-4">
    <p class="max-w-[60ch] text-sm leading-relaxed text-muted">
      {t("permissions.intro")}
    </p>

    <PermissionsList bind:this={list} />
  </div>

  {#snippet actions()}
    <div class="flex w-full items-center justify-between gap-2">
      <Button variant="ghost" size="sm" onclick={() => void list?.refresh()}>
        {t("permissions.retry")}
      </Button>
      <Button variant="primary" size="sm" onclick={onClose}>
        {t("permissions.done")}
      </Button>
    </div>
  {/snippet}
</Modal>
