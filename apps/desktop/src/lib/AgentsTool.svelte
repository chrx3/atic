<script lang="ts">
  /**
   * Agentes en la ventana principal.
   *
   * La conversación entera vive en [`AgentsChat`], que es la misma que abre la
   * pill: son dos ventanas mirando las mismas sesiones, no dos consolas
   * distintas. Lo que se empiece acá se puede seguir desde la pill y al revés.
   */
  import AgentsChat from "$lib/AgentsChat.svelte";
</script>

<AgentsChat />
