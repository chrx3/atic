<script lang="ts">
  /**
   * La tinta: el contenido que va encima de una silueta.
   *
   * Existe para que la geometría se escriba una sola vez. La piel y la tinta
   * tienen que coincidir exactamente —es la única forma de que el texto caiga
   * dentro de la forma que lo contiene— y el modo de garantizarlo es que las
   * dos salgan del mismo rectángulo.
   *
   * Sin fondo propio, a propósito: el fondo lo pone la piel. Poner uno acá es
   * cómo aparece un borde recto asomando por debajo de una esquina fundida.
   */
  import type { Snippet } from "svelte";
  import type { Rect } from "./geometry";

  let {
    rect,
    radius,
    children,
    ...rest
  }: {
    rect: Rect;
    /** Solo para recortar lo que se desborde. La forma la dibuja la piel. */
    radius?: number;
    children: Snippet;
    [key: string]: unknown;
  } = $props();
</script>

<div
  class="ink"
  style:left="{rect.x}px"
  style:top="{rect.y}px"
  style:width="{rect.w}px"
  style:height="{rect.h}px"
  style:border-radius={radius === undefined ? undefined : `${radius}px`}
  {...rest}
>
  {@render children()}
</div>

<style>
  .ink {
    position: absolute;
    overflow: hidden;
  }
</style>
