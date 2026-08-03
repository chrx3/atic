<script lang="ts">
  /**
   * La ventana principal.
   *
   * Tres líneas: todo lo que había acá —25 variables de estado, 22 oyentes en
   * un `onMount` y nueve modales como booleanos sueltos— vive ahora repartido
   * entre `domain/` y `features/`, que es donde se puede leer y probar.
   *
   * La versión anterior sigue en `/legacy` hasta que esta pase una vuelta
   * completa en la ventana real de Tauri; se llega con Ctrl+Alt+M en dev.
   */
  import MainSurface from "$surfaces/main/MainSurface.svelte";
</script>

<MainSurface />
