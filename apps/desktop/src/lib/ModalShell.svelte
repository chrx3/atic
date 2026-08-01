<script lang="ts">
  import { onMount, tick, type Snippet } from "svelte";

  let {
    title,
    subtitle,
    size = "md",
    dismissible = true,
    embedded = false,
    onClose,
    children,
    actions,
  }: {
    title: string;
    subtitle?: string;
    size?: "sm" | "md" | "lg" | "xl";
    dismissible?: boolean;
    embedded?: boolean;
    onClose: () => void;
    children: Snippet;
    actions?: Snippet;
  } = $props();

  let dialogEl = $state<HTMLDialogElement | undefined>(undefined);

  onMount(() => {
    let cancelled = false;
    void (async () => {
      await tick();
      if (!cancelled && !embedded) dialogEl?.showModal();
    })();
    return () => {
      cancelled = true;
      if (dialogEl?.open) dialogEl.close();
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

{#snippet panel()}
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
{/snippet}

{#if embedded}
  <div class="rb-embedded-modal" aria-label={title}>
    {@render panel()}
  </div>
{:else}
  <dialog
    bind:this={dialogEl}
    class="rb-dialog"
    aria-label={title}
    oncancel={handleCancel}
    onclick={handleBackdrop}
  >
    {@render panel()}
  </dialog>
{/if}

<style>
  .rb-embedded-modal {
    container-type: inline-size;
    container-name: atic-settings;
    display: flex;
    min-width: 0;
    min-height: 0;
    flex: 1;
  }

  .rb-embedded-modal :global(.rb-modal) {
    display: flex;
    width: 100%;
    max-width: none;
    min-height: 0;
    max-height: none;
    flex: 1;
    border-radius: 0;
    box-shadow: none;
  }

  .rb-embedded-modal :global(.rb-modal-header) {
    padding: 0 0 0.8rem;
    background: transparent;
  }

  .rb-embedded-modal :global(.rb-modal-body) {
    display: flex;
    min-height: 0;
    flex: 1;
    overflow: hidden;
    padding: 0;
  }

  .rb-embedded-modal :global(.rb-modal-footer) {
    padding: 0.75rem 0 0;
    background: transparent;
  }

  @container atic-settings (max-width: 36rem) {
    .rb-embedded-modal :global(.rb-settings-layout) {
      flex-direction: column;
    }

    .rb-embedded-modal :global(.rb-settings-sidebar) {
      width: 100%;
      flex-direction: row;
      gap: 0.2rem;
      overflow-x: auto;
      overflow-y: hidden;
      padding: 0.35rem;
      background: var(--rb-bg1);
    }

    .rb-embedded-modal :global(.rb-settings-nav-item) {
      width: auto;
      min-height: 2.25rem;
      flex: 0 0 auto;
      padding-inline: 0.7rem;
      white-space: nowrap;
    }

    .rb-embedded-modal :global(.rb-settings-content) {
      min-height: 0;
    }
  }
</style>
