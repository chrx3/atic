<script lang="ts">
  import ModalShell from "$lib/ModalShell.svelte";

  let {
    title = "Confirmar",
    message,
    confirmLabel = "Eliminar",
    cancelLabel = "Cancelar",
    danger = true,
    onConfirm,
    onCancel,
  }: {
    title?: string;
    message: string;
    confirmLabel?: string;
    cancelLabel?: string;
    danger?: boolean;
    onConfirm: () => void | Promise<void>;
    onCancel: () => void;
  } = $props();

  let busy = $state(false);

  async function confirm() {
    if (busy) return;
    busy = true;
    try {
      await onConfirm();
    } finally {
      busy = false;
    }
  }
</script>

<ModalShell {title} size="sm" onClose={onCancel}>
  <p class="text-[0.875rem] leading-relaxed rb-text-muted">{message}</p>

  {#snippet actions()}
      <button class="rb-btn rb-btn-ghost" onclick={onCancel} disabled={busy}>
        {cancelLabel}
      </button>
      <button
        class="rb-btn {danger ? 'rb-btn-danger-solid' : 'rb-btn-primary'}"
        onclick={confirm}
        disabled={busy}
      >
        {busy ? "…" : confirmLabel}
      </button>
  {/snippet}
</ModalShell>
