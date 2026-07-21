<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";

  // Si el overlay cae en error (p. ej. race al arrancar), vuelve a la ruta
  // limpia para que el listener de captura pueda registrarse de nuevo.
  onMount(() => {
    const t = setTimeout(() => {
      void goto("/capture-overlay", { replaceState: true, invalidateAll: true });
    }, 50);
    return () => clearTimeout(t);
  });
</script>

<div class="recover" aria-hidden="true"></div>

<style>
  .recover {
    position: fixed;
    inset: 0;
    background: #111;
  }
</style>
