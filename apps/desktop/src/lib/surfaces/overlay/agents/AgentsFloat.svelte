<script lang="ts">
  /**
   * Shell del float: bubble + liquid + surfaces + drag.
   * Por ahora hospeda la misma demo visual que la ventana principal.
   */
  import { onMount, tick } from "svelte";
  import { agents } from "$lib/agentSessions.svelte";
  import {
    agentsAlwaysOnTop,
    hideAgentsWindow,
    onAgentsBubbleAnchor,
    onAgentsBubbleDismiss,
    saveAgentsBubbleSize,
  } from "$ipc/agents";
  import {
    onOverlayDismiss,
    onOverlayReady,
    overlayWorkAreas,
    workAreaOf,
  } from "$ipc/overlay";
  import type { Area } from "$ipc/overlay";
  import type { BubbleOpen } from "$core/types";
  import { applyTheme, readCachedTheme } from "$lib/theme";
  import { liquid } from "$surfaces/overlay/group.svelte";
  import {
    publishEmergeSkin,
    publishFollowSkin,
  } from "$surfaces/overlay/floatEmergeSkin";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";
  import { Bubble, BUBBLE_MIN_W } from "$surfaces/overlay/bubble.svelte";
  import { createBubbleDrag } from "$surfaces/overlay/bubbleDrag";
  import {
    expandPanelFromSeed,
    placePanelFusedSeed,
    placePanelResting,
  } from "$surfaces/overlay/floatPlace";
  import { separateAxisProp, waitFrames } from "$surfaces/overlay/floatReveal";
  import AgentLauncher from "$features/agents/AgentLauncher.svelte";
  import { isAgentsDismissSuppressed } from "$surfaces/overlay/agents/dismissGuard";
  import { toasts } from "$domain/toasts.svelte";
  import ToastStack from "$ui/ToastStack.svelte";
  import { afterTransition, MOTION, ms, prefersReducedMotion, wait } from "$lib/motion";
  import {
    armOpenDismissGrace,
    isOpenDismissGrace,
  } from "$surfaces/overlay/openDismissGrace";

  const BUBBLE_CORNER = 26;
  /* Un beat visible en la pill antes de crecer: así se LEE que nace de ahí. */
  const BIRTH_SEED_HOLD_MS = 60;
  const POSITION_STORAGE_KEY = "atic.agents.consolePosition";
  const SETUP_WIDTH_STORAGE_KEY = "atic.agents.setupWidth";
  const POSITION_MARGIN = 12;
  const SETUP_DEFAULT_W = 360;
  const SETUP_WIDE_H = 176;
  const SETUP_NARROW_H = 196;
  const SETUP_NARROW_W = 560;
  const BROWSER_DEFAULT_W = 520;
  const BROWSER_DEFAULT_H = 420;
  const BROWSER_MIN_H = 360;
  const CONSOLE_DEFAULT_W = 680;
  const CONSOLE_DEFAULT_H = 520;
  const CONSOLE_MIN_H = 340;
  let workAreas = $state<Area[]>([]);
  let restingOpen = $state<BubbleOpen | null>(null);

  type LauncherView = "setup" | "console";
  let launcherView = $state<LauncherView>("setup");
  let setupWidth = SETUP_DEFAULT_W;
  let consoleSize = { w: CONSOLE_DEFAULT_W, h: CONSOLE_DEFAULT_H };
  let browserOpen = $state(false);
  let browserSize = { w: BROWSER_DEFAULT_W, h: BROWSER_DEFAULT_H };
  let modeResizing = $state(false);
  let modeResizeEpoch = 0;

  type RevealPhase = "hidden" | "expand" | "settle" | "ready";
  let revealPhase = $state<RevealPhase>("hidden");
  let revealEpoch = 0;
  const expanding = $derived(revealPhase === "expand");
  const settling = $derived(revealPhase === "settle");
  const motionPhase = $derived(expanding || settling);
  const growDur = ms(MOTION.slow);
  const settleDur = ms(MOTION.medium);

  type SavedPosition = { x: number; y: number };

  function readSetupWidth(): number {
    const saved = Number(localStorage.getItem(SETUP_WIDTH_STORAGE_KEY));
    return Number.isFinite(saved) ? Math.max(BUBBLE_MIN_W, saved) : SETUP_DEFAULT_W;
  }

  function saveSetupWidth() {
    try {
      localStorage.setItem(SETUP_WIDTH_STORAGE_KEY, String(Math.round(setupWidth)));
    } catch {
      /* El tamaño compacto sigue funcionando aunque el storage esté bloqueado. */
    }
  }

  function savePosition() {
    const a = bubble.anchor;
    if (!a) return;
    try {
      localStorage.setItem(
        POSITION_STORAGE_KEY,
        JSON.stringify({ x: Math.round(a.x), y: Math.round(a.y) }),
      );
    } catch {
      /* El float sigue funcionando aunque el storage esté bloqueado. */
    }
  }

  function positionInWorkspace(
    pill: { x: number; y: number; w: number; h: number },
    panel: { w: number; h: number },
    preferred: SavedPosition,
  ): SavedPosition {
    const point = { x: preferred.x + panel.w / 2, y: preferred.y + panel.h / 2 };
    const pillPoint = { x: pill.x + pill.w / 2, y: pill.y + pill.h / 2 };
    const contains = (area: Area, p: { x: number; y: number }) =>
      p.x >= area.x &&
      p.x <= area.x + area.w &&
      p.y >= area.y &&
      p.y <= area.y + area.h;
    const area =
      workAreas.find((candidate) => contains(candidate, pillPoint)) ??
      workAreas.find((candidate) => contains(candidate, point)) ??
      workAreas[0] ??
      ({ x: 0, y: 0, w: window.innerWidth, h: window.innerHeight } satisfies Area);
    const work = workAreaOf(area);
    const minX = work.x + POSITION_MARGIN;
    const minY = work.y + POSITION_MARGIN;
    const maxX = Math.max(minX, work.x + work.w - panel.w - POSITION_MARGIN);
    const maxY = Math.max(minY, work.y + work.h - panel.h - POSITION_MARGIN);
    return {
      x: Math.round(Math.min(Math.max(preferred.x, minX), maxX)),
      y: Math.round(Math.min(Math.max(preferred.y, minY), maxY)),
    };
  }

  function placeNearPill(a: BubbleOpen, size: { w: number; h: number }): BubbleOpen {
    const pill = surfaces.live["pill-skin"] ?? surfaces.live["pill"];
    if (!pill) {
      return {
        ...a,
        ...positionInWorkspace(
          { x: a.x, y: a.y, w: 1, h: 1 },
          size,
          { x: a.x, y: a.y },
        ),
        ...size,
      };
    }
    return {
      ...a,
      ...placePanelResting(pill, size, {
        corner: BUBBLE_CORNER,
        work: workAreas,
      }),
      ...size,
    };
  }

  function resolveRestingOpen(
    a: BubbleOpen,
    keep: SavedPosition | null,
  ): BubbleOpen {
    const panel = { w: a.w, h: a.h };
    if (keep) {
      const pill = surfaces.live["pill-skin"] ?? surfaces.live["pill"] ?? {
        x: a.x,
        y: a.y,
        w: 1,
        h: 1,
      };
      return { ...a, ...positionInWorkspace(pill, panel, keep) };
    }
    return placeNearPill(a, panel);
  }

  async function ensureWorkAreas() {
    if (workAreas.length > 0) return;
    try {
      workAreas = await overlayWorkAreas();
    } catch {
      workAreas = [];
    }
  }

  function setupHeight(width: number): number {
    return width <= SETUP_NARROW_W ? SETUP_NARROW_H : SETUP_WIDE_H;
  }

  function frameForView(a: BubbleOpen): BubbleOpen {
    if (launcherView === "console") {
      return {
        ...a,
        w: Math.max(CONSOLE_DEFAULT_W, consoleSize.w),
        h: Math.max(CONSOLE_DEFAULT_H, consoleSize.h),
      };
    }
    if (browserOpen) {
      return {
        ...a,
        w: Math.max(BUBBLE_MIN_W, browserSize.w, a.w),
        h: Math.max(BROWSER_MIN_H, browserSize.h),
      };
    }
    return { ...a, w: setupWidth, h: setupHeight(setupWidth) };
  }

  function placeBirthSeed(a: BubbleOpen) {
    const pill = surfaces.live["pill-skin"] ?? surfaces.live["pill"];
    if (!pill) {
      bubble.place(a);
      return;
    }
    bubble.place({
      ...a,
      ...placePanelFusedSeed(
        pill,
        { w: a.w, h: a.h },
        { corner: BUBBLE_CORNER, work: workAreas },
      ),
    });
  }

  let placeEpoch = 0;

  async function placeFromPill(a: BubbleOpen) {
    const epoch = ++placeEpoch;
    clearSizeToggles();
    a = frameForView(a);
    const fresh = !bubble.alive || !bubble.shown;
    if (fresh) armOpenDismissGrace();

    await ensureWorkAreas();
    if (epoch !== placeEpoch) return;

    const keep =
      !fresh && bubble.anchor
        ? { x: bubble.anchor.x, y: bubble.anchor.y }
        : null;
    restingOpen = resolveRestingOpen(a, keep);

    if (fresh || revealPhase === "hidden") {
      placeBirthSeed(a);
      return;
    }
    if (revealPhase === "ready") bubble.place(restingOpen);
  }

  async function animateToSize(current: BubbleOpen, size: { w: number; h: number }) {
    await ensureWorkAreas();
    const target = placeNearPill(current, size);
    const epoch = ++modeResizeEpoch;
    modeResizing = true;
    await tick();
    bubble.setFrame(target.x, target.y, target.w, target.h);
    restingOpen = target;
    await wait(ms(MOTION.medium));
    if (epoch === modeResizeEpoch) modeResizing = false;
  }

  async function changeLauncherView(next: LauncherView) {
    if (launcherView === next) return;
    clearSizeToggles();
    const current = bubble.anchor;
    launcherView = next;
    if (next === "console") browserOpen = false;
    if (!current) return;

    if (next === "console") {
      setupWidth = current.w;
      saveSetupWidth();
    } else if (current.h >= CONSOLE_MIN_H) {
      consoleSize = {
        w: Math.max(CONSOLE_DEFAULT_W, current.w),
        h: Math.max(CONSOLE_DEFAULT_H, current.h),
      };
    }

    const size =
      next === "console"
        ? {
            w: Math.max(CONSOLE_DEFAULT_W, consoleSize.w, current.w),
            h: Math.max(CONSOLE_DEFAULT_H, consoleSize.h),
          }
        : {
            w: Math.max(BUBBLE_MIN_W, setupWidth),
            h: setupHeight(Math.max(BUBBLE_MIN_W, setupWidth)),
          };
    await animateToSize(current, size);
  }

  async function changeBrowser(open: boolean) {
    if (browserOpen === open) return;
    clearSizeToggles();
    const current = bubble.anchor;
    browserOpen = open;
    if (!current) return;

    if (open) {
      setupWidth = current.w;
      saveSetupWidth();
    } else browserSize = { w: current.w, h: current.h };

    const width = open
      ? Math.max(BUBBLE_MIN_W, browserSize.w, current.w)
      : Math.max(BUBBLE_MIN_W, setupWidth);
    const size = open
      ? { w: width, h: Math.max(BROWSER_MIN_H, browserSize.h) }
      : { w: width, h: setupHeight(width) };
    await animateToSize(current, size);
  }

  async function runOpenReveal() {
    const epoch = ++revealEpoch;
    const resting = restingOpen;
    if (!resting) return;
    if (prefersReducedMotion()) {
      bubble.place(resting);
      revealPhase = "ready";
      return;
    }

    revealPhase = "expand";
    await tick();
    await waitFrames(2);
    await wait(BIRTH_SEED_HOLD_MS);
    if (epoch !== revealEpoch || !bubble.anchor) return;

    bubble.place({
      ...resting,
      ...expandPanelFromSeed(
        {
          side: bubble.anchor.side as BubbleOpen["side"],
          offset: bubble.anchor.offset,
          x: bubble.anchor.x,
          y: bubble.anchor.y,
          w: bubble.anchor.w,
          h: bubble.anchor.h,
        },
        { w: resting.w, h: resting.h },
      ),
    });
    await afterTransition(bubEl, "width", growDur);
    if (epoch !== revealEpoch) return;

    revealPhase = "settle";
    await tick();
    await waitFrames(2);
    if (epoch !== revealEpoch) return;
    const settleProp = separateAxisProp(bubble.anchor?.side);
    bubble.place(resting);
    await afterTransition(bubEl, settleProp, settleDur);
    if (epoch !== revealEpoch) return;
    revealPhase = "ready";
  }

  /* ─── Agrandar / minimizar ──────────────────────────────────────────────
     Agrandar llena el área de trabajo del monitor actual (toggle restaura el
     marco previo). Minimizar colapsa a solo la barra vía CSS: el ancla lógica
     no baja de `BUBBLE_MIN_H`, así que el marco queda intacto y restaurar es
     quitar la clase. Redimensionar a mano o reabrir desde la pill los limpia. */
  type Frame = { x: number; y: number; w: number; h: number };
  let maximized = $state(false);
  let minimized = $state(false);
  let frameBeforeMax: Frame | null = null;

  async function animateFrame(target: Frame) {
    const epoch = ++modeResizeEpoch;
    modeResizing = true;
    await tick();
    bubble.setFrame(target.x, target.y, target.w, target.h);
    const a = bubble.anchor;
    if (a) restingOpen = { ...a, ...target, side: a.side as BubbleOpen["side"] };
    await wait(ms(MOTION.medium));
    if (epoch === modeResizeEpoch) modeResizing = false;
  }

  function workAreaAround(frame: Frame) {
    const cx = frame.x + frame.w / 2;
    const cy = frame.y + frame.h / 2;
    const area =
      workAreas.find(
        (a) => cx >= a.x && cx <= a.x + a.w && cy >= a.y && cy <= a.y + a.h,
      ) ??
      workAreas[0] ??
      ({ x: 0, y: 0, w: window.innerWidth, h: window.innerHeight } satisfies Area);
    return workAreaOf(area);
  }

  function clearSizeToggles() {
    maximized = false;
    minimized = false;
    frameBeforeMax = null;
  }

  function toggleMaximize() {
    const a = bubble.anchor;
    if (!a) return;
    if (minimized) minimized = false;
    if (maximized && frameBeforeMax) {
      const prev = frameBeforeMax;
      maximized = false;
      frameBeforeMax = null;
      void animateFrame(prev);
      return;
    }
    frameBeforeMax = { x: a.x, y: a.y, w: a.w, h: a.h };
    maximized = true;
    const work = workAreaAround(frameBeforeMax);
    void animateFrame({
      x: work.x + POSITION_MARGIN,
      y: work.y + POSITION_MARGIN,
      w: work.w - POSITION_MARGIN * 2,
      h: work.h - POSITION_MARGIN * 2,
    });
  }

  async function toggleMinimize() {
    if (!bubble.anchor) return;
    const epoch = ++modeResizeEpoch;
    // `is-mode-resizing` antes del flip de clase: así la altura transiciona.
    modeResizing = true;
    minimized = !minimized;
    await tick();
    await wait(ms(MOTION.medium));
    if (epoch === modeResizeEpoch) modeResizing = false;
  }

  type ResizeEdge = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";

  const bubble = new Bubble();

  let bubEl = $state<HTMLElement | null>(null);
  const { startDrag, endDrag } = createBubbleDrag(bubble, () => bubEl, {
    onEnd: savePosition,
  });

  /** Estirar el globo desde cualquier borde o esquina. */
  let resize: {
    edge: ResizeEdge;
    ox: number;
    oy: number;
    ax: number;
    ay: number;
    ow: number;
    oh: number;
    pointerId: number;
  } | null = null;

  function startResize(edge: ResizeEdge, event: PointerEvent) {
    if (event.button !== 0 || !bubble.anchor) return;
    event.preventDefault();
    event.stopPropagation();
    // Estirar a mano toma el control: el tamaño ya no es "maximizado".
    clearSizeToggles();
    const a = bubble.anchor;
    resize = {
      edge,
      ox: event.clientX,
      oy: event.clientY,
      ax: a.x,
      ay: a.y,
      ow: a.w,
      oh: a.h,
      pointerId: event.pointerId,
    };
    try {
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    } catch {
      /* ignore */
    }
    window.addEventListener("pointermove", onResizeMove);
    window.addEventListener("pointerup", endResize);
    window.addEventListener("pointercancel", endResize);
  }

  function onResizeMove(event: PointerEvent) {
    const r = resize;
    if (!r || !bubble.anchor) return;
    const dx = event.clientX - r.ox;
    const dy = event.clientY - r.oy;
    const east = r.edge.includes("e");
    const west = r.edge.includes("w");
    const south = r.edge.includes("s");
    const north = r.edge.includes("n");

    let x = r.ax;
    let y = r.ay;
    let w = r.ow;
    let h = r.oh;

    if (east) w = Math.max(BUBBLE_MIN_W, r.ow + dx);
    if (west) {
      w = Math.max(BUBBLE_MIN_W, r.ow - dx);
      x = r.ax + r.ow - w;
    }
    const minHeight =
      launcherView === "console"
        ? CONSOLE_MIN_H
        : browserOpen
          ? BROWSER_MIN_H
          : setupHeight(w);
    if (south) h = Math.max(minHeight, r.oh + dy);
    if (north) {
      h = Math.max(minHeight, r.oh - dy);
      y = r.ay + r.oh - h;
    }
    if (launcherView === "setup") h = Math.max(h, minHeight);

    bubble.setFrame(x, y, w, h);
  }

  function endResize() {
    const r = resize;
    if (!r) return;
    resize = null;
    window.removeEventListener("pointermove", onResizeMove);
    window.removeEventListener("pointerup", endResize);
    window.removeEventListener("pointercancel", endResize);
    const a = bubble.anchor;
    if (a) {
      if (launcherView === "console") consoleSize = { w: a.w, h: a.h };
      else if (browserOpen) browserSize = { w: a.w, h: a.h };
      else {
        setupWidth = a.w;
        saveSetupWidth();
      }
      if (!browserOpen) void saveAgentsBubbleSize(a.w, a.h);
      savePosition();
    }
  }

  function releaseOverlayKeyboard() {
    const ae = document.activeElement;
    if (ae instanceof HTMLElement && bubEl?.contains(ae)) ae.blur();
    surfaces.resetInteraction();
    window.dispatchEvent(new Event("atic-overlay-leave-text"));
  }

  function close() {
    if (!bubble.shown) return;
    revealEpoch += 1;
    modeResizeEpoch += 1;
    modeResizing = false;
    revealPhase = "ready";
    clearSizeToggles();
    endDrag();
    endResize();
    releaseOverlayKeyboard();
    bubble.hide();
    void hideAgentsWindow();
    agents.watch(null);
  }

  /**
   * El contenido NO se desmonta al cerrar el float: las PTYs viven en Rust y
   * el xterm conserva su scrollback mientras el componente exista. Cerrar la
   * ventana solo la oculta (`is-off`); reabrir desde la pill muestra las
   * consolas tal como estaban. Se pierden al cerrar cada pestaña o al apagar
   * la app — no al esconder la ventana.
   */
  let everAlive = $state(false);
  $effect(() => {
    if (bubble.alive) everAlive = true;
  });

  $effect(() => {
    if (!bubble.alive) {
      if (revealPhase !== "hidden") revealPhase = "hidden";
      return;
    }
    if (bubble.shown && revealPhase === "hidden") void runOpenReveal();
  });

  /** Cierre por intención (clic afuera / Esc). Respeta pin y diálogos nativos. */
  function tryAutoClose() {
    if (!bubble.shown || isAgentsDismissSuppressed() || isOpenDismissGrace()) return;
    void agentsAlwaysOnTop()
      .then((pinned) => {
        if (pinned || isAgentsDismissSuppressed() || !bubble.shown) return;
        close();
      })
      .catch(() => {
        /* sin lectura del pin, no cerrar */
      });
  }

  $effect(() => {
    if (!bubble.alive || !bubEl) {
      liquid.publish("agents", []);
      return;
    }
    // Seguir el morph visual: el ancla lógica no escala al cerrar.
    void bubble.shown;
    void revealPhase;
    void bubble.anchor;
    if (motionPhase) {
      return publishFollowSkin("agents", bubEl, BUBBLE_CORNER);
    }
    return publishEmergeSkin("agents", bubEl, BUBBLE_CORNER);
  });

  $effect(() => {
    // Solo con el globo visible. Si queda `alive` al esconder (PTYs vivas),
    // publicar el rect de 680×520 arma el overlay sobre un hueco invisible
    // y la pill deja de recibir el mouse.
    if (!bubEl || !bubble.alive || !bubble.shown) return;
    const stop = surfaces.add("agents", bubEl);
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
      growDur + settleDur + 64,
    );
    return () => window.clearTimeout(t);
  });

  $effect(() => {
    void bubble.anchor;
    void surfaces.dragging;
    if (surfaces.dragging) return;
    surfaces.schedule();
  });

  onMount(() => {
    applyTheme(readCachedTheme());
    setupWidth = readSetupWidth();
    void overlayWorkAreas()
      .then((areas) => {
        workAreas = areas;
      })
      .catch(() => {
        workAreas = [];
      });
    const un: Promise<() => void>[] = [
      // Cambió la geometría del overlay (monitores, hibernación): el caché de
      // áreas quedó en el espacio viejo y `ensureWorkAreas` no recarga si ya
      // hay algo. Refrescar acá deja la próxima colocación en el espacio real.
      onOverlayReady(() => {
        void overlayWorkAreas()
          .then((areas) => {
            workAreas = areas;
          })
          .catch(() => {});
      }),
      onAgentsBubbleAnchor((a) => void placeFromPill(a)),
      onAgentsBubbleDismiss(() => {
        close();
      }),
      // Clic afuera (Raw Input → overlay-dismiss). Pin / diálogo nativo → no.
      onOverlayDismiss(() => {
        tryAutoClose();
      }),
    ];
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !bubble.shown) return;
      // Esc: cierre explícito solo si no está fijada (panel sticky).
      if (isAgentsDismissSuppressed()) return;
      // Consola PTY / xterm: AgentsDemo maneja Esc (cerrar consola); no cerrar el float.
      const t = e.target as HTMLElement | null;
      if (t?.closest?.(".console, .xterm")) return;
      e.preventDefault();
      void agentsAlwaysOnTop()
        .then((pinned) => {
          if (!pinned && bubble.shown) close();
        })
        .catch(() => {
          /* sin pin, no cerrar */
        });
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      endDrag();
      endResize();
      for (const p of un) void p.then((fn) => fn());
      liquid.publish("agents", []);
      agents.watch(null);
    };
  });
