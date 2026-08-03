<script lang="ts">
  /**
   * La envoltura de un control de formulario: etiqueta, pista y error.
   *
   * Reemplaza al trío `.rb-label` + `.rb-field` + `.rb-hint`, que aparece 55
   * veces y cada vez volvía a cablear a mano el `for`/`id` — cuando lo cableaba.
   *
   * El `id` se genera acá y se le pasa al contenido por el snippet, así que la
   * asociación entre etiqueta y control no puede quedar a medias.
   */
  import type { Snippet } from "svelte";

  let {
    label,
    hint,
    error,
    required = false,
    children,
  }: {
    label: string;
    hint?: string;
    /** Si viene, reemplaza a la pista y marca el control como inválido. */
    error?: string;
    required?: boolean;
    children: Snippet<[{ id: string; describedBy: string | undefined }]>;
  } = $props();

  const id = $props.id();
  const hintId = $derived(`${id}-hint`);
  const describedBy = $derived(error || hint ? hintId : undefined);
</script>

<div class="flex flex-col gap-1">
  <label for={id} class="text-xs font-medium text-muted">
    {label}
    {#if required}<span class="text-danger" aria-hidden="true">*</span>{/if}
  </label>

  {@render children({ id, describedBy })}

  {#if error}
    <p id={hintId} class="text-xs text-danger" role="alert">{error}</p>
  {:else if hint}
    <p id={hintId} class="text-xs text-faint">{hint}</p>
  {/if}
</div>
