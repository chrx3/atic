<script lang="ts">
  /** Una entrada de texto. Pensada para vivir dentro de un `<Field>`. */

  let {
    value = $bindable(""),
    type = "text",
    invalid = false,
    mono = false,
    ...rest
  }: {
    value?: string;
    type?: "text" | "email" | "password" | "search" | "url" | "number";
    invalid?: boolean;
    /** Para rutas, claves y todo lo que se compara carácter a carácter. */
    mono?: boolean;
    [key: string]: unknown;
  } = $props();
</script>

<input
  {type}
  bind:value
  aria-invalid={invalid ? "true" : undefined}
  class="h-8 w-full rounded-sm border bg-surface-2 px-2 text-sm text-text
         transition-colors duration-(--duration-quick) ease-calm
         placeholder:text-faint
         disabled:opacity-45
         {invalid ? 'border-danger' : 'border-line focus:border-line-strong'}
         {mono ? 'font-mono' : ''}"
  {...rest}
/>
