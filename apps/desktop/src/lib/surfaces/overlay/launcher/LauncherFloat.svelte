<script lang="ts">
  import { tip } from "$surfaces/overlay/tip.svelte";
  /**
   * Float del launcher Spotlight.
   *
   * Apertura (nacimiento centrado, sin viaje):
   * 1) La pill vuela al slot (PillSurface) y deja libre el centro.
   * 2) La barra nace en su centro: fade + grow en el sitio, con la silueta a
   *    cargo del líquido (el mismo gesto que los favs).
   * 3) Cada favorito se desprende de a uno.
   *
   * Cierre = espejo: tuck favs → repliegue en el centro (fade + shrink) →
   * dismiss; la pill vuelve a casa al terminar (PillSurface espera el hit-rect).
   */
  import { onMount, tick, untrack } from "svelte";
  import type { BubbleOpen, LauncherHit } from "$core/types";
  import {
    hideLauncher,
    launcherListFavorites,
    launcherListRecents,
    launcherQuit,
    launcherRun,
    launcherSearch,
    launcherToggleFavorite,
    onLauncherBubbleAnchor,
    onLauncherBubbleDismiss,
    onLauncherOpened,
  } from "$ipc/search";
  import {
    onOverlayDismiss,
    overlayWorkAreas,
    overlayActiveAnchor,
    setOverlayTextMode,
  } from "$ipc/overlay";
  import type { Area } from "$ipc/overlay";
  import { Bubble } from "$surfaces/overlay/bubble.svelte";
  import { createBubbleDrag } from "$surfaces/overlay/bubbleDrag";
  import { resolveSlot } from "$surfaces/overlay/toolSlots";
  import { gapBetween, pillShape } from "$lib/liquid/geometry";
  import { REACH } from "$lib/liquid/constants";
  import { sminReach, type Shape } from "$lib/liquid/sdf";
  import { launcherLab } from "$lib/dev/launcherLab.svelte";
  import { liquid, LIQUID_HUB } from "$surfaces/overlay/group.svelte";
  import {
    publishEmergeSkin,
    publishFollowSkin,
    publishMeasuredSkin,
  } from "$surfaces/overlay/floatEmergeSkin";
  import { rectKey } from "$surfaces/overlay/floatEmergeSkinMath";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";
  import { notifyToolResting, toolBirth } from "$surfaces/overlay/toolBirth";
  import {
    armOpenDismissGrace,
    isOpenDismissGrace,
  } from "$surfaces/overlay/openDismissGrace";
  import { afterTransition, MOTION, ms, prefersReducedMotion, wait } from "$lib/motion";
  import LauncherIcon from "$surfaces/launcher/LauncherIcon.svelte";
  import Icon from "$ui/Icon.svelte";
  import { t } from "$domain/i18n.svelte";
  import { spanFrom } from "$surfaces/overlay/pill/pillQuota";
  import Kbd from "$ui/Kbd.svelte";
  import { Star, X } from "$lib/icons";

  const isDev = import.meta.env.DEV;
  /** Radio SDF: mitad del alto compacto → disco limpio en la semilla. */
  const CORNER = 20;
  /**
   * Hueco barra→favs en reposo. > REACH (10px) para cortar el cuello en idle;
   * al desprenderse queda bajo REACH un instante → fusión líquida.
   */
  const FAVS_GAP_PX = 15;
  /**
   * Hueco entre dots. > REACH en idle → bolitas individuales, no óvalo.
   */
  const DOT_GAP_PX = 15;
  /**
   * Semilla del nacimiento: disco del alto de la pill (40 px), centrado donde
   * vivirá la barra. De ahí **estira** al stadium final.
   */
  const BIRTH_SEED_PX = 40;
  /** Aparición de la gota: fade + scale, el mismo gesto que los favs. */
  const BIRTH_DROP_DUR_MS = 120;
  /** Beat en la gota antes de estirar (ms): deja leer el nacimiento. */
  const BIRTH_HOLD_MS = 60;
  /**
   * Estirón: 40 → 324 px de ancho.
   *
   * 200 ms (y no los 100 del grow de panel) porque acá el recorrido es 8× el
   * ancho original: el nacimiento tiene que **verse**, no adivinarse.
   */
  const BIRTH_DUR_MS = 200;
  /** Alto ancla compacto (= pill 40px; alineado a `LAUNCHER_SHAPE` en launcher.rs). */
  const COMPACT_H = 40;
  const EXPANDED_H = 360;
  const bubble = new Bubble();
  let el = $state<HTMLElement | null>(null);
  /** Toolbar de favs: hit-rect propio (viven fuera del ancho del float). */
  let favsEl = $state<HTMLElement | null>(null);
  const { startDrag, endDrag } = createBubbleDrag(bubble, () => el);
  let workAreas = $state<Area[]>([]);
  /** Último ancla: re-colocar cuando llegan work areas (evita race multi-monitor). */
  let lastOpen: BubbleOpen | null = null;

  let query = $state("");
  let hits = $state<LauncherHit[]>([]);
  let recents = $state<LauncherHit[]>([]);
  let favorites = $state<LauncherHit[]>([]);
  let favoriteIds = $state<string[]>([]);
  let favoritesLoaded = false;
  let recentsLoaded = false;
  let favoritesInFlight: Promise<void> | null = null;
  let recentsInFlight: Promise<void> | null = null;
  let loadEpoch = 0;
  let selected = $state(0);
  let searching = $state(false);
  let error = $state("");
  let input = $state<HTMLInputElement | null>(null);
  let generation = 0;
  /** Debounce IPC: cada tecla no debe invocar `launcher_search`. */
  let searchTimer: ReturnType<typeof setTimeout> | null = null;
  const SEARCH_DEBOUNCE_MS = 120;

  /**
   * Coreografía del float:
   * open:  hidden → birth → favs → ready
   * close: tuck → recede → (dismiss)
   *
   * El reveal pasa en el sitio: la barra nace centrada, no viaja formada.
   */
  type RevealPhase = "hidden" | "birth" | "favs" | "ready" | "tuck" | "recede";
  let revealPhase = $state<RevealPhase>("hidden");
  /** Rect de reposo del nacimiento: el que la gota estira. */
  let restRect: { x: number; y: number; w: number; h: number } | null = null;
  let revealEpoch = 0;
  /** Animación de morph en curso: se cancela al reabrir o forzar dismiss. */
  let revealAnim: Animation | null = null;
  /** Evita reentrar close / segundo Esc fuerza dismiss. */
  let closing = false;
  /** `hideLauncher` re-dispara dismiss IPC: ignorar ese eco. */
  let ignoreIpcDismiss = false;
  /** Cuántos favs ya salieron (0 = ninguno; N = primeros N visibles). */
  let favRevealCount = $state(0);
  /** Nace o se repliega: silueta líquida y chrome apagado. */
  const revealing = $derived(revealPhase === "birth" || revealPhase === "recede");
  const favsSequencing = $derived(
    revealPhase === "favs" || revealPhase === "ready" || revealPhase === "tuck",
  );
  const favsStaggering = $derived(revealPhase === "favs" || revealPhase === "tuck");
  const motionPhase = $derived(revealing || revealPhase === "tuck");

  const hasQuery = $derived(query.trim().length > 0);
  const list = $derived(hasQuery ? hits : recents);
  const showResults = $derived(hasQuery || recents.length > 0);
  /** El resultado seleccionado es una app: Ctrl+Enter la cierra. */
  const selectedIsApp = $derived(list[selected]?.kind === "app");

  const favGap = $derived(isDev && launcherLab.open ? launcherLab.favGap : FAVS_GAP_PX);
  const dotGap = $derived(isDev && launcherLab.open ? launcherLab.dotGap : DOT_GAP_PX);
  const labOpenDur = $derived(
    isDev && launcherLab.open ? launcherLab.openDur : BIRTH_DUR_MS,
  );
  const labCloseDur = $derived(isDev && launcherLab.open ? launcherLab.closeDur : 120);
  const compactH = $derived(isDev && launcherLab.open ? launcherLab.barH : COMPACT_H);
  const recentsHeight = $derived(compactH + 28 + recents.length * 44 + 10);
  const reach = $derived(
    isDev && launcherLab.open ? sminReach(launcherLab.blend) : REACH,
  );

  let openDur = $state(BIRTH_DUR_MS);
  let closeDur = $state(120);
  let favStaggerDur = $state(90);

  function armOpenDur() {
    openDur = labOpenDur;
    favStaggerDur = ms(MOTION.launcherFavStagger);
  }

  function armCloseDur() {
    closeDur = labCloseDur;
  }

  function cancelReveal() {
    revealEpoch += 1;
    revealAnim?.cancel();
    revealAnim = null;
  }

  /**
   * Nacimiento: la barra ya está colocada como gota en su centro; acá se la
   * deja leer un beat y **estira** hasta el stadium (width + left, mismo centro).
   * Los favs salen después; no hay viaje: la pill se corre al slot.
   */
  async function runOpenReveal() {
    const epoch = ++revealEpoch;
    if (prefersReducedMotion()) {
      // Sin morph: la barra va directo a su rect de reposo.
      revealPhase = "ready";
      if (lastOpen) await applyCenterPlace(lastOpen);
      await ensureFavoritesLoaded();
      await ensureRecentsLoaded();
      favRevealCount = favorites.length;
      return;
    }

    revealPhase = "birth";
    favRevealCount = 0;
    // Favs/recientes viajan en paralelo al grow: cuando la barra se asienta,
    // el primer peel ya tiene datos.
    const hydration = (async () => {
      await ensureFavoritesLoaded();
      await ensureRecentsLoaded();
    })();
    // La gota aparece con el mismo gesto que los favs (fade + scale desde el
    // centro). WAAPI: los keyframes son explícitos, no dependen de un frame
    // previo pintado.
    await animateEl(
      el,
      [
        { opacity: 0, transform: "scale(0.82)" },
        { opacity: 1, transform: "none" },
      ],
      BIRTH_DROP_DUR_MS,
      {
        easeVar: "--ease-smooth-out",
        easeFallback: "cubic-bezier(0.22, 1, 0.36, 1)",
      },
    );
    if (epoch !== revealEpoch) return;
    // Beat: deja leer la gota antes del estirón.
    await wait(BIRTH_HOLD_MS);
    if (epoch !== revealEpoch) return;
    await stretchToRest(openDur);
    if (epoch !== revealEpoch) return;
    await hydration;
    if (epoch !== revealEpoch) return;

    if (favorites.length === 0) {
      revealPhase = "ready";
      return;
    }

    // Favs: un solo cambio de estado; CSS aplica el delay escalonado vía --lf-i.
    revealPhase = "favs";
    favRevealCount = favorites.length;
    await tick();
    const lastDot = el?.querySelector(`.lf-dot:nth-child(${favorites.length})`);
    if (lastDot instanceof HTMLElement) {
      await afterTransition(lastDot, "transform", favStaggerDur * favorites.length);
    } else {
      await wait(favStaggerDur * favorites.length);
    }
    if (epoch !== revealEpoch) return;
    revealPhase = "ready";
  }

  /**
   * Close = espejo del open: tuck favs → repliegue en el centro → caller dismiss.
   * No pone `hidden` (eso re-dispararía open mientras `shown`).
   */
  async function runCloseReveal(epoch: number): Promise<void> {
    if (prefersReducedMotion()) {
      favRevealCount = 0;
      return;
    }

    // Panel de resultados: volver a stadium compacto antes del repliegue.
    if (query.trim() || showResults) {
      clearSearchTimer();
      query = "";
      clearHits();
      await tick();
      if (epoch !== revealEpoch) return;
    }

    if (favRevealCount > 0) {
      revealPhase = "tuck";
      favRevealCount = 0;
      await tick();
      const firstDot = el?.querySelector(".lf-dot");
      if (firstDot instanceof HTMLElement) {
        await afterTransition(firstDot, "transform", favStaggerDur * favorites.length);
      } else {
        await wait(favStaggerDur * favorites.length);
      }
      if (epoch !== revealEpoch) return;
    }

    // Repliegue: espejo del nacimiento, en el mismo centro (sin viaje a la pill).
    revealPhase = "recede";
    await tick();
    if (epoch !== revealEpoch) return;
    await shrinkToSeed(closeDur);
  }

  function livePillRect() {
    return surfaces.live["pill-skin"] ?? surfaces.live["pill"];
  }

  /** Centro de la pill (o del nacimiento) para elegir el monitor del reveal. */
  function pillCenter(): { x: number; y: number } | null {
    const pill = toolBirth() ?? livePillRect();
    return pill ? { x: pill.x + pill.w / 2, y: pill.y + pill.h / 2 } : null;
  }

  /**
   * Gota del nacimiento: cápsula de `BIRTH_SEED_PX` de ancho, al alto de la
   * barra y centrada en el rect de reposo. Con el alto compacto (40) es un
   * disco —el tamaño de la pill—; y como el alto no cambia, el estirón es
   * puramente horizontal (no hay salto vertical posible).
   */
  function seedRect(rest: { x: number; y: number; w: number; h: number }) {
    const d = Math.min(rest.w, BIRTH_SEED_PX);
    return {
      x: rest.x + (rest.w - d) / 2,
      y: rest.y,
      w: d,
      h: rest.h,
    };
  }

  /**
   * Anima el float y resuelve al terminar.
   *
   * WAAPI y no una transición CSS a propósito: los keyframes llevan las medidas
   * explícitas (40 → 324 px), así que la animación no depende de que el frame
   * anterior haya quedado pintado ni de que una clase arme la transición a
   * tiempo — las dos formas de perder el morph. `finished` es el hecho.
   * La curva sale del token del proyecto (`--ease-liquid` por defecto).
   *
   * `hold` fija el último keyframe (`fill: forwards`) y no cancela: es lo que
   * necesita el repliegue para llegar al dismiss ya apagado, sin un frame de
   * barra a opacidad plena.
   */
  async function animateEl(
    node: HTMLElement | null,
    frames: Keyframe[],
    dur: number,
    opts: { easeVar?: string; easeFallback?: string; hold?: boolean } = {},
  ): Promise<void> {
    if (!node || dur <= 0) return;
    const ease =
      getComputedStyle(node)
        .getPropertyValue(opts.easeVar ?? "--ease-liquid")
        .trim() ||
      opts.easeFallback ||
      "cubic-bezier(0.5, 0, 0.2, 1)";
    const anim = node.animate(frames, {
      duration: dur,
      easing: ease,
      fill: opts.hold ? "forwards" : "both",
    });
    revealAnim = anim;
    try {
      await anim.finished;
    } catch {
      // Cancelada por close/reopen: no hay nada que esperar.
    } finally {
      if (revealAnim === anim) revealAnim = null;
      if (!opts.hold) anim.cancel();
    }
  }

  /** Estira la gota al rect de reposo: width + left, mismo centro. */
  async function stretchToRest(dur: number): Promise<void> {
    const rest = restRect;
    const cur = bubble.anchor;
    if (!rest || !cur) return;
    const from = { width: `${cur.w}px`, left: `${cur.x}px` };
    const to = { width: `${rest.w}px`, left: `${rest.x}px` };
    bubble.place({
      side: cur.side as BubbleOpen["side"],
      offset: rest.h / 2,
      x: rest.x,
      y: rest.y,
      w: rest.w,
      h: rest.h,
    });
    await animateEl(el, [from, to], dur);
  }

  /**
   * Reverse: repliega el stadium a la gota, en el centro donde está parado, y la
   * apaga ahí mismo — espejo del nacimiento (rect + fade + scale).
   */
  async function shrinkToSeed(dur: number): Promise<void> {
    const cur = bubble.anchor;
    if (!cur) return;
    const seed = seedRect(cur);
    const from = {
      width: `${cur.w}px`,
      left: `${cur.x}px`,
      opacity: "1",
      transform: "scale(1)",
    };
    const to = {
      width: `${seed.w}px`,
      left: `${seed.x}px`,
      opacity: "0",
      transform: "scale(0.82)",
    };
    bubble.place({
      side: cur.side as BubbleOpen["side"],
      offset: seed.h / 2,
      x: seed.x,
      y: seed.y,
      w: seed.w,
      h: seed.h,
    });
    await animateEl(el, [from, to], dur, { hold: true });
  }

  /**
   * Centro del monitor (horizontal y vertical) con las work areas que ya
   * tenemos: la barra nace ahí mismo, sin esperar un IPC.
   *
   * Con `seed` queda guardado el rect de reposo y se coloca solo la gota: el
   * estirón llega después, cuando el reveal lo dispare.
   *
   * El monitor lo marca el nacimiento (Ctrl+Q = mouse), no la pill ya de
   * vuelta en el notch.
   */
  function placeAtCenter(
    a: BubbleOpen,
    anchor: { x: number; y: number },
    seed: boolean,
  ): void {
    const labCompact = isDev && launcherLab.open && !showResults;
    const w = labCompact ? launcherLab.barW : a.w;
    const h = labCompact ? compactH : a.h;
    const pos = resolveSlot("center", workAreas, { w, h }, anchor);
    restRect = { x: pos.x, y: pos.y, w, h };
    const rect = seed ? seedRect(restRect) : restRect;
    bubble.place({
      ...a,
      w: rect.w,
      h: rect.h,
      x: rect.x,
      y: rect.y,
      side: "left",
      offset: rect.h / 2,
    });
    notifyToolResting();
  }

  /** Nacimiento: gota centrada, con lo que ya sabemos, en el frame del atajo. */
  function centerPlace(a: BubbleOpen): void {
    placeAtCenter(a, pillCenter() ?? { x: a.x + a.w / 2, y: a.y + a.h / 2 }, true);
  }

  /**
   * Centro con work areas frescas (el monitor puede llegar después del atajo).
   * Idempotente: si el rect no cambió, `bubble.place` no re-monta el reveal.
   *
   * Mientras el reveal no cerró (fase distinta de `ready`) coloca la **gota**;
   * a mitad del morph no toca nada: un rect distinto ahí es un salto visible.
   */
  async function applyCenterPlace(a: BubbleOpen) {
    try {
      workAreas = await overlayWorkAreas();
    } catch {
      // Fuera de Tauri o IPC fallido: se usa lo último que haya.
    }
    // Bail temprano: si el reveal ya arrancó no vale la pena pedir el ancla.
    if (revealPhase !== "hidden" && revealPhase !== "ready") return;
    let anchor = pillCenter();
    if (!anchor) {
      try {
        anchor = (await overlayActiveAnchor()) ?? {
          x: a.x + a.w / 2,
          y: a.y + a.h / 2,
        };
      } catch {
        anchor = { x: a.x + a.w / 2, y: a.y + a.h / 2 };
      }
    }
    // Última verificación antes de mover: entre los awaits la fase pudo cambiar
    // (birth/favs/tuck/recede) y un rect distinto ahí es un salto visible.
    if (revealPhase !== "hidden" && revealPhase !== "ready") return;
    placeAtCenter(a, anchor, revealPhase !== "ready");
  }

  async function placeFromPill(a: BubbleOpen) {
    lastOpen = a;
    const fresh = !bubble.alive || !bubble.shown;
    if (fresh) {
      armOpenDur();
      armOpenDismissGrace();
    }
    if (workAreas.length === 0) {
      try {
        workAreas = await overlayWorkAreas();
      } catch {
        workAreas = [];
      }
    }
    if (lastOpen !== a) return;
    // Nace en su centro: se coloca sync (el reveal arranca en el primer frame)
    // y el refine de work areas llega después, idempotente si el monitor no
    // cambió. Nunca recentrar a mitad del reveal: salta.
    if (fresh || revealPhase === "hidden") {
      centerPlace(a);
      void applyCenterPlace(a);
      return;
    }
    if (revealPhase === "ready" && !closing) {
      await applyCenterPlace(a);
    }
  }

  /** Cuando la barra monta `.is-shown`, arranca el grow (no durante close). */
  $effect(() => {
    if (!bubble.alive) {
      if (revealPhase !== "hidden") {
        revealPhase = "hidden";
        favRevealCount = 0;
      }
      closing = false;
      return;
    }
    if (bubble.shown && revealPhase === "hidden" && !closing) {
      void runOpenReveal();
    }
  });

  /** Crece/achica el float sin los mínimos de la consola de agentes. */
  function fitHeight(h: number) {
    const a = bubble.anchor;
    if (!a) return;
    const nh = Math.round(h);
    if (a.h === nh) return;
    bubble.anchor = {
      ...a,
      h: nh,
      y: a.side === "bottom" ? a.y + a.h - nh : a.y,
    };
  }

  const pillSkin = $derived(surfaces.live["pill-skin"]);
  const joined = $derived.by(() => {
    const a = bubble.anchor;
    const p = pillSkin;
    if (!a || !p || !bubble.alive) return false;
    return gapBetween(p, a) <= reach;
  });

  /**
   * Compacto con favoritos: barra + dots en el goo.
   * Al abrir, los dots nacen pegados a la barra (gap < REACH → cuello) y se
   * alejan hasta favGap (> REACH → el smin corta). Entre dots el gap es
   * dotGap (> REACH) para que queden círculos separados, no un óvalo.
   *
   * Idle: no rAF eterno (epsilon + tope en `publishMeasuredSkin`). Solo se
   * despierta al abrir/cerrar/mover gaps o ancla.
   */
  function publishCompactPills(root: HTMLElement, _group?: string): () => void {
    return publishMeasuredSkin("launcher", () => {
      const shapes: Shape[] = [];
      const parts: string[] = [];
      const head = root.querySelector(".lf-head");
      if (head instanceof HTMLElement) {
        const r = head.getBoundingClientRect();
        if (r.width > 0 && r.height > 0) {
          const rect = { x: r.x, y: r.y, w: r.width, h: r.height };
          parts.push(`h:${rectKey(rect)}`);
          shapes.push(pillShape(rect));
        }
      }
      // Solo bolitas ya reveladas (secuencial).
      root.querySelectorAll(".lf-dot.is-out").forEach((node, i) => {
        if (!(node instanceof HTMLElement)) return;
        const r = node.getBoundingClientRect();
        if (r.width <= 0 || r.height <= 0) return;
        const rect = { x: r.x, y: r.y, w: r.width, h: r.height };
        parts.push(`d${i}:${rectKey(rect)}`);
        shapes.push(pillShape(rect));
      });
      return { key: parts.join("|"), shapes };
    });
  }

  $effect(() => {
    if (!bubble.alive || !el) {
      liquid.publish("launcher", []);
      return;
    }
    void bubble.shown;
    void favorites.length;
    void showResults;
    void revealPhase;
    void favRevealCount;
    void favGap;
    void dotGap;
    void bubble.anchor;
    const group = motionPhase || joined ? LIQUID_HUB : undefined;
    // Panel de resultados: chrome opaco (`.is-expanded`); no remeshear SDF
    // en cada tecla / transición de alto — era el trancazo al buscar.
    if (showResults) {
      liquid.publish("launcher", []);
      return;
    }
    // Nacimiento/repliegue: la barra se pinta sola mientras crece (la caja es la
    // que estira). Sin silueta líquida propia: no hay cuello con la pill y
    // duplicaría el blob.
    if (revealing) {
      liquid.publish("launcher", []);
      return;
    }
    // Tuck de peels: siguen la geometría cuadro a cuadro.
    if (motionPhase) {
      return publishFollowSkin("launcher", el, CORNER, group);
    }
    if (favorites.length > 0 && favRevealCount > 0) {
      return publishCompactPills(el, group);
    }
    return publishEmergeSkin("launcher", el, CORNER, group);
  });

  $effect(() => {
    if (bubble.shown) surfaces.bringToFront("launcher");
  });

  $effect(() => {
    if (!el || !bubble.alive) return;
    const stop = surfaces.add("launcher", el);
    void surfaces.flush();
    return stop;
  });
  /**
   * Hit-rect de favs cuando ya hay al menos una bolita afuera.
   * También con panel de resultados: los dots siguen a la derecha del stadium.
   */
  $effect(() => {
    if (
      !favsEl ||
      !bubble.alive ||
      !bubble.shown ||
      favRevealCount <= 0 ||
      favorites.length === 0
    ) {
      return;
    }
    const stop = surfaces.add("launcher-favs", favsEl);
    void surfaces.flush();
    return stop;
  });
  $effect(() => {
    if (!bubble.alive || !bubble.shown) return;
    void bubble.anchor;
    void surfaces.recoverHits();
    const t = window.setTimeout(
      () => {
        void surfaces.recoverHits();
      },
      ms(MOTION.floatOpen) + 48,
    );
    return () => window.clearTimeout(t);
  });
  $effect(() => {
    void bubble.anchor;
    void surfaces.dragging;
    void favorites.length;
    void showResults;
    if (surfaces.dragging) return;
    surfaces.schedule();
  });

  $effect(() => {
    if (!bubble.alive) return;
    const idleRecents = !hasQuery && recents.length > 0 && revealPhase === "ready";
    const open = hasQuery || idleRecents;
    fitHeight(open ? (hasQuery ? EXPANDED_H : recentsHeight) : compactH);
  });

  /**
   * Lab: re-aplicar ancho/alto compacto al mover barW/barH.
   * Solo en ready (no pelear con el reveal).
   */
  $effect(() => {
    if (
      !isDev ||
      !launcherLab.open ||
      !bubble.alive ||
      !bubble.shown ||
      showResults ||
      revealPhase !== "ready"
    ) {
      return;
    }
    void launcherLab.barW;
    void compactH;
    const a = lastOpen;
    if (!a) return;
    untrack(() => {
      void applyCenterPlace({ ...a, w: launcherLab.barW, h: compactH });
    });
  });

  async function loadFavorites(epoch = loadEpoch) {
    try {
      const next = await launcherListFavorites();
      if (epoch !== loadEpoch) return;
      favorites = next;
      favoriteIds = next.map((f) => f.id);
    } catch {
      if (epoch !== loadEpoch) return;
      favorites = [];
      favoriteIds = [];
    } finally {
      if (epoch === loadEpoch) favoritesLoaded = true;
    }
  }

  async function ensureFavoritesLoaded() {
    if (favoritesLoaded) return;
    const epoch = loadEpoch;
    if (!favoritesInFlight) {
      const promise = loadFavorites(epoch).finally(() => {
        if (favoritesInFlight === promise) favoritesInFlight = null;
      });
      favoritesInFlight = promise;
    }
    await favoritesInFlight;
  }

  async function refreshFavorites() {
    favoritesLoaded = false;
    await loadFavorites(loadEpoch);
  }

  async function loadRecents(epoch = loadEpoch) {
    try {
      const next = await launcherListRecents();
      if (epoch !== loadEpoch) return;
      recents = next;
    } catch {
      if (epoch !== loadEpoch) return;
      recents = [];
    } finally {
      if (epoch === loadEpoch) recentsLoaded = true;
    }
  }

  async function ensureRecentsLoaded() {
    if (recentsLoaded) return;
    const epoch = loadEpoch;
    if (!recentsInFlight) {
      const promise = loadRecents(epoch).finally(() => {
        if (recentsInFlight === promise) recentsInFlight = null;
      });
      recentsInFlight = promise;
    }
    await recentsInFlight;
  }

  function recencyLabel(hit: LauncherHit): string {
    const now = Date.now();
    if (hit.foreground) return t("overlay.inUse");
    if (hit.running && hit.openedAt) {
      const age = now - hit.openedAt;
      if (age < 60_000) return t("overlay.inUse");
      const span = spanFrom(age);
      return t("overlay.openedAgo", {
        when: `${span.value} ${t(`overlay.span.${span.unit}`)}`,
      });
    }
    if (hit.lastUsedAt) {
      const age = now - hit.lastUsedAt;
      if (age < 60_000) return t("overlay.justNow");
      const span = spanFrom(age);
      return t("overlay.usedAgo", {
        when: `${span.value} ${t(`overlay.span.${span.unit}`)}`,
      });
    }
    return t("overlay.recents");
  }

  function clearSearchTimer() {
    if (searchTimer !== null) {
      clearTimeout(searchTimer);
      searchTimer = null;
    }
  }

  function clearHits() {
    generation += 1;
    hits = [];
    searching = false;
    error = "";
    selected = 0;
  }

  async function search(text: string) {
    const trimmed = text.trim();
    if (!trimmed) {
      generation += 1;
      hits = [];
      searching = false;
      selected = 0;
      return;
    }
    const mine = ++generation;
    searching = true;
    error = "";
    try {
      const next = await launcherSearch(trimmed);
      if (mine !== generation) return;
      hits = next;
      selected = 0;
    } catch (failure) {
      if (mine !== generation) return;
      error = failure instanceof Error ? failure.message : String(failure);
      hits = [];
    } finally {
      if (mine === generation) searching = false;
    }
  }

  /** Vacío al instante; query con debounce (mismo ritmo que SearchModal). */
  function scheduleSearch(text: string) {
    clearSearchTimer();
    if (!text.trim()) {
      clearHits();
      return;
    }
    searchTimer = setTimeout(() => {
      searchTimer = null;
      void search(text);
    }, SEARCH_DEBOUNCE_MS);
  }

  /**
   * Enfoca el input de búsqueda si la barra ya está usable (`.is-shown`).
   * Antes: `set_overlay_text_mode` — el overlay nace `focusable: false`
   * (WS_EX_NOACTIVATE); sin eso `input.focus()` no recibe teclas hasta un clic.
   * No pelea si el usuario ya enfocó algo dentro del launcher (fav, clear…).
   */
  async function focusSearch(select = false): Promise<boolean> {
    const node = input;
    if (!node || !bubble.shown) return false;
    if (document.activeElement === node) {
      if (select) node.select();
      return true;
    }
    const active = document.activeElement;
    if (active instanceof HTMLElement && el?.contains(active) && active !== node) {
      return false;
    }
    try {
      await setOverlayTextMode(true);
    } catch {
      // Fuera de Tauri no hay ventana a la que pedirle el foco.
    }
    if (!bubble.shown) return false;
    node.focus({ preventScroll: true });
    if (select) node.select();
    return document.activeElement === node;
  }

  async function reset(select = false) {
    clearSearchTimer();
    query = "";
    hits = [];
    searching = false;
    error = "";
    selected = 0;
    // Favoritos/recientes se coalescen: anchor/opened/reveal pueden cruzarse al abrir.
    await ensureFavoritesLoaded();
    await ensureRecentsLoaded();
    await tick();
    // Si aún no hay `.is-shown`, el $effect de abajo toma el foco al abrir.
    await focusSearch(select);
  }

  /**
   * En cuanto hay barra visible + input: modo texto del overlay + foco.
   * Reintentos cortos: `force_foreground` corre en hilo aparte y el morph
   * a veces devuelve el foco al host en los primeros frames.
   */
  $effect(() => {
    if (!bubble.shown || !input) return;
    // Re-correr al cambiar de fase: recuperación si algo robó el foco.
    if (revealPhase === "hidden" || closing) return;
    void revealPhase;

    let cancelled = false;
    const timers: ReturnType<typeof setTimeout>[] = [];

    const tryFocus = () => {
      if (cancelled || !bubble.shown) return;
      void focusSearch(false);
    };

    void tick().then(() => {
      if (cancelled) return;
      tryFocus();
      timers.push(setTimeout(tryFocus, 48));
      // force_foreground es async en Rust: un reintento más tarde.
      timers.push(setTimeout(tryFocus, 120));
      if (revealPhase === "ready") {
        timers.push(setTimeout(tryFocus, 0));
      }
    });

    return () => {
      cancelled = true;
      for (const t of timers) clearTimeout(t);
    };
  });

  async function run(id?: string) {
    const target = id ?? list[selected]?.id;
    if (!target) return;
    try {
      await launcherRun(target);
    } catch (failure) {
      error = failure instanceof Error ? failure.message : String(failure);
    }
  }

  /**
   * Ctrl+Enter sobre una app: cerrarla con `WM_CLOSE` (graceful: la app y el
   * sistema deciden, incluido preguntar por cambios sin guardar). La barra queda
   * abierta para cerrar varias seguidas.
   */
  async function quitSelected() {
    const hit = list[selected];
    if (!hit || hit.kind !== "app") return;
    try {
      await launcherQuit(hit.id);
      // Cambió el estado «en uso»: refrescar la lista sin cerrar la barra.
      if (hasQuery) {
        void search(query);
      } else {
        recentsLoaded = false;
        void ensureRecentsLoaded();
      }
    } catch (failure) {
      error = failure instanceof Error ? failure.message : String(failure);
    }
  }

  async function toggleFavorite(id: string, event?: Event) {
    event?.stopPropagation();
    try {
      favoriteIds = await launcherToggleFavorite(id);
      await refreshFavorites();
      // Sin esto las bolitas existen en el DOM pero con opacity 0 (`is-out`
      // exige i < favRevealCount). Había que cerrar y reabrir para verlas.
      favRevealCount = favorites.length;
      await tick();
      surfaces.schedule();
    } catch (failure) {
      error = failure instanceof Error ? failure.message : String(failure);
      await refreshFavorites();
      favRevealCount = favorites.length;
    }
  }

  function isFavorite(id: string) {
    return favoriteIds.includes(id);
  }

  function finishDismiss(wasShown: boolean, opts: { skipHideLauncher?: boolean } = {}) {
    clearSearchTimer();
    loadEpoch += 1;
    favoritesLoaded = false;
    recentsLoaded = false;
    favoritesInFlight = null;
    recentsInFlight = null;
    lastOpen = null;
    favRevealCount = 0;
    revealPhase = "hidden";
    endDrag();
    surfaces.resetInteraction();
    if (input && document.activeElement === input) {
      input.blur();
    }
    armCloseDur();
    bubble.hide();
    if (!wasShown) bubble.alive = false;
    if (!opts.skipHideLauncher) {
      ignoreIpcDismiss = true;
      void hideLauncher().finally(() => {
        window.setTimeout(() => {
          ignoreIpcDismiss = false;
        }, 320);
      });
    }
    closing = false;
  }

  async function close(opts: { fromIpcDismiss?: boolean } = {}) {
    if (!bubble.shown && !bubble.alive) return;
    // Eco de hideLauncher o dismiss mientras ya cerramos.
    if (closing) {
      if (opts.fromIpcDismiss) return;
      // Segundo Esc durante reverse: abortar morph y dismiss inmediato.
      cancelReveal();
      finishDismiss(bubble.shown, { skipHideLauncher: opts.fromIpcDismiss });
      return;
    }
    closing = true;
    const wasShown = bubble.shown;
    armCloseDur();
    clearSearchTimer();
    endDrag();
    surfaces.resetInteraction();
    if (input && document.activeElement === input) {
      input.blur();
    }
    const epoch = ++revealEpoch;
    await runCloseReveal(epoch);
    // Si un Esc forzado ya dismiss-ó, no repetir.
    if (!closing) return;
    if (epoch !== revealEpoch) {
      closing = false;
      return;
    }
    finishDismiss(wasShown, { skipHideLauncher: opts.fromIpcDismiss });
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      // Recuperación: aunque el float esté a medias, Esc corta drag + cierra.
      event.preventDefault();
      surfaces.resetInteraction();
      if (bubble.shown || bubble.alive) void close();
      return;
    }
    if (!bubble.shown) return;
    if (event.key === "ArrowDown" && list.length > 0) {
      event.preventDefault();
      selected = (selected + 1) % list.length;
    } else if (event.key === "ArrowUp" && list.length > 0) {
      event.preventDefault();
      selected = (selected - 1 + list.length) % list.length;
    } else if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void quitSelected();
    } else if (event.key === "Enter") {
      event.preventDefault();
      void run();
    }
  }

  onMount(() => {
    void overlayWorkAreas()
      .then((areas) => {
        workAreas = areas;
        // Si el ancla llegó antes que las áreas, el fallback era el viewport
        // virtual entero → el float quedaba corrido. No recentrar durante el
        // reveal: el nacimiento ya aterrizó centrado y re-placearlo salta.
        if (lastOpen && bubble.alive && revealPhase === "ready") {
          void applyCenterPlace(lastOpen);
        } else if (
          lastOpen &&
          bubble.alive &&
          (revealPhase === "hidden" || revealPhase === "birth") &&
          !bubble.shown
        ) {
          centerPlace(lastOpen);
        }
      })
      .catch(() => {
        workAreas = [];
      });
    const un: Promise<() => void>[] = [
      onLauncherBubbleAnchor((a) => {
        // Acto 2 ya: no esperar favs (eso hacía “snap” tras la carga).
        void placeFromPill(a);
        void reset(true);
      }),
      onLauncherBubbleDismiss(() => {
        if (ignoreIpcDismiss) return;
        void close({ fromIpcDismiss: true });
      }),
      onLauncherOpened(() => void reset(true)),
      onOverlayDismiss(() => {
        surfaces.resetInteraction();
        if (isOpenDismissGrace()) return;
        if (bubble.shown || bubble.alive) void close();
      }),
    ];
    window.addEventListener("keydown", onKeydown);
    return () => {
      window.removeEventListener("keydown", onKeydown);
      clearSearchTimer();
      endDrag();
      surfaces.resetInteraction();
      for (const p of un) void p.then((fn) => fn());
      liquid.publish("launcher", []);
    };
  });
