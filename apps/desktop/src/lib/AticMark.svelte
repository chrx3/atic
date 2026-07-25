<script lang="ts">
  /**
   * Marca de Atic: la 'a' de caja baja construida sobre la misma retícula de
   * 24 que los iconos. Un círculo (ojo) y una asta recta, sin tipografía de
   * sistema, para que escale igual que el resto del sistema.
   */

  let {
    size = 24,
    strokeWidth = 1.25,
  }: {
    size?: number;
    strokeWidth?: number;
  } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="butt"
  aria-hidden="true"
>
  <!-- Tinta centrada en la retícula: ojo x 6.5→17.5, asta tangente en 17.5,
       ambos de 11 de alto. El centro geométrico cae en (12, 12). -->
  <circle cx="12" cy="12" r="5.5" />
  <path d="M17.5 6.5V17.5" />
</svg>
