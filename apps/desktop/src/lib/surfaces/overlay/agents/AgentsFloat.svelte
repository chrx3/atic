<script lang="ts">
  /**
   * Shell del float: bubble + liquid + surfaces + drag.
   * Por ahora hospeda la misma demo visual que la ventana principal.
   */
  import { onMount, tick } from "svelte";
  import { emit } from "@tauri-apps/api/event";
  import { agents } from "$lib/agentSessions.svelte";
  import { presence } from "$lib/agentPresence.svelte";
  import {
    agentsAlwaysOnTop,
    hideAgentsWindow,
    onAgentsBubbleAnchor,
    onAgentsBubbleDismiss,
    onAgentsBubbleExpand,
    onAgentsTransfer,
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
  import { liquid, LIQUID_HUB } from "$surfaces/overlay/group.svelte";
  import { publishEmergeSkin } from "$surfaces/overlay/floatEmergeSkin";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";
  import {
    createDetachHandoffGate,
    nextDetachHandoff,
    notifyToolResting,
    toolResting,
  } from "$surfaces/overlay/toolBirth";
  import { Bubble, BUBBLE_MIN_W } from "$surfaces/overlay/bubble.svelte";
  import { createBubbleDrag } from "$surfaces/overlay/bubbleDrag";
  import { snapFrame, snapTarget } from "$surfaces/overlay/floatSnap";
  import { snapPreview } from "$surfaces/overlay/snapPreview.svelte";
  import {
    placePanelFusedFull,
    placePanelResting,
  } from "$surfaces/overlay/floatPlace";
  import { gapBetween } from "$lib/liquid/geometry";
  import {
    createRetachGesture,
    retachReady,
    trackRetach,
  } from "$surfaces/overlay/retachMagnet";
  import { showRetachPreview } from "$surfaces/overlay/retachPreview.svelte";
  import { REACH } from "$lib/liquid/constants";
  import AgentLauncher from "$features/agents/AgentLauncher.svelte";
  import { isAgentsDismissSuppressed } from "$surfaces/overlay/agents/dismissGuard";
  import { agentsDock } from "$surfaces/overlay/agents/agentsDock.svelte";
  import { agentsIslandHost, agentsFloatHandoff, agentsFloatLive } from "./agentsIslandHost.svelte";
  import { presenceIdsToDismissOnAticHide } from "$surfaces/overlay/pill/pillAgentChip";
  import {
    reuseDockedFrame,
    shouldResizeLauncher,
    rememberedSetupWidth,
  } from "$surfaces/overlay/agents/dockExpand";
  import { toasts } from "$domain/toasts.svelte";
  import { t } from "$domain/i18n.svelte";
  import {
    transferInbox,
    type TransferPayload,
  } from "$features/agents/consoleTransfer.svelte";
  import ToastStack from "$ui/ToastStack.svelte";
  import { MOTION, ms, prefersReducedMotion, wait } from "$lib/motion";
  import {
    armOpenDismissGrace,
    isOpenDismissGrace,
  } from "$surfaces/overlay/openDismissGrace";

  const BUBBLE_CORNER = 26;
  const POSITION_STORAGE_KEY = "atic.agents.consolePosition";
  const SETUP_WIDTH_STORAGE_KEY = "atic.agents.setupWidth";
  const POSITION_MARGIN = 12;
  const SETUP_DEFAULT_W = 400;
  /* La estructura del lanzador es la misma a todo ancho (elegir /
     condicionar / commit), así que una sola altura lo cubre: los anchos
     grandes respiran en la carpeta, no en más alto. */
  const SETUP_H = 208;
  const BROWSER_DEFAULT_W = 680;
  const BROWSER_DEFAULT_H = 620;
  const CONSOLE_DEFAULT_W = 680;
  const CONSOLE_DEFAULT_H = 520;
  const CONSOLE_MIN_H = 340;
  /** Imán del re-acople (reglas y porqués en `retachMagnet`). */
  let retachGesture = createRetachGesture();
  function magnetPill() {
    return surfaces.live["pill-skin"] ?? surfaces.live["pill"];
  }
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
  let liveConsoles = $state(false);
  const resizable = $derived(launcherView === "console");

  type RevealPhase = "hidden" | "ready";
  let revealPhase = $state<RevealPhase>("hidden");
  /** Entró por despegue: nace en el rect de la cara, sin morph de nacimiento. */
  let detachDirect = false;
  /**
   * El gesto de detach ya traspasó al motor de arrastre: el panel es de la
   * mano. Un ancla o expand tardío no puede re-colocarlo ni re-correr el
   * morph (lo devuelve a la cara a mitad del gesto y el arrastre queda con
   * ese offset). Se baja al soltar.
   */
  let detachDragActive = false;

  type SavedPosition = { x: number; y: number };

  function readSetupWidth(): number {
    const saved = Number(localStorage.getItem(SETUP_WIDTH_STORAGE_KEY));
    return rememberedSetupWidth(saved, SETUP_DEFAULT_W);
  }

  function setupPanelWidth(): number {
    return rememberedSetupWidth(setupWidth, SETUP_DEFAULT_W);
  }

  function saveSetupWidth() {
    try {
      setupWidth = setupPanelWidth();
      localStorage.setItem(SETUP_WIDTH_STORAGE_KEY, String(Math.round(setupWidth)));
    } catch {
      /* El tamaño compacto sigue funcionando aunque el storage esté bloqueado. */
    }
  }

  function savePosition() {
    const a = bubble.anchor;
    if (!a || minimized) return;
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

  function resolveRestingOpen(a: BubbleOpen, keep: SavedPosition | null): BubbleOpen {
    const panel = { w: a.w, h: a.h };
    if (keep) {
      const pill = surfaces.live["pill-skin"] ??
        surfaces.live["pill"] ?? {
          x: a.x,
          y: a.y,
          w: 1,
          h: 1,
        };
      return { ...a, ...positionInWorkspace(pill, panel, keep) };
    }
    // Sin última posición: al lado de la pill, como el resto de los floats.
    // El centro de pantalla era el pedido viejo y ya no va.
    const pill = surfaces.live["pill-skin"] ?? surfaces.live["pill"];
    if (!pill) return { ...a, ...panel };
    return {
      ...a,
      ...placePanelResting(pill, panel, { corner: BUBBLE_CORNER, work: workAreas }),
    };
  }

  async function ensureWorkAreas() {
    if (workAreas.length > 0) return;
    try {
      workAreas = await overlayWorkAreas();
    } catch {
      workAreas = [];
    }
  }

  function setupHeight(): number {
    return SETUP_H;
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
        w: Math.max(BROWSER_DEFAULT_W, browserSize.w, a.w),
        h: Math.max(BROWSER_DEFAULT_H, browserSize.h),
      };
    }
    return { ...a, w: setupPanelWidth(), h: setupHeight() };
  }

  let placeEpoch = 0;

  async function placeFromPill(a: BubbleOpen) {
    // El arrastre del detach es dueño de la geometría: un ancla repetida
    // (present con OPEN, reanchor) no re-coloca a mitad del gesto.
    if (detachDragActive) return;
    if (agentsIslandHost.on) {
      void hideAgentsWindow().catch(() => {});
      return;
    }
    // Despegue: el rect de la cara manda sobre todo lo demás (incluido un
    // marco dockeado viejo). El panel ya vive en su rect exacto (era la
    // cara): sin semilla ni morph, el mismo elemento cortado en dos.
    const rest = toolResting();
    if (rest) {
      detachDirect = true;
      // Un dock previo (retach con sesiones del hub) deja `minimized`: sin
      // bajarlo, `.is-docked` esconde el panel que acaba de nacer.
      minimized = false;
      agentsDock.setMinimized(false);
      armOpenDismissGrace();
      bubble.place({
        ...a,
        x: rest.x,
        y: rest.y,
        w: rest.w,
        h: rest.h,
        side: "top",
        offset: rest.w / 2,
      });
      // Sin frame replegado: nace ya mostrado en su rect, como si la cara
      // nunca se hubiera ido.
      bubble.shown = true;
      return;
    }
    detachDirect = false;
    if (
      reuseDockedFrame({
        minimized,
        alive: bubble.alive,
        hasAnchor: bubble.anchor != null,
      })
    ) {
      showAgentsPanel();
      notifyToolResting();
      return;
    }
    const epoch = ++placeEpoch;
    clearSizeToggles();
    a = frameForView(a);
    const fresh = !bubble.alive || !bubble.shown;
    if (fresh) armOpenDismissGrace();

    await ensureWorkAreas();
    if (epoch !== placeEpoch) return;

    const keep =
      !fresh && bubble.anchor ? { x: bubble.anchor.x, y: bubble.anchor.y } : null;
    restingOpen = resolveRestingOpen(a, keep);

    if (fresh || revealPhase === "hidden") {
      // Nace directo en su reposo (fade): ya no crece desde la semilla de la
      // pill anclado a una esquina.
      bubble.place(restingOpen);
      return;
    }
    if (revealPhase === "ready") {
      bubble.place(restingOpen);
      notifyToolResting();
    }
  }

  function asOpen(a: {
    side: string;
    offset: number;
    x: number;
    y: number;
    w: number;
    h: number;
  }): BubbleOpen {
    const side: BubbleOpen["side"] =
      a.side === "bottom" || a.side === "left" || a.side === "right" ? a.side : "top";
    return { ...a, side };
  }

  async function animateToSize(
    current: {
      side: string;
      offset: number;
      x: number;
      y: number;
      w: number;
      h: number;
    },
    size: { w: number; h: number },
  ) {
    await ensureWorkAreas();
    const target = {
      ...asOpen(current),
      ...positionInWorkspace(
        surfaces.live["pill-skin"] ??
          surfaces.live["pill"] ?? { x: current.x, y: current.y, w: 1, h: 1 },
        size,
        { x: current.x, y: current.y },
      ),
      ...size,
    };
    const epoch = ++modeResizeEpoch;
    modeResizing = true;
    await tick();
    bubble.setFrame(target.x, target.y, target.w, target.h);
    restingOpen = target;
    await wait(ms(MOTION.slow));
    if (epoch === modeResizeEpoch) modeResizing = false;
  }

  async function changeLauncherView(next: LauncherView) {
    const current = bubble.anchor;
    if (
      !shouldResizeLauncher({
        current: launcherView,
        next,
        height: current?.h,
        minConsoleHeight: CONSOLE_MIN_H,
      })
    ) {
      return;
    }

    const prev = launcherView;
    launcherView = next;
    if (next === "console") browserOpen = false;
    // Sin marco no hay a dónde crecer.
    if (!current) return;

    clearSizeToggles();

    if (prev !== next) {
      if (next === "console") {
        if (current.w >= SETUP_DEFAULT_W) {
          setupWidth = current.w;
          saveSetupWidth();
        }
      } else if (current.h >= CONSOLE_MIN_H) {
        consoleSize = {
          w: Math.max(CONSOLE_DEFAULT_W, current.w),
          h: Math.max(CONSOLE_DEFAULT_H, current.h),
        };
      }
    }

    const setupW = setupPanelWidth();
    const size =
      next === "console"
        ? {
            w: Math.max(CONSOLE_DEFAULT_W, consoleSize.w, current.w),
            h: Math.max(CONSOLE_DEFAULT_H, consoleSize.h),
          }
        : {
            w: setupW,
            h: setupHeight(),
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
      if (current.w >= SETUP_DEFAULT_W) {
        setupWidth = current.w;
        saveSetupWidth();
      }
    } else browserSize = { w: current.w, h: current.h };

    const setupW = setupPanelWidth();
    const width = open ? Math.max(BROWSER_DEFAULT_W, browserSize.w, current.w) : setupW;
    const size = open
      ? { w: width, h: Math.max(BROWSER_DEFAULT_H, browserSize.h) }
      : { w: setupW, h: setupHeight() };
    await animateToSize(current, size);
  }

  function restingForView(): BubbleOpen | null {
    return restingOpen ? frameForView(restingOpen) : null;
  }

  function runOpenReveal() {
    const initial = restingForView();
    if (!initial) return;
    // Despegue: el ancla ya quedó en el rect de la cara (`placeFromPill`).
    // La bandera es local: el `rest` global ya se limpió cuando corre esto.
    // Si no, directo al reposo: el panel aparece con el fade de `.is-shown`
    // en su lugar, sin crecer desde la semilla de la pill.
    if (!detachDirect) bubble.place(initial);
    revealPhase = "ready";
    notifyToolResting();
  }

  /* ─── Agrandar / minimizar ──────────────────────────────────────────────
     Agrandar llena el área de trabajo. Minimizar esconde el panel y deja
     una pestaña en la pill (no una gota suelta: esa se fundía con el
     launcher). No hay X: Esc, clic afuera y hide_agents_window también
     dockean. Las PTYs siguen vivas. */
  type Frame = { x: number; y: number; w: number; h: number };
  let maximized = $state(false);
  let minimized = $state(false);
  /** Encajada a un canto/esquina (incluye maximizada). */
  let snapped = $state(false);
  let frameBeforeMax: Frame | null = null;

  async function animateFrame(target: Frame) {
    const epoch = ++modeResizeEpoch;
    modeResizing = true;
    await tick();
    bubble.setFrame(target.x, target.y, target.w, target.h);
    const a = bubble.anchor;
    if (a) restingOpen = { ...a, ...target, side: a.side as BubbleOpen["side"] };
    await wait(ms(MOTION.slow));
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
    snapped = false;
    frameBeforeMax = null;
    snapPreview.frame = null;
    agentsDock.setMinimized(false);
  }

  /** Vacía la piel líquida y republica hits: sin esto queda el blob fantasma. */
  function clearAgentsOverlaySkin() {
    liquid.publish("agents", []);
    void tick().then(() => {
      void surfaces.flush();
      void surfaces.recoverHits();
    });
  }

  /**
   * Dockear devuelve el tamaño al de antes de agrandar.
   *
   * `restingOpen` es el marco al que crece `runOpenReveal`, y agrandar lo deja
   * en el área de trabajo entera. Como dockear no limpiaba los toggles, la
   * pestaña de la pill reabría a pantalla completa: el usuario minimizaba una
   * consola normal y volvía con toda la altura del monitor.
   *
   * Sin agrandar, el reposo es el marco actual: tras un despegue arrastrado
   * `restingOpen` quedaba viejo y la pestaña reabría lejos de donde se soltó.
   */
  function collapseMaximizedForDock() {
    const prev = frameBeforeMax;
    maximized = false;
    snapped = false;
    frameBeforeMax = null;
    snapPreview.frame = null;
    const current = bubble.anchor ? asOpen(bubble.anchor) : null;
    if (!prev) {
      if (current) restingOpen = current;
      return;
    }
    const base = restingOpen ?? current;
    if (base) restingOpen = { ...base, ...prev };
  }

  function dismissAticConsoleCues() {
    agents.markAllRead();
    presence.markSeenMany(presenceIdsToDismissOnAticHide(presence.view));
  }

  function dockToPill() {
    if (minimized) return;
    // Se apaga en su lugar (fade de `.is-docked`): ya no se encoge hacia la
    // esquina de la pill. El marco queda donde estaba para volver ahí.
    collapseMaximizedForDock();
    releaseOverlayKeyboard();
    revealPhase = "ready";
    minimized = true;
    agentsDock.setMinimized(true);
    bubble.shown = false;
    clearAgentsOverlaySkin();
    dismissAticConsoleCues();
  }

  /** El lanzador (antes de abrir una consola): X cierra, no achica. */
  function dismissSetup() {
    if (liveConsoles || agents.sessions.length > 0) {
      dockToPill();
      return;
    }
    close();
  }

  /** Agranda si hay globo vivo. Si no, el ancla de Rust tiene que nacerlo. */
  function showAgentsPanel() {
    const wasDocked = minimized;
    minimized = false;
    agentsDock.setMinimized(false);
    if (!bubble.alive || !bubble.anchor) return;
    // Desde el dock el marco quedó en la semilla: hay que crecer otra vez.
    // Ya abierto, no re-disparar el morph.
    revealPhase = wasDocked ? "hidden" : "ready";
    bubble.shown = true;
    void tick().then(() => {
      void surfaces.flush();
      void surfaces.recoverHits();
    });
  }

  function expandFromDock() {
    showAgentsPanel();
  }

  function toggleMaximize() {
    const a = bubble.anchor;
    if (!a) return;
    if (minimized) expandFromDock();
    if (maximized && frameBeforeMax) {
      const prev = frameBeforeMax;
      maximized = false;
      snapped = false;
      frameBeforeMax = null;
      void animateFrame(prev);
      return;
    }
    if (!frameBeforeMax) {
      frameBeforeMax = { x: a.x, y: a.y, w: a.w, h: a.h };
    }
    maximized = true;
    snapped = true;
    const work = workAreaAround(frameBeforeMax);
    void animateFrame({
      x: work.x + POSITION_MARGIN,
      y: work.y + POSITION_MARGIN,
      w: work.w - POSITION_MARGIN * 2,
      h: work.h - POSITION_MARGIN * 2,
    });
  }

  function toggleMinimize() {
    if (!bubble.anchor) return;
    if (minimized) expandFromDock();
    else dockToPill();
  }

  type ResizeEdge = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";

  const bubble = new Bubble();

  let bubEl = $state<HTMLElement | null>(null);
  const { startDrag, endDrag } = createBubbleDrag(bubble, () => bubEl, {
    clamp: "visible",
    onGrab: ({ cursor, setHome }) => {
      // El gesto arranca: lo que ya está lejos (marco o cursor) arma desde ya.
      retachGesture = createRetachGesture();
      trackRetach(retachGesture, magnetPill(), bubble.anchor, cursor);
      modeResizeEpoch += 1;
      modeResizing = false;
      // Agarrar una ventana agrandada la devuelve a su tamaño previo, como
      // hace Windows. `maximized` va aparte de `snapped` porque el botón de
      // agrandar deja las dos, pero un rescate futuro podría dejar solo una.
      if ((!snapped && !maximized) || !frameBeforeMax) return;
      const prev = frameBeforeMax;
      maximized = false;
      snapped = false;
      // Sin esto el marco previo queda viejo: `onDrop` solo lo resiembra si
      // está vacío, y el próximo restaurar volvería a un tamaño de hace rato.
      frameBeforeMax = null;
      const nx = cursor.x - prev.w / 2;
      const ny = cursor.y - 24;
      bubble.setFrame(nx, ny, prev.w, prev.h);
      setHome(nx, ny);
    },
    onMove: ({ cursor, frame, areas }) => {
      const pill = magnetPill();
      trackRetach(retachGesture, pill, frame, cursor);
      const ready = retachReady(retachGesture, pill, frame, cursor);
      showRetachPreview("agents", ready);
      // Soltar ahí coloca en la isla: el fantasma del snap mentiría.
      if (ready) {
        snapPreview.frame = null;
        return;
      }
      // El lanzador compacto tiene alto fijo (`setupHeight`). Snapearlo a
      // pantalla completa o a la mitad lo deforma y el layout no lo aguanta.
      if (!resizable) {
        snapPreview.frame = null;
        return;
      }
      const hit = snapTarget(cursor, areas);
      snapPreview.frame = hit ? snapFrame(hit.kind, hit.work, POSITION_MARGIN) : null;
    },
    onDrop: ({ cursor, areas }) => {
      snapPreview.frame = null;
      // Imán armado y en rango, o cursor sobre la pill: se coloca en la isla.
      if (retachReady(retachGesture, magnetPill(), bubble.anchor, cursor)) {
        retachGesture = createRetachGesture();
        void retachToIsland();
        return;
      }
      if (!resizable) {
        savePosition();
        return;
      }
      const hit = snapTarget(cursor, areas);
      if (!hit) {
        savePosition();
        return;
      }
      const a = bubble.anchor;
      if (a && !frameBeforeMax) {
        frameBeforeMax = { x: a.x, y: a.y, w: a.w, h: a.h };
      }
      maximized = hit.kind === "max";
      snapped = true;
      const dest = snapFrame(hit.kind, hit.work, POSITION_MARGIN);
      void animateFrame(dest).then(() => savePosition());
    },
    onEnd: () => showRetachPreview("agents", false),
  });

  /**
   * Re-acople: el panel viaja hasta fundirse con la pill y recién ahí pide la
   * cara. La pill orquesta el resto (muda las consolas, abre la cara y apaga
   * este float), así que el apagado ocurre junto a la isla y no donde se soltó.
   */
  async function retachToIsland(): Promise<void> {
    const pill = magnetPill();
    const a = bubble.anchor;
    if (pill && a && !prefersReducedMotion()) {
      const fused = placePanelFusedFull(
        pill,
        { w: a.w, h: a.h },
        a.side as BubbleOpen["side"],
        { corner: BUBBLE_CORNER, work: workAreas },
      );
      await animateFrame({ x: fused.x, y: fused.y, w: fused.w, h: fused.h });
    }
    void emit("dock-tool-face", "agents").catch(() => {});
  }

  /* ─── Doble clic en la barra: agrandar / restaurar ───────────────────────
     No se usa el evento `dblclick` del DOM. Las dos barras que arrastran
     (`ConsolePanel.bar` y `AgentLauncher.drag-rail`) cancelan el default del
     `pointerdown` —lo necesitan para que arrastrar no seleccione texto ni
     empiece un drag nativo—, y con el default cancelado los eventos de
     compatibilidad del mouse dejan de ser confiables. Contar los `pointerdown`
     usa el mismo flujo que ya alimenta el arrastre, y de paso cubre las dos
     barras sin tocar ninguno de los dos componentes. */
  const DOUBLE_CLICK_MS = 400;
  /** Tolerancia de mano temblorosa: más que esto ya fue un arrastre corto. */
  const DOUBLE_CLICK_SLOP = 6;
  let lastHeaderClick: { t: number; x: number; y: number } | null = null;

  function onHeaderPointerDown(event: PointerEvent) {
    const prev = lastHeaderClick;
    const near =
      // Solo la consola: el lanzador tiene alto derivado de `setupHeight`, y
      // agrandarlo a la pantalla entera deja una caja vacía enorme.
      resizable &&
      prev !== null &&
      event.timeStamp - prev.t <= DOUBLE_CLICK_MS &&
      Math.hypot(event.clientX - prev.x, event.clientY - prev.y) <= DOUBLE_CLICK_SLOP;
    if (near) {
      lastHeaderClick = null;
      // El arrastre del primer clic ya se cerró en su `pointerup` sin haberse
      // movido, así que no queda gesto colgado; el `endDrag` es por las dudas.
      endDrag();
      toggleMaximize();
      return;
    }
    lastHeaderClick = { t: event.timeStamp, x: event.clientX, y: event.clientY };
    startDrag(event);
  }

  const pillSkin = $derived(surfaces.live["pill-skin"]);
  /**
   * Monitor de la pill: ahí van los avisos. El overlay abarca todos los
   * monitores y el pie del overlay caía entre dos pantallas.
   */
  const toastArea = $derived.by(() => {
    const pill = pillSkin ?? surfaces.live["pill"];
    return pill && workAreas.length > 0 ? workAreaAround(pill) : null;
  });
  /**
   * La pila de avisos vive fuera del bubble: cada aviso publica su propio
   * hit-rect para poder cerrarlo. Estable por instancia: un literal en el
   * markup re-registraría en cada render.
   */
  const toastSurface = {
    prefix: "agents-toasts",
    add: (id: string, el: HTMLElement) => surfaces.add(id, el),
  };
  const joined = $derived.by(() => {
    const a = bubble.anchor;
    const p = pillSkin;
    if (!a || !p || !bubble.alive) return false;
    return gapBetween(p, a) <= REACH;
  });

  /* ─── Bordes de redimensión ─────────────────────────────────────────────
     Antes esto eran ocho `div.grip` apilados en z 7, y el contenido los tapaba:
     `.composer` (z 8) y `.top-acts` (z 9) están deliberadamente por encima para
     no perder sus propios clics, así que el canto inferior NUNCA se podía
     agarrar y los laterales solo a medias. Apilar más alto rompía lo otro.

     La salida es no preguntarle al DOM quién está debajo del puntero, sino
     medir la distancia del puntero al marco. El gesto se decide por geometría
     en la fase de captura de la raíz, así que gana sobre cualquier contenido
     sin pelear por z-index — y solo en los pocos píxeles del canto, que es lo
     que hace una ventana de verdad. */
  const RESIZE_BAND = 8;
  /** El vértice se agarra desde más lejos: apuntar a 8×8 es incómodo. */
  const RESIZE_CORNER = 18;

  function edgeAt(clientX: number, clientY: number): ResizeEdge | null {
    const el = bubEl;
    if (!el) return null;
    const r = el.getBoundingClientRect();
    const l = clientX - r.left;
    const rt = r.right - clientX;
    const t = clientY - r.top;
    const b = r.bottom - clientY;
    if (l < 0 || rt < 0 || t < 0 || b < 0) return null;
    const h = l <= RESIZE_CORNER ? "w" : rt <= RESIZE_CORNER ? "e" : "";
    const v = t <= RESIZE_CORNER ? "n" : b <= RESIZE_CORNER ? "s" : "";
    if (h && v) return `${v}${h}` as ResizeEdge;
    if (t <= RESIZE_BAND) return "n";
    if (b <= RESIZE_BAND) return "s";
    if (l <= RESIZE_BAND) return "w";
    if (rt <= RESIZE_BAND) return "e";
    return null;
  }

  /** Qué borde toca el puntero al pasar, para pintar el cursor. */
  let hoverEdge = $state<ResizeEdge | null>(null);

  function onRootPointerDown(event: PointerEvent) {
    if (event.button !== 0 || !resizable || minimized) return;
    const edge = edgeAt(event.clientX, event.clientY);
    if (!edge) return;
    // Captura + stop: el contenido no llega a verlo, así que el arrastre por
    // la barra tampoco se dispara cuando el clic cayó en el canto.
    event.stopPropagation();
    startResize(edge, event);
  }

  function onRootPointerMove(event: PointerEvent) {
    // Durante el gesto manda el borde tomado, no el que haya bajo el puntero.
    if (resize) return;
    hoverEdge = resizable && !minimized ? edgeAt(event.clientX, event.clientY) : null;
  }

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
    if (event.button !== 0 || !bubble.anchor || minimized || !resizable) return;
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
      // La raíz, no el blanco del clic: el gesto lo decide la banda del canto,
      // y el elemento que hubiera debajo puede desmontarse a mitad del estirón.
      bubEl?.setPointerCapture(event.pointerId);
    } catch {
      /* ignore */
    }
    // El mismo blindaje que el arrastre, y por el mismo motivo: publica un
    // hit-rect a pantalla completa y arma el gesto en Rust sin esperar al IPC.
    // Sin esto, agrandar rápido saca el cursor del rect publicado —que solo se
    // remide un rAF más tarde, con los 6px de `ARM_MARGIN` de holgura—, el
    // overlay vuelve a ser click-through y el gesto se corta a mitad.
    surfaces.dragging = true;
    window.addEventListener("pointermove", onResizeMove);
    window.addEventListener("pointerup", endResize);
    window.addEventListener("pointercancel", endResize);
  }

  /**
   * Sigue con coordenadas del DOM, no con `overlayCursor()` como el arrastre.
   *
   * El arrastre necesita el cursor de Rust porque parte del origen absoluto del
   * globo y cerca del canto de la pantalla el webview se queda sin eventos. Acá
   * solo se usan DIFERENCIAS contra el punto de agarre, y el hit-rect a pantalla
   * completa que publica `startResize` mantiene vivo el flujo de `pointermove`
   * durante todo el gesto. Un viaje de IPC por cuadro no compraría nada.
   */
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
    // `resizable` exige la vista de consola, y entrar a ella apaga el
    // navegador: acá el mínimo es siempre el del terminal.
    if (south) h = Math.max(CONSOLE_MIN_H, r.oh + dy);
    if (north) {
      h = Math.max(CONSOLE_MIN_H, r.oh - dy);
      y = r.ay + r.oh - h;
    }

    bubble.setFrame(x, y, w, h);
  }

  function endResize() {
    const r = resize;
    if (!r) return;
    resize = null;
    window.removeEventListener("pointermove", onResizeMove);
    window.removeEventListener("pointerup", endResize);
    window.removeEventListener("pointercancel", endResize);
    // Devuelve el hit-rect a la silueta real: el setter fuerza la remedición.
    surfaces.dragging = false;
    const a = bubble.anchor;
    if (a) {
      // Mismo motivo que en `onResizeMove`: acá solo se llega desde la consola.
      consoleSize = { w: a.w, h: a.h };
      void saveAgentsBubbleSize(a.w, a.h);
      savePosition();
    }
  }

  function releaseOverlayKeyboard() {
    const ae = document.activeElement;
    if (ae instanceof HTMLElement && bubEl?.contains(ae)) ae.blur();
    surfaces.resetInteraction();
    window.dispatchEvent(new Event("atic-overlay-leave-text"));
  }

  /**
   * Solo desmontaje / apagado. La UI no llama esto: Esc y clic afuera
   * dockean. hide() deja `everAlive` y unpublish incompleto pintaba el ghost.
   */
  function close() {
    if (!bubble.shown && !minimized && !bubble.alive) return;
    modeResizeEpoch += 1;
    modeResizing = false;
    clearSizeToggles();
    endDrag();
    endResize();
    releaseOverlayKeyboard();
    revealPhase = "ready";
    liquid.publish("agents", []);
    bubble.hide();
    void hideAgentsWindow();
    agents.watch(null);
    dismissAticConsoleCues();
  }

  /**
   * El contenido NO se desmonta al achicar el float: las PTYs viven en Rust y
   * el xterm conserva su scrollback mientras el componente exista. Achicar
   * solo oculta el panel (`shown=false`); el chip de la pill lo restaura.
   * Se pierden al cerrar cada pestaña o al apagar la app — no al dockear.
   */
  let everAlive = $state(false);
  $effect(() => {
    if (bubble.alive) everAlive = true;
  });

  $effect(() => {
    if (!bubble.alive) {
      if (revealPhase !== "hidden") revealPhase = "hidden";
      detachDirect = false;
      return;
    }
    if (bubble.shown && revealPhase === "hidden") void runOpenReveal();
  });

  /** Clic afuera: en el lanzador cierra; en la consola achica. Respeta pin. */
  function tryAutoClose() {
    if (
      !bubble.shown ||
      minimized ||
      isAgentsDismissSuppressed() ||
      isOpenDismissGrace()
    )
      return;
    void agentsAlwaysOnTop()
      .then((pinned) => {
        if (pinned || isAgentsDismissSuppressed() || !bubble.shown) return;
        if (launcherView === "setup") dismissSetup();
        else dockToPill();
      })
      .catch(() => {
        /* sin lectura del pin, no achicar */
      });
  }

  $effect(() => {
    if (!bubble.alive || !bubEl || !bubble.shown) {
      liquid.publish("agents", []);
      return;
    }
    void bubble.shown;
    void bubble.anchor;
    const group = joined ? LIQUID_HUB : undefined;
    return publishEmergeSkin("agents", bubEl, BUBBLE_CORNER, group);
  });

  $effect(() => {
    if (bubble.shown) surfaces.bringToFront("agents");
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
      ms(MOTION.slow) + 64,
    );
    return () => window.clearTimeout(t);
  });

  $effect(() => {
    void bubble.anchor;
    void surfaces.dragging;
    if (surfaces.dragging) return;
    surfaces.schedule();
  });

  // El motor baja `dragging` por todos sus caminos (soltar, watchdog de 15s,
  // resetInteraction): la bandera del detach no puede quedar pegada aunque el
  // pointerup se pierda fuera del overlay. Pegada bloquearía las colocaciones
  // siguientes (float negro) y reviviría el offset.
  $effect(() => {
    if (!surfaces.dragging) detachDragActive = false;
  });

  onMount(() => {
    applyTheme(readCachedTheme());
    setupWidth = readSetupWidth();
    // Handoff del detach: la pill traspasa el gesto vivo directo al motor
    // de arrastre, sin evento DOM de por medio (el header aún no existe: la
    // consola llega después por el traspaso). La compuerta no acepta con el
    // marco viejo (el float dockeado sigue vivo con su última posición):
    // espera a que el detach lo coloque en el rect de la cara para que el
    // panel nazca bajo la mano y la siga sin offset.
    const handoffGate = createDetachHandoffGate();
    agentsFloatHandoff.current = (init) => {
      if (!nextDetachHandoff(handoffGate, bubble.anchor, toolResting()))
        return false;
      // Desde acá el panel es de la mano: ni el ancla repetida ni el expand
      // tardío lo tocan hasta soltar (ver `placeFromPill` y el expand).
      detachDragActive = true;
      startDrag(
        new PointerEvent("pointerdown", {
          button: 0,
          pointerId: init.pointerId,
          clientX: init.x,
          clientY: init.y,
          bubbles: true,
        }),
      );
      return true;
    };
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
        if (launcherView === "setup") dismissSetup();
        else dockToPill();
      }),
      onAgentsBubbleExpand(() => {
        // `present` con el globo abierto emite expand ANTES de reanclar: a
        // mitad de un detach correría el morph de nacimiento sobre el panel
        // que ya sigue a la mano (~1s perdido + offset). El detach es dueño.
        if (detachDragActive || toolResting()) return;
        expandFromDock();
      }),
      // Mudanza desde la principal: el lanzador adopta desde el buzón.
      onAgentsTransfer((raw) => {
        let payload: TransferPayload;
        try {
          payload = JSON.parse(raw) as TransferPayload;
        } catch {
          return;
        }
        if (!payload || payload.to !== "overlay") return;
        transferInbox.offer(payload);
      }),
      // Clic afuera (Raw Input → overlay-dismiss). Pin / diálogo nativo → no.
      onOverlayDismiss(() => {
        tryAutoClose();
      }),
    ];
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !bubble.shown || minimized) return;
      // Esc: achica a la pill solo si no está fijada (panel sticky).
      if (isAgentsDismissSuppressed()) return;
      // Consola PTY / xterm: AgentsDemo maneja Esc (cerrar consola); no achicar el float.
      const t = e.target as HTMLElement | null;
      if (t?.closest?.(".console, .xterm")) return;
      e.preventDefault();
      void agentsAlwaysOnTop()
        .then((pinned) => {
          if (!pinned && bubble.shown) {
            if (launcherView === "setup") dismissSetup();
            else dockToPill();
          }
        })
        .catch(() => {
          /* sin pin, no achicar */
        });
    };
    window.addEventListener("keydown", onKey);
    const unbindDock = agentsDock.bind(expandFromDock);
    return () => {
      unbindDock();
      if (agentsFloatHandoff.current) agentsFloatHandoff.current = null;
      window.removeEventListener("keydown", onKey);
      showRetachPreview("agents", false);
      endDrag();
      endResize();
      for (const p of un) void p.then((fn) => fn());
      close();
      liquid.publish("agents", []);
      agents.watch(null);
    };
  });
