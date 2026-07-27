<script lang="ts">
  /**
   * Íconos del compositor de agentes.
   *
   * # Por qué el escudo cambia de forma y no solo de etiqueta
   *
   * El modo de permisos es la única decisión del compositor que puede costar
   * caro, y a 11px el interior de un ícono no se lee — pero **relleno contra
   * contorno sí**. Así que el nivel de guardia va en la silueta: lleno es
   * guardia completa, contorno es parcial, tachado es sin guardia. El ámbar
   * queda reservado para ese último, que es el único que debería dar respeto.
   *
   * Con la etiqueta sola («Preguntar siempre» / «Acceso total») hay que leer
   * para saber en qué modo estás, y leer es justo lo que no se hace antes de
   * apretar Enter.
   */
  import type { ToolKind } from "$lib/types";

  let {
    name,
    size = 11,
  }: {
    name: "shield-manual" | "shield-edits" | "shield-plan" | "shield-open"
      | "folder" | "mic" | "clip" | ToolKind;
    size?: number;
  } = $props();

  const SHIELD = "M12 3 5.6 5.8v5.4c0 4.1 2.6 7.7 6.4 8.8 3.8-1.1 6.4-4.7 6.4-8.8V5.8z";
</script>

{#if name === "shield-manual"}
  <!-- Guardia completa: pregunta por todo. -->
  <svg
    viewBox="0 0 24 24"
    width={size}
    height={size}
    fill="currentColor"
    aria-hidden="true"><path d={SHIELD} /></svg
  >
{:else if name === "shield-edits"}
  <!-- Deja pasar las ediciones: contorno con un punto, algo cruza. -->
  <svg
    viewBox="0 0 24 24"
    width={size}
    height={size}
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linejoin="round"
    aria-hidden="true"
    ><path d={SHIELD} /><circle
      cx="12"
      cy="11.4"
      r="2.1"
      fill="currentColor"
      stroke="none"
    /></svg
  >
{:else if name === "shield-plan"}
  <!-- No ejecuta nada: contorno con una barra, nada cruza. -->
  <svg
    viewBox="0 0 24 24"
    width={size}
    height={size}
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linejoin="round"
    stroke-linecap="round"
    aria-hidden="true"><path d={SHIELD} /><path d="M9 11.4h6" /></svg
  >
{:else if name === "shield-open"}
  <!-- Sin guardia. Es el único que rompe el gris. -->
  <svg
    viewBox="0 0 24 24"
    width={size}
    height={size}
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linejoin="round"
    stroke-linecap="round"
    style="color:#d4a24c"
    aria-hidden="true"><path d={SHIELD} /><path d="M6.6 4.9 17.4 18.6" /></svg
  >
{:else if name === "folder"}
  <svg
    viewBox="0 0 24 24"
    width={size}
    height={size}
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linejoin="round"
    aria-hidden="true"
    ><path
      d="M3 7.5a1 1 0 0 1 1-1h4.6l2 2H20a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z"
    /></svg
  >
{:else if name === "mic"}
  <svg
    viewBox="0 0 24 24"
    width={size}
    height={size}
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linecap="round"
    aria-hidden="true"
    ><path
      d="M12 4a2.5 2.5 0 0 1 2.5 2.5v5a2.5 2.5 0 0 1-5 0v-5A2.5 2.5 0 0 1 12 4z M6 11a6 6 0 0 0 12 0 M12 17v3"
    /></svg
  >
{:else if name === "clip"}
  <svg
    viewBox="0 0 24 24"
    width={size}
    height={size}
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linecap="round"
    aria-hidden="true"><path d="M9 4h6v3H9z M7 5H5v15h14V5h-2" /></svg
  >
{/if}
