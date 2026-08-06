<script lang="ts">
  /**
   * Rueda de herramientas con campo de partículas en canvas 2D.
   *
   * Los nodos son botones HTML en posiciones fijas (accesibles por teclado y
   * con memoria muscular); el canvas solo aporta el movimiento de fondo.
   *
   * El campo no guarda velocidades: cada partícula orbita a un radio relativo
   * al anillo y el puntero solo la desplaza con un campo de distancia. Así el
   * resize nunca la "rehace" y nada puede quedarse pegado al cursor.
   */

  import AticMark from "$lib/AticMark.svelte";
  import ToolIcon from "$lib/ToolIcon.svelte";
  import GooFilter, { preFilter } from "$lib/GooFilter.svelte";
  import { playWheelTick } from "$core/uiSound";
  import { TOOLS, type ToolId } from "$lib/tools";
  import {
    nodeAngle as angleOf,
    nodePosition,
    separators as wedgeSeparators,
    wedgeClip,
  } from "$surfaces/overlay/pill/wheelGeometry";

  let {
    caption = "Elige una herramienta",
    /** Variante para la pill: anillo más abierto, sin etiquetas sueltas. */
    compact = false,
    /** Navegación con la rueda del ratón (la pill la usa como selector). */
    wheelNav = false,
    /** Campo de partículas de fondo; la pill lo apaga. */
    particles = true,
    /** false = anillo colapsado en el centro (estado cerrado del morph). */
    revealed = true,
    activeId = $bindable<ToolId | null>(null),
    hint = "",
    onSelect,
    onCenter,
  }: {
    caption?: string;
    compact?: boolean;
    wheelNav?: boolean;
    particles?: boolean;
    revealed?: boolean;
    activeId?: ToolId | null;
    /** Pie discreto: enseña que la rueda también vive en la pill. */
    hint?: string;
    onSelect?: (id: ToolId) => void;
    /** Si está, la marca central es un botón (la pill abre la app). */
    onCenter?: () => void;
  } = $props();

  const NODE_COUNT = TOOLS.length;

  /**
   * Piel líquida de la variante compacta (la pill), en px.
   *
   * Las seis herramientas no se dibujan sobre un disco: SON gotas que salen
   * del núcleo. Mientras viajan siguen fundidas con él y se sueltan cuando la
   * separación supera el alcance del filtro —1.72·σ, o sea 8.6 px con σ = 5—.
   *
   * Con el anillo en 70 y estos radios, el hueco final es 70 − 29 − 28 = 13,
   * así que se sueltan con margen. Subir σ por encima de 7.5 las dejaría
   * pegadas al núcleo para siempre.
   */
  const SKIN = {
    /** Diámetro de cada gota, abierta. */
    node: 56,
    /** Diámetro del núcleo, abierto. */
    core: 58,
    /** Cerrado todo vuelve al disco de la pill en reposo (`PILL.bar`). */
    closed: 40,
  } as const;

  let host = $state<HTMLDivElement | null>(null);
  let canvas = $state<HTMLCanvasElement | null>(null);
  let width = $state(0);
  let height = $state(0);
  let nodeEls: HTMLButtonElement[] = [];

  /**
   * Radio del anillo respecto al lado menor del lienzo.
   *
   * En compacto era 0.34 (78.9 px sobre 232) cuando los nodos eran iconos
   * sueltos sobre un disco. Con gotas de 56 el conjunto tiene que leerse como
   * UNA cosa y no como seis botones desperdigados, así que el anillo se cerró
   * a 0.30 — 69.6 px, casi los 70 con los que están calculados los radios de
   * `SKIN`.
   */
  const ringRatio = $derived(compact ? 0.3 : 0.27);
  const ringRadius = $derived(Math.min(width, height) * ringRatio);
  const activeTool = $derived(
    TOOLS.find((tool) => tool.id === activeId) ?? null,
  );

  /** Fija la herramienta activa; suena solo si el paso cambia y la rueda navega. */
  function setActive(id: ToolId | null, { tick = false } = {}) {
    if (id === activeId) return;
    activeId = id;
    if (tick && wheelNav && id) playWheelTick();
  }

  /** Mueve la selección un paso; la pill lo usa desde la rueda del ratón. */
  function stepActive(direction: 1 | -1) {
    const index = TOOLS.findIndex((tool) => tool.id === activeId);
    const next = index < 0 ? 0 : (index + direction + NODE_COUNT) % NODE_COUNT;
    setActive(TOOLS[next].id, { tick: true });
  }

  function onWheel(event: WheelEvent) {
    if (!wheelNav || event.deltaY === 0) return;
    event.preventDefault();
    stepActive(event.deltaY > 0 ? 1 : -1);
  }

  /** Flechas: mueven el foco al nodo vecino (el foco fija activeId). */
  function onNodeKeydown(event: KeyboardEvent) {
    let dir = 0;
    if (event.key === "ArrowRight" || event.key === "ArrowDown") dir = 1;
    else if (event.key === "ArrowLeft" || event.key === "ArrowUp") dir = -1;
    else return;
    event.preventDefault();
    const index = TOOLS.findIndex((tool) => tool.id === activeId);
    const next = ((index < 0 ? 0 : index + dir) + NODE_COUNT) % NODE_COUNT;
    nodeEls[next]?.focus();
  }

  /** Ángulo del nodo i; 0 rad = arriba, sentido horario. */
  function nodeAngle(index: number): number {
    return angleOf(index, NODE_COUNT);
  }

  /** Lado menor: escala el anillo y el núcleo (los gajos usan todo el área). */
  const side = $derived(Math.min(width, height));
  /**
   * Diámetro del núcleo neutro del centro (no selecciona ningún gajo).
   *
   * En compacto se ata a la gota del núcleo: si fuera más grande, la zona
   * clickeable saldría del líquido y un clic sobre la ventana transparente
   * —donde no se ve nada— abriría la app.
   */
  const coreSize = $derived(compact ? SKIN.core : ringRadius * 1.2);

  const nodes = $derived(
    TOOLS.map((tool, index) => ({
      tool,
      ...nodePosition(index, NODE_COUNT, { width, height }, ringRadius),
      clip: wedgeClip(index, NODE_COUNT, { width, height }),
    })),
  );

  /** El separador nace fuera del núcleo. */
  const sepInner = $derived(coreSize / 2 + 8);

  /** Fronteras entre gajos: se disuelven en el borde del lienzo. */
  const separators = $derived(wedgeSeparators(NODE_COUNT, { width, height }, sepInner));

  // El tamaño se mide siempre, haya partículas o no: de él dependen las
  // posiciones de los nodos. offsetWidth y no getBoundingClientRect, porque
  // este último incluye el transform del padre durante el morph de la pill.
  $effect(() => {
    const hostEl = host;
    if (!hostEl) return;
    const observer = new ResizeObserver(() => {
      width = hostEl.offsetWidth;
      height = hostEl.offsetHeight;
    });
    observer.observe(hostEl);
    return () => observer.disconnect();
  });

  // $effect (y no onMount) porque depende de que bind:this ya esté asignado.
  $effect(() => {
    const hostEl = host;
    const ctx = canvas?.getContext("2d");
    if (!hostEl || !ctx || !particles) return;
    return runField(hostEl, ctx);
  });

  /** La simulación vive aparte para trabajar con referencias ya no nulas. */
  function runField(hostEl: HTMLDivElement, ctx: CanvasRenderingContext2D) {
    const canvasEl = ctx.canvas;
    const reduceMotion = window.matchMedia(
      "(prefers-reduced-motion: reduce)",
    ).matches;

    type Particle = {
      /** Ángulo orbital actual. */
      a: number;
      /** Radio relativo al anillo (≈0.58–1.42). */
      rf: number;
      /** Velocidad angular (rad/s), mayor cerca del centro. */
      va: number;
      size: number;
      /** Fase y velocidad del titileo. */
      tw: number;
      tws: number;
    };

    const REACH = 130;

    let parts: Particle[] = [];
    let raf = 0;
    let running = true;
    let cssW = 0;
    let cssH = 0;
    let prev = 0;
    // Puntero suavizado + peso de influencia con inercia: la interacción
    // aparece y se apaga en fundido en vez de a saltos.
    let px = 0;
    let py = 0;
    let tx = 0;
    let ty = 0;
    let inf = 0;
    let infTarget = 0;

    let ink: [number, number, number] = [26, 26, 23];
    let sprite: HTMLCanvasElement | null = null;

    /** Normaliza cualquier color CSS a RGB usando el propio canvas. */
    function resolveRgb(value: string, fallback: [number, number, number]) {
      ctx.fillStyle = "#000";
      ctx.fillStyle = value.trim();
      const normalized = ctx.fillStyle;
      if (typeof normalized !== "string") return fallback;
      if (normalized.startsWith("#") && normalized.length === 7) {
        return [
          parseInt(normalized.slice(1, 3), 16),
          parseInt(normalized.slice(3, 5), 16),
          parseInt(normalized.slice(5, 7), 16),
        ] as [number, number, number];
      }
      const parts = normalized.match(/[\d.]+/g);
      if (!parts || parts.length < 3) return fallback;
      return [Number(parts[0]), Number(parts[1]), Number(parts[2])] as [
        number,
        number,
        number,
      ];
    }

    /** Sprite de degradado radial: brillo suave sin círculos duros. */
    function buildSprite() {
      const side = 64;
      const off = document.createElement("canvas");
      off.width = side;
      off.height = side;
      const g = off.getContext("2d");
      if (!g) return;
      const rgb = `${ink[0]}, ${ink[1]}, ${ink[2]}`;
      const grad = g.createRadialGradient(
        side / 2,
        side / 2,
        0,
        side / 2,
        side / 2,
        side / 2,
      );
      grad.addColorStop(0, `rgba(${rgb}, 0.95)`);
      grad.addColorStop(0.18, `rgba(${rgb}, 0.55)`);
      grad.addColorStop(0.45, `rgba(${rgb}, 0.16)`);
      grad.addColorStop(0.75, `rgba(${rgb}, 0.04)`);
      grad.addColorStop(1, `rgba(${rgb}, 0)`);
      g.fillStyle = grad;
      g.fillRect(0, 0, side, side);
      sprite = off;
    }

    function readPalette() {
      const styles = getComputedStyle(hostEl);
      const next = resolveRgb(styles.getPropertyValue("--rb-accent"), ink);
      if (
        sprite &&
        next[0] === ink[0] &&
        next[1] === ink[1] &&
        next[2] === ink[2]
      ) {
        return;
      }
      ink = next;
      buildSprite();
    }

    function desiredCount(ring: number) {
      return Math.round(Math.min(130, Math.max(45, ring * 0.5)));
    }

    function makeParticle(): Particle {
      const rf = 1 + (Math.random() + Math.random() - 1) * 0.42;
      return {
        a: Math.random() * Math.PI * 2,
        rf,
        va: (0.05 + Math.random() * 0.07) / rf,
        size: 0.8 + Math.random() * 1.9,
        tw: Math.random() * Math.PI * 2,
        tws: 0.5 + Math.random() * 0.9,
      };
    }

    /** Ajusta la densidad sin tocar las partículas existentes. */
    function fitCount() {
      const ring = Math.min(cssW, cssH) * ringRatio;
      if (ring <= 0) return;
      const want = desiredCount(ring);
      while (parts.length < want) parts.push(makeParticle());
      if (parts.length > want * 1.4) parts.length = want;
    }

    function resize() {
      cssW = hostEl.offsetWidth;
      cssH = hostEl.offsetHeight;
      if (cssW === 0 || cssH === 0) return;
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      canvasEl.width = Math.round(cssW * dpr);
      canvasEl.height = Math.round(cssH * dpr);
      canvasEl.style.width = `${cssW}px`;
      canvasEl.style.height = `${cssH}px`;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      readPalette();
      fitCount();
      if (reduceMotion) draw(prev, 0);
    }

    /** Nodo seleccionado: las partículas cercanas suben de brillo. */
    function activeNodePos(): { x: number; y: number } | null {
      if (!activeId) return null;
      const index = TOOLS.findIndex((tool) => tool.id === activeId);
      if (index < 0) return null;
      const angle = nodeAngle(index);
      const ring = Math.min(cssW, cssH) * ringRatio;
      return {
        x: cssW / 2 + Math.cos(angle) * ring,
        y: cssH / 2 + Math.sin(angle) * ring,
      };
    }

    function draw(t: number, dt: number) {
      ctx.clearRect(0, 0, cssW, cssH);
      if (!sprite) return;
      const ring = Math.min(cssW, cssH) * ringRatio;
      const cx = cssW / 2;
      const cy = cssH / 2;
      const node = activeNodePos();

      for (const p of parts) {
        p.a += p.va * dt;
        const r = p.rf * ring;
        let x = cx + Math.cos(p.a) * r;
        let y = cy + Math.sin(p.a) * r;
        let alpha = 0.14 + 0.4 * (0.5 + 0.5 * Math.sin(t * p.tws + p.tw));

        // Campo del puntero: aparta y arremolina sin alterar la órbita.
        if (inf > 0.002) {
          const dx = x - px;
          const dy = y - py;
          const d = Math.hypot(dx, dy);
          if (d < REACH && d > 0.001) {
            const k = (1 - d / REACH) ** 2 * inf;
            const ux = dx / d;
            const uy = dy / d;
            x += (ux * 22 - uy * 12) * k;
            y += (uy * 22 + ux * 12) * k;
            alpha = Math.min(1, alpha + k * 0.5);
          }
        }

        if (node) {
          const nd = Math.hypot(node.x - x, node.y - y);
          if (nd < 90) alpha = Math.min(1, alpha + (1 - nd / 90) * 0.4);
        }

        const radius = p.size * 2.2;
        ctx.globalAlpha = alpha;
        ctx.drawImage(sprite, x - radius, y - radius, radius * 2, radius * 2);
      }
      ctx.globalAlpha = 1;
    }

    function loop(now: number) {
      if (!running) return;
      const t = now / 1000;
      const dt = prev ? Math.min(0.05, t - prev) : 0.016;
      prev = t;
      const follow = Math.min(1, dt * 10);
      px += (tx - px) * follow;
      py += (ty - py) * follow;
      inf += (infTarget - inf) * Math.min(1, dt * 5);
      draw(t, dt);
      raf = requestAnimationFrame(loop);
    }

    function start() {
      if (reduceMotion || !running) return;
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(loop);
    }

    function onPointerMove(event: PointerEvent) {
      const rect = hostEl.getBoundingClientRect();
      tx = event.clientX - rect.left;
      ty = event.clientY - rect.top;
      // Primer contacto: partir del cursor, no barrer desde (0,0).
      if (inf < 0.01) {
        px = tx;
        py = ty;
      }
      infTarget = 1;
    }

    function onPointerLeave() {
      infTarget = 0;
    }

    function onVisibility() {
      running = !document.hidden;
      if (running) {
        prev = 0;
        start();
      } else {
        cancelAnimationFrame(raf);
      }
    }

    const observer = new ResizeObserver(() => resize());
    observer.observe(hostEl);

    // El tema vive en <html data-theme>; al cambiar hay que releer los tokens.
    const themeObserver = new MutationObserver(() => readPalette());
    themeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });

    hostEl.addEventListener("pointermove", onPointerMove);
    hostEl.addEventListener("pointerleave", onPointerLeave);
    document.addEventListener("visibilitychange", onVisibility);

    resize();
    start();

    return () => {
      running = false;
      cancelAnimationFrame(raf);
      observer.disconnect();
      themeObserver.disconnect();
      hostEl.removeEventListener("pointermove", onPointerMove);
      hostEl.removeEventListener("pointerleave", onPointerLeave);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }
