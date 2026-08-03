<script lang="ts">
  /**
   * Una preferencia: qué es a la izquierda, con qué se cambia a la derecha.
   *
   * El control va en un snippet y no como hijo suelto porque la etiqueta tiene
   * que poder asociarse con él. Cuando el control trae su propia etiqueta —un
   * `Switch`— se usa `bare`, o el lector de pantalla anuncia el nombre dos
   * veces.
   */
  import type { Snippet } from "svelte";

  let {
    label,
    hint,
    bare = false,
    control,
  }: {
    label?: string;
    hint?: string;
    /** El control ya se describe solo: no se dibuja etiqueta. */
    bare?: boolean;
    control: Snippet<[{ id: string; describedBy: string | undefined }]>;
  } = $props();

  const id = $props.id();
  const hintId = $derived(`${id}-hint`);
</script>

<div class="flex items-start justify-between gap-4 py-2">
  {#if !bare}
    <div class="flex min-w-0 flex-1 flex-col gap-0.5">
      <label for={id} class="text-sm text-text">{label}</label>
      {#if hint}
        <p id={hintId} class="text-xs text-faint">{hint}</p>
      {/if}
    </div>
    <div class="w-52 shrink-0">
      {@render control({ id, describedBy: hint ? hintId : undefined })}
    </div>
  {:else}
    <div class="min-w-0 flex-1">
      {@render control({ id, describedBy: undefined })}
    </div>
  {/if}
</div>
