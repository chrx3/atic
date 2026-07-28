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
      | "folder" | "mic" | "clip" | "camera" | "mcp" | "history" | ToolKind;
    size?: number;
  } = $props();

  const SHIELD = "M12 3 5.6 5.8v5.4c0 4.1 2.6 7.7 6.4 8.8 3.8-1.1 6.4-4.7 6.4-8.8V5.8z";

  /**
   * Un trazo por `kind` de ACP.
   *
   * `delete`, `move` y `switch_mode` comparten dibujo con su pariente más
   * cercano en vez de inventarles uno: a 11px la diferencia entre «mover» y
   * «editar» no se lee, y dos íconos casi iguales confunden más que uno solo.
   */
  const KINDS: Record<string, string> = {
    read: "M4 3h8l4 4v14H4z M12 3v4h4",
    edit: "M4 20h16 M6 15l9-9 3 3-9 9H6z",
    delete: "M5 7h14 M9 7V5h6v2 M7 7l1 13h8l1-13",
    move: "M4 20h16 M6 15l9-9 3 3-9 9H6z",
    search: "M11 4a7 7 0 1 1 0 14 7 7 0 0 1 0-14z M16.5 16.5 21 21",
    execute: "M4 5h16v14H4z M7 9l3 3-3 3 M13 15h4",
    think: "M12 3a6 6 0 0 1 4 10.5V17H8v-3.5A6 6 0 0 1 12 3z M9 20h6",
    fetch:
      "M12 3a9 9 0 1 1 0 18 9 9 0 0 1 0-18z M3 12h18 M12 3c3 4 3 14 0 18 M12 3c-3 4-3 14 0 18",
    switch_mode: "M4 8h12l-3-3 M20 16H8l3 3",
    other: "M5 12h14 M12 5v14",
  };
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
{:else if name === "camera"}
  <svg
    viewBox="0 0 24 24"
    width={size}
    height={size}
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
    ><path
      d="M4 8h3l1.5-2h7L17 8h3a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V9a1 1 0 0 1 1-1z"
    /><circle cx="12" cy="13.5" r="3.2" /></svg
  >
{:else if name === "mcp"}
  <svg
    viewBox="0 0 24 24"
    width={size}
    height={size}
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
    ><circle cx="6" cy="12" r="2.2" /><circle cx="18" cy="7" r="2.2" /><circle
      cx="18"
      cy="17"
      r="2.2"
    /><path d="M8.2 12h5.3M15.8 8.2 11.5 11.2M15.8 15.8 11.5 12.8" /></svg
  >
{:else if name === "history"}
  <svg
    viewBox="0 0 24 24"
    width={size}
    height={size}
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
    ><path d="M12 3a9 9 0 1 1-8.5 12 M12 7v5l3.5 2 M3 3v5h5" /></svg
  >
{:else}
  <!-- Lo que queda es un `kind` de ACP. Lo manda el agente, así que el ícono
       se elige y no se adivina del nombre de la herramienta. Los que el mapa no
       cubre caen en `other`, que es una cruz: mejor una marca neutra que
       ninguna, porque la fila sin ícono se desalinea de las de al lado. -->
  <svg
    viewBox="0 0 24 24"
    width={size}
    height={size}
    fill="none"
    stroke="currentColor"
    stroke-width="1.7"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"><path d={KINDS[name] ?? KINDS.other} /></svg
  >
{/if}
