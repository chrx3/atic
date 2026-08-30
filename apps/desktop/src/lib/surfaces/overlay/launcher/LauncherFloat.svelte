<script lang="ts">
  import { tip } from "$surfaces/overlay/tip.svelte";
  /**
   * Float del launcher Spotlight.
   *
   * Apertura (como expandir al dictar + Spotlight):
   * 1) La pill vuela al slot (PillSurface).
   * 2) Disco-semilla coincidente con la pill; se estira a la derecha (width).
   * 3) Se separa al centro (cuello líquido).
   * 4) Cada favorito se desprende de a uno.
   *
   * Cierre = reverse: tuck favs → approach (fuse) → shrink → dismiss;
   * la pill vuelve a casa al terminar (PillSurface espera el hit-rect).
   */
  import { onMount, tick, untrack } from "svelte";
  import type { BubbleOpen, LauncherHit } from "$core/types";
  import {
    hideLauncher,
    launcherListFavorites,
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
  import { resolveSlot, LAUNCHER_BAR_W } from "$surfaces/overlay/toolSlots";
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
  import { separateAxisProp, waitFrames } from "$surfaces/overlay/floatReveal";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";
  import {
    armOpenDismissGrace,
    isOpenDismissGrace,
  } from "$surfaces/overlay/openDismissGrace";
  import { afterTransition, MOTION, ms, prefersReducedMotion, wait } from "$lib/motion";
  import LauncherIcon from "$surfaces/launcher/LauncherIcon.svelte";
  import Icon from "$ui/Icon.svelte";
  import { t } from "$domain/i18n.svelte";
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
   * Semilla = disco de la pill (40×40). El overlap es el diámetro: dos
   * círculos desfasados 20 px se leían como óvalo desde el primer frame.
   */
  const GROW_START_W = 40;
  /** Approach/close: gap bajo REACH para re-fundir el cuello. */
  const FUSED_GAP_PX = 2;
  /** Hold en disco fused antes de estirar (ms). */
  const SEED_HOLD_MS = 100;
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
  let favorites = $state<LauncherHit[]>([]);
  let favoriteIds = $state<string[]>([]);
  let selected = $state(0);
  let searching = $state(false);
  let error = $state("");
  let input = $state<HTMLInputElement | null>(null);
  let generation = 0;
  /** Debounce IPC: cada tecla no debe invocar `launcher_search`. */
  let searchTimer: ReturnType<typeof setTimeout> | null = null;
  const SEARCH_DEBOUNCE_MS = 120;

  /**
   * Coreografía tipo dictado → Spotlight:
   * open:  hidden → expand → separate → favs → ready
   * close: tuck → approach → shrink → (dismiss)
   */
  type RevealPhase =
    | "hidden"
    | "expand"
    | "separate"
    | "favs"
    | "ready"
    | "tuck"
    | "approach"
    | "shrink";
  let revealPhase = $state<RevealPhase>("hidden");
  let revealEpoch = 0;
  /** Evita reentrar close / segundo Esc fuerza dismiss. */
  let closing = false;
  /** `hideLauncher` re-dispara dismiss IPC: ignorar ese eco. */
  let ignoreIpcDismiss = false;
  /** Cuántos favs ya salieron (0 = ninguno; N = primeros N visibles). */
  let favRevealCount = $state(0);
  const expanding = $derived(revealPhase === "expand" || revealPhase === "shrink");
  const separating = $derived(revealPhase === "separate" || revealPhase === "approach");
  const favsSequencing = $derived(
    revealPhase === "favs" || revealPhase === "ready" || revealPhase === "tuck",
  );
  const motionPhase = $derived(expanding || separating || revealPhase === "tuck");

  const hasQuery = $derived(query.trim().length > 0);
  const showResults = $derived(hasQuery);

  const favGap = $derived(isDev && launcherLab.open ? launcherLab.favGap : FAVS_GAP_PX);
  const dotGap = $derived(isDev && launcherLab.open ? launcherLab.dotGap : DOT_GAP_PX);
  const labOpenDur = $derived(
    isDev && launcherLab.open ? launcherLab.openDur : ms(MOTION.launcherBar),
  );
  const labCloseDur = $derived(isDev && launcherLab.open ? launcherLab.closeDur : 120);
  const compactH = $derived(isDev && launcherLab.open ? launcherLab.barH : COMPACT_H);
  const reach = $derived(
    isDev && launcherLab.open ? sminReach(launcherLab.blend) : REACH,
  );

  let openDur = $state(150);
  let closeDur = $state(120);
  let separateDur = $state(150);
  let favStaggerDur = $state(150);

  function armOpenDur() {
    openDur = labOpenDur;
    separateDur = ms(MOTION.launcherSeparate);
    favStaggerDur = ms(MOTION.launcherFavStagger);
  }

  function armCloseDur() {
    closeDur = labCloseDur;
  }

  function cancelReveal() {
    revealEpoch += 1;
  }

  /**
   * Acto expand→separate→favs secuenciales.
   * No usa scale de `.float-emerge`: crece width como la barra de dictado.
   */
  async function runOpenReveal() {
    const epoch = ++revealEpoch;
    if (prefersReducedMotion()) {
      if (lastOpen) await applyCenterPlace(lastOpen);
      if (favorites.length === 0) await loadFavorites();
      favRevealCount = favorites.length;
      revealPhase = "ready";
      return;
    }

    revealPhase = "expand";
    favRevealCount = 0;
    await tick();
    await waitFrames(2);
    // Un beat en disco fused: si estiramos al primer frame, se lee “apareció
    // una barra” en vez de “nació de la pill”.
    await wait(SEED_HOLD_MS);
    if (epoch !== revealEpoch) return;

    // Crecer a la derecha (borde izquierdo clavado; semilla aún solapada).
    const fullW =
      isDev && launcherLab.open ? launcherLab.barW : (lastOpen?.w ?? LAUNCHER_BAR_W);
    if (bubble.anchor) {
      bubble.anchor = { ...bubble.anchor, w: fullW };
    }
    await afterTransition(el, "width", openDur);
    if (epoch !== revealEpoch) return;

    // Separar: armar transición de `left` un frame antes de mover el ancla
    // (si no, el left cambia sin `.is-separating` y se ve snap).
    revealPhase = "separate";
    await tick();
    await waitFrames(2);
    if (epoch !== revealEpoch) return;
    if (lastOpen) await applyCenterPlace({ ...lastOpen, w: fullW, h: compactH });
    await afterTransition(el, "left", separateDur);
    if (epoch !== revealEpoch) return;

    if (favorites.length === 0) {
      await loadFavorites();
      if (epoch !== revealEpoch) return;
    }
    if (favorites.length === 0) {
      revealPhase = "ready";
      return;
    }

    // Favs de a uno: cada bolita se desprende de la barra.
    revealPhase = "favs";
    for (let i = 1; i <= favorites.length; i++) {
      if (epoch !== revealEpoch) return;
      favRevealCount = i;
      await tick();
      const dot = el?.querySelector(`.lf-dot:nth-child(${i})`);
      if (dot instanceof HTMLElement) {
        await afterTransition(dot, "transform", favStaggerDur);
      } else {
        await wait(favStaggerDur);
      }
    }
    if (epoch !== revealEpoch) return;
    revealPhase = "ready";
  }

  /**
   * Close = reverse del open: tuck favs → approach (fuse) → shrink → caller dismiss.
   * No pone `hidden` (eso re-dispararía open mientras `shown`).
   */
  async function runCloseReveal(epoch: number): Promise<void> {
    if (prefersReducedMotion()) {
      favRevealCount = 0;
      return;
    }

    // Panel de resultados: volver a stadium compacto antes del fuse.
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
        await afterTransition(firstDot, "transform", favStaggerDur);
      } else {
        await wait(favStaggerDur);
      }
      if (epoch !== revealEpoch) return;
    }

    const fullW =
      isDev && launcherLab.open
        ? launcherLab.barW
        : (bubble.anchor?.w ?? lastOpen?.w ?? LAUNCHER_BAR_W);

    revealPhase = "approach";
    await tick();
    await waitFrames(2);
    if (epoch !== revealEpoch) return;
    placeFusedFullToPill(fullW);
    await afterTransition(el, separateAxisProp("left"), separateDur);
    if (epoch !== revealEpoch) return;

    revealPhase = "shrink";
    await tick();
    await waitFrames(2);
    if (epoch !== revealEpoch) return;
    const seedBase = lastOpen ?? {
      side: "left" as const,
      offset: compactH / 2,
      x: bubble.anchor?.x ?? 0,
      y: bubble.anchor?.y ?? 0,
      w: LAUNCHER_BAR_W,
      h: compactH,
    };
    placeFusedToPill({ ...seedBase, w: fullW, h: compactH });
    await afterTransition(el, "width", openDur);
  }

  /**
   * Centra la barra stadium (ancho `a.w`) en el monitor del mouse / foco.
   * Los favs van fuera del float (CSS absolute a la derecha); no desplazan
   * el centro de la barra.
   */
  async function applyCenterPlace(a: BubbleOpen) {
    try {
      workAreas = await overlayWorkAreas();
    } catch {
      // Fuera de Tauri o IPC fallido: se usa lo último que haya.
    }
    const pill = surfaces.live["pill-skin"] ?? surfaces.live["pill"];
    const labCompact = isDev && launcherLab.open && !showResults;
    const w = labCompact ? launcherLab.barW : a.w;
    const h = labCompact ? compactH : a.h;
    const size = { w, h };
    // La pill ya está en el monitor correcto (vuelo del atajo, o clic local).
    // overlayActiveAnchor prefería el foco de otra app y saltaba de pantalla.
    let anchor: { x: number; y: number };
    if (pill) {
      anchor = { x: pill.x + pill.w / 2, y: pill.y + pill.h / 2 };
    } else {
      try {
        anchor = (await overlayActiveAnchor()) ?? {
          x: a.x + a.w / 2,
          y: a.y + a.h / 2,
        };
      } catch {
        anchor = { x: a.x + a.w / 2, y: a.y + a.h / 2 };
      }
    }
    const pos = resolveSlot("center", workAreas, size, anchor);
    bubble.place({
      ...a,
      w,
      h,
      x: pos.x,
      y: pos.y,
      side: "left",
      offset: size.h / 2,
    });
  }

  /**
   * Disco-semilla coincidente con la pill (un círculo). Luego `runOpenReveal`
   * estira el ancho a la derecha. w y h van iguales: si el alto compacto
   * cambia en el lab, no nacer como óvalo.
   */
  function placeFusedToPill(a: BubbleOpen) {
    const pill = surfaces.live["pill-skin"] ?? surfaces.live["pill"];
    const d = Math.min(isDev && launcherLab.open ? compactH : a.h, GROW_START_W);
    let x = a.x;
    let y = a.y;
    if (pill) {
      x = pill.x + pill.w - d;
      y = pill.y + (pill.h - d) / 2;
    }
    bubble.place({
      ...a,
      w: d,
      h: d,
      x,
      y,
      side: "left",
      offset: d / 2,
    });
  }

  /** Ancho completo aún fused (reverse de separate / previa al shrink). */
  function placeFusedFullToPill(fullW: number) {
    const pill = surfaces.live["pill-skin"] ?? surfaces.live["pill"];
    const h = compactH;
    let x = bubble.anchor?.x ?? 0;
    let y = bubble.anchor?.y ?? 0;
    if (pill) {
      x = pill.x + pill.w + FUSED_GAP_PX;
      y = pill.y + (pill.h - h) / 2;
    }
    const base = lastOpen ?? {
      side: "left" as const,
      offset: h / 2,
      x,
      y,
      w: fullW,
      h,
    };
    bubble.place({
      ...base,
      w: fullW,
      h,
      x,
      y,
      side: "left",
      offset: h / 2,
    });
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
    // Nunca recentrar durante birth/close: un segundo anchor hacía snap a
    // barra completa al lado → “elemento externo”.
    if (fresh || revealPhase === "hidden") {
      placeFusedToPill(a);
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
  function publishCompactPills(root: HTMLElement, group?: string): () => void {
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
    // Durante grow/separate/close reverse el ancho/left se mueve: seguir frame a frame.
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
    fitHeight(showResults ? EXPANDED_H : compactH);
  });

  /**
   * Lab: re-aplicar ancho/alto compacto al mover barW/barH.
   * Solo en ready (no pelear con expand/separate).
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

  async function loadFavorites() {
    try {
      const next = await launcherListFavorites();
      favorites = next;
      favoriteIds = next.map((f) => f.id);
    } catch {
      favorites = [];
      favoriteIds = [];
    }
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
      clearHits();
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
    // Favoritos ya se cargan antes del place (para el acto 3); no bloquear foco.
    if (favorites.length === 0) await loadFavorites();
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
    const target = id ?? hits[selected]?.id;
    if (!target) return;
    try {
      await launcherRun(target);
    } catch (failure) {
      error = failure instanceof Error ? failure.message : String(failure);
    }
  }

  async function toggleFavorite(id: string, event?: Event) {
    event?.stopPropagation();
    try {
      favoriteIds = await launcherToggleFavorite(id);
      await loadFavorites();
      // Sin esto las bolitas existen en el DOM pero con opacity 0 (`is-out`
      // exige i < favRevealCount). Había que cerrar y reabrir para verlas.
      favRevealCount = favorites.length;
      await tick();
      surfaces.schedule();
    } catch (failure) {
      error = failure instanceof Error ? failure.message : String(failure);
      await loadFavorites();
      favRevealCount = favorites.length;
    }
  }

  function isFavorite(id: string) {
    return favoriteIds.includes(id);
  }

  function finishDismiss(wasShown: boolean, opts: { skipHideLauncher?: boolean } = {}) {
    clearSearchTimer();
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
    if (event.key === "ArrowDown" && hits.length > 0) {
      event.preventDefault();
      selected = (selected + 1) % hits.length;
    } else if (event.key === "ArrowUp" && hits.length > 0) {
      event.preventDefault();
      selected = (selected - 1 + hits.length) % hits.length;
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
        // virtual entero → el float quedaba corrido. No recentrar durante
        // expand/separate: eso mataría el grow desde la pill.
        if (lastOpen && bubble.alive && revealPhase === "ready") {
          void applyCenterPlace(lastOpen);
        } else if (
          lastOpen &&
          bubble.alive &&
          (revealPhase === "hidden" || revealPhase === "expand") &&
          !bubble.shown
        ) {
          placeFusedToPill(lastOpen);
        }
      })
      .catch(() => {
        workAreas = [];
      });
    const un: Promise<() => void>[] = [
      onLauncherBubbleAnchor((a) => {
        // Acto 2 ya: no esperar favs (eso hacía “snap” tras la carga).
        void placeFromPill(a);
        void (async () => {
          await loadFavorites();
          await reset(true);
        })();
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
    class:is-expanded={showResults}
    class:is-expanding={expanding}
    class:is-separating={separating}
    class:is-favs-seq={favsSequencing}
    data-float="launcher"
    data-side={bubble.anchor?.side ?? "left"}
    style={bubble.vars}
    style:--float-stack={surfaces.stack("launcher")}
    style:--lf-fav-gap="{favGap}px"
    style:--lf-dot-gap="{dotGap}px"
    style:--launcher-bar-open-dur="{openDur}ms"
    style:--launcher-separate-dur="{separateDur}ms"
    style:--launcher-fav-stagger="{favStaggerDur}ms"
    style:--float-close-dur="{closeDur}ms"
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

    {#if showResults}
      <ul class="lf-list" role="listbox" aria-label={t("overlay.results")}>
        {#each hits as hit, i (hit.id)}
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
                  <span class="lf-hit-sub">{hit.subtitle}</span>
                </span>
              </button>
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
      </footer>
    {/if}
  </div>
{/if}

<style>
  /*
   * Nace como disco coincidente con la pill; el width crece a stadium; luego
   * separate + favs. El chrome se apaga durante `.is-expanding`.
   */
  .lf {
    /* bar-open/separate heredan de :root (app.css). fav-stagger y float-close
       quedan como overrides locales a propósito: difieren del root. */
    --launcher-fav-stagger: 150ms;
    --float-close-dur: var(--duration-quick);

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
       (layout + hit-rects) en cada tecla. El grow de apertura anima width. */
  }

  .lf.is-shown {
    opacity: 1;
    pointer-events: auto;
  }

  .lf.is-expanding {
    transition:
      width var(--launcher-bar-open-dur) var(--ease-smooth-out),
      height var(--launcher-bar-open-dur) var(--ease-smooth-out);
    overflow: hidden;
  }

  /*
   * Durante el grow la silueta la pinta el líquido. El float está por encima
   * de la pill: un head opaco taparía la «a» y, si el input impone min-width,
   * se leería stadium desde el primer frame.
   */
  .lf.is-expanding .lf-head {
    opacity: 0;
    pointer-events: none;
    background: transparent;
  }

  .lf.is-separating {
    transition:
      left var(--launcher-separate-dur) var(--ease-smooth-out),
      top var(--launcher-separate-dur) var(--ease-smooth-out),
      width var(--duration-quick) var(--ease-smooth-out);
  }

  /* Expandido: panel único con surface. Compacto: chrome transparente.
   * overflow visible: los favs viven fuera (absolute a la der. del stadium). */
  .lf.is-expanded {
    background: var(--skin);
    overflow: visible;
    border-radius: 18px;
  }

  .lf:not(.is-expanded) {
    justify-content: center;
  }

  .lf:not(.is-expanded):not(.is-expanding) {
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
    border-bottom: 1px solid color-mix(in srgb, var(--text) 10%, transparent);
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

  .lf-dot.is-out {
    opacity: 1;
    pointer-events: auto;
    transform: none;
  }

  .lf-dot.is-out:hover {
    color: var(--text);
    background: color-mix(in srgb, var(--text) 12%, var(--skin));
    transform: scale(1.06);
  }

  .lf-dot.is-out:active {
    transform: scale(0.96);
  }

  .lf-dot.is-action {
    background: color-mix(in srgb, var(--ok, #3a8) 16%, var(--skin));
    color: var(--ok, #3a8);
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
    background: color-mix(in srgb, var(--text) 8%, transparent);
  }

  .lf-icon:active {
    transform: scale(0.96);
  }

  .lf-err {
    margin: 0;
    padding: 0.4rem 0.75rem;
    background: color-mix(in srgb, var(--danger, #c44) 18%, transparent);
    color: var(--danger, #c44);
    font-size: 0.75rem;
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
    background: color-mix(in srgb, var(--text) 5%, transparent);
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
    background: color-mix(in srgb, var(--text) 6%, transparent);
    color: var(--muted);
  }

  .lf-hit-ico.is-action {
    background: color-mix(in srgb, var(--ok, #3a8) 18%, transparent);
    color: var(--ok, #3a8);
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
    background: color-mix(in srgb, var(--text) 8%, transparent);
    color: var(--text);
  }

  .lf-star.is-on {
    color: var(--warn, #c90);
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
    border-top: 1px solid color-mix(in srgb, var(--text) 10%, transparent);
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
    .lf.is-expanding,
    .lf.is-separating,
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
