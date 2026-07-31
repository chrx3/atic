<script lang="ts">
  /**
   * Banco de pruebas del sistema líquido. Solo dev.
   *
   * Existe para contestar una pregunta que `Features/liquid.md` deja abierta:
   * si el filtro de fusión se comporta igual dentro de WebView2 que en Chrome,
   * que es donde se diseñó. La respuesta condiciona toda la capa `liquid/` de
   * la reescritura, así que se mide antes de escribirla.
   *
   * A diferencia de `docs/demos/`, que son una implementación aparte, esto usa
   * el `GooFilter` de producción y la misma geometría de cuello que
   * `AgentsSurface`. Lo que se ve acá es lo que hace la app.
   *
   * Qué se mide:
   *
   *   - **Alcance.** Apagando "dibujar cuello" quedan dos formas sueltas y el
   *     único puente posible es el que pone el filtro. El hueco donde se cortan
   *     tiene que ser `1.72·σ`. Si no lo es, WebView2 no está interpretando el
   *     endurecido igual (el sospechoso es `color-interpolation-filters`).
   *   - **Engorde.** El contorno punteado marca la geometría EXACTA pedida. La
   *     silueta filtrada tiene que morir encima de él: si asoma, `preFilter()`
   *     no está compensando lo que este motor engorda.
   *   - **Costo.** El filtro declara una región de `-50%/200%`, o sea cuatro
   *     veces el área de la caja. "Animar" mueve la geometría en cada cuadro,
   *     que es el peor caso, y el interruptor del filtro da el A/B contra el
   *     mismo trabajo sin filtrar.
   */
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import GooFilter, { GOO_GROW, preFilter } from "$lib/GooFilter.svelte";
  import { Field, sminBulge, sminReach, type Shape } from "$lib/liquid/sdf";
  import { fieldToPath } from "$lib/liquid/contour";

  let { standalone = false, onClose }: {
    /** Fuera del overlay no hay escritorio detrás: hace falta un fondo propio. */
    standalone?: boolean;
    onClose?: () => void;
  } = $props();

  /* ─── Geometría, copiada de AgentsSurface a propósito ───────────────────
   *
   * Copiada y no importada: el lab tiene que poder contradecir a la app. Si un
   * día estos números se separan de los de allá, el que manda es el de allá y
   * este archivo está desactualizado.
   */
  const NECK_THICK = 26;
  const NECK_THIN = 10;
  const NECK_MIN_THICK = 6;
  const NECK_MAX = 140;
  const BUBBLE_CORNER = 26;

  /** Medidas reales: la barra de la pill y el cuerpo de la consola. */
  const PILL_W = 176;
  const PILL_H = 40;
  const BUB_W = 580;
  const BUB_H = 520;

  type Box = { x: number; y: number; w: number; h: number };

  /**
   * Los dos renderizadores, sobre exactamente la misma geometría.
   *
   * `goo` es el de producción: difuminar el alfa y volver a endurecerlo, con lo
   * que eso implica (depende del motor, engorda 0.28σ por lado, y no cruza más
   * de 1.72σ por su cuenta).
   *
   * `sdf` calcula el campo de distancia, lo une con `smin` y traza el contorno.
   * No depende del motor, no engorda, y el alcance lo fija `k` en vez de estar
   * atado a la viscosidad. La comparación es el punto de este banco.
   */
  let renderer = $state<"goo" | "sdf">("goo");

  let sigma = $state(5);
  let drawNeck = $state(true);
  let filterOn = $state(true);
  let showOutline = $state(true);
  let animate = $state(false);

  /** Mezcla del `smin`. Es al SDF lo que σ es al filtro. */
  let blend = $state(26);
  /**
   * Lado de la celda de muestreo.
   *
   * Marching squares no ve nada más fino que su celda, y el cuello de esta
   * escena baja a 6 px: por eso el valor por defecto es 3 y no el 6 que se usa
   * en escenas de tarjetas. El costo va con el cuadrado.
   */
  let cell = $state(3);
  let smooth = $state(2);

  /** El alcance del filtro por su cuenta, sin cuello dibujado. */
  const reach = $derived(1.72 * sigma);

  let vw = $state(1280);
  let vh = $state(800);

  /**
   * Dónde vive la escena. Las dos formas se arrastran por separado.
   *
   * El overlay cubre el escritorio VIRTUAL, así que centrar en el viewport deja
   * la junta partida justo en la unión entre dos monitores — que es el único
   * sitio donde no se puede mirar. Rust sabe dónde está cada pantalla:
   * `overlay_work_areas` devuelve sus rectángulos en los mismos píxeles CSS del
   * overlay que usa `pillCssStage`.
   */
  type XY = { x: number; y: number };
  let pillAt = $state<XY | null>(null);
  let bubAt = $state<XY | null>(null);
  let animPhase = $state(0);

  let areas = $state<Box[]>([]);
  let areaIdx = $state(0);

  const pill = $derived<Box>({
    x: pillAt?.x ?? Math.round(vw / 2 - PILL_W / 2),
    y: pillAt?.y ?? Math.round(vh * 0.72),
    w: PILL_W,
    h: PILL_H,
  });

  const bubble = $derived<Box>({
    x: bubAt?.x ?? Math.round(pill.x + PILL_W / 2 - BUB_W / 2),
    y:
      (bubAt?.y ?? pill.y - BUB_H - 10) +
      (animate ? Math.round(Math.sin(animPhase) * 60 - 60) : 0),
    w: BUB_W,
    h: BUB_H,
  });

  /** Planta la escena en una pantalla: pill abajo, globo encima. */
  function place(area: Box) {
    const lo = area.y + 40 + BUB_H + 10;
    const hi = area.y + area.h - PILL_H - 40;
    const y = hi >= lo ? Math.min(Math.max(area.y + area.h * 0.72, lo), hi) : lo;
    const cx = area.x + area.w / 2;
    pillAt = { x: Math.round(cx - PILL_W / 2), y: Math.round(y) };
    bubAt = { x: Math.round(cx - BUB_W / 2), y: Math.round(y - BUB_H - 10) };
    resetMetrics();
  }

  function nextArea() {
    if (areas.length === 0) return;
    areaIdx = (areaIdx + 1) % areas.length;
    place(areas[areaIdx]);
  }

  /* ─── El puente, con la misma cuenta que la app ─────────────────────────── */

  const span = $derived.by(() => {
    const a = bubble;
    const p = pill;
    const gapX = Math.max(p.x - (a.x + a.w), a.x - (p.x + p.w));
    const gapY = Math.max(p.y - (a.y + a.h), a.y - (p.y + p.h));
    const vertical =
      gapY > gapX ||
      (gapY === gapX &&
        Math.abs(p.y + p.h / 2 - (a.y + a.h / 2)) >
          Math.abs(p.x + p.w / 2 - (a.x + a.w / 2)));

    const gap = vertical ? gapY : gapX;
    const pillFirst = vertical
      ? p.y + p.h / 2 < a.y + a.h / 2
      : p.x + p.w / 2 < a.x + a.w / 2;
    const pillEdge = vertical
      ? pillFirst
        ? p.y + p.h
        : p.y
      : pillFirst
        ? p.x + p.w
        : p.x;
    const bubEdge = vertical
      ? pillFirst
        ? a.y + a.h
        : a.y
      : pillFirst
        ? a.x + a.w
        : a.x;

    const pillLo = vertical ? p.x : p.y;
    const pillHi = vertical ? p.x + p.w : p.y + p.h;
    const bubLo = (vertical ? a.x : a.y) + BUBBLE_CORNER;
    const bubHi = (vertical ? a.x + a.w : a.y + a.h) - BUBBLE_CORNER;
    const lo = Math.max(pillLo, bubLo);
    const hi = Math.min(pillHi, bubHi);

    return {
      vertical,
      gap,
      pillEdge,
      bubEdge: vertical
        ? pillFirst
          ? a.y
          : a.y + a.h
        : pillFirst
          ? a.x
          : a.x + a.w,
      stretch: Math.min(Math.max(gap, 0) / NECK_MAX, 1),
      center:
        lo > hi
          ? (lo + hi) / 2
          : Math.min(Math.max((pillLo + pillHi) / 2, lo), hi),
      overlap:
        Math.min(pillHi, vertical ? a.x + a.w : a.y + a.h) -
        Math.max(pillLo, vertical ? a.x : a.y),
    };
  });

  const joined = $derived(span.gap <= NECK_MAX && span.overlap >= NECK_THIN);

  const skinBox = $derived.by<Box>(() => {
    const a = bubble;
    const p = joined ? pill : null;
    const x = p ? Math.min(a.x, p.x) : a.x;
    const y = p ? Math.min(a.y, p.y) : a.y;
    return {
      x,
      y,
      w: (p ? Math.max(a.x + a.w, p.x + p.w) : a.x + a.w) - x,
      h: (p ? Math.max(a.y + a.h, p.y + p.h) : a.y + a.h) - y,
    };
  });

  function local(r: Box): Box {
    return { ...r, x: r.x - skinBox.x, y: r.y - skinBox.y };
  }

  /** Encogida lo que el endurecido le va a devolver. */
  function preFiltered(r: Box): Box {
    const l = local(r);
    return { x: l.x + GOO_GROW, y: l.y + GOO_GROW, w: preFilter(r.w), h: preFilter(r.h) };
  }

  /** El cuello en coordenadas del overlay. El SDF lo necesita sin trasladar. */
  const neckAbs = $derived.by(() => {
    if (!drawNeck || !joined) return null;
    const s = span;
    const thick = Math.max(
      NECK_MIN_THICK,
      NECK_THICK + (NECK_THIN - NECK_THICK) * s.stretch,
    );
    const dir = Math.sign(s.bubEdge - s.pillEdge) || 1;
    const from = s.pillEdge - dir * 9;
    const to = s.bubEdge + dir * 7;
    const lo = Math.min(from, to);
    const long = Math.abs(to - from);
    return s.vertical
      ? { x: s.center - thick / 2, y: lo, w: thick, h: long }
      : { x: lo, y: s.center - thick / 2, w: long, h: thick };
  });

  const neck = $derived(neckAbs ? local(neckAbs) : null);

  const pillBlob = $derived({ ...preFiltered(pill), r: PILL_H / 2 });
  const bubBlob = $derived({ ...preFiltered(bubble), r: BUBBLE_CORNER - GOO_GROW });

  /** Cuántos píxeles rasteriza el filtro: la región es -50%/200%, o sea 4×. */
  const regionMpx = $derived((skinBox.w * 2 * (skinBox.h * 2)) / 1_000_000);

  /* ─── El renderizador SDF ───────────────────────────────────────────────
   *
   * Las mismas tres formas, descritas como campos en vez de como divs. Acá NO
   * se aplica `preFilter()`: el contorno pasa por la geometría pedida, que es
   * justamente una de las cosas que este banco tiene que comprobar contra el
   * contorno punteado.
   */
  const sdf = $derived.by(() => {
    if (renderer !== "sdf") return null;

    const shapes: Shape[] = [
      {
        kind: "box",
        cx: pill.x + pill.w / 2,
        cy: pill.y + pill.h / 2,
        hw: pill.w / 2,
        hh: pill.h / 2,
        r: PILL_H / 2,
      },
      {
        kind: "box",
        cx: bubble.x + bubble.w / 2,
        cy: bubble.y + bubble.h / 2,
        hw: bubble.w / 2,
        hh: bubble.h / 2,
        r: BUBBLE_CORNER,
      },
    ];

    // El cuello explícito es opcional a propósito: la pregunta interesante es
    // si `blend` solo ya cruza el hueco sin necesidad de dibujarlo.
    const n = neckAbs;
    if (n) {
      const r = Math.min(n.w, n.h) / 2;
      shapes.push(
        n.w > n.h
          ? { kind: "capsule", ax: n.x + r, ay: n.y + n.h / 2, bx: n.x + n.w - r, by: n.y + n.h / 2, r }
          : { kind: "capsule", ax: n.x + n.w / 2, ay: n.y + r, bx: n.x + n.w / 2, by: n.y + n.h - r, r },
      );
    }

    const t0 = performance.now();
    const path = fieldToPath(new Field(shapes, blend), { cell, smooth });
    return { ...path, ms: Math.round((performance.now() - t0) * 100) / 100 };
  });

  /* ─── Medición de cuadros ───────────────────────────────────────────────── */

  let fps = $state(0);
  let p95 = $state(0);
  let worst = $state(0);

  function resetMetrics() {
    times.length = 0;
    fps = 0;
    p95 = 0;
    worst = 0;
  }

  const times: number[] = [];

  onMount(() => {
    const measure = () => {
      vw = window.innerWidth;
      vh = window.innerHeight;
    };
    measure();
    window.addEventListener("resize", measure);

    // La más grande primero: con monitores desiguales, es donde se mira.
    void invoke<Box[]>("overlay_work_areas")
      .then((list) => {
        areas = [...list].sort((a, b) => b.w * b.h - a.w * a.h);
        if (areas[0]) place(areas[0]);
      })
      .catch(() => {
        // Fuera de Tauri no hay monitores que preguntar: el viewport es todo.
        place({ x: 0, y: 0, w: window.innerWidth, h: window.innerHeight });
      });

    let last = performance.now();
    let raf = 0;
    const tick = (now: number) => {
      const dt = now - last;
      last = now;
      if (dt > 0 && dt < 1000) {
        times.push(dt);
        if (times.length > 180) times.shift();
      }
      if (animate) animPhase += 0.03;
      if (times.length > 20) {
        const sorted = [...times].sort((a, b) => a - b);
        const avg = times.reduce((s, t) => s + t, 0) / times.length;
        fps = Math.round(1000 / avg);
        p95 = Math.round(sorted[Math.floor(sorted.length * 0.95)] * 10) / 10;
        worst = Math.round(sorted[sorted.length - 1] * 10) / 10;
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);

    return () => {
      window.removeEventListener("resize", measure);
      cancelAnimationFrame(raf);
    };
  });

  /* ─── Arrastre del globo ────────────────────────────────────────────────── */

  type DragId = "pill" | "bub";
  let from: { id: DragId; x: number; y: number; ox: number; oy: number } | null = null;

  function startDrag(id: DragId) {
    return (event: PointerEvent) => {
      if (event.button !== 0) return;
      event.preventDefault();
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
      // Del origen guardado, no del derivado: el del globo lleva sumado el
      // vaivén de "animar" y el arrastre daría un salto al empezar.
      const at = id === "pill" ? pillAt : bubAt;
      from = {
        id,
        x: event.clientX,
        y: event.clientY,
        ox: at?.x ?? (id === "pill" ? pill.x : bubble.x),
        oy: at?.y ?? (id === "pill" ? pill.y : bubble.y),
      };
      resetMetrics();
    };
  }

  function moveDrag(event: PointerEvent) {
    if (!from) return;
    const next = {
      x: Math.round(from.ox + (event.clientX - from.x)),
      y: Math.round(from.oy + (event.clientY - from.y)),
    };
    if (from.id === "pill") pillAt = next;
    else bubAt = next;
  }

  function endDrag(event: PointerEvent) {
    if (!from) return;
    from = null;
    (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
  }

  /** Acerca o aleja el globo sin arrastrar, para buscar el corte al píxel. */
  function nudge(dy: number) {
    const at = bubAt ?? { x: bubble.x, y: bubble.y };
    bubAt = { x: at.x, y: at.y - dy };
    resetMetrics();
  }
</script>

<div class="lab" class:is-standalone={standalone}>
  <GooFilter id="lab-goo" {sigma} />

  {#if renderer === "sdf"}
    <!-- La piel como una sola forma trazada. Sin filtro: es geometría. -->
    {#if sdf && sdf.d}
      <svg
        class="sdf"
        style:left="{sdf.minX}px"
        style:top="{sdf.minY}px"
        width={sdf.width}
        height={sdf.height}
        viewBox="{sdf.minX} {sdf.minY} {sdf.width} {sdf.height}"
        aria-hidden="true"
      >
        <!-- `evenodd` porque los lazos no se orientan de forma consistente: con
             la regla por defecto, una isla interior se rellenaría. -->
        <path d={sdf.d} fill="#1c1917" fill-rule="evenodd" />
      </svg>
    {/if}
  {:else}
  <!-- La piel: siluetas y nada más. Filtrada. -->
  <div
    class="skin"
    style:left="{skinBox.x}px"
    style:top="{skinBox.y}px"
    style:width="{skinBox.w}px"
    style:height="{skinBox.h}px"
    style:filter={filterOn
      ? `url(#lab-goo) drop-shadow(0 18px 30px rgba(0,0,0,.45))`
      : "none"}
    aria-hidden="true"
  >
    <i
      class="blob"
      style:left="{pillBlob.x}px"
      style:top="{pillBlob.y}px"
      style:width="{pillBlob.w}px"
      style:height="{pillBlob.h}px"
      style:border-radius="{pillBlob.r}px"
    ></i>
    {#if neck}
      <i
        class="blob"
        style:left="{neck.x}px"
        style:top="{neck.y}px"
        style:width="{neck.w}px"
        style:height="{neck.h}px"
        style:border-radius="{Math.min(neck.w, neck.h) / 2}px"
      ></i>
    {/if}
    <i
      class="blob"
      style:left="{bubBlob.x}px"
      style:top="{bubBlob.y}px"
      style:width="{bubBlob.w}px"
      style:height="{bubBlob.h}px"
      style:border-radius="{bubBlob.r}px"
    ></i>
  </div>
  {/if}

  <!-- Contornos exactos, sin filtrar: la piel tiene que morir justo encima. -->
  {#if showOutline}
    <i
      class="outline"
      style:left="{pill.x}px"
      style:top="{pill.y}px"
      style:width="{pill.w}px"
      style:height="{pill.h}px"
      style:border-radius="{PILL_H / 2}px"
    ></i>
    <i
      class="outline"
      style:left="{bubble.x}px"
      style:top="{bubble.y}px"
      style:width="{bubble.w}px"
      style:height="{bubble.h}px"
      style:border-radius="{BUBBLE_CORNER}px"
    ></i>
  {/if}

  <!-- Zonas de arrastre. Las dos formas se mueven por separado: el hueco y el
       eje de la junta son justo lo que hay que poder cambiar a mano. -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="grab"
    style:left="{bubble.x}px"
    style:top="{bubble.y}px"
    style:width="{bubble.w}px"
    style:height="{bubble.h}px"
    onpointerdown={startDrag("bub")}
    onpointermove={moveDrag}
    onpointerup={endDrag}
    onpointercancel={endDrag}
  >
    <span class="grab-hint">globo</span>
  </div>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="grab"
    style:left="{pill.x}px"
    style:top="{pill.y}px"
    style:width="{pill.w}px"
    style:height="{pill.h}px"
    onpointerdown={startDrag("pill")}
    onpointermove={moveDrag}
    onpointerup={endDrag}
    onpointercancel={endDrag}
  ></div>

  <div class="panel">
    <header>
      <strong>liquid lab</strong>
      <span class="tag">{standalone ? "ventana normal" : "overlay real"}</span>
    </header>

    <div class="seg">
      <button
        type="button"
        class:on={renderer === "goo"}
        onclick={() => { renderer = "goo"; resetMetrics(); }}>filtro goo</button
      >
      <button
        type="button"
        class:on={renderer === "sdf"}
        onclick={() => { renderer = "sdf"; resetMetrics(); }}>sdf</button
      >
    </div>

    {#if renderer === "goo"}
      <label class="row">
        <span>viscosidad σ</span>
        <input type="range" min="0" max="20" step="0.5" bind:value={sigma} />
        <b>{sigma.toFixed(1)}</b>
      </label>
    {:else}
      <label class="row">
        <span>mezcla k</span>
        <input type="range" min="0" max="90" step="1" bind:value={blend} />
        <b>{blend}</b>
      </label>
      <label class="row">
        <span>celda</span>
        <input type="range" min="1" max="12" step="1" bind:value={cell} />
        <b>{cell} px</b>
      </label>
      <label class="row">
        <span>suavizado</span>
        <input type="range" min="0" max="4" step="1" bind:value={smooth} />
        <b>{smooth}</b>
      </label>
    {/if}

    <div class="grid">
      <span>hueco real</span>
      <b class:bad={renderer === "goo" && span.gap > reach && !drawNeck}>
        {span.gap.toFixed(1)} px
      </b>
      {#if renderer === "goo"}
        <span>alcance 1.72·σ</span><b>{reach.toFixed(1)} px</b>
        <span>predicción</span>
        <b>
          {#if drawNeck}
            {joined ? "cuello dibujado" : "cortado"}
          {:else}
            {span.gap <= reach ? "debe fundir" : "debe cortar"}
          {/if}
        </b>
        <span>región filtro</span><b>{regionMpx.toFixed(2)} Mpx</b>
      {:else if sdf}
        <span>alcance k/2</span>
        <b class:bad={span.gap > sminReach(blend) && !drawNeck}>
          {sminReach(blend).toFixed(1)} px
        </b>
        <span>bulto k/4</span><b>{sminBulge(blend).toFixed(1)} px</b>
        <span>evaluadas</span>
        <b class:bad={sdf.evals > 40_000}>
          {(sdf.evals / 1000).toFixed(1)}k de {(sdf.samples / 1000).toFixed(0)}k
        </b>
        <span>celda real</span>
        <b class:bad={sdf.cell > cell}>{sdf.cell.toFixed(1)} px</b>
        <span>puntos</span><b>{sdf.points}</b>
        <span>cálculo</span><b class:bad={sdf.ms > 8}>{sdf.ms} ms</b>
      {/if}
      <span>grosor cuello</span>
      <b>{neck ? Math.min(neck.w, neck.h).toFixed(1) + " px" : "—"}</b>
      <span>caja</span><b>{skinBox.w}×{skinBox.h}</b>
      <span>fps</span><b class:bad={fps > 0 && fps < 50}>{fps || "—"}</b>
      <span>cuadro p95</span><b class:bad={p95 > 20}>{p95 || "—"} ms</b>
      <span>peor cuadro</span><b>{worst || "—"} ms</b>
    </div>

    <div class="nudges">
      <span>hueco</span>
      <button type="button" onclick={() => nudge(-1)}>−1</button>
      <button type="button" onclick={() => nudge(1)}>+1</button>
      <button type="button" onclick={() => nudge(-5)}>−5</button>
      <button type="button" onclick={() => nudge(5)}>+5</button>
    </div>

    <div class="nudges">
      <button type="button" onclick={() => areas[areaIdx] && place(areas[areaIdx])}>
        recentrar
      </button>
      {#if areas.length > 1}
        <button type="button" onclick={nextArea}>
          pantalla {areaIdx + 1}/{areas.length}
        </button>
      {/if}
    </div>

    <label class="check">
      <input type="checkbox" bind:checked={drawNeck} />
      {renderer === "sdf" ? "cuello explícito" : "dibujar cuello"}
    </label>
    {#if renderer === "goo"}
      <label class="check"><input type="checkbox" bind:checked={filterOn} /> aplicar filtro</label>
    {/if}
    <label class="check"><input type="checkbox" bind:checked={showOutline} /> contorno exacto</label>
    <label class="check">
      <input type="checkbox" bind:checked={animate} onchange={resetMetrics} /> animar (peor caso)
    </label>

    {#if onClose}
      <button type="button" class="close" onclick={onClose}>cerrar lab</button>
    {/if}
  </div>
</div>

<style>
  .lab {
    position: fixed;
    inset: 0;
    touch-action: none;
    user-select: none;
    font-family: var(--rb-mono, monospace);
    color: #e7e2dd;
  }

  /* En el overlay el fondo es el escritorio; suelto hace falta uno. */
  .is-standalone {
    background:
      radial-gradient(60% 60% at 30% 20%, #2f3a63 0%, transparent 60%),
      linear-gradient(160deg, #14161f, #0d1016 60%, #090b10);
  }

  .skin {
    position: absolute;
    pointer-events: none;
  }

  .blob {
    position: absolute;
    display: block;
    /* Regla 2 del sistema líquido: todo lo que se funde, del mismo color. */
    background: #1c1917;
  }

  /* La sombra va sobre el path ya trazado, igual que va después del goo. */
  .sdf {
    position: absolute;
    overflow: visible;
    pointer-events: none;
    filter: drop-shadow(0 18px 30px rgba(0, 0, 0, 0.45));
  }

  .seg {
    display: flex;
    gap: 4px;
  }

  .seg button {
    flex: 1;
  }

  .seg button.on {
    background: #da7756;
    border-color: #da7756;
    color: #1c1917;
  }

  .outline {
    position: absolute;
    pointer-events: none;
    border: 1px dashed rgba(255, 120, 60, 0.9);
    box-sizing: border-box;
  }

  .grab {
    position: absolute;
    cursor: grab;
    background: transparent;
  }

  .grab:active {
    cursor: grabbing;
  }

  .grab-hint {
    position: absolute;
    inset: auto 0 12px 0;
    text-align: center;
    font-size: 11px;
    color: #6b615a;
  }

  .panel {
    position: absolute;
    top: 24px;
    left: 24px;
    width: 236px;
    padding: 12px;
    display: grid;
    gap: 8px;
    background: rgba(18, 18, 22, 0.9);
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 12px;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .panel header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }

  .tag {
    color: #8d827a;
  }

  .row {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 4px 8px;
    align-items: center;
  }

  .row input {
    grid-column: 1 / -1;
    width: 100%;
  }

  .grid {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 3px 8px;
  }

  .grid span,
  .row span {
    color: #8d827a;
  }

  .grid b {
    color: #e7e2dd;
    font-weight: 500;
  }

  .bad {
    color: #da7756;
  }

  .nudges {
    display: flex;
    gap: 4px;
    align-items: center;
  }

  .nudges span {
    color: #8d827a;
    margin-right: 2px;
  }

  button {
    font: inherit;
    color: #e7e2dd;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 6px;
    padding: 3px 6px;
    cursor: pointer;
  }

  button:hover {
    background: rgba(255, 255, 255, 0.16);
  }

  .check {
    display: flex;
    gap: 6px;
    align-items: center;
    cursor: pointer;
  }

  .close {
    margin-top: 4px;
  }
</style>
