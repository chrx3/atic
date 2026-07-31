<script lang="ts">
  /**
   * El lab en una ventana normal — la referencia contra la que comparar.
   *
   * Sirve para dos cosas: iterar rápido, y sobre todo tener la medición de
   * Chrome al lado de la de WebView2. Es donde se diseñó el sistema líquido, y
   * la pregunta del spike es exactamente si los dos coinciden.
   *
   * En dev, Vite sirve esto en `http://localhost:1420/dev/liquid-lab`, así que
   * se abre en Chrome sin pasar por Tauri.
   *
   * Para verlo DENTRO del overlay real: Ctrl+Alt+L en la ventana principal de
   * la app.
   */
  import LiquidLab from "$lib/dev/LiquidLab.svelte";
</script>

<svelte:head><title>liquid lab</title></svelte:head>

<LiquidLab standalone />
