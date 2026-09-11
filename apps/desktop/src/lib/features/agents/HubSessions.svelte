<script lang="ts">
  /**
   * Las sesiones que los agentes abren entre sí por MCP.
   *
   * No las abre nadie desde la interfaz: nacen de un `atic_delegate` y hasta
   * ahora corrían sin ventana, así que la única forma de saber qué se decían
   * era preguntárselo al agente. Acá se ven: el árbol de quién pidió a quién a
   * la izquierda, y la conversación de la que elijas a la derecha.
   *
   * Es de solo lectura a propósito. Escribirle a un hijo desde acá es una
   * conversación aparte —quién queda como interlocutor, qué pasa si el padre
   * está esperando ese turno— y meterla ahora mezclaría dos problemas.
   */
  import {
    agents,
    nombrePadre,
    type AgentSessionView,
  } from "$lib/agentSessions.svelte";
  import type { AgentItem, AgentTurn } from "$lib/types";

  /** Una sesión y su sitio en la cadena. */
  type Nodo = { s: AgentSessionView; hondura: number };

  let abiertas = $state<string[]>([]);
  let activa = $state<string | null>(null);

  /**
   * El árbol, aplanado con su hondura.
   *
   * Recorrido en anchura desde las raíces —las que nadie pidió, o las que
   * cuelgan de una app de fuera— para que un hijo no aparezca antes que su
   * padre. Las huérfanas (el padre ya murió) se cuelgan de la raíz en vez de
   * desaparecer: la conversación sigue ahí y es lo que interesa.
   */
  const arbol = $derived.by(() => {
    const vivas = agents.sessions;
    const conocidas = new Set(vivas.map((s) => s.id));
    // Tabla de paso del recorrido, no estado reactivo: se arma en cada
    // derivación y se lee por `get`.
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- ver arriba
    const hijos = new Map<string | null, AgentSessionView[]>();
    for (const s of vivas) {
      const padre = s.parent && conocidas.has(s.parent) ? s.parent : null;
      const lista = hijos.get(padre) ?? [];
      lista.push(s);
      hijos.set(padre, lista);
    }
    const salida: Nodo[] = [];
    const visitar = (padre: string | null, hondura: number) => {
      for (const s of hijos.get(padre) ?? []) {
        salida.push({ s, hondura });
        visitar(s.id, hondura + 1);
      }
    };
    visitar(null, 0);
    return salida;
  });

  const activaSesion = $derived(agents.byId(activa));
  const pestañas = $derived(
    abiertas.map((id) => agents.byId(id)).filter((s): s is AgentSessionView => !!s),
  );

  /** Nombre corto: el que le puso quien la pidió, o el del agente. */
  function nombre(s: AgentSessionView): string {
    return s.label?.trim() || s.backendName;
  }

  function abrir(id: string) {
    if (!abiertas.includes(id)) abiertas = [...abiertas, id];
    activa = id;
    agents.watch(id);
  }

  function cerrar(id: string) {
    abiertas = abiertas.filter((x) => x !== id);
    if (activa === id) activa = abiertas.at(-1) ?? null;
  }

  /**
   * Una sesión que abre otro agente se abre también acá.
   *
   * Es la mitad de la pregunta «ver cómo interactúan»: si hay que ir a
   * buscarla a mano, para cuando la encuentras el turno ya pasó. Solo las que
   * tienen padre: las que abre el usuario ya tienen su propia ventana.
   */
  $effect(() => {
    for (const { s } of arbol) {
      if (!s.parent || abiertas.includes(s.id)) continue;
      abiertas = [...abiertas, s.id];
      activa ??= s.id;
    }
  });

  $effect(() => {
    void agents.refreshMeta();
  });

  function textoDe(item: AgentItem): string {
    return item.kind === "message" || item.kind === "reasoning" ? item.text : "";
  }

  /** Los turnos con algo que enseñar; un turno vacío es ruido. */
  function turnosCon(s: AgentSessionView): AgentTurn[] {
    return s.turns.filter((t) => t.items.length > 0);
  }
</script>

