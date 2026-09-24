<script lang="ts">
  /**
   * La ventana de agentes como pizarra: un espacio abierto donde cada
   * consola (el CLI de cada IA) flota, se mueve y cambia de tamaño.
   *
   * La pizarra es más grande que la ventana y se mira con una cámara
   * (corrimiento + zoom): se recorre arrastrando el fondo o con la rueda, y
   * se acerca con Ctrl+rueda o pellizcando. La consola que queda al centro de
   * lo que se mira toma el foco sola. A la izquierda flota la lista de
   * consolas; abajo, una sola entrada de texto que le escribe a la enfocada.
   *
   * Rust es el dueño de cada PTY; acá solo vive dónde está cada una. El chat
   * estructurado (`AgentsWorkspace`) queda guardado para cuando se retome ACP.
   */
  import { onMount, tick, untrack } from "svelte";
  import {
    cliOnPath,
    consoleAgentSession,
    consoleClose,
    consoleForParent,
    consoleForPresence,
    consoleTail,
    consoleWrite,
    onAgentsFocus,
    setAgentsWindowOpen,
  } from "$ipc/agents";
  import { onAgentsComposerInsert, onAgentsWindowInsert } from "$ipc/clipboard";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import type { AgentsComposerInsert } from "$core/types";
  import { t } from "$domain/i18n.svelte";
  import ConfirmDialog from "$ui/ConfirmDialog.svelte";
  import Icon from "$ui/Icon.svelte";
  import {
    ChevronRight,
    Folder,
    Forward,
    Minus,
    Plus,
    SquareTerminal,
  } from "$lib/icons";
  import AgentLogo from "./AgentLogo.svelte";
  import AgentSettingsModal from "./AgentSettingsModal.svelte";
  import BoardCard from "./BoardCard.svelte";
  import BoardComposer from "./BoardComposer.svelte";
  import BoardList from "./BoardList.svelte";
  import BoardMinimap from "./BoardMinimap.svelte";
  import BoardThreads from "./BoardThreads.svelte";
  import AgentChatPanel from "./AgentChatPanel.svelte";
  import { agents, type AgentSessionView } from "$lib/agentSessions.svelte";
  import { toasts } from "$domain/toasts.svelte";
  import BoardZoom from "./BoardZoom.svelte";
  import FolderBrowser from "./FolderBrowser.svelte";
  import TerminalView from "./TerminalView.svelte";
  import { AGENTS, type AgentDef } from "./agentCatalog";
  import {
    CHILD_SIZE,
    arrange,
    bounds,
    cardAt,
    centerOn,
    childRect,
    composerWrites,
    fitRect,
    focusedByView,
    canResume,
    launchCommand,
    fullyVisible,
    parentKind,
    parseCamera,
    parseSpaces,
    placeNew,
    placeSpace,
    quotePaths,
    upsertSpace,
    visibleArea,
    zoomAt,
    BACKDROPS,
    BACKDROP_TILE,
    type Arrangement,
    type Backdrop,
    type Camera,
    type Insets,
    type Rect,
    type SavedSpace,
    type Thread,
  } from "./agentBoard";
  import type { WorkspaceItem } from "./agentWorkspace";
  import { workspace } from "./agentWorkspace.svelte";
  import { presence } from "$lib/agentPresence.svelte";
  import type { PresenceStatus } from "$core/types";
  import { watchOutput } from "./consoleBus";
  import {
    consoleState,
    needsAttention,
    noteOutput,
    type Activity,
    type ConsoleState,
  } from "./consoleStatus";

  type TerminalItem = Extract<WorkspaceItem, { kind: "terminal" }>;

  const CWD_KEY = "atic.agents.startFolder";
  const CAMERA_KEY = "atic.agents.board.camera";
  const LIST_KEY = "atic.agents.board.listCollapsed";
  const LAYOUT_KEY = "atic.agents.board.layout";
  const START_COUNT_KEY = "atic.agents.board.startCount";
  const BACKDROP_KEY = "atic.agents.board.backdrop";
  /** Lo que tarda la vista en volver a esconder el minimapa tras moverse. */
  const VIEW_IDLE_MS = 1200;
  /** Tope de consolas de una vez, el mismo del lanzador de la pill. */
  const MAX_START = 6;
  const SPACES_KEY = "atic.agents.board.spaces";
  /** Más alejado que esto, tocar una consola la acerca: así no se lee. */
  const READABLE_ZOOM = 0.55;
  /** Lo que dura el vuelo de la cámara cuando se mueve sola. */
  const FLIGHT_MS = 250;

  let cwd = $state("");
  let installed = $state<Record<string, boolean>>({});
  let newOpen = $state(false);
  /** Terminales cuyo proceso terminó: siguen en la pizarra hasta cerrarlas. */
  let ended = $state<Record<string, boolean>>({});
  let pendingClose = $state<TerminalItem | null>(null);
  let settingsOpen = $state(false);
  let browsingFolder = $state(false);
  let listCollapsed = $state(false);
  /**
   * El acomodo elegido queda puesto: con fila, columna o grilla, lo que se
   * abre o se cierra se reacomoda solo. `free` = cada una donde se la deje.
   */
  let layout = $state<Arrangement | "free">("free");
  /** Cuántas abre de una vez la pizarra vacía. */
  let startCount = $state(1);
  let backdrop = $state<Backdrop>("dots");
  /** La vista se está moviendo: el minimapa y el zoom se muestran enteros. */
  let viewBusy = $state(false);
  let spaces = $state<SavedSpace[]>([]);

  let boardEl = $state<HTMLElement | null>(null);
  let size = $state({ w: 0, h: 0 });
  let cam = $state<Camera>({ x: 0, y: 0, zoom: 1 });
  /** La cámara se mueve sola (encuadrar, acomodar): con transición. */
  let flying = $state(false);
  /** El rectángulo en vivo mientras se arrastra; al soltar pasa al store. */
  let drafts = $state<Record<string, Rect>>({});
  /** Orden de apilado: la última es la de más arriba. */
  let stack = $state<string[]>([]);
  /** Falso mientras se escribe en la entrada: la terminal no roba el foco. */
  let termFocus = $state(true);
  /**
   * La consola que ocupa toda la vista, con la cámara de antes para volver.
   * Su rectángulo guardado no se toca: el grande es provisorio (`drafts`).
   */
  let maximized = $state<{ key: string; cam: Camera } | null>(null);
  /** La consola sobre la que se está por soltar algo, para marcarla. */
  let dropTarget = $state<string | null>(null);
  let composer = $state<{ insertText: (text: string) => void } | null>(null);

  // ── Sub-agentes ─────────────────────────────────────────────────────────
  //
  // Las sesiones que abre el MCP de Atic (o «Encargar a…») aparecen como
  // tarjetas junto a la consola que las pidió, unidas por un hilo.

  /** Encargos hechos desde la pizarra: sesión del sub-agente → consola. */
  let handed = $state<Record<string, string>>({});
  /** De cada `external:<cli>:<pid>`, la PTY donde corre (`null` = ninguna). */
  let externalConsole = $state<Record<string, string | null>>({});
  /** Lo que el usuario corrió o agrandó cada sub-agente. */
  let childOffsets = $state<
    Record<string, { dx: number; dy: number; w: number; h: number }>
  >({});
  let childDrafts = $state<Record<string, Rect>>({});
  /** Las terminales, para leer lo seleccionado al encargar. */
  const terms: Record<string, { selection: () => string }> = {};
  /** El menú de «Encargar a…» abierto, y dónde. */
  let delegateMenu = $state<{ key: string; x: number; y: number; text: string } | null>(
    null,
  );

  const children = $derived(agents.sessions.filter((s) => s.parent || handed[s.id]));

  /** La tarjeta de la que cuelga un sub-agente: una consola o `child:<id>`. */
  function anchorOf(session: AgentSessionView): string | null {
    if (handed[session.id]) return handed[session.id];
    const kind = parentKind(session.parent);
    if (kind === "session")
      return children.some((c) => c.id === session.parent)
        ? `child:${session.parent}`
        : null;
    if (kind === "external" && session.parent) {
      const pty = externalConsole[session.parent];
      return pty ? (workspace.findSession("terminal", pty)?.key ?? null) : null;
    }
    return null;
  }

  /**
   * Dónde va cada sub-agente: a la derecha de quien lo pidió, en columna;
   * los que no cuelgan de ninguna consola, a la derecha de todo. En pasadas,
   * porque uno puede colgar de otro sub-agente.
   */
  const childRects = $derived.by(() => {
    const out: Record<string, Rect> = {};
    const anchors = Object.fromEntries(children.map((c) => [c.id, anchorOf(c)]));
    const all = bounds(terminals.map(rectOf));
    const orphanBase = all
      ? { x: all.x + all.w, y: all.y, w: 0, h: 0 }
      : { x: 0, y: 0, w: 0, h: 0 };
    for (let pass = 0; pass < 4; pass++) {
      for (const child of children) {
        if (out[child.id]) continue;
        const anchor = anchors[child.id];
        let base: Rect | undefined;
        if (!anchor) base = orphanBase;
        else if (anchor.startsWith("child:")) base = out[anchor.slice(6)];
        else {
          const item = terminals.find((i) => i.key === anchor);
          base = item ? rectOf(item) : orphanBase;
        }
        if (!base) continue;
        const siblings = children.filter((c) => anchors[c.id] === anchor);
        out[child.id] =
          childDrafts[child.id] ??
          childRect(base, siblings.indexOf(child), childOffsets[child.id]);
      }
    }
    return out;
  });

  const threads = $derived(
    children.flatMap((child): Thread[] => {
      const anchor = anchorOf(child);
      const to = childRects[child.id];
      if (!anchor || !to) return [];
      const item = terminals.find((i) => i.key === anchor);
      const from = anchor.startsWith("child:")
        ? childRects[anchor.slice(6)]
        : item
          ? rectOf(item)
          : undefined;
      if (!from) return [];
      const busy = child.status === "working" || child.status === "waiting";
      return [
        {
          id: child.id,
          from,
          to,
          flow: busy ? "out" : child.unread > 0 ? "back" : "idle",
          tone: child.backendId,
          label: child.label?.trim() || child.backendName,
        },
      ];
    }),
  );

  // Las `external:` se traducen a consola una vez; si todavía no hay, se
  // vuelve a preguntar en un rato (la consola puede estar abriéndose).
  $effect(() => {
    for (const child of children) {
      const parent = child.parent;
      if (parentKind(parent) !== "external" || !parent || parent in externalConsole)
        continue;
      externalConsole = { ...externalConsole, [parent]: null };
      void consoleForParent(parent)
        .catch(() => null)
        .then((pty) => {
          externalConsole = { ...externalConsole, [parent]: pty ?? null };
          if (!pty)
            window.setTimeout(() => {
              externalConsole = Object.fromEntries(
                Object.entries(externalConsole).filter(([k]) => k !== parent),
              );
            }, 5000);
        });
    }
  });

  // Cada sub-agente nuevo nace arriba de la pila.
  $effect(() => {
    for (const child of children) {
      const key = `child:${child.id}`;
      if (!untrack(() => stack).includes(key)) raise(key);
    }
  });

  function childStatus(child: AgentSessionView): ConsoleState {
    if (child.status === "working") return "working";
    if (child.status === "waiting") return "waiting";
    return "ready";
  }

  function cliOfBackend(backend: string): string | null {
    return AGENTS.find((a) => a.backend === backend)?.cli ?? null;
  }

  /** Dónde estaba cada sub-agente al empezar a arrastrarlo. */
  const childDragStart: Record<string, Rect> = {};

  /**
   * Arrastrar o agrandar un sub-agente guarda cuánto se corrió respecto de
   * donde iría solo: así sigue moviéndose con su consola.
   */
  function onChildRect(child: AgentSessionView, rect: Rect, commit: boolean) {
    const id = child.id;
    childDragStart[id] ??= childRects[id] ?? rect;
    if (!commit) {
      childDrafts = { ...childDrafts, [id]: rect };
      return;
    }
    const start = childDragStart[id];
    delete childDragStart[id];
    const before = childOffsets[id] ?? {
      dx: 0,
      dy: 0,
      w: CHILD_SIZE.w,
      h: CHILD_SIZE.h,
    };
    childOffsets = {
      ...childOffsets,
      [id]: {
        dx: before.dx + rect.x - start.x,
        dy: before.dy + rect.y - start.y,
        w: rect.w,
        h: rect.h,
      },
    };
    childDrafts = Object.fromEntries(
      Object.entries(childDrafts).filter(([k]) => k !== id),
    );
  }

  /** Abrir el menú de «Encargar a…» con lo seleccionado en esa consola. */
  function openDelegate(key: string, event: MouseEvent) {
    const text = terms[key]?.selection().trim() ?? "";
    if (!text) {
      toasts.push(t("page.agents.board.delegateNoSelection"), 2500);
      return;
    }
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    delegateMenu = { key, x: box.right, y: box.bottom + 6, text };
  }

  /**
   * Encargar lo seleccionado a otro agente: una sesión nueva en la misma
   * carpeta, que aparece colgando de la consola con su hilo.
   */
  async function delegate(agent: AgentDef) {
    const menu = delegateMenu;
    delegateMenu = null;
    const item = terminals.find((i) => i.key === menu?.key);
    if (!menu || !item) return;
    try {
      const id = await agents.start(agent.backend, {
        cwd: item.cwd ?? (cwd || undefined),
      });
      handed = { ...handed, [id]: item.key };
      await agents.send(id, menu.text);
    } catch (err) {
      toasts.push(String(err), 4000);
    }
  }

  /**
   * Salida por sesión, fuera de la reactividad: llega en ráfagas de decenas
   * de trozos por segundo. El estado se recalcula con un reloj.
   */
  const activity: Record<string, Activity> = {};
  /** Presencia → consola donde corre (`null` = en ninguna de Atic). */
  const presenceConsole: Record<string, string | null> = {};
  let presenceTick = $state(0);
  let states = $state<Record<string, ConsoleState>>({});
  /** Consolas que terminaron algo sin que se las mirara. */
  let attention = $state<Record<string, boolean>>({});

  /** El estado de cada sesión según la presencia, si alguna corre en ella. */
  const presenceBySession = $derived.by(() => {
    void presenceTick;
    const out: Record<string, PresenceStatus> = {};
    for (const p of presence.list) {
      const session = presenceConsole[p.id];
      if (session) out[session] = p.status;
    }
    return out;
  });

  /**
   * De cada presencia nueva, en qué consola corre. Se pregunta una vez; si
   * no está en ninguna (todavía), se vuelve a preguntar un rato después.
   */
  $effect(() => {
    for (const p of presence.list) {
      if (p.id in presenceConsole) continue;
      presenceConsole[p.id] = null;
      void consoleForPresence(p.id)
        .catch(() => null)
        .then((session) => {
          presenceConsole[p.id] = session ?? null;
          if (session) presenceTick += 1;
          else window.setTimeout(() => delete presenceConsole[p.id], 10_000);
        });
    }
  });

  function watched(key: string): boolean {
    return key === workspace.active && document.hasFocus();
  }

  function clearAttention(key: string) {
    if (!attention[key]) return;
    attention = Object.fromEntries(
      Object.entries(attention).filter(([k]) => k !== key),
    );
  }

  /** Recalcula el estado de cada consola y marca las que piden atención. */
  function refreshStates() {
    const now = Date.now();
    const next: Record<string, ConsoleState> = {};
    let raised: Record<string, boolean> | null = null;
    for (const item of terminals) {
      const run = item.session ? activity[item.session] : undefined;
      const state = consoleState({
        ended: !!ended[item.key],
        presence: item.session ? (presenceBySession[item.session] ?? null) : null,
        activity: run,
        now,
      });
      const prev = states[item.key] ?? state;
      if (
        needsAttention({
          prev,
          next: state,
          turnMs: run ? run.last - run.since : 0,
          watched: watched(item.key),
        })
      ) {
        raised = { ...(raised ?? attention), [item.key]: true };
      }
      next[item.key] = state;
    }
    if (raised) attention = raised;
    if (JSON.stringify(next) !== JSON.stringify(states)) states = next;
  }

  const choices = $derived(AGENTS.filter((a) => installed[a.cli] !== false));
  const terminals = $derived(
    workspace.items.filter((i): i is TerminalItem => i.kind === "terminal"),
  );
  const activeTerm = $derived(
    terminals.find((i) => i.key === workspace.active) ?? null,
  );
  /** Lo que tapan la lista y la entrada: encuadrar cuenta solo lo libre. */
  const insets = $derived<Insets>({
    top: 56,
    right: 12,
    bottom: 96,
    left: (listCollapsed ? 52 : 232) + 24,
  });

  function rectOf(item: TerminalItem): Rect {
    return (
      drafts[item.key] ?? item.rect ?? placeNew([], visibleArea(cam, size, insets))
    );
  }

  function zOf(key: string): number {
    return stack.indexOf(key) + 1;
  }

  // ── Cámara ──────────────────────────────────────────────────────────────

  let saveTimer = 0;
  let flightTimer = 0;
  let viewIdle = 0;

  function saveCamera() {
    window.clearTimeout(saveTimer);
    saveTimer = window.setTimeout(() => {
      try {
        localStorage.setItem(CAMERA_KEY, JSON.stringify(cam));
      } catch {
        /* se vuelve al origen al recargar */
      }
    }, 250);
  }

  /** Mover la cámara: a mano, directo; cuando se mueve sola, volando. */
  function setCamera(next: Camera, fly = false) {
    window.clearTimeout(flightTimer);
    flying = fly;
    cam = next;
    saveCamera();
    viewBusy = true;
    window.clearTimeout(viewIdle);
    viewIdle = window.setTimeout(() => (viewBusy = false), VIEW_IDLE_MS);
    if (fly) flightTimer = window.setTimeout(() => (flying = false), FLIGHT_MS + 40);
  }

  function center(): { x: number; y: number } {
    return {
      x: insets.left + (size.w - insets.left - insets.right) / 2,
      y: insets.top + (size.h - insets.top - insets.bottom) / 2,
    };
  }

  function zoomBy(factor: number, at = center()) {
    setCamera(zoomAt(cam, cam.zoom * factor, at), true);
  }

  function fitAll() {
    unmaximize(false);
    const box = bounds([...terminals.map(rectOf), ...Object.values(childRects)]);
    if (box) setCamera(fitRect(box, size, insets), true);
  }

  function fitCard(key: string) {
    const item = terminals.find((i) => i.key === key);
    if (item) setCamera(fitRect(rectOf(item), size, insets), true);
  }

  /**
   * Tocar una consola la trae a su tamaño de trabajo: encuadrada y tan
   * cerca como quepa (hasta el 100%). Si ya está así, no se mueve nada:
   * seleccionar texto no debería sacudir la vista.
   */
  function ensureVisible(key: string) {
    const item = terminals.find((i) => i.key === key);
    if (!item) return;
    const rect = rectOf(item);
    const target = fitRect(rect, size, insets);
    const settled =
      cam.zoom >= target.zoom * 0.98 && fullyVisible(rect, cam, size, insets);
    if (!settled) setCamera(target, true);
  }

  /**
   * Maximizar: la consola crece a todo el espacio libre al 100% y la cámara
   * se para sobre ella. Volver le devuelve su tamaño y la cámara de antes.
   */
  function toggleMaximize(key: string) {
    if (maximized?.key === key) {
      unmaximize(true);
      return;
    }
    unmaximize(false);
    const item = terminals.find((i) => i.key === key);
    if (!item) return;
    const rect = rectOf(item);
    maximized = { key, cam };
    drafts = {
      ...drafts,
      [key]: {
        x: rect.x,
        y: rect.y,
        w: Math.max(1, size.w - insets.left - insets.right),
        h: Math.max(1, size.h - insets.top - insets.bottom),
      },
    };
    focusCard(key);
    setCamera({ x: insets.left - rect.x, y: insets.top - rect.y, zoom: 1 }, true);
  }

  function unmaximize(restoreCamera: boolean) {
    if (!maximized) return;
    const { key, cam: before } = maximized;
    maximized = null;
    drafts = Object.fromEntries(Object.entries(drafts).filter(([k]) => k !== key));
    if (restoreCamera) setCamera(before, true);
  }

  /**
   * Al terminar de moverse, la consola que quedó al centro pasa a ser la
   * enfocada: mirar a OpenCode es elegir OpenCode. Si se está escribiendo en
   * la entrada, cambia su destino pero el cursor se queda ahí.
   */
  function focusByView() {
    const index = focusedByView(terminals.map(rectOf), cam, size, insets);
    const item = terminals[index];
    if (!item || item.key === workspace.active) return;
    workspace.select(item.key);
    raise(item.key);
    clearAttention(item.key);
  }

  // ── Fondo: arrastrar, rueda, doble clic ─────────────────────────────────

  let panDrag: { x: number; y: number; start: Camera } | null = null;
  let panning = $state(false);
  let wheelIdle = 0;

  /** Solo el fondo desplaza: sobre una consola, el puntero es de ella. */
  function onBackground(target: EventTarget | null): boolean {
    return target === boardEl;
  }

  function closeDelegateMenu(event: PointerEvent) {
    if (!delegateMenu) return;
    if ((event.target as HTMLElement).closest(".delegate-menu, .card-action")) return;
    delegateMenu = null;
  }

  function onPanDown(event: PointerEvent) {
    if (event.button !== 0 || !onBackground(event.target)) return;
    // Sin esto, arrastrar el fondo empieza una selección de texto que se
    // lleva lo que haya en las consolas por donde pasa el puntero.
    event.preventDefault();
    boardEl?.setPointerCapture(event.pointerId);
    panDrag = { x: event.clientX, y: event.clientY, start: cam };
    panning = true;
    newOpen = false;
  }

  function onPanMove(event: PointerEvent) {
    if (!panDrag) return;
    setCamera({
      ...panDrag.start,
      x: panDrag.start.x + event.clientX - panDrag.x,
      y: panDrag.start.y + event.clientY - panDrag.y,
    });
  }

  function onPanUp() {
    if (!panDrag) return;
    panDrag = null;
    panning = false;
    focusByView();
  }

  /**
   * Ctrl+rueda (y el pellizco del trackpad, que llega igual) acerca y aleja
   * en cualquier parte, también sobre una consola: sin frenarlo, el webview
   * agrandaría la página entera. La rueda sola desplaza solo desde el fondo;
   * sobre una consola es su scroll.
   */
  function onWheel(event: WheelEvent) {
    if (!boardEl) return;
    if (event.ctrlKey || event.metaKey) {
      event.preventDefault();
      event.stopPropagation();
      const box = boardEl.getBoundingClientRect();
      const at = { x: event.clientX - box.left, y: event.clientY - box.top };
      setCamera(zoomAt(cam, cam.zoom * Math.exp(-event.deltaY * 0.0022), at));
    } else if (onBackground(event.target)) {
      event.preventDefault();
      const dx = event.shiftKey ? event.deltaY : event.deltaX;
      const dy = event.shiftKey ? 0 : event.deltaY;
      setCamera({ ...cam, x: cam.x - dx, y: cam.y - dy });
    } else {
      return;
    }
    window.clearTimeout(wheelIdle);
    wheelIdle = window.setTimeout(focusByView, 180);
  }

  // ── Consolas ────────────────────────────────────────────────────────────

  function raise(key: string) {
    if (stack[stack.length - 1] === key) return;
    stack = [...stack.filter((k) => k !== key), key];
  }

  function focusCard(key: string) {
    workspace.select(key);
    raise(key);
    termFocus = true;
    clearAttention(key);
  }

  /** Desde la lista, la pill o el teclado: enfocarla y encuadrarla. */
  function reveal(key: string) {
    focusCard(key);
    fitCard(key);
  }

  /**
   * Abre `count` consolas iguales. Sin acomodo puesto, varias nacen en
   * grilla donde se está mirando; la vista las muestra a todas.
   */
  async function openConsole(agent: AgentDef | null, count = 1) {
    newOpen = false;
    const rects = terminals.map(rectOf);
    // Con un acomodo puesto nace con el tamaño de la última y en la esquina
    // de todas: así no corre el origen y el acomodo la ubica al final.
    const last = rects[rects.length - 1];
    const box = bounds(rects);
    const first =
      layout !== "free" && last && box
        ? { x: box.x, y: box.y, w: last.w, h: last.h }
        : placeNew(rects, visibleArea(cam, size, insets));
    const placed =
      layout === "free" && count > 1
        ? arrange(
            Array.from({ length: count }, () => first),
            "grid",
          )
        : Array.from({ length: count }, () => first);
    let key = "";
    for (const rect of placed) {
      key = workspace.add({
        kind: "terminal",
        session: null,
        label: agent?.name ?? t("page.agents.window.shellLabel"),
        cli: agent?.cli ?? null,
        command: agent?.cli ?? null,
        cwd: cwd || null,
        rect,
      });
      raise(key);
    }
    termFocus = true;
    await tick();
    if (layout !== "free") relayout(count > 1 ? "all" : key);
    else if (count > 1) fitAll();
    else ensureVisible(key);
  }

  function folderName(path: string): string {
    if (!path) return t("page.agents.window.home");
    return (
      path
        .replace(/[/\\]+$/, "")
        .split(/[/\\]/)
        .pop() || path
    );
  }

  function setBackdrop(next: Backdrop) {
    backdrop = next;
    try {
      localStorage.setItem(BACKDROP_KEY, next);
    } catch {
      /* queda en memoria */
    }
  }

  function setStartCount(next: number) {
    startCount = Math.max(1, Math.min(MAX_START, next));
    try {
      localStorage.setItem(START_COUNT_KEY, String(startCount));
    } catch {
      /* queda en memoria */
    }
  }

  function onRect(key: string, rect: Rect, commit: boolean, moved: boolean) {
    if (!commit) {
      drafts = { ...drafts, [key]: rect };
      return;
    }
    workspace.update(key, { rect });
    drafts = Object.fromEntries(Object.entries(drafts).filter(([k]) => k !== key));
    // Moverla a mano es salirse del acomodo: si no, la próxima que se abra
    // la devolvería a su lugar. Cambiarle el tamaño sí reacomoda al resto.
    if (layout === "free") return;
    if (moved) setLayout("free");
    else relayout();
  }

  function setLayout(next: Arrangement | "free") {
    layout = next;
    try {
      localStorage.setItem(LAYOUT_KEY, next);
    } catch {
      /* queda en memoria */
    }
  }

  /**
   * Aplica el acomodo puesto. Con `show`, la cámara va a mostrar el
   * resultado: todas si se leen, o solo esa si juntas quedarían muy chicas.
   */
  function relayout(show?: string | "all") {
    if (layout === "free") return;
    unmaximize(false);
    const placed = arrange(terminals.map(rectOf), layout);
    terminals.forEach((item, i) => workspace.update(item.key, { rect: placed[i] }));
    const box = bounds(placed);
    if (!show || !box) return;
    const all = fitRect(box, size, insets);
    const target = placed[terminals.findIndex((i) => i.key === show)];
    if (show === "all" || all.zoom >= READABLE_ZOOM || !target) setCamera(all, true);
    else setCamera(fitRect(target, size, insets), true);
  }

  /** Elegir el acomodo que ya está puesto lo saca: vuelve a libre. */
  function arrangeAll(mode: Arrangement) {
    if (layout === mode) {
      setLayout("free");
      return;
    }
    setLayout(mode);
    relayout("all");
  }

  /** Cerrar termina el proceso: sin deshacer, así que lo vivo pregunta. */
  function requestClose(key: string) {
    const item = terminals.find((i) => i.key === key);
    if (!item) return;
    if (item.session && !ended[key]) pendingClose = item;
    else closeNow(item);
  }

  function closeNow(item: TerminalItem) {
    pendingClose = null;
    if (maximized?.key === item.key) unmaximize(true);
    workspace.remove(item.key);
    stack = stack.filter((k) => k !== item.key);
    relayout();
    if (item.session) void consoleClose(item.session).catch(() => {});
  }

  /** La entrada única: a la consola enfocada, como un solo mensaje. */
  function send(text: string) {
    const session = activeTerm?.session;
    if (!session) return;
    const [body, enter] = composerWrites(text, !!activeTerm.command);
    void consoleWrite(session, body)
      // El Enter aparte y un instante después: pegado junto, el TUI lo
      // tomaría como parte del texto.
      .then(() => new Promise((r) => window.setTimeout(r, 40)))
      .then(() => consoleWrite(session, enter))
      .catch(() => {});
  }

  /**
   * Un ítem del historial del portapapeles, pegado con esta ventana al
   * frente: entra en la consola enfocada como un pegado, sin Enter, para
   * poder seguir escribiendo. De una imagen va su ruta, que es lo que los
   * CLI de agentes saben adjuntar.
   */
  function pasteFromHistory(payload: AgentsComposerInsert) {
    const text = payload.text || payload.imagePath;
    if (!text) return;
    // Escribiendo en la entrada, lo pegado va ahí; si no, a la consola.
    if (!termFocus && composer) composer.insertText(text);
    else if (activeTerm) pasteInto(activeTerm.key, text);
  }

  /** Soltado en un punto de la ventana (px CSS): la entrada, una consola o la enfocada. */
  function dropAt(point: { x: number; y: number }, text: string) {
    if (!text) return;
    if (document.elementFromPoint(point.x, point.y)?.closest(".composer")) {
      composer?.insertText(text);
      return;
    }
    const key = consoleUnder(point) ?? workspace.active;
    if (key) {
      focusCard(key);
      pasteInto(key, text);
    }
  }

  function pasteInto(key: string, text: string) {
    const item = terminals.find((i) => i.key === key);
    if (!item?.session) return;
    const [body] = composerWrites(text, !!item.command);
    void consoleWrite(item.session, body).catch(() => {});
  }

  /**
   * Archivos soltados desde el sistema (o una captura arrastrada desde Atic)
   * sobre una consola: sus rutas se pegan en ella, sin Enter. Los CLI de
   * agentes adjuntan lo que reciben como ruta. Tauri da la posición en
   * píxeles físicos; se pasa a coordenadas de pizarra para saber cuál está
   * debajo.
   */
  function toCss(physical: { x: number; y: number }) {
    const scale = window.devicePixelRatio || 1;
    return { x: physical.x / scale, y: physical.y / scale };
  }

  /** La consola bajo un punto de la ventana (px CSS), si hay alguna. */
  function consoleUnder(point: { x: number; y: number }): string | null {
    if (!boardEl) return null;
    const box = boardEl.getBoundingClientRect();
    const sx = point.x - box.left;
    const sy = point.y - box.top;
    return cardAt(
      terminals.map((i) => ({ key: i.key, rect: rectOf(i), z: zOf(i.key) })),
      { x: (sx - cam.x) / cam.zoom, y: (sy - cam.y) / cam.zoom },
    );
  }

  // ── Sesiones guardadas ──────────────────────────────────────────────────

  function writeSpaces(next: SavedSpace[]) {
    spaces = next;
    try {
      localStorage.setItem(SPACES_KEY, JSON.stringify(next));
    } catch {
      /* queda en memoria */
    }
  }

  /** La conversación del agente que corre en una consola, si Rust la ubica. */
  async function agentSessionIn(item: TerminalItem): Promise<string | undefined> {
    if (!canResume(item.cli) || !item.session) return undefined;
    return (await consoleAgentSession(item.session).catch(() => null)) ?? undefined;
  }

  async function saveSpace(name: string) {
    const resumes = await Promise.all(terminals.map(agentSessionIn));
    const consoles = terminals.map((i, n) => {
      const resume = resumes[n];
      return {
        label: i.label,
        cli: i.cli,
        // Una consola retomada se abrió con su conversación en el comando: se
        // guarda el comando limpio y la conversación aparte.
        command: i.cli ?? i.command,
        cwd: i.cwd ?? (cwd || null),
        rect: i.rect ?? rectOf(i),
        ...(resume ? { resume } : {}),
      };
    });
    writeSpaces(upsertSpace(spaces, { name, savedAt: Date.now(), layout, consoles }));
    toasts.push(t("page.agents.board.spaceSaved", { name }), 2000);
  }

  /**
   * Abre las consolas de una sesión guardada. Los procesos no se guardan: cada CLI
   * arranca de nuevo en su carpeta, y el agente retoma su conversación. Con la pizarra vacía vuelve también su
   * acomodo; si ya hay consolas, se abren al lado y el acomodo no se toca.
   */
  async function openSpace(space: SavedSpace) {
    unmaximize(false);
    const empty = terminals.length === 0;
    const rects = placeSpace(
      space.consoles.map((c) => c.rect),
      terminals.map(rectOf),
    );
    space.consoles.forEach((c, i) => {
      const key = workspace.add({
        kind: "terminal",
        session: null,
        label: c.label,
        cli: c.cli,
        command: launchCommand(c),
        cwd: c.cwd,
        rect: rects[i],
      });
      raise(key);
    });
    if (empty) setLayout(space.layout);
    await tick();
    const box = bounds(rects);
    if (box) setCamera(fitRect(box, size, insets), true);
  }

  function deleteSpace(space: SavedSpace) {
    writeSpaces(spaces.filter((s) => s !== space));
  }

  function pickFolder(path: string) {
    browsingFolder = false;
    cwd = path;
    try {
      localStorage.setItem(CWD_KEY, path);
    } catch {
      /* queda en memoria */
    }
  }

  function setListCollapsed(collapsed: boolean) {
    listCollapsed = collapsed;
    try {
      localStorage.setItem(LIST_KEY, collapsed ? "1" : "0");
    } catch {
      /* queda en memoria */
    }
  }

  // ── Teclado ─────────────────────────────────────────────────────────────

  /**
   * En captura: los atajos de la pizarra llegan antes que la terminal. Sin
   * eso, Ctrl+W además le borraba una palabra al CLI enfocado.
   */
  function onKey(event: KeyboardEvent) {
    if (!(event.ctrlKey || event.metaKey) || event.altKey) return;
    const index = terminals.findIndex((i) => i.key === workspace.active);
    const key = event.key;
    if (key === "n" || key === "N") {
      newOpen = true;
    } else if ((key === "w" || key === "W") && activeTerm) {
      requestClose(activeTerm.key);
    } else if (key === "Tab" && terminals.length > 1) {
      const step = event.shiftKey ? -1 : 1;
      reveal(terminals[(index + step + terminals.length) % terminals.length].key);
    } else if (/^[1-9]$/.test(key) && terminals[Number(key) - 1]) {
      reveal(terminals[Number(key) - 1].key);
    } else if (key === "=" || key === "+") {
      zoomBy(1.2);
    } else if (key === "-") {
      zoomBy(1 / 1.2);
    } else if (key === "0") {
      fitAll();
    } else {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
  }

  // ── Arranque ────────────────────────────────────────────────────────────

  /** Lo guardado que sigue vivo; a lo que no tenía lugar se le busca uno. */
  async function restore() {
    const saved = workspace.load();
    const alive = new Map<string, boolean>(
      await Promise.all(
        saved.map(async (item): Promise<[string, boolean]> => {
          if (item.kind !== "terminal" || !item.session) return [item.key, true];
          const ok = await consoleTail(item.session, 1).then(
            () => true,
            () => false,
          );
          return [item.key, ok];
        }),
      ),
    );
    workspace.keep((item) => alive.get(item.key) !== false);
    await tick();
    const placed: Rect[] = [];
    for (const item of terminals) {
      const rect = item.rect ?? placeNew(placed, visibleArea(cam, size, insets));
      placed.push(rect);
      if (!item.rect) workspace.update(item.key, { rect });
    }
    stack = terminals.map((i) => i.key);
    if (activeTerm) raise(activeTerm.key);
  }

  onMount(() => {
    try {
      cwd = localStorage.getItem(CWD_KEY) ?? "";
      listCollapsed = localStorage.getItem(LIST_KEY) === "1";
      spaces = parseSpaces(localStorage.getItem(SPACES_KEY));
      const savedLayout = localStorage.getItem(LAYOUT_KEY);
      if (savedLayout === "row" || savedLayout === "column" || savedLayout === "grid")
        layout = savedLayout;
      const savedBackdrop = localStorage.getItem(BACKDROP_KEY);
      if (BACKDROPS.includes(savedBackdrop as Backdrop))
        backdrop = savedBackdrop as Backdrop;
      const savedCount = Number(localStorage.getItem(START_COUNT_KEY));
      if (Number.isInteger(savedCount) && savedCount >= 1)
        startCount = Math.min(MAX_START, savedCount);
      const saved = parseCamera(JSON.parse(localStorage.getItem(CAMERA_KEY) ?? "null"));
      if (saved) cam = saved;
    } catch {
      /* se arranca en el origen */
    }
    void restore();
    void Promise.all(
      AGENTS.map(
        async (a) => [a.cli, await cliOnPath(a.cli).catch(() => true)] as const,
      ),
    ).then((entries) => (installed = Object.fromEntries(entries)));

    // La pill pide mostrar una consola: acá solo hay terminales.
    const seen: number[] = [];
    const unlisten = onAgentsFocus((request) => {
      if (seen.includes(request.nonce) || request.kind !== "terminal") return;
      seen.push(request.nonce);
      const item = workspace.findSession("terminal", request.session);
      if (item) reveal(item.key);
    });

    // Con esta ventana al frente, el historial del portapapeles pega acá y
    // no en la app de atrás. El overlay no toma el foco al hacerle clic, así
    // que elegir un ítem no apaga la bandera.
    const unlistenPaste = onAgentsComposerInsert(pasteFromHistory);
    // Texto del historial arrastrado y soltado sobre esta ventana.
    const unlistenWindowDrop = onAgentsWindowInsert((payload) => {
      const text = payload.text || payload.imagePath;
      if (text && payload.x != null && payload.y != null)
        dropAt({ x: payload.x, y: payload.y }, text);
    });
    const flagFocus = () =>
      void setAgentsWindowOpen(document.hasFocus()).catch(() => {});
    flagFocus();
    window.addEventListener("focus", flagFocus);
    window.addEventListener("blur", flagFocus);

    const unlistenDrop = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === "leave") {
        dropTarget = null;
      } else if (payload.type === "enter" || payload.type === "over") {
        dropTarget = consoleUnder(toCss(payload.position));
      } else if (payload.type === "drop") {
        dropTarget = null;
        const text = quotePaths(payload.paths);
        if (text) dropAt(toCss(payload.position), `${text} `);
      }
    });

    // Estado de las consolas: la salida marca actividad, un reloj decide.
    const unwatchOutput = watchOutput((session) => {
      activity[session] = noteOutput(activity[session], Date.now());
    });
    const statusClock = window.setInterval(refreshStates, 500);
    void presence.init();
    void agents.init();

    const board = boardEl;
    board?.addEventListener("wheel", onWheel, { passive: false, capture: true });
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("pointerdown", closeDelegateMenu, true);
    return () => {
      window.removeEventListener("pointerdown", closeDelegateMenu, true);
      void unlisten.then((un) => un());
      void unlistenPaste.then((un) => un());
      void unlistenWindowDrop.then((un) => un());
      void unlistenDrop.then((un) => un());
      unwatchOutput();
      window.clearInterval(statusClock);
      window.removeEventListener("focus", flagFocus);
      window.removeEventListener("blur", flagFocus);
      void setAgentsWindowOpen(false).catch(() => {});
      board?.removeEventListener("wheel", onWheel, true);
      window.removeEventListener("keydown", onKey, true);
      window.clearTimeout(saveTimer);
      window.clearTimeout(flightTimer);
      window.clearTimeout(viewIdle);
      window.clearTimeout(wheelIdle);
    };
  });
