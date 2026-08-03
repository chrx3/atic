<script lang="ts">
  /**
   * Texto de varias líneas, con crecimiento opcional.
   *
   * El autocrecimiento se hace midiendo `scrollHeight` con la altura en `auto`
   * y no con una copia oculta del contenido: es un reflow por tecla, pero el
   * compositor de agentes y el bloc son campos donde se escribe despacio, y la
   * alternativa —duplicar el nodo— se desincroniza en cuanto cambia una fuente.
   */
  let {
    value = $bindable(""),
    rows = 3,
    autogrow = false,
    maxRows = 12,
    invalid = false,
    ...rest
  }: {
    value?: string;
    rows?: number;
    autogrow?: boolean;
    maxRows?: number;
    invalid?: boolean;
    [key: string]: unknown;
  } = $props();

  let el = $state<HTMLTextAreaElement | null>(null);

  $effect(() => {
    // `value` se lee para que el efecto vuelva a correr en cada tecla.
    void value;
    const node = el;
    if (!node || !autogrow) return;
    node.style.height = "auto";
    const line = Number.parseFloat(getComputedStyle(node).lineHeight) || 18;
    node.style.height = `${Math.min(node.scrollHeight, line * maxRows)}px`;
  });
</script>

<textarea
  bind:this={el}
  bind:value
  {rows}
  aria-invalid={invalid ? "true" : undefined}
  class="w-full resize-y rounded-sm border bg-surface-2 px-2 py-1.5 text-sm text-text
         transition-colors duration-(--duration-quick) ease-calm
         placeholder:text-faint
         disabled:opacity-45
         {invalid ? 'border-danger' : 'border-line focus:border-line-strong'}
         {autogrow ? 'resize-none overflow-hidden' : ''}"
  {...rest}></textarea>
