<script lang="ts">
  import { onMount } from "svelte";
  import "../app.css";
  import { theme } from "$lib/site/theme.svelte";

  let { children } = $props();

  onMount(() => {
    // El script inline de `app.html` ya puso el tema antes del primer pintado;
    // esto solo re-sincroniza el estado reactivo.
    theme.init();
  });
</script>

{@render children()}
