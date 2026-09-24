<script lang="ts">
  /**
   * Los hilos de la pizarra: una curva desde la consola que pidió hasta el
   * sub-agente que abrió (por el MCP de Atic o con «Encargar a…»).
   *
   * Mientras el sub-agente trabaja, el trazo corre hacia él; cuando terminó
   * y todavía no se miró, corre de vuelta (el resultado va hacia quien lo
   * pidió). Quieto, es solo el vínculo. Vive en el plano, detrás de las
   * tarjetas, en coordenadas de pizarra.
   */
  import type { Rect, Thread } from "./agentBoard";

  let { threads }: { threads: Thread[] } = $props();

  /**
   * Del borde derecho de quien pide al izquierdo del sub-agente; si el
   * sub-agente quedó a la izquierda, de lado a lado igual, con la curva
   * abierta hacia afuera.
   */
  function geometry(from: Rect, to: Rect) {
    const leftToRight = to.x >= from.x + from.w / 2;
    const a = {
      x: leftToRight ? from.x + from.w : from.x,
      y: from.y + Math.min(from.h / 2, 120),
    };
    const b = {
      x: leftToRight ? to.x : to.x + to.w,
      y: to.y + Math.min(to.h / 2, 120),
    };
    const bend = Math.max(60, Math.abs(b.x - a.x) / 2) * (leftToRight ? 1 : -1);
    return {
      d: `M ${a.x} ${a.y} C ${a.x + bend} ${a.y}, ${b.x - bend} ${b.y}, ${b.x} ${b.y}`,
      mid: { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 },
      a,
      b,
    };
  }
</script>

<svg class="threads" aria-hidden="true">
  {#each threads as thread (thread.id)}
    {@const g = geometry(thread.from, thread.to)}
    <g class="thread is-{thread.flow}" data-agent={thread.tone}>
      <path class="base" d={g.d} />
      <path class="flow" d={g.d} />
      <circle class="end" cx={g.a.x} cy={g.a.y} r="4" />
      <circle class="end" cx={g.b.x} cy={g.b.y} r="4" />
      <text class="label" x={g.mid.x} y={g.mid.y - 10}>{thread.label}</text>
    </g>
  {/each}
</svg>

<style>
  .threads {
    position: absolute;
    top: 0;
    left: 0;
    width: 1px;
    height: 1px;
    overflow: visible;
    pointer-events: none;
  }

  .thread {
    --tone: var(--agent-accent, var(--accent));
  }

  .base {
    fill: none;
    stroke: color-mix(in sRGB, var(--tone) 35%, transparent);
    stroke-width: 2;
  }

  .flow {
    fill: none;
    stroke: var(--tone);
    stroke-dasharray: 10 14;
    stroke-linecap: round;
    stroke-width: 2.5;
    opacity: 0;
  }

  .is-out .flow {
    opacity: 1;
    animation: flow-out 0.9s linear infinite;
  }

  .is-back .flow {
    opacity: 1;
    animation: flow-back 0.9s linear infinite;
  }

  @keyframes flow-out {
    to {
      stroke-dashoffset: -24;
    }
  }

  @keyframes flow-back {
    to {
      stroke-dashoffset: 24;
    }
  }

  .end {
    fill: var(--tone);
  }

  .label {
    fill: var(--rb-muted);
    font-family: var(--rb-font);
    font-size: 12px;
    text-anchor: middle;
  }

  @media (prefers-reduced-motion: reduce) {
    .is-out .flow,
    .is-back .flow {
      animation: none;
    }
  }
</style>
