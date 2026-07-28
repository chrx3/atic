<script lang="ts">
  /**
   * La marca del agente, en pixel art.
   *
   * # Por qué las cuatro comparten retícula
   *
   * Son cuatro programas de consola. Dibujarlas todas con celdas de 1×2
   * subpíxeles —los medios bloques de una terminal— lo dice sin explicarlo, y
   * hace que convivan en una misma fila de pestañas sin que ninguna desentone.
   * Mezclar un logo vectorial de marca con el cangrejo en píxeles rompería esa
   * lectura al instante.
   *
   * # De dónde sale cada figura
   *
   * - **Claude**: decodificada de la ASCII que imprime el propio CLI al
   *   arrancar. Sin ojos: los huecos están en los hombros, no en la cara. La
   *   primera versión los tenía y parecía una calavera.
   * - **OpenCode**: el prompt de un shell — chevron y subrayado. Es una TUI.
   * - **Codex**: anillo hexagonal, la reducción honesta del nudo de OpenAI a
   *   esta escala; el nudo real se convierte en barro con 12 píxeles de ancho.
   * - **Cursor**: un puntero. Cursor es, literalmente, el cursor.
   *
   * Se dibujan en vez de incrustar los archivos de cada marca: no hay que
   * redistribuir recursos ajenos, escalan sin emborronarse y heredan el color
   * por `currentColor`, que es lo que permite apagarlas cuando el backend no
   * está instalado y teñirlas con el acento del proveedor.
   */
  let {
    backend = "claude-code",
    size = 32,
    title,
  }: { backend?: string; size?: number; title?: string } = $props();

  /** Cada `1` es un píxel encendido. El ancho lo define la propia figura. */
  const GRIDS: Record<string, string[]> = {
    "claude-code": [
      "0011111111111100",
      "0011011111101100",
      "1111111111111110",
      "0111111111111000",
      "0001010001010000",
    ],
    opencode: [
      "0110000000000000",
      "0011000000000000",
      "0001100000000000",
      "0011000000000000",
      "0110000111111110",
    ],
    codex: [
      "000111100000",
      "011000110000",
      "110000011000",
      "110000011000",
      "011000110000",
      "000111100000",
    ],
    cursor: ["1000000000", "1110000000", "1111100000", "1111111000", "1110110000"],
  };

  const NAMES: Record<string, string> = {
    "claude-code": "Claude Code",
    opencode: "OpenCode",
    codex: "Codex",
    cursor: "Cursor",
  };

  const grid = $derived(GRIDS[backend] ?? GRIDS["claude-code"]);
  const w = $derived(grid[0].length);
  const h = $derived(grid.length);
  const cells = $derived(
    grid
      .flatMap((row, y) => [...row].map((on, x) => ({ on: on === "1", x, y })))
      .filter((c) => c.on),
  );
</script>

<!--
  Píxeles NO cuadrados: cada uno es el doble de alto que de ancho.

  La retícula salió de bloques de medio carácter de una terminal, y una celda de
  terminal mide ~1:2. Al partirla en cuadrantes, cada subpíxel queda de
  1 ancho × 2 alto. Dibujándolos cuadrados el bicho sale aplastado.
-->
<svg
  width={size}
  height={(size * h * 2) / w}
  viewBox="0 0 {w} {h * 2}"
  role="img"
  aria-label={title ?? NAMES[backend] ?? "Agente"}
  shape-rendering="crispEdges"
>
  {#each cells as c (`${c.x}-${c.y}`)}
    <rect x={c.x} y={c.y * 2} width="1" height="2" fill="currentColor" />
  {/each}
</svg>
