<script lang="ts">
  /**
   * La ventana principal nueva, todavía en una ruta de dev.
   *
   * No reemplaza a `/` a propósito: la app se usa todos los días y cinco de
   * sus seis herramientas siguen sin reescribir. Cuando estén, el cambio es
   * mover este `<MainSurface />` a `routes/+page.svelte` y borrar las 1.135
   * líneas de allá.
   *
   * Ojo al probar: esto habla con los MISMOS comandos de Rust que la app real.
   * Grabar acá graba de verdad y borrar borra de verdad.
   */
  import MainSurface from "$surfaces/main/MainSurface.svelte";
</script>

<svelte:head><title>Atic · UI nueva</title></svelte:head>

<MainSurface />
