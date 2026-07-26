<script lang="ts">
  /**
   * El bicho naranja de Claude Code, en pixel art.
   *
   * Se dibuja con rectángulos sobre una rejilla en vez de incrustar el archivo
   * de Anthropic: no hay que redistribuir un recurso ajeno, escala sin
   * emborronarse a cualquier tamaño y hereda el color por `currentColor`, que
   * es lo que permite apagarlo cuando el backend no está disponible.
   *
   * Identifica al backend, no a Atic. Cuando el agente sea Codex o Gemini, en
   * su lugar irá la marca que corresponda.
   */
  let {
    size = 32,
    title = "Claude Code",
  }: { size?: number; title?: string } = $props();

  /**
   * La rejilla, fila por fila: cada `1` es un píxel encendido.
   *
   * Cuerpo redondeado con dos ojos huecos y cuatro patas. Mantenerlo como
   * datos y no como un `path` cerrado hace que retocar la figura sea mover
   * unos por la rejilla, en vez de recalcular coordenadas a mano.
   */
  const GRID = [
    "0011111100",
    "0111111110",
    "1111111111",
    "1100110011",
    "1100110011",
    "1111111111",
    "1111111111",
    "1111111111",
    "0110110110",
    "0110000110",
  ];

  const cells = GRID.flatMap((row, y) =>
    [...row].map((on, x) => ({ on: on === "1", x, y })),
  ).filter((c) => c.on);
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 10 10"
  role="img"
  aria-label={title}
  shape-rendering="crispEdges"
>
  {#each cells as c (`${c.x}-${c.y}`)}
    <rect x={c.x} y={c.y} width="1" height="1" fill="currentColor" />
  {/each}
</svg>