</script>

{#if bubble.alive}
  <div
    class="lf"
    class:is-shown={bubble.shown}
    class:is-joined={joined}
    class:is-expanded={hasQuery || (recents.length > 0 && revealPhase === "ready")}
    class:is-revealing={revealing}
    class:is-favs-seq={favsSequencing}
    class:is-favs-stagger={favsStaggering}
    class:is-tucking={revealPhase === "tuck"}
    data-float="launcher"
    data-side={bubble.anchor?.side ?? "left"}
    style={bubble.vars}
    style:--float-stack={surfaces.stack("launcher")}
    style:--lf-fav-gap="{favGap}px"
    style:--lf-dot-gap="{dotGap}px"
    style:--launcher-fav-stagger="{favStaggerDur}ms"
    style:--lf-fav-last-index={Math.max(favorites.length - 1, 0)}
    bind:this={el}
    role="dialog"
    aria-label={t("overlay.searchApps")}
  >
    <div class="lf-bar" class:has-favs={favorites.length > 0}>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <header class="lf-head" onpointerdown={startDrag}>
        <span class="lf-search-icon" aria-hidden="true">
          <LauncherIcon id="" kind="" />
        </span>
        <input
          bind:this={input}
          bind:value={query}
          oninput={() => scheduleSearch(query)}
          onpointerdown={(e) => e.stopPropagation()}
          type="text"
          placeholder={t("overlay.searchPlaceholder")}
          aria-label={t("overlay.searchApps")}
          autocomplete="off"
          spellcheck="false"
          class="lf-input"
          data-no-drag
        />
        {#if searching}
          <span class="lf-busy" data-numeric aria-hidden="true">…</span>
        {:else if query}
          <button
            type="button"
            class="lf-icon"
            aria-label={t("overlay.clearSearch")}
            data-no-drag
            onclick={() => void reset()}
          >
            <Icon icon={X} size={12} />
          </button>
        {/if}
      </header>
      {#if favorites.length > 0}
        <div
          class="lf-favs"
          role="toolbar"
          aria-label={t("overlay.favorites")}
          data-no-drag
          bind:this={favsEl}
        >
          {#each favorites as fav, i (fav.id)}
            <button
              type="button"
              class="lf-dot"
              class:is-action={fav.kind === "action"}
              class:is-out={i < favRevealCount}
              style:--lf-i={i}
              use:tip={fav.title}
              aria-label={t("overlay.openApp", { title: fav.title })}
              tabindex={i < favRevealCount ? 0 : -1}
              onpointerdown={(e) => e.stopPropagation()}
              onclick={() => void run(fav.id)}
            >
              <LauncherIcon id={fav.id} kind={fav.kind} size={20} />
            </button>
          {/each}
        </div>
      {/if}
    </div>

    {#if error}
      <p class="lf-err" role="alert">{error}</p>
    {/if}

    {#if showResults && (hasQuery || revealPhase === "ready")}
      {#if !hasQuery}
        <p class="lf-heading">{t("overlay.recents")}</p>
      {/if}
      <ul
        class="lf-list"
        role="listbox"
        aria-label={hasQuery ? t("overlay.results") : t("overlay.recents")}
      >
        {#each list as hit, i (hit.id)}
          <li>
            <div class="lf-hit" class:is-sel={i === selected}>
              <button
                type="button"
                role="option"
                aria-selected={i === selected}
                class="lf-hit-main"
                onmouseenter={() => (selected = i)}
                onclick={() => {
                  selected = i;
                  void run(hit.id);
                }}
              >
                <span
                  class="lf-hit-ico"
                  class:is-action={hit.kind === "action"}
                  aria-hidden="true"
                >
                  <LauncherIcon id={hit.id} kind={hit.kind} size={18} />
                </span>
                <span class="lf-hit-text">
                  <span class="lf-hit-title">{hit.title}</span>
                  <span class="lf-hit-sub"
                    >{hasQuery ? hit.subtitle : recencyLabel(hit)}</span
                  >
                </span>
              </button>
              {#if !hit.id.startsWith("calc:")}
                <button
                  type="button"
                  class="lf-star"
                  class:is-on={isFavorite(hit.id)}
                  data-no-drag
                  aria-label={isFavorite(hit.id)
                    ? t("overlay.favRemove", { title: hit.title })
                    : t("overlay.favAdd", { title: hit.title })}
                  aria-pressed={isFavorite(hit.id)}
                  onpointerdown={(e) => e.stopPropagation()}
                  onclick={(e) => void toggleFavorite(hit.id, e)}
                >
                  <Icon
                    icon={Star}
                    size={14}
                    fill={isFavorite(hit.id) ? "currentColor" : "none"}
                  />
                </button>
              {/if}
            </div>
          </li>
        {:else}
          <li class="lf-empty">
            {#if searching}
              {t("overlay.searching")}
            {:else}
              {t("overlay.noResults")}
            {/if}
          </li>
        {/each}
      </ul>

      <footer class="lf-foot">
        <span class="lf-hint"><Kbd combo="↑↓" /> {t("overlay.navHint")}</span>
        <span class="lf-hint"><Kbd combo="Enter" /> {t("overlay.openHint")}</span>
        <span class="lf-hint"><Kbd combo="Esc" /> {t("overlay.closeHint")}</span>
        {#if selectedIsApp}
          <span class="lf-hint"><Kbd combo="Ctrl+Enter" /> {t("overlay.quitHint")}</span
          >
        {/if}
      </footer>
    {/if}
  </div>
{/if}

<style>
  /*
   * Nace y muere en su centro (fade + grow), con la silueta a cargo del
   * líquido; el chrome recién aparece cuando la barra se asienta.
   */
  .lf {
    /* Duraciones locales (el inline las pisa con las del lab de dev): stagger de
       los favs y entrada del chrome cuando la barra se asienta. */
    --launcher-fav-stagger: 90ms;
    --lf-chrome-dur: 120ms;

    position: absolute;
    z-index: calc(var(--z-overlay-float) + var(--float-stack, 0));
    display: flex;
    flex-direction: column;
    left: var(--x);
    top: var(--y);
    width: var(--w);
    height: var(--h);
    min-width: 0;
    min-height: 0;
    box-sizing: border-box;
    border-radius: 999px;
    background: transparent;
    color: var(--text);
    overflow: hidden;
    opacity: 0;
    pointer-events: none;

    /* Sin transition de height: al buscar, saltar a EXPANDED_H evita thrash
       (layout + hit-rects) en cada tecla; el nacimiento anima transform. */
  }

  .lf.is-shown {
    opacity: 1;
    pointer-events: auto;
  }

  /*
   * Morph en curso (nacimiento o repliegue): el rect, el fade y el scale los
   * mueve JS (`animateEl`, WAAPI) y acá solo se recorta el contenido y se apaga
   * el chrome, así lo que se ve crecer/replegarse es la barra y nada más.
   */
  .lf.is-revealing {
    overflow: hidden;
  }

  /*
   * Nacimiento/repliegue: acá la barra crece sola y el chrome todavía no
   * corresponde. Un head visible en una caja de 40 px taparía la silueta y el
   * input se leería stadium desde el primer frame.
   */
  .lf.is-revealing .lf-head {
    opacity: 0;
    pointer-events: none;
    background: transparent;
  }

  /* Expandido: panel único con surface. Compacto: chrome transparente.
   * overflow visible: los favs viven fuera (absolute a la der. del stadium). */
  .lf.is-expanded {
    background: var(--skin);
    overflow: visible;
    border-radius: 18px;
    box-shadow:
      0 18px 48px color-mix(in sRGB, var(--text) 18%, transparent),
      inset 0 0 0 1px color-mix(in sRGB, var(--text) 10%, transparent);
  }

  .lf:not(.is-expanded) {
    justify-content: center;
  }

  .lf:not(.is-expanded, .is-revealing) {
    overflow: visible;
  }

  .lf-bar {
    position: relative;
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    gap: 14px;

    /* Compacto: mismo alto que la pill (40px). */
    min-height: 40px;
    height: 40px;
    padding: 0;
    box-sizing: border-box;
  }

  /*
   * Expandido: misma composición stadium (~40px) que al abrir.
   * Solo un hairline separa resultados; no aplastar el head ni meter favs.
   */
  .lf.is-expanded .lf-bar {
    min-height: 40px;
    height: 40px;
    padding: 0;
    border-bottom: 1px solid color-mix(in sRGB, var(--text) 10%, transparent);
  }

  .lf-head {
    display: flex;
    min-width: 0;
    flex: none;
    width: 100%;
    align-items: center;
    gap: 0.3rem;
    height: 100%;
    padding: 0 0.35rem 0 0.5rem;
    border-radius: 999px;
    background: var(--skin);
    cursor: grab;
    touch-action: none;
    user-select: none;

    /* El chrome entra cuando la barra se asienta: misma familia que el grow. */
    transition: opacity var(--lf-chrome-dur) var(--ease-smooth-out);
  }

  .lf-head:active {
    cursor: grabbing;
  }

  .lf-search-icon {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    color: var(--muted);
  }

  .lf-input {
    min-width: 0;
    flex: 1;
    border: none;
    background: transparent;
    color: var(--text);
    font-size: 0.8125rem;
    line-height: 1.2;
    outline: none;
    cursor: text;
  }

  .lf-input::placeholder {
    color: var(--faint);
  }

  /*
   * Favs siempre a la derecha del stadium (compacto y buscando).
   * Absolute respecto a `.lf-bar` — no entran al flex del head.
   * Cada `.lf-dot.is-out` se desprende sola (secuencia JS).
   */
  .lf-favs {
    position: absolute;
    left: calc(100% + var(--lf-fav-gap, 15px));
    top: 0;
    z-index: 2;
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: var(--lf-dot-gap, 15px);
    max-width: none;
    overflow: visible;
    pointer-events: none;
  }

  .lf.is-favs-seq .lf-favs {
    pointer-events: auto;
  }

  /* Diámetro = pill / COMPACT_H (40px). */
  .lf-dot {
    display: grid;
    place-items: center;
    box-sizing: border-box;
    width: 40px;
    height: 40px;
    flex-shrink: 0;
    border: 0;
    border-radius: 999px;
    padding: 0;
    background: var(--skin);
    color: var(--muted);
    cursor: pointer;
    pointer-events: none;
    position: relative;
    z-index: 1;
    opacity: 0;

    /* Pegada a la barra; al .is-out viaja a su sitio. */
    transform: translateX(calc(-1 * var(--lf-fav-gap, 15px) - 8px)) scale(0.82);
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      transform var(--launcher-fav-stagger) var(--ease-smooth-out),
      opacity var(--launcher-fav-stagger) var(--ease-smooth-out);
  }

  .lf.is-favs-stagger .lf-dot {
    transition-delay: calc(var(--lf-i, 0) * var(--launcher-fav-stagger));
  }

  .lf.is-favs-stagger.is-tucking .lf-dot {
    transition-delay: calc(
      (var(--lf-fav-last-index, 0) - var(--lf-i, 0)) * var(--launcher-fav-stagger)
    );
  }

  .lf-dot.is-out {
    opacity: 1;
    pointer-events: auto;
    transform: none;
  }

  .lf-dot.is-out:hover {
    color: var(--text);
    background: color-mix(in sRGB, var(--text) 12%, var(--skin));
    transform: scale(1.06);
  }

  .lf-dot.is-out:active {
    transform: scale(0.96);
  }

  .lf-dot.is-action {
    background: color-mix(in sRGB, var(--ok) 16%, var(--skin));
    color: var(--ok);
  }

  .lf-dot :global(img) {
    border-radius: 999px;
  }

  .lf-busy {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 0.7rem;
    color: var(--faint);
  }

  .lf-icon {
    display: grid;
    place-items: center;
    box-sizing: border-box;
    width: 1.75rem;
    height: 1.75rem;
    flex-shrink: 0;
    border: 1px solid transparent;
    border-radius: 0.4rem;
    padding: 0;
    background: transparent;
    color: var(--faint);
    cursor: pointer;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .lf-icon:hover {
    color: var(--text);
    background: color-mix(in sRGB, var(--text) 8%, transparent);
  }

  .lf-icon:active {
    transform: scale(0.96);
  }

  .lf-head:has(.lf-input:focus-visible),
  .lf-icon:focus-visible,
  .lf-dot:focus-visible,
  .lf-hit-main:focus-visible,
  .lf-star:focus-visible {
    outline: 2px solid color-mix(in sRGB, var(--ok) 78%, var(--text));
    outline-offset: 3px;
    box-shadow: 0 0 0 4px color-mix(in sRGB, var(--ok) 18%, transparent);
  }

  .lf-hit-main:focus-visible {
    background: color-mix(in sRGB, var(--text) 7%, transparent);
  }

  .lf-err {
    margin: 0;
    padding: 0.4rem 0.75rem;
    background: color-mix(in sRGB, var(--danger) 18%, transparent);
    color: var(--danger);
    font-size: 0.75rem;
  }

  .lf-heading {
    margin: 0;
    flex-shrink: 0;
    padding: 0.45rem 0.7rem 0.1rem;
    color: var(--faint);
    font-size: 0.62rem;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .lf-list {
    flex: 1;
    min-height: 0;
    margin: 0;
    padding: 0.35rem;
    list-style: none;
    overflow: auto;
    contain: content;
  }

  .lf-list > li {
    content-visibility: auto;
    contain-intrinsic-size: auto 44px;
  }

  .lf-hit {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 0.15rem;
    border-radius: 0.5rem;
    transition: background var(--duration-quick) var(--ease-smooth-out);
  }

  .lf-hit.is-sel {
    background: color-mix(in sRGB, var(--text) 5%, transparent);
  }

  .lf-hit-main {
    display: flex;
    min-width: 0;
    flex: 1;
    align-items: center;
    gap: 0.7rem;
    border: none;
    border-radius: 0.5rem;
    padding: 0.45rem 0.35rem 0.45rem 0.55rem;
    background: transparent;
    color: inherit;
    text-align: left;
    cursor: pointer;
  }

  .lf-hit-main:active {
    transform: scale(0.99);
  }

  .lf-hit-ico {
    display: grid;
    place-items: center;
    width: 2rem;
    height: 2rem;
    flex-shrink: 0;
    border-radius: 0.4rem;
    background: color-mix(in sRGB, var(--text) 6%, transparent);
    color: var(--muted);
  }

  .lf-hit-ico.is-action {
    background: color-mix(in sRGB, var(--ok) 18%, transparent);
    color: var(--ok);
  }

  .lf-hit-text {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.05rem;
  }

  .lf-hit-title {
    overflow: hidden;
    font-size: 0.9rem;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .lf-hit-sub {
    overflow: hidden;
    font-size: 0.7rem;
    color: var(--muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .lf-star {
    display: grid;
    place-items: center;
    position: relative;
    z-index: 2;
    width: 2.5rem;
    height: 2.5rem;
    flex-shrink: 0;
    margin-right: 0.15rem;
    border: none;
    border-radius: 0.4rem;
    padding: 0;
    background: transparent;
    color: var(--faint);
    cursor: pointer;
    opacity: 0.55;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      opacity var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .lf-star :global(svg) {
    pointer-events: none;
  }

  .lf-hit.is-sel .lf-star,
  .lf-star:hover,
  .lf-star.is-on {
    opacity: 1;
  }

  .lf-star:hover {
    background: color-mix(in sRGB, var(--text) 8%, transparent);
    color: var(--text);
  }

  .lf-star.is-on {
    color: var(--warn);
  }

  .lf-star:active {
    transform: scale(0.94);
  }

  .lf-empty {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 1.5rem 0.75rem;
    color: var(--faint);
    font-size: 0.85rem;
    text-align: center;
  }

  .lf-foot {
    display: flex;
    flex-shrink: 0;
    justify-content: space-between;
    gap: 0.4rem;
    min-width: 0;
    padding: 0.35rem 0.5rem 0.5rem;
    overflow: hidden;
    border-top: 1px solid color-mix(in sRGB, var(--text) 10%, transparent);
    color: var(--faint);
    font-size: 0.6rem;
  }

  .lf-hint {
    display: inline-flex;
    flex-shrink: 1;
    min-width: 0;
    align-items: center;
    gap: 0.25rem;
    overflow: hidden;
    white-space: nowrap;
  }

  @media (prefers-reduced-motion: reduce) {
    .lf,
    .lf-head,
    .lf-favs,
    .lf:not(.is-shown) .lf-favs,
    .lf-dot {
      transition: none;
      transition-delay: 0ms;
    }

    .lf:not(.is-shown) .lf-favs {
      transform: none;
    }

    .lf:not(.is-shown) .lf-dot {
      opacity: 1;
      transform: none;
    }

    .lf-icon:active,
    .lf-dot:hover,
    .lf-dot:active,
    .lf-hit-main:active,
    .lf-star:active {
      transform: none;
    }
  }
</style>
