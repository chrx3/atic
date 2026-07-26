<script lang="ts">
  /**
   * Clawd, el cangrejo de Claude Code, en pixel art.
   *
   * # De dónde sale la figura
   *
   * De la ASCII que el propio CLI imprime al arrancar, que son tres líneas de
   * bloques de medio carácter:
   *
   * ```text
   * ▐▛███▜▌
   * ▜█████▛▘
   * ▘▘  ▝▝
   * ```
   *
   * Cada carácter de esos ocupa una celda de 2×2 subpíxeles (`▐` es la mitad
   * derecha, `▛` son tres cuadrantes menos el inferior derecho, `▘` es solo el
   * superior izquierdo…), así que tres filas de texto son SEIS filas de
   * píxeles. Decodificarlas da la rejilla de abajo.
   *
   * La primera versión de esto me la inventé de memoria y le puse dos ojos:
   * parecía una calavera. El bicho real no tiene ojos — los huecos están en los
   * hombros, no en la cara — y esa es justo la diferencia entre las dos
   * lecturas.
   *
   * Se dibuja en vez de incrustar el archivo de Anthropic: no hay que
   * redistribuir un recurso ajeno, escala sin emborronarse y hereda el color
   * por `currentColor`, que es lo que permite apagarlo cuando el backend no
   * está disponible.
   *
   * Identifica al BACKEND, no a Atic. Con Codex o Gemini irá la marca que
   * corresponda. Su color oficial es `#da7756`, el mismo acento de la consola.
   */
  let {
    size = 32,
    title = "Claude Code",
  }: { size?: number; title?: string } = $props();

  /**
   * La rejilla ya decodificada, 16×5. Cada `1` es un píxel encendido.
   *
   * Filas 0-1: cuerpo de arriba, con las muescas de los hombros.
   * Filas 2-3: cuerpo de abajo, más ancho — las pinzas sobresalen.
   * Fila 4: las cuatro patas.
   */
  const GRID = [
    "0011111111111100",
    "0011011111101100",
    "1111111111111110",
    "0111111111111000",
    "0001010001010000",
  ];

  const cells = GRID.flatMap((row, y) =>
    [...row].map((on, x) => ({ on: on === "1", x, y })),
  ).filter((c) => c.on);
</script>

<!--
  Píxeles NO cuadrados: cada uno es el doble de alto que de ancho.

  La rejilla salió de bloques de medio carácter de una terminal, y una celda de
  terminal mide ~1:2. Al partirla en cuadrantes, cada subpíxel queda de
  1 ancho × 2 alto. Dibujándolos cuadrados —que fue lo primero que hice— el
  bicho sale aplastado por arriba y por abajo, que es exactamente como se veía.
-->
<svg
  width={size}
  height={(size * 10) / 16}
  viewBox="0 0 16 10"
  role="img"
  aria-label={title}
  shape-rendering="crispEdges"
>
  {#each cells as c (`${c.x}-${c.y}`)}
    <rect x={c.x} y={c.y * 2} width="1" height="2" fill="currentColor" />
  {/each}
</svg>
