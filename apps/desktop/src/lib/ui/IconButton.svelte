<script lang="ts">
  /**
   * Un botón que solo es un icono.
   *
   * `label` es obligatoria y no tiene valor por defecto a propósito: un botón
   * sin texto es invisible para un lector de pantalla, y la forma de que eso no
   * se olvide es que el componente no compile sin ella. Sirve además de
   * `title`, así que también aparece al pasar el mouse.
   */
  import type { Snippet } from "svelte";

  type Variant = "ghost" | "soft" | "danger";

  let {
    label,
    variant = "ghost",
    size = "md",
    disabled = false,
    pressed,
    onclick,
    children,
    ...rest
  }: {
    label: string;
    variant?: Variant;
    size?: "sm" | "md";
    disabled?: boolean;
    /** Para botones que conmutan algo: se anuncia como `aria-pressed`. */
    pressed?: boolean;
    onclick?: (event: MouseEvent) => void;
    children: Snippet;
    [key: string]: unknown;
  } = $props();

  const VARIANTS: Record<Variant, string> = {
    ghost: "text-muted hover:bg-surface-2 hover:text-text",
    soft: "bg-surface-2 text-text border border-line hover:bg-elevated",
    danger: "text-muted hover:bg-danger-soft hover:text-danger",
  };

  const SIZES = { sm: "size-6", md: "size-8" };
</script>

<button
  type="button"
  aria-label={label}
  title={label}
  aria-pressed={pressed}
  {disabled}
  {onclick}
  class="relative grid shrink-0 place-items-center rounded-sm
         transition-[color,background-color,border-color,transform]
         duration-(--duration-quick) ease-calm active:scale-[0.96]
         disabled:pointer-events-none disabled:opacity-45
         aria-pressed:bg-surface-2 aria-pressed:text-text
         before:absolute before:content-['']
         {size === 'sm' ? 'before:inset-[-8px]' : 'before:inset-[-4px]'}
         {VARIANTS[variant]} {SIZES[size]}"
  {...rest}
>
  {@render children()}
</button>