</script>

{#if bubble.alive || everAlive}
  <div
    class="af"
    class:is-shown={bubble.shown}
    class:is-off={!bubble.alive}
    class:is-expanding={expanding}
    class:is-settling={settling}
    class:is-mode-resizing={modeResizing}
    class:is-minimized={minimized}
    data-agents-float
    data-side={bubble.anchor?.side ?? "top"}
    style={bubble.vars}
    style:--agents-grow-dur="{growDur}ms"
    style:--agents-settle-dur="{settleDur}ms"
    bind:this={bubEl}
  >
    <div class="af-stage">
      <AgentLauncher
        onHeaderPointerDown={startDrag}
        onClose={close}
        onViewChange={(view) => void changeLauncherView(view)}
        onBrowserChange={(open) => void changeBrowser(open)}
        onToggleMaximize={toggleMaximize}
        onToggleMinimize={() => void toggleMinimize()}
        {maximized}
        {minimized}
        shown={bubble.shown}
      />
      <!-- local: sin popover/viewport; el overlay es fullscreen y el toast
         quedaría abajo de toda la pantalla, lejos del bubble. -->
      <ToastStack
        placement="local"
        items={toasts.items}
        onDismiss={(id) => toasts.dismiss(id)}
      />
    </div>
    <!-- Agarraderas: los 4 bordes y las 4 esquinas. -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="grip grip-n"
      data-no-drag
      onpointerdown={(e) => startResize("n", e)}
    ></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="grip grip-s"
      data-no-drag
      onpointerdown={(e) => startResize("s", e)}
    ></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="grip grip-e"
      data-no-drag
      onpointerdown={(e) => startResize("e", e)}
    ></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="grip grip-w"
      data-no-drag
      onpointerdown={(e) => startResize("w", e)}
    ></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="grip grip-ne"
      data-no-drag
      onpointerdown={(e) => startResize("ne", e)}
    ></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="grip grip-nw"
      data-no-drag
      onpointerdown={(e) => startResize("nw", e)}
    ></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="grip grip-se"
      data-no-drag
      onpointerdown={(e) => startResize("se", e)}
    ></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="grip grip-sw"
      data-no-drag
      onpointerdown={(e) => startResize("sw", e)}
    ></div>
  </div>
{/if}

<style>
  .af {
    /* Sobreimpulso sutil solo en el tamaño: el panel "respira" al abrirse.
       La posición va en smooth-out para que la trayectoria no serpentee. */
    --ease-emerge: cubic-bezier(0.3, 1.18, 0.36, 1);

    position: absolute;

    /* Bajo la pill (a diferencia de los otros floats): esta ventana es grande
       y puede taparla entera; la pill y sus elementos siempre quedan visibles. */
    z-index: calc(var(--z-overlay-pill) - 1);
    left: var(--x);
    top: var(--y);
    width: var(--w);
    height: var(--h);
    box-sizing: border-box;

    /* Columna: el contenido (launcher / consola) llena con flex:1. Sin esto
       la consola colapsa a la altura de su barra (~34px). */
    display: flex;
    flex-direction: column;
    border-radius: 1.625rem;
    /* Transparente: un fill opaco tapa la sombra de la piel y deja un
       hairline en el cuello fundido con la pill. */
    background: transparent;
    overflow: hidden;
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--float-close-dur) var(--ease-smooth-out);
  }

  .af.is-shown {
    opacity: 1;
    pointer-events: auto;
  }

  .af.is-expanding {
    transition:
      width var(--agents-grow-dur) var(--ease-emerge),
      height var(--agents-grow-dur) var(--ease-emerge),
      left var(--agents-grow-dur) var(--ease-smooth-out),
      top var(--agents-grow-dur) var(--ease-smooth-out),
      opacity var(--duration-quick) var(--ease-smooth-out);
  }

  .af.is-settling {
    transition:
      left var(--agents-settle-dur) var(--ease-smooth-out),
      top var(--agents-settle-dur) var(--ease-smooth-out),
      width var(--duration-quick) var(--ease-smooth-out),
      height var(--duration-quick) var(--ease-smooth-out),
      opacity var(--duration-quick) var(--ease-smooth-out);
  }

  .af.is-mode-resizing {
    transition:
      left var(--duration-medium) var(--ease-smooth-out),
      top var(--duration-medium) var(--ease-smooth-out),
      width var(--duration-medium) var(--ease-smooth-out),
      height var(--duration-medium) var(--ease-smooth-out),
      opacity var(--duration-quick) var(--ease-smooth-out);
  }

  /* Colapsado a solo la barra. La altura se pisa por CSS (el ancla lógica no
     baja de BUBBLE_MIN_H); el contenido bajo la barra queda recortado. */
  .af.is-minimized {
    height: 52px;
  }

  .af.is-shown.is-minimized {
    overflow: hidden;
  }

  .af-stage {
    display: flex;
    min-width: 0;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    border-radius: inherit;
    opacity: 0;
    transform: translateY(-8px) scale(0.985);
    transform-origin: var(--tail, 50%) 0;
    pointer-events: none;
    transition:
      opacity var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-fast) var(--ease-smooth-out);
  }

  /* El contenido entra DESDE la pill: `side` dice en qué borde quedó el
     cuello (top = pill arriba del panel), y el origen sigue a --tail. */
  .af[data-side="bottom"] .af-stage {
    transform: translateY(8px) scale(0.985);
    transform-origin: var(--tail, 50%) 100%;
  }

  .af[data-side="left"] .af-stage {
    transform: translateX(-8px) scale(0.985);
    transform-origin: 0 var(--tail, 50%);
  }

  .af[data-side="right"] .af-stage {
    transform: translateX(8px) scale(0.985);
    transform-origin: 100% var(--tail, 50%);
  }

  .af.is-shown:not(.is-expanding) .af-stage {
    opacity: 1;
    transform: none;
    pointer-events: auto;
    transition-delay: 36ms;
  }

  /* Oculto pero vivo: las PTYs siguen corriendo. Sin pointer-events ni
     visibilidad, el overlay no arma clics sobre una ventana que no está. */
  .af.is-off {
    visibility: hidden;
    pointer-events: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .af,
    .af.is-expanding,
    .af.is-settling,
    .af.is-mode-resizing,
    .af-stage,
    .af.is-shown:not(.is-expanding) .af-stage {
      transition: none;
      transform: none;
    }
  }

  .grip {
    position: absolute;

    /* Bajo el header (.top-acts z 9): no robar pin/cerrar. */
    z-index: 7;
    background: transparent;
  }

  .grip-n,
  .grip-s {
    left: 10px;
    right: 10px;
    height: 6px;
    cursor: ns-resize;
  }

  .grip-n {
    top: 0;
  }

  .grip-s {
    bottom: 0;
  }

  .grip-e,
  .grip-w {
    /* Debajo del header (~top-ctrl + padding): no tapar pin / Bypass / X. */
    top: 40px;
    bottom: 10px;
    width: 6px;
    cursor: ew-resize;
  }

  .grip-e {
    right: 0;
  }

  .grip-w {
    left: 0;
  }

  .grip-ne,
  .grip-nw,
  .grip-se,
  .grip-sw {
    width: 14px;
    height: 14px;
  }

  .grip-nw {
    top: 0;
    left: 0;
    cursor: nwse-resize;
  }

  .grip-ne {
    top: 0;
    right: 0;
    cursor: nesw-resize;
  }

  .grip-sw {
    bottom: 0;
    left: 0;
    cursor: nesw-resize;
  }

  .grip-se {
    bottom: 0;
    right: 0;
    cursor: nwse-resize;
  }
</style>
