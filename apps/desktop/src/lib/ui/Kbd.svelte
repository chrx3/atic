<script lang="ts">
  /**
   * Una combinación de teclas.
   *
   * Se dibuja tecla por tecla y no como una cadena: `Ctrl+Shift+K` de corrido
   * se lee como texto, y separado se lee como algo que se aprieta.
   */
  let {
    combo,
    separator = "+",
  }: {
    combo: string;
    /** Con qué viene unida la cadena de entrada. */
    separator?: string;
  } = $props();

  const keys = $derived(
    combo
      .split(separator)
      .map((k) => k.trim())
      .filter(Boolean),
  );
</script>

<span class="inline-flex items-center gap-0.5">
  {#each keys as key, i (i)}
    <kbd
      class="inline-flex h-5 min-w-5 items-center justify-center rounded-xs border border-line bg-surface-2 px-1 font-mono text-xs text-muted"
    >
      {key}
    </kbd>
  {/each}
</span>
