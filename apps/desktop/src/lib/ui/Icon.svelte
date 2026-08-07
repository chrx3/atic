<script lang="ts">
  /**
   * Icono Lucide vía Morphicons.
   *
   * `icon` estático, o cambio de `icon` / `from`+`to` para morph.
   * Datos desde `lucide` (IconNode), no desde `@lucide/svelte`.
   */
  import { MorphIcon, type MorphIconProps } from "morphicons/svelte";

  let {
    icon,
    from,
    to,
    progress,
    spring = "snappy",
    reducedMotion = "user",
    size = 16,
    strokeWidth = 1.75,
    absoluteStrokeWidth,
    color,
    label,
    class: className,
    ...rest
  }: MorphIconProps = $props();
</script>

<MorphIcon
  {icon}
  {from}
  {to}
  {progress}
  {spring}
  {reducedMotion}
  {size}
  {strokeWidth}
  {absoluteStrokeWidth}
  {color}
  {label}
  class={className}
  {...rest}
/>