</script>

<div
  class="pw"
  class:is-compact={compact}
  class:is-revealed={revealed}
  bind:this={host}
  onwheel={onWheel}
>
  {#if particles}
    <canvas class="pw-canvas" bind:this={canvas} aria-hidden="true"></canvas>
  {/if}

  {#if compact}
    <GooFilter id="pw-goo" />
  {/if}

  {#if width > 0}
    {#if compact}
      <!-- Piel líquida: el núcleo y una gota por herramienta, fundidos entre
           sí. Va aparte del contenido porque el filtro difumina y vuelve a
           endurecer todo lo que tenga adentro: un icono acá sería una mancha. -->
      <!-- `--d` es el tamaño ANTES del filtro: `preFilter` le descuenta lo
           que el endurecido va a devolver, así que la silueta cerrada mide
           exactamente los 40 del disco de la pill y el morph no salta. -->
      <div class="pw-skin" aria-hidden="true">
        <i
          class="pw-blob"
          style="--d: {preFilter(SKIN.core)}px;
                 --sc: {preFilter(SKIN.closed) / preFilter(SKIN.core)}"
        ></i>
        {#each nodes as node (node.tool.id)}
          <i
            class="pw-blob"
            style="left: {node.x}px; top: {node.y}px;
                   --d: {preFilter(SKIN.node)}px;
                   --tx: {width / 2 - node.x}px; --ty: {height / 2 - node.y}px;
                   --sc: {preFilter(SKIN.closed) / preFilter(SKIN.node)}"
          ></i>
        {/each}
      </div>
    {/if}

    <!-- Gajos: cada botón cubre el cuadrado completo recortado a su sector.
         Todo el gajo es zona de clic; no dibuja líneas ni fondos. -->
    <div class="pw-nodes" role="toolbar" aria-label="Herramientas Atic">
      {#each separators as sep (sep.deg)}
        <span
          class="pw-sep"
          style="height: {sep.len}px; transform: rotate({sep.deg -
            90}deg) translateY({sepInner}px)"
          aria-hidden="true"
        ></span>
      {/each}

      {#each nodes as node, index (node.tool.id)}
        <button
          type="button"
          class="pw-node"
          class:is-hot={activeId === node.tool.id}
          style="clip-path: {node.clip}"
          title="{node.tool.label} — {node.tool.short}"
          bind:this={nodeEls[index]}
          onpointerenter={() => setActive(node.tool.id, { tick: true })}
          onpointerleave={() => {
            // La selección vive solo mientras el puntero está encima: salir
            // del gajo (o de la pill) la limpia. La rueda del ratón puede
            // volver a fijarla, pero soltar con el mouse fuera no activa nada.
            if (activeId === node.tool.id) setActive(null);
          }}
          onfocus={() => setActive(node.tool.id, { tick: true })}
          onblur={() => {
            if (activeId === node.tool.id) setActive(null);
          }}
          onkeydown={onNodeKeydown}
          onclick={() => {
            if (wheelNav) playWheelTick();
            onSelect?.(node.tool.id);
          }}
        >
          <span
            class="pw-node-body"
            style="left: {node.x}px; top: {node.y}px; --i: {index}"
          >
            <!-- 21 y no 17: la gota de 56 pide más tinta que el icono suelto
                 que había sobre el disco. -->
            <span class="pw-node-dot">
              <ToolIcon id={node.tool.id} size={compact ? 21 : 20} />
            </span>
            {#if !compact}
              <span class="pw-node-label">{node.tool.label}</span>
            {/if}
          </span>
        </button>
      {/each}
    </div>

    <div class="pw-center">
      <!-- Núcleo neutro: evita que el centro salte entre gajos y, en la
           pill, es el botón que abre la app. -->
      {#if onCenter}
        <button
          type="button"
          class="pw-core"
          style="width: {coreSize}px; height: {coreSize}px"
          onclick={onCenter}
          aria-label="Abrir Atic"
          title="Abrir Atic"
        >
          <span class="pw-mark">
            <AticMark size={compact ? 22 : 30} />
          </span>
          <span class="pw-caption">
            {compact
              ? (activeTool?.label ?? caption)
              : (activeTool?.short ?? caption)}
          </span>
        </button>
      {:else}
        <div class="pw-core is-inert" style="width: {coreSize}px; height: {coreSize}px">
          <span class="pw-mark">
            <AticMark size={compact ? 22 : 30} />
          </span>
          <!-- En compacto el centro nombra la selección; en grande, la acción. -->
          <span class="pw-caption">
            {compact
              ? (activeTool?.label ?? caption)
              : (activeTool?.short ?? caption)}
          </span>
        </div>
      {/if}
    </div>

    {#if hint && !compact}
      <p class="pw-hint">{hint}</p>
    {/if}
  {/if}
</div>

<style>
  .pw {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    user-select: none;
  }

  .pw-canvas {
    position: absolute;
    inset: 0;
    display: block;
  }

  .pw-center {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
  }

  /* ─── Piel líquida (solo compacto) ──────────────────────────────────── */
  .pw-skin {
    position: absolute;
    inset: 0;
    pointer-events: none;
    filter: url(#pw-goo);
  }
  .pw-blob {
    position: absolute;
    top: 50%;
    left: 50%;
    width: var(--d);
    height: var(--d);
    margin: calc(var(--d) / -2) 0 0 calc(var(--d) / -2);
    border-radius: 999px;
    background: var(--skin);
    /*
     * Cerrado, cada gota vuelve al centro Y al tamaño del disco de la pill.
     *
     * `translate` a la izquierda de `scale` en la cadena significa: escalar
     * primero sobre el propio centro, y recién después mover ese centro al del
     * núcleo. Al revés, el desplazamiento saldría escalado y las gotas no
     * llegarían al centro.
     */
    transform: translate(var(--tx, 0px), var(--ty, 0px)) scale(var(--sc));
    transition: transform var(--morph-close-dur) var(--morph-close-ease);
  }
  .pw.is-revealed .pw-blob {
    transform: none;
    transition: transform var(--morph-open-dur) var(--morph-ease);
  }

  /* Cubre todo el lienzo: los gajos se reparten el rectángulo completo. */
  .pw-nodes {
    position: absolute;
    inset: 0;
  }

  /* Morph: el anillo sale del centro y vuelve a él. La marca del centro no
     escala nunca — es el punto fijo que hace continua la transición con la
     pill cerrada. */
  .pw-nodes,
  .pw-caption {
    transform-origin: 50% 50%;
    transition:
      opacity var(--morph-fade-dur) var(--morph-close-ease),
      transform var(--morph-close-dur) var(--morph-close-ease),
      filter var(--morph-fade-dur) var(--morph-close-ease);
  }

  .pw:not(.is-revealed) .pw-nodes,
  .pw:not(.is-revealed) .pw-caption {
    opacity: 0;
    transform: scale(0.35);
    filter: blur(var(--morph-blur));
    pointer-events: none;
  }

  .pw.is-revealed .pw-nodes,
  .pw.is-revealed .pw-caption {
    opacity: 1;
    transform: scale(1);
    filter: blur(0);
    transition:
      opacity var(--morph-fade-dur) var(--morph-ease),
      transform var(--morph-open-dur) var(--morph-ease),
      filter var(--morph-fade-dur) var(--morph-ease);
  }

  .pw-mark {
    color: var(--rb-text);
    line-height: 0;
  }

  /* Núcleo central: zona neutra circular; sin fondo ni borde. */
  .pw-core {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.85rem;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: inherit;
    text-align: center;
    pointer-events: auto;
  }

  .pw-core:not(.is-inert) {
    cursor: pointer;
    transition:
      opacity var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .pw-core:not(.is-inert):hover {
    opacity: 0.7;
  }

  .pw-core:not(.is-inert):active {
    transform: scale(0.96);
  }

  .pw-core:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  /* Con la rueda colapsada (pill cerrada) el núcleo no debe capturar nada:
     su pointer-events auto explícito perfora el none del contenedor oculto
     y bloqueaba el arrastre de la pill. */
  .pw:not(.is-revealed) .pw-core {
    pointer-events: none;
  }

  /* Pie del hub: enseña que esta misma rueda se invoca sobre cualquier app.
     Es el momento natural para aprenderlo — el usuario la está mirando. */
  .pw-hint {
    position: absolute;
    right: 0;
    bottom: 0.9rem;
    left: 0;
    margin: 0;
    color: var(--rb-faint);
    font-size: 0.5625rem;
    font-weight: 500;
    letter-spacing: 0.1em;
    text-align: center;
    text-transform: uppercase;
    pointer-events: none;
    animation: pw-fade-in 400ms cubic-bezier(0.22, 1, 0.36, 1) backwards;
    animation-delay: 320ms;
  }

  .pw-caption {
    max-width: 18ch;
    color: var(--rb-faint);
    font-size: 0.625rem;
    font-weight: 500;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    text-wrap: balance;
  }

  .pw.is-compact .pw-center {
    gap: 0.6rem;
  }

  /*
   * En compacto no hay pie de texto, y no es un descuido: no entra.
   *
   * Antes cabía porque el rótulo caía sobre un disco de 232 que le daba fondo.
   * Con gotas, el centro mide 58 y "HERRAMIENTAS" pide unos 90; cualquier
   * sobrante queda sobre la ventana TRANSPARENTE, o sea sobre el escritorio,
   * ilegible. Debajo del anillo tampoco entra: las gotas llegan a 98 del
   * centro y `.p-wheel` mide 232.
   *
   * El nombre sigue estando en el `title` de cada gajo. Para recuperarlo en
   * pantalla habría que agrandar la ventana de la rueda (`PILL.wheel`) y
   * colgarle una pastilla fundida abajo.
   */
  .pw.is-compact .pw-caption {
    display: none;
  }

  /* Las gotas ya dividen el espacio: las fronteras solo agregan ruido. */
  .pw.is-compact .pw-sep {
    display: none;
  }

  .pw.is-compact .pw-node-dot {
    width: 2.1rem;
    height: 2.1rem;
  }

  /* Frontera entre gajos: 1px que se desvanece en ambos extremos. */
  .pw-sep {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 1px;
    margin-left: -0.5px;
    transform-origin: 0 0;
    background: linear-gradient(
      to bottom,
      transparent,
      color-mix(in srgb, var(--rb-text) 10%, transparent) 18%,
      color-mix(in srgb, var(--rb-text) 10%, transparent) 45%,
      transparent 96%
    );
    pointer-events: none;
  }

  .pw:not(.is-compact) .pw-sep {
    animation: pw-fade-in 320ms cubic-bezier(0.22, 1, 0.36, 1) backwards;
    animation-delay: 180ms;
  }

  /* El gajo: invisible, cubre todo su sector; solo existe como zona de
     interacción. */
  .pw-node {
    position: absolute;
    inset: 0;
    margin: 0;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: pointer;
  }

  /* El contenido del gajo: icono + etiqueta, transparente, sin líneas.
     Al entrar al sector crece un poco. */
  .pw-node-body {
    position: absolute;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.3rem;
    color: var(--rb-muted);
    pointer-events: none;
    transform: translate(-50%, -50%);
    transition:
      transform var(--duration-quick) var(--ease-smooth-out),
      color var(--duration-quick) var(--ease-smooth-out);
  }

  /* Entrada del hub: los gajos aparecen en abanico alrededor de la marca.
     fill backwards y no both: al terminar deben mandar las transiciones. */
  .pw:not(.is-compact) .pw-node-body {
    animation: pw-node-in var(--duration-medium) var(--ease-smooth-out) backwards;
    animation-delay: calc(var(--i, 0) * 40ms + 40ms);
  }

  .pw:not(.is-compact) .pw-center {
    animation: pw-fade-in var(--duration-fast) var(--ease-smooth-out) backwards;
  }

  .pw-node-dot {
    display: flex;
    width: 2.5rem;
    height: 2.5rem;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    color: inherit;
  }

  .pw-node-label {
    color: var(--rb-faint);
    font-size: 0.625rem;
    font-weight: 500;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    white-space: nowrap;
    transition: color var(--duration-quick) var(--ease-smooth-out);
  }

  .pw-node.is-hot .pw-node-body {
    color: var(--rb-text);
    transform: translate(-50%, -50%) scale(1.05);
  }

  .pw-node.is-hot .pw-node-label {
    color: var(--rb-text);
  }

  .pw-node:active .pw-node-body {
    transform: translate(-50%, -50%) scale(0.96);
  }

  .pw-node:focus-visible {
    outline: none;
  }

  .pw-node:focus-visible .pw-node-dot {
    box-shadow: var(--rb-focus);
  }

  @keyframes pw-node-in {
    from {
      opacity: 0;
      transform: translate(-50%, -50%) scale(var(--morph-scale));
      filter: blur(var(--blur-medium));
    }
    to {
      opacity: 1;
      transform: translate(-50%, -50%) scale(1);
      filter: blur(0);
    }
  }

  @keyframes pw-fade-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .pw-core:not(.is-inert),
    .pw-node-body,
    .pw-center,
    .pw-node-dot,
    .pw-node-label,
    .pw-nodes,
    .pw-caption,
    .pw-blob,
    .pw.is-revealed .pw-nodes,
    .pw.is-revealed .pw-caption,
    .pw.is-revealed .pw-blob {
      animation: none !important;
      transition: none !important;
    }

    .pw:not(.is-revealed) .pw-nodes,
    .pw:not(.is-revealed) .pw-caption {
      filter: none;
    }
  }
</style>
