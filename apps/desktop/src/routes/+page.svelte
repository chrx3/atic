<script lang="ts">
  /**
   * La ventana principal.
   *
   * Tres líneas: todo lo que había acá —25 variables de estado, 22 oyentes en
   * un `onMount` y nueve modales como booleanos sueltos— vive ahora repartido
   * entre `domain/` y `features/`, que es donde se puede leer y probar.
   */
  import MainSurface from "$surfaces/main/MainSurface.svelte";
</script>

<MainSurface />
