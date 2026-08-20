<script lang="ts">
  /**
   * «¿Seguro?». Construido sobre `Modal`, no en paralelo.
   *
   * `dismissible` queda en `true`: cancelar con Esc siempre tiene que poder,
   * porque el camino seguro es no hacer nada. Lo que no se puede es confirmar
   * sin apuntar al botón.
   */
  import Button from "./Button.svelte";
  import Modal from "./Modal.svelte";
  import { t } from "$domain/i18n.svelte";

  let {
    title,
    body,
    confirmLabel,
    cancelLabel,
    tone = "default",
    busy = false,
    onConfirm,
    onCancel,
  }: {
    title: string;
    body: string;
    confirmLabel?: string;
    cancelLabel?: string;
    tone?: "default" | "danger";
    /** Mientras la acción está en vuelo. Bloquea los dos botones. */
    busy?: boolean;
    onConfirm: () => void;
    onCancel: () => void;
  } = $props();

  const confirmText = $derived(confirmLabel ?? t("chrome.confirm"));
  const cancelText = $derived(cancelLabel ?? t("chrome.cancel"));
</script>

<Modal {title} size="sm" onClose={onCancel} dismissible={!busy}>
  {#snippet actions()}
    <Button variant="ghost" disabled={busy} onclick={onCancel}>{cancelText}</Button>
    <Button
      variant={tone === "danger" ? "danger-solid" : "primary"}
      loading={busy}
      onclick={onConfirm}
    >
      {confirmText}
    </Button>
  {/snippet}
  <p class="text-sm text-muted">{body}</p>
</Modal>
