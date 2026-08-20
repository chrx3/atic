<script lang="ts">
  /**
   * La pila de avisos efímeros.
   *
   * Recibe la lista por props y no lee el store: así se puede mirar en el
   * kitchen sink con datos inventados, y así se respeta que una primitiva no
   * sabe de dominio. Quien la conecta es la superficie.
   *
   * `placement="viewport"` (default): `popover="manual"` mete la pila en la
   * top layer del navegador (como `showModal()`). Sin eso, un toast con
   * `z-index` queda debajo de cualquier `<dialog>` modal — p.ej. Ajustes →
   * Probar SSH. Anclado fixed al fondo del viewport.
   *
   * `placement="local"`: absoluto al contenedor (p.ej. el bubble de agentes).
   * Sin popover: en top layer el anclaje sería al viewport entero del overlay
   * transparente, lejos del float.
   *
   * `aria-live="polite"` y no `assertive`: un aviso de «copiado» no debe
   * interrumpir lo que el lector de pantalla esté diciendo.
   */
  import { fly } from "svelte/transition";
  import Icon from "$ui/Icon.svelte";
  import { X } from "$lib/icons";
  import IconButton from "./IconButton.svelte";
  import { t } from "$domain/i18n.svelte";

  let {
    items,
    onDismiss,
    placement = "viewport",
  }: {
    items: { id: number; message: string }[];
    onDismiss?: (id: number) => void;
    placement?: "viewport" | "local";
  } = $props();

  let root = $state<HTMLDivElement | null>(null);
  const usePopover = $derived(placement === "viewport");

  $effect(() => {
    const el = root;
    if (!usePopover || !el || typeof el.showPopover !== "function") return;
    const open = el.matches(":popover-open");
    if (items.length > 0) {
      if (!open) el.showPopover();
    } else if (open) {
      el.hidePopover();
    }
  });
</script>

<!--
  UA popover: margin/inset/border propios. Los anulamos para conservar el
  anclaje fixed bottom + stack centrado.
-->
<div
  bind:this={root}
  popover={usePopover ? "manual" : undefined}
  class="pointer-events-none z-(--z-toast) m-0 max-w-none
         border-0 bg-transparent p-0 text-inherit shadow-none
         flex flex-col items-center gap-1.5 px-3
         {placement === 'local'
           ? 'absolute inset-x-0 bottom-3'
           : 'fixed inset-x-0 bottom-3'}"
  style:inset={usePopover ? "auto 0 0.75rem 0" : undefined}
  style:width={usePopover ? "100%" : undefined}
  style:overflow="visible"
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
        <IconButton label={t("chrome.dismiss")} size="sm" onclick={() => onDismiss(toast.id)}>
          <Icon icon={X} size={12} />
        </IconButton>
      {/if}
    </div>
  {/each}
</div>