</script>

{#if bubble.alive || everAlive}
  <!-- Los handlers de puntero de la raíz son solo la banda de redimensión: el
       teclado ya agranda por el botón del header, que sí es un `button`. -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="af"
    class:is-shown={bubble.shown}
    class:is-off={!bubble.alive}
    class:is-mode-resizing={modeResizing}
    class:is-joined={joined}
    class:is-docked={minimized}
    data-float="agents"
    data-agents-float
    data-side={bubble.anchor?.side ?? "top"}
    data-edge={hoverEdge ?? undefined}
    style={bubble.vars}
    style:--float-stack={surfaces.stack("agents")}
    bind:this={bubEl}
    onpointerdowncapture={onRootPointerDown}
    onpointermove={onRootPointerMove}
    onpointerleave={() => (hoverEdge = null)}
  >
    <div class="af-stage">
      <AgentLauncher
        {onHeaderPointerDown}
        onClose={dismissSetup}
        onViewChange={(view) => void changeLauncherView(view)}
        onBrowserChange={(open) => void changeBrowser(open)}
        onToggleMaximize={toggleMaximize}
        onToggleMinimize={toggleMinimize}
        onLiveChange={(live) => {
          liveConsoles = live;
          agentsFloatLive.on = live;
        }}
        onNeedsAttention={(label) => toasts.push(t("page.agents.turnDone", { label }))}
        {maximized}
        {minimized}
        shown={bubble.shown}
      />
    </div>
  </div>
{/if}

<!--
  Avisos del overlay, fuera del ciclo del bubble: el float puede no nacer
  (la isla hospeda la consola) y el detach necesita feedback igual. Host del
  tamaño del área útil del monitor de la pill (anclaje `local`, al pie de esa
  pantalla), sin depender de la vida del float.
-->
<div
  class="toast-host"
  style:left={toastArea ? `${toastArea.x}px` : undefined}
  style:top={toastArea ? `${toastArea.y}px` : undefined}
  style:width={toastArea ? `${toastArea.w}px` : undefined}
  style:height={toastArea ? `${toastArea.h}px` : undefined}
>
  <ToastStack
    placement="local"
    surface={toastSurface}
    items={toasts.items}
    onDismiss={(id) => toasts.dismiss(id)}
  />
</div>

<style>
  /*
   * Host de los avisos: fixed (a pantalla completa hasta conocer el monitor
   * de la pill) para que la pila (`local`, `absolute inset-x-0 bottom-3`)
   * quede al pie. No recibe punteros; cada aviso publica su propio hit-rect
   * (`surface`) para que su X reciba clics.
   */
  .toast-host {
    position: fixed;
    inset: 0;
    z-index: var(--z-toast);
    pointer-events: none;
  }

  .af {
    position: absolute;

    /* En reposo, el stack de floats: el último tocado gana. Junto a la pill
       queda bajo ella para no taparla entera (esta ventana es grande). */
    z-index: calc(var(--z-overlay-float) + var(--float-stack, 0));
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
    visibility: visible;
    pointer-events: auto;
  }

  .af.is-joined {
    z-index: calc(var(--z-overlay-pill) - 1);
  }

  .af.is-mode-resizing {
    transition:
      left var(--duration-slow) var(--ease-smooth-out),
      top var(--duration-slow) var(--ease-smooth-out),
      width var(--duration-slow) var(--ease-smooth-out),
      height var(--duration-slow) var(--ease-smooth-out),
      opacity var(--duration-quick) var(--ease-smooth-out);
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

  .af.is-shown .af-stage {
    opacity: 1;
    transform: none;
    pointer-events: auto;
    transition-delay: 36ms;
  }

  /* Oculto pero vivo: las PTYs siguen corriendo. Sin pointer-events ni
     visibilidad, el overlay no arma clics sobre una ventana que no está.
     `visibility` espera a que termine la opacidad: si salta en el mismo
     cuadro, el fade de cierre no se ve. */
  .af.is-off,
  .af.is-docked {
    visibility: hidden;
    pointer-events: none;
    opacity: 0;
    transition:
      opacity var(--float-close-dur) var(--ease-smooth-out),
      visibility 0s linear var(--float-close-dur);
  }

  @media (prefers-reduced-motion: reduce) {
    .af,
    .af.is-mode-resizing,
    .af.is-off,
    .af.is-docked,
    .af-stage,
    .af.is-shown .af-stage {
      transition: none;
      transform: none;
    }
  }

  /* El cursor del canto: lo pone `data-edge`, que escribe `onRootPointerMove`.
     `cursor` se hereda, pero cualquier hijo con regla propia (botón, input,
     xterm) ganaría sobre la raíz y el usuario no vería el ns-/ew-resize justo
     donde el gesto SÍ va a tomarse. Mientras el puntero está en la banda, se
     fuerza a todo el subárbol a heredar. */
  .af[data-edge="n"],
  .af[data-edge="s"] {
    cursor: ns-resize;
  }

  .af[data-edge="e"],
  .af[data-edge="w"] {
    cursor: ew-resize;
  }

  .af[data-edge="nw"],
  .af[data-edge="se"] {
    cursor: nwse-resize;
  }

  .af[data-edge="ne"],
  .af[data-edge="sw"] {
    cursor: nesw-resize;
  }

  .af[data-edge] :global(*) {
    cursor: inherit;
  }
</style>
