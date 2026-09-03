<script lang="ts">
  /**
   * Marca de Atic: la 'a' de caja baja construida sobre la misma retícula de
   * 24 que los iconos. Un círculo (ojo) y una asta recta, sin tipografía de
   * sistema, para que escale igual que el resto del sistema.
   *
   * `alive`: el círculo es una cara. Mira al puntero del escritorio (no solo
   * al que entra en la overlay), parpadea y tiene ojeras.
   *
   * `state`: qué herramienta está corriendo AHORA. La cara la dice: los ojos
   * dejan paso al glifo del estado dentro del mismo círculo. La cabeza y el
   * asta no se mueven, así que la marca se sigue leyendo como la 'a' de Atic
   * — que es la razón de contarlo acá y no colgando una gota aparte.
   */

  import { prefersReducedMotion } from "$lib/motion";
  import { overlayCursor } from "$ipc/overlay";

  export type MarkState = "idle" | "recording" | "dictating";

  let {
    size = 24,
    strokeWidth = 1.25,
    alive = false,
    state: markState = "idle",
  }: {
    size?: number;
    strokeWidth?: number;
    alive?: boolean;
    state?: MarkState;
  } = $props();

  /** Con estado, la cara la ocupa el glifo: no hay ojos que animar. */
  const facing = $derived(alive && markState === "idle");

  let svgEl = $state<SVGSVGElement | null>(null);

  const MID_X = 12;
  const MID_Y = 11.15;
  const REST_SPREAD = 1.88;
  const LOOK_MAX = 1.18;
  const TURN_MAX = 12;
  const RX_REST = 0.88;
  const RY_REST = 1.32;
  const JUNTO = 1.22;
  const LEJOS = 2.05;
  const BAG = {
    adentro: 0.95,
    altura: 0.94,
    largo: 0.47,
    inclinacion: 0.88,
    curva: 0.42,
  };

  function lerp(a: number, b: number, t: number) {
    return a + (b - a) * t;
  }

  function clampSpread(s: number) {
    return Math.min(LEJOS, Math.max(JUNTO, s));
  }

  function bagPath(rx: number, ry: number, side: number) {
    const startX = -side * rx * BAG.adentro;
    const startY = ry * BAG.altura;
    const endX = -side * rx * (BAG.adentro - BAG.inclinacion);
    const endY = ry * (BAG.altura + BAG.largo);
    const cX = (startX + endX) / 2 - side * rx * BAG.curva * 0.35;
    const cY = (startY + endY) / 2 + ry * BAG.curva * 0.22;
    return `M ${startX} ${startY} Q ${cX} ${cY} ${endX} ${endY}`;
  }

  function overflow(x: number, y: number, pad: number) {
    const dx = x - 12;
    const dy = y - 12;
    const limit = 4.55 - pad;
    const d = Math.hypot(dx, dy) || 1;
    if (d <= limit) return { x: 0, y: 0 };
    const extra = (d - limit) / d;
    return { x: dx * extra, y: dy * extra };
  }

  function placePair(
    lookX: number,
    lookY: number,
    spread: number,
    rx: number,
    ry: number,
  ) {
    const pad = Math.max(rx, ry);
    const Lx = MID_X + lookX - spread;
    const Rx = MID_X + lookX + spread;
    const y = MID_Y + lookY;
    const oL = overflow(Lx, y, pad);
    const oR = overflow(Rx, y, pad);
    const ox = Math.abs(oL.x) > Math.abs(oR.x) ? oL.x : oR.x;
    const oy = Math.abs(oL.y) > Math.abs(oR.y) ? oL.y : oR.y;
    return {
      L: { x: Lx - ox, y: y - oy },
      R: { x: Rx - ox, y: y - oy },
    };
  }

  $effect(() => {
    if (!facing || !svgEl || prefersReducedMotion()) return;

    const svg = svgEl;
    const headNode = svg.querySelector<SVGGElement>(".am-head");
    const eyeLNode = svg.querySelector<SVGGElement>(".am-eye-l");
    const eyeRNode = svg.querySelector<SVGGElement>(".am-eye-r");
    const ovalLNode = svg.querySelector<SVGEllipseElement>(".am-eye-l .am-ball");
    const ovalRNode = svg.querySelector<SVGEllipseElement>(".am-eye-r .am-ball");
    const bagLNode = svg.querySelector<SVGPathElement>(".am-eye-l .am-bag");
    const bagRNode = svg.querySelector<SVGPathElement>(".am-eye-r .am-bag");
    if (
      !headNode ||
      !eyeLNode ||
      !eyeRNode ||
      !ovalLNode ||
      !ovalRNode ||
      !bagLNode ||
      !bagRNode
    ) {
      return;
    }
    const head = headNode;
    const eyeL = eyeLNode;
    const eyeR = eyeRNode;
    const ovalL = ovalLNode;
    const ovalR = ovalRNode;
    const bagL = bagLNode;
    const bagR = bagRNode;

    const leanMax = size * 0.05;
    const want = {
      lookX: 0,
      lookY: 0,
      rot: 0,
      leanX: 0,
      leanY: 0,
      spread: REST_SPREAD,
      rx: RX_REST,
      ry: RY_REST,
      lid: 1,
    };
    const now = { ...want };
    let t0 = performance.now();
    let lidHold = 0;
    let raf = 0;
    let blinkTimer = 0;
    let blinkFollow = 0;
    let cancelled = false;
    let cursorPending = false;

    function aim(clientX: number, clientY: number) {
      const box = svg.getBoundingClientRect();
      const cx = box.left + box.width / 2;
      const cy = box.top + box.height / 2;
      const dx = clientX - cx;
      const dy = clientY - cy;
      const dist = Math.hypot(dx, dy) || 1;
      const face = box.width * 0.5;
      const reach = Math.min(
        1,
        Math.max(0, (dist - face * 0.18) / (face * 1.35)),
      );
      const nx = dx / dist;
      const ny = dy / dist;
      want.lookX = nx * LOOK_MAX * reach;
      want.lookY = ny * LOOK_MAX * reach;
      want.rot = nx * TURN_MAX * reach;
      want.leanX = nx * leanMax * reach;
      want.leanY = ny * leanMax * 0.4 * reach;
      want.spread = clampSpread(lerp(JUNTO, LEJOS, reach));
      want.rx = lerp(RX_REST + 0.1, RX_REST * 0.72, reach * Math.abs(nx));
      want.ry = lerp(RY_REST + 0.08, RY_REST * 0.92, reach);
    }

    function blink(hold = 70) {
      want.lid = 0.1;
      lidHold = hold;
    }

    function scheduleBlink() {
      blinkTimer = window.setTimeout(() => {
        if (cancelled) return;
        blink(60 + Math.random() * 50);
        if (Math.random() < 0.22) {
          blinkFollow = window.setTimeout(() => {
            if (!cancelled) blink(55);
          }, 160);
        }
        scheduleBlink();
      }, 1800 + Math.random() * 3400);
    }

    function onMove(e: PointerEvent | MouseEvent) {
      aim(e.clientX, e.clientY);
    }

    function pollCursor() {
      if (cursorPending || cancelled) return;
      cursorPending = true;
      void overlayCursor()
        .then((p) => {
          if (!cancelled && p) aim(p.x, p.y);
        })
        .catch(() => {
          /* Sin overlay (tests, ventana principal): alcanza pointermove. */
        })
        .finally(() => {
          cursorPending = false;
        });
    }

    function onDown() {
      blink(80);
      window.clearTimeout(blinkTimer);
      window.clearTimeout(blinkFollow);
      scheduleBlink();
    }

    let last = performance.now();

    function tick(time: number) {
      if (cancelled) return;
      const dt = Math.min(32, time - last);
      last = time;

      if (lidHold > 0) {
        lidHold -= dt;
        if (lidHold <= 0) want.lid = 1;
      }

      now.rot += (want.rot - now.rot) * 0.08;
      now.leanX += (want.leanX - now.leanX) * 0.08;
      now.leanY += (want.leanY - now.leanY) * 0.08;
      now.lookX += (want.lookX - now.lookX) * 0.16;
      now.lookY += (want.lookY - now.lookY) * 0.16;
      now.spread += (want.spread - now.spread) * 0.14;
      now.rx += (want.rx - now.rx) * 0.14;
      now.ry += (want.ry - now.ry) * 0.14;
      now.lid += (want.lid - now.lid) * 0.42;
      now.spread = clampSpread(now.spread);

      const sway = Math.sin((time - t0) / 1100) * 1.2;
      const rx = now.rx;
      const ry = Math.max(0.08, now.ry * now.lid);

      ovalL.setAttribute("rx", rx.toFixed(3));
      ovalR.setAttribute("rx", rx.toFixed(3));
      ovalL.setAttribute("ry", ry.toFixed(3));
      ovalR.setAttribute("ry", ry.toFixed(3));
      bagL.setAttribute("d", bagPath(rx, now.ry, -1));
      bagR.setAttribute("d", bagPath(rx, now.ry, 1));
      const bagOp = now.lid > 0.88 ? "1" : "0";
      bagL.setAttribute("opacity", bagOp);
      bagR.setAttribute("opacity", bagOp);

      const pair = placePair(now.lookX, now.lookY, now.spread, rx, ry);
      eyeL.setAttribute("transform", `translate(${pair.L.x} ${pair.L.y})`);
      eyeR.setAttribute("transform", `translate(${pair.R.x} ${pair.R.y})`);
      head.style.transform = `translate(${now.leanX}px, ${now.leanY}px) rotate(${now.rot + sway}deg)`;

      pollCursor();
      raf = requestAnimationFrame(tick);
    }

    window.addEventListener("pointermove", onMove, { passive: true });
    svg.addEventListener("pointerdown", onDown);
    scheduleBlink();
    pollCursor();
    raf = requestAnimationFrame(tick);

    return () => {
      cancelled = true;
      cancelAnimationFrame(raf);
      window.clearTimeout(blinkTimer);
      window.clearTimeout(blinkFollow);
      window.removeEventListener("pointermove", onMove);
      svg.removeEventListener("pointerdown", onDown);
    };
  });
