<script lang="ts">
  /**
   * La conversación de una sesión que abrió otro agente por MCP.
   *
   * Solo lectura: escribirle a un hijo desde acá abre preguntas que no se
   * responden a medias —quién queda como interlocutor, qué pasa si el padre
   * está esperando ese mismo turno—, así que esto se limita a enseñar lo que
   * ya está pasando.
   *
   * Vive aparte de la vista que lo lista para poder montarlo en los dos sitios:
   * el panel de la ventana principal y una pestaña del rail de consolas.
   */
  import { agents, nombrePadre } from "$lib/agentSessions.svelte";
  import type { AgentItem } from "$lib/types";

  let { sessionId }: { sessionId: string } = $props();

  const sesion = $derived(agents.byId(sessionId));
  /** Los turnos con algo que enseñar: uno vacío es ruido. */
  const turnos = $derived(sesion?.turns.filter((t) => t.items.length > 0) ?? []);

  function texto(item: AgentItem): string {
    return item.kind === "message" || item.kind === "reasoning" ? item.text : "";
  }
</script>

<div class="conv">
  {#if !sesion}
    <p class="vacio">Esta sesión ya no está viva.</p>
  {:else}
    <header class="cab">
      <span class="quien">{sesion.label?.trim() || sesion.backendName}</span>
      <span class="meta">
        {sesion.backendName}{#if nombrePadre(sesion.parent)}
          · pedido por {nombrePadre(sesion.parent)}{/if}
      </span>
    </header>

    {#if turnos.length === 0}
      <p class="vacio">Todavía no ha dicho nada.</p>
    {:else}
      <div class="turnos">
        {#each turnos as turno (turno.id)}
          <article class="turno" data-status={turno.status}>
            {#each turno.items as item (item.id)}
              {#if item.kind === "message"}
                <div class="msg" data-role={item.role}>
                  {#if item.origin}
                    <!-- Quién preguntó: sin esto, lo que manda otro agente
                         parece escrito por el usuario. -->
                    <span class="de">{item.origin.via}</span>
                  {/if}
                  <p>{texto(item)}</p>
                </div>
              {:else if item.kind === "tool" || item.kind === "collab"}
                <div class="tool" data-status={item.status}>
                  <span class="tool-nombre">{item.name}</span>
                  <span class="tool-titulo">{item.title}</span>
                </div>
              {:else if item.kind === "permission"}
                <div class="aviso">Espera permiso: {item.tool}</div>
              {:else if item.kind === "notice"}
                <div class="aviso">{item.text}</div>
              {/if}
            {/each}
          </article>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .conv {
    display: flex;
    flex-direction: column;
    gap: 10px;
    height: 100%;
    min-height: 0;
    overflow-y: auto;
    padding: 10px 12px;
    font-size: 12px;
    line-height: 1.5;
  }

  .cab {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-bottom: 6px;
    border-bottom: 1px solid color-mix(in sRGB, var(--rb-text) 10%, transparent);
  }

  .quien {
    font-weight: 600;
  }

  .meta,
  .vacio {
    opacity: 0.6;
    font-size: 11px;
  }

  .vacio {
    margin: 0;
  }

  .turnos {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .turno {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .msg p {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .msg[data-role="user"] {
    opacity: 0.85;
    border-left: 2px solid color-mix(in sRGB, var(--rb-text) 30%, transparent);
    padding-left: 8px;
  }

  .de {
    display: block;
    font-size: 11px;
    opacity: 0.7;
    margin-bottom: 2px;
  }

  .tool {
    display: flex;
    gap: 6px;
    font-size: 11px;
    opacity: 0.75;
  }

  .tool[data-status="failed"] {
    opacity: 1;
  }

  .tool-nombre {
    font-weight: 600;
  }

  .tool-titulo {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .aviso {
    font-size: 11px;
    opacity: 0.8;
  }
</style>
