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
   *     Tiene que ser `drop-shadow` (sigue el alpha del path): un `box-shadow`
   *     del bounding box deja un rectángulo gris encima de los floats.
   *
   * Lo que NO va acá: contenido. El texto y los iconos viven en `Ink`, encima y
   * con la misma geometría.
   */
  import type { LiquidPath } from "./contour";
  import type { Shape } from "./sdf";
  import { BLEND, CELL, SMOOTH } from "./constants";
  import { PathTracer } from "./trace";

  let {
    shapes,
    blend = BLEND,
    cell = CELL,
    smooth = SMOOTH,
    color = "var(--skin)",
    shadow = "var(--shadow-goo)",
    breathe = false,
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
    /** Un solo pulso de brillo: la gota está viva (grabar / dictar). */
    breathe?: boolean;
    /**
     * Lo que se acaba de trazar, con su costo.
     *
     * Existe para que el banco de pruebas mida **lo que producción dibuja** en
     * vez de calcularlo por su cuenta: dos cálculos por cuadro duplicarían el
     * costo y falsearían justamente la medición que se está tomando.
     */
    onPath?: (path: LiquidPath, ms: number) => void;
  } = $props();

  /**
   * Aire alrededor del path para que la sombra quede DENTRO del SVG.
   *
   * Con el SVG del tamaño justo, el `drop-shadow` pintaba fuera de su caja
   * (`overflow: visible`) y WebKit no repinta ese sobrante cuando la forma se
   * achica: en macOS quedaba una sombra fantasma de cada tamaño del notch
   * durante unos segundos. Con el margen, lo que la sombra ensucia es parte
   * de la caja del elemento y se limpia con ella. Cubre `--shadow-goo`
   * (0 10px 22px): desenfoque más desplazamiento, con holgura.
   */
  const SHADOW_PAD = 48;

  const tracer = new PathTracer();
  const traced = $derived.by(() => tracer.next(shapes, { blend, cell, smooth }));
  const path = $derived(traced.path);

  // El par de `performance.now()` cuesta menos que un solo muestreo del campo,
  // así que no hace falta condicionarlo a que alguien esté escuchando.
  $effect(() => onPath?.(traced.path, traced.ms));
</script>

{#if path.d}
  <!--
    Transform y filter en nodos distintos: en WebView2, `drop-shadow` +
    `translate3d` en el mismo elemento infla el hit-test y se come el mouse
    aunque `pointer-events` sea none.
  -->
  <div
    class="skin"
    class:is-breathing={breathe}
    style:left="{path.minX}px"
    style:top="{path.minY}px"
    style:width="{path.width}px"
    style:height="{path.height}px"
    style:transform="translate3d({traced.tx}px, {traced.ty}px, 0)"
    aria-hidden="true"
  >
    <svg
      class="skin-path"
      style:filter="drop-shadow({shadow})"
      style:margin="{-SHADOW_PAD}px"
      width={path.width + SHADOW_PAD * 2}
      height={path.height + SHADOW_PAD * 2}
      viewBox="{path.minX - SHADOW_PAD} {path.minY - SHADOW_PAD} {path.width +
        SHADOW_PAD * 2} {path.height + SHADOW_PAD * 2}"
    >
      <!-- `evenodd` porque los lazos del contorno no salen orientados de forma
           consistente: con la regla por defecto, una isla interior se rellenaría
           en vez de quedar hueca. El stroke del mismo color no es un borde de
           contraste: redondea el aliasing de marching squares (~celda de 6 px).
           Sin filete blanco ni degradé: el notch de Apple es un recorte mate. -->
      <path
        d={path.d}
        fill={color}
        fill-rule="evenodd"
        stroke={color}
        stroke-width="1.25"
        stroke-linejoin="round"
        stroke-linecap="round"
      />
    </svg>
  </div>
{/if}

<style>
  .skin {
    position: absolute;
    overflow: visible;
    contain: layout;
    pointer-events: none;
  }

  .skin-path {
    display: block;
    overflow: visible;
    pointer-events: none;
  }

  .skin.is-breathing {
    animation: skin-breathe 2.4s ease-in-out infinite;
  }

  @keyframes skin-breathe {
    0%,
    100% {
      filter: brightness(1);
    }

    50% {
      filter: brightness(1.08);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .skin.is-breathing {
      animation: none;
    }
  }
</style>
