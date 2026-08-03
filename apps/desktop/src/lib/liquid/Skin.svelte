<script lang="ts">
  /**
   * La piel: todas las siluetas de un grupo, fundidas en una sola forma.
   *
   * Recibe **solo números**. Eso no es minimalismo: es lo que hace que cambiar
   * de renderizador —del campo de distancia a otra cosa, si alguna vez hiciera
   * falta— sea un cambio de este archivo y de ninguno más.
   *
   * Dos reglas del sistema líquido que este componente hace cumplir por
   * construcción, y que antes había que recordar en cada sitio:
   *
   *   - **Todo lo que se funde va del mismo color.** Hay un solo `color`, no
   *     uno por forma: el cuello lo pinta la unión, y dos tonos dejarían una
   *     franja sucia justo en la junta.
   *   - **La sombra va después de la fusión.** Va sobre el path ya trazado, no
   *     por forma; una sombra por silueta se vería por dentro de la unión.
   *
   * Lo que NO va acá: contenido. El texto y los iconos viven en `Ink`, encima y
   * con la misma geometría.
   */
  import { fieldToPath, type LiquidPath } from "./contour";
  import { Field, type Shape } from "./sdf";
  import { BLEND, CELL, SMOOTH } from "./constants";

  let {
    shapes,
    blend = BLEND,
    cell = CELL,
    smooth = SMOOTH,
    color = "var(--skin)",
    shadow = "var(--shadow-goo)",
    onPath,
  }: {
    shapes: Shape[];
    /** Cuánto se mezclan. El alcance de la unión es la mitad de esto. */
    blend?: number;
    /** Lado de la celda de muestreo. Nada más fino que esto se ve. */
    cell?: number;
    smooth?: number;
    color?: string;
    shadow?: string;
    /**
     * Lo que se acaba de trazar, con su costo.
     *
     * Existe para que el banco de pruebas mida **lo que producción dibuja** en
     * vez de calcularlo por su cuenta: dos cálculos por cuadro duplicarían el
     * costo y falsearían justamente la medición que se está tomando.
     */
    onPath?: (path: LiquidPath, ms: number) => void;
  } = $props();

  const traced = $derived.by(() => {
    const t0 = performance.now();
    const path = fieldToPath(new Field(shapes, blend), { cell, smooth });
    return { path, ms: Math.round((performance.now() - t0) * 100) / 100 };
  });

  const path = $derived(traced.path);

  // El par de `performance.now()` cuesta menos que un solo muestreo del campo,
  // así que no hace falta condicionarlo a que alguien esté escuchando.
  $effect(() => onPath?.(traced.path, traced.ms));
</script>

{#if path.d}
  <svg
    class="skin"
    style:left="{path.minX}px"
    style:top="{path.minY}px"
    style:filter="drop-shadow({shadow})"
    width={path.width}
    height={path.height}
    viewBox="{path.minX} {path.minY} {path.width} {path.height}"
    aria-hidden="true"
  >
    <!-- `evenodd` porque los lazos del contorno no salen orientados de forma
         consistente: con la regla por defecto, una isla interior se rellenaría
         en vez de quedar hueca. -->
    <path d={path.d} fill={color} fill-rule="evenodd" />
  </svg>
{/if}

<style>
  .skin {
    position: absolute;
    overflow: visible;

    /* La piel no recibe el mouse: quien lo recibe es la tinta de encima, que
       es donde están los controles de verdad. */
    pointer-events: none;
  }
</style>
