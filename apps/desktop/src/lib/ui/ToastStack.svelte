<script lang="ts">
  /**
   * La pila de avisos efímeros.
   *
   * Recibe la lista por props y no lee el store: así se puede mirar en el
   * kitchen sink con datos inventados, y así se respeta que una primitiva no
   * sabe de dominio. Quien la conecta es la superficie.
   *
   * `aria-live="polite"` y no `assertive`: un aviso de «copiado» no debe
   * interrumpir lo que el lector de pantalla esté diciendo.
   */
  import { fly } from "svelte/transition";
  import IconButton from "./IconButton.svelte";

  let {
    items,
    onDismiss,
  }: {
    items: { id: number; message: string }[];
    onDismiss?: (id: number) => void;
  } = $props();
</script>

<div
  class="pointer-events-none fixed inset-x-0 bottom-3 z-(--z-toast) flex flex-col
         items-center gap-1.5 px-3"
  aria-live="polite"
  aria-atomic="false"
>
  {#each items as toast (toast.id)}
    <div
      class="pointer-events-auto flex max-w-100 items-center gap-2 rounded-sm border
             border-line bg-elevated py-1.5 pr-1.5 pl-3 shadow-pop"
      transition:fly={{ y: 8, duration: 200 }}
    >
      <p class="min-w-0 flex-1 truncate text-sm text-text">{toast.message}</p>
      {#if onDismiss}
        <IconButton label="Descartar" size="sm" onclick={() => onDismiss(toast.id)}>
          <svg
            width="12"
            height="12"
            viewBox="0 0 24 24"
            fill="none"
            aria-hidden="true"
          >
            <path
              d="M6 6l12 12M18 6L6 18"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
            />
          </svg>
        </IconButton>
      {/if}
    </div>
  {/each}
</div>
