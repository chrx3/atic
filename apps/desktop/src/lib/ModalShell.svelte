<script lang="ts">
  import { onMount, tick, type Snippet } from "svelte";

  let {
    title,
    subtitle,
    size = "md",
    dismissible = true,
    onClose,
    children,
    actions,
  }: {
    title: string;
    subtitle?: string;
    size?: "sm" | "md" | "lg" | "xl";
    dismissible?: boolean;
    onClose: () => void;
    children: Snippet;
    actions?: Snippet;
  } = $props();

  let dialogEl: HTMLDialogElement;

  onMount(() => {
    let cancelled = false;
    void (async () => {
      await tick();
      if (!cancelled) dialogEl.showModal();
    })();
    return () => {
      cancelled = true;
      if (dialogEl.open) dialogEl.close();
    };
  });

  function requestClose() {
    if (dismissible) onClose();
  }

  function handleCancel(event: Event) {
    event.preventDefault();
    requestClose();
  }

  function handleBackdrop(event: MouseEvent) {
    if (event.target === dialogEl) requestClose();
  }
</script>

<dialog
  bind:this={dialogEl}
  class="rb-dialog"
  aria-label={title}
  oncancel={handleCancel}
  onclick={handleBackdrop}
>
  <section class="rb-modal rb-modal-{size}">
    <header class="rb-modal-header">
      <div class="min-w-0">
        <h2 class="rb-modal-title">{title}</h2>
        {#if subtitle}
          <p class="rb-modal-subtitle">{subtitle}</p>
        {/if}
      </div>
      {#if dismissible}
        <button class="rb-icon-btn" onclick={onClose} aria-label="Cerrar">
          <span aria-hidden="true">×</span>
        </button>
      {/if}
    </header>

    <div class="rb-modal-body">
      {@render children()}
    </div>

    {#if actions}
      <footer class="rb-modal-footer">
        {@render actions()}
      </footer>
    {/if}
  </section>
</dialog>
