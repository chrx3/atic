<script lang="ts">
  /**
   * El botón. Reemplaza a `.rb-btn` y sus siete modificadores, que es la clase
   * más usada de la app (82 apariciones).
   *
   * En una paleta monocroma `primary` y `accent` eran el mismo botón, así que
   * quedan cinco variantes y cada una dice algo distinto: `primary` es la
   * acción de la pantalla, `soft` es una acción normal, `ghost` es una acción
   * que no debería competir con el contenido, y las dos de peligro se separan
   * en «se puede deshacer» y «no».
   */
  import type { Snippet } from "svelte";

  type Variant = "primary" | "soft" | "ghost" | "danger" | "danger-solid";

  let {
    variant = "soft",
    size = "md",
    type = "button",
    disabled = false,
    loading = false,
    full = false,
    onclick,
    icon,
    children,
    ...rest
  }: {
    variant?: Variant;
    size?: "sm" | "md";
    type?: "button" | "submit" | "reset";
    disabled?: boolean;
    /** Bloquea la acción y lo marca ocupado, sin cambiar el ancho. */
    loading?: boolean;
    full?: boolean;
    onclick?: (event: MouseEvent) => void;
    icon?: Snippet;
    children: Snippet;
    [key: string]: unknown;
  } = $props();

  const VARIANTS: Record<Variant, string> = {
    primary: "bg-accent text-on-accent hover:opacity-90",
    soft: "bg-surface-2 text-text border border-line hover:bg-elevated",
    ghost: "bg-transparent text-muted hover:bg-surface-2 hover:text-text",
    danger: "bg-transparent text-danger border border-line hover:bg-danger-soft",
    "danger-solid": "bg-danger text-on-accent hover:opacity-90",
  };

  const SIZES = {
    sm: "h-6 px-2 text-xs gap-1",
    md: "h-8 px-3 text-sm gap-1.5",
  };

  const off = $derived(disabled || loading);
  const classes = $derived(
    [
      "inline-flex items-center justify-center rounded-sm font-medium",
      "transition-colors duration-(--duration-quick) ease-calm",
      "disabled:pointer-events-none disabled:opacity-45",
      VARIANTS[variant],
      SIZES[size],
      full ? "w-full" : "",
    ].join(" "),
  );
</script>

<!-- Solo `<button>`: no hay una sola navegación en la app —es una SPA de
     ventanas, no de páginas— y un botón que a veces es enlace obliga a razonar
     dos veces sobre foco, teclado y estado deshabilitado en cada uso. -->
<button {type} class={classes} disabled={off} aria-busy={loading} {onclick} {...rest}>
  {#if icon}{@render icon()}{/if}
  {@render children()}
</button>
