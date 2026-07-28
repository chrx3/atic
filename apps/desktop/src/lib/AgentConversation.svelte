<script lang="ts">
  /**
   * La conversación: los items de un hilo, en orden.
   *
   * # Por qué vive aparte
   *
   * `routes/agents/+page.svelte` mezclaba tres cosas que no tienen nada que ver
   * entre sí: la geometría del globo anclado a la pill, el protocolo con el
   * backend, y esto. Sacar el render deja el archivo grande hablando de una
   * cosa menos, y sobre todo hace que dibujar una conversación **no dependa de
   * estar dentro de la burbuja**: el historial de hilos guardados usa este mismo
   * componente, y cualquier vista futura también.
   *
   * # Lo que NO sabe
   *
   * De dónde salen los items. Una sesión viva y un hilo guardado tienen la
   * misma forma —turnos con items—, así que acá no se distinguen: lo único que
   * cambia es que el guardado ya no crece. Tampoco sabe de sesiones, permisos
   * pendientes ni compositor; todo eso sigue siendo del contenedor.
   */
  import AgentCollabCard from "$lib/AgentCollabCard.svelte";
  import AgentMessage from "$lib/AgentMessage.svelte";
  import AgentToolCard from "$lib/AgentToolCard.svelte";
  import type { AgentItem } from "$lib/types";

  let {
    items,
    turnEnds,
  }: {
    items: AgentItem[];
    /**
     * Dónde cae el cierre de cada turno, y con cuánto costó.
     *
     * Se indexa por el id del ÚLTIMO item del turno: la regla se dibuja
     * después de él, que es donde se lee como separación entre respuestas.
     * El valor es `null` cuando el backend no informa costo —Codex manda
     * tokens y no dinero—, y ahí la regla lo dice con palabras.
     */
    turnEnds: Map<string, number | null>;
  } = $props();
</script>