</script>

<svg
  bind:this={svgEl}
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="butt"
  aria-hidden="true"
>
  <!-- Tinta centrada en la retícula: ojo x 6.5→17.5, asta tangente en 17.5,
       ambos de 11 de alto. El centro geométrico cae en (12, 12). -->
  <g class="am-head">
    <circle cx="12" cy="12" r="5.5" />
    <path d="M17.5 6.5V17.5" />
    {#if facing}
      <g class="am-eye-l" transform="translate(10.12 11.15)">
        <ellipse class="am-ball" cx="0" cy="0" rx="0.88" ry="1.32" />
        <path
          class="am-bag"
          d="M 0.836 1.241 Q 0.312 1.684 -0.062 1.861"
        />
      </g>
      <g class="am-eye-r" transform="translate(13.88 11.15)">
        <ellipse class="am-ball" cx="0" cy="0" rx="0.88" ry="1.32" />
        <path
          class="am-bag"
          d="M -0.836 1.241 Q -0.312 1.684 0.062 1.861"
        />
      </g>
    {:else if markState === "recording"}
      <!-- El cuadrado de parar, en el hueco de la cara. Late como la gota que
           antes colgaba; el punto es que ahora no sobresale de la silueta. -->
      <rect
        class="am-state am-rec"
        x="9.8"
        y="9.8"
        width="4.4"
        height="4.4"
        rx="1.15"
      />
    {:else if markState === "dictating"}
      <!-- Tres barras: la misma lectura que las ondas de la barra, al tamaño
           de una pupila. Se mueven desfasadas para que se lea "escuchando". -->
      <g class="am-state am-dict">
        <rect x="9.15" y="10.05" width="1.25" height="3.9" rx="0.62" />
        <rect x="11.38" y="9.05" width="1.25" height="5.9" rx="0.62" />
        <rect x="13.6" y="10.05" width="1.25" height="3.9" rx="0.62" />
      </g>
    {/if}
  </g>
</svg>

<style>
  svg {
    overflow: visible;
    display: block;
  }

  .am-head {
    transform-box: view-box;
    transform-origin: 12px 12px;
  }

  .am-ball {
    fill: currentColor;
    stroke: none;
  }

  .am-bag {
    fill: none;
    stroke: currentColor;
    stroke-width: 0.14;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .am-state {
    stroke: none;
  }

  .am-rec {
    fill: var(--rb-record, #e5483f);
    transform-box: view-box;
    transform-origin: 12px 12px;
    animation: am-rec-pulse 1.6s ease-in-out infinite;
  }

  .am-dict rect {
    fill: currentColor;
    transform-box: view-box;
    transform-origin: 12px 12px;
    animation: am-dict-wave 0.9s ease-in-out infinite;
  }

  .am-dict rect:nth-child(2) {
    animation-delay: 0.15s;
  }

  .am-dict rect:nth-child(3) {
    animation-delay: 0.3s;
  }

  @keyframes am-rec-pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.45;
    }
  }

  @keyframes am-dict-wave {
    0%,
    100% {
      transform: scaleY(1);
    }
    50% {
      transform: scaleY(0.55);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .am-rec,
    .am-dict rect {
      animation: none;
    }
  }
</style>
