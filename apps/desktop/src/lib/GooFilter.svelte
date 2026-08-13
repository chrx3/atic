<script module lang="ts">
  /**
   * Viscosidad por defecto de todo el sistema líquido, en px.
   *
   * El alcance —el hueco máximo que el cuello todavía cruza— es `1.72·σ`, o
   * sea ~10.3 px. De ahí sale, por ejemplo, que el cuello de la burbuja
   * necesite dos gotas intermedias: la separación real entre la pill y el
   * globo es 10, y con σ = 5 el filtro se quedaba corto.
   */
  export const GOO_SIGMA = 6;

  /**
   * Cuánto engorda la silueta el endurecido del filtro, por lado.
   *
   * El umbral es 7/18 ≈ 0.389, y el alfa difuminado vale eso a 0.28σ POR FUERA
   * del borde original. Quien dibuje una forma de tamaño exacto tiene que
   * restarle `GOO_GROW * 2` antes, o saldrá ~3.4 px más grande de lo que pide
   * su geometría.
   *
   * El mismo número está en `app.css` como `--goo-grow`, porque desde CSS no
   * se puede importar. Si cambia `GOO_SIGMA`, cambian los dos.
   */
  export const GOO_GROW = 0.28 * GOO_SIGMA;

  /** Tamaño a dibujar para que, pasado el filtro, la forma mida `size`. */
  export function preFilter(size: number): number {
    return size - GOO_GROW * 2;
  }
</script>

<script lang="ts">
  /**
   * Filtro de fusión: dos formas cercanas se unen con un cuello cóncavo en vez
   * de solaparse, y al separarse el cuello se afina hasta cortarse.
   *
   * Cómo funciona: se difumina el grupo entero y se vuelve a endurecer el alfa
   * con `a' = 18·a − 7`. Entre dos formas cercanas los halos SE SUMAN, pasan el
   * umbral `7/18`, y el hueco se rellena. `feComposite atop` devuelve el
   * gráfico nítido encima para que el endurecido no coma los bordes.
   *
   * DOS CONSECUENCIAS que mandan sobre el markup y no son opcionales:
   *
   *   - Solo se funde lo que está DENTRO del elemento filtrado, y todo lo que
   *     esté ahí adentro pasa por el difuminado. El texto y los iconos tienen
   *     que vivir en otra capa, encima.
   *   - Las formas que se funden van del MISMO color: el cuello lo pinta el
   *     difuminado, que promedia lo que tenga cerca.
   *
   * El alcance —el hueco máximo que el cuello todavía cruza— sale de
   * `2·Φ(−hueco/2σ) > 7/18`, o sea **1.72·σ**. Con σ = 6 son ~10.3 px, y por
   * eso el cuello de la burbuja de agentes necesita dos gotas intermedias: la
   * separación real ahí es de 10.
   */

  let {
    id,
    /** Viscosidad, en px. Bajo = juntas nítidas, alto = gotas gomosas. */
    sigma = GOO_SIGMA,
  }: {
    id: string;
    sigma?: number;
  } = $props();
</script>

<!-- Región generosa: el cuello vive FUERA de la caja de las formas, y con la
     región por defecto se recorta en cuanto la viscosidad sube. -->
<svg class="goo-defs" width="0" height="0" aria-hidden="true" focusable="false">
  <defs>
    <filter
      {id}
      x="-50%"
      y="-50%"
      width="200%"
      height="200%"
      color-interpolation-filters="sRGB"
    >
      <feGaussianBlur in="SourceGraphic" stdDeviation={sigma} result="blur" />
      <feColorMatrix
        in="blur"
        type="matrix"
        values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 18 -7"
        result="goo"
      />
      <feComposite in="SourceGraphic" in2="goo" operator="atop" />
    </filter>
  </defs>
</svg>

<style>
  .goo-defs {
    position: absolute;
    width: 0;
    height: 0;
    overflow: hidden;
  }
</style>
