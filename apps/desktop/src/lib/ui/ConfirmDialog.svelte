<script lang="ts">
  /**
   * «¿Seguro?». Construido sobre `Modal`, no en paralelo.
   *
   * `dismissible` queda en `true`: cancelar con Esc siempre tiene que poder,
   * porque el camino seguro es no hacer nada. Lo que no se puede es confirmar
   * sin apuntar al botón.
   */
  let {
    title,
    body,
    confirmLabel = "Confirmar",
    cancelLabel = "Cancelar",
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

  import Button from "./Button.svelte";
  import Modal from "./Modal.svelte";
</script>

<Modal {title} size="sm" onClose={onCancel} dismissible={!busy}>
  {#snippet actions()}
    <Button variant="ghost" disabled={busy} onclick={onCancel}>{cancelLabel}</Button>
    <Button
      variant={tone === "danger" ? "danger-solid" : "primary"}
      loading={busy}
      onclick={onConfirm}
    >
      {confirmLabel}
    </Button>
  {/snippet}
  <p class="text-sm text-muted">{body}</p>
</Modal>