</script>

<div
  class="board"
  class:is-panning={panning}
  bind:this={boardEl}
  bind:clientWidth={size.w}
  bind:clientHeight={size.h}
  class:is-grid={backdrop === "grid"}
  class:is-plain={backdrop === "plain"}
  style:background-position={`${cam.x}px ${cam.y}px`}
  style:background-size={`${BACKDROP_TILE[backdrop] * cam.zoom}px ${BACKDROP_TILE[backdrop] * cam.zoom}px`}
  role="presentation"
  onpointerdown={onPanDown}
  onpointermove={onPanMove}
  onpointerup={onPanUp}
  onpointercancel={onPanUp}
  ondblclick={(e) => onBackground(e.target) && fitAll()}
>
  <div
    class="plane"
    class:is-flying={flying}
    style:transform={`translate(${cam.x}px, ${cam.y}px) scale(${cam.zoom})`}
  >
    <BoardThreads {threads} />
    {#each terminals as item (item.key)}
      <BoardCard
        label={item.label}
        cli={item.cli}
        rect={rectOf(item)}
        zoom={cam.zoom}
        z={zOf(item.key)}
        active={item.key === workspace.active}
        status={states[item.key] ?? "ready"}
        attention={!!attention[item.key]}
        onFocus={() => focusCard(item.key)}
        maximized={maximized?.key === item.key}
        dropping={dropTarget === item.key}
        onActivate={() => !maximized && ensureVisible(item.key)}
        onMaximize={() => toggleMaximize(item.key)}
        onRect={(rect, commit, moved) => onRect(item.key, rect, commit, moved)}
        onClose={() => requestClose(item.key)}
      >
        {#snippet actions()}
          <button
            type="button"
            class="card-action"
            aria-label={t("page.agents.board.delegate")}
            title={t("page.agents.board.delegate")}
            onclick={(e) => openDelegate(item.key, e)}
          >
            <Icon icon={Forward} size={12} />
          </button>
        {/snippet}
        <TerminalView
          bind:this={terms[item.key]}
          sessionId={item.session}
          command={item.command}
          cwd={item.cwd}
          focused={item.key === workspace.active && termFocus}
          onSession={(id) => workspace.update(item.key, { session: id })}
          onExit={() => (ended = { ...ended, [item.key]: true })}
        />
      </BoardCard>
    {/each}
    {#each children as child (child.id)}
      {@const rect = childRects[child.id]}
      {#if rect}
        <BoardCard
          label={child.label?.trim() || child.backendName}
          cli={cliOfBackend(child.backendId)}
          {rect}
          zoom={cam.zoom}
          z={zOf(`child:${child.id}`)}
          active={false}
          status={childStatus(child)}
          attention={child.unread > 0}
          maximized={false}
          dropping={false}
          onFocus={() => {
            raise(`child:${child.id}`);
            agents.markRead(child.id);
          }}
          onActivate={() => {
            const target = fitRect(rect, size, insets);
            if (!fullyVisible(rect, cam, size, insets) || cam.zoom < target.zoom * 0.98)
              setCamera(target, true);
          }}
          onMaximize={() => setCamera(fitRect(rect, size, insets), true)}
          onRect={(next, commit) => onChildRect(child, next, commit)}
          onClose={() => void agents.stop(child.id).catch(() => {})}
        >
          <AgentChatPanel sessionId={child.id} readOnly decides {choices} />
        </BoardCard>
      {/if}
    {/each}
  </div>

  {#if delegateMenu}
    <div
      class="delegate-menu"
      role="menu"
      style:left={`${delegateMenu.x}px`}
      style:top={`${delegateMenu.y}px`}
    >
      <p class="delegate-hint">{t("page.agents.board.delegateHint")}</p>
      {#each choices as agent (agent.backend)}
        <button
          type="button"
          class="delegate-pick"
          onclick={() => void delegate(agent)}
        >
          <AgentLogo agent={agent.cli} size={15} />
          {agent.name}
        </button>
      {/each}
    </div>
  {/if}

  {#if terminals.length === 0}
    <div class="empty">
      <p class="empty-title">{t("page.agents.board.emptyTitle")}</p>
      <p class="empty-hint">{t("page.agents.board.emptyHint")}</p>
      <div class="empty-setup">
        <button
          type="button"
          class="empty-folder"
          title={`${t("page.agents.window.pickFolder")}: ${cwd || t("page.agents.window.home")}`}
          onclick={() => (browsingFolder = true)}
        >
          <Icon icon={Folder} size={14} />
          <span class="empty-name">{folderName(cwd)}</span>
          {#if cwd}
            <span class="empty-path">{cwd}</span>
          {/if}
          <Icon icon={ChevronRight} size={13} />
        </button>
        <div
          class="empty-stepper"
          role="group"
          aria-label={t("page.agents.launcher.consolesCount")}
        >
          <button
            type="button"
            aria-label={t("page.agents.launcher.fewerConsoles")}
            disabled={startCount <= 1}
            onclick={() => setStartCount(startCount - 1)}
          >
            <Icon icon={Minus} size={13} />
          </button>
          <span class="empty-count">
            {startCount}
            {startCount === 1
              ? t("page.agents.consoleSingular")
              : t("page.agents.consolePlural")}
          </span>
          <button
            type="button"
            aria-label={t("page.agents.launcher.moreConsoles")}
            disabled={startCount >= MAX_START}
            onclick={() => setStartCount(startCount + 1)}
          >
            <Icon icon={Plus} size={13} />
          </button>
        </div>
      </div>
      <div class="empty-agents">
        {#each choices as agent (agent.cli)}
          <button
            type="button"
            class="empty-agent"
            onclick={() => void openConsole(agent, startCount)}
          >
            <AgentLogo agent={agent.cli} size={22} />
            <span>{agent.name}</span>
          </button>
        {/each}
        <button
          type="button"
          class="empty-agent is-shell"
          onclick={() => void openConsole(null, startCount)}
        >
          <Icon icon={SquareTerminal} size={22} />
          <span>{t("page.agents.window.shellLabel")}</span>
        </button>
      </div>
    </div>
  {/if}

  <div class="float is-list">
    <BoardList
      entries={terminals.map((i) => ({
        key: i.key,
        label: i.label,
        cli: i.cli,
        status: states[i.key] ?? "ready",
        attention: !!attention[i.key],
      }))}
      {spaces}
      onSaveSpace={(name) => void saveSpace(name)}
      onOpenSpace={(space) => void openSpace(space)}
      onDeleteSpace={deleteSpace}
      active={workspace.active}
      {choices}
      {cwd}
      {newOpen}
      collapsed={listCollapsed}
      onCollapse={setListCollapsed}
      onNewToggle={(open) => (newOpen = open)}
      onNew={(agent) => void openConsole(agent)}
      onSelect={reveal}
      onClose={requestClose}
      onPickFolder={() => (browsingFolder = true)}
      onSettings={() => (settingsOpen = true)}
    />
  </div>

  <div class="float is-composer">
    <BoardComposer
      bind:this={composer}
      target={activeTerm?.session
        ? { label: activeTerm.label, cli: activeTerm.cli }
        : null}
      onSend={send}
      onFocusChange={(focused) => (termFocus = !focused)}
    />
  </div>

  {#if terminals.length > 0}
    <div class="float is-minimap" class:is-quiet={!viewBusy}>
      <BoardMinimap
        cards={[
          ...terminals.map((i) => ({
            key: i.key,
            rect: rectOf(i),
            active: i.key === workspace.active,
            tone: AGENTS.find((a) => a.cli === i.cli)?.backend ?? null,
          })),
          ...children.flatMap((c) =>
            childRects[c.id]
              ? [
                  {
                    key: c.id,
                    rect: childRects[c.id],
                    active: false,
                    tone: c.backendId,
                  },
                ]
              : [],
          ),
        ]}
        view={visibleArea(cam, size, insets)}
        onGo={(at) => {
          setCamera(centerOn(at, cam, size, insets));
          focusByView();
        }}
      />
    </div>
  {/if}

  <div class="float is-zoom" class:is-quiet={!viewBusy}>
    <BoardZoom
      {backdrop}
      onBackdrop={setBackdrop}
      zoom={cam.zoom}
      canArrange={terminals.length > 0}
      {layout}
      onZoom={(factor) => zoomBy(factor)}
      onReset={() => zoomBy(1 / cam.zoom)}
      onFitAll={fitAll}
      onArrange={arrangeAll}
    />
  </div>
</div>

{#if browsingFolder}
  <FolderBrowser
    contained={false}
    initialPath={cwd}
    onPick={pickFolder}
    onClose={() => (browsingFolder = false)}
  />
{/if}

{#if settingsOpen}
  <AgentSettingsModal onClose={() => (settingsOpen = false)} />
{/if}

{#if pendingClose}
  <ConfirmDialog
    title={t("page.agents.window.closeTitle", { name: pendingClose.label })}
    body={t("page.agents.board.closeBody")}
    confirmLabel={t("page.agents.window.closeAction")}
    tone="danger"
    onConfirm={() => pendingClose && closeNow(pendingClose)}
    onCancel={() => (pendingClose = null)}
  />
{/if}

<style>
  /* Una grilla de puntos que se corre y se escala con la pizarra: dice que
     el espacio sigue más allá de la ventana. */
  .board {
    position: relative;
    height: 100%;
    min-height: 0;
    overflow: hidden;
    background-color: var(--rb-bg0);
    background-image: radial-gradient(
      color-mix(in sRGB, var(--rb-text) 13%, transparent) 1px,
      transparent 1.4px
    );
    color: var(--rb-text);
    font-family: var(--rb-font);
    cursor: grab;
    -webkit-font-smoothing: antialiased;
  }

  .board.is-grid {
    background-image:
      linear-gradient(
        to right,
        color-mix(in sRGB, var(--rb-text) 7%, transparent) 1px,
        transparent 1px
      ),
      linear-gradient(
        to bottom,
        color-mix(in sRGB, var(--rb-text) 7%, transparent) 1px,
        transparent 1px
      );
  }

  .board.is-plain {
    background-image: none;
  }

  .board.is-panning {
    cursor: grabbing;
    user-select: none;
  }

  /* El plano sin tamaño propio: las consolas cuelgan de su esquina. */
  .plane {
    position: absolute;
    top: 0;
    left: 0;
    width: 0;
    height: 0;
    transform-origin: 0 0;
    cursor: auto;
  }

  .plane.is-flying {
    transition: transform var(--duration-very-slow) var(--ease-calm);
  }

  /* Lo flotante va siempre por encima de las consolas. */
  .float {
    position: absolute;
    z-index: 10000;
    cursor: auto;
    pointer-events: none;
  }

  .float > :global(*) {
    pointer-events: auto;
    transition: opacity var(--duration-slow) var(--ease-smooth-out);
  }

  /* Quietos, el zoom se atenúa y el minimapa se esconde: vuelven al mover la
     vista o al pasarles el mouse encima. */
  .float.is-zoom.is-quiet > :global(*) {
    opacity: 0.4;
  }

  .float.is-minimap.is-quiet > :global(*) {
    opacity: 0;
  }

  .float.is-quiet:hover > :global(*),
  .float.is-quiet:focus-within > :global(*) {
    opacity: 1;
  }

  /* Abajo a la izquierda y creciendo hacia arriba: sus menús abren hacia
     arriba con lugar de sobra. En ventanas angostas sube por sobre la
     entrada de texto para no taparla. */
  .float.is-list {
    top: 12px;
    bottom: 16px;
    left: 12px;
    display: flex;
    align-items: flex-end;
  }

  @media (width < 1200px) {
    .float.is-list {
      bottom: 88px;
    }
  }

  .float.is-composer {
    right: 0;
    bottom: 16px;
    left: 0;
    display: flex;
    justify-content: center;
  }

  /* Arriba a la derecha: abajo chocaba con la entrada en ventanas angostas. */
  .delegate-menu {
    position: fixed;
    z-index: 20000;
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 230px;
    border-radius: 12px;
    padding: 6px;
    background: var(--rb-surface-elevated, var(--rb-surface));
    box-shadow:
      0 0 0 1px color-mix(in sRGB, var(--rb-text) 10%, transparent),
      0 12px 32px -8px rgb(0 0 0 / 45%);
    transform: translateX(-100%);
  }

  .delegate-hint {
    margin: 2px 6px 4px;
    color: var(--rb-faint);
    font-size: 11.5px;
  }

  .delegate-pick {
    display: flex;
    align-items: center;
    gap: 10px;
    border: 0;
    border-radius: 8px;
    padding: 7px 8px;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }

  .delegate-pick:hover {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .card-action {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 24px;
    height: 24px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--rb-faint);
    cursor: pointer;
    transition:
      background-color var(--duration-fast) ease,
      color var(--duration-fast) ease;
  }

  .card-action:hover {
    background: color-mix(in sRGB, var(--rb-text) 9%, transparent);
    color: var(--rb-text);
  }

  .float.is-minimap {
    right: 12px;
    bottom: 16px;
  }

  /* En una ventana angosta taparía la entrada: ahí sobra. */
  @media (width < 1100px) {
    .float.is-minimap {
      display: none;
    }
  }

  .float.is-zoom {
    top: 12px;
    right: 12px;
  }

  .empty {
    position: absolute;
    top: 50%;
    left: 50%;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    width: min(520px, calc(100% - 32px));
    text-align: center;
    cursor: auto;
    transform: translate(-50%, -60%);
  }

  .empty-title {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
    text-wrap: balance;
  }

  .empty-hint {
    margin: 0 0 14px;
    color: var(--rb-muted);
    font-size: 13px;
    text-wrap: balance;
  }

  .empty-setup {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 8px;
    max-width: 100%;
    margin-bottom: 14px;
  }

  .empty-folder {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
    max-width: 320px;
    height: 34px;
    border: 0;
    border-radius: 10px;
    padding: 0 10px;
    background: var(--rb-surface);
    color: var(--rb-muted);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
    box-shadow: 0 0 0 1px color-mix(in sRGB, var(--rb-text) 8%, transparent);
    transition:
      box-shadow var(--duration-fast) ease,
      color var(--duration-fast) ease;
  }

  .empty-folder:hover {
    color: var(--rb-text);
    box-shadow: 0 0 0 1px color-mix(in sRGB, var(--accent) 50%, transparent);
  }

  .empty-name {
    flex-shrink: 0;
    color: var(--rb-text);
    font-weight: 600;
  }

  .empty-path {
    min-width: 0;
    overflow: hidden;
    color: var(--rb-faint);
    font-size: 11.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .empty-stepper {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 34px;
    box-sizing: border-box;
    border-radius: 10px;
    padding: 3px;
    background: var(--rb-surface);
    box-shadow: 0 0 0 1px color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .empty-stepper button {
    display: grid;
    place-items: center;
    width: 28px;
    height: 100%;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--rb-text);
    cursor: pointer;
    transition: background-color var(--duration-fast) ease;
  }

  .empty-stepper button:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .empty-stepper button:disabled {
    color: var(--rb-faint);
    cursor: default;
    opacity: 0.4;
  }

  .empty-count {
    min-width: 76px;
    font-size: 12.5px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    text-align: center;
    white-space: nowrap;
  }

  .empty-agents {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 8px;
  }

  .empty-agent {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    width: 112px;
    border: 0;
    border-radius: 14px;
    padding: 14px 8px;
    background: var(--rb-surface);
    color: var(--rb-text);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
    box-shadow: 0 0 0 1px color-mix(in sRGB, var(--rb-text) 8%, transparent);
    transition:
      box-shadow var(--duration-fast) ease,
      scale var(--duration-fast) ease;
  }

  .empty-agent.is-shell {
    color: var(--rb-muted);
  }

  .empty-agent:hover {
    box-shadow: 0 0 0 1px color-mix(in sRGB, var(--accent) 50%, transparent);
  }

  .empty-agent:active {
    scale: 0.96;
  }

  @media (prefers-reduced-motion: reduce) {
    .plane.is-flying {
      transition: none;
    }
  }
</style>