<div class="hs">
  <aside class="arbol">
    <p class="titulo">Delegaciones</p>
    {#if arbol.length === 0}
      <p class="vacio">
        Ninguna. Aparecen solas cuando un agente le encarga algo a otro.
      </p>
    {:else}
      <ul>
        {#each arbol as { s, hondura } (s.id)}
          <li style="--hondura: {hondura}">
            <button
              type="button"
              class="nodo"
              class:activa={activa === s.id}
              onclick={() => abrir(s.id)}
            >
              <span class="nombre">{nombre(s)}</span>
              <span class="meta">
                {s.backendName}{#if nombrePadre(s.parent)}
                  · {nombrePadre(s.parent)}{/if}
              </span>
              <span class="estado" data-estado={s.status}>
                {#if s.pending.length > 0}
                  permiso
                {:else if s.status === "working"}
                  trabajando
                {:else if s.unread > 0}
                  {s.unread}
                {/if}
              </span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </aside>

  <section class="chat">
    {#if pestañas.length > 0}
      <nav class="tabs">
        {#each pestañas as s (s.id)}
          <span class="tab" class:activa={activa === s.id}>
            <button type="button" class="tab-abrir" onclick={() => abrir(s.id)}>
              {nombre(s)}
            </button>
            <button
              type="button"
              class="tab-cerrar"
              onclick={() => cerrar(s.id)}
              aria-label="Cerrar {nombre(s)}">×</button
            >
          </span>
        {/each}
      </nav>
    {/if}

    {#if activaSesion}
      <div class="turnos">
        {#each turnosCon(activaSesion) as turno (turno.id)}
          <article class="turno" data-status={turno.status}>
            {#each turno.items as item (item.id)}
              {#if item.kind === "message"}
                <div class="msg" data-role={item.role}>
                  {#if item.origin}
                    <!-- Quién preguntó: sin esto, el mensaje de otro agente
                         parece escrito por el usuario. -->
                    <span class="de">{item.origin.via}</span>
                  {/if}
                  <p>{textoDe(item)}</p>
                </div>
              {:else if item.kind === "tool" || item.kind === "collab"}
                <div class="tool" data-status={item.status}>
                  <span class="tool-nombre">{item.name}</span>
                  <span class="tool-titulo">{item.title}</span>
                </div>
              {:else if item.kind === "permission"}
                <div class="permiso">Espera permiso: {item.tool}</div>
              {:else if item.kind === "notice"}
                <div class="aviso">{item.text}</div>
              {/if}
            {/each}
          </article>
        {/each}
        {#if turnosCon(activaSesion).length === 0}
          <p class="vacio">Todavía no ha dicho nada.</p>
        {/if}
      </div>
    {:else}
      <p class="vacio centro">Elige una sesión para ver qué se dicen.</p>
    {/if}
  </section>
</div>

<style>
  .hs {
    display: grid;
    grid-template-columns: minmax(160px, 240px) 1fr;
    gap: 12px;
    height: 100%;
    min-height: 0;
    font-size: 12px;
  }

  .arbol {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-height: 0;
    overflow-y: auto;
    border-right: 1px solid color-mix(in sRGB, var(--rb-text) 10%, transparent);
    padding-right: 10px;
  }

  .titulo {
    margin: 0;
    font-weight: 600;
    opacity: 0.75;
  }

  .vacio {
    margin: 0;
    opacity: 0.6;
    line-height: 1.5;
  }

  .centro {
    align-self: center;
    margin: auto;
  }

  .arbol ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .arbol li {
    /* La sangría dibuja la cadena: hijo, nieto. */
    padding-left: calc(var(--hondura) * 12px);
  }

  .nodo {
    display: grid;
    grid-template-columns: 1fr auto;
    grid-template-areas: "nombre estado" "meta estado";
    gap: 0 6px;
    width: 100%;
    text-align: left;
    background: transparent;
    border: 0;
    border-radius: 6px;
    padding: 4px 6px;
    color: inherit;
    cursor: pointer;
  }

  .nodo:hover {
    background: color-mix(in sRGB, var(--rb-text) 7%, transparent);
  }

  .nodo.activa {
    background: color-mix(in sRGB, var(--rb-text) 12%, transparent);
  }

  .nombre {
    grid-area: nombre;
    font-weight: 600;
  }

  .meta {
    grid-area: meta;
    opacity: 0.6;
    font-size: 11px;
  }

  .estado {
    grid-area: estado;
    align-self: center;
    opacity: 0.7;
    font-size: 11px;
  }

  .estado[data-estado="working"] {
    opacity: 1;
  }

  .chat {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
  }

  .tabs {
    display: flex;
    gap: 4px;
    overflow-x: auto;
    padding-bottom: 6px;
  }

  .tab {
    display: inline-flex;
    align-items: center;
    border-radius: 6px 6px 0 0;
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
  }

  .tab.activa {
    background: color-mix(in sRGB, var(--rb-text) 14%, transparent);
  }

  .tab-abrir,
  .tab-cerrar {
    background: transparent;
    border: 0;
    color: inherit;
    cursor: pointer;
    padding: 4px 6px;
  }

  .tab-cerrar {
    opacity: 0.6;
  }

  .turnos {
    display: flex;
    flex-direction: column;
    gap: 10px;
    overflow-y: auto;
    min-height: 0;
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

  .permiso,
  .aviso {
    font-size: 11px;
    opacity: 0.8;
  }
</style>