{#each items as item (item.id)}
  {#if item.kind === "message" && item.role === "user"}
    <!-- El turno del usuario. Antes no se dibujaba en ninguna parte: el
         registro solo tenía lo que venía del backend, así que la conversación
         se leía como un monólogo del agente. -->
    <div class="mine">
      <span class="mine-who">
        tú
        <!-- Por dónde entró. Es lo propio de Atic: en cualquier otra GUI todo
             lo que dice el usuario vino del teclado, así que no habría nada
             que distinguir. -->
        {#if item.origin}
          <span class="via">{item.origin.via}</span>
        {/if}
      </span>
      <AgentMessage text={item.text} />
      {#if item.origin?.file}
        <div class="shot">
          <span class="thumb" aria-hidden="true"></span>
          {item.origin.file}
        </div>
      {/if}
    </div>
  {:else if item.kind === "message"}
    <div class:wip={item.streaming}>
      <AgentMessage text={item.text} />
      {#if item.streaming}
        <span class="caret" aria-hidden="true"></span>
      {/if}
    </div>
  {:else if item.kind === "tool"}
    <AgentToolCard
      name={item.name}
      title={item.title}
      toolKind={item.toolKind}
      input={item.input}
      output={item.output}
      status={item.status}
      locations={item.locations}
    />
  {:else if item.kind === "collab"}
    <AgentCollabCard
      name={item.name}
      title={item.title}
      subagentType={item.subagentType}
      status={item.status}
      summary={item.summary}
    />
  {:else if item.kind === "reasoning"}
    <!-- El razonamiento es trabajo previo, no la respuesta: se ofrece plegado
         para que no compita con lo que el agente dice. -->
    <details class="think">
      <!-- Los puntos mientras sigue llegando: es la única señal de que el
           razonamiento no terminó, porque plegado no se ve crecer. -->
      <summary>pensando{item.streaming ? "…" : ""}</summary>
      <p>{item.text}</p>
    </details>
  {:else if item.kind === "plan"}
    <div class="plan">
      <div class="plan-h">plan</div>
      {#each item.entries as e, i (i)}
        <div class="plan-e" data-s={e.status}>
          <span class="plan-b"
            >{e.status === "completed"
              ? "✓"
              : e.status === "in_progress"
                ? "▸"
                : "○"}</span
          >
          <span class="plan-t">{e.text}</span>
        </div>
      {/each}
    </div>
  {:else if item.kind === "notice"}
    <p class="warn">{item.text}</p>
  {/if}

  <!-- Cierre de turno: una regla con el costo al medio. Separa las respuestas
       entre sí, que sin esto se leían como una sola. -->
  {#if turnEnds.has(item.id)}
    <p class="turn">
      <span class="turn-t"
        >{turnEnds.get(item.id) !== null
          ? `$${turnEnds.get(item.id)?.toFixed(4)}`
          : "fin del turno"}</span
      >
    </p>
  {/if}
{/each}

<style>
  /* Los tokens (--coral, --line, --dim, …) los pone la burbuja y bajan por
     herencia: este componente no los define para no quedar con una copia que
     se desincronice del original. */

  /* Texto en vivo: el cursor parpadeante es la señal de que sigue escribiendo,
     y evita tener que poner un «trabajando…» encima de su propia respuesta. */
  .wip {
    position: relative;
  }

  .caret {
    display: inline-block;
    width: 0.45rem;
    height: 0.85rem;
    margin-left: 0.15rem;
    background: var(--coral);
    vertical-align: text-bottom;
    animation: blink 1s steps(2, start) infinite;
  }

  @keyframes blink {
    50% {
      opacity: 0;
    }
  }

  /* El turno del usuario: barra al costado, sin caja. Es la voz propia dentro
     del registro, no una tarjeta más. */
  .mine {
    border-left: 2px solid var(--coral);
    padding-left: 0.55rem;
  }
  .mine-who {
    display: block;
    color: var(--faint);
    font-size: 0.6875rem;
    letter-spacing: 0.03em;
  }

  /* De dónde vino: dictado, captura, portapapeles. */
  .via {
    display: inline-flex;
    align-items: center;
    border: 1px solid var(--line);
    border-radius: 999px;
    padding: 0 0.375rem 1px;
    color: var(--coral);
    font-size: 0.625rem;
    gap: 0.25rem;
    vertical-align: 1px;
  }

  /* Lo adjuntado. La miniatura es un degradado y no la imagen: cargar el PNG
     para un recuadro de 30×20 costaría más de lo que dice. */
  .shot {
    display: flex;
    max-width: 190px;
    align-items: center;
    margin-top: 0.35rem;
    border: 1px solid var(--line);
    border-radius: 7px;
    padding: 0.3rem 0.5rem;
    background: var(--code);
    color: var(--dim);
    font-size: 0.6875rem;
    gap: 0.45rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .shot .thumb {
    width: 30px;
    height: 20px;
    flex-shrink: 0;
    border-radius: 3px;
    background: linear-gradient(135deg, #3a332e, #262120 60%, #1d1a18);
    box-shadow: inset 0 0 0 1px var(--line);
  }

  /* Plan propuesto por el agente, con el estado de cada paso. */
  .plan {
    border: 1px solid var(--line);
    border-radius: 9px;
    padding: 0.45rem 0.6rem;
    background: var(--card);
  }
  .plan-h {
    color: var(--faint);
    font-size: 0.6875rem;
    letter-spacing: 0.03em;
  }
  .plan-e {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    color: var(--dim);
    font-size: 0.71875rem;
  }
  .plan-b {
    flex-shrink: 0;
    width: 1.1em;
    color: var(--faint);
  }
  .plan-e[data-s="in_progress"] {
    color: var(--text);
  }
  .plan-e[data-s="in_progress"] .plan-b {
    color: var(--coral);
  }
  .plan-e[data-s="completed"] .plan-b {
    color: var(--add);
  }
  .plan-e[data-s="completed"] .plan-t {
    text-decoration: line-through;
    text-decoration-color: var(--line);
  }

  .think {
    border-left: 2px solid var(--line);
    padding-left: 0.55rem;
    color: var(--faint);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.6875rem;
  }
  /* El mismo marcador que la tarjeta de herramienta. El triangulito por
     defecto de `<details>` es del navegador y desentona con todo lo demás. */
  .think summary {
    cursor: pointer;
    list-style: none;
    user-select: none;
  }
  .think summary::-webkit-details-marker {
    display: none;
  }
  .think summary::before {
    content: "⌄ ";
  }
  .think[open] summary::before {
    content: "⌃ ";
  }
  /* La barra la lleva el bloque entero, no el párrafo: así el resumen queda
     dentro y no colgando al costado de su propio contenido. */
  .think p {
    margin: 0.25rem 0 0;
    color: var(--dim);
    line-height: 1.5;
    white-space: pre-wrap;
  }

  /* Cierre de turno: una regla fina que corta el ancho. Sin ella, dos
     respuestas seguidas se leían como una sola. */
  .turn {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    margin: 0.15rem 0;
    color: var(--faint);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.625rem;
  }
  .turn::before,
  .turn::after {
    height: 1px;
    flex: 1;
    background: var(--line);
    content: "";
  }
  .turn-t {
    flex-shrink: 0;
  }

  /* Mismo aviso que en la burbuja. Se repite acá y no se comparte porque los
     estilos de Svelte son de cada componente: llevarlo a un global por cinco
     líneas costaría más de lo que ahorra. */
  .warn {
    margin: 0;
    color: var(--coral);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.71875rem;
    line-height: 1.5;
  }
</style>
