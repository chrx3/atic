<script lang="ts">
  import { tip } from "$surfaces/overlay/tip.svelte";
  /**
   * La pill: barra flotante siempre visible con la rueda de herramientas.
   *
   * Modelo: tres ejes ORTOGONALES en vez de un enum de prioridad.
   *
   *   activity  qué está haciendo la app   (idle | recording | dictating)
   *   surface   qué hay desplegado          (none | wheel)
   *   queue     pegados pendientes
   *
   * Antes eran un solo `mode` con prioridad clipboard > … > idle, así que abrir
   * el historial mientras grababas hacía desaparecer la grabación de la pill —
   * y con ella el botón de detener. Separados, cada eje se pinta en su lugar.
   *
   * Geometría: `content` se deriva del estado, un único $effect reconcilia la
   * ventana, y el tamaño lo aplica Rust en un solo IPC (resize + posición). No
   * hay banderas de carrera: el reconciliador descarta destinos obsoletos.
   */
  import { onMount, tick, untrack } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import type { DictationPhase } from "$core/types";
  import { capture } from "$domain/capture.svelte";
  import { dictation as dictationStore } from "$domain/dictation.svelte";
  import { paste } from "$domain/paste.svelte";
  import { appUpdate } from "$domain/appUpdate.svelte";
  import { sessionEffect } from "$domain/session";
  import Waveform from "$lib/Waveform.svelte";
  import AticMark from "$lib/AticMark.svelte";
  import { liquid } from "$surfaces/overlay/group.svelte";
  import { publishEmergeSkin } from "$surfaces/overlay/floatEmergeSkin";
  import { RectTracker } from "$lib/liquid/measure.svelte";
  import {
    clampDockedTabRect,
    gapBetween,
    notchShape,
    pillShape,
    stemBetween,
    type Rect,
  } from "$lib/liquid/geometry";
  import { INFLUENCE, REACH } from "$lib/liquid/constants";
  import ToolIcon, { type IconId } from "$lib/ToolIcon.svelte";
  import AgentLogo from "$features/agents/AgentLogo.svelte";
  import { agents, sessionAnswering } from "$lib/agentSessions.svelte";
  import { presence } from "$lib/agentPresence.svelte";
  import ParticleWheel from "$lib/ParticleWheel.svelte";
  import {
    WHEEL_TOOLS,
    AGENTS_ENABLED,
    AGENT_PAGER_ENABLED,
    toolById,
    type ToolId,
  } from "$lib/tools";
  import {
    PILL_BACK_ID,
    PILL_CUSTOMIZE_ID,
    PILL_MORE_ID,
    PILL_WINDOW_ID,
    pillLayout,
    pillStripPage,
    type PillStripId,
    type PillWheelId,
  } from "$core/pillTools";
  import { config } from "$domain/config.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import { localizeTool, t } from "$domain/i18n.svelte";
  import { formatShortcut } from "$lib/format";
  import Icon from "$ui/Icon.svelte";
  import {
    Check,
    Cpu,
    Download,
    MemoryStick,
    Pause,
    Play,
    Volume2,
    VolumeX,
    X,
  } from "$lib/icons";
  import { media } from "$domain/media.svelte";
  import { lyricIndex } from "$domain/lyrics";
  import { mediaPosition } from "$domain/mediaTime";
  import type { IconNode } from "morphicons/svelte";
  import { PILL, windowFor, type Size } from "$surfaces/overlay/pillStage";
  import { createCssStage } from "$surfaces/overlay/pillCssStage";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";
  import {
    blocksBrowserChrome,
    consoleSideFor,
    contentFor,
    discJoinsTail,
    dragClosesWheel,
    FLIGHT_SKIP_PX,
    isDiscOnly,
    islandLiveSlots,
    morphsInPlace,
    bloomPivot,
    nextBarWidth,
    pivotFor,
    stepWheel as nextWheelTool,
    undockForSummon,
    shouldRecenterTopNotch,
    shouldReturnToEdgeOnActivate,
    shouldMeasureBar,
    islandHoverStay,
    islandHoverOpens,
    floatWheelHoverWatches,
    floatWheelHoverOpens,
    islandFaceAgent,
    islandFaceDictation,
    islandFaceLive,
    islandFacePanel,
    islandFaceBlocksHover,
    islandNotchRadius,
    pointInRect,
    notchRecenterSize,
    sideExitX,
    pointerMoveDrags,
    pointerGestureWasClick,
    ISLAND_COLLAPSE_MS,
    ISLAND_COLLAPSE_MORE_MS,
    wheelChromeActive,
    wheelKeyAction,
    wheelOpenFlight,
    type Dock,
    type IslandFace,
    type Surface,
  } from "$surfaces/overlay/pill/pillPlan";
  import {
    agentChip,
    agentChips,
    agentChipLogos,
    logoSlots,
    type AgentChip,
  } from "$surfaces/overlay/pill/pillAgentChip";
  import { consoleCue } from "$surfaces/overlay/agents/consoleCue.svelte";
  import { agentsDock } from "$surfaces/overlay/agents/agentsDock.svelte";
  import { retachPreview } from "$surfaces/overlay/retachPreview.svelte";
  import { pushRecentColor } from "$features/color/colorMath";
  import {
    isPeekTool,
    leavePeekPanel,
    peekFor,
    toolPeek,
    toolPeekState,
  } from "$surfaces/overlay/pill/toolPeek.svelte";
  import {
    areaFor,
    dockAxis,
    defaultPillHome,
    edgeCenterPoint,
    geometryReseat,
    edgeWallsFor,
    MENISCUS_FLARE,
    snapDrop,
    snapMagnet,
    shouldUndock,
    pillHomeFrom,
    pillHomePoint,
    type DockEdge,
    type PillHome,
  } from "$surfaces/overlay/edgeDock";
  import { clearPillHome, readPillHome, writePillHome } from "./pillHomeStore";
  import PillPending, { type PendingItem } from "$surfaces/overlay/pill/PillPending.svelte";
  import { isWorking } from "$features/agents/chatStatus";
  import IslandPeek from "$surfaces/overlay/pill/IslandPeek.svelte";
  import {
    afterTransition,
    MOTION,
    ms,
    opacityFade,
    prefersReducedMotion,
    tabPanel,
  } from "$lib/motion";
  import { playWheelTick } from "$ipc/uiSound";
  import { launcherLab } from "$lib/dev/launcherLab.svelte";
  import type { PermissionDecision, PresenceActivity } from "$core/types";
  // Lo que queda son los comandos DE LA PILL: su geometría, sus atajos y las
  // ventanas que abre. El estado de la app lo traen los stores.
  import {
    agentsAlwaysOnTop,
    agentsWindowVisible,
    hideAgentsWindow,
    presentAgentsWindow,
    onAgentsBubbleAnchor,
    onAgentsBubbleDismiss,
    agentPresenceFocus,
    agentPresenceBind,
    consoleForPresence,
    agentsEnsureWindow,
    focusAgentSession,
    setAgentsConsoleOpen,
  } from "$ipc/agents";
  import {
    clipboardAlwaysOnTop,
    hideClipboardWindow,
    onClipboardBubbleDismiss,
    showClipboardWindow,
  } from "$ipc/clipboard";
  import { getConfig, showMainWindow } from "$ipc/config";
  import { on } from "$ipc/events";
  import { hideLauncher, onLauncherBubbleDismiss } from "$ipc/search";
  import {
    hideSnippetsWindow,
    onSnippetsBubbleDismiss,
    showSnippetsWindow,
    snippetsAlwaysOnTop,
  } from "$ipc/snippets";
  import {
    hideSystemWindow,
    onSystemBubbleDismiss,
    showSystemWindow,
    systemAlwaysOnTop,
  } from "$ipc/system";
  import ClipboardHistoryList from "$lib/ClipboardHistoryList.svelte";
  import SnippetsList from "$lib/SnippetsList.svelte";
  import SystemPanel from "$features/system/SystemPanel.svelte";
  import PillCustomize from "./PillCustomize.svelte";
  import { systemAlerts } from "$domain/systemAlerts.svelte";
  import { system } from "$domain/system.svelte";
  import AgentLauncher from "$features/agents/AgentLauncher.svelte";
  import { transferInbox } from "$features/agents/consoleTransfer.svelte";
  import { OVERLAY_LABEL } from "$surfaces/overlay/contract";
  import {
    agentsIslandHost,
    agentsFloatHandoff,
  } from "$surfaces/overlay/agents/agentsIslandHost.svelte";
  import {
    AGENTS_OVERLAY_DETACH,
    AGENTS_OVERLAY_DETACHED,
    type AgentsOverlayDetachDetail,
    type AgentsOverlayDetachedDetail,
  } from "$features/agents/consoleTransfer.svelte";
  import { clipboard } from "$domain/clipboard.svelte";
  import { snippets } from "$domain/snippets.svelte";
  import { executeToolAction } from "$surfaces/toolActions";
  import { isSpatialTool } from "$surfaces/overlay/toolSlots";
  import {
    enqueueActivate,
    isCursorAnchored,
    pillToCursorMovePx,
    shouldCommitShow,
    shouldReturnHomeAfterClose,
    slotIntent,
    spatialDismissTargets,
    type SlotRequest,
  } from "$surfaces/overlay/slotIntent";
  import {
    armOpenDismissGrace,
    isOpenDismissGrace,
  } from "$surfaces/overlay/openDismissGrace";
  import {
    birthAtCursor,
    captureToolBirth,
    captureToolResting,
    waitToolResting,
  } from "$surfaces/overlay/toolBirth";
  import {
    onOverlayDismiss,
    onOverlayYieldMain,
    onOverlayReady,
    onOverlayWorkArea,
    onPillRadialPress,
    onPillRadialRelease,
    onPillReset,
    overlayCursor,
    overlayCursorOverHit,
    overlayPrimaryDown,
    pillTrace,
    savePillHome,
    setOverlayPointerGesture,
    setOverlayTextMode,
    workAreaOf,
  } from "$ipc/overlay";
  import { OVERLAY_GEOMETRY } from "$surfaces/overlay/overlayGeometry";

  /*
   * Los tipos y las decisiones viven en `pill/pillPlan.ts`, que es TS puro y
   * está testeado. Acá queda la ejecución: el estado, los efectos y los viajes
   * a Rust.
   *
   * Clipboard, textos y agentes abren como cara de la isla (un solo blob).
   * El launcher sigue siendo float espacial.
   */

  /**
   * En Mac el overlay lo maneja AppKit y el teclado de la cara funciona sin
   * pedir nada. En Windows la ventana es `WS_EX_NOACTIVATE` y hay que pedirlo
   * explícito: ver el efecto de `faceKeysOpen`.
   */
  const isMac =
    typeof navigator !== "undefined" &&
    /mac/i.test(navigator.platform || navigator.userAgent);

  /**
   * El estado de la app no es de la pill.
   *
   * Grabación, dictado y cola vivían acá con sus propias copias y sus propios
   * oyentes, duplicados con la ventana principal: dos cronómetros contando lo
   * mismo. Ahora se declara una vez qué necesita esta ventana y el resto se lee.
   */
  $effect(() =>
    sessionEffect(["config", "capture", "dictation", "paste", "clipboard", "snippets"]),
  );

  // ─── Eje 1: actividad ────────────────────────────────────────────────────
  const recording = $derived(capture.active);
  const elapsed = $derived(capture.elapsed);
  const levels = $derived(capture.levels);
  const dictation = $derived(dictationStore.phase);
  const dictationMessage = $derived(dictationStore.message);
  /** Llegó al menos un fragmento en vivo: la transcripción en directo anda. */
  const liveActive = $derived(capture.segments.length > 0);
  const liveError = $derived(capture.liveError);
  const btWarning = $derived(capture.note);
  const busy = $derived(capture.busy);

  const dictating = $derived(dictationStore.active);
  const activity = $derived(recording ? "recording" : dictating ? "dictating" : "idle");

  // ─── Eje 2: superficie ───────────────────────────────────────────────────
  let surface = $state<Surface>("none");
  /** Visual, separado del lógico: la rueda se revela recién con la ventana ya
   *  reencuadrada, para que el morph nunca se pinte a mitad del resize. */
  let wheelShown = $state(false);
  let wheelTool = $state<PillWheelId | null>(null);
  /** Qué anillo se está mirando. `more` es el submenú detrás del gajo «Más». */
  let wheelPage = $state<"ring" | "more">("ring");
  /** Lo mismo para la tira acoplada: comparten preferencia y comparten paso. */
  let stripPage = $state<"ring" | "more">("ring");
  /** Cierre acelerado: al elegir herramienta la rueda ya cumplió su función. */
  let wheelQuick = $state(false);
  /**
   * La rueda la abrió el hover del disco flotante, no un clic ni el atajo.
   *
   * Solo entonces alejar el mouse la cierra, como la isla. Un summon con
   * `Alt+Z` se queda hasta Esc, clic afuera o elegir herramienta.
   */
  let wheelHeldByHover = $state(false);
  /** Tras cerrar, no reabrir en el mismo hover: el morph deja el cursor encima. */
  let floatWheelHoverLockUntil = 0;
  /**
   * Qué superficie se está cerrando.
   *
   * El pivote del colapso depende de **qué se cierra**, no del estado destino:
   * para cuando el reconciliador corre, `surface` ya vale `"none"` y el
   * colapso de la rueda es indistinguible del reposo. Es `$state` porque el
   * markup (stack dim, `.p-wheel`, piel) también tiene que reaccionar.
   */
  let collapsingFrom = $state<"wheel" | null>(null);
  /** Un solo chrome visible: rueda abierta o colapsando in-situ. */
  const wheelChrome = $derived(wheelChromeActive({ surface, collapsingFrom }));

  /**
   * Aviso de agente en la barra compacta.
   *
   * Une el chat de Atic (si está habilitado) con las TUI que el pager mira.
   * La decisión es pura (`agentChip`); acá solo se ejecuta.
   */
  const chipState = $derived({
    chat: {
      unread: agents.unread,
      working: agents.working,
      waiting: agents.waiting,
      readyLabel: agents.readyLabel,
      readyBackendId: agents.readyBackendId,
      updatedAt: agents.readyUpdatedAt,
      answering: agents.answering,
      providerSessions: agents.sessions.map((s) => s.providerSession),
    },
    presence: presence.view,
    // Un chip por chat propio (los que abre otro agente por MCP no avisan en
    // la pill): tocar uno lleva a esa sesión en la ventana de agentes.
    chats: agents.sessions
      .filter((s) => !s.parent)
      .map((s) => ({
        id: s.id,
        backendId: s.backendId,
        status: s.status === "working" && !isWorking(s) ? "ready" : s.status,
        pending: s.pending.length,
        unread: s.unread,
        lastText: s.lastText,
        answering: sessionAnswering(s),
        updatedAt: s.lastTextAt,
      })),
    chatEnabled: AGENTS_ENABLED,
    pagerEnabled: AGENT_PAGER_ENABLED,
    consoles: consoleCue.clis,
    now: Date.now(),
    // Textos del chip cuando el agente todavía no escribió nada: el estado
    // distingue contestar (stream vivo) de trabajar (herramientas).
    workingLabel: t("pill.chipWorking"),
    answeringLabel: t("pill.chipAnswering"),
  });
  const chips = $derived(agentChips(chipState));
  const chip = $derived(agentChip(chipState));
  const agentAlert = $derived(chips.length > 0);
  const agentWorking = $derived(
    chips.some((c) => c.tone === "working" || c.tone === "count"),
  );
  /**
   * El panel desplegado ya cubre el aviso; achicado, la pestaña vive en la pill.
   *
   * La cara de agentes abierta EN LA ISLA no cuenta como panel desplegado:
   * comparte la banda con la marca y los chips siguen siendo el aviso. El
   * hit-rect `agents` sí se publica con la cara abierta (drop OLE), así que se
   * excluye por la cara y no por el rect.
   *
   * Va con `$derived.by` y no con una expresión: `agentsFaceOpen` se declara
   * más abajo y los deriveds son perezosos, pero el chequeo de tipos no admite
   * la referencia directa.
   */
  const agentsExpanded = $derived.by(() => {
    if (agentsDock.minimized) return false;
    if (surfaces.live["agents"] == null) return false;
    return !agentsFaceOpen;
  });
  const showAgentTab = $derived(
    !wheelChrome && (agentsDock.minimized || (agentAlert && !agentsExpanded)),
  );
  /**
   * Pestaña acoplada más gruesa + chrome de estado. En 10 px no cabe un punto
   * legible. Misma condición que el chip flotante (aviso o dock achicado).
   */
  const islandCue = $derived(surface === "edge" && showAgentTab);
  /**
   * El primer aviso dice qué pasa: preview del agente («Refactorizando el
   * hub»), «permiso» en espera, o el estado («Trabajando…»). Solo en notch
   * del eje y —a los lados no hay largo que regalar— y solo el chip primero,
   * el urgente; el resto queda logo solo.
   *
   * También a los lados: ahí el texto va girado a lo largo del canto, y el
   * tramo que reserva `islandCueLong` se suma al alto en vez de al ancho.
   * Esconderlo dejaba al notch lateral diciendo menos que el de arriba.
   */
  const islandCueMsg = $derived.by(() => {
    if (!islandCue || !dock) return false;
    if (chips.length === 0) return false;
    return chips[0].tone !== "count" && chips[0].label != null;
  });
  /**
   * Aviso de actualización: qué muestra el chip y qué dice al pasar el mouse.
   *
   * Un solo botón para las cuatro fases porque `appUpdate.advance()` ya es un
   * solo camino: si no está descargada, baja; si ya está, instala y reinicia.
   * Los textos son los mismos que la gota de la ventana principal — es el
   * mismo aviso, no otro dialecto.
   */
  const updateChip = $derived.by(() => {
    if (!appUpdate.visible) return null;
    const version = appUpdate.version ?? "";
    if (appUpdate.installing) {
      return {
        tone: "busy" as const,
        icon: Download,
        text: "…",
        label: t("about.bubbleInstalling", { version }),
      };
    }
    if (appUpdate.downloading) {
      return {
        tone: "busy" as const,
        icon: Download,
        // Sin `contentLength` no hay porcentaje: GitHub no siempre lo manda.
        text: appUpdate.percent == null ? "…" : `${appUpdate.percent}%`,
        label: t("about.bubbleDownloading", { version }),
      };
    }
    if (appUpdate.downloaded) {
      return {
        tone: "ready" as const,
        icon: Check,
        text: version,
        label: t("about.bubbleInstall", { version }),
      };
    }
    return {
      tone: "new" as const,
      icon: Download,
      text: version,
      label: t("about.bubbleDownload", { version }),
    };
  });

  /**
   * Aviso del equipo: CPU o memoria sostenidas por encima del umbral.
   *
   * Mismo sitio y misma cápsula que el aviso de actualización — es otra cosa
   * que la pill CUENTA, no otra cosa que la reemplace. El texto es el
   * porcentaje; quién se lo está comiendo va en el globo, que es donde cabe.
   */
  const systemChip = $derived.by(() => {
    const aviso = systemAlerts.top;
    if (!aviso) return null;
    const valor = Math.round(aviso.value);
    const que =
      aviso.kind === "cpu" ? t("overlay.system.cpu") : t("overlay.system.ram");
    return {
      // Un icono por métrica, no un pulso genérico: acoplada, el icono es TODO
      // lo que se ve, y un "87%" sin decir de qué no informa nada.
      icon: aviso.kind === "cpu" ? Cpu : MemoryStick,
      text: `${que} ${valor}%`,
      kind: aviso.kind,
      label: aviso.culprit
        ? t("overlay.system.alertBy", { what: que, value: valor, name: aviso.culprit })
        : t("overlay.system.alert", { what: que, value: valor }),
    };
  });

  /**
   * Rueda del mouse sobre la pill = volumen del equipo.
   *
   * Es el gesto que el panel nunca debió pedir: subir el volumen no puede
   * costar rueda → Sistema → pestaña → slider. El cursor ya está sobre la
   * pill, que además vive siempre arriba: es el único sitio de Atic donde
   * este gesto sale gratis.
   *
   * No se roba el scroll de nadie: si el puntero está sobre una cara (la
   * lista del historial, la consola, el panel del costado), la rueda es de
   * ellas. Solo el cuerpo de la pill lo toma.
   */
  const VOLUME_STEP = 0.04;
  /** Cuánto queda el chip a la vista después del último giro. */
  const VOLUME_CUE_MS = 1400;
  let volumeCue = $state<number | null>(null);
  let volumeCueTimer = 0;

  async function onPillWheel(event: WheelEvent) {
    const el = event.target as HTMLElement | null;
    // Lo que tiene scroll propio se queda con la rueda.
    if (el?.closest(".p-face, .p-side, .p-wheel")) return;
    if (event.deltaY === 0) return;
    event.preventDefault();
    // Sin audio leído todavía: se pide una vez y este giro ya cuenta.
    if (!system.audio) await system.hydrate("audio");
    const actual = system.audio?.volume ?? 0;
    // Rueda arriba sube: `deltaY` es negativo hacia arriba.
    const next = Math.min(
      1,
      Math.max(0, actual - Math.sign(event.deltaY) * VOLUME_STEP),
    );
    volumeCue = Math.round(next * 100);
    window.clearTimeout(volumeCueTimer);
    volumeCueTimer = window.setTimeout(() => (volumeCue = null), VOLUME_CUE_MS);
    if (system.audio?.muted && next > 0) void system.setMuted(false);
    await system.setVolume(next);
  }

  /**
   * El chip del volumen comparte cápsula con los otros avisos.
   *
   * Es lo mismo que hacen el update y el aviso del equipo: algo que la pill
   * cuenta por un rato, en el sitio donde ya se cuentan las cosas. Así no hay
   * un HUD nuevo que posicionar contra el borde ni una superficie que medir.
   */
  const volumeChip = $derived.by(() => {
    if (volumeCue == null) return null;
    return {
      icon: volumeCue === 0 ? VolumeX : Volume2,
      text: `${volumeCue}%`,
      label: t("overlay.system.volume"),
    };
  });

  /**
   * El clic del aviso abre el panel de sistema y baja el chip.
   *
   * Bajar no es "olvidar": el aviso vuelve si el problema se va y regresa. Lo
   * que no puede pasar es que quede pulsando mientras lo estás mirando.
   */
  function onSystemChipClick() {
    const aviso = systemAlerts.top;
    if (aviso) systemAlerts.dismiss(aviso.kind);
    requestActivateAtSlot("system", { force: true });
  }

  /**
   * Qué estado lleva la cara de la marca. La mascota dice QUÉ está corriendo;
   * antes lo decía una gota colgada que cambiaba la silueta.
   */
  const markState = $derived<"idle" | "recording" | "dictating">(
    recording ? "recording" : dictating ? "dictating" : "idle",
  );

  /**
   * Qué hace el clic en la marca, y qué dice el globo.
   *
   * Si la cara muestra la herramienta que está corriendo, apretarla la para:
   * un segundo botón rojo al lado era decir dos veces lo mismo y ocupar el
   * ancho que la cápsula necesita para el contador. En reposo el clic no hace
   * nada: la rueda se abre por hover (flotante) o con el atajo.
   */
  const markAction = $derived.by(() => {
    if (markState === "recording") {
      return { label: btWarning ?? t("pill.stopRecord"), run: toggleRecord };
    }
    if (markState === "dictating") {
      return { label: t("pill.stopDictate"), run: toggleDictate };
    }
    // Sin acción en reposo: acoplada la rueda no se abre nunca y flotante la
    // despliega el hover.
    return { label: discHint, run: () => {} };
  });

  /**
   * Acoplada: la pestaña engorda si hay algo que decir además de la marca.
   *
   * La actividad NO cuenta: su control es la propia marca, que ya estaba ahí.
   */
  /**
   * Lo que suena, en la pestaña en reposo: carátula chica y un ecualizador.
   *
   * Solo sonando —en pausa la pestaña vuelve a estar limpia— y cede ante los
   * avisos que piden algo (un agente, una actualización): la música se mira,
   * no se atiende. Lee `media.now` directo y no `mediaCell`, que se declara
   * más abajo.
   */
  const mediaRest = $derived(
    surface === "edge" && media.now?.playing && !showAgentTab && !updateChip
      ? media.now
      : null,
  );
  /**
   * Tema nuevo: la pestaña se alarga unos segundos con el título y vuelve a
   * carátula + ecualizador. Mismo tramo que el texto de los avisos de agentes.
   */
  const MEDIA_TITLE_MS = 4_000;
  let mediaTitleKey: string | null = null;
  let mediaTitleShown = $state(false);
  /*
   * El temporizador vive fuera del efecto a propósito: el sondeo trae un
   * objeto nuevo cada segundo y medio, el efecto se vuelve a correr y su
   * limpieza se llevaría el temporizador — el título quedaba pegado.
   */
  let mediaTitleTimer = 0;
  $effect(() => {
    const key = mediaRest?.thumb_key ?? null;
    if (!key || key === mediaTitleKey) return;
    mediaTitleKey = key;
    mediaTitleShown = true;
    window.clearTimeout(mediaTitleTimer);
    mediaTitleTimer = window.setTimeout(() => (mediaTitleShown = false), MEDIA_TITLE_MS);
  });
  $effect(() => () => window.clearTimeout(mediaTitleTimer));
  const mediaTitleOn = $derived(
    mediaRest != null && mediaTitleShown && Boolean(mediaRest.title),
  );
  /**
   * La línea de letra que suena, si el tema tiene letra sincronizada: `""`
   * entre versos (el tramo se queda, así la caja no salta en cada pausa
   * instrumental). Arriba/abajo cuelga de la pestaña, grande, con el verso
   * siguiente (`lyricHang`); al costado va girada en la fila, tras el título.
   */
  let lyricClock = $state(Date.now());
  $effect(() => {
    if (!mediaRest || !media.lyrics) return;
    const timer = window.setInterval(() => (lyricClock = Date.now()), 250);
    return () => window.clearInterval(timer);
  });
  const mediaLyricAt = $derived.by(() => {
    const lines = media.lyrics;
    if (!mediaRest || !lines) return null;
    const position = mediaPosition(mediaRest, lyricClock);
    return position == null ? null : lyricIndex(lines, position);
  });
  const mediaLyric = $derived(
    mediaLyricAt == null ? null : (media.lyrics?.[mediaLyricAt]?.text ?? ""),
  );
  const mediaLyricNext = $derived(
    mediaLyricAt == null ? "" : (media.lyrics?.[mediaLyricAt + 1]?.text ?? ""),
  );

  const edgeCue = $derived(
    surface === "edge" &&
      (showAgentTab ||
        updateChip != null ||
        systemChip != null ||
        volumeChip != null ||
        mediaRest != null),
  );
  /**
   * Celdas del aviso de la consola minimizada (sin chip propio).
   *
   * Ese aviso junta los logos de todos los agentes vivos en UN botón que crece
   * con su contenido; la pestaña se mide antes y contaba uno solo, así que con
   * cinco agentes los logos se salían. `logoSlots` los topa en tres celdas.
   */
  const dockCueCells = $derived(
    chips.length === 0 && agentsDock.minimized ? logoSlots(chipLogos(chip)).cells : 1,
  );
  const edgeCueMarks = $derived(
    (showAgentTab
      ? Math.max(chips.length, agentsDock.minimized ? dockCueCells : 0)
      : 0) +
      (updateChip ? 1 : 0) +
      (systemChip || volumeChip ? 1 : 0) +
      // Carátula + ecualizador: dos celdas.
      (mediaRest ? 2 : 0),
  );

  /**
   * ¿El aviso lleva a algún lado?
   *
   * Sin nada que abrir, enfocar ni atar, un botón promete lo que no cumple:
   * cursor de mano, hover y un clic que se come el gesto sin decir nada. En
   * ese caso el aviso es texto, y el mensaje entero vive en el globo.
   */
  function chipActs(c: AgentChip): boolean {
    return (
      agentsDock.minimized || c.target.kind !== "none" || c.target.presenceId != null
    );
  }

  function chipLogos(c: AgentChip): string[] {
    return agentChipLogos(
      c,
      {
        sessions: agents.sessions,
        presence: presence.view,
        consoles: consoleCue.clis,
      },
      agentsDock.minimized,
    );
  }

  function chipAria(c: AgentChip): string {
    if (agentsDock.minimized && c.tone === "off") return t("page.agents.dockExpand");
    const target = c.target;
    if (target.kind === "focus") {
      const name =
        presence.list.find((p) => p.id === target.presenceId)?.backendName ??
        t("pill.agentFallback");
      return t("pill.goToAgent", { name });
    }
    if (target.kind === "console") return t("pill.openConsole");
    if (target.kind === "none") return t("pill.unbound");
    if (c.tone === "working") return t("pill.working");
    return c.label ?? t("pill.ready");
  }

  function chipTitle(c: AgentChip): string {
    if (agentsDock.minimized && c.tone === "off") {
      return t("page.agents.dockExpand");
    }
    if (c.target.kind === "none") return t("pill.unboundTitle");
    const base =
      c.tone === "waiting"
        ? t("pill.waiting")
        : c.tone === "ready"
          ? (c.label ?? t("pill.ready"))
          : c.tone === "count"
            ? t("pill.unread", { label: c.label ?? "" })
            : (c.label ?? t("pill.working"));
    if (c.target.kind === "focus") return t("pill.rebind", { base });
    return base;
  }

  /**
   * Qué hace el agente de esta fila, si se sabe.
   *
   * Solo la presencia que lee el transcript (Claude Code) la trae; el chat y
   * las demás TUI dicen «trabajando» a secas y se animan como «pensando».
   */
  function liveActivity(c: AgentChip): PresenceActivity | null {
    if (c.tone !== "working") return null;
    return presence.view.find((p) => p.id === c.id)?.activity ?? null;
  }

  function activityText(a: PresenceActivity): string {
    const detail = a.detail?.trim();
    return detail
      ? t(`pill.activity.${a.kind}On`, { detail })
      : t(`pill.activity.${a.kind}`);
  }

  /** El aviso completo al pasar el mouse: la fila lo recorta a una línea. */
  function liveTitle(c: AgentChip): string {
    const full = presence.view.find((p) => p.id === c.id)?.preview?.trim();
    if (c.tone === "ready" && full) return full;
    return chipTitle(c);
  }

  /** «Ya lo vi»: solo los avisos de listo; trabajar o pedir permiso siguen. */
  function dismissLive(c: AgentChip): void {
    // Cada chat trae su fila (`chat:<id>`); `chat` a secas es el resumen.
    if (c.target.kind === "chat") agents.markRead(c.target.session);
    else if (c.id === "chat") agents.markAllRead();
    else presence.markSeen(c.id);
  }

  /** Texto visible de una fila: el pipeline manda; el fallback solo nombra el estado. */
  function chipLiveLabel(c: AgentChip): string {
    const label = c.label?.trim();
    if (label) return label;
    if (c.tone === "count") return String(c.label ?? 0);
    if (c.tone === "waiting") return t("pill.permission");
    if (c.tone === "working") return t("pill.chipWorking");
    return t("pill.ready");
  }

  const agentChipAria = $derived(chipAria(chip));
  const authRequest = $derived(agents.primaryPending);
  /**
   * Todo lo que los agentes esperan, de todos a la vez: la tarjeta pagina entre
   * ellos. `authRequest` sigue siendo el primero, para la cara del borde.
   */
  const pendingItems = $derived<PendingItem[]>(
    agents.sessions.flatMap((s) =>
      s.pending.map((permission) => ({
        sessionId: s.id,
        backendId: s.backendId,
        agentName: s.label?.trim() || s.backendName,
        permission,
      })),
    ),
  );
  /** Consola abierta: el permiso se decide ahí; no duplicar el diálogo. */
  let agentsConsoleOpen = $state(false);
  const showAuthCard = $derived(
    authRequest !== null && !agentsConsoleOpen && surface === "none",
  );
  /**
   * Vivo / mostrado: como `Bubble`, para que el cierre pueda replegarse al
   * campo líquido en vez de desmontarse a mitad del morph.
   */
  let authAlive = $state(false);
  let authShown = $state(false);
  /** Último permiso visible: sobrevive un frame al cerrar para el repliegue. */
  let authView = $state<NonNullable<typeof authRequest> | null>(null);
  let authBusy = $state(false);
  let authEl = $state<HTMLElement | null>(null);
  const AUTH_CORNER = 12;

  // ─── Eje 3: cola de pegado ───────────────────────────────────────────────
  const hasQueue = $derived(paste.count > 0);

  /**
   * Reposo: la barra es SOLO el disco.
   *
   * Estaba escrito en línea en la clase de `.p-bar`; ahora lo mira también la
   * piel, que monta la gota que llega justo cuando esto deja de valer.
   */
  const discOnly = $derived(
    isDiscOnly({
      surface,
      activity,
      hasQueue,
      agentAlert: agentAlert || agentsDock.minimized,
      hasUpdate: updateChip != null || systemChip != null || volumeChip != null,
    }),
  );

  let wheelShortcut = $state("");

  /**
   * La ayuda del disco: atajo, que el hover abre las tools y que se puede arrastrar.
   *
   * Antes vivía inline en el `use:tip`. Ahora la leen dos consumidores —el
   * `aria-label` del disco y el pie del panel de cupos—, y repetirla dejaría
   * que se separaran sin que nadie lo note.
   */
  /**
   * Qué herramienta abre el panel de cupos. Es una lista porque
   * `ParticleWheel.tipSilent` recibe varias, no porque vaya a haber otra.
   */
  const QUOTA_TOOL = ["agents"] as const;

  let wheelEl = $state<HTMLElement | null>(null);

  /** Lo que dice el vistazo si no hay nada que mostrar: lo que diría el tooltip. */
  function toolHelp(id: ToolId): string {
    const tool = localizeTool(toolById(id));
    return `${tool.label} — ${tool.short}`;
  }

  /**
   * Un gajo con vistazo abre el mismo panel que su botón en la isla.
   *
   * Va por `wheelTool` y no por un `use:toolPeek` porque los gajos los pinta
   * `ParticleWheel`, que es compartido: la pill sabe cuál está activo por el
   * `bind:activeId` que ya existía, y con eso alcanza.
   *
   * Cierra desde el cleanup y solo si fue esta rama la que abrió. Un hide()
   * inmediato ciclaba con el panel encima del gajo; la gracia es la misma
   * que al cruzar isla→panel. Llamar a hide() en cada render con el gajo
   * inactivo también cerraría el panel que abrió la isla.
   */
  let wheelOwnsPeek = false;
  $effect(() => {
    const peekTool = wheelTool;
    if (!wheelShown || !isPeekTool(peekTool) || !wheelEl) return;
    const o = tracker.originAt;
    const r = tracker.rects;
    const parts = [];
    for (const [id, rect] of Object.entries(r)) {
      if (!id.startsWith("wheel-") || !rect) continue;
      parts.push({ x: rect.x + o.x, y: rect.y + o.y, w: rect.w, h: rect.h });
    }
    // El ancla es el gajo, no `.p-wheel`: esa caja es el escenario de 252 px
    // y el panel de cupos se iba abajo de toda la flor.
    const petal =
      wheelEl.querySelector<HTMLElement>(".pw-node.is-hot .pw-node-body") ??
      wheelEl.querySelector<HTMLElement>(".pw-node.is-hot");
    const box = (petal ?? wheelEl).getBoundingClientRect();
    toolPeekState.show(
      { tool: peekTool, fallback: toolHelp(peekTool) },
      { x: box.left, y: box.top, w: box.width, h: box.height },
      parts.length > 0 ? parts : null,
    );
    wheelOwnsPeek = true;
    return () => {
      if (!wheelOwnsPeek) return;
      wheelOwnsPeek = false;
      // Gracia, no corte: si el gajo pierde el mouse porque el panel nació
      // encima, o al cruzar el hueco hacia el pin, hide() inmediato ciclaba.
      leavePeekPanel();
    };
  });

  const discHint = $derived(
    [
      wheelShortcut
        ? t("pill.toolsWithShortcut", { shortcut: formatShortcut(wheelShortcut) })
        : "",
      t("pill.hoverTools"),
      t("pill.dragMove"),
    ]
      .filter(Boolean)
      .join(" · "),
  );

  // ─── Geometría ───────────────────────────────────────────────────────────
  /**
   * El escenario ya no mueve una ventana: escribe `left`/`top` en este
   * componente. La lógica de arriba —`reconcile`, `pivotFor`, los pivotes— es
   * la misma; lo único que cambió es quién ejecuta.
   */
  const stage = createCssStage();
  /** Esquina de la pill dentro del overlay. La escribe el escenario. */
  let at = $state({ x: 0, y: 0 });
  /**
   * Tamaño aplicado, como estado reactivo.
   *
   * El escenario ya lo sabe, pero lo guarda en una closure: leerlo desde el
   * markup no crea dependencia, así que la caja se quedaba con el tamaño viejo
   * mientras el resto del estado sí cambiaba. El síntoma era que un atajo
   * ejecutaba su acción y la pill no se inmutaba.
   */
  let box = $state<Size>({ w: PILL.bar + PILL.pad * 2, h: PILL.bar + PILL.pad * 2 });
  /** Caja exterior: es la que se registra como zona viva y la que se arrastra. */
  let rootEl = $state<HTMLElement | null>(null);
  /** El cuerpo líquido: la silueta de verdad, sin el respiro de la rueda. */
  let liquidEl = $state<HTMLElement | null>(null);
  /** Piel de la isla acoplada: ancla de los floats (no la barra oculta). */
  let islandSkinEl = $state<HTMLElement | null>(null);
  /** El stack: contra él se miden las siluetas, porque no recorta. */
  let stackEl = $state<HTMLElement | null>(null);

  /* ─── La piel, por campo de distancia ───────────────────────────────────
   *
   * Ya no hay vuelta atrás al filtro SVG, y por eso se fue el `Ctrl+Alt+P`: la
   * consola también dibuja por campo, así que volver al goo dejaría la mitad
   * del grupo sin trazar. No hay a qué volver.
   *
   * El CSS no cambia. Sigue decidiendo la geometría y las animaciones —cómo
   * llega la gota— y lo único que cambia es quién dibuja el contorno.
   */
  const tracker = new RectTracker();

  $effect(() => {
    tracker.origin = stackEl;
  });

  /**
   * Las dos altas, creadas UNA vez.
   *
   * No pueden ser flechas escritas en el markup: ahí se recrean en cada
   * render, Svelte las trata como un adjunto distinto y desmonta y vuelve a
   * montar el anterior. Y como la baja borra el rectángulo del que depende
   * este mismo template, el resultado era un bucle: medir, redibujar, dar de
   * baja, volver a medir. La pill quedaba sin piel y la pestaña colgada.
   */
  const trackBar = (el: HTMLElement) => tracker.track("bar", el);
  const trackTail = (el: HTMLElement) => tracker.track("tail", el);
  /** La gota de dictado (onda que cuelga): su silueta va al mismo campo. */
  const trackDrop = (el: HTMLElement) => tracker.track("drop", el);
  /** La isla acoplada: llena la caja, así que se anima con ella. */
  const trackIsland = (el: HTMLElement) => tracker.track("island", el);
  const trackIslandCue = (el: HTMLElement) => tracker.track("island-cue", el);
  const trackIslandUpdateCue = (el: HTMLElement) =>
    tracker.track("island-update-cue", el);
  /**
   * Cada herramienta se mide aparte, aunque hoy la silueta no las use.
   *
   * El intento de que cada icono fuera una GOTA del campo —para que la tira se
   * abriera separándose en vez de crecer— quedó parado: la silueta salía a
   * medias, con las primeras gotas ausentes del registro. Se sigue midiendo
   * para poder ver en el log si llegan las cinco; sin ese dato no tiene sentido
   * volver a intentarlo.
   *
   * Las funciones se crean UNA vez, una por índice: un `@attach` se desmonta y
   * se vuelve a montar cuando cambia la identidad de su función, y la baja de
   * `track()` borra el rect. Con una closure nueva por render las gotas se
   * daban de baja y volvían un cuadro después.
   */
  /**
   * Lo que el usuario dejó en la pill (Ajustes → Pill).
   *
   * Es una sola preferencia para las dos siluetas: el anillo y la tira del
   * canto muestran lo mismo, porque son la misma pill en dos formas. Lo único
   * que no viaja al canto es el submenú: una tira es lineal y no tiene ángulo
   * que ahorrar, así que ahí «Más» se despliega sin gajo intermedio.
   */
  const layout = $derived(
    pillLayout(config.current?.pill_tools, config.current?.pill_more_tools),
  );
  /**
   * La tira, resuelta a lo que hace falta para dibujar cada botón.
   *
   * `Más` y `atrás` no son herramientas —no tienen acción ni slot al que
   * volar—, así que el `id` es el que decide qué hace el toque, no un
   * `ToolDef` que habría que inventar.
   */
  const stripNodes: { id: PillStripId; label: string; short: string; icon: IconId }[] =
    $derived(
      pillStripPage(layout, stripPage, { windowOnFirst: true }).map((id) => {
        if (id === PILL_MORE_ID) {
          return { id, label: t("pill.more"), short: t("pill.moreHint"), icon: "more" };
        }
        if (id === PILL_BACK_ID) {
          return {
            id,
            label: t("pill.wheelBack"),
            short: t("pill.backHint"),
            icon: "back",
          };
        }
        if (id === PILL_WINDOW_ID) {
          return {
            id,
            label: t("pill.openMain"),
            short: t("pill.openMainHint"),
            icon: "window",
          };
        }
        if (id === PILL_CUSTOMIZE_ID) {
          return {
            id,
            label: t("pill.customize"),
            short: t("pill.customizeHint"),
            icon: "customize",
          };
        }
        const tool = localizeTool(toolById(id));
        return { id, label: tool.label, short: tool.short, icon: id };
      }),
    );

  type WheelNode = { id: PillWheelId; label: string; short: string; icon?: IconId };

  const moreNode: WheelNode = $derived({
    id: PILL_MORE_ID,
    label: t("pill.more"),
    short: t("pill.moreHint"),
    icon: "more",
  });
  const windowNode: WheelNode = $derived({
    id: PILL_WINDOW_ID,
    label: t("pill.openMain"),
    short: t("pill.openMainHint"),
    icon: "window",
  });
  const ringNodes: WheelNode[] = $derived([...layout.ring.map(localizeTool), moreNode]);
  const wheelNodes: WheelNode[] = $derived(
    wheelPage === "more" ? [...layout.more.map(localizeTool), windowNode] : ringNodes,
  );

  // Los adjuntadores se crean UNA vez sobre el catálogo completo y se indexan
  // por posición: la tira muestra un subconjunto, así que el índice siempre
  // cae dentro. Derivarlos de la lista visible les cambiaría la identidad con
  // cada preferencia, y un `@attach` que cambia de función se desmonta y da de
  // baja su rect —justo lo que el comentario de arriba está evitando.
  const islandAttachers = Array.from(
    // Dos más: «Más»/«atrás» y Ventana, que en el canto va en el primer paso.
    { length: WHEEL_TOOLS.length + 2 },
    (_, i) => (el: HTMLElement) => tracker.track(`island-${i}`, el),
  );
  /**
   * Gota 0 = núcleo, 1…n = gajos, n+1 = viva. Holgura para el submenú.
   * Mismas funciones estables que la tira: un `@attach` nuevo desmonta el rect.
   */
  const wheelAttachers = Array.from(
    { length: WHEEL_TOOLS.length + 4 },
    (_, i) => (el: HTMLElement) => tracker.track(`wheel-${i}`, el),
  );
  function attachWheelBlob(index: number) {
    return wheelAttachers[index] ?? wheelAttachers[0]!;
  }

  /** Margen tras la coreografía, por si el último cuadro llega tarde. */
  const ISLAND_SETTLE_MS = 120;

  /**
   * Mantener despierto al tracker durante TODA la transición de la isla
   * y del bloom de la rueda.
   *
   * `RectTracker` deja de mirar tras 3 cuadros sin cambios, y su propio
   * comentario avisa del riesgo: «una transición puede tener un cuadro sin
   * cambio visible en el medio». El escalonado mete varios de esos cuadros
   * —isla o rueda—, así que se dormía a mitad de camino y la silueta quedaba
   * congelada donde la hubiera agarrado.
   *
   * `wake()` es idempotente y solo reinicia el contador de quietud, así que
   * bombearlo por cuadro durante el tramo cuesta nada y garantiza que la
   * silueta siga la animación hasta el final.
   */
  $effect(() => {
    void dock;
    void surface;
    void recording;
    void wheelShown;
    void tailIn;
    void flying;
    void bootHidden;
    void birthing;
    void agentFaceOpen;
    void seating;
    void islandFace;
    void box.w;
    void box.h;
    const blooming = wheelChrome;
    if (
      surface !== "edge" &&
      !beadsAlive &&
      !blooming &&
      !flying &&
      !birthing &&
      !seating
    )
      return;
    let raf = 0;
    const bloomMs =
      ms(MOTION.morphOpen) + (WHEEL_TOOLS.length + 1) * ms(MOTION.morphStagger);
    const until =
      performance.now() +
      Math.max(
        blooming ? bloomMs : flying ? ms(MOTION.flight) : ms(MOTION.islandOpen),
        seating ? ms(MOTION.fast) : 0,
      ) +
      ISLAND_SETTLE_MS;
    const pump = () => {
      tracker.wake();
      raf = performance.now() < until ? requestAnimationFrame(pump) : 0;
    };
    pump();
    return () => {
      if (raf) cancelAnimationFrame(raf);
    };
  });

  // Cada cambio de estado arranca una animación de CSS: hay que volver a mirar.
  // La posición también cuenta: un vuelo no toca los estados de arriba, y sin
  // `at` la silueta quedaba en el sitio del que la pill se fue.
  $effect(() => {
    void surface;
    void discOnly;
    void barW;
    void at.x;
    void at.y;
    // Abrir y cerrar la isla no toca `surface`: cambia `dock`, y de ahí sale la
    // transición más larga de todas. Sin declararlo, que el tracker despertara
    // dependía de que el reencuadre moviera `at` en el mismo tick —cierto hoy,
    // pero por casualidad—.
    void dock;
    void beadsAlive;
    void recording;
    void wheelShown;
    void bootHidden;
    void birthing;
    void agentFaceOpen;
    void seating;
    void islandFace;
    void box.w;
    void box.h;
    tracker.wake(true);
  });

  onMount(() => () => tracker.stop());

  /**
   * Las work areas viven en una closure del escenario (TS puro, sin `$state`):
   * leerlas dentro de un `$derived` no crea dependencia reactiva. La época
   * sube cada vez que `loadAreas()` recarga y despierta los deriveds de abajo.
   */
  let areasEpoch = $state(0);
  $effect(() => stage.onAreasChanged(() => (areasEpoch += 1)));

  /**
   * Las siluetas medidas, como formas del campo.
   *
   * Los radios se repiten acá porque el campo los necesita como número y el
   * CSS los declara como estilo. Son los mismos dos de siempre: la barra y la
   * gota son pastillas.
   */
  const skinShapes = $derived.by(() => {
    // Con el chrome de la rueda activo (abierta o colapsando) la silueta
    // son las gotas medidas de ParticleWheel, en el mismo campo SDF que el
    // resto del overlay. Publicar la barra en ese tramo deja un disco
    // fantasma arriba-izquierda: el stack vive anclado al top-left del root,
    // no al centro donde está la marca de la rueda.
    if (wheelChrome) {
      const r = tracker.rects;
      const o = tracker.originAt;
      const at = (rect: Rect): Rect => ({
        ...rect,
        x: rect.x + o.x,
        y: rect.y + o.y,
      });
      const shapes = [];
      for (const [id, rect] of Object.entries(r)) {
        if (!id.startsWith("wheel-") || !rect) continue;
        shapes.push(pillShape(at(rect)));
      }
      return shapes;
    }
    const r = tracker.rects;
    // A coordenadas del overlay: el grupo mezcla las formas de la pill con las
    // de la consola, y solo son comparables en un origen común.
    const o = tracker.originAt;
    const at = (rect: Rect): Rect => ({
      ...rect,
      x: rect.x + o.x,
      y: rect.y + o.y,
    });
    // Las work areas pueden llegar después del primer paint.
    void areasEpoch;
    const wallFor = (blob?: Rect) => {
      if (!blob) return [];
      const pill = at(blob);
      const areas = stage.workAreas();
      if (areas.length === 0) return [];
      // Acoplada: el canto del dock manda. Si hay otro canto cerca (esquina),
      // `edgeWallsFor` emite las dos paredes y el dintel llega al vértice.
      // Sin acople, el techo gana el empate para que el menisco no tire a un
      // costado cuando está igual de pegada arriba que a la derecha.
      return edgeWallsFor(pill, areas, {
        maxGap: INFLUENCE,
        prefer: surface === "edge" && dock ? dock.edge : null,
      }).map(pillShape);
    };
    // Acoplada: la silueta es la isla, en los dos estados.
    //
    // Se mide un elemento propio que llena la caja, así que sigue la
    // transición de tamaño en vez de saltar. Y deja fuera la barra normal:
    // publicarla dibujaba el disco de reposo sobre una zona viva de pestaña
    // —la pill se veía sin cambiar y no respondía, porque lo que recibe el
    // puntero es la caja y no lo dibujado—.
    if (surface === "edge") {
      const shapes = [];
      // `notchShape` mete el radio del canto en el bisel y los lados bajan
      // derechos; la pared de abajo le devuelve el menisco del contacto con
      // un filete medido —uno ancho era más ancho que la isla y el smin lo
      // convertía en alas—.
      if (r.island) {
        let box = at(r.island);
        // El rebote de `--ease-island` se pasa del dintel; si el tracker
        // se duerme en ese valle, la pestaña queda cortada. El aplastón
        // de asiento sí puede encoger un instante: no se pisa acá.
        if (dock && islandFace === "tab" && !dock.expanded && !seating) {
          box = clampDockedTabRect(box, dock.edge, faceTabH);
        }
        shapes.push(
          dock ? notchShape(box, dock.edge, islandNotchRadius(box)) : pillShape(box),
        );
        // Menisco: la isla se funde al canto como la pill suelta que llega
        // al borde. `box` ya viene en coords del overlay: nada de `at()`.
        if (dock) {
          shapes.push(
            ...edgeWallsFor(box, stage.workAreas(), {
              maxGap: INFLUENCE,
              prefer: dock.edge,
              flare: meniscusFlare,
            }).map(pillShape),
          );
        }
      }
      // Avisos: misma tinta, otro bulto. El smin los funde con la pestaña
      // en vez de pintar una cápsula encima.
      if (r["island-cue"]) shapes.push(pillShape(at(r["island-cue"])));
      if (r["island-update-cue"]) shapes.push(pillShape(at(r["island-update-cue"])));
      return shapes;
    }

    const shapes = [];
    // Recién despegada del canto: el cuerpo de la isla sigue publicado unos
    // cuadros y para entonces ya encogió al tamaño de la pill, así que se funde
    // con el disco en vez de desaparecer de golpe. Las dos formas en el mismo
    // campo es lo que hace que se FUNDAN y no que una reemplace a la otra.
    if (beadsAlive && r.island) shapes.push(pillShape(at(r.island)));
    // La gota, si está. El disco solo mientras la gota no lo cubra: ver
    // `discJoinsTail` — publicar ambos en reposo engordaba el lado izquierdo.
    if (r.tail) shapes.push(pillShape(at(r.tail)));
    // Dictado flotante: la gota de la onda cuelga del disco. Con el blend de
    // render en 0 el campo no filetea el hueco, así que el cuello es una forma
    // más (cápsula del hilo, como el globo de agentes): sin ella se verían dos
    // pastillas sueltas.
    if (r.bar && r.drop) {
      const neck = stemBetween(at(r.bar), at(r.drop), "top", PILL.recDropNeck);
      if (neck) shapes.push(neck);
    }
    if (r.drop) shapes.push(pillShape(at(r.drop)));
    if (r.bar && (!r.tail || discJoinsTail(r.bar, r.tail))) {
      shapes.push(pillShape(at(r.bar)));
    }
    shapes.push(...wallFor(r.island ?? r.bar));
    return shapes;
  });

  /**
   * La pill no traza su contorno: lo publica.
   *
   * Quien traza es el overlay, con TODO el grupo en un solo campo. Es lo único
   * que hace que el cuello hacia la consola exista sin dibujarlo: dos campos
   * separados no se pueden fundir por definición.
   */
  $effect(() => {
    // Oculta en boot: el SDF es una capa aparte y el `opacity: 0` del DOM no
    // lo tapa. Sin este gate el blob se vería antes que la gota.
    if (bootHidden) {
      liquid.publish("pill", []);
      return;
    }
    liquid.publish("pill", skinShapes);
  });

  /**
   * La piel respira: grabando/dictando y también mientras un agente trabaja.
   *
   * El latido sale de la silueta (Skin), no de un adorno encima: el aviso
   * «estoy trabajando» nace de la misma piel que el resto de la pill.
   */
  $effect(() => {
    liquid.breathe = recording || dictation === "listening" || agentWorking;
  });

  /**
   * El hogar: donde el usuario la acopló por última vez (`chosenHome`), o el
   * centro de arriba del monitor principal si nunca la acopló o ese monitor
   * ya no está. Arrastrar la deja donde cae; abrir una herramienta o cerrar
   * un float la devuelve acá.
   */
  let home = $state({ x: 0, y: 0 });
  /** Canto del hogar vigente. */
  let homeEdge: DockEdge = "top";
  /** El hogar vigente es el elegido (no el de por defecto). */
  let homeChosen = false;
  /**
   * Lo elige el usuario al SOLTARLA contra un canto. Dejarla flotando no lo
   * cambia: un arrastre de paso no es mudarse.
   */
  let chosenHome: PillHome | null = readPillHome();

  function restSize() {
    return windowFor({ w: PILL.bar, h: PILL.bar });
  }

  /** Tamaño de la isla en reposo en ese canto: el hogar se centra con ESTO. */
  function notchHomeSize(edge: DockEdge = "top") {
    return windowFor(contentFor("edge", barW, { edge, expanded: false }));
  }

  function refreshDefaultHome() {
    const areas = stage.workAreas();
    const chosen = chosenHome
      ? pillHomePoint(chosenHome, notchHomeSize(chosenHome.edge), areas)
      : null;
    const dest = chosen ?? defaultPillHome(notchHomeSize(), areas);
    if (!dest) return;
    home = dest.at;
    homeEdge = dest.edge;
    homeChosen = chosen !== null;
  }

  /** La soltaron contra un canto: ese pasa a ser su hogar. */
  function rememberHome(edge: DockEdge, size: { w: number; h: number }): void {
    const next = pillHomeFrom(edge, { x: at.x, y: at.y, w: size.w, h: size.h }, stage.workAreas());
    if (!next) return;
    chosenHome = next;
    writePillHome(next);
    refreshDefaultHome();
  }

  /** Ajustes → Pill: olvidar el hogar elegido y volver al de por defecto. */
  async function resetHome(): Promise<void> {
    chosenHome = null;
    clearPillHome();
    await goDefaultHome();
  }

  function atDefaultHome(px = 8): boolean {
    return Math.hypot(home.x - at.x, home.y - at.y) < px;
  }

  async function goDefaultHome(): Promise<void> {
    refreshDefaultHome();
    if (atDefaultHome(2) && surface === "edge" && dock?.edge === homeEdge) return;
    if (Math.hypot(home.x - at.x, home.y - at.y) >= 2) {
      await flyTo(home);
    } else {
      stage.moveTo(home);
      at = stage.at();
    }
    dock = { edge: homeEdge, expanded: false };
    surface = "edge";
    settleDock({ keep: homeChosen });
  }
  /** El vuelo lo hace una transición CSS; esto la enciende solo cuando toca. */
  let flying = $state(false);
  /**
   * Inverso FLIP del vuelo: `left`/`top` ya están en el destino y esto
   * mantiene el dibujo en el origen hasta que `transform` llega a 0.
   * Animar `left`/`top` forzaba layout en cada cuadro; `translate3d` va en GPU.
   */
  let flightLift = $state<{ x: number; y: number } | null>(null);
  /** Generación del vuelo: Esc/reabrir invalidan un `flyTo` en curso. */
  let flightEpoch = 0;
  /** Cambio de anillo (Más / atrás): no comparte epoch con el cierre. */
  let wheelPageEpoch = 0;
  /** Desde qué canto brotó la rueda. Null = morph al centro (summon). */
  let wheelBloomEdge = $state<DockEdge | null>(null);
  /** Compresión de 2–3 px contra el muro al acoplarse. */
  let seating = $state(false);
  let seatingTimer = 0;
  /**
   * Nacimiento desde el centro (boot): la pill no viaja desde la esquina,
   * brota como gota en su hogar. `bootHidden` la mantiene fuera (DOM + SDF)
   * mientras se asienta la geometría; `birthing` anima solo transform/opacity.
   */
  let bootHidden = $state(true);
  let birthing = $state(false);
  let birthTimer = 0;

  function cancelFlight() {
    flightEpoch += 1;
    flying = false;
    flightLift = null;
  }

  function visualOrigin(): { x: number; y: number } {
    const r = rootEl?.getBoundingClientRect();
    return r ? { x: r.left, y: r.top } : stage.at();
  }

  /**
   * Mueve la pill con transición CSS hasta `p`.
   *
   * FLIP: primero se escribe el destino en `left`/`top` (sin transición) y un
   * `translate3d` deja el dibujo donde estaba; después se anima ese translate
   * a 0. Hay que pintar el inverso *antes* de activar `.is-flying`: si clase y
   * destino llegan juntos, el navegador salta sin animar.
   *
   * Un vuelo que pierde la epoch NO apaga `flying`: el que lo invalidó es el
   * nuevo y necesita la clase. Devuelve la duración usada, o `-1` si se canceló.
   */
  async function flyTo(
    p: { x: number; y: number },
    opts: { skipIfNear?: number } = {},
  ): Promise<number> {
    const epoch = ++flightEpoch;
    const from = visualOrigin();
    const skip = opts.skipIfNear ?? 0;
    const dur = ms(MOTION.flight);

    stage.moveTo(p);
    const dest = stage.at();
    const dx = from.x - dest.x;
    const dy = from.y - dest.y;
    const dist = Math.hypot(dx, dy);

    if (dist < skip || dur <= 0) {
      flying = false;
      flightLift = null;
      at = dest;
      surfaces.schedule();
      return 0;
    }

    // Sin transición: clavar el inverso para que el salto de left/top no se vea.
    flying = false;
    flightLift = { x: dx, y: dy };
    at = dest;
    await tick();
    if (epoch !== flightEpoch) return -1;
    void rootEl?.offsetWidth;

    flying = true;
    await tick();
    if (epoch !== flightEpoch) return -1;
    void rootEl?.offsetWidth;

    flightLift = { x: 0, y: 0 };
    await afterTransition(rootEl, "transform", dur);

    if (epoch !== flightEpoch) return -1;
    flying = false;
    flightLift = null;
    // Republicar al aterrizar: el hit-rect usa left/top de layout, no el
    // translate visual. Sin esto Rust arma el sitio de origen.
    surfaces.schedule();
    return dur;
  }

  /** Dónde está el puntero, en coordenadas del overlay. */
  async function cursorPoint(): Promise<{ x: number; y: number } | null> {
    try {
      return await overlayCursor();
    } catch {
      return null;
    }
  }

  /** Ancho real de la barra, medido del DOM. Sin esto habría que mantener una
   *  tabla de anchos mágicos por estado — la fuente original del desajuste. */
  let barW = $state<number>(PILL.bar);
  let barEl = $state<HTMLElement | null>(null);
  // `HTMLElement` y no `HTMLButtonElement`: sin nada que abrir, el aviso se
  // renderiza como `<span>`. Sigue midiéndose igual para el ancla del float.
  let agentDockEl = $state<HTMLElement | null>(null);
  let agentStackEl = $state<HTMLElement | null>(null);

  /**
   * Acoplada a un borde. `null` = flotando (centro de pantalla u otro imán).
   *
   * El hogar canónico es el canto de arriba del monitor principal; no se
   * restaura desde `pill_home`. Los imanes de `snapMagnet` deciden el resto.
   */
  let dock = $state<Dock | null>(null);

  /**
   * Cara `agent` de la isla: el permiso pendiente vive en la tarjeta cuando
   * está acoplada (flotando ya tiene la suya: `showAuthCard`).
   *
   * Auto-abre con un pedido nuevo —bloquea al agente— y no re-abre el que se
   * colapsó a mano (clic afuera / Esc): el cue sigue pulsando para volver.
   */
  const agentFaceReq = $derived(
    authRequest && surface === "edge" && !agentsConsoleOpen ? authRequest : null,
  );
  let agentFaceDismissed = $state<string | null>(null);
  /** Pedido explícito de panel en la isla (clipboard, textos, agentes). */
  let toolFace = $state<"tab" | "clipboard" | "snippets" | "system" | "agents" | "customize">(
    "tab",
  );
  let snippetsTab = $state<"list" | "scratchpad">("list");
  const islandFace: IslandFace = $derived.by(() => {
    if (
      islandFaceAgent({
        surface,
        dock,
        authId: agentFaceReq?.permission.id ?? null,
        dismissedAuthId: agentFaceDismissed,
      })
    ) {
      return "agent";
    }
    if (islandFaceDictation({ surface, dock, dictating })) {
      return "dictation";
    }
    if (islandFacePanel({ surface, dock, requested: toolFace !== "tab" })) {
      return toolFace;
    }
    if (islandFaceLive({ surface, dock, live: chips.length > 0 })) {
      return "live";
    }
    return "tab";
  });
  const agentFaceOpen = $derived(islandFace === "agent");
  const dictationFaceOpen = $derived(islandFace === "dictation");
  const liveFaceOpen = $derived(islandFace === "live");
  const clipboardFaceOpen = $derived(islandFace === "clipboard");
  const snippetsFaceOpen = $derived(islandFace === "snippets");
  const systemFaceOpen = $derived(islandFace === "system");
  const agentsFaceOpen = $derived(islandFace === "agents");
  const customizeFaceOpen = $derived(islandFace === "customize");
  /** Ficha desde la que se abrió el editor (presión larga o clic derecho). */
  let customizeFocus = $state<ToolId | null>(null);
  let agentsMounted = $state(false);
  let agentsLive = $state(false);
  let agentsConsoleView = $state(false);
  let agentsBrowserOpen = $state(false);
  /** Panel que convive CON la consola, dentro de la isla (clipboard/textos). */
  let sidePanel = $state<"clipboard" | "snippets" | null>(null);
  /**
   * El layout con panel sobrevive al cierre: mientras la caja encoge, el
   * pivote tiene que seguir clavando el lado de la consola (si no, salta media
   * diferencia). Se suelta solo, pasado el tramo de `--island-open-dur`.
   */
  let sideHold = $state(false);
  /** Hay panel abierto (o su layout todavía aplicado durante el cierre). */
  const sideLayout = $derived(sidePanel !== null || sideHold);
  /** Ese layout se pinta solo con la cara de la consola a la vista. */
  const sideApplied = $derived(sideLayout && agentsFaceOpen);
  /**
   * La caja OBJETIVO ya suma el panel. No es lo mismo que el layout: al cerrar
   * la caja encoge de inmediato (sin panel) mientras el layout sigue aplicado
   * un tramo. El pivote usa el layout; la unidad del recentrado, esto.
   */
  const sideInBox = $derived(sidePanel !== null);
  const faceOpen = $derived(islandFace !== "tab");

  /*
   * El reproductor: una celda más de la tira, solo mientras algo suena o está
   * en pausa. El clic pausa o reanuda; el resto (saltos, qué suena) está en
   * su vistazo. Se sondea solo acoplada: es el único lugar donde se muestra.
   */
  $effect(() => {
    if (surface !== "edge") return;
    return media.watch();
  });
  const mediaCell = $derived(surface === "edge" ? media.now : null);
  const mediaLabel = $derived(
    mediaCell
      ? mediaCell.playing
        ? t("pill.peek.mediaPause")
        : t("pill.peek.mediaPlay")
      : "",
  );

  /*
   * El vistazo, dentro de la tira.
   *
   * Con la tira acoplada abierta, la isla se transforma en el vistazo en vez
   * de sacar un panel al lado (ver `IslandPeek`). Flotando o con la rueda
   * sigue el panel de `PillPeekHost`: ahí no hay isla que crezca.
   */
  const peekInIsland = $derived(
    surface === "edge" && dock?.expanded === true && islandFace === "tab",
  );
  $effect(() => {
    toolPeekState.inIsland = peekInIsland;
  });
  const islandPeekTool = $derived(
    peekInIsland && toolPeekState.open ? toolPeekState.tool : null,
  );
  /** Medida del vistazo ya pintado. Hasta tenerla, la isla no crece. */
  let islandPeekSize = $state<{ w: number; h: number } | null>(null);
  const islandPeekBox = $derived(islandPeekTool ? islandPeekSize : null);
  /**
   * El anclaje de la tira al canto sobrevive al cierre del vistazo: la caja
   * encoge durante `--island-open-dur` y, soltado de golpe, la tira se
   * centraba en la caja todavía alta —los iconos caían al fondo y subían con
   * el encogimiento—. Mismo patrón que `sideHold`.
   */
  let peekHold = $state(false);
  $effect(() => {
    if (islandPeekBox != null) {
      peekHold = true;
      return;
    }
    if (!peekHold) return;
    const timer = setTimeout(() => (peekHold = false), ms(MOTION.islandOpen));
    return () => clearTimeout(timer);
  });

  /**
   * Hay una consola viva en la isla: a la vista, tapada por otra cara
   * (clipboard / textos) o achicada en el dock.
   *
   * Alcanza para que el pegado del historial entre a la sesión en vez de
   * salir a la app de atrás, y para no cerrarla al abrir otra herramienta.
   */
  function islandConsoleAlive(): boolean {
    return agentsMounted && agentsLive;
  }

  /** La cara de la consola está abierta a la vista: el panel va al costado. */
  function consoleFaceShowing(): boolean {
    return islandFace === "agents" && agentsConsoleView && agentsLive;
  }

  // El puente tiene que enterarse de la consola de la isla (no tiene ventana
  // propia): sin esto, pegar desde el historial salía a la app de atrás.
  $effect(() => {
    const alive = islandConsoleAlive();
    if (!alive || !agentsConsoleView) sidePanel = null;
    agentsConsoleOpen = alive;
    void setAgentsConsoleOpen(alive).catch(() => {});
  });

  /**
   * Un traspaso dirigido al overlay necesita la cara montada.
   *
   * El lanzador es quien adopta y ackea: sin cara, el traspaso se queda en el
   * buzón y la emisora aborta a los 4 s. Pasa al volver de la ventana dedicada
   * si la isla quedó en setup (sin fichas montadas) o tras una recarga. Se
   * abre igual que con el atajo; el lanzador consume el buzón al montar.
   */
  $effect(() => {
    const inbox = transferInbox.current;
    if (!inbox || inbox.to !== OVERLAY_LABEL) return;
    if (agentsMounted && agentsFaceOpen) return;
    void openIslandTool("agents");
  });

  /**
   * Suelta el layout con panel recién cuando la caja terminó de encoger.
   *
   * Abrir y cerrar son simétricos: durante el tramo de cierre el pivote tiene
   * que seguir clavando el borde del lado de la consola; sin eso, el pivote
   * del canto recentraría una caja todavía ancha y la consola saltaría media
   * diferencia. Con el cierre total (dismiss) no hay nada que sostener: la
   * pestaña vuelve al centro como siempre.
   */
  $effect(() => {
    if (sidePanel !== null && consoleFaceShowing()) {
      sideHold = true;
      return;
    }
    if (!sideHold) return;
    if (!agentsFaceOpen) {
      sideHold = false;
      return;
    }
    const timer = setTimeout(() => (sideHold = false), ms(MOTION.islandOpen));
    return () => clearTimeout(timer);
  });

  $effect(() => {
    if (!authRequest) agentFaceDismissed = null;
  });
  $effect(() => {
    // Fuera del canto no hay cara ni panel: si el flag quedara colgado, al
    // reabrir la consola el panel reaparecía solo y el primer atajo de
    // clipboard "cerraba" algo invisible. Cubre la rueda y el desacople.
    if (surface !== "edge") {
      toolFace = "tab";
      sidePanel = null;
    }
  });
  /** Colapso manual: decidir, consola, clic afuera y Esc pasan por acá. */
  function dismissAgentFace(): void {
    agentFaceDismissed = agentFaceReq?.permission.id ?? agentFaceDismissed;
  }
  let islandFaceLockUntil = 0;
  let clipFaceEl = $state<HTMLElement | null>(null);
  /** Filas de avisos: sobre ellas el hover no abre la tira (ver el sondeo). */
  let liveListEl = $state<HTMLElement | null>(null);
  /**
   * La cara de agentes montada, para publicarla como hit-rect `agents`.
   *
   * No alcanza con el chip del dock: durante un arrastre OLE Rust poda los
   * hit-rects a los drop-targets y la consola ABIERTA en la isla también tiene
   * que estar en la lista (ver el efecto de registro).
   */
  let agentsFaceEl = $state<HTMLElement | null>(null);

  /**
   * El elemento de la cara `id`.
   *
   * Las caras comparten `clipFaceEl`: al desmontarse la del historial (o
   * textos / sistema) queda en null, y la de agentes —montada siempre— no
   * vuelve a enlazarse. El despegue medía null, nacía sin reposo y el float
   * crecía desde la semilla de la esquina, lejos de la mano.
   */
  function faceElFor(id: "clipboard" | "snippets" | "system" | "agents") {
    return id === "agents" ? agentsFaceEl : clipFaceEl;
  }

  /**
   * Teclado de la cara abierta (Windows).
   *
   * La ventana del overlay nace `focusable: false` —`WS_EX_NOACTIVATE`— para no
   * robarle el foco a la app donde escribes. El precio es que **tampoco recibe
   * teclas**: hasta hoy el `keydown` de Esc solo llegaba después de tocar un
   * campo de texto, que es lo único que pide `set_overlay_text_mode`. Con esto
   * se pide al abrir la cara y se devuelve al cerrarla, así Esc cierra sin
   * tener que clicar un input primero.
   */
  const faceKeysOpen = $derived(
    clipboardFaceOpen ||
      snippetsFaceOpen ||
      systemFaceOpen ||
      agentsFaceOpen ||
      customizeFaceOpen ||
      sidePanel !== null,
  );
  /** Ya pedimos el teclado por esta cara: evita repetir el viaje a Rust. */
  let faceKeysHeld = false;
  $effect(() => {
    if (isMac || faceKeysOpen === faceKeysHeld) return;
    faceKeysHeld = faceKeysOpen;
    if (faceKeysOpen) {
      void setOverlayTextMode(true).catch(() => {});
      return;
    }
    releaseFaceKeys();
  });
  // Se desmonta la pill con una cara abierta: nadie baja el modo texto y la
  // app de abajo se queda sin teclado.
  $effect(() => () => releaseFaceKeys());

  /**
   * Devuelve el teclado al cerrar la cara.
   *
   * Salvo que el foco esté en un campo de verdad: ahí el modo texto lo sigue
   * necesitando y lo gobierna `OverlaySurface`. Se le avisa con el mismo evento
   * que él ya escucha, para que no quede creyendo que el teclado sigue pedido
   * —si quedara así, al clicar el campo la próxima vez no lo repondría y la
   * consola quedaría muda.
   */
  function releaseFaceKeys(): void {
    if (isMac) return;
    const ae = document.activeElement;
    const typing =
      ae instanceof HTMLElement &&
      ae.closest(
        "input, textarea, [contenteditable='true'], [data-console-term], .xterm",
      ) !== null;
    if (typing) return;
    window.dispatchEvent(new Event("atic-overlay-leave-text"));
    void setOverlayTextMode(false).catch(() => {});
  }

  function dismissToolFace(): void {
    sidePanel = null;
    const was = toolFace;
    toolFace = "tab";
    if (was !== "tab" && spatialIntent === was) spatialIntent = null;
    islandFaceLockUntil = performance.now() + ISLAND_COLLAPSE_MS;
    setIslandExpanded(false);
    // Durante el despegue el botón sigue apretado. Bajar el gesto acá desarma
    // el overlay en Windows: el cursor ya no está sobre la cara, el puntero
    // se pierde y el panel nace pegado al notch.
    if (!detachGesture) {
      void setOverlayPointerGesture(false).catch(() => {});
    }
    if (was === "agents" && agentsLive) agentsDock.setMinimized(true);
  }

  /**
   * Despegar la cara: el panel sale de la isla y queda flotando.
   *
   * El rect de la cara es el ancla de nacimiento (`toolBirth`): el float nace
   * fundido ahí y descansa al lado, en vez de saltar a la pill. El pin ("no
   * se cierra con clic afuera") y la X ya vienen de fábrica en el float; el
   * cierre por clic afuera lo maneja el float, como con cualquier otra
   * apertura por atajo.
   *
   * Agentes no entra: su consola vive en la isla y mudarla pasa por el
   * traspaso de consola, no por este botón.
   */
  async function detachToolFace(): Promise<void> {
    const id = toolFace;
    if (id !== "clipboard" && id !== "snippets" && id !== "system" && id !== "agents")
      return;
    const r = faceElFor(id)?.getBoundingClientRect();
    if (r && r.width > 0 && r.height > 0) {
      const rect = { x: r.x, y: r.y, w: r.width, h: r.height };
      captureToolBirth(rect);
      // El reposo EXACTO es el rect de la cara: el float nace en el sitio
      // del panel, no al lado.
      captureToolResting(rect);
      void waitToolResting().then(() => captureToolBirth(null));
    }
    dismissToolFace();
    armIslandBounce();
    if (id === "clipboard") await showClipboardWindow().catch(() => {});
    else if (id === "snippets") await showSnippetsWindow().catch(() => {});
    else if (id === "system") await showSystemWindow().catch(() => {});
    else await detachAgentsFloat();
  }

  /**
   * Gesto del grab: arrastre y clic comparten la misma barrita.
   *
   * Poca distancia al soltar = clic: despega en el sitio (el camino de
   * siempre). Arrastre real = despegar YA mientras sigue la mano: la cara
   * colapsa, el float nace del rect de la cara y, cuando descansa, le
   * traspasamos el gesto vivo a su header para que siga el cursor hasta el
   * soltar. El notch (la banda) no se mueve en ninguno de los dos.
   *
   * El traspaso no necesita eventos DOM confiables: `startDrag` del float
   * siembra su origen con el cursor de Rust en el primer tick, y su lazo
   * sigue `overlayCursor`. El pointerup del usuario lo recogen los
   * oyentes de window (capture) que el mismo float instala.
   */
  const DETACH_DRAG_THRESHOLD = 10;
  let detachGesture: {
    pointerId: number;
    startX: number;
    startY: number;
    /** Última posición de la mano, para sembrar el arrastre del float. */
    lastX: number;
    lastY: number;
    detached: boolean;
  } | null = null;
  /** El float ya tomó el puntero: el pointerup no debe soltar el armado. */
  let detachHandoff = false;

  function startDetachGesture(event: PointerEvent): void {
    if (event.button !== 0 || detachGesture) return;
    detachHandoff = false;
    detachGesture = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      lastX: event.clientX,
      lastY: event.clientY,
      detached: false,
    };
    window.addEventListener("pointermove", onDetachPointerMove, true);
    window.addEventListener("pointerup", onDetachPointerUp, true);
    window.addEventListener("pointercancel", onDetachPointerCancel, true);
    event.preventDefault();
    // La cara hace stopPropagation, así que el arrastre de la pill no arma
    // el overlay. En Windows, sin esto, colapsar la cara deja el cursor
    // fuera de los hit-rects y el overlay pasa a click-through a mitad del
    // gesto: el panel aparece en el sitio y no sigue la mano. El primero
    // tarda más (el globo todavía no existe) y es el que se queda pegado.
    // Hay que armarlo con el botón todavía apretado; si el flag ya estaba
    // levantado sin botón (apertura de la cara), repetir el aviso lo sella.
    surfaces.dragging = true;
    void setOverlayPointerGesture(true).catch(() => {});
  }

  function releaseDetachArm(): void {
    if (detachHandoff) return;
    detachHandoff = false;
    surfaces.dragging = false;
  }

  function endDetachGesture(): typeof detachGesture {
    const g = detachGesture;
    detachGesture = null;
    window.removeEventListener("pointermove", onDetachPointerMove, true);
    window.removeEventListener("pointerup", onDetachPointerUp, true);
    window.removeEventListener("pointercancel", onDetachPointerCancel, true);
    return g;
  }

  function onDetachPointerMove(event: PointerEvent): void {
    const g = detachGesture;
    if (!g || event.pointerId !== g.pointerId) return;
    g.lastX = event.clientX;
    g.lastY = event.clientY;
    if (g.detached) return;
    const dx = event.clientX - g.startX;
    const dy = event.clientY - g.startY;
    if (Math.hypot(dx, dy) < DETACH_DRAG_THRESHOLD) return;
    g.detached = true;
    void beginDetachDrag();
  }

  function onDetachPointerUp(event: PointerEvent): void {
    const g = endDetachGesture();
    if (!g || event.pointerId !== g.pointerId) return;
    // Si el float ya escuchaba, su pointerup cierra el arrastre. Si no
    // alcanzó a nacer, soltar el armado: si no, el hit-rect queda a
    // pantalla completa.
    releaseDetachArm();
    // Soltar sin arrastre = clic: despega en el sitio.
    if (!g.detached) void detachToolFace();
  }

  function onDetachPointerCancel(event: PointerEvent): void {
    const g = endDetachGesture();
    if (!g || event.pointerId !== g.pointerId) return;
    releaseDetachArm();
    if (!g.detached) void detachToolFace();
  }

  async function beginDetachDrag(): Promise<void> {
    const id = toolFace;
    if (id !== "clipboard" && id !== "snippets" && id !== "system" && id !== "agents")
      return;
    const r = faceElFor(id)?.getBoundingClientRect();
    if (r && r.width > 0 && r.height > 0) {
      const rect = { x: r.x, y: r.y, w: r.width, h: r.height };
      captureToolBirth(rect);
      captureToolResting(rect);
      void waitToolResting().then(() => captureToolBirth(null));
    }
    dismissToolFace();
    armIslandBounce();
    if (id === "clipboard") await showClipboardWindow().catch(() => {});
    else if (id === "snippets") await showSnippetsWindow().catch(() => {});
    else if (id === "system") await showSystemWindow().catch(() => {});
    else await detachAgentsFloat();
    // El gesto NO se suelta: apenas el header del float existe, el arrastre
    // pasa a él y sigue pegado a la mano. Agentes no espera al header (su
    // barra monta tras adoptar): traspasa directo al motor del float, que se
    // registra al montar — el panel sigue a la mano mientras el contenido
    // llega en segundo plano. Se sondea mientras el gesto siga vivo. La
    // posición es la REAL de la mano: el drag arranca moviéndose al instante.
    const gesture = detachGesture;
    if (!gesture || !gesture.detached) return;
    if (id === "agents") {
      for (let i = 0; i < 600; i++) {
        const live = detachGesture;
        if (!live || !live.detached || live.pointerId !== gesture.pointerId) return;
        const handoff = agentsFloatHandoff.current;
        if (
          handoff?.({ pointerId: gesture.pointerId, x: live.lastX, y: live.lastY })
        ) {
          detachHandoff = true;
          return;
        }
        await new Promise<void>((r) => requestAnimationFrame(() => r()));
      }
      return;
    }
    for (let i = 0; i < 600; i++) {
      const live = detachGesture;
      if (!live || !live.detached || live.pointerId !== gesture.pointerId) return;
      if (id !== "clipboard" && id !== "snippets" && id !== "system") return;
      const head = detachFloatHeader(id);
      if (head) {
        head.dispatchEvent(
          new PointerEvent("pointerdown", {
            bubbles: true,
            button: 0,
            pointerId: gesture.pointerId,
            pointerType: "mouse",
            isPrimary: true,
            clientX: live.lastX,
            clientY: live.lastY,
          }),
        );
        return;
      }
      await new Promise<void>((r) => requestAnimationFrame(() => r()));
    }
  }

  /** Header del float ya montado: ahí vive su `startDrag`. */
  function detachFloatHeader(
    id: "clipboard" | "snippets" | "system",
  ): HTMLElement | null {
    const sel =
      id === "clipboard"
        ? '[data-float="clipboard"] .cf-head'
        : id === "snippets"
          ? '[data-float="snippets"] .sf-head'
          : '[data-float="system"] .sys-head';
    return document.querySelector(sel);
  }

  /**
   * Abrir el float de agentes en el rect de la cara y mudar las consolas.
   *
   * La isla deja de hospedar (`agentsIslandHost.on = false`) para que el
   * ancla de Rust nazca el float en vez de ignorarse; con consolas vivas se
   * pide la mudanza isla → float por el bus local.
   */
  async function detachAgentsFloat(): Promise<void> {
    const live = agentsLive;
    agentsIslandHost.on = false;
    // `show` es un interruptor: si Rust todavía cree el globo abierto, el
    // despegue lo CERRABA. Presentar reancla si ya está, y abre si no.
    await presentAgentsWindow().catch(() => {});
    if (live) {
      window.dispatchEvent(
        new CustomEvent<AgentsOverlayDetachDetail>(AGENTS_OVERLAY_DETACH, {
          detail: { from: "island", to: "float" },
        }),
      );
    }
  }

  /**
   * Re-acople desde el float: el lanzador del float muda sus consolas a la
   * isla; al terminar (`onOverlayDetached`) se abre la cara y se cierra el
   * float.
   */
  function retachAgentsFromFloat(): void {
    window.dispatchEvent(
      new CustomEvent<AgentsOverlayDetachDetail>(AGENTS_OVERLAY_DETACH, {
        detail: { from: "float", to: "island" },
      }),
    );
  }

  /**
   * Cierre de la mudanza intra-overlay.
   *
   * Float: las consolas ya no están en la isla — el chip no queda como
   * "minimizado" (eso lo puso `dismissToolFace` al despegar).
   * Isla: la cara recibe las consolas; `openIslandTool` esconde el float.
   */
  function onOverlayDetached(event: Event): void {
    const detail = (event as CustomEvent<AgentsOverlayDetachedDetail>).detail;
    if (!detail) return;
    if (detail.to === "float") {
      agentsDock.setMinimized(false);
      return;
    }
    void openIslandTool("agents");
  }

  /**
   * Rebote del despegue: el panel se fue de golpe y la isla queda liviana.
   *
   * Aplastón + estirón (squash & stretch) sobre el cuerpo de la isla, con el
   * origen clavado en el canto: la pestaña "rebota" contra el borde. El
   * tracker se despierta al arrancar y al terminar: el transform animado
   * cambia el rect cada cuadro y el campo lo sigue.
   */
  let detachBounce = $state(false);
  let detachBounceTimer = 0;

  /**
   * Vista previa del imán: un float arrastrado se colocaría si se soltara ya.
   * Con una cara abierta no: hinchar la tarjeta entera sería un salto.
   */
  const retachCue = $derived(retachPreview.tool !== null && !faceOpen);
  $effect(() => {
    void retachCue;
    // El transform cambia el rect cada cuadro: el campo líquido tiene que seguirlo.
    tracker.wake(true);
  });

  function armIslandBounce(): void {
    detachBounce = true;
    tracker.wake(true);
    window.clearTimeout(detachBounceTimer);
    // Cubre delay (200) + animación (360) del CSS: soltar la bandera antes
    // cortaría el rebote a mitad del estirón.
    detachBounceTimer = window.setTimeout(() => {
      detachBounce = false;
      detachBounceTimer = 0;
      tracker.wake(true);
    }, 620);
  }
  /** Alto de la zona de pestaña cuando la tarjeta cuelga (igual que cerrada). */
  const faceTabH = $derived(islandCue ? PILL.islandCueThick : PILL.islandThick);

  /** La letra cuelga de la pestaña cerrada: solo arriba/abajo, sin otra cara. */
  const lyricHang = $derived(
    mediaLyric != null && dock != null && dockAxis(dock.edge) === "y" && islandFace === "tab",
  );
  let lyricsEl = $state<HTMLElement | null>(null);

  /**
   * Celdas de la tira abierta: la marca, las herramientas y el aviso de update.
   *
   * La marca es una celda más y no un adorno: abierta y cerrada tienen que
   * leerse como la misma cosa, y antes al abrir la marca desaparecía. El
   * update entra acá porque ya no cuelga.
   */
  const islandSlots = $derived(
    1 +
      stripNodes.length +
      (mediaCell ? 1 : 0) +
      (updateChip ? 1 : 0) +
      (systemChip || volumeChip ? 1 : 0) +
      islandLiveSlots(activity),
  );

  /**
   * Piso CSS del dintel: el overshoot de `--ease-island` no puede dejar la
   * caja más chica que la pestaña en reposo. Crece sí; encoger de más, no.
   */
  const dockedTabWindow = $derived(
    surface === "edge" && dock
      ? windowFor(
          contentFor(
            "edge",
            barW,
            { edge: dock.edge, expanded: false },
            activity,
            islandSlots,
            edgeCue,
            edgeCueMarks,
            0,
            "tab",
            false,
          ),
        )
      : null,
  );

  const target = $derived(
    windowFor(
      contentFor(
        surface,
        barW,
        dock,
        activity,
        // La tira mide en celdas, no en herramientas: la marca y el update
        // ocupan las suyas.
        islandSlots,
        edgeCue,
        edgeCueMarks,
        surface === "none" ? chips.length : 0,
        islandFace,
        agentsConsoleView,
        agentsBrowserOpen,
        // El panel al costado agranda la caja ya, sin esperar el layout: la
        // transición de cierre la tiene que encoger de una.
        sideInBox,
        // El primer aviso con texto reserva su tramo fijo en la pestaña.
        mediaLyric != null && !lyricHang ? PILL.islandLyricW : islandCueMsg || mediaTitleOn,
        // La cara live crece una fila compacta por chip.
        chips.length,
        // El vistazo abierto dentro de la tira la hace crecer hacia adentro.
        islandPeekBox,
        lyricHang ? { w: PILL.islandLyricHangW, h: PILL.islandLyricHangH } : null,
      ),
    ),
  );

  /**
   * Contra qué eje se aplana la isla, o `null` si no está acoplada.
   *
   * `"x"` (izquierda/derecha) despliega las herramientas en columna; `"y"`
   * (arriba/abajo), en fila. Siempre a lo largo del borde: es el único eje
   * donde crecer no le tapa la pantalla al usuario.
   */
  const peekEdgeAxis = $derived(
    surface === "edge" && dock ? dockAxis(dock.edge) : null,
  );

  /**
   * Cifra corta en el badge de la tira: un número, o «!» si un agente espera.
   * Va igual en los cuatro cantos: el botón mide lo mismo en la columna que
   * en la fila, así que al costado también entra.
   */
  const islandAgentBadgeLabel = $derived.by(() => {
    if (!islandCue) return null;
    if (chip.tone === "count") return chip.label;
    if (chip.tone === "waiting") return "!";
    return null;
  });

  /** La tira está desplegada (o se está desplegando). */
  const islandOpen = $derived(surface === "edge" && dock?.expanded === true);

  // Cerrada la tira, vuelve al primer paso: reabrirla en el submenú dejaría al
  // usuario frente a media lista sin saber por qué.
  $effect(() => {
    if (!islandOpen) stripPage = "ring";
  });

  /**
   * Re-deal de la tira al cambiar de página («Más»/atrás).
   *
   * Es una animación CSS disparada por esta clase, no una transición de
   * Svelte a propósito: `islandAttachers` se indexa por posición sobre el
   * catálogo, y un elemento saliente vivo durante el outro correría el índice
   * y daría de baja el rect de otra gota.
   */
  let stripSwapping = $state(false);
  let lastStripPage: "ring" | "more" = untrack(() => stripPage);
  $effect(() => {
    const page = stripPage;
    if (page === lastStripPage) return;
    lastStripPage = page;
    if (!islandOpen) return;
    stripSwapping = true;
    const timer = setTimeout(() => (stripSwapping = false), 260);
    return () => clearTimeout(timer);
  });

  /**
   * Las gotas siguen siendo la silueta un rato DESPUÉS de que el estado cambió.
   *
   * Sin esto no hay cierre ni despegue que animar: al bajar `expanded` —o al
   * soltarse del canto— el estado salta y las gotas dejarían de publicarse en
   * el mismo cuadro, o sea un corte. Manteniéndolas vivas lo que dura la
   * coreografía, se las ve juntarse: al cerrar, hacia la pestaña; al despegar,
   * fundiéndose con el disco de la pill, que para entonces ya está publicado en
   * el mismo sitio.
   */
  let beadsAlive = $state(false);
  $effect(() => {
    if (islandOpen) {
      beadsAlive = true;
      return;
    }
    if (!beadsAlive) return;
    const timer = setTimeout(() => (beadsAlive = false), ms(MOTION.islandOpen));
    return () => clearTimeout(timer);
  });

  /**
   * La gota de la barra (grabación, cola, aviso) tiene que existir también
   * al irse: si se desmonta en el mismo cuadro, el campo corta y no hay
   * cierre que leer. `tailIn` va un frame tarde para que `inset` transicione.
   */
  let tailAlive = $state(false);
  let tailIn = $state(false);
  $effect(() => {
    if (!discOnly) {
      tailAlive = true;
      const raf = requestAnimationFrame(() => (tailIn = true));
      return () => cancelAnimationFrame(raf);
    }
    tailIn = false;
    if (!tailAlive) return;
    const timer = setTimeout(() => (tailAlive = false), ms(MOTION.panel));
    return () => clearTimeout(timer);
  });

  /**
   * La gota del dictado: la onda cuelga debajo de la barra flotante.
   *
   * Misma mecánica que el tail: el elemento vive un tramo de más al cerrar
   * (la caja encoge con `--island-open-dur`) y `dropIn` entra un frame tarde
   * para que la caída transicione.
   */
  const dictatingBar = $derived(
    surface === "none" && dictation === "listening" && !wheelChrome,
  );
  let dropAlive = $state(false);
  let dropIn = $state(false);
  $effect(() => {
    if (dictatingBar) {
      dropAlive = true;
      const raf = requestAnimationFrame(() => (dropIn = true));
      return () => cancelAnimationFrame(raf);
    }
    dropIn = false;
    if (!dropAlive) return;
    const timer = setTimeout(() => (dropAlive = false), ms(MOTION.islandOpen));
    return () => clearTimeout(timer);
  });

  /**
   * Lado del chip de consola: opuesto al borde horizontal más cercano.
   * Usa el centro de la caja de la pill (no solo el disco) para no saltar
   * al expandirse el aviso.
   */
  const consoleSide = $derived.by(() => {
    void areasEpoch;
    return consoleSideFor(stage.workAreas(), at, box);
  });

  /** Traza al log de Rust. Fire-and-forget: no debe alterar el flujo ni fallar. */
  function trace(msg: string) {
    void pillTrace(msg).catch(() => {});
  }

  /** Las consolas de agentes viven en su ventana: abrirla es mostrarlas. */
  async function openAgentsConsole() {
    await agentsEnsureWindow().catch((err) => {
      console.warn("abrir la ventana de agentes", err);
    });
  }

  /**
   * Abre la consola en la pestaña de ESTE agente, si corre en una de Atic.
   * Si Rust no la encuentra, la consola abre como siempre.
   */
  async function openAgentsConsoleAt(presenceId: string | undefined): Promise<void> {
    const session = presenceId ? await consoleForPresence(presenceId).catch(() => null) : null;
    if (session) await openConsoleSession(session);
    else await openAgentsConsole();
  }

  /**
   * La consola puede estar montándose recién (la isla la monta al abrirse): se
   * pide la pestaña ahora y otra vez un momento después. Pedirla dos veces no
   * cambia nada si ya está a la vista.
   */
  async function openConsoleSession(session: string): Promise<void> {
    // Las consolas viven en la ventana de agentes: ahí se muestra esta PTY.
    await focusAgentSession("terminal", session);
  }

  function activateAgentChip(which: AgentChip | null, preferBindFromGesture = false) {
    const current = which ?? chip;
    if (agentsDock.minimized && (current.tone === "off" || !which)) {
      void agentsEnsureWindow().catch(() => {});
      return;
    }
    const target = current.target;
    const preferBind = preferBindFromGesture;
    if (target.kind === "chat") {
      void focusAgentSession("chat", target.session);
      return;
    }
    if (target.kind === "console") {
      void openAgentsConsoleAt(target.presenceId);
      if (target.presenceId) presence.markSeen(target.presenceId);
      if (current.id === "chat") agents.markAllRead();
      return;
    }
    const id = target.presenceId;
    if (!id) return;
    const unbound = target.kind === "none";
    void (async () => {
      try {
        // Corre en una consola de Atic: esa pestaña, sin adivinar ventanas.
        // Con varios Claude abiertos, atar por ventana no es confiable.
        const session = await consoleForPresence(id).catch(() => null);
        if (session) {
          presence.markSeen(id);
          await openConsoleSession(session);
          return;
        }
        let result =
          preferBind || unbound
            ? await agentPresenceBind(id)
            : await agentPresenceFocus(id);
        if (result.kind === "none" && !preferBind && !unbound) {
          result = await agentPresenceBind(id);
        }
        presence.markSeen(id);
        if (result.kind === "console") {
          await openAgentsConsoleAt(id);
        }
      } catch (err) {
        presence.markSeen(id);
        console.warn("enfocar terminal del agente", err);
      }
    })();
  }

  function onAgentChipClick(event: MouseEvent, which: AgentChip | null = null) {
    if (suppressAgentChipClick && event.detail > 0) {
      event.preventDefault();
      suppressAgentChipClick = false;
      return;
    }
    activateAgentChip(which, event.ctrlKey || event.metaKey);
  }

  function onUpdateChipClick(event: MouseEvent) {
    if (suppressUpdateChipClick && event.detail > 0) {
      event.preventDefault();
      suppressUpdateChipClick = false;
      return;
    }
    void appUpdate.advance();
  }

  async function decidePending(item: PendingItem, decision: PermissionDecision) {
    if (authBusy) return;
    authBusy = true;
    try {
      await agents.decide(item.sessionId, item.permission.id, decision);
    } catch (err) {
      console.warn("decidir permiso de agente", err);
    } finally {
      authBusy = false;
    }
  }

  async function answerPending(item: PendingItem, updatedInput: unknown) {
    if (authBusy) return;
    authBusy = true;
    try {
      await agents.answer(item.sessionId, item.permission.id, updatedInput);
    } catch (err) {
      console.warn("contestar pregunta de agente", err);
    } finally {
      authBusy = false;
    }
  }

  async function decideAuth(decision: PermissionDecision) {
    const req = authRequest;
    if (!req || authBusy) return;
    authBusy = true;
    try {
      await agents.decide(req.sessionId, req.permission.id, decision);
    } catch (err) {
      console.warn("decidir permiso de agente", err);
    } finally {
      authBusy = false;
    }
  }

  /**
   * Ancla la tarjeta de auth debajo (o arriba) de la pastilla.
   * Crece hacia el lado libre del monitor (misma regla que `consoleSide`):
   * cerca del borde izquierdo → se expande a la derecha; cerca del derecho → a la izquierda.
   */
  const authAt = $derived.by(() => {
    void areasEpoch;
    // Ancho para leer un comando o una pregunta con sus opciones.
    const w = 360;
    // Hueco corto: tiene que quedar dentro de REACH para que nazca el cuello.
    const gap = 8;
    const areas = stage.workAreas();
    // `consoleSide` ya es el lado libre: "right" crece a la derecha, "left" a la izquierda.
    let x = consoleSide === "right" ? at.x : at.x + box.w - w;
    let y = at.y + box.h + gap;
    let side: "top" | "bottom" = "top";
    const area =
      areas.find(
        (a) =>
          at.x + box.w / 2 >= a.x &&
          at.x + box.w / 2 <= a.x + a.w &&
          at.y + box.h / 2 >= a.y &&
          at.y + box.h / 2 <= a.y + a.h,
      ) ?? areas[0];
    if (area) {
      const maxX = Math.max(area.x + area.w - w - 8, area.x + 8);
      x = Math.min(Math.max(x, area.x + 8), maxX);
      // Si no cabe abajo, subir por encima de la pill.
      // Altura estimada de la barra compacta de auth (~2 filas).
      const authH = 88;
      if (y + authH > area.y + area.h - 8) {
        y = Math.max(area.y + 8, at.y - gap - authH);
        side = "bottom";
      }
    }
    // Origen del morph: el centro de la pill respecto de la tarjeta
    // (cerca del borde de anclaje → el scale nace asimétrico hacia el lado libre).
    const tail = Math.min(Math.max(at.x + box.w / 2 - x, 24), w - 24);
    return { x, y, w, side, tail };
  });

  /** La tarjeta de auth también entra al campo: nace fundida a la pill. */
  const authJoined = $derived.by(() => {
    if (!authAlive || !authEl) return false;
    const pill = surfaces.live["pill-skin"];
    const auth = surfaces.live["agent-auth"];
    if (!pill || !auth) return false;
    return gapBetween(pill, auth) <= REACH;
  });

  $effect(() => {
    if (!authAlive || !authEl) {
      liquid.publish("agent-auth", []);
      return;
    }
    // Seguir el morph visual de `.float-emerge` (misma causa que clipboard).
    void authShown;
    void authAt.x;
    void authAt.y;
    void at.x;
    void at.y;
    return publishEmergeSkin("agent-auth", authEl, AUTH_CORNER);
  });

  /** Monta / repliega la auth con el mismo ritmo que los floats. */
  $effect(() => {
    if (showAuthCard && authRequest) {
      authView = authRequest;
      // Ya abierta: solo refrescar el permiso, sin reiniciar el morph.
      if (authAlive && authShown) return;
      authAlive = true;
      authShown = false;
      void tick().then(() => {
        requestAnimationFrame(() => {
          if (showAuthCard) authShown = true;
        });
      });
      return;
    }
    if (!authAlive) return;
    authShown = false;
    const timer = window.setTimeout(() => {
      if (!authShown) {
        authAlive = false;
        authView = null;
      }
    }, ms(MOTION.floatClose));
    return () => window.clearTimeout(timer);
  });

  /**
   * Hay una transición dueña de la geometría corriendo (morph de la rueda).
   * Mientras esté puesta, el reconciliador no toca la ventana: un resize suyo
   * cancelaría el morph a mitad de camino y la dejaría donde llegó.
   */
  let opening = $state(false);
  /** Una apertura de rueda ya está en vuelo. Corta el auto-repeat del atajo. */
  let openingWheel = false;
  /**
   * Generación del colapso en curso. Al reabrir a mitad del morph se incrementa
   * para que el resize pendiente no encaje la ventana después del summon.
   */
  let collapseEpoch = 0;
  /**
   * La caja APLICADA llevaba panel. Lo mantiene solo `reconcile`, al final de
   * su rama exitosa: distingue su salida (cierre total) del estado normal, y
   * ahí la pestaña se ancla a la unidad de la consola en vez de al centro de
   * una caja ancha que ya no está.
   */
  let appliedSide = false;

  /** Invalida un encoger/vuelo en curso. */
  function cancelPendingCollapse() {
    collapseEpoch += 1;
    cancelFlight();
    opening = false;
  }
  /** El estado del que dependen las decisiones de `pillPlan`. */
  function plan() {
    return { surface, collapsingFrom, dock, side: sideApplied };
  }

  /**
   * Único reconciliador. La regla de crecer-antes / encoger-después es lo que
   * reemplaza a `chromeHidden`, `radialClosing` y `quickClose` como banderas:
   * la ventana es siempre la unión de origen y destino mientras algo se anima.
   */
  async function reconcile(next: Size) {
    if (opening) return;
    const from = stage.applied();
    // Punto de partida previo al resize: la salida del layout con panel se
    // ancla a la consola, no a la caja ancha que ya no está.
    const startAt = stage.at();
    const outcome = await stage.resize(
      next,
      pivotFor(plan()),
      morphsInPlace({ ...plan(), from }),
    );
    if (outcome.ok) {
      collapsingFrom = null;
      // El resize es `await`: con el gesto vivo, para cuando aterriza el
      // arrastre ya movió la pill varios cuadros. Escribir `at` sin más la
      // devolvía a donde estaba antes del resize —el salto— y encima dejaba el
      // origen del gesto apuntando a una caja de otro tamaño, así que los
      // cuadros siguientes la mantenían corrida medio ancho.
      //
      // Con el gesto vivo mandan el puntero y el tamaño nuevo: se re-centra y
      // se vuelve a sembrar, igual que hace `releaseDockIfFar`.
      if (dragOrigin && dragCursor) {
        dragOrigin.cx = dragCursor.x;
        dragOrigin.cy = dragCursor.y;
        dragOrigin.ox = dragCursor.x - next.w / 2;
        dragOrigin.oy = dragCursor.y - next.h / 2;
        stage.moveTo({ x: dragOrigin.ox, y: dragOrigin.oy });
      } else {
        // Salida del layout con panel: el pivote del canto centra la caja
        // ancha y, sin panel, la pestaña queda corrida media diferencia. Se
        // ancla por la unidad (la consola). En laterales no hace falta: el
        // canto ya clava el eje. En el techo el recentrado de abajo reafirma
        // el medio de la pantalla; en el piso, este ancla es la autoridad.
        if (appliedSide && !sideInBox && from && dock && dockAxis(dock.edge) === "y") {
          stage.moveTo({
            x: sideExitX({ at: startAt, from, next }),
            y: stage.at().y,
          });
        }
        if (
          shouldRecenterTopNotch({
            surface,
            dock,
            flying,
            opening: opening || openingWheel,
          })
        ) {
          // Solo la isla en el techo, en reposo. Si corre al abrir la rueda,
          // el `dock.top` que quedó tira el cuadrado al canto y el clic
          // parece no hacer nada.
          const areas = stage.workAreas();
          const here = { x: stage.at().x, y: stage.at().y, w: next.w, h: next.h };
          const area = areaFor(here, areas);
          if (area) {
            // Con panel en la caja, la unidad que va al medio del techo es la
            // consola: la caja entera incluye el panel y centrarla la correría.
            stage.moveTo(
              edgeCenterPoint(
                "top",
                notchRecenterSize(next, sideInBox),
                workAreaOf(area),
              ),
            );
          }
        }
      }
      at = stage.at();
      box = next;
      appliedSide = sideInBox;
    }
  }

  /**
   * Espera el morph visual de la rueda (transform de nodos/gotas).
   * No animar width/height del root: con pivot center left/top saltan y el
   * centro deriva — eso empeoró el morph anterior.
   */
  async function awaitWheelMorph(
    token:
      typeof MOTION.morphOpen | typeof MOTION.morphClose | typeof MOTION.morphQuick,
  ) {
    const el =
      rootEl?.querySelector<HTMLElement>(".pw-nodes") ??
      rootEl?.querySelector<HTMLElement>(".pw-blob") ??
      null;
    await afterTransition(el, "transform", ms(token));
  }

  function focusWheelToolbar() {
    const toolbar = rootEl?.querySelector<HTMLElement>(".pw-nodes");
    toolbar?.focus({ preventScroll: true });
  }

  $effect(() => {
    const next = target;
    // `opening` va en la traza a propósito: si `target` cambió durante un
    // morph (y no volvió a cambiar), el efecto no se re-dispararía solo y la
    // ventana quedaría con el tamaño viejo. Cuando la coreografía suelta la
    // bandera, acá se reconcilia el destino que quedó pendiente.
    void opening;
    void surfaces.dragging;
    // Durante el gesto el timer/chip no pueden reencuadrar: pelea con el
    // arrastre. El despegue (isla → barra) sí: `releaseDockIfFar` cambia
    // `surface` y `target` salta de golpe.
    if (surfaces.dragging) {
      const from = stage.applied();
      if (!from || Math.abs(from.w - next.w) + Math.abs(from.h - next.h) < 8) {
        return;
      }
    }
    void reconcile(next);
  });

  // La zona viva: sin esto Rust deja el overlay en click-through y la pill no
  // recibe ni un clic.
  $effect(() => (rootEl ? surfaces.add("pill", rootEl) : undefined));

  /**
   * El filete del menisco, en vivo desde el launcher lab (dev).
   *
   * Fuera del lab manda `MENISCUS_FLARE`; con el lab abierto (Ctrl+Alt+F), su
   * slider. Cuando el número quede elegido, se hornea en la constante.
   */
  const meniscusFlare = $derived(
    import.meta.env.DEV && launcherLab.open
      ? launcherLab.meniscusFlare
      : MENISCUS_FLARE,
  );

  /**
   * La SILUETA, aparte de la zona viva.
   *
   * No son lo mismo: la zona incluye el respiro que la pill deja alrededor
   * para que quepa la rueda, y de ahí cuelga la burbuja de agentes. Anclada a
   * la zona, el globo quedaría a `gap` más ese respiro de distancia y el
   * cuello no llegaría a cruzarlo.
   */
  /*
   * Acoplada se publica la isla, no el stack: `liquidEl` sigue midiendo la
   * barra compacta aunque esté oculta —`overflow: hidden` recorta lo que se
   * ve, no lo que mide— y eso dejaba una zona viva más grande que la pestaña.
   * Los floats anclan contra esta silueta, no contra el respiro de `pill`.
   */
  $effect(() => {
    const el = surface === "edge" ? islandSkinEl : liquidEl;
    return el ? surfaces.add("pill-skin", el) : undefined;
  });

  /** La tarjeta de auth también tiene que armar hit-rects o queda click-through. */
  $effect(() => (authEl && authAlive ? surfaces.add("agent-auth", authEl) : undefined));

  $effect(() => {
    // Con un arrastre OLE, Rust poda los hit-rects a los drop-targets
    // (`is_ole_drop_target`): solo `agents`. Minimizada, el target es el chip;
    // ABIERTA en la isla, la cara de agentes — sin esto el overlay queda
    // click-through sobre la consola y el drop cae en la app de atrás.
    const el = agentsDock.minimized
      ? agentDockEl
      : agentsFaceOpen
        ? agentsFaceEl
        : null;
    return el ? surfaces.add("agents", el) : undefined;
  });

  /**
   * El racimo de chips de agentes también arma hit-rects.
   *
   * Cuelga al costado del disco, fuera de la caja del root: sin esto el
   * overlay seguía click-through sobre los chips y no se podía ni abrir un
   * agente ni agarrar la pill desde ahí.
   */
  $effect(() => (agentStackEl ? surfaces.add("agent-stack", agentStackEl) : undefined));

  /**
   * Republicar cuando la pill se MUEVE.
   *
   * El `ResizeObserver` del registro solo ve cambios de tamaño, y mover algo
   * con `left`/`top` no es uno. Sin esto, Rust arma el overlay donde la pill
   * estaba en el primer frame —(0,0), antes de leer su hogar— y la pill queda
   * dibujada en su sitio pero sin recibir un solo evento.
   *
   * Durante el drag `surfaces` ya publicó pantalla completa: no reprogramar.
   */
  $effect(() => {
    void at.x;
    void at.y;
    void box.w;
    void box.h;
    void authAt.x;
    void authAt.y;
    void authAlive;
    void authShown;
    void surfaces.dragging;
    if (surfaces.dragging) return;
    surfaces.schedule();
  });

  // Medir la barra SOLO en reposo.
  //
  // `max-content` la hace independiente del ancho de ventana. Medir con la rueda
  // abierta cierra un lazo —ventana define barra define ventana— y el colapso
  // oscilaba de ancho.
  $effect(() => {
    const el = barEl;
    if (!el) return;
    const measure = () => {
      if (!shouldMeasureBar(surface, surfaces.dragging)) return;
      // max(offset, scroll): si alguna regla vuelve a topar la barra, el ancho
      // real del contenido sigue estando en scrollWidth. Sin esto, un clamp
      // aguas arriba deja la medición mintiendo y la ventana no crece nunca.
      barW = nextBarWidth(
        barW,
        Math.max(el.offsetWidth, el.scrollWidth),
        recording || dictating,
      );
    };
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    measure();
    return () => observer.disconnect();
  });

  /**
   * El ancho real del contenido de la barra, ahora mismo.
   *
   * `max(offset, scroll)`: acoplada, `.p-shell` topa la barra al ancho de la
   * ventana (una pestaña de 42) y `offsetWidth` miente; el contenido de verdad
   * sigue en `scrollWidth`. Por eso se puede medir aunque la pill NO esté
   * flotando, que es lo que hace falta al despegar.
   */
  function measuredBarW(): number | null {
    const el = barEl;
    if (!el) return null;
    return Math.max(el.offsetWidth, el.scrollWidth);
  }

  // Re-medir cuando cambia lo que la barra MUESTRA.
  //
  // El ResizeObserver debería alcanzar, pero con `width: max-content` dentro de
  // una ventana más angosta no dispara de forma confiable: al dictar con el
  // atajo `barW` se quedaba en 40 y la pill seguía redonda, con la tira de
  // ondas recortada adentro. Por la rueda no se notaba porque ahí `wheelCollapse()`
  // redimensiona explícitamente y arrastra la medición nueva.
  $effect(() => {
    const el = barEl;
    if (!el) return;
    // Dependencias explícitas: todo lo que cambia el contenido de la barra.
    void activity;
    void dictation;
    void hasQueue;
    void liveActive;
    void btWarning;
    void agentAlert;
    void agentsDock.minimized;
    void chips;
    void consoleSide;
    void elapsed;
    void surfaces.dragging;
    if (!shouldMeasureBar(surface, surfaces.dragging)) return;
    // Un frame después: en este tick el DOM todavía tiene el contenido viejo.
    const frame = requestAnimationFrame(() => {
      if (!shouldMeasureBar(surface, surfaces.dragging)) return;
      // max(offset, scroll): si alguna regla vuelve a topar la barra, el ancho
      // real del contenido sigue estando en scrollWidth. Sin esto, un clamp
      // aguas arriba deja la medición mintiendo y la ventana no crece nunca.
      barW = nextBarWidth(
        barW,
        Math.max(el.offsetWidth, el.scrollWidth),
        recording || dictating,
      );
    });
    return () => cancelAnimationFrame(frame);
  });

  function fmt(secs: number): string {
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m}:${s.toString().padStart(2, "0")}`;
  }

  function dictationLabel(phase: DictationPhase): string {
    switch (phase) {
      case "listening":
        return t("pill.listening");
      case "transcribing":
        return t("pill.transcribing");
      case "pasted":
        return dictationMessage ?? t("pill.pasted");
      case "error":
        return dictationMessage ?? t("pill.error");
      default:
        return t("pill.start");
    }
  }

  // ─── Isla de borde ───────────────────────────────────────────────────────
  /**
   * Suelta el arrastre: si quedó contra un canto exterior, se acopla.
   *
   * El acople no es solo mover: cambia `surface` a `"edge"`, y a partir de ahí
   * `contentFor` devuelve la pestaña y `pivotFor` clava el lado pegado. Se
   * llama con la posición YA final, después de que `moveTo` clampeó.
   *
   * `drop`: el usuario la acaba de soltar. Ahí no mandan los imanes de
   * centro —un gesto de 40 px volvía al hogar—. Solo un canto cercano. Y ese
   * canto pasa a ser su hogar.
   *
   * `keep`: volvió a un hogar elegido. Se engancha como al soltar, sin los
   * imanes: el del canto la recentraría y perdería la altura que eligió.
   */
  function settleDock(opts?: { drop?: boolean; keep?: boolean }): boolean {
    // La rueda manda: mientras esté abierta o colapsando, la pill no es una
    // isla aunque esté parada sobre el canto.
    if (surface === "wheel" || collapsingFrom === "wheel") return false;
    const size = stage.applied() ?? windowFor({ w: PILL.bar, h: PILL.bar });
    const rect = { x: at.x, y: at.y, w: size.w, h: size.h };
    const found =
      opts?.drop || opts?.keep
        ? snapDrop(rect, stage.workAreas())
        : snapMagnet(rect, stage.workAreas());
    if (!found) {
      dock = null;
      if (surface === "edge") surface = "none";
      return false;
    }
    if (found.edge) {
      dock = { edge: found.edge, expanded: false };
      surface = "edge";
      if (opts?.drop) {
        seating = true;
        window.clearTimeout(seatingTimer);
        seatingTimer = window.setTimeout(() => {
          seating = false;
          seatingTimer = 0;
          tracker.wake(true);
        }, ms(MOTION.slow));
      }
    } else {
      dock = null;
      if (surface === "edge") surface = "none";
    }
    stage.moveTo(found.at);
    at = stage.at();
    if (opts?.drop && found.edge) rememberHome(found.edge, size);
    surfaces.schedule();
    return true;
  }

  /**
   * ¿El arrastre ya se alejó lo suficiente como para soltarla del borde?
   *
   * Si se suelta, hay que **re-anclarla al cursor**. Es el único momento en que
   * la caja cambia de tamaño a mitad de un arrastre: la tira de herramientas
   * mide ~194 px de largo y el disco 40. El pivote de reposo es `topLeft`, así
   * que conservaría la esquina y el disco aparecería en el extremo de donde
   * estaba la tira —a más de 150 px de la mano si la habías agarrado del otro
   * lado—. Recentrarla bajo el puntero es lo que hace que se sienta como que
   * seguís sosteniendo la misma cosa.
   */
  function releaseDockIfFar(cursor: { x: number; y: number } | null): void {
    // Con la rueda abierta el canto no se toca: desacoplar pone
    // `surface = "none"` y la cerraría a mitad del arrastre. El canto se
    // re-evalúa al cerrar, en `settleDock`, ya con la posición nueva.
    if (surface === "wheel") return;
    if (!dock) return;
    const size = stage.applied() ?? windowFor({ w: PILL.bar, h: PILL.bar });
    const rect = { x: at.x, y: at.y, w: size.w, h: size.h };
    if (!shouldUndock(rect, dock.edge, stage.workAreas())) return;
    dock = null;
    surface = "none";
    if (!cursor || !dragOrigin) return;
    // Medir ACÁ, aunque el gesto esté vivo.
    //
    // `shouldMeasureBar` congela la medición mientras se arrastra, así que
    // acoplada `barW` es de la última vez que la pill estuvo flotando. Con esa
    // medida rancia el destino sale del tamaño equivocado y la pill despega
    // como disco para inflarse recién al soltar. En reposo son 18 px y no se
    // ve; grabando son ~120 y la cápsula aterriza en cualquier lado.
    const fresh = measuredBarW();
    if (fresh !== null) barW = fresh;
    // `target` ya refleja el estado nuevo: los derivados se recalculan al
    // leerlos, no al final del tick.
    const next = target;
    // Re-sembrar el origen del gesto, no solo mover: los cuadros siguientes
    // calculan la posición como `origen + (cursor − semilla)`, y con la semilla
    // vieja el disco volvería a saltar al primer movimiento.
    dragOrigin.cx = cursor.x;
    dragOrigin.cy = cursor.y;
    dragOrigin.ox = cursor.x - next.w / 2;
    dragOrigin.oy = cursor.y - next.h / 2;
    stage.moveTo({ x: dragOrigin.ox, y: dragOrigin.oy });
    at = stage.at();
  }

  /**
   * La rueda no se acopla: al arrastrarla hay que volver al disco, recentrado
   * bajo el puntero, para que `settleDock` al soltar pueda enganchar un canto.
   */
  function collapseWheelForDrag(cursor: { x: number; y: number }): void {
    if (!dragClosesWheel(surface) || !dragOrigin) return;
    wheelHeldByHover = false;
    floatWheelHoverLockUntil = performance.now() + ISLAND_COLLAPSE_MS;
    cancelPendingCollapse();
    collapsingFrom = null;
    wheelShown = false;
    wheelQuick = false;
    wheelTool = null;
    wheelPage = "ring";
    surface = "none";
    const next = target;
    dragOrigin.cx = cursor.x;
    dragOrigin.cy = cursor.y;
    dragOrigin.ox = cursor.x - next.w / 2;
    dragOrigin.oy = cursor.y - next.h / 2;
    stage.moveTo({ x: dragOrigin.ox, y: dragOrigin.oy });
    at = stage.at();
  }

  /**
   * Abre o cierra la isla. El puntero es el único que la maneja.
   *
   * Idempotente: `reevaluate_arm` puede mandar varios `pointerenter` seguidos
   * mientras el overlay se arma, y cada uno no debe relanzar el morph.
   */
  function setIslandExpanded(open: boolean): void {
    if (!dock || dock.expanded === open) return;
    dock = { ...dock, expanded: open };
  }

  /**
   * Cada cuánto se le pregunta a Rust si el cursor está sobre la isla.
   *
   * No hay evento que sustituya al sondeo: el armado ya ocurre por movimiento
   * del mouse, pero lo que hace falta saber es si el cursor SIGUE encima, y eso
   * solo lo sabe quien lo ve siempre. 100 ms se siente inmediato y son ~10
   * llamadas por segundo, contra las 60 que ya hace el arrastre.
   */
  const ISLAND_HOVER_MS = 100;
  /**
   * Huella de la cara live mientras el hover abre la tira. La caja encoge
   * hacia el canto; si el cursor estaba en el aviso, sin este rectángulo
   * Rust lo da por fuera y la isla se cierra sola.
   */
  let liveHoverHold: { x: number; y: number; w: number; h: number } | null = null;

  /**
   * Abrir y cerrar la isla lo decide Rust, no el DOM.
   *
   * `pointerenter` no sirve acá y no es un detalle de implementación: mientras
   * el overlay es click-through el webview NO ve el mouse. Cuando Rust lo arma
   * —porque el cursor ya entró en la zona— hace falta otro `mousemove` para que
   * el DOM emita el `pointerenter`, y contra un canto uno tira el mouse y lo
   * deja quieto. Sin ese movimiento extra el evento no llega nunca y la isla no
   * abre. Es el mismo problema que `reevaluate_arm` resuelve del lado del
   * armado cuando una superficie nace debajo del puntero.
   *
   * Rust es la única fuente: mezclar esto con `pointerenter`/`pointerleave`
   * daría dos verdades que se contradicen a mitad de una transición.
   */
  $effect(() => {
    if (surface !== "edge") return;
    void stripPage;
    // El panel de cupos cuelga de la isla: si esta se cierra, el hilo queda
    // pintado en el vacío (pezón en el techo). Mientras el panel vive, la
    // tira es el cuerpo.
    void toolPeekState.open;
    void islandFace;
    let alive = true;
    let leftAt: number | null = null;
    let hoveredAt: number | null = null;
    const look = async () => {
      // Arrastrando no: agrandar la caja a mitad del gesto mueve el suelo bajo
      // el puntero, y encima el destino de acople se calcula con ese tamaño.
      if (dragOrigin) return;
      const lingerMs =
        stripPage === "more" ? ISLAND_COLLAPSE_MORE_MS : ISLAND_COLLAPSE_MS;
      const now = performance.now();
      // Cara de panel o recién cerrada: no pasar por la tira. Si leftAt queda
      // null, el linger de hover abre la tira 400 ms y se ve el morph doble.
      // `live` no cuenta: es un aviso, el hover tiene que abrir las tools.
      if (islandFaceBlocksHover(islandFace) || now < islandFaceLockUntil) {
        if (dock?.expanded) setIslandExpanded(false);
        leftAt = now - lingerMs;
        hoveredAt = null;
        liveHoverHold = null;
        return;
      }
      const overPill = await overlayCursorOverHit("pill").catch(() => null);
      if (!alive || dragOrigin || overPill === null) return;
      // Sobre las filas de avisos la tira no abre: ahí se elige un agente o se
      // descarta un aviso. Las herramientas se piden desde la pestaña, que es
      // de donde nace la tira.
      if (overPill && islandFace === "live" && !dock?.expanded && liveListEl) {
        const cursor = await overlayCursor().catch(() => null);
        if (!alive) return;
        const r = liveListEl.getBoundingClientRect();
        if (pointInRect(cursor, { x: r.x, y: r.y, w: r.width, h: r.height })) {
          hoveredAt = null;
          liveHoverHold = null;
          return;
        }
      }
      // Sobre la letra tampoco: la tira es más baja que la pestaña con letra,
      // y abrirla dejaría el cursor afuera (cerrar, crecer, abrir…).
      if (overPill && lyricHang && !dock?.expanded && lyricsEl) {
        const cursor = await overlayCursor().catch(() => null);
        if (!alive) return;
        const r = lyricsEl.getBoundingClientRect();
        if (pointInRect(cursor, { x: r.x, y: r.y, w: r.width, h: r.height })) {
          hoveredAt = null;
          return;
        }
      }
      let over = overPill;
      if (!over && liveHoverHold) {
        const cursor = await overlayCursor().catch(() => null);
        over = pointInRect(cursor, liveHoverHold);
      }
      if (islandFace === "live" && overPill) {
        const pill = surfaces.live["pill"];
        if (pill) liveHoverHold = { x: pill.x, y: pill.y, w: pill.w, h: pill.h };
      }
      if (!over) liveHoverHold = null;
      if (over) hoveredAt = hoveredAt ?? now;
      else hoveredAt = null;
      const next = islandHoverStay({
        over: islandHoverOpens({
          over: over || toolPeekState.open,
          expanded: dock?.expanded === true,
          hasUpdate: updateChip != null || systemChip != null || islandFace === "live",
          hoveredMs: hoveredAt == null ? 0 : now - hoveredAt,
        }),
        now,
        leftAt,
        lingerMs,
      });
      leftAt = next.leftAt;
      setIslandExpanded(next.open);
    };
    void look();
    const timer = setInterval(() => void look(), ISLAND_HOVER_MS);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  });

  /**
   * Disco flotante: el hover abre la rueda, alejarla la cierra.
   *
   * Misma fuente que la isla (`overlayCursorOverHit`), no `pointerenter`: el
   * overlay es click-through y el primer clic en macOS se lo queda AppKit.
   * Por eso el notch ya andaba y el disco pedía dos clics.
   */
  $effect(() => {
    if (
      !floatWheelHoverWatches({
        surface,
        discOnly,
        idleCapsule:
          surface === "none" && activity === "idle" && !hasQueue && !discOnly,
        heldByHover: wheelHeldByHover,
        collapsingFrom,
      })
    ) {
      return;
    }
    let alive = true;
    let leftAt: number | null = null;
    let hoveredAt: number | null = null;
    const look = async () => {
      if (dragOrigin || openingWheel || flying || bootHidden || birthing || seating) {
        return;
      }
      const now = performance.now();
      if (now < floatWheelHoverLockUntil) {
        leftAt = now - ISLAND_COLLAPSE_MS;
        hoveredAt = null;
        return;
      }
      const over = await overlayCursorOverHit("pill").catch(() => null);
      if (!alive || dragOrigin || openingWheel || over === null) return;
      if (over) hoveredAt = hoveredAt ?? now;
      else hoveredAt = null;
      const next = islandHoverStay({
        over: floatWheelHoverOpens({
          over,
          alreadyOpen: surface === "wheel",
          hoveredMs: hoveredAt == null ? 0 : now - hoveredAt,
        }),
        now,
        leftAt,
        lingerMs: ISLAND_COLLAPSE_MS,
      });
      leftAt = next.leftAt;
      if (next.open) {
        if (surface === "none") {
          wheelHeldByHover = true;
          void openWheel({ fromClick: true, fromHover: true });
        }
        return;
      }
      if (surface === "wheel" && wheelHeldByHover) {
        void closeWheel();
      }
    };
    void look();
    const timer = setInterval(() => void look(), ISLAND_HOVER_MS);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  });

  /**
   * Dictar acoplada ya NO desacopla: la onda vive en la cara de dictado
   * (`.p-face[data-face="dictation"]`), colgada de la pestaña. La pill
   * mantiene la forma del notch y crece hacia el escritorio —el pivote del
   * canto clava el lado pegado—. La gota (`.p-skin-drop`) queda solo para la
   * pill flotante, donde no hay pestaña en qué colgar.
   */

  /**
   * Elegir una herramienta desde la isla.
   *
   * Mismo camino único que la rueda y los atajos, con `force`: apuntar un
   * icono es pedir «abrí esto». No cierra la isla a mano — al alejarse el
   * puntero, `pointerleave` la deja como pestaña.
   */
  function activateFromIsland(id: ToolId): void {
    if (surface !== "edge") return;
    if (id === "agents" && !AGENTS_ENABLED) return;
    requestActivateAtSlot(id, { force: true });
  }

  // ─── Rueda ───────────────────────────────────────────────────────────────
  /**
   * Abre la rueda: vuela al cursor si aporta distancia, crece la caja (hit-box)
   * y morflea el anillo. Esc cancela el vuelo y vuelve al hogar.
   *
   * La firma visual es ParticleWheel (gotas/nodos), no un tween de width del
   * root: animar la caja con pivot center hacía derivar el centro.
   */
  async function openWheel(opts: { fromClick?: boolean; fromHover?: boolean } = {}) {
    // `surface` recién vale "wheel" al final. En esa ventana el auto-repeat del
    // atajo (Windows reenvía `Pressed` mientras la tecla está sostenida) podía
    // reentrar acá: el segundo pase cancelaba el tween del primero.
    if (surface === "wheel" || openingWheel) return;
    if (!opts.fromHover) wheelHeldByHover = false;
    openingWheel = true;
    // El mismo clic que abre llega tarde como `overlay-dismiss` (el overlay
    // se rearma al soltar el gesto). Sin gracia, aborta el vuelo y la pill
    // pega un salto al canto y vuelve.
    armOpenDismissGrace();
    try {
      await openWheelInner(opts);
    } finally {
      // Pase lo que pase: una bandera trabada acá dejaría la rueda muerta para
      // el resto de la sesión.
      openingWheel = false;
    }
  }

  async function openWheelInner(opts: { fromClick?: boolean } = {}) {
    trace("openWheel");
    // Cancela un encoger pendiente: si el morph de cierre aún corre, el resize
    // chico no debe llegar después de que ya volvimos a crecer.
    cancelPendingCollapse();
    await closeWheel();
    wheelQuick = false;
    wheelPage = "ring";
    // Sin selección inicial: un toque accidental del atajo no debe disparar
    // ninguna acción al soltar.
    wheelTool = null;
    opening = true;
    const openEpoch = collapseEpoch;
    const fromEdge = surface === "edge" ? (dock?.edge ?? null) : null;
    try {
      // 1) Guardar el hogar ANTES de tocar la geometría.
      home = { ...at };
      // 2) Volar al sitio donde la rueda cabe: cursor si está lejos (atajo),
      //    o el centro actual clampeado si el clic es sobre la pill.
      //    En el clic no usamos `overlayCursor()`: durante el gesto Rust
      //    publica hit fullscreen y el punto sale en otro lado (visto: un
      //    segundo pegada al canto derecho y de vuelta).
      await stage.loadAreas();
      const size = stage.applied() ?? windowFor({ w: PILL.bar, h: PILL.bar });
      const wheel = windowFor({
        w: PILL.wheel - PILL.pad * 2,
        h: PILL.wheel - PILL.pad * 2,
      });
      const dest = wheelOpenFlight({
        cursor: opts.fromClick ? null : await cursorPoint(),
        pill: { x: at.x, y: at.y, w: size.w, h: size.h },
        wheel,
        areas: stage.workAreas(),
        skipIfNear: FLIGHT_SKIP_PX,
      });
      const flew = await flyTo(dest, { skipIfNear: 2 });
      if (flew < 0 || openEpoch !== collapseEpoch) {
        surface = "none";
        wheelShown = false;
        wheelBloomEdge = null;
        await flyTo(home, { skipIfNear: 2 });
        return;
      }
      if (openEpoch !== collapseEpoch) return;

      // 3) Stack apagado + chrome de rueda colapsado ANTES de crecer: un solo
      //    "a" (ParticleWheel) fijo en el centro.
      collapsingFrom = null;
      surface = "wheel";
      wheelShown = false;
      // Clic en la isla (sin vuelo): la rueda brota del canto. Summon al
      // cursor: morph al centro, como siempre.
      wheelBloomEdge = fromEdge && flew === 0 ? fromEdge : null;
      // 4) Crecer hit-box al instante. El morph visual es el anillo.
      const side = PILL.wheel - PILL.pad * 2;
      const next = windowFor({ w: side, h: side });
      await stage.resize(next, bloomPivot(wheelBloomEdge));
      at = stage.at();
      box = next;
      await tick();
      void rootEl?.offsetWidth;
      // 5) Revelar anillo; soltar `opening` ya — las tools son usables al morph.
      wheelShown = true;
      opening = false;
      focusWheelToolbar();
    } catch (err) {
      console.warn("pill wheel open", err);
      surface = "none";
      wheelShown = false;
      wheelBloomEdge = null;
      opening = false;
    } finally {
      // Abort por Esc: soltar locks aunque el epoch haya cambiado.
      if (opening) opening = false;
    }
  }

  /**
   * Encoge la caja al tamaño compacto (pivot center). No redefine el hogar:
   * el caller decide si vuelve a `home` o vuela a un slot.
   */
  async function wheelCollapse() {
    const next = target;
    await stage.resize(
      next,
      wheelBloomEdge
        ? bloomPivot(wheelBloomEdge)
        : pivotFor({ surface: "none", collapsingFrom: "wheel" }),
    );
    at = stage.at();
    // El escenario ya encogió: el DOM tiene que seguirlo YA, antes de soltar
    // `collapsingFrom`. Si `box` se queda en tamaño rueda, el handoff al stack
    // pinta la marca compacta arriba-izquierda del cuadrado fantasma.
    box = next;
    collapsingFrom = null;
    wheelBloomEdge = null;
    surfaces.schedule();
    trace(`wheelCollapse -> ${next.w}x${next.h} @ ${at.x},${at.y}`);
  }

  /**
   * Cierre: primero el morph de gotas (sin recortar), después encoge la caja.
   * Opcionalmente vuelve al hogar. Devuelve false si cancelaron el epoch.
   */
  async function playCloseMorph(
    epoch: number,
    opts: { returnHome?: boolean } = {},
  ): Promise<boolean> {
    const returnHome = opts.returnHome ?? false;
    wheelShown = false;
    await awaitWheelMorph(wheelQuick ? MOTION.morphQuick : MOTION.morphClose);
    if (epoch !== collapseEpoch) return false;
    await wheelCollapse();
    if (epoch !== collapseEpoch) return false;
    if (returnHome && !returnHomeSuppressed) {
      const flew = await flyTo(home, { skipIfNear: FLIGHT_SKIP_PX });
      if (flew < 0 || epoch !== collapseEpoch) return false;
      // Si el hogar estaba en un canto, volver a él es volver a ser isla. Sin
      // esto la rueda "desacoplaba" la pill: volvía al borde con forma de
      // barra y no se recuperaba hasta arrastrarla de nuevo.
      settleDock({ keep: homeChosen });
    }
    return true;
  }

  /** Cierra la rueda. Por defecto vuelve al hogar; un atajo de tool no debe. */
  async function closeWheel(opts: { returnHome?: boolean } = {}) {
    if (surface !== "wheel") return;
    wheelHeldByHover = false;
    floatWheelHoverLockUntil = performance.now() + ISLAND_COLLAPSE_MS;
    trace("closeWheel");
    const epoch = ++collapseEpoch;
    opening = true;
    // Orden importa: `collapsingFrom` ANTES de `surface = "none"`, para que el
    // stack y la piel líquida no asomen un frame en el top-left del root grande
    // mientras ParticleWheel aún colapsa en el centro.
    collapsingFrom = "wheel";
    wheelTool = null;
    wheelPage = "ring";
    wheelPageEpoch += 1;
    surface = "none";
    try {
      await playCloseMorph(epoch, { returnHome: opts.returnHome ?? true });
    } finally {
      if (epoch === collapseEpoch) opening = false;
    }
  }

  /** Soltar la tecla: activa lo apuntado si la rueda llegó a mostrarse. */
  function onWheelRelease() {
    if (surface !== "wheel") return;
    if (wheelShown && wheelTool) pickWheelNode(wheelTool);
    else void closeWheel();
  }

  /** Teclado con la rueda abierta. No preselecciona: enfocar un nodo al abrir
   *  dejaría una herramienta armada y soltar la tecla la dispararía. */
  function onWheelKey(event: KeyboardEvent): boolean {
    if (surface !== "wheel" || !wheelShown) return false;
    const action = wheelKeyAction(event.key, event.shiftKey);
    if (!action) return false;
    if (action === "activate") {
      if (wheelTool) pickWheelNode(wheelTool);
      else if (wheelPage === "more") backToRing();
      else void closeWheel();
    } else {
      // Sobre los gajos que hay AHORA: dentro del submenú las flechas no
      // pueden pasearse por herramientas que no están en pantalla.
      const next = nextWheelTool(wheelTool, action === "next" ? 1 : -1, wheelNodes);
      if (next !== wheelTool) playWheelTick();
      wheelTool = next;
    }
    return true;
  }

  /**
   * Más / atrás: las gotas vuelven al núcleo y salen las nuevas.
   * Cambiar `tools` con la rueda ya revelada desmontaba el anillo viejo
   * sin outro — apertura sin cierre.
   */
  async function swapWheelPage(next: "ring" | "more") {
    if (wheelPage === next || surface !== "wheel") return;
    const epoch = ++wheelPageEpoch;
    wheelTool = null;
    playWheelTick();
    if (!wheelShown) {
      wheelPage = next;
      return;
    }
    const prevQuick = wheelQuick;
    wheelQuick = true;
    wheelShown = false;
    await awaitWheelMorph(MOTION.morphQuick);
    wheelQuick = prevQuick;
    if (epoch !== wheelPageEpoch || surface !== "wheel") return;
    wheelPage = next;
    await tick();
    void wheelEl?.offsetWidth;
    if (epoch !== wheelPageEpoch || surface !== "wheel") return;
    wheelShown = true;
  }

  /**
   * Elegir un gajo: ejecutar la herramienta, o bajar al segundo anillo.
   *
   * «Más» no es una herramienta y por eso no pasa por `activateTool`: no tiene
   * acción que encolar ni slot al que volar — solo cambia qué se está mirando,
   * con la rueda abierta y en el sitio.
   */
  function pickWheelNode(id: PillWheelId) {
    if (id === PILL_MORE_ID) {
      void swapWheelPage("more");
      return;
    }
    if (id === PILL_WINDOW_ID) {
      void showMainWindow();
      void closeWheel();
      return;
    }
    activateTool(id);
  }

  /** Volver del submenú al primer anillo. El núcleo hace de «atrás». */
  function backToRing() {
    void swapWheelPage("ring");
  }

  /**
   * La rueda ejecuta la herramienta, no navega la app.
   *
   * No tiene despacho propio: encola por el MISMO camino que los atajos, con
   * `force` para que apuntar un gajo siempre abra (nunca alterne).
   *
   * Antes esto reimplementaba `runActivateAtSlot` —cerrar rueda, volar al
   * slot, ejecutar— con su propio contador de generación (`collapseEpoch`) y
   * sin cola. Los dos caminos compartían `returnHomeSuppressed` y
   * `spatialIntent` sin verse entre sí: soltar la rueda y apretar un atajo en
   * el acto los dejaba corriendo en paralelo, cada uno llamando a `flyTo`, y
   * el `finally` del primero soltaba el lock del segundo a mitad del vuelo.
   */
  function activateTool(id: ToolId) {
    if (id === "agents" && !AGENTS_ENABLED) return;
    if (surface !== "wheel") return;
    // El cierre lo hace `runActivateAtSlot` con su propio epoch; acá solo se
    // pide la curva acelerada (la rueda ya cumplió su función).
    wheelQuick = true;
    requestActivateAtSlot(id, { force: true });
  }

  /**
   * Exclusive espacial: al abrir `keep`, cierra los floats no fijados.
   * El pin (“siempre arriba”) mantiene el panel — p. ej. agentes fijado +
   * clipboard para pegar. `returnHomeSuppressed` evita que esos dismiss
   * disparen flyTo(home) a mitad del switch.
   */
  let returnHomeSuppressed = false;
  /** Destino espacial del acto en curso: bloquea volver a casa si B aún nace. */
  let spatialIntent: ToolId | null = null;
  let slotBusy = false;
  let slotPending: SlotRequest | null = null;
  let slotGen = 0;
  async function dismissSpatialTools(keep?: ToolId) {
    returnHomeSuppressed = true;
    const [clipPinned, snipPinned, sysPinned, agentsPinned, consoleLive] =
      await Promise.all([
        clipboardAlwaysOnTop().catch(() => false),
        snippetsAlwaysOnTop().catch(() => false),
        systemAlwaysOnTop().catch(() => false),
        agentsAlwaysOnTop().catch(() => false),
        // Pregunta viva al puente: el flag local puede quedar viejo tras una
        // recarga, y entonces abrir el clipboard cerraba la consola.
        agentsWindowVisible()
          .then((live) => {
            agentsConsoleOpen = live;
            return live;
          })
          .catch(() => agentsConsoleOpen),
      ]);
    const targets = spatialDismissTargets(keep, {
      clipboard: clipPinned,
      snippets: snipPinned,
      system: sysPinned,
      // La consola abierta no se cierra al abrir otra herramienta: es la
      // superficie donde se pega (clipboard → composer/PTY). En la isla el
      // puente la ve por el aviso del efecto; el chequeo local evita la
      // carrera mientras el aviso viaja.
      agents: agentsPinned || consoleLive || islandConsoleAlive(),
    });
    await Promise.all(targets.map((id) => dismissSpatialTool(id).catch(() => {})));
  }

  async function dismissSpatialTool(id: ToolId) {
    switch (id) {
      case "launcher":
        await hideLauncher();
        return;
      case "clipboard":
        await hideClipboardWindow();
        return;
      case "snippets":
        await hideSnippetsWindow();
        return;
      case "system":
        await hideSystemWindow();
        return;
      case "agents":
        await hideAgentsWindow();
        return;
      default:
        return;
    }
  }

  /** Float espacial ya visible, o panel como cara de la isla. */
  function spatialToolOpen(id: ToolId): boolean {
    if (id === "clipboard" && (toolFace === "clipboard" || sidePanel === "clipboard"))
      return true;
    if (id === "snippets" && (toolFace === "snippets" || sidePanel === "snippets"))
      return true;
    if (id === "system" && toolFace === "system") return true;
    // Solo la cara: una consola viva detrás de otra cara (o en el dock) no
    // es «abierta» para el interruptor — pedirla la trae al frente.
    if (id === "agents") return toolFace === "agents";
    return isSpatialTool(id) && surfaces.live[id] != null;
  }

  function islandStayPut(id: ToolId): boolean {
    return (
      (id === "clipboard" && (toolFace === "clipboard" || sidePanel === "clipboard")) ||
      (id === "snippets" && (toolFace === "snippets" || sidePanel === "snippets")) ||
      (id === "system" && toolFace === "system") ||
      (id === "agents" && toolFace === "agents")
    );
  }

  /**
   * Espera a que el float espacial suelte su hit-rect.
   * El close reverse (fuse→shrink) mantiene `shown` hasta el final; si
   * volvemos a casa al dismiss IPC, la pill se va mientras el blob aún
   * se funde. Agentes (`.float-emerge`) no usan este wait.
   */
  function waitSpatialSurfaceGone(id: string, timeoutMs = 2500): Promise<void> {
    return new Promise((resolve) => {
      const start = performance.now();
      const tick = () => {
        if (surfaces.live[id] == null || performance.now() - start > timeoutMs) {
          resolve();
          return;
        }
        requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
    });
  }

  function anySpatialOpen(): boolean {
    return (
      spatialToolOpen("clipboard") ||
      spatialToolOpen("snippets") ||
      spatialToolOpen("system") ||
      spatialToolOpen("launcher") ||
      spatialToolOpen("agents")
    );
  }

  /** El cursor está sobre UI de Atic: hay que no desarmar el overlay a mitad. */
  function overlayUiBusy(): boolean {
    return (
      surface === "wheel" ||
      collapsingFrom === "wheel" ||
      openingWheel ||
      anySpatialOpen()
    );
  }

  /**
   * Re-asienta la pill cuando la geometría del overlay cambió bajo sus pies.
   *
   * Tras un reinicio o al despertar de hibernación, el viewport CSS pasa unos
   * segundos en el recuadro chico del create (visto: 1551×864 con monitores de
   * 3840×1080) mientras Windows asienta las pantallas. Rust ya reencuadra la
   * ventana; acá recargamos áreas y recentramos: acoplada, el centro de ESE
   * canto; si no, el hogar de arriba. Solo si está en reposo.
   */
  let resettleTimer = 0;
  let homeRestored = false;
  /**
   * Hasta cuándo el reencuadre manda por encima de la isla expandida.
   *
   * Abajo no se toca una isla expandida: es la que el usuario tiene abierta
   * bajo el mouse, y moverla sería sacarle el gajo de abajo del dedo. Pero al
   * arrancar se expande sola, y el viewport pasa unos segundos en el recuadro
   * chico de WebView2: con el guard puesto, la pill se quedaba donde la dejó la
   * primera cuenta —contra un recuadro que no era la pantalla— y ya nada la
   * volvía a centrar. Se veía como «arranca corrida a la derecha y al moverla
   * se acomoda». En esos primeros segundos no hay usuario que proteger.
   */
  const BOOT_RESEAT_MS = 8_000;
  let bootAt = 0;
  /** Corta el sondeo de actualizaciones al desmontar. */
  let stopUpdatePolling: (() => void) | null = null;

  function queueResettle() {
    window.clearTimeout(resettleTimer);
    // Los resize del boot llegan en ráfaga (1551→3072→3840): coalescer.
    // No descartar los que llegan antes del hogar: el timer espera
    // `homeRestored` al disparar, no al encolar.
    resettleTimer = window.setTimeout(() => {
      if (!homeRestored) return;
      void resettleAfterGeometry();
    }, 150);
  }

  /**
   * Un cambio de pantallas llegó con la pill ocupada y quedó sin aplicar.
   *
   * Antes se descartaba: desenchufar un monitor con una cara abierta dejaba la
   * pill donde estaba —a veces fuera de toda pantalla— hasta el próximo cambio.
   * Ahora se reintenta cuando vuelve al reposo (ver el efecto de abajo).
   */
  let resettlePending = $state(false);

  async function resettleAfterGeometry() {
    await stage.loadAreas();
    const booting = bootAt > 0 && Date.now() - bootAt < BOOT_RESEAT_MS;
    const busy =
      flying ||
      openingWheel ||
      slotBusy ||
      returnHomeSuppressed ||
      surfaces.dragging ||
      dragOrigin ||
      anySpatialOpen() ||
      (surface !== "none" && surface !== "edge") ||
      (dock?.expanded && !booting);
    resettlePending = Boolean(busy);
    if (busy) return;
    const size = stage.applied() ?? restSize();
    refreshDefaultHome();
    const docked = surface === "edge" ? (dock?.edge ?? null) : null;
    // Con hogar elegido, a su punto exacto: `geometryReseat` la recentraría
    // en el canto (o la mandaría arriba si flotaba).
    const seat =
      homeChosen && (docked === null || docked === homeEdge)
        ? { at: home, edge: homeEdge }
        : geometryReseat(
            { docked, size, current: { x: at.x, y: at.y, w: size.w, h: size.h } },
            stage.workAreas(),
          );
    if (!seat) return;
    stage.moveTo(seat.at);
    at = stage.at();
    if (seat.edge) {
      dock = { edge: seat.edge, expanded: false };
      surface = "edge";
    }
    settleDock({ keep: homeChosen });
  }

  $effect(() => {
    if (!resettlePending) return;
    // Solo lo reactivo: el resto de las condiciones las vuelve a mirar
    // `resettleAfterGeometry`, que deja la marca puesta si aún no toca.
    if (dock?.expanded || (surface !== "none" && surface !== "edge")) return;
    queueResettle();
  });

  /**
   * Tras cerrar un float: re-acopla solo si ya está en un canto o justo en el
   * hogar default. Flotando en otro sitio se queda: no la arrastra al borde.
   */
  async function maybeReturnHome() {
    if (returnHomeSuppressed || slotBusy || spatialIntent) return;
    if (anySpatialOpen()) return;
    if (surface === "edge") return;
    refreshDefaultHome();
    if (atDefaultHome(2)) settleDock({ keep: homeChosen });
  }

  /** Dismiss de tool con reverse liquid: esperar a que el float se funda. */
  async function maybeReturnHomeAfterFloat(
    id: "launcher" | "clipboard" | "snippets" | "system",
  ) {
    // El acto de la pill ya espera el cierre (close / relocate). Si
    // también esperamos acá, un reabrir pisa el wait y se queda colgado
    // del float nuevo.
    if (slotBusy || returnHomeSuppressed) return;
    const gen = slotGen;
    await waitSpatialSurfaceGone(id);
    if (gen !== slotGen) return;
    if (spatialIntent === id) spatialIntent = null;
    await maybeReturnHome();
  }

  /**
   * Tras abrir una tool: la pill vuelve a su hogar. Desde la rueda
   * (Ctrl+Q) eso es el notch; desde un canto, el mismo canto.
   */
  async function restAfterToolOpen(opts: {
    returnToEdge: boolean;
    fromWheel: boolean;
  }): Promise<void> {
    if (opts.fromWheel) {
      if (Math.hypot(home.x - at.x, home.y - at.y) >= 2) {
        await flyTo(home);
      }
      settleDock({ keep: homeChosen });
    }
    if (!opts.returnToEdge) return;
    if (surface !== "edge") await goDefaultHome();
    if (dock && !dock.expanded) {
      dock = { ...dock, expanded: true };
      await tick();
      await reconcile(target);
    }
  }

  /** Historial, textos, sistema o agentes dentro de la isla del notch, un solo blob. */
  async function openIslandTool(
    id: "clipboard" | "snippets" | "system" | "agents",
  ): Promise<void> {
    if (id === "clipboard") await hideClipboardWindow().catch(() => {});
    else if (id === "snippets") await hideSnippetsWindow().catch(() => {});
    else if (id === "system") await hideSystemWindow().catch(() => {});
    else await hideAgentsWindow().catch(() => {});
    if (dock) {
      // El canto actual: laterales también. No volar al techo.
      surface = "edge";
      dock = { ...dock, expanded: false };
    } else if (surface !== "edge") {
      await goDefaultHome();
    }
    if (dock) dock = { ...dock, expanded: false };
    toolFace = id;
    if (id === "clipboard") void clipboard.hydrate();
    if (id === "snippets") {
      snippetsTab = "list";
      void snippets.hydrate();
    }
    if (id === "agents") {
      agentsIslandHost.on = true;
      agentsMounted = true;
      agentsDock.setMinimized(false);
      if (!agentsLive) agentsConsoleView = false;
    }
    armOpenDismissGrace();
    await setOverlayPointerGesture(true).catch(() => {});
    await tick();
    faceElFor(id)?.focus({ preventScroll: true });
  }

  /**
   * Abre el editor de la pill como cara de la isla.
   *
   * Solo acoplada: es donde vive la tira que se edita. `focus` destaca la
   * ficha desde la que se pidió (presión larga o clic derecho sobre ella);
   * «Más», Ventana y compañía no son herramientas y no se destacan.
   */
  async function openCustomizeFace(focus: PillStripId | null): Promise<void> {
    if (surface !== "edge" || !dock) return;
    const tool = WHEEL_TOOLS.find((item) => item.id === focus);
    customizeFocus = tool ? tool.id : null;
    dock = { ...dock, expanded: false };
    toolFace = "customize";
    armOpenDismissGrace();
    await setOverlayPointerGesture(true).catch(() => {});
  }

  /** Cada soltar del editor se guarda en el acto: la tira ya queda así. */
  function saveCustomize(next: { ring: string[]; more: string[] }): void {
    void config
      .patch({ pill_tools: next.ring, pill_more_tools: next.more })
      .catch(toastError);
  }

  /**
   * Presión larga sobre una ficha de la tira: abre el editor con ella a mano.
   *
   * La presión arranca también el arrastre de la pill (la isla se agarra desde
   * cualquier parte). Si se cumple el plazo sin moverse, el gesto deja de ser
   * arrastre y clic: se suelta la vigilancia, así el soltar no ejecuta la
   * herramienta ni un movimiento posterior desacopla la pill.
   */
  const STRIP_HOLD_MS = 480;
  let stripHoldTimer = 0;

  function armStripHold(id: PillStripId): void {
    cancelStripHold();
    stripHoldTimer = window.setTimeout(() => {
      stripHoldTimer = 0;
      if (!dragOrigin || dragMoved) return;
      stopDragWatch();
      islandPressTool = null;
      playWheelTick();
      void openCustomizeFace(id);
    }, STRIP_HOLD_MS);
  }

  function cancelStripHold(): void {
    if (!stripHoldTimer) return;
    window.clearTimeout(stripHoldTimer);
    stripHoldTimer = 0;
  }

  /**
   * Tras pegar en la sesión, el panel del costado se retira: la consola
   * queda sola a la vista. Con la cara abierta (sin consola) no se toca.
   */
  function backToConsoleAfterPaste(): void {
    if (sidePanel !== null) {
      sidePanel = null;
      return;
    }
    if (islandConsoleAlive()) void openIslandTool("agents");
  }

  /**
   * Camino ÚNICO de activación: catálogo, atajo global y rueda.
   *
   * El atajo nombra el destino: misma tool abierta y cursor ahí → cerrar;
   * clipboard/textos con el cursor lejos → reubicar; otra (o ninguna) →
   * mostrar. La rueda pasa `force` y se salta esa decisión (ver `slotIntent`).
   * Si hay un acto en curso, el último pedido gana (no se tira).
   */
  function requestActivateAtSlot(id: ToolId, opts: { force?: boolean } = {}) {
    const req: SlotRequest = { id, force: opts.force ?? false };
    const queued = enqueueActivate(slotBusy, req);
    if (!queued.start) {
      slotPending = queued.pending;
      cancelFlight();
      return;
    }
    void runActivateAtSlot(req);
  }

  /** Cuánto tendría que volar la pill para centrarse en el cursor. */
  async function cursorMovePx(id: ToolId): Promise<number> {
    if (!isCursorAnchored(id) || !spatialToolOpen(id)) return 0;
    const cursor = await cursorPoint();
    const size = stage.applied() ?? windowFor({ w: PILL.bar, h: PILL.bar });
    return pillToCursorMovePx(at, size, cursor);
  }

  async function runActivateAtSlot(req: SlotRequest) {
    const { id, force = false } = req;
    slotBusy = true;
    returnHomeSuppressed = true;
    const gen = ++slotGen;
    // Atajo en frío: el overlay está click-through. Sin armar gesto + gracia
    // de dismiss, el primer toque dejaba la pill inalcanzable o cerraba el
    // float al nacer (Raw Input ve el key-up / un clic fantasma).
    const hold = overlayUiBusy() || isSpatialTool(id) || id === "dictation";
    if (hold) {
      armOpenDismissGrace();
      await setOverlayPointerGesture(true).catch(() => {});
    }
    try {
      const fromWheel = surface === "wheel";
      const returnToEdge = shouldReturnToEdgeOnActivate(surface, dock);
      cancelPendingCollapse();
      await closeWheel({ returnHome: false });
      // El cierre acelerado ya se consumió; el resto del acto usa las curvas
      // normales. Dejarlo puesto no rompe nada, pero miente sobre el estado.
      wheelQuick = false;
      surfaces.resetInteraction();

      // Los agentes viven en su ventana (docs/PLAN_VENTANA_AGENTES.md): la
      // rueda, el atajo y el catálogo la abren en vez de la cara de consola.
      if (id === "agents") {
        await agentsEnsureWindow().catch((err) => {
          console.warn("abrir la ventana de agentes", err);
        });
        return;
      }

      const intent = slotIntent(
        id,
        spatialToolOpen(id),
        // Con `force` la distancia no decide nada: ahorrarse el IPC del cursor.
        force ? 0 : await cursorMovePx(id),
        FLIGHT_SKIP_PX,
        { force, stayPut: islandStayPut(id) },
      );

      if (intent === "close") {
        // El panel del costado se apaga solo: la isla no colapsa.
        if (sidePanel === id) {
          sidePanel = null;
          // Espejo del cierre genérico: el int de la tool no puede quedar
          // colgado bloqueando la vuelta al hogar.
          if (spatialIntent === id) spatialIntent = null;
          return;
        }
        if (islandStayPut(id)) {
          dismissToolFace();
          if (slotPending) return;
          if (!shouldReturnHomeAfterClose(slotPending)) return;
          returnHomeSuppressed = false;
          await maybeReturnHome();
          return;
        }
        if (spatialIntent === id) spatialIntent = null;
        await dismissSpatialTool(id).catch(() => {});
        if (slotPending) return;
        if (
          id === "launcher" ||
          id === "clipboard" ||
          id === "snippets" ||
          id === "system"
        ) {
          await waitSpatialSurfaceGone(id);
        }
        if (gen !== slotGen) return;
        if (!shouldReturnHomeAfterClose(slotPending)) return;
        returnHomeSuppressed = false;
        await maybeReturnHome();
        return;
      }

      if (intent === "relocate") {
        if (spatialIntent === id) spatialIntent = null;
        await dismissSpatialTool(id).catch(() => {});
        if (slotPending) return;
        await waitSpatialSurfaceGone(id);
        if (gen !== slotGen) return;
        if (!shouldCommitShow(slotPending)) return;
      }

      if (isSpatialTool(id) || id === "dictation") await dismissSpatialTools(id);
      if (isSpatialTool(id)) spatialIntent = id;
      else if (id === "dictation") spatialIntent = null;

      if (gen !== slotGen) return;
      if (!shouldCommitShow(slotPending)) return;
      if (id === "clipboard" || id === "snippets" || id === "system") {
        // Con la cara de la consola a la vista, clipboard y textos entran AL
        // COSTADO dentro de la isla: la consola no se cierra y el pegado
        // entra a su sesión. Sin consola, abren como cara, como siempre.
        // Sistema no pega a la consola: siempre es cara propia.
        if ((id === "clipboard" || id === "snippets") && consoleFaceShowing()) {
          sidePanel = id;
          return;
        }
        sidePanel = null;
        await openIslandTool(id);
        return;
      }
      // Launcher: junto al cursor o al centro. El reveal corre después del vuelo a casa.
      const pill = surfaces.live["pill-skin"] ??
        surfaces.live["pill"] ?? {
          x: at.x,
          y: at.y,
          w: box.w,
          h: box.h,
        };
      let birth: { x: number; y: number; w: number; h: number } = pill;
      if (isCursorAnchored(id)) {
        const cursor = await cursorPoint();
        if (cursor) birth = birthAtCursor(cursor, { w: pill.w, h: pill.h });
      }
      captureToolBirth(birth);
      await surfaces.flush();
      await executeToolAction(id);
      await tick();
      await surfaces.flush();
      if (isSpatialTool(id)) await waitToolResting();
      if (gen !== slotGen) return;
      // Recién ahora la pill vuelve a su lugar (notch / hogar).
      await restAfterToolOpen({ returnToEdge, fromWheel });
    } catch (err) {
      console.warn("activate-tool-slot", err);
    } finally {
      if (hold && toolFace === "tab") {
        void setOverlayPointerGesture(false).catch(() => {});
      }
      slotBusy = false;
      returnHomeSuppressed = false;
      const next = slotPending;
      slotPending = null;
      if (next && gen === slotGen) void runActivateAtSlot(next);
    }
  }

  /** Solo vuelo (PTT: en paralelo al start de Rust). */
  async function flySlotOnly(id: ToolId) {
    returnHomeSuppressed = true;
    try {
      const fromWheel = surface === "wheel";
      const returnToEdge = shouldReturnToEdgeOnActivate(surface, dock);
      cancelPendingCollapse();
      await closeWheel({ returnHome: false });
      if (isSpatialTool(id) || id === "dictation") await dismissSpatialTools(id);
      await restAfterToolOpen({ returnToEdge, fromWheel });
    } catch (err) {
      console.warn("fly-tool-slot", err);
    } finally {
      returnHomeSuppressed = false;
    }
  }

  // ─── Acciones ────────────────────────────────────────────────────────────
  async function toggleRecord() {
    if (busy || dictating) return;
    try {
      await capture.toggle();
    } catch (err) {
      console.warn("grabación", err);
    }
  }

  async function toggleDictate() {
    if (busy || recording) return;
    try {
      await dictationStore.toggle();
    } catch (err) {
      console.warn("dictado", err);
    }
  }

  // ─── Arrastre ────────────────────────────────────────────────────────────
  /**
   * El umbral sigue, pero por otro motivo.
   *
   * Antes existía porque `startDragging()` metía la ventana en el loop modal de
   * Windows y se comía el clic: ni el simple ni el doble llegaban nunca. Eso ya
   * no pasa —acá no hay ventana que arrastrar— pero el umbral sigue siendo lo
   * que distingue "clic" de "arrastre" para no abrir la rueda al soltar de un
   * movimiento.
   *
   * El seguimiento lo manda el DOM (`pointermove` / `clientX`). `.ov` es
   * `inset: 0`, así que esas coords YA son las del overlay: no hay que
   * traducirlas ni esperar un IPC. Preguntarle a Rust cada cuadro atrasaba la
   * pill uno o dos frames y, al soltar rápido, el último `await` se descartaba
   * con el origen ya anulado — la caja quedaba atrás y `settleDock` enganchaba
   * el imán equivocado.
   *
   * Rust (`overlayCursor` / `overlayPrimaryDown`) solo entra cuando el DOM se
   * queda mudo: la barra de tareas u otra ventana always-on-top se queda con
   * el mouse y el webview deja de emitir eventos.
   */
  const DRAG_THRESHOLD = 4;
  /** Sin `pointermove` en este rato, el cursor de Win32 toma el relevo. */
  const DRAG_DOM_STALE_MS = 32;
  /** Down original: no se re-siembra. De ahí sale si el gesto fue un clic. */
  let dragPress: { x: number; y: number } | null = null;
  let dragOrigin: {
    cx: number | null;
    cy: number | null;
    ox: number;
    oy: number;
    pointerId: number;
  } | null = null;
  /**
   * Último cursor que vio el gesto, en coordenadas del overlay.
   *
   * Lo necesita `reconcile`: si la caja cambia de tamaño con el arrastre vivo,
   * hay que re-centrarla bajo el puntero, y el reconciliador no tiene de dónde
   * sacar el puntero por su cuenta.
   */
  let dragCursor: { x: number; y: number } | null = null;
  let dragMoved = false;
  let dragRaf = 0;
  /** El último sample vino del DOM: al pasar a Rust hay que re-sembrar. */
  let dragUsedDom = false;
  let dragLastDomAt = 0;
  /** Ya se vio el botón apretado: recién ahí un `false` significa "soltó". */
  let dragSawDown = false;
  /** Icono de la isla donde arrancó el gesto, si arrancó en uno. */
  let islandPressTool: PillStripId | null = null;
  /**
   * La marca de la isla se aprieta como cualquier otro botón de la isla.
   *
   * En la isla el `click` nativo no sirve: el arrastre captura el puntero y
   * el evento se re-apunta al root. Por eso todo acá se resuelve en `endDrag`
   * con un `pointerdown` que solo deja anotado qué se apretó.
   */
  let islandPressMark = false;
  /**
   * La cara agent se aprieta como cualquier botón de la isla: el arrastre
   * captura el puntero y el clic nativo se re-apunta al root, así que se
   * anota en `pointerdown` y se resuelve en `endDrag`. El teclado va por
   * `onkeydown` del botón (ahí no hay pointerdown que lo duplique).
   */
  let facePress: "allow" | "deny" | null = null;
  /** El gesto arrancó sobre el aviso/botón de consola de agentes. */
  let agentChipPressed = false;
  let agentChipPressedId = "";
  let agentChipPreferBind = false;
  /** Un drag sobre el chip no debe terminar convertido en click. */
  let suppressAgentChipClick = false;
  let suppressAgentChipClickTimer = 0;
  /** El gesto arrancó sobre el chip de update. */
  let updateChipPressed = false;
  /** Un drag sobre el chip no debe terminar convertido en click. */
  let suppressUpdateChipClick = false;
  let suppressUpdateChipClickTimer = 0;
  /** El gesto arrancó sobre el núcleo de la rueda: su asa mientras está abierta. */
  let wheelCorePressed = false;
  /** Un drag desde el núcleo no debe terminar cerrando la rueda. */
  let suppressWheelCoreClick = false;
  let suppressWheelCoreClickTimer = 0;

  function beginDrag(event: PointerEvent) {
    const el = event.target as HTMLElement | null;
    if (!el || event.button !== 0) return;
    // La isla se agarra desde CUALQUIER parte, iconos incluidos.
    //
    // Es casi toda botones —cinco de 34 px con 6 de hueco—, así que excluirlos
    // como en la barra dejaba una isla imposible de mover: habría que apuntar a
    // los huecos. Lo que separa arrastrar de elegir es el umbral de siempre, y
    // la herramienta se dispara al soltar sin haber movido (ver `endDrag`),
    // igual que hace la rueda.
    const onIsland = el.closest(".p-island") !== null;
    const onAgentChip = el.closest(".p-agent") !== null;
    const onUpdateChip = el.closest(".p-update") !== null;
    // Abierta, la rueda se agarra por el núcleo y SOLO por ahí.
    //
    // Al revés que la isla: ahí todo es asa porque no queda hueco donde
    // apuntar, pero acá cada gajo es una elección, y un umbral de 4 px sobre
    // un menú radial convertiría cualquier clic con pulso en un arrastre. El
    // núcleo mide 58 px y es donde ya está el puntero cuando la rueda acaba de
    // abrirse, así que no hay que ir a buscarlo. El trato es el del disco en
    // reposo: clic cierra, arrastre mueve.
    const onWheelCore = el.closest(".pw-core") !== null;
    const onRestDisc = el.closest(".p-mark.is-disc") !== null;
    if (
      !onIsland &&
      !onAgentChip &&
      !onUpdateChip &&
      !onWheelCore &&
      !onRestDisc &&
      el.closest("button, a, input, textarea, [data-no-drag]")
    ) {
      return;
    }
    // No preventDefault ni capture todavía: el disco en reposo necesita el
    // `click` nativo (y el sintético de macOS). Capturar al down re-apunta el
    // clic al root y no abre. El SVG no se arrastra: `-webkit-user-drag: none`.
    agentChipPressed = onAgentChip;
    agentChipPressedId = onAgentChip
      ? ((el.closest(".p-agent") as HTMLElement | null)?.dataset.chipId ?? "")
      : "";
    agentChipPreferBind = onAgentChip && (event.ctrlKey || event.metaKey);
    updateChipPressed = onUpdateChip;
    wheelCorePressed = onWheelCore;
    // Semilla del DOM: `.ov` cubre el viewport (`inset: 0`), `clientX/Y` son
    // las coords del overlay. Esperar el primer IPC dejaba el gesto un cuadro
    // atrás desde el primer pixel.
    dragPress = { x: event.clientX, y: event.clientY };
    dragOrigin = {
      cx: event.clientX,
      cy: event.clientY,
      ox: at.x,
      oy: at.y,
      pointerId: event.pointerId,
    };
    dragMoved = false;
    dragSawDown = false;
    dragUsedDom = true;
    dragLastDomAt = performance.now();
    window.addEventListener("pointermove", onDragPointerMove, true);
    window.addEventListener("pointerup", endDrag, true);
    window.addEventListener("pointercancel", endDrag, true);
    // Armar el overlay YA: esperar el umbral de 4px dejaba un hueco donde
    // Rust desarma y el pointerup se pierde. La captura del puntero espera
    // al umbral para no comerse el clic del disco.
    surfaces.dragging = true;
    if (!dragRaf) dragRaf = requestAnimationFrame(() => void tickDrag());
  }

  function captureDragPointer(pointerId: number): void {
    try {
      rootEl?.setPointerCapture(pointerId);
    } catch {
      // Puntero ya liberado: el oyente de `window` alcanza.
    }
  }

  /** Mueve la pill con el sample ya en coords del overlay. */
  function applyDragCursor(cur: { x: number; y: number }) {
    const origin = dragOrigin;
    if (!origin) return;
    dragCursor = cur;
    if (origin.cx === null || origin.cy === null) {
      origin.cx = cur.x;
      origin.cy = cur.y;
      return;
    }
    const dx = cur.x - origin.cx;
    const dy = cur.y - origin.cy;
    const fromPress = dragPress
      ? Math.hypot(cur.x - dragPress.x, cur.y - dragPress.y)
      : Math.hypot(dx, dy);
    if (!dragMoved && fromPress > DRAG_THRESHOLD) {
      dragMoved = true;
      cancelStripHold();
      captureDragPointer(origin.pointerId);
      // Rueda abierta: se cierra al mover para poder acoplar a un canto.
      if (dragClosesWheel(surface)) {
        collapseWheelForDrag(cur);
      }
    }
    if (dragMoved) {
      stage.moveTo({ x: origin.ox + dx, y: origin.oy + dy });
      at = stage.at();
      // Despegar en cuanto se aleja de verdad: si esperáramos a soltar, la
      // isla arrastraría su forma de pestaña por toda la pantalla.
      releaseDockIfFar(cur);
    }
  }

  function onDragPointerMove(event: PointerEvent) {
    const origin = dragOrigin;
    if (!origin || event.pointerId !== origin.pointerId) return;
    if (!pointerMoveDrags(event.buttons)) return;
    const cur = { x: event.clientX, y: event.clientY };
    // Volvimos del fallback de Rust: re-sembrar o el delta mezcla dos relojes
    // y la pill salta. El apply lo hace el rAF, no cada evento: si no, el
    // tracker y la piel se despertarían a 200 Hz.
    if (!dragUsedDom) {
      origin.cx = cur.x;
      origin.cy = cur.y;
      origin.ox = at.x;
      origin.oy = at.y;
      dragUsedDom = true;
    }
    dragLastDomAt = performance.now();
    dragCursor = cur;
  }

  async function tickDrag() {
    dragRaf = 0;
    const origin = dragOrigin;
    if (!origin) return;

    const stale = performance.now() - dragLastDomAt >= DRAG_DOM_STALE_MS;
    if (!stale) {
      if (dragCursor) applyDragCursor(dragCursor);
    } else {
      const [cur, down] = await Promise.all([
        overlayCursor().catch(() => null),
        overlayPrimaryDown().catch(() => null),
      ]);
      // Fin del gesto por Win32 y no por el DOM: soltar sobre la barra de
      // tareas no manda `pointerup` acá, y el arrastre quedaba colgado con el
      // hit-rect a pantalla completa. Se exige haberlo visto apretado antes,
      // para que un cuadro madrugador no aborte el arrastre apenas empieza.
      if (down === true) dragSawDown = true;
      if (dragSawDown && down === false) {
        endDrag();
        return;
      }
      if (cur && dragOrigin === origin) {
        if (dragUsedDom) {
          origin.cx = cur.x;
          origin.cy = cur.y;
          origin.ox = at.x;
          origin.oy = at.y;
          dragUsedDom = false;
        }
        applyDragCursor(cur);
      }
    }

    if (dragOrigin) {
      dragRaf = requestAnimationFrame(() => void tickDrag());
    }
  }

  function stopDragWatch() {
    const pointerId = dragOrigin?.pointerId;
    dragOrigin = null;
    dragPress = null;
    dragCursor = null;
    dragUsedDom = false;
    dragLastDomAt = 0;
    if (pointerId !== undefined && rootEl?.hasPointerCapture(pointerId)) {
      rootEl.releasePointerCapture(pointerId);
    }
    if (dragRaf) {
      cancelAnimationFrame(dragRaf);
      dragRaf = 0;
    }
    surfaces.dragging = false;
    window.removeEventListener("pointermove", onDragPointerMove, true);
    window.removeEventListener("pointerup", endDrag, true);
    window.removeEventListener("pointercancel", endDrag, true);
  }

  /** Soltar sin haber movido = clic. Abre la rueda aunque haya cola o grabación. */
  function endDrag(event?: Event) {
    // El último `pointermove` puede quedar atrás del `pointerup` si el gesto
    // fue rápido. Aplicar acá, con el origen todavía vivo, evita que
    // `settleDock` enganche un imán desde una caja rezagada.
    if (event instanceof PointerEvent && dragOrigin) {
      const cur = { x: event.clientX, y: event.clientY };
      if (!dragUsedDom) {
        dragOrigin.cx = cur.x;
        dragOrigin.cy = cur.y;
        dragOrigin.ox = at.x;
        dragOrigin.oy = at.y;
        dragUsedDom = true;
      }
      applyDragCursor(cur);
    }
    const release =
      event instanceof PointerEvent
        ? { x: event.clientX, y: event.clientY }
        : dragCursor;
    const wasClick = release
      ? pointerGestureWasClick(dragPress, release, DRAG_THRESHOLD)
      : dragOrigin !== null && !dragMoved;
    cancelStripHold();
    const moved = dragMoved;
    const pressedTool = islandPressTool;
    const pressedMark = islandPressMark;
    const pressedAgentChip = agentChipPressed;
    const pressedAgentChipId = agentChipPressedId;
    const preferAgentBind = agentChipPreferBind;
    const pressedWheelCore = wheelCorePressed;
    const pressedUpdateChip = updateChipPressed;
    const pressedFace = facePress;
    islandPressTool = null;
    islandPressMark = false;
    agentChipPressed = false;
    agentChipPressedId = "";
    agentChipPreferBind = false;
    wheelCorePressed = false;
    updateChipPressed = false;
    facePress = null;
    stopDragWatch();
    if (moved) {
      if (pressedAgentChip) {
        suppressAgentChipClick = true;
        window.clearTimeout(suppressAgentChipClickTimer);
        suppressAgentChipClickTimer = window.setTimeout(() => {
          suppressAgentChipClick = false;
        }, 250);
      }
      if (pressedWheelCore) {
        suppressWheelCoreClick = true;
        window.clearTimeout(suppressWheelCoreClickTimer);
        suppressWheelCoreClickTimer = window.setTimeout(() => {
          suppressWheelCoreClick = false;
        }, 250);
      }
      if (pressedUpdateChip) {
        suppressUpdateChipClick = true;
        window.clearTimeout(suppressUpdateChipClickTimer);
        suppressUpdateChipClickTimer = window.setTimeout(() => {
          suppressUpdateChipClick = false;
        }, 250);
      }
      // Al soltar se queda donde quedó, salvo que esté contra un canto.
      // Los imanes de centro son para volver del hogar, no para deshacer
      // un gesto corto.
      settleDock({ drop: true });
      return;
    }
    // Soltar sobre un icono sin haber movido: eso sí era elegirlo.
    // El click nativo del botón abre la consola; no abrir también la rueda.
    if (wasClick && pressedAgentChip) {
      suppressAgentChipClick = true;
      window.clearTimeout(suppressAgentChipClickTimer);
      suppressAgentChipClickTimer = window.setTimeout(() => {
        suppressAgentChipClick = false;
      }, 250);
      activateAgentChip(
        chips.find((c) => c.id === pressedAgentChipId) ?? null,
        preferAgentBind,
      );
      return;
    }
    if (wasClick && pressedUpdateChip) {
      suppressUpdateChipClick = true;
      window.clearTimeout(suppressUpdateChipClickTimer);
      suppressUpdateChipClickTimer = window.setTimeout(() => {
        suppressUpdateChipClick = false;
      }, 250);
      void appUpdate.advance();
      return;
    }
    // Cara agent: la decisión pasa por el mismo `decideAuth` de la tarjeta.
    if (wasClick && pressedFace) {
      void decideAuth(pressedFace);
      return;
    }
    // Soltar el núcleo sin haber movido sigue siendo cerrar. Se decide acá y
    // no por el click nativo del botón porque el arrastre captura el puntero:
    // dejarlo en manos del click sería confiar en cómo cada motor lo re-apunta.
    // Un segundo cierre no molesta: `closeWheel` sale de una si ya no está.
    if (wasClick && pressedWheelCore && surface === "wheel") {
      // Hover la mantiene abierta: el núcleo está bajo el cursor al brotar y
      // un clic residual la cerraría. Se cierra al alejar, como la isla.
      if (wheelHeldByHover) return;
      void closeWheel();
      return;
    }
    // La marca es el control de lo que esté corriendo: para la grabación o el
    // dictado. En reposo el clic no abre la rueda: acoplada no se abre y
    // flotante la despliega el hover. Va antes del clic genérico de la
    // pestaña para que no dispare las dos cosas.
    if (wasClick && pressedMark) {
      if (
        clipboardFaceOpen ||
        snippetsFaceOpen ||
        systemFaceOpen ||
        agentsFaceOpen ||
        customizeFaceOpen
      ) {
        dismissToolFace();
        return;
      }
      if (markState !== "idle" && !busy) {
        markAction.run();
      }
      return;
    }
    if (wasClick && pressedTool) {
      // «Más», «atrás» y Ventana no ejecutan una herramienta: no pasan por
      // `activateFromIsland` ni cierran la isla.
      if (pressedTool === PILL_MORE_ID) {
        stripPage = "more";
        playWheelTick();
        return;
      }
      if (pressedTool === PILL_BACK_ID) {
        stripPage = "ring";
        playWheelTick();
        return;
      }
      if (pressedTool === PILL_WINDOW_ID) {
        void showMainWindow();
        return;
      }
      if (pressedTool === PILL_CUSTOMIZE_ID) {
        void openCustomizeFace(null);
        return;
      }
      activateFromIsland(pressedTool);
      return;
    }
    // El clic genérico no abre la rueda en ninguna forma: acoplada no se abre
    // y flotante la despliega el hover. Antes los huecos entre gotas
    // circulares también la disparaban desde la pestaña.
  }

  // ─── Ciclo de vida ───────────────────────────────────────────────────────
  onMount(() => {
    const unlisteners: Promise<UnlistenFn>[] = [];

    // Escuchar a los agentes desde el arranque: una sesión que responde con la
    // pill cerrada tiene que dejar el aviso puesto.
    // La pill es la que notifica: es la única ventana que siempre está viva, y
    // si notificaran todas habría un toast por ventana abierta.
    void agents.init({ notify: true });
    if (AGENT_PAGER_ENABLED) void presence.init();
    // El aviso del equipo no puede depender de abrir el panel de sistema.
    void systemAlerts.init();

    (async () => {
      // Los monitores y el hogar, antes de nada: el primer reencuadre ya los
      // necesita para clampear, y sin hogar la pill arrancaría en 0,0.
      bootAt = Date.now();
      await stage.loadAreas();
      refreshDefaultHome();
      stage.moveTo(home);
      at = stage.at();
      dock = { edge: homeEdge, expanded: false };
      surface = "edge";
      settleDock({ keep: homeChosen });
      void savePillHome(home.x, home.y);
      // Recién ahora hay hogar de verdad: antes de esto, un resize temprano
      // re-asentaría la pill sobre el {0,0} inicial.
      homeRestored = true;
      // Asentar ANTES del primer frame visible: con el viewport del boot el
      // hogar puede salir corrido (p. ej. a la izquierda) y el `left/top` con
      // transición lo animaría hasta el centro. Oculta (`is-boot` = sin
      // transición) el teleport no se ve; el reveal es gota desde el centro.
      await resettleAfterGeometry();
      if (prefersReducedMotion()) {
        bootHidden = false;
      } else {
        await tick();
        await new Promise<void>((r) =>
          requestAnimationFrame(() => requestAnimationFrame(() => r())),
        );
        bootHidden = false;
        birthing = true;
        tracker.wake(true);
        window.clearTimeout(birthTimer);
        birthTimer = window.setTimeout(
          () => {
            birthing = false;
            birthTimer = 0;
          },
          ms(MOTION.islandOpen) + 60,
        );
      }
      try {
        const cfg = await getConfig();
        wheelShortcut = cfg.pill_radial_shortcut;
        // El sondeo de updates arranca ACÁ y no en un `$effect`.
        //
        // `startPolling()` consulta en el acto, y ese `check()` escribe el
        // mismo estado del store que el efecto tendría que leer: el efecto se
        // invalida a sí mismo y Svelte aborta el árbol entero con
        // `effect_update_depth_exceeded`, dejando la VENTANA COMPLETA sin
        // reactividad. `MainSurface` lo resuelve con `untrack`; acá no hace
        // falta efecto ninguno, porque la condición se lee una sola vez.
        //
        // En dev no se sondea, igual que en la ventana principal: el
        // instalador que responde GitHub no es el que estás corriendo.
        if (!import.meta.env.DEV && cfg.onboarding_done === true) {
          stopUpdatePolling = appUpdate.startPolling();
        }
      } catch {
        // Sin config, el tooltip solo omite el atajo.
      }
      try {
        agentsConsoleOpen = await agentsWindowVisible();
      } catch {
        agentsConsoleOpen = false;
      }
    })();

    // Lo que queda acá son los eventos DE LA PILL: los atajos que la abren y la
    // cierran, y el clic fuera. La actividad, los datos y la cola los escuchan
    // sus stores.
    unlisteners.push(
      onAgentsBubbleAnchor(() => {
        agentsConsoleOpen = true;
      }),
      onAgentsBubbleDismiss(() => {
        agentsConsoleOpen = false;
        void maybeReturnHome();
      }),
      onClipboardBubbleDismiss(() => void maybeReturnHomeAfterFloat("clipboard")),
      onSnippetsBubbleDismiss(() => void maybeReturnHomeAfterFloat("snippets")),
      onSystemBubbleDismiss(() => void maybeReturnHomeAfterFloat("system")),
      onLauncherBubbleDismiss(() => void maybeReturnHomeAfterFloat("launcher")),
      on("activate-tool-slot", (tool) => requestActivateAtSlot(tool)),
      // Retach: el float pide volver a la isla. Clipboard/textos abren la
      // cara directo (mismo camino que el atajo); agentes mueve primero sus
      // consolas vivas y la cara se abre al terminar (ver `onOverlayDetached`).
      on("dock-tool-face", (tool) => {
        if (tool === "clipboard" || tool === "snippets" || tool === "system") {
          void openIslandTool(tool);
          return;
        }
        if (tool === "agents") retachAgentsFromFloat();
      }),
      on("overlay-session-started", () => {
        surfaces.resetInteraction();
        void closeWheel({ returnHome: false });
      }),
      on("fly-tool-slot", (tool) => void flySlotOnly(tool)),
      on("pill-home-reset", () => void resetHome()),
      // El overlay tiene `data_directory` propio: los recientes que guarda la
      // lupa no llegan acá. Rust avisa cada color confirmado y el vistazo de
      // Color arma su propia lista con eso.
      on("color-picked", (hex) => void pushRecentColor(hex)),
      onPillRadialPress(() => void openWheel()),
      onPillRadialRelease(() => onWheelRelease()),
      onPillReset(async () => {
        trace("pill-reset (summon)");
        // Misma cancelación que al reabrir: un encoger a medias no debe
        // reescribir la posición después del vuelo al cursor.
        cancelPendingCollapse();
        // Sin `returnHome` el pill volaba al hogar y recién después al cursor:
        // un detour visible. Acá el vuelo al cursor ES el destino.
        await closeWheel({ returnHome: false });
        const cursor = await cursorPoint();
        if (!cursor) return;
        // Traer al cursor la saca del canto. Si sigue `surface === "edge"`,
        // contentFor mide la pestaña y vuela una isla de ~10 px.
        const undocked = undockForSummon({ surface, dock });
        surface = undocked.surface;
        dock = undocked.dock;
        const size = target;
        await flyTo({ x: cursor.x - size.w / 2, y: cursor.y - size.h / 2 });
      }),
    );

    const onKey = (event: KeyboardEvent) => {
      // La cara colapsa a cue sin resolver: Esc nunca decide por el usuario.
      if (event.key === "Escape" && agentFaceOpen) {
        event.preventDefault();
        event.stopPropagation();
        dismissAgentFace();
        return;
      }
      // El panel del costado se apaga sin tocar la consola de atrás.
      if (event.key === "Escape" && sidePanel) {
        const inConsole = (event.target as HTMLElement | null)?.closest?.(
          ".console, .xterm",
        );
        if (!inConsole) {
          event.preventDefault();
          event.stopPropagation();
          sidePanel = null;
          return;
        }
      }
      if (
        event.key === "Escape" &&
        (clipboardFaceOpen ||
          snippetsFaceOpen ||
          systemFaceOpen ||
          agentsFaceOpen ||
          customizeFaceOpen)
      ) {
        const inConsole = (event.target as HTMLElement | null)?.closest?.(
          ".console, .xterm",
        );
        if (agentsFaceOpen && inConsole) return;
        event.preventDefault();
        event.stopPropagation();
        dismissToolFace();
        return;
      }
      if (event.key === "Escape" && (surface === "wheel" || openingWheel)) {
        event.preventDefault();
        event.stopPropagation();
        if (openingWheel && surface !== "wheel") {
          // Aborta el vuelo; `openWheelInner` vuelve al hogar al ver el epoch.
          cancelFlight();
          collapseEpoch += 1;
          return;
        }
        // Dentro del submenú, Esc deshace un nivel: cerrar de una obligaría a
        // reabrir la rueda para corregir una entrada equivocada.
        if (wheelPage === "more") {
          backToRing();
          return;
        }
        void closeWheel();
        return;
      }
      if (onWheelKey(event)) {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
      // Consola PTY / xterm: no comer Ctrl+U, Ctrl+R, etc.
      const t = event.target as HTMLElement | null;
      if (t?.closest?.(".console, .xterm")) return;
      if (blocksBrowserChrome(event)) {
        event.preventDefault();
        event.stopPropagation();
      }
    };
    /**
     * Cerrar porque el usuario tocó otra cosa.
     *
     * Antes esto era el `blur` de la ventana de la pill. En el overlay ese
     * evento ya no sirve: la ventana abarca la pantalla entera y casi nunca
     * tiene el foco, así que ni se pierde ni se recupera cuando corresponde.
     * Ahora lo detecta Rust —ve todos los clics por Raw Input— y avisa cuando
     * uno cae fuera de las zonas vivas.
     *
     * Es más correcto que el blur, y se llevó puesto el margen de 400 ms que
     * existía solo para tragarse el blur que causaba el propio `setFocus` de la
     * apertura.
     */
    const onOutside = () => {
      surfaces.resetInteraction();
      if (isOpenDismissGrace()) return;
      if (dragMoved) {
        // Solo el clic que terminó el arrastre. La bandera vive hasta el
        // próximo `beginDrag`, y sin consumirla acá un arrastre de la rueda
        // dejaba sordo el cierre por clic afuera de ahí en adelante.
        dragMoved = false;
        return;
      }
      // La cara no resuelve nada: colapsa a cue, que sigue pulsando.
      if (agentFaceOpen) dismissAgentFace();
      if (
        clipboardFaceOpen ||
        snippetsFaceOpen ||
        systemFaceOpen ||
        agentsFaceOpen ||
        customizeFaceOpen
      )
        dismissToolFace();
      if (openingWheel && surface !== "wheel") {
        cancelFlight();
        collapseEpoch += 1;
        return;
      }
      if (surface === "wheel") void closeWheel();
    };

    window.addEventListener("keydown", onKey, true);
    // Mudanza intra-overlay de consolas (detach/retach de agentes): termina
    // la coreografía abriendo/cerrando superficies.
    window.addEventListener(AGENTS_OVERLAY_DETACHED, onOverlayDetached);
    // El viewport que crece tarde (boot tras hibernar) y el reencuadre de
    // Rust tras un cambio de monitores: ambos re-asientan el hogar.
    window.addEventListener("resize", queueResettle);
    window.addEventListener(OVERLAY_GEOMETRY, queueResettle);
    unlisteners.push(onOverlayReady(() => queueResettle()));
    unlisteners.push(onOverlayWorkArea(() => queueResettle()));
    unlisteners.push(onOverlayDismiss(onOutside));
    unlisteners.push(
      onOverlayYieldMain(() => {
        if (dragOrigin) stopDragWatch();
      }),
    );
    trace(`listeners registrados n=${unlisteners.length}`);

    return () => {
      stopDragWatch();
      stopUpdatePolling?.();
      if (detachGesture) {
        const owned = detachHandoff;
        endDetachGesture();
        if (!owned) surfaces.dragging = false;
      }
      window.clearTimeout(detachBounceTimer);
      cancelStripHold();
      window.clearTimeout(suppressAgentChipClickTimer);
      window.clearTimeout(suppressWheelCoreClickTimer);
      window.clearTimeout(resettleTimer);
      window.clearTimeout(seatingTimer);
      window.clearTimeout(birthTimer);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener(AGENTS_OVERLAY_DETACHED, onOverlayDetached);
      window.removeEventListener("resize", queueResettle);
      window.removeEventListener(OVERLAY_GEOMETRY, queueResettle);
      unlisteners.forEach((u) => u.then((fn) => fn()));
    };
  });
</script>

<!-- Testigo de grabación (barra flotante). En la rueda y la isla acoplada
     la parada es una gota líquida colgada, no este botón. -->
{#snippet iconBtn(label: string, icon: IconNode, onClick: () => void, size = 15)}
  <button
    type="button"
    class="p-icon"
    data-no-drag
    onclick={onClick}
    aria-label={label}
    use:tip={label}
  >
    <Icon {icon} {size} />
  </button>
{/snippet}

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="p-root"
  class:is-wheel={wheelChrome}
  class:is-quick={wheelQuick}
  class:is-flying={flying}
  class:is-docked={surface === "edge"}
  class:is-dragging={surfaces.dragging}
  class:is-seating={seating}
  class:is-boot={bootHidden}
  class:is-birthing={birthing}
  data-edge={surface === "edge" ? dock?.edge : undefined}
  data-bloom={wheelBloomEdge ?? undefined}
  style="left: {at.x}px; top: {at.y}px; width: {box.w}px; height: {box.h}px; {dockedTabWindow
    ? `min-width: ${dockedTabWindow.w}px; min-height: ${dockedTabWindow.h}px; `
    : ''}--pill-bar: {PILL.bar}px; --pill-pad: {PILL.pad}px; --island-tool: {PILL.islandTool}px; --island-gap: {PILL.islandGap}px; --island-cue-btn: {PILL.islandCueBtn}px; --island-cue-mark: {PILL.islandCueMark}px; --island-cue-msg-w: {PILL.islandCueMsgW}px; --island-lyric-w: {PILL.islandLyricW}px; --face-tab-h: {faceTabH}px; --face-card-h: {PILL.islandCardH}px; --face-dict-h: {PILL.islandDictH}px; --face-clip-w: {PILL.islandClipW}px; --face-clip-h: {PILL.islandClipH}px; --face-sys-w: {PILL.islandSysW}px; --face-sys-h: {PILL.islandSysH}px; --face-agents-w: {agentsBrowserOpen
    ? PILL.islandBrowseW
    : agentsConsoleView
      ? PILL.islandAgentsW
      : PILL.islandAgentsSetupW}px; --face-agents-h: {agentsBrowserOpen
    ? PILL.islandBrowseH
    : agentsConsoleView
      ? PILL.islandAgentsH
      : PILL.islandAgentsSetupH}px; --face-live-w: {PILL.islandDictW}px; --face-live-side-w: {PILL.islandLiveSideW}px; --island-live-row: {PILL.islandLiveRow}px; --island-live-rows: {Math.max(
    1,
    chips.length,
  )}; --island-clip-r: {PILL.islandClipR}px; --rec-drop: {PILL.recDrop}px; --rec-drop-gap: {PILL.recDropGap}px; {flightLift
    ? `transform: translate3d(${flightLift.x}px, ${flightLift.y}px, 0)`
    : ''}"
  bind:this={rootEl}
  onpointerdown={beginDrag}
  onwheel={onPillWheel}
>
  <!-- Acoplada al borde. `.p-island-skin` llena el cuerpo de la tira y es
       lo que se mide: la silueta líquida sale de ahí. Si hay grabación, una
       gota cuelga hacia adentro, fundida, con el stop encima. -->
  <!-- Montada mientras haya isla O gotas vivas. Las gotas NO se montan con la
       apertura: si nacieran ya abiertas no habría estado inicial desde el cual
       transicionar y el CSS pintaría el final directo. Existen desde que la
       pill se acopla, cerradas, y la clase las abre. -->
  {#if surface === "edge" || beadsAlive}
    <div
      class="p-island"
      class:is-open={islandOpen}
      class:is-face={faceOpen}
      class:is-peek={islandPeekBox != null || peekHold}
      class:has-side={sideApplied}
      class:is-detach-bounce={detachBounce}
      class:is-retach-cue={retachCue}
    >
      <div class="p-island-body">
        <i
          class="p-island-skin"
          bind:this={islandSkinEl}
          {@attach trackIsland}
          aria-hidden="true"
        ></i>
        <div
          class="p-island-along"
          class:is-hidden={islandOpen && !faceOpen}
          class:is-column={peekEdgeAxis === "x"}
          inert={(islandOpen && !faceOpen) || undefined}
        >
          <button
            type="button"
            class="p-island-mark"
            disabled={busy && markState !== "idle"}
            onpointerdown={() => (islandPressMark = true)}
            use:tip={markAction.label}
            aria-label={markAction.label}
          >
            <AticMark
              size={PILL.islandMark}
              strokeWidth={1.6}
              alive={!islandOpen}
              state={markState}
              lag={flying}
            />
          </button>
          {#if edgeCue && !islandOpen && !liveFaceOpen}
            <div class="p-island-cues">
              {#if islandCue}
                {#each chips.length > 0 ? chips : [chip] as c, i (c.id || "dock")}
                  {@const logos = chipLogos(c)}
                  {@const slots = logoSlots(logos)}
                  <button
                    type="button"
                    class="p-agent p-island-cue"
                    class:is-dock={agentsDock.minimized && chips.length === 0}
                    class:is-msg={i === 0 && islandCueMsg}
                    class:is-waiting={c.tone === "waiting"}
                    class:is-working={c.tone === "working"}
                    class:is-ready={c.tone === "ready"}
                    class:is-count={c.tone === "count"}
                    data-chip-id={c.id}
                    {@attach i === 0 ? trackIslandCue : () => {}}
                    onclick={(e) => onAgentChipClick(e, c.tone === "off" ? null : c)}
                    use:tip={chipTitle(c)}
                    aria-label={chipAria(c)}
                  >
                    {#if c.tone === "count"}
                      <span class="p-island-cue-mark">{c.label}</span>
                    {:else if i === 0 && islandCueMsg}
                      <span class="p-island-cue-logos" aria-hidden="true">
                        {#each slots.shown as id (id)}
                          <AgentLogo agent={id} size={PILL.islandCueMark} />
                        {/each}
                        {#if slots.extra > 0}
                          <span class="p-island-cue-more">+{slots.extra}</span>
                        {/if}
                      </span>
                      <span class="p-island-cue-msg">{c.label}</span>
                    {:else if logos.length > 0}
                      <span class="p-island-cue-logos" aria-hidden="true">
                        {#each slots.shown as id (id)}
                          <AgentLogo agent={id} size={PILL.islandCueMark} />
                        {/each}
                        {#if slots.extra > 0}
                          <span class="p-island-cue-more">+{slots.extra}</span>
                        {/if}
                      </span>
                    {:else}
                      <span class="p-island-cue-logo" aria-hidden="true">
                        <AgentLogo agent={null} size={PILL.islandCueMark} />
                      </span>
                    {/if}
                  </button>
                {/each}
              {/if}
              {#if volumeChip}
                <!-- Volumen recién cambiado con la rueda. No es un botón: no
                     lleva a ningún lado, y un botón que no hace nada miente. -->
                <span class="p-update is-volume" aria-hidden="true">
                  <span class="p-update-ico">
                    <Icon
                      icon={volumeChip.icon}
                      size={PILL.islandCueMark - 1}
                      strokeWidth={1.9}
                    />
                  </span>
                </span>
              {:else if systemChip}
                <!-- El equipo se está ahogando. Comparte cápsula con el aviso
                     de actualización porque es lo mismo: algo que la pill
                     cuenta sin robarle el sitio a la marca. -->
                <button
                  type="button"
                  class="p-update p-island-update is-alert"
                  {@attach trackIslandUpdateCue}
                  onclick={onSystemChipClick}
                  use:tip={systemChip.label}
                  aria-label={systemChip.label}
                >
                  <span class="p-update-ico" aria-hidden="true">
                    <Icon
                      icon={systemChip.icon}
                      size={PILL.islandCueMark - 1}
                      strokeWidth={1.9}
                    />
                  </span>
                </button>
              {/if}
              {#if updateChip}
                <button
                  type="button"
                  class="p-update p-island-update"
                  class:is-ready={updateChip.tone === "ready"}
                  class:is-busy={updateChip.tone === "busy"}
                  {@attach trackIslandUpdateCue}
                  disabled={appUpdate.busy}
                  onclick={onUpdateChipClick}
                  use:tip={updateChip.label}
                  aria-label={updateChip.label}
                >
                  <span class="p-update-ico" aria-hidden="true">
                    <Icon
                      icon={updateChip.icon}
                      size={PILL.islandCueMark - 1}
                      strokeWidth={1.9}
                    />
                  </span>
                </button>
              {/if}
              {#if mediaRest}
                {@const mediaTip = mediaRest.artist
                  ? `${mediaRest.title} — ${mediaRest.artist}`
                  : mediaRest.title}
                <!-- Clic: pausa, igual que la celda de la tira. -->
                <button
                  type="button"
                  class="p-media-cue"
                  class:is-msg={mediaTitleOn || (mediaLyric != null && !lyricHang)}
                  disabled={!mediaRest.can_toggle}
                  onclick={() => void media.control("toggle")}
                  use:tip={mediaTip}
                  aria-label={`${t("pill.peek.mediaPause")}: ${mediaTip}`}
                >
                  {#if mediaRest.thumbnail}
                    <img class="p-media-cover" src={mediaRest.thumbnail} alt="" draggable="false" />
                  {/if}
                  {#if mediaTitleOn}
                    <span class="p-island-cue-msg" class:is-lyric={mediaLyric != null && !lyricHang}
                      >{mediaRest.title}</span
                    >
                  {:else if mediaLyric != null && !lyricHang}
                    {#key mediaLyric}
                      <span class="p-island-cue-msg is-lyric">{mediaLyric || "♪"}</span>
                    {/key}
                  {/if}
                  <span class="p-media-eq" aria-hidden="true">
                    <i></i><i></i><i></i>
                  </span>
                </button>
              {/if}
            </div>
          {/if}
        </div>
        {#if lyricHang && !islandOpen}
          <!-- Clic: trae la app que suena. El hover acá no abre la tira. -->
          <button
            type="button"
            class="p-island-lyrics"
            data-no-drag
            bind:this={lyricsEl}
            onpointerdown={(e) => e.stopPropagation()}
            onclick={() => void media.focus()}
            aria-label={mediaLyric || t("pill.peek.mediaOpenPlayer")}
            transition:opacityFade
          >
            {#key mediaLyricAt}
              <span class="p-lyric-now">{mediaLyric || "♪"}</span>
              <span class="p-lyric-next" aria-hidden="true">{mediaLyricNext}</span>
            {/key}
          </button>
        {/if}
        <div
          class="p-island-tools"
          class:is-open={islandOpen}
          class:is-column={peekEdgeAxis === "x"}
          class:is-swapping={stripSwapping}
          style="--n: {islandSlots}"
        >
          <!-- La marca abre la tira igual que ocupa la pestaña: es la misma
               pill, desplegada. Y si algo está corriendo, lo para: la cara ya
               dice qué es, así que el clic actúa sobre eso. -->
          <button
            type="button"
            class="p-island-tool p-island-tool-mark"
            style="--i: 0; --s: {Math.abs((islandSlots - 1) / 2)}"
            disabled={busy && markState !== "idle"}
            use:tip={markAction.label}
            aria-label={markAction.label}
            onpointerdown={() => (islandPressMark = true)}
          >
            <AticMark
              size={PILL.islandMark}
              strokeWidth={1.6}
              alive={islandOpen}
              state={markState}
              lag={flying}
            />
          </button>
          {#each stripNodes as node, i (node.id)}
            {@const slot = i + 1 + islandLiveSlots(activity)}
            {@const help = `${node.label} — ${node.short}`}
            <!-- Las que tienen vistazo no llevan `use:tip`: su hover abre el
               vistazo, y dos globos sobre el mismo botón se taparían. Sin nada
               que mostrar, el vistazo dice lo mismo que habría dicho el tooltip. -->
            <button
              type="button"
              class="p-island-tool"
              style="--i: {slot}; --s: {Math.abs((islandSlots - 1) / 2 - slot)}"
              use:tip={isPeekTool(node.id) ? "" : help}
              use:toolPeek={peekFor(node.id, help)}
              aria-label={node.id === "agents" && islandCue
                ? agentChipAria
                : `${node.label}. ${node.short}`}
              {@attach islandAttachers[i]}
              onpointerdown={(e) => {
                islandPressTool = node.id;
                if (e.button === 0) armStripHold(node.id);
              }}
              oncontextmenu={(e) => {
                e.preventDefault();
                void openCustomizeFace(node.id);
              }}
            >
              <ToolIcon id={node.icon} size={22} strokeWidth={1.6} />
              {#if node.id === "agents" && islandCue}
                <span
                  class="p-island-agent-badge"
                  class:is-dock={agentsDock.minimized}
                  class:is-waiting={chip.tone === "waiting"}
                  class:is-working={chip.tone === "working"}
                  class:is-ready={chip.tone === "ready"}
                  class:is-count={chip.tone === "count"}
                  class:is-label={islandAgentBadgeLabel != null}
                  aria-hidden="true">{islandAgentBadgeLabel ?? ""}</span
                >
              {/if}
            </button>
          {/each}
          <!-- Abierta, la pestaña se desmonta y con ella su chip de update.
               Reaparece como celda de la tira —no colgando— para que el clic
               siga existiendo sin que la silueta cambie. -->
          {#if mediaCell}
            {@const slot = stripNodes.length + 1 + islandLiveSlots(activity)}
            <button
              type="button"
              class="p-island-tool p-island-tool-media"
              class:is-playing={mediaCell.playing}
              style="--i: {slot}; --s: {Math.abs((islandSlots - 1) / 2 - slot)}"
              disabled={!mediaCell.can_toggle}
              use:toolPeek={{ tool: "media", fallback: mediaLabel }}
              aria-label={mediaLabel}
              onclick={() => void media.control("toggle")}
            >
              <Icon icon={mediaCell.playing ? Pause : Play} size={20} strokeWidth={1.7} />
            </button>
          {/if}
          {#if updateChip}
            {@const slot =
              stripNodes.length + 1 + (mediaCell ? 1 : 0) + islandLiveSlots(activity)}
            <button
              type="button"
              class="p-island-tool p-island-tool-update"
              class:is-ready={updateChip.tone === "ready"}
              class:is-busy={updateChip.tone === "busy"}
              style="--i: {slot}; --s: {Math.abs((islandSlots - 1) / 2 - slot)}"
              disabled={appUpdate.busy}
              onclick={onUpdateChipClick}
              use:tip={updateChip.label}
              aria-label={updateChip.label}
            >
              <Icon icon={updateChip.icon} size={18} strokeWidth={1.8} />
            </button>
          {/if}
        </div>
        {#if islandPeekTool}
          <div class="p-island-peek">
            <IslandPeek
              tool={islandPeekTool}
              fallback={toolPeekState.fallback}
              shown={islandPeekBox != null}
              vertical={peekEdgeAxis === "x"}
              onsize={(size) => (islandPeekSize = size)}
            />
          </div>
        {/if}
        {#if liveFaceOpen}
          <div
            class="p-face"
            data-face="live"
            data-no-drag
            tabindex="-1"
            onpointerdown={(e) => e.stopPropagation()}
            transition:opacityFade
          >
            <div class="p-live-list" role="list" bind:this={liveListEl}>
              {#each chips as c (c.id)}
                {@const logos = chipLogos(c)}
                {@const act = liveActivity(c)}
                <div class="p-live-item" role="listitem" class:has-x={c.tone === "ready"}>
                  <button
                    type="button"
                    class="p-live-row"
                    class:is-waiting={c.tone === "waiting"}
                    class:is-working={c.tone === "working"}
                    class:is-ready={c.tone === "ready"}
                    class:is-count={c.tone === "count"}
                    data-chip-id={c.id}
                    onclick={(e) => onAgentChipClick(e, c.tone === "off" ? null : c)}
                    use:tip={liveTitle(c)}
                    aria-label={chipAria(c)}
                  >
                    <!-- Al costado la fila es solo esto: el estado. -->
                    <span class="p-live-state" aria-hidden="true">
                      {#if c.tone === "ready"}
                        <Icon icon={Check} size={14} strokeWidth={2.2} />
                      {:else if c.tone === "waiting"}
                        !
                      {:else}
                        <i class="p-live-spin"></i>
                      {/if}
                    </span>
                    <span class="p-live-row-logos" aria-hidden="true">
                      {#if logos.length > 0}
                        {#each logos as id (id)}
                          <AgentLogo agent={id} size={16} />
                        {/each}
                      {:else}
                        <AgentLogo agent={null} size={16} />
                      {/if}
                    </span>
                    <span class="p-live-row-label">
                      {act ? activityText(act) : chipLiveLabel(c)}
                    </span>
                    {#if c.tone === "working"}
                      <!-- La animación dice qué hace: tres gotas que piensan,
                           un trazo que edita, un cursor que ejecuta… -->
                      <span class="p-live-act" data-act={act?.kind ?? "thinking"} aria-hidden="true">
                        <i></i><i></i><i></i>
                      </span>
                    {/if}
                  </button>
                  {#if c.tone === "ready"}
                    <button
                      type="button"
                      class="p-live-x"
                      aria-label={t("pill.dismiss")}
                      use:tip={t("pill.dismiss")}
                      onclick={() => dismissLive(c)}
                    >
                      <Icon icon={X} size={11} />
                    </button>
                  {/if}
                </div>
              {/each}
            </div>
          </div>
        {/if}
        <!-- Cara agent: tarjeta colgada de la pestaña con el permiso pendiente.
             Mismas palabras que la tarjeta flotante y la consola: es el mismo
             pedido, no otro dialecto. La decisión pasa por `decideAuth`. -->
        {#if clipboardFaceOpen}
          <div
            class="p-face"
            data-face="clipboard"
            data-no-drag
            tabindex="-1"
            bind:this={clipFaceEl}
            onpointerdown={(e) => e.stopPropagation()}
            onkeydown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                dismissToolFace();
              }
            }}
            transition:opacityFade
          >
            <!-- Grab del panel: entre el notch y el contenido. Clic despega
                 en el sitio; arrastre despega siguiendo la mano. El notch
                 (la banda) no se mueve. -->
            <div class="p-face-grab">
              <button
                type="button"
                class="p-face-grab-btn"
                use:tip={t("overlay.detach")}
                aria-label={t("overlay.detach")}
                onpointerdown={startDetachGesture}
                onkeydown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    void detachToolFace();
                  }
                }}
              >
                <i class="p-face-grab-bar" aria-hidden="true"></i>
              </button>
            </div>
            <ClipboardHistoryList
              items={clipboard.items}
              loading={clipboard.loading}
              compact
              island
              onPasted={backToConsoleAfterPaste}
              onRefresh={() => clipboard.hydrate()}
            />
          </div>
        {/if}
        {#if snippetsFaceOpen}
          <div
            class="p-face"
            data-face="snippets"
            data-no-drag
            tabindex="-1"
            bind:this={clipFaceEl}
            onpointerdown={(e) => e.stopPropagation()}
            onkeydown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                dismissToolFace();
              }
            }}
            transition:opacityFade
          >
            <div class="p-face-grab">
              <button
                type="button"
                class="p-face-grab-btn"
                use:tip={t("overlay.detach")}
                aria-label={t("overlay.detach")}
                onpointerdown={startDetachGesture}
                onkeydown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    void detachToolFace();
                  }
                }}
              >
                <i class="p-face-grab-bar" aria-hidden="true"></i>
              </button>
            </div>
            <div class="p-face-tabs" role="tablist" aria-label={t("overlay.snippets")}>
              <button
                type="button"
                role="tab"
                class="p-face-tab"
                class:is-on={snippetsTab === "list"}
                aria-selected={snippetsTab === "list"}
                onclick={() => (snippetsTab = "list")}
              >
                {t("overlay.texts")}
              </button>
              <button
                type="button"
                role="tab"
                class="p-face-tab"
                class:is-on={snippetsTab === "scratchpad"}
                aria-selected={snippetsTab === "scratchpad"}
                onclick={() => (snippetsTab = "scratchpad")}
              >
                {t("overlay.notes")}
              </button>
            </div>
            {#if snippetsTab === "list"}
              <div class="p-face-pane" in:tabPanel|local out:tabPanel|local>
                <SnippetsList
                  items={snippets.items}
                  loading={snippets.loading}
                  compact
                  island
                  onPasted={backToConsoleAfterPaste}
                  onRefresh={() => void snippets.hydrate()}
                />
              </div>
            {:else}
              <div class="p-face-pane" in:tabPanel|local out:tabPanel|local>
                <textarea
                  class="p-face-scratch"
                  value={snippets.scratchpad?.body ?? ""}
                  oninput={(e) => snippets.editScratchpad(e.currentTarget.value)}
                  placeholder={t("overlay.scratchPlaceholder")}
                  aria-label={t("overlay.scratchAria")}></textarea>
              </div>
            {/if}
          </div>
        {/if}
        {#if systemFaceOpen}
          <div
            class="p-face"
            data-face="system"
            data-no-drag
            tabindex="-1"
            bind:this={clipFaceEl}
            onpointerdown={(e) => e.stopPropagation()}
            onkeydown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                dismissToolFace();
              }
            }}
            transition:opacityFade
          >
            <div class="p-face-grab">
              <button
                type="button"
                class="p-face-grab-btn"
                use:tip={t("overlay.detach")}
                aria-label={t("overlay.detach")}
                onpointerdown={startDetachGesture}
                onkeydown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    void detachToolFace();
                  }
                }}
              >
                <i class="p-face-grab-bar" aria-hidden="true"></i>
              </button>
            </div>
            <SystemPanel island />
          </div>
        {/if}
        {#if customizeFaceOpen}
          <div
            class="p-face"
            data-face="customize"
            data-no-drag
            tabindex="-1"
            bind:this={clipFaceEl}
            onpointerdown={(e) => e.stopPropagation()}
            onkeydown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                dismissToolFace();
              }
            }}
            transition:opacityFade
          >
            <PillCustomize
              {layout}
              focus={customizeFocus}
              onplace={saveCustomize}
              onreset={() => saveCustomize({ ring: [], more: [] })}
              ondone={dismissToolFace}
            />
          </div>
        {/if}
        <!-- Fila consola + panel: con el panel abierto, la caja suma el ancho
             de los dos y la consola queda clavada contra su canto. Sin panel
             la fila es `display: contents`: el layout no cambia. -->
        <div class="p-row" class:is-side={sideApplied}>
          {#if agentsMounted}
            <div
              class="p-face"
              class:is-stowed={!agentsFaceOpen}
              data-face="agents"
              data-no-drag
              tabindex="-1"
              bind:this={clipFaceEl}
              {@attach (el: HTMLElement) => {
                agentsFaceEl = el;
                return () => {
                  if (agentsFaceEl === el) agentsFaceEl = null;
                };
              }}
              onpointerdown={(e) => e.stopPropagation()}
              onkeydown={(e) => {
                if (e.key === "Escape") {
                  const inConsole = (e.target as HTMLElement | null)?.closest?.(
                    ".console, .xterm",
                  );
                  if (inConsole) return;
                  e.preventDefault();
                  dismissToolFace();
                }
              }}
              transition:opacityFade
            >
              {#if agentsFaceOpen}
                <!-- Grab del panel: entre el notch y la consola. Clic despega
                     en el sitio; arrastre despega siguiendo la mano. -->
                <div class="p-face-grab">
                  <button
                    type="button"
                    class="p-face-grab-btn"
                    use:tip={t("overlay.detach")}
                    aria-label={t("overlay.detach")}
                    onpointerdown={startDetachGesture}
                    onkeydown={(e) => {
                      if (e.key === "Enter" || e.key === " ") {
                        e.preventDefault();
                        void detachToolFace();
                      }
                    }}
                  >
                    <i class="p-face-grab-bar" aria-hidden="true"></i>
                  </button>
                </div>
              {/if}
              <AgentLauncher
                island
                shown={agentsFaceOpen}
                onLiveChange={(live) => (agentsLive = live)}
                onViewChange={(view) => (agentsConsoleView = view === "console")}
                onBrowserChange={(open) => (agentsBrowserOpen = open)}
                onRevealFloat={() => {
                  // Las consolas están en el float: la cara se retira para
                  // que el float no crezca encima de ella.
                  dismissToolFace();
                  agentsDock.expand();
                }}
              />
            </div>
          {/if}
          {#if sidePanel && consoleFaceShowing()}
            <!-- El pointerdown no sube al root: sin esto, arrastrar un ítem
                 del historial (drag nativo) sembraba el arrastre de la isla y
                 desacoplaba la consola. Mismo patrón que las caras. -->
            <div
              class="p-side"
              data-no-drag
              onpointerdown={(e) => e.stopPropagation()}
              transition:opacityFade
            >
              {#if sidePanel === "clipboard"}
                <div class="p-face-pane">
                  <ClipboardHistoryList
                    items={clipboard.items}
                    loading={clipboard.loading}
                    compact
                    island
                    onPasted={backToConsoleAfterPaste}
                    onRefresh={() => clipboard.hydrate()}
                  />
                </div>
              {:else}
                <div
                  class="p-face-tabs"
                  role="tablist"
                  aria-label={t("overlay.snippets")}
                >
                  <button
                    type="button"
                    role="tab"
                    class="p-face-tab"
                    class:is-on={snippetsTab === "list"}
                    aria-selected={snippetsTab === "list"}
                    onclick={() => (snippetsTab = "list")}
                  >
                    {t("overlay.texts")}
                  </button>
                  <button
                    type="button"
                    role="tab"
                    class="p-face-tab"
                    class:is-on={snippetsTab === "scratchpad"}
                    aria-selected={snippetsTab === "scratchpad"}
                    onclick={() => (snippetsTab = "scratchpad")}
                  >
                    {t("overlay.notes")}
                  </button>
                </div>
                {#if snippetsTab === "list"}
                  <div class="p-face-pane">
                    <SnippetsList
                      items={snippets.items}
                      loading={snippets.loading}
                      compact
                      island
                      onPasted={backToConsoleAfterPaste}
                      onRefresh={() => void snippets.hydrate()}
                    />
                  </div>
                {:else}
                  <div class="p-face-pane">
                    <textarea
                      class="p-face-scratch"
                      value={snippets.scratchpad?.body ?? ""}
                      oninput={(e) => snippets.editScratchpad(e.currentTarget.value)}
                      placeholder={t("overlay.scratchPlaceholder")}
                      aria-label={t("overlay.scratchAria")}></textarea>
                  </div>
                {/if}
              {/if}
            </div>
          {/if}
        </div>
        {#if agentFaceOpen && agentFaceReq}
          {@const perm = agentFaceReq.permission}
          {@const faceLogos = chipLogos(chip)}
          <div class="p-face" data-face="agent" transition:opacityFade>
            <div class="p-face-head">
              <AgentLogo agent={faceLogos[0] ?? null} size={16} />
              <span class="p-face-title">{t("page.agents.permission.title")}</span>
            </div>
            <p class="p-face-desc" use:tip={perm.description?.trim() || perm.tool}>
              <strong>{perm.tool}</strong>{perm.description?.trim()
                ? ` · ${perm.description.trim()}`
                : ""}
            </p>
            <div class="p-face-actions">
              <button
                type="button"
                class="p-face-btn is-deny"
                disabled={authBusy}
                aria-label={t("page.agents.permission.deny")}
                onpointerdown={() => (facePress = "deny")}
                onkeydown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    void decideAuth("deny");
                  }
                }}
              >
                {t("page.agents.permission.deny")}
              </button>
              <button
                type="button"
                class="p-face-btn is-allow"
                disabled={authBusy}
                aria-label={t("page.agents.permission.allow")}
                onpointerdown={() => (facePress = "allow")}
                onkeydown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    void decideAuth("allow");
                  }
                }}
              >
                {t("page.agents.permission.allow")}
              </button>
            </div>
          </div>
        {/if}
        {#if dictationFaceOpen}
          <!-- Cara dictado: la onda (o el estado) cuelga de la pestaña, mismo
               blob que la cara de agentes. La silueta es la isla entera, no
               una gota aparte. Parar el dictado sigue siendo el clic en la
               marca, en la banda de arriba. -->
          <div class="p-face" data-face="dictation" transition:opacityFade>
            {#if dictation === "listening"}
              <span class="p-face-wave" aria-hidden="true">
                <Waveform mic={levels.mic} bars={8} variant="voice" live />
              </span>
            {:else}
              <span
                class="p-face-status"
                class:is-busy={dictation === "transcribing"}
                class:is-ok={dictation === "pasted"}
                class:is-error={dictation === "error"}
                aria-live="polite"
              >
                <ToolIcon id="dictation" size={16} strokeWidth={1.5} />
                <span class="p-face-label">{dictationLabel(dictation)}</span>
              </span>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  {/if}

  <!-- La rueda vive siempre montada. Durante el colapso sigue opaca (aunque
       `revealed` ya sea false) hasta que el root encoge: el handoff al stack
       ocurre en el mismo centro, no con un fundido top-left ↔ centro. -->
  <div class="p-wheel" class:is-open={wheelChrome} data-no-drag bind:this={wheelEl}>
    <ParticleWheel
      compact
      wheelNav
      particles={false}
      revealed={wheelShown}
      tools={wheelNodes}
      skinAttach={attachWheelBlob}
      tipSilent={QUOTA_TOOL}
      bind:activeId={wheelTool}
      caption={wheelPage === "more" ? t("pill.more") : t("tools.wheelCaption")}
      centerLabel={wheelPage === "more" ? t("pill.wheelBack") : t("tools.wheelClose")}
      live={activity === "recording"
        ? "recording"
        : dictation === "listening"
          ? "dictating"
          : "off"}
      liveBusy={busy}
      onLive={() => {
        if (activity === "recording") toggleRecord();
        else void toggleDictate();
      }}
      onSelect={(id) => pickWheelNode(id)}
      onCenter={() => {
        // Arrastrarla por el núcleo no debe además cerrarla: el click nativo
        // llega igual después del pointerup.
        if (suppressWheelCoreClick) return;
        // En el submenú el núcleo es «atrás»: cerrar de un salto obligaría a
        // reabrir la rueda para corregir un gajo mal apuntado.
        if (wheelPage === "more") {
          backToRing();
          return;
        }
        if (wheelHeldByHover) return;
        void closeWheel();
      }}
    />
  </div>

  <!-- El stack es la REFERENCIA de medida y nada más: la silueta ya no se
       dibuja acá, se publica al grupo del overlay. Sigue estando fuera de
       `.p-liquid` porque ese recorta con `overflow: hidden` para contener el
       sobrepaso del morph, y el recorte se comería lo que se mide.
       Apagado (y sin marca) mientras el chrome de la rueda es la silueta
       activa: el stack vive anclado al top-left del root, no al centro. -->
  <div
    class="p-stack"
    class:is-dim={wheelChrome}
    aria-hidden={wheelChrome}
    inert={wheelChrome || undefined}
    bind:this={stackEl}
  >
    <div
      class="p-liquid"
      class:is-working={agentWorking && activity === "idle" && !hasQueue}
      bind:this={liquidEl}
    >
      <div
        class="p-skin"
        class:is-console-start={consoleSide === "left" && !discOnly}
        aria-hidden="true"
      >
        <i class="p-skin-bar" {@attach trackBar}></i>
        {#if tailAlive}
          <i
            class="p-skin-tail"
            class:is-in={tailIn}
            class:is-from-start={consoleSide === "left"}
            {@attach trackTail}
          ></i>
        {/if}
        {#if dropAlive}
          <!-- Dictado: la onda baja a una gota que cuelga de la caja. La
               silueta la publica el campo (un solo blob con la barra); acá
               vive la onda. Decorativa: el stop es la cara. -->
          <i
            class="p-skin-drop"
            class:is-in={dropIn}
            aria-hidden="true"
            {@attach trackDrop}
          >
            <span class="p-wave-hang">
              <Waveform mic={levels.mic} bars={14} variant="voice" live />
            </span>
          </i>
        {/if}
      </div>

      <div class="p-shell">
        <div
          class="p-bar"
          class:is-disc-only={discOnly}
          class:is-console-start={consoleSide === "left" &&
            (agentAlert || agentsDock.minimized) &&
            activity === "idle" &&
            !hasQueue}
          bind:this={barEl}
        >
          {#if activity === "recording"}
            <div class="p-bar-slot" transition:opacityFade>
              {#if !wheelChrome}
                <button
                  type="button"
                  class="p-mark is-lead"
                  data-no-drag
                  disabled={busy}
                  onpointerdown={(e) => e.stopPropagation()}
                  onclick={markAction.run}
                  use:tip={markAction.label}
                  aria-label={markAction.label}
                >
                  <AticMark
                    size={28}
                    strokeWidth={1.5}
                    alive
                    state={markState}
                    lag={flying}
                  />
                </button>
              {/if}
              <span class="p-timer">{fmt(elapsed)}</span>
              {#if liveError}
                <span class="p-chip is-error" role="status">{t("pill.error")}</span>
              {:else if btWarning}
                <span
                  class="p-chip is-warn"
                  role="status"
                  use:tip={btWarning}
                  aria-label={btWarning}>BT</span
                >
              {:else if liveActive}
                <span class="p-chip" role="status">{t("pill.live")}</span>
              {/if}
              <div class="p-wave">
                <Waveform
                  mic={levels.mic}
                  system={levels.system}
                  bars={10}
                  variant="quiet"
                />
              </div>
            </div>
          {:else if dictation === "listening"}
            <!-- Escuchando: la cara queda sola en su lugar y la onda cuelga
               debajo (gota `.p-skin-drop`). El ícono del micrófono no va: el
               stop es el clic en la cara, como en grabación. -->
            <div class="p-bar-slot" transition:opacityFade>
              {#if !wheelChrome}
                <button
                  type="button"
                  class="p-mark is-lead"
                  data-no-drag
                  disabled={busy}
                  onpointerdown={(e) => e.stopPropagation()}
                  onclick={markAction.run}
                  use:tip={markAction.label}
                  aria-label={markAction.label}
                >
                  <AticMark
                    size={28}
                    strokeWidth={1.5}
                    alive
                    state={markState}
                    lag={flying}
                  />
                </button>
              {/if}
            </div>
          {:else if activity === "dictating"}
            <div class="p-bar-slot" transition:opacityFade>
              {#if !wheelChrome}
                <button
                  type="button"
                  class="p-mark is-lead"
                  data-no-drag
                  disabled={busy}
                  onpointerdown={(e) => e.stopPropagation()}
                  onclick={markAction.run}
                  use:tip={markAction.label}
                  aria-label={markAction.label}
                >
                  <AticMark
                    size={28}
                    strokeWidth={1.5}
                    alive
                    state={markState}
                    lag={flying}
                  />
                </button>
              {/if}
              <button
                type="button"
                class="p-dict"
                class:is-busy={dictation === "transcribing"}
                class:is-ok={dictation === "pasted"}
                class:is-error={dictation === "error"}
                data-no-drag
                onclick={toggleDictate}
                disabled={busy || dictation === "transcribing"}
                aria-label={t("pill.dictation")}
                use:tip={dictationLabel(dictation)}
              >
                <ToolIcon id="dictation" size={16} strokeWidth={1.5} />
              </button>
              <span class="p-label" aria-live="polite">{dictationLabel(dictation)}</span
              >
            </div>
          {:else if hasQueue}
            <!-- La cola es un badge sobre el disco, no un reemplazo: antes borraba
               la pill entera y con ella el acceso a la rueda. -->
            <!-- Sin marca mientras la rueda manda: el stack queda en el
               top-left del root grande y una segunda «a» fantasma se veía ahí. -->
            <div class="p-bar-slot" transition:opacityFade>
              {#if !wheelChrome}
                <span class="p-mark is-disc"
                  ><AticMark
                    size={28}
                    strokeWidth={1.5}
                    alive
                    state={markState}
                    lag={flying}
                  /></span
                >
              {/if}
              <span class="p-queue-count">{paste.count}</span>
              <span class="p-queue-text" use:tip={paste.front?.text}>
                {paste.front?.text ?? ""}
              </span>
              <button
                type="button"
                class="p-queue-btn"
                data-no-drag
                disabled={paste.busy}
                onclick={() => void paste.paste()}
              >
                {t("pill.paste")}
              </button>
              {@render iconBtn(t("pill.dismiss"), X, () => void paste.dismiss(), 13)}
            </div>
          {:else}
            <!-- Reposo: disco con la marca. El hover abre la rueda, como la
               isla; alejar el mouse la cierra. El clic sigue abriendo.
               Con la rueda abierta/colapsando no se monta: el único «a» visible
               es el de ParticleWheel (centro). El stack sigue midiendo el
               disco vía `.p-bar.is-disc-only` (el diámetro de `PILL.bar`). -->
            <div class="p-bar-slot" transition:opacityFade>
              {#if !wheelChrome}
                <button
                  type="button"
                  class="p-mark is-disc"
                  onclick={() => {
                    if (dragMoved) return;
                    markAction.run();
                  }}
                  use:tip={discHint}
                  aria-label={discHint}
                >
                  <AticMark
                    size={32}
                    strokeWidth={1.5}
                    alive
                    state={markState}
                    lag={flying}
                  />
                </button>
              {/if}
              <!-- Aviso del agente: aparece solo si hay algo que decir. Es un chip
               junto al disco y no un reemplazo, porque el disco sigue siendo la
               puerta a la rueda. -->
              {#if showAgentTab}
                <div class="p-agent-stack" bind:this={agentStackEl}>
                  {#each chips.length > 0 ? chips : [chip] as c, i (c.id || "dock")}
                    {@const logos = chipLogos(c)}
                    {@const acts = chipActs(c)}
                    <svelte:element
                      this={acts ? "button" : "span"}
                      type={acts ? "button" : undefined}
                      role={acts ? undefined : "status"}
                      class="p-agent"
                      class:is-inert={!acts}
                      class:is-dock={agentsDock.minimized && chips.length === 0}
                      class:is-waiting={c.tone === "waiting"}
                      class:is-working={c.tone === "working"}
                      class:is-ready={c.tone === "ready"}
                      class:is-count={c.tone === "count"}
                      data-chip-id={c.id}
                      {@attach (el: HTMLElement) => {
                        if (i === 0) agentDockEl = el;
                      }}
                      onclick={acts
                        ? (e: MouseEvent) =>
                            onAgentChipClick(e, c.tone === "off" ? null : c)
                        : undefined}
                      use:tip={chipTitle(c)}
                      aria-label={chipAria(c)}
                    >
                      <span
                        class="p-agent-ico"
                        class:is-row={logos.length > 1}
                        aria-hidden="true"
                      >
                        {#if logos.length > 0}
                          {#each logos as id (id)}
                            <AgentLogo
                              agent={id}
                              size={agentsDock.minimized ? 13 : 11}
                            />
                          {/each}
                        {:else}
                          <AgentLogo
                            agent={null}
                            size={agentsDock.minimized ? 13 : 11}
                          />
                        {/if}
                      </span>
                      {#if c.tone === "waiting"}
                        <span class="p-agent-count">{t("pill.permission")}</span>
                      {:else if c.tone === "ready"}
                        <span class="p-agent-msg">{c.label ?? t("pill.ready")}</span>
                      {:else if c.tone === "working" && c.label}
                        <span class="p-agent-msg">{c.label}</span>
                      {:else if c.tone === "count"}
                        <span class="p-agent-count">{c.label}</span>
                      {/if}
                    </svelte:element>
                  {/each}
                </div>
              {/if}
              <!-- Hay versión nueva. Mismo sitio y misma cápsula que el aviso de
               agentes: el disco sigue siendo la puerta a la rueda, y esto es
               algo que la pill cuenta, no algo que la reemplace. -->
              {#if volumeChip && !wheelChrome}
                <span class="p-update is-volume" aria-hidden="true">
                  <span class="p-update-ico">
                    <Icon icon={volumeChip.icon} size={11} strokeWidth={1.9} />
                  </span>
                  <span class="p-update-text">{volumeChip.text}</span>
                </span>
              {:else if systemChip && !wheelChrome}
                <button
                  type="button"
                  class="p-update is-alert"
                  onclick={onSystemChipClick}
                  use:tip={systemChip.label}
                  aria-label={systemChip.label}
                >
                  <span class="p-update-ico" aria-hidden="true">
                    <Icon icon={systemChip.icon} size={11} strokeWidth={1.9} />
                  </span>
                  <span class="p-update-text">{systemChip.text}</span>
                </button>
              {/if}
              {#if updateChip && !wheelChrome}
                <button
                  type="button"
                  class="p-update"
                  class:is-ready={updateChip.tone === "ready"}
                  class:is-busy={updateChip.tone === "busy"}
                  disabled={appUpdate.busy}
                  onclick={onUpdateChipClick}
                  use:tip={updateChip.label}
                  aria-label={updateChip.label}
                >
                  <span class="p-update-ico" aria-hidden="true">
                    <Icon icon={updateChip.icon} size={11} strokeWidth={1.9} />
                  </span>
                  <span class="p-update-text">{updateChip.text}</span>
                </button>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    </div>
  </div>
</div>

{#if AGENTS_ENABLED && authAlive && authView}
  <div
    class="p-auth-host float-emerge"
    class:is-shown={authShown}
    class:is-joined={authJoined}
    data-side={authAt.side}
    style="left: {authAt.x}px; top: {authAt.y}px; width: {authAt.w}px; --tail: {authAt.tail}px"
    bind:this={authEl}
  >
    <PillPending
      items={pendingItems}
      busy={authBusy}
      onDecide={(item, d) => void decidePending(item, d)}
      onAnswer={(item, input) => void answerPending(item, input)}
      onOpen={(item) => void focusAgentSession("chat", item.sessionId)}
    />
  </div>
{/if}

<style>
  /*
   * La pill dejó de ser una ventana: ahora es una caja dentro del overlay.
   *
   * `100vw/100vh` era «lo que mida mi ventana», y el tamaño lo escribía Rust.
   * Acá lo escribe el escenario en `style`, y `left`/`top` lo posicionan. El
   * `overflow: hidden` sigue, con el mismo trabajo de siempre: recortar el
   * contenido mientras el reencuadre lo alcanza.
   */
  .p-root {
    position: absolute;
    z-index: var(--z-overlay-pill);
    display: flex;
    box-sizing: border-box;
    flex-direction: column;
    padding: 4px;
    overflow: hidden;
    cursor: grab;
  }

  .p-root:active {
    cursor: grabbing;
  }

  /* WebKit puede iniciar el arrastre nativo de un SVG/imagen y quedarse con
     el gesto: la pill no se mueve aunque el pointerdown haya llegado. */
  .p-root :where(svg, img) {
    -webkit-user-drag: none;
  }

  .p-root.is-wheel {
    padding: 0;
    cursor: default;
  }

  /*
   * Acoplada, la caja cambia de tamaño Y DE POSICIÓN, y las dos hay que
   * animarlas.
   *
   * El pivote `dock*` clava el lado pegado al canto, así que al abrirse de 40 a
   * 194 px de largo el `left` se corre ~77 px. Ese valor lo escribe el
   * escenario de una vez, mientras el ancho iba animado: se veía el cuerpo
   * desplazarse de golpe hacia un lado y recién después crecer. Al cerrar,
   * igual pero al revés — que es el "se mueve a la derecha y después se cierra
   * de golpe".
   *
   * Misma duración y curva que las gotas (`--island-open-dur` / `--ease-liquid`)
   * y no las del morph: si la caja termina antes, el contenido queda quieto
   * mientras las gotas siguen viaje y se lee como dos animaciones distintas.
   */
  .p-root.is-docked {
    /* Misma curva en tamaño y posición: si width rebota y left no, el
       canto acoplado se despega (el lado derecho queda a medio camino).
       El overshoot de `--ease-island` no puede bajar del dintel: el
       `min-width`/`min-height` inline es el piso de la pestaña en reposo. */
    transition:
      width var(--island-open-dur) var(--ease-island),
      height var(--island-open-dur) var(--ease-island),
      left var(--island-open-dur) var(--ease-island),
      top var(--island-open-dur) var(--ease-island);
  }

  /* El vuelo al hogar y al cursor. Solo mientras dura: si la transición
     quedara siempre puesta, cada reencuadre de la barra compacta —el timer
     tictaqueando, el badge de la cola— se arrastraría con cada tick.
     Va DESPUÉS de `.is-docked` para ganar el `transition` cuando coinciden:
     el vuelo mueve con transform (GPU), no con left/top. */
  .p-root.is-flying {
    transition: transform var(--flight-dur) var(--ease-smooth-out);
    will-change: transform;
  }

  /* Arrastrando no: la posición tiene que seguir al dedo sin inercia.
     Antes solo se apagaba en `.is-docked`; grabando (barra flotante) el
     timer reencuadraba y la transición peleaba con el gesto. */
  .p-root.is-dragging {
    transition: none;
    will-change: auto;
  }

  /*
   * Nacimiento desde el centro (boot): sin viaje desde la esquina.
   * `is-boot` apaga TODA transición para que los teleports de asentamiento
   * no se vean, y encoge+apaga la isla. Al salir, `is-birthing` anima solo
   * transform/opacity de la isla —GPU, sin layout— con la curva/duración de
   * la isla para que el tracker la siga cuadro a cuadro. Origen según el
   * canto: la gota cae del borde.
   */
  .p-root.is-boot {
    transition: none;
  }

  .p-root.is-boot .p-island {
    opacity: 0;
    transform: scale(0.55);
  }

  .p-root.is-birthing .p-island {
    transition:
      opacity var(--island-open-dur) var(--ease-liquid),
      transform var(--island-open-dur) var(--ease-liquid);
    will-change: opacity, transform;
  }

  /* Gota contra el cristal: 2–3 px de aplastón, sin rebote.
     `overflow: visible` para que el scale no recorte el redondeo del dintel. */
  .p-root.is-seating:not(.is-flying) {
    overflow: visible;
  }

  .p-root.is-seating:not(.is-flying)[data-edge="top"] {
    transform-origin: center top;
    animation: p-seat-y var(--duration-fast) var(--ease-smooth-out);
  }

  .p-root.is-seating:not(.is-flying)[data-edge="bottom"] {
    transform-origin: center bottom;
    animation: p-seat-y var(--duration-fast) var(--ease-smooth-out);
  }

  .p-root.is-seating:not(.is-flying)[data-edge="left"] {
    transform-origin: left center;
    animation: p-seat-x var(--duration-fast) var(--ease-smooth-out);
  }

  .p-root.is-seating:not(.is-flying)[data-edge="right"] {
    transform-origin: right center;
    animation: p-seat-x var(--duration-fast) var(--ease-smooth-out);
  }

  @keyframes p-seat-y {
    0%,
    100% {
      transform: scaleY(1);
    }

    38% {
      transform: scaleY(0.86);
    }
  }

  @keyframes p-seat-x {
    0%,
    100% {
      transform: scaleX(1);
    }

    38% {
      transform: scaleX(0.86);
    }
  }

  /*
   * Rebote del despegue: el panel se fue y la isla queda liviana. Aplastón
   * (suelta el peso) + estirón + asentamiento, con el origen clavado en el
   * canto. Va sobre `.p-island-body` y no sobre el root: la caja ya está
   * corriendo su transición de encogimiento y no la quiero pisar. El delay
   * deja que el colapso (`--island-open-dur`) termine antes del aplastón.
   */
  .p-island.is-detach-bounce .p-island-body {
    will-change: transform;
  }

  /*
   * Vista previa del imán: la pestaña se hincha hacia afuera del canto para
   * recibir el panel. Anuncia lo que hará el soltar; no decide nada.
   */
  .p-island-body {
    transition: transform var(--duration-medium) var(--ease-smooth-out);
  }

  .p-root[data-edge="top"] .p-island.is-retach-cue .p-island-body {
    transform-origin: center top;
    transform: scale(1.1, 1.2);
  }

  .p-root[data-edge="bottom"] .p-island.is-retach-cue .p-island-body {
    transform-origin: center bottom;
    transform: scale(1.1, 1.2);
  }

  .p-root[data-edge="left"] .p-island.is-retach-cue .p-island-body {
    transform-origin: left center;
    transform: scale(1.2, 1.1);
  }

  .p-root[data-edge="right"] .p-island.is-retach-cue .p-island-body {
    transform-origin: right center;
    transform: scale(1.2, 1.1);
  }

  .p-root[data-edge="top"] .p-island.is-detach-bounce .p-island-body {
    transform-origin: center top;
    animation: island-detach-y 360ms var(--ease-liquid) 200ms backwards;
  }

  .p-root[data-edge="bottom"] .p-island.is-detach-bounce .p-island-body {
    transform-origin: center bottom;
    animation: island-detach-y 360ms var(--ease-liquid) 200ms backwards;
  }

  .p-root[data-edge="left"] .p-island.is-detach-bounce .p-island-body {
    transform-origin: left center;
    animation: island-detach-x 360ms var(--ease-liquid) 200ms backwards;
  }

  .p-root[data-edge="right"] .p-island.is-detach-bounce .p-island-body {
    transform-origin: right center;
    animation: island-detach-x 360ms var(--ease-liquid) 200ms backwards;
  }

  @keyframes island-detach-y {
    0%,
    100% {
      transform: scaleY(1);
    }

    30% {
      transform: scaleY(0.9);
    }

    62% {
      transform: scaleY(1.05);
    }

    84% {
      transform: scaleY(0.99);
    }
  }

  @keyframes island-detach-x {
    0%,
    100% {
      transform: scaleX(1);
    }

    30% {
      transform: scaleX(0.9);
    }

    62% {
      transform: scaleX(1.05);
    }

    84% {
      transform: scaleX(0.99);
    }
  }

  /* Acoplada, la barra normal no se muestra: la isla la reemplaza entera.
     `opacity` y no `display` para que el stack siga existiendo y midiéndose
     —el resto del componente cuenta con sus rects—. */
  .p-root.is-docked .p-stack {
    opacity: 0;
    pointer-events: none;
  }

  /* La isla llena la caja. Como la caja es la que cambia de tamaño y esto la
     sigue, medir esto da una silueta que se estira con la transición en vez
     de saltar de pestaña a tira. */
  .p-island {
    position: absolute;
    z-index: 1;
    inset: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--rec-drop-gap);
  }

  .p-root[data-edge="top"] .p-island {
    top: 0;
    flex-direction: column;
    transform-origin: center top;
  }

  .p-root[data-edge="bottom"] .p-island {
    bottom: 0;
    flex-direction: column-reverse;
    transform-origin: center bottom;
  }

  .p-root[data-edge="left"] .p-island {
    left: 0;
    flex-direction: row;
    transform-origin: left center;
  }

  .p-root[data-edge="right"] .p-island {
    right: 0;
    flex-direction: row-reverse;
    transform-origin: right center;
  }

  .p-island-body {
    position: relative;
    display: grid;
    flex: 1 1 auto;
    align-self: stretch;
    min-width: 0;
    min-height: 0;
    place-items: center;
  }

  /*
   * Fila consola + panel.
   *
   * Cerrada es `display: contents`: la cara agents vuelve a ser hija directa
   * del body y el layout no cambia. Abierta (`is-side`) es una fila de verdad:
   * la consola conserva su ancho y el panel entra al costado, en flujo, en vez
   * de montarse encima. El lado pegado al canto lo clava el pivote de la caja
   * (ver `pivotFor`), así que la consola no se corre al abrir ni al cerrar.
   */
  .p-row {
    display: contents;
  }

  .p-row.is-side {
    display: flex;
    flex: 1 1 auto;
    align-items: stretch;
    justify-content: flex-start;

    /* Sin hueco: consola y panel son UNA superficie, no dos tarjetas. */
    gap: 0;
    min-width: 0;
    min-height: 0;
  }

  /* Canto derecho: el panel aparece del otro lado de la consola. */
  .p-root[data-edge="right"] .p-row.is-side {
    flex-direction: row-reverse;
  }

  .p-island-skin {
    position: absolute;
    inset: 0;
    display: block;
    border-radius: 999px;
    pointer-events: none;
  }

  /* El redondeo va al escritorio, no al bisel: lados derechos al canto. */
  .p-root[data-edge="top"] .p-island-skin {
    border-radius: 0 0 var(--island-clip-r, 22px) var(--island-clip-r, 22px);
  }

  .p-root[data-edge="bottom"] .p-island-skin {
    border-radius: var(--island-clip-r, 22px) var(--island-clip-r, 22px) 0 0;
  }

  .p-root[data-edge="left"] .p-island-skin {
    border-radius: 0 999px 999px 0;
  }

  .p-root[data-edge="right"] .p-island-skin {
    border-radius: 999px 0 0 999px;
  }

  /*
   * Lo que se ve con la pestaña CERRADA, a lo largo del borde: la marca y,
   * si hay algo que decir, el aviso al lado. Antes el aviso iba en `inset: 0`
   * y tapaba la marca entera; ahora comparten la fila y la caja se alarga.
   */
  .p-island-along {
    position: absolute;
    inset: 0;
    z-index: 2;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--island-gap);
    pointer-events: none;
    opacity: 1;
    transition: opacity var(--island-open-dur) var(--ease-liquid);
  }

  /* Pegada al canto, no al centro de la caja: al cerrar el clipboard el
     logo se quedaba en el medio del panel y subía con el encogimiento. */
  .p-root[data-edge="top"] .p-island-along {
    bottom: auto;
    height: var(--face-tab-h);
  }

  .p-root[data-edge="bottom"] .p-island-along {
    top: auto;
    bottom: 0;
    height: var(--face-tab-h);
  }

  .p-root[data-edge="left"] .p-island-along {
    top: 0;
    right: auto;
    bottom: 0;
    height: auto;
    width: var(--face-tab-h);
  }

  .p-root[data-edge="right"] .p-island-along {
    inset: 0 0 0 auto;
    height: auto;
    width: var(--face-tab-h);
  }

  /*
   * Con el panel al costado, la banda de la pestaña se limita a la consola:
   * así la marca queda exactamente donde estaba en vez de centrarse sobre el
   * blob entero. En laterales la consola ya ocupa todo el alto: no hay nada
   * que atar.
   */
  .p-root[data-edge="top"] .p-island.has-side .p-island-along,
  .p-root[data-edge="bottom"] .p-island.has-side .p-island-along {
    right: auto;
    left: 0;
    width: var(--face-agents-w);
  }

  .p-island-along > * {
    pointer-events: auto;
  }

  .p-island-along.is-column {
    flex-direction: column;
  }

  .p-island-along.is-hidden {
    opacity: 0;
    pointer-events: none;
  }

  /*
   * La marca es el control de lo que esté corriendo: grabando o dictando lo
   * para; en reposo el clic no hace nada (la rueda va por hover o atajo).
   * Sigue siendo `<button>` por el foco y el `aria-label`.
   */
  .p-island-mark {
    position: relative;
    z-index: 1;
    display: grid;
    flex: 0 0 auto;
    border: 0;
    padding: 0;
    border-radius: 999px;
    background: transparent;
    place-items: center;
    overflow: visible;
    color: var(--text);
    line-height: 0;
    cursor: pointer;
    transition: transform var(--duration-quick) var(--ease-smooth-out);
  }

  .p-island-mark:active:not(:disabled) {
    transform: scale(0.96);
  }

  .p-island-mark:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .p-island-mark:disabled {
    cursor: default;
    opacity: 0.55;
  }

  /*
   * Grab del panel: la barrita entre el notch y el contenido. Clic despega;
   * el gesto largo no hace nada — despegar es un clic, mover el float es
   * tarea de su header.
   */
  .p-face-grab {
    display: flex;
    flex: 0 0 auto;
    justify-content: center;
  }

  .p-face-grab-btn {
    display: grid;
    place-items: center;
    width: 2.75rem;
    height: 0.95rem;
    border: 0;
    padding: 0;
    border-radius: 999px;
    background: transparent;
    cursor: grab;
    transform: scale(0.96);
    transition: transform var(--duration-quick) var(--ease-smooth-out);
  }

  .p-face-grab-btn:active {
    transform: scale(0.9);
  }

  .p-face-grab-btn:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .p-face-grab-bar {
    width: 2rem;
    height: 3px;
    border-radius: 999px;
    background: color-mix(in sRGB, var(--rb-text) 24%, transparent);
    transition: background var(--duration-quick) var(--ease-smooth-out);
  }

  .p-face-grab-btn:hover .p-face-grab-bar {
    background: color-mix(in sRGB, var(--rb-text) 55%, transparent);
  }

  @keyframes p-island-rec-pulse {
    0%,
    100% {
      opacity: 0.55;
    }

    50% {
      opacity: 1;
    }
  }

  .p-island-tools {
    display: flex;
    z-index: 1;
    flex-direction: row;
    gap: var(--island-gap);
  }

  .p-island-tools.is-column {
    flex-direction: column;
  }

  .p-island-tools.is-column .p-island-tool {
    --island-bx: 0px;
    --island-by: var(--island-bunch);
  }

  /*
   * La tira no aparece: se SEPARA. Y al cerrarse, se junta.
   *
   * Es una TRANSICIÓN y no una animación justamente por eso: una animación
   * corre en un solo sentido, y el cierre quedaba de golpe. Con el estado de
   * reposo puesto acá y el abierto en `.is-open`, el mismo tramo se recorre en
   * los dos sentidos sin describirlo dos veces.
   *
   * Cerradas, las gotas se amontonan hacia el centro y encogen: a esa distancia
   * el `smin` las funde y se leen como un solo cuerpo. Abiertas quedan a
   * `--island-gap` (6 px), todavía muy por debajo de REACH, así que siguen
   * fundidas pero con una cintura entre iconos. Lo que se ve moverse es ese
   * cuello estirándose y adelgazando.
   *
   * Nada de esto se dibuja. Las gotas son estos mismos botones, y el `tracker`
   * los mide **con su transform**, así que el campo sigue la transición cuadro
   * a cuadro. La opacidad solo afecta al glifo: la forma sale del rect, y el
   * rect no la mira.
   *
   * El escalonado va por `--s` (distancia al centro, no el índice): saliendo
   * todas del medio, un barrido de punta a punta se leería al revés del
   * movimiento. Se calcula en JS porque `abs()` en CSS no está garantizado en
   * el WebView2 que nos toque.
   */
  .p-island-tool {
    /* Distancia de esta gota al centro de la tira: hacia ahí se amontona. */
    --island-slot: calc(var(--island-tool) + var(--island-gap));
    --island-bunch: calc(((var(--n) - 1) / 2 - var(--i)) * var(--island-slot));
    --island-bx: var(--island-bunch);
    --island-by: 0px;

    display: grid;
    position: relative;
    width: var(--island-tool);
    height: var(--island-tool);
    border: 0;
    border-radius: 999px;
    overflow: visible;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    opacity: 0;
    place-items: center;

    /* Reposo = cerrada. El apretón no es total: dejándolas repartidas en un
       tramo corto, el cuerpo fundido queda parecido a la pestaña y el relevo
       entre gotas y silueta de pestaña no se nota. */
    transform: translate(
        calc(var(--island-bx) * var(--island-shut-squeeze) + var(--island-from-x, 0px)),
        calc(var(--island-by) * var(--island-shut-squeeze) + var(--island-from-y, 0px))
      )
      scale(var(--island-shut-scale));
    transition:
      transform var(--island-open-dur) var(--ease-liquid),
      opacity var(--island-open-dur) var(--ease-liquid),
      background var(--duration-quick) var(--ease-smooth-out),
      color var(--duration-quick) var(--ease-smooth-out);
    transition-delay: calc(var(--s, 0) * var(--island-stagger));
  }

  .p-island-tools.is-open .p-island-tool {
    opacity: 1;
    transform: none;
  }

  /*
   * Re-deal de página: las gotas vuelven a entrar con el escalonado de `--s`.
   * Solo opacidad del glifo; la forma la sigue midiendo el tracker del rect.
   */
  .p-island-tools.is-swapping.is-open .p-island-tool {
    animation: island-tool-swap var(--island-open-dur) var(--ease-liquid) backwards;
    animation-delay: calc(var(--s, 0) * var(--island-stagger));
  }

  @keyframes island-tool-swap {
    from {
      opacity: 0;
    }
  }

  .p-island-tools.is-open .p-island-tool:hover:not(:disabled),
  .p-island-tools.is-open .p-island-tool:focus-visible {
    color: var(--text);
    transform: translate(
        calc(var(--island-from-x, 0px) * -0.55),
        calc(var(--island-from-y, 0px) * -0.55)
      )
      scale(1.14);
  }

  /* El visor es circular; el clic no. Un filete extra cubre el hueco entre
     gotas para que no se cuele al cuerpo de la isla. */
  .p-island-tools.is-open .p-island-tool::before {
    content: "";
    position: absolute;
    inset: -2px calc(var(--island-gap) / -2 - 2px);
  }

  .p-island-tools.is-open.is-column .p-island-tool::before {
    inset: calc(var(--island-gap) / -2 - 2px) -2px;
  }

  /* Cerrada, las gotas no deben robar el clic: lo toma la pestaña o el aviso. */
  .p-island-tools:not(.is-open) {
    pointer-events: none;
  }

  /*
   * Aviso pintado EN la pestaña. El stack flotante sigue apagado acoplado;
   * estos botones cubren el cuerpo: agente y/o update.
   */
  .p-island-cues {
    z-index: 2;
    display: flex;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    gap: 3px;
    pointer-events: none;
  }

  .p-root[data-edge="left"] .p-island-cues,
  .p-root[data-edge="right"] .p-island-cues {
    flex-direction: column;
  }

  .p-island-cues > * {
    pointer-events: auto;
  }

  /* La marca encabeza la tira: misma celda que una herramienta, sin el
     recuadro del icono — es la pill, no una herramienta más. */
  .p-island-tool-mark {
    color: var(--text);
    line-height: 0;
    overflow: visible;
  }

  .p-island-tool-update {
    color: var(--info);
  }

  /*
   * Lo que suena, en la pestaña cerrada: carátula y ecualizador en una celda
   * doble. Al costado se apilan, como el resto de los avisos.
   */
  .p-media-cue {
    position: relative;
    z-index: 2;
    display: flex;
    flex: 0 0 auto;
    align-items: center;
    height: var(--island-cue-btn);
    gap: 5px;
    border: 0;
    padding: 0 4px;
    background: transparent;
    color: var(--text);
    cursor: pointer;
    pointer-events: auto;
  }

  .p-root[data-edge="left"] .p-media-cue,
  .p-root[data-edge="right"] .p-media-cue {
    width: var(--island-cue-btn);
    height: auto;
    flex-direction: column;
    padding: 4px 0;
  }

  .p-media-cue:disabled {
    cursor: default;
  }

  .p-media-cover {
    width: var(--island-cue-mark);
    height: var(--island-cue-mark);
    flex: none;
    border-radius: 5px;
    object-fit: cover;
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--text) 12%, transparent);
  }

  /* Tres barras que laten a destiempo: se lee «suena» sin decir nada. */
  .p-media-eq {
    /* Tres ritmos que no coinciden: si latieran juntas se leería un bloque. */
    --eq-a: 0.9s;
    --eq-b: 0.7s;
    --eq-c: 1.1s;

    display: flex;
    width: 12px;
    height: 12px;
    flex: none;
    align-items: flex-end;
    justify-content: space-between;
  }

  .p-media-eq i {
    width: 2.5px;
    height: 100%;
    border-radius: 1px;
    background: currentColor;
    transform-origin: bottom;
    animation: p-media-eq var(--eq-a) var(--ease-smooth-out, ease-out) infinite alternate;
  }

  .p-media-eq i:nth-child(2) {
    animation-delay: -0.45s;
    animation-duration: var(--eq-b);
  }

  .p-media-eq i:nth-child(3) {
    animation-delay: -0.2s;
    animation-duration: var(--eq-c);
  }

  @keyframes p-media-eq {
    from {
      transform: scaleY(0.25);
    }

    to {
      transform: scaleY(1);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .p-media-eq i {
      animation: none;
      transform: scaleY(0.6);
    }
  }

  /* En pausa baja el tono: sonando es lo que importa ver de reojo. */
  .p-island-tool-media {
    color: var(--muted);
  }

  .p-island-tool-media.is-playing {
    color: var(--text);
  }

  .p-island-tool-update.is-ready {
    color: var(--ok);
  }

  .p-island-tool-update.is-busy {
    color: var(--muted);
  }

  /*
   * Cara expandida: tarjeta colgada de la pestaña con gap 0 —la skin llena
   * la caja entera, así que pestaña + tarjeta son un solo blob.
   * La tira no convive con la cara: se oculta (el hover la reabre al cerrar).
   */
  .p-island.is-face .p-island-body {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    justify-content: flex-start;
    padding-top: var(--face-tab-h);
  }

  .p-root[data-edge="bottom"] .p-island.is-face .p-island-body {
    flex-direction: column-reverse;
    padding-top: 0;
    padding-bottom: var(--face-tab-h);
  }

  .p-root[data-edge="left"] .p-island.is-face .p-island-body {
    flex-direction: row;
    padding-top: 0;
    padding-left: var(--face-tab-h);
  }

  .p-root[data-edge="right"] .p-island.is-face .p-island-body {
    flex-direction: row-reverse;
    padding-top: 0;
    padding-right: var(--face-tab-h);
  }

  .p-island.is-face .p-face {
    flex: 1 1 auto;
    min-height: 0;
  }

  /*
   * Vistazo dentro de la tira: la tira queda contra el canto y el vistazo
   * ocupa el tramo de adentro que la caja ganó (ver `contentFor`). Sin esto
   * la tira se centraría en la caja crecida y se despegaría del borde.
   */
  .p-root[data-edge="top"] .p-island.is-peek .p-island-body {
    place-items: start center;
  }

  .p-root[data-edge="bottom"] .p-island.is-peek .p-island-body {
    place-items: end center;
  }

  .p-root[data-edge="left"] .p-island.is-peek .p-island-body {
    place-items: center start;
  }

  .p-root[data-edge="right"] .p-island.is-peek .p-island-body {
    place-items: center end;
  }

  /*
   * Anclada, la tira queda a la misma distancia del canto que centrada en la
   * tira sola: la caja suma `PILL.pad` por lado y la isla pierde uno hacia
   * adentro, así que centrada queda a `pad / 2`. Sin esto, al soltar el
   * anclaje tras cerrar el vistazo, la fila entera saltaba 2 px.
   */
  .p-root[data-edge="top"] .p-island.is-peek .p-island-tools {
    margin-top: calc(var(--pill-pad) / 2);
  }

  .p-root[data-edge="bottom"] .p-island.is-peek .p-island-tools {
    margin-bottom: calc(var(--pill-pad) / 2);
  }

  .p-root[data-edge="left"] .p-island.is-peek .p-island-tools {
    margin-left: calc(var(--pill-pad) / 2);
  }

  .p-root[data-edge="right"] .p-island.is-peek .p-island-tools {
    margin-right: calc(var(--pill-pad) / 2);
  }

  .p-island-peek {
    position: absolute;
    z-index: 2;
  }

  /* Debajo de la tira ya corrida `pad / 2`: el sobrante reparte igual arriba y abajo. */
  .p-root[data-edge="top"] .p-island-peek {
    top: calc(var(--island-tool) + var(--pill-pad) / 2);
    left: 50%;
    transform: translateX(-50%);
  }

  .p-root[data-edge="bottom"] .p-island-peek {
    bottom: calc(var(--island-tool) + var(--pill-pad) / 2);
    left: 50%;
    transform: translateX(-50%);
  }

  .p-root[data-edge="left"] .p-island-peek {
    top: 50%;
    left: calc(var(--island-tool) + var(--pill-pad) / 2);
    transform: translateY(-50%);
  }

  .p-root[data-edge="right"] .p-island-peek {
    top: 50%;
    right: calc(var(--island-tool) + var(--pill-pad) / 2);
    transform: translateY(-50%);
  }

  .p-island.is-face .p-island-tools {
    display: none;
  }

  .p-root[data-edge="top"] .p-island.is-face .p-island-along,
  .p-root[data-edge="bottom"] .p-island.is-face .p-island-along {
    height: var(--face-tab-h);
  }

  .p-face {
    z-index: 2;
    display: flex;
    height: var(--face-card-h);
    flex-direction: column;
    justify-content: center;
    gap: 6px;
    padding: 10px 12px;
    pointer-events: auto;

    /* La cara se enfoca por código al abrir una herramienta (`tabindex="-1"`):
       WebKit dibujaría su anillo de foco (azul) alrededor del panel. No es una
       parada de tabulación; los controles de adentro conservan su foco real. */
    outline: none;
  }

  /* Cara ambiental: una tarjeta angosta, una fila por agente, sin duplicar el
     logo que ya vive en la banda de la pestaña. */
  .p-island.is-face .p-face[data-face="live"] {
    box-sizing: border-box;
    width: var(--face-live-w);
    height: calc(var(--island-live-rows) * var(--island-live-row));
    flex: 0 0 auto;
    min-width: 0;
    min-height: 0;
    align-self: center;
    justify-content: center;
    gap: 0;
    padding: 0 8px;
    overflow: hidden;
  }

  .p-root[data-edge="left"] .p-face[data-face="live"],
  .p-root[data-edge="right"] .p-face[data-face="live"] {
    height: auto;
    min-height: calc(var(--island-live-rows) * var(--island-live-row));
    align-self: stretch;
  }

  /*
   * Al costado: una columna angosta con el estado de cada agente y nada más.
   * El texto (qué hace, en qué carpeta) queda en el tooltip; el clic sigue
   * llevando a la consola, y un «listo» se descarta abriéndolo.
   */
  .p-root[data-edge="left"] .p-island.is-face .p-face[data-face="live"],
  .p-root[data-edge="right"] .p-island.is-face .p-face[data-face="live"] {
    width: var(--face-live-side-w);
    padding: 0;
  }

  .p-live-state {
    display: none;
  }

  .p-root[data-edge="left"] .p-face[data-face="live"] .p-live-state,
  .p-root[data-edge="right"] .p-face[data-face="live"] .p-live-state {
    display: grid;
    width: 100%;
    height: 100%;
    place-items: center;
    font-size: 0.8125rem;
    font-weight: 750;
  }

  .p-root[data-edge="left"] .p-face[data-face="live"] :is(.p-live-row-logos, .p-live-row-label, .p-live-act, .p-live-x),
  .p-root[data-edge="right"] .p-face[data-face="live"] :is(.p-live-row-logos, .p-live-row-label, .p-live-act, .p-live-x) {
    display: none;
  }

  .p-root[data-edge="left"] .p-face[data-face="live"] .p-live-row,
  .p-root[data-edge="right"] .p-face[data-face="live"] .p-live-row {
    justify-content: center;
    padding: 0;
  }

  /* Trabajando: un aro que gira. Sin texto, el movimiento es el mensaje. */
  .p-live-spin {
    width: 12px;
    height: 12px;
    box-sizing: border-box;
    border: 2px solid color-mix(in sRGB, currentColor 25%, transparent);
    border-top-color: currentColor;
    border-radius: 999px;
    animation: p-live-spin 0.8s linear infinite;
  }

  @keyframes p-live-spin {
    to {
      transform: rotate(1turn);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .p-live-spin {
      animation: none;
    }
  }

  .p-live-list {
    display: flex;
    width: 100%;
    height: 100%;
    flex-direction: column;
    align-items: stretch;
    justify-content: center;
    gap: 0;
    min-height: 0;
    overflow: hidden;
  }

  /* Fila + su «descartar»: la X no puede ir DENTRO del botón de la fila. */
  .p-live-item {
    position: relative;
    display: flex;
    min-width: 0;
    height: var(--island-live-row);
    flex: 0 0 var(--island-live-row);
  }

  /*
   * Sin fondo en reposo, y el del hover concéntrico con la isla.
   *
   * Eran bloques tintados de radio 8, pegados uno con otro, dentro de una
   * pill de radio 22: los radios no acompañaban y el aviso se leía como cajas
   * apiladas sobre la pill. El estado ya lo dicen el color del texto y el
   * logo; el fondo aparece al apuntar, 2 px más adentro que la fila para dejar
   * aire entre una y otra, con el radio de la isla menos su margen.
   */
  .p-live-row {
    position: relative;
    display: flex;
    width: 100%;
    min-width: 0;
    height: calc(100% - 4px);
    flex: 1 1 auto;
    align-items: center;
    align-self: center;
    gap: 8px;
    border: 0;
    border-radius: calc(var(--island-clip-r, 22px) - 8px);
    padding: 0 8px;
    background: transparent;
    color: var(--muted);
    text-align: left;
    cursor: pointer;
    transition:
      background var(--duration-quick) var(--ease-smooth-out),
      color var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .p-live-row:hover,
  .p-live-row:focus-visible {
    background: color-mix(in sRGB, var(--text) 10%, transparent);
    color: var(--text);
  }

  .p-live-row:active {
    transform: scale(0.985);
  }

  .p-live-row:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .p-live-row.is-waiting {
    color: var(--rec);
  }

  .p-live-row.is-working {
    color: var(--warn);
    animation: p-agent-pulse 1.8s var(--ease-liquid) infinite;
  }

  .p-live-row.is-ready {
    color: var(--ok);
  }

  .p-live-row.is-count {
    color: var(--accent);
  }

  .p-live-row-logos {
    display: flex;
    width: 20px;
    height: 20px;
    flex: 0 0 20px;
    align-items: center;
    justify-content: center;
    gap: 2px;
    overflow: hidden;
  }

  /* Con la X a la vista, el texto le deja lugar. */
  .p-live-item.has-x:hover .p-live-row,
  .p-live-item.has-x:focus-within .p-live-row {
    padding-right: 26px;
  }

  .p-live-x {
    position: absolute;
    top: 50%;
    right: 4px;
    display: grid;
    width: 20px;
    height: 20px;
    border: 0;
    border-radius: 999px;
    padding: 0;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    opacity: 0;
    place-items: center;
    transform: translateY(-50%);
    transition:
      opacity var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      color var(--duration-quick) var(--ease-smooth-out);
  }

  .p-live-item:hover .p-live-x,
  .p-live-x:focus-visible {
    opacity: 1;
  }

  .p-live-x:hover {
    background: color-mix(in sRGB, var(--text) 12%, transparent);
    color: var(--text);
  }

  .p-live-x:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  /* Con animación de actividad, la fila deja de latir: una cosa moviéndose alcanza. */
  .p-live-row.is-working:has(.p-live-act) {
    animation: none;
  }

  /*
   * La actividad, en 14 × 10 px al final de la fila. Tres <i> que cada estado
   * usa a su modo: puntos que respiran (pensar), que saltan (escribir), un
   * trazo que crece (editar), una barra que barre (leer), un punto que orbita
   * (buscar), un cursor que parpadea (ejecutar), dos puntos que se pasan la
   * posta (delegar) y un aro que gira (otra herramienta).
   */
  .p-live-act {
    position: relative;
    display: flex;
    width: 14px;
    height: 10px;
    flex: 0 0 14px;
    align-items: center;
    justify-content: space-between;
    margin-left: auto;
  }

  .p-live-act i {
    display: block;
    width: 3px;
    height: 3px;
    border-radius: 999px;
    background: currentColor;
  }

  .p-live-act[data-act="thinking"] i {
    animation: p-act-breathe 1.2s var(--ease-smooth-out) infinite;
  }

  .p-live-act[data-act="writing"] i {
    animation: p-act-hop 0.9s var(--ease-smooth-out) infinite;
  }

  .p-live-act[data-act="thinking"] i:nth-child(2),
  .p-live-act[data-act="writing"] i:nth-child(2) {
    animation-delay: 0.15s;
  }

  .p-live-act[data-act="thinking"] i:nth-child(3),
  .p-live-act[data-act="writing"] i:nth-child(3) {
    animation-delay: 0.3s;
  }

  .p-live-act[data-act="editing"] i,
  .p-live-act[data-act="reading"] i,
  .p-live-act[data-act="running"] i,
  .p-live-act[data-act="searching"] i,
  .p-live-act[data-act="tool"] i {
    display: none;
  }

  .p-live-act[data-act="editing"] i:first-child {
    display: block;
    position: absolute;
    bottom: 1px;
    left: 0;
    width: 0;
    height: 2px;
    animation: p-act-stroke 1.1s var(--ease-smooth-out) infinite;
  }

  .p-live-act[data-act="reading"] i:first-child {
    display: block;
    position: absolute;
    left: 0;
    width: 2px;
    height: 10px;
    animation: p-act-scan 1.1s var(--ease-liquid) infinite alternate;
  }

  .p-live-act[data-act="running"] i:first-child {
    display: block;
    width: 6px;
    height: 9px;
    border-radius: 1px;
    animation: p-act-blink 0.9s steps(2, jump-none) infinite;
  }

  .p-live-act[data-act="searching"] {
    justify-content: center;
    animation: p-act-orbit 1s linear infinite;
  }

  .p-live-act[data-act="searching"] i:first-child {
    display: block;
    transform: translateX(4px);
  }

  .p-live-act[data-act="delegating"] i:nth-child(3) {
    display: none;
  }

  .p-live-act[data-act="delegating"] i:first-child {
    animation: p-act-pass 1s var(--ease-liquid) infinite alternate;
  }

  .p-live-act[data-act="delegating"] i:nth-child(2) {
    animation: p-act-pass 1s var(--ease-liquid) infinite alternate-reverse;
  }

  .p-live-act[data-act="tool"] i:first-child {
    display: block;
    width: 9px;
    height: 9px;
    margin: 0 auto;
    border: 1.5px solid color-mix(in sRGB, currentColor 30%, transparent);
    border-top-color: currentColor;
    background: transparent;
    animation: p-act-spin 0.9s linear infinite;
  }

  @keyframes p-act-breathe {
    0%,
    100% {
      opacity: 0.3;
      transform: scale(0.8);
    }

    50% {
      opacity: 1;
      transform: scale(1.1);
    }
  }

  @keyframes p-act-hop {
    0%,
    60%,
    100% {
      transform: translateY(0);
    }

    30% {
      transform: translateY(-3px);
    }
  }

  @keyframes p-act-stroke {
    0% {
      width: 0;
      opacity: 1;
    }

    70% {
      width: 14px;
      opacity: 1;
    }

    100% {
      width: 14px;
      opacity: 0;
    }
  }

  @keyframes p-act-scan {
    from {
      transform: translateX(0);
    }

    to {
      transform: translateX(12px);
    }
  }

  @keyframes p-act-blink {
    to {
      opacity: 0;
    }
  }

  @keyframes p-act-orbit {
    to {
      transform: rotate(360deg);
    }
  }

  @keyframes p-act-pass {
    from {
      transform: translateX(0);
    }

    to {
      transform: translateX(6px);
    }
  }

  @keyframes p-act-spin {
    to {
      transform: rotate(360deg);
    }
  }

  .p-live-row-label {
    min-width: 0;
    overflow: hidden;
    font-family: var(--font-sans);
    font-size: 0.6875rem;
    font-weight: 600;
    line-height: 1;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .p-face[data-face="clipboard"],
  .p-face[data-face="snippets"],
  .p-face[data-face="system"] {
    position: relative;
    height: var(--face-clip-h);
    min-height: 0;
    justify-content: stretch;
    gap: 0;
    padding: 4px 10px 0;
    overflow: hidden;
  }

  /*
   * Sistema pisa SOLO la medida: arriba de la lista lleva fila rápida,
   * pestañas, medidores, orden y filtro (~190 px fijos), y con el alto del
   * clipboard la lista quedaba en dos filas. Tiene que sumar exacto con
   * `islandSys*` (ver `pillStage`), o la caja y la cara se desalinean.
   *
   * Regla aparte a propósito: la de arriba es compartida, y cambiarla ahí
   * agrandaba también el historial y los textos, que se salían de su caja.
   */
  .p-face[data-face="system"] {
    width: var(--face-sys-w);
    height: var(--face-sys-h);
  }

  .p-face[data-face="agents"] {
    height: var(--face-agents-h);
    min-height: 0;
    justify-content: stretch;
    gap: 0;
    padding: 6px 8px 8px;
    overflow: hidden;
  }

  .p-face[data-face="agents"].is-stowed {
    position: fixed;
    left: -12000px;
    top: 0;
    width: var(--face-agents-w);
    height: var(--face-agents-h);
    pointer-events: none;
  }

  /*
   * Cara dictado: onda centrada mientras escucha; icono + estado
   * («Transcribiendo…», «Pegado», error) después. La onda crece desde el
   * centro —la pestaña de arriba ya ancla el gesto, no hace falta que «caiga».
   */
  .p-face[data-face="dictation"] {
    height: var(--face-dict-h, 72px);
    align-items: center;
    justify-content: center;
  }

  .p-face-wave {
    display: block;
    width: 8rem;
    padding: 0 2px;
  }

  .p-face-wave :global(.rb-wave) {
    flex: 1 1 auto;
    width: 100%;
    min-width: 0;
    align-items: center;
  }

  .p-face-wave :global(.rb-wave-voice) {
    gap: 1px;
    height: 0.65rem;
    justify-content: space-between;
  }

  .p-face-wave :global(.rb-wave-bar) {
    transform-origin: center;
  }

  .p-face-status {
    display: flex;
    max-width: 100%;
    align-items: center;
    gap: 8px;
    color: var(--muted);
  }

  .p-face-status.is-busy {
    color: var(--warn);
  }

  .p-face-status.is-ok {
    color: var(--ok);
  }

  .p-face-status.is-error {
    color: var(--rec);
  }

  .p-face-label {
    overflow: hidden;
    font-family: var(--font-sans);
    font-size: 0.6875rem;
    font-weight: 500;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /*
   * Panel del costado: clipboard / textos conviven con la consola dentro de
   * la isla, AL LADO y en flujo. Una sola superficie: sin fondo, anillo ni
   * sombra propios —el contenido va directo sobre la silueta líquida, como el
   * de la consola— y sin hueco con ella. Solo el padding interno para que no
   * toque el borde redondeado del blob.
   */
  .p-side {
    z-index: 3;
    display: flex;
    width: var(--face-clip-w, 280px);
    flex: 0 0 auto;
    align-self: stretch;
    flex-direction: column;
    min-height: 0;
    padding: 4px 10px 0;
    overflow: hidden;

    /* Entrada: desplazamiento corto desde el lado de la consola, con la
       familia de motion de la isla. El fade lo pone `opacityFade`. */
    animation: p-side-in var(--island-open-dur) var(--ease-liquid) backwards;
  }

  @keyframes p-side-in {
    from {
      transform: translateX(calc(var(--distance-micro, 4px) * -1));
    }
  }

  /* Canto derecho: el panel nace del otro lado de la consola. */
  .p-root[data-edge="right"] .p-side {
    animation-name: p-side-in-right;
  }

  @keyframes p-side-in-right {
    from {
      transform: translateX(var(--distance-micro, 4px));
    }
  }

  /* Con panel, la consola conserva su ancho: el ancho extra es del panel. */
  .p-row.is-side > .p-face[data-face="agents"] {
    flex: 0 0 auto;
    width: var(--face-agents-w);
  }

  .p-face-tabs {
    display: flex;
    flex: 0 0 auto;
    align-items: center;
    gap: 0.15rem;
    padding: 0 0.15rem 0.25rem;
  }

  .p-face-tab {
    border: 0;
    border-radius: 999px;
    padding: 0.18rem 0.55rem;
    background: transparent;
    color: var(--muted);
    font-size: 0.6875rem;
    font-weight: 600;
    cursor: pointer;
  }

  .p-face-tab.is-on {
    background: color-mix(in sRGB, var(--rb-text) 10%, transparent);
    color: var(--text);
  }

  .p-face-pane {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
  }

  .p-face-scratch {
    width: 100%;
    min-height: 0;
    flex: 1;
    resize: none;
    border: 0;
    border-radius: 0.55rem;
    padding: 0.45rem 0.5rem;
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
    color: var(--text);
    font: inherit;
    font-size: 0.75rem;
    line-height: 1.35;
    outline: none;
  }

  .p-face-head {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text);
    font-size: 0.75rem;
    font-weight: 600;
    line-height: 1.2;
  }

  .p-face-desc {
    overflow: hidden;
    margin: 0;
    color: var(--muted);
    font-size: 0.75rem;
    line-height: 1.2;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .p-face-desc strong {
    color: var(--text);
    font-weight: 600;
  }

  .p-face-actions {
    display: flex;
    gap: 8px;
  }

  .p-face-btn {
    position: relative;
    display: inline-flex;
    height: 2rem;
    flex: 1 1 0;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: 999px;
    background: color-mix(in sRGB, var(--text) 8%, transparent);
    color: var(--text);
    font-size: 0.6875rem;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    cursor: pointer;
    transition: transform var(--duration-quick) var(--ease-smooth-out);
  }

  /* Hit ≥40px sin inflar la tarjeta visible (igual que la tarjeta auth). */
  .p-face-btn::after {
    content: "";
    position: absolute;
    inset-block: 50%;
    inset-inline: 0;
    height: 40px;
    transform: translateY(-50%);
  }

  /* Aprobar: tinta ok; rechazar: tinta rec con anillo. Igual que la tarjeta. */
  .p-face-btn.is-allow {
    background: color-mix(in sRGB, var(--ok) 18%, transparent);
    color: var(--ok);
  }

  .p-face-btn.is-deny {
    background: color-mix(in sRGB, var(--rec) 18%, transparent);
    color: var(--rec);
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--rec) 42%, transparent);
  }

  .p-face-btn:active:not(:disabled) {
    transform: scale(0.96);
  }

  .p-face-btn:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .p-face-btn:disabled {
    opacity: 0.45;
    cursor: default;
  }

  /* Flotando, la marca encabeza los tres estados y es su control. */
  .p-mark.is-lead {
    flex-shrink: 0;
    border: 0;
    padding: 0;
    border-radius: 999px;
    background: transparent;
    cursor: pointer;
    transition: transform var(--duration-quick) var(--ease-smooth-out);
  }

  .p-mark.is-lead:active:not(:disabled) {
    transform: scale(0.96);
  }

  .p-mark.is-lead:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .p-mark.is-lead:disabled {
    cursor: default;
    opacity: 0.55;
  }

  .p-island-cue.p-agent,
  .p-island-cue.p-agent.is-waiting,
  .p-island-cue.p-agent.is-working,
  .p-island-cue.p-agent.is-ready,
  .p-island-cue.p-agent.is-count,
  .p-island-cue.p-agent.is-dock {
    position: absolute;
    inset: 0;
    z-index: 2;
    display: grid;
    min-height: 0;
    max-height: 100%;
    max-width: none;
    overflow: hidden;
    padding: 0;
    place-items: center;
    background: transparent;
    border-radius: 999px;
  }

  .p-island-cues .p-island-cue.p-agent {
    position: relative;
    inset: auto;
    flex: 0 0 auto;
    width: auto;
    height: var(--island-cue-btn);
    min-width: var(--island-cue-btn);
    padding: 0 4px;
  }

  .p-island-update .p-update-ico {
    width: var(--island-cue-mark);
    height: var(--island-cue-mark);
  }

  .p-island-update {
    position: absolute;
    inset: 0;
    z-index: 3;
    display: grid;
    place-items: center;
    min-width: 0;
    min-height: 0;
    width: auto;
    height: auto;
    padding: 0;
    border: 0;
    border-radius: 999px;
    background: transparent;
    color: var(--info);
    cursor: pointer;
  }

  .p-island-cues .p-island-update {
    position: relative;
    inset: auto;
    flex: 0 0 auto;
    width: var(--island-cue-btn);
    height: var(--island-cue-btn);
    background: transparent;
  }

  .p-island-update.is-ready {
    background: transparent;
    color: var(--ok);
  }

  .p-island-update.is-busy {
    background: transparent;
    color: var(--muted);
  }

  .p-island-update::after {
    content: none;
  }

  .p-root[data-edge="top"] .p-island-cue.p-agent {
    align-items: center;
    padding-top: 0;
  }

  .p-root[data-edge="bottom"] .p-island-cue.p-agent {
    align-items: center;
    padding-bottom: 0;
  }

  .p-island-cue.p-agent::after {
    content: none;
  }

  /* Trabajando/contestando: el icono de la pestaña late suave. El latido de la
     silueta lo lleva la piel (`liquid.breathe`); acá el aviso puntual. */
  .p-island-cue.is-working {
    animation: p-agent-pulse 1.8s var(--ease-liquid) infinite;
  }

  /*
   * Estados en la pestaña: espera y listo también se leen. Eran iguales al
   * dock apagado —solo el latido distinguía—. Misma lengua que el chip
   * flotante: alerta (lo que bloquea al agente: una pregunta/permiso) y ok
   * (lo terminado). La cápsula sirve para cualquier marca, también para el
   * logo genérico de un CLI sin logo propio.
   */
  .p-island-cues .p-island-cue.p-agent.is-waiting {
    background: color-mix(in sRGB, var(--rec) 16%, transparent);
    color: var(--rec);
  }

  .p-island-cues .p-island-cue.p-agent.is-ready {
    background: color-mix(in sRGB, var(--ok) 14%, transparent);
    color: var(--ok);
  }

  .p-island-cue-logo {
    display: grid;
    width: var(--island-cue-mark);
    height: var(--island-cue-mark);
    overflow: hidden;
    place-items: center;
    color: currentColor;
  }

  .p-island-cue-logos {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 3px;
    max-width: 100%;
    max-height: 100%;
    overflow: hidden;
  }

  .p-root[data-edge="left"] .p-island-cue-logos,
  .p-root[data-edge="right"] .p-island-cue-logos {
    flex-direction: column;
  }

  /* "+3": los agentes que no entran. Del alto de un logo, para que la
     celda mida lo mismo que las otras y la pestaña no salte. */
  .p-island-cue-more {
    display: grid;
    min-width: var(--island-cue-mark);
    height: var(--island-cue-mark);
    place-items: center;
    color: var(--muted);
    font-size: 0.625rem;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }

  .p-island-cue.is-working .p-island-cue-logo {
    animation: none;
  }

  .p-island-cue.is-dock:not(.is-waiting, .is-working, .is-ready, .is-count) {
    color: var(--muted);
  }

  .p-island-cue-mark {
    font-size: 0.75rem;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    line-height: 1;
  }

  /*
   * Aviso con texto (notch del eje y): el preview del agente o el estado.
   * El ancho lo reservó `islandCueLong` — el texto solo recorta con elipsis
   * dentro de ese tramo, no re-mide la pestaña.
   */
  .p-island-cues .p-island-cue.p-agent.is-msg {
    display: flex;
    gap: 0.25rem;
  }

  .p-island-cue-msg {
    overflow: hidden;
    max-width: calc(var(--island-cue-msg-w, 96px) - 0.25rem);
    font-family: var(--font-sans);
    font-size: 0.625rem;
    font-weight: 650;
    letter-spacing: 0.02em;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /*
   * Al costado el aviso se apila a lo largo del canto: los logos en columna
   * y el texto girado. El botón pasa a medir su ancho fijo y el alto que pida
   * el contenido; con el alto fijo de la fila, varios logos apilados quedaban
   * recortados.
   */
  .p-root[data-edge="left"] .p-island-cues .p-island-cue.p-agent,
  .p-root[data-edge="right"] .p-island-cues .p-island-cue.p-agent {
    width: var(--island-cue-btn);
    height: auto;
    min-height: var(--island-cue-btn);
    padding: 4px 0;
  }

  .p-root[data-edge="left"] .p-island-cues .p-island-cue.p-agent.is-msg,
  .p-root[data-edge="right"] .p-island-cues .p-island-cue.p-agent.is-msg {
    flex-direction: column;
    align-items: center;
  }

  .p-root[data-edge="left"] .p-island-cue-msg,
  .p-root[data-edge="right"] .p-island-cue-msg {
    max-width: none;
    max-height: calc(var(--island-cue-msg-w, 96px) - 0.25rem);
    writing-mode: vertical-rl;
  }

  /*
   * La letra colgando de la pestaña, a lo Apple Music: el verso actual grande
   * y blanco, el siguiente del mismo cuerpo pero apagado. Al cambiar, el nuevo
   * sube desde abajo.
   */
  .p-island-lyrics {
    position: absolute;
    z-index: 2;
    left: 0;
    right: 0;
    top: var(--face-tab-h);
    bottom: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 2px;
    border: 0;
    padding: 0 18px 8px;
    background: transparent;
    color: var(--text);
    font-family: var(--font-sans);
    font-size: 0.9375rem;
    font-weight: 700;
    letter-spacing: -0.01em;
    line-height: 1.25;
    text-align: center;
    cursor: pointer;
    pointer-events: auto;
  }

  .p-root[data-edge="bottom"] .p-island-lyrics {
    top: 0;
    bottom: var(--face-tab-h);
    padding: 8px 18px 0;
  }

  .p-island-lyrics:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .p-lyric-now,
  .p-lyric-next {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Igual que en el vistazo: el verso actual parte en dos renglones en vez de
     cortarse; el alto de la caja (`islandLyricHangH`) ya cuenta con ellos. */
  .p-lyric-now {
    display: flex;
    min-height: 2.5em;
    align-items: center;
    justify-content: center;
    white-space: normal;
    text-wrap: balance;
    animation: p-lyric-up var(--duration-slow, 200ms) var(--ease-smooth-out, ease-out);
  }

  .p-lyric-next {
    min-height: 1.25em;
    color: color-mix(in sRGB, var(--text) 32%, transparent);
    animation: p-lyric-in var(--duration-slow, 200ms) var(--ease-smooth-out, ease-out);
  }

  @keyframes p-lyric-up {
    from {
      opacity: 0.32;
      transform: translateY(0.6em);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .p-lyric-now,
    .p-lyric-next {
      animation: none;
    }
  }

  /* Al costado, la letra tiene su propio tramo en la fila y entra con un fundido. */
  .p-island-cue-msg.is-lyric {
    max-width: calc(var(--island-lyric-w, 200px) - 0.25rem);
    font-weight: 600;
    letter-spacing: 0;
    animation: p-lyric-in var(--duration-slow, 200ms) var(--ease-smooth-out, ease-out);
  }

  .p-root[data-edge="left"] .p-island-cue-msg.is-lyric,
  .p-root[data-edge="right"] .p-island-cue-msg.is-lyric {
    max-width: none;
    max-height: calc(var(--island-lyric-w, 200px) - 0.25rem);
  }

  @keyframes p-lyric-in {
    from {
      opacity: 0;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .p-island-cue-msg.is-lyric {
      animation: none;
    }
  }

  /* A la izquierda se lee de abajo hacia arriba, mirando hacia la pantalla. */
  .p-root[data-edge="left"] .p-island-cue-msg {
    transform: rotate(180deg);
  }

  .p-island-agent-badge {
    position: absolute;
    top: 2px;
    right: 2px;
    width: 7px;
    height: 7px;
    border-radius: 999px;
    pointer-events: none;
    background: currentColor;
    color: var(--accent);
  }

  .p-island-agent-badge.is-waiting {
    color: var(--rec);
  }

  .p-island-agent-badge.is-working {
    color: var(--muted);
  }

  .p-island-agent-badge.is-ready {
    color: var(--ok);
  }

  .p-island-agent-badge.is-dock:not(.is-waiting, .is-working, .is-ready, .is-count) {
    color: var(--muted);
  }

  .p-island-agent-badge.is-label,
  .p-island-agent-badge.is-count {
    display: grid;
    width: auto;
    min-width: 11px;
    height: 11px;
    padding: 0 3px;
    place-items: center;
    background: color-mix(in sRGB, currentColor 22%, transparent);
    color: inherit;
    font-size: 8px;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    line-height: 1;
  }

  .p-island-agent-badge.is-waiting.is-label {
    color: var(--rec);
  }

  .p-island-agent-badge.is-count {
    color: var(--accent);
  }

  /* De dónde nace cada uno: siempre desde el lado por el que está acoplada. */
  .p-root[data-edge="bottom"] .p-island-tool {
    --island-from-y: var(--island-rise);
  }

  .p-root[data-edge="top"] .p-island-tool {
    --island-from-y: calc(var(--island-rise) * -1);
  }

  .p-root[data-edge="right"] .p-island-tool {
    --island-from-x: var(--island-rise);
  }

  .p-root[data-edge="left"] .p-island-tool {
    --island-from-x: calc(var(--island-rise) * -1);
  }

  /* Cierre acelerado: al elegir herramienta la rueda ya cumplió su función. */
  .p-root.is-quick {
    --morph-close-dur: var(--morph-quick-dur);
    --morph-fade-dur: var(--morph-quick-dur);
  }

  .p-stack {
    position: relative;
    display: flex;
    width: 100%;
    height: 100%;
    min-height: 0;
    flex-direction: column;
    visibility: visible;
    transition:
      opacity var(--morph-fade-dur) var(--morph-close-ease),
      visibility 0s linear 0s;
  }

  .p-stack.is-dim {
    /* opacity sola no basta en WebView: con filter:blur el trazo de la «a»
       del stack seguía pintando un fantasma arriba-izquierda del root grande
       aunque opacity fuera 0. visibility + sin blur + sin AticMark en el DOM. */
    opacity: 0;
    visibility: hidden;
    pointer-events: none;

    /* Cada estado declara la curva de su dirección; si no, el chrome se iría
       con la del cierre y rompería el espejo. */
    transition:
      opacity var(--morph-fade-dur) var(--morph-ease),
      visibility 0s linear var(--morph-fade-dur);
  }

  /* ─── Rueda ─────────────────────────────────────────────────────────── */

  /* Tamaño fijo y centrado: la rueda mide siempre lo mismo, así las posiciones
     de los nodos no se recalculan durante el morph. */
  .p-wheel {
    position: absolute;
    top: 50%;
    left: 50%;
    z-index: 2;
    display: grid;
    width: 232px;
    height: 232px;
    margin: -116px 0 0 -116px;
    place-items: center;
    overflow: visible;
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--morph-fade-dur) var(--morph-close-ease);
  }

  .p-wheel.is-open {
    opacity: 1;
    pointer-events: auto;
    transition: opacity var(--morph-fade-dur) var(--morph-ease);
  }

  /* Brota del canto: el núcleo queda donde estaba la marca de la isla. */
  .p-root[data-bloom="top"] .p-wheel {
    top: 20px;
    left: 50%;
    margin: -116px 0 0 -116px;
  }

  .p-root[data-bloom="bottom"] .p-wheel {
    top: auto;
    bottom: 20px;
    left: 50%;
    margin: 0 0 -116px -116px;
  }

  .p-root[data-bloom="left"] .p-wheel {
    top: 50%;
    left: 20px;
    margin: -116px 0 0 -116px;
  }

  .p-root[data-bloom="right"] .p-wheel {
    top: 50%;
    left: auto;
    right: 20px;
    margin: -116px -116px 0 0;
  }

  /* El disco de fondo se fue: ahora la superficie la ponen el núcleo y las
     gotas de `ParticleWheel`, que escalan y viajan por su cuenta. La
     marca del centro sigue sin escalar — es el punto fijo del morph. */

  /* ─── Cuerpo líquido (barra) ─────────────────────────────────────────── */
  .p-liquid {
    /* `--goo-grow` viene de `app.css`. Sin compensarlo, el disco de reposo
       saldría más grande que `PILL.bar` y se comería el respiro que
       `PILL.pad` deja dentro de la ventana. */
    position: relative;
    display: flex;
    width: max-content;
    max-width: 100%;
    min-height: 0;
    flex-direction: column;
  }

  /*
   * La piel: solo REFERENCIAS DE MEDIDA.
   *
   * Estos `<i>` no se pintan. El CSS de abajo sigue decidiendo su geometría y
   * sus animaciones —cómo llega la gota—, la pill los mide, y el contorno lo
   * traza el campo del overlay a partir de esos rectángulos.
   */
  .p-skin {
    /*
     * El descuento del engorde queda en cero.
     *
     * Las medidas de acá abajo están escritas como `var(--pill-bar) - goo-grow * 2`
     * porque el endurecido del filtro devolvía 2.8 px por lado. El contorno
     * trazado NO engorda: pasa por la geometría pedida. Con el descuento
     * puesto, el disco se medía más chico que `PILL.bar`.
     *
     * Se apaga con la variable y no regla por regla: así vale para las dos
     * siluetas y para el `inset` de una sola vez.
     */
    --goo-grow: 0px;

    position: absolute;
    z-index: 0;
    inset: 0;
    display: flex;
    flex-direction: inherit;
    pointer-events: none;
  }

  /* Consola al inicio: el disco de referencia vive bajo la «a» (derecha). */
  .p-skin.is-console-start {
    align-items: flex-end;
  }

  .p-skin > i {
    display: block;
    background: transparent;
  }

  /* La pastilla de siempre. El alto descuenta lo que el filtro va a devolver
     por los dos lados, así que el borde final cae exactamente en el disco. */
  .p-skin-bar {
    height: calc(var(--pill-bar) - var(--goo-grow) * 2);
    width: calc(var(--pill-bar) - var(--goo-grow) * 2);
    flex-shrink: 0;
    border-radius: 999px;
  }

  /*
   * La barra crece absorbiendo lo que llega.
   *
   * La gota aparece chica contra el borde derecho —ya separada del disco, el
   * hueco supera los ~10.3 px de alcance del filtro— y se expande hacia él
   * hasta fundirse.
   * Se anima con `left/right/top/bottom` y no con `transform`: escalar una
   * pastilla le achata los remates, y justo al principio, que es cuando se ve
   * sola, quedaría una astilla en vez de una gota.
   *
   * Termina TAPANDO al disco (`inset: 0`) y no pegada a él: dos formas que
   * apenas se tocan dejan un pellizco cóncavo en la unión, muy bien mientras
   * la cosa se mueve pero no en reposo — ahí tiene que ser la pastilla limpia.
   */
  .p-skin-tail {
    position: absolute;
    inset: 7px 4px 7px calc(100% - 28px);
    border-radius: 999px;
    transition: inset var(--panel-dur) var(--morph-ease);
  }

  /* Consola al inicio (cerca del borde derecho): la gota llega desde la izquierda. */
  .p-skin-tail.is-from-start {
    inset: 7px calc(100% - 28px) 7px 4px;
  }

  .p-skin-tail.is-in,
  .p-skin-tail.is-from-start.is-in {
    inset: 0;
  }

  /*
   * La gota del dictado: cuelga de la barra flotante con la onda adentro.
   *
   * Cae desde la pill (mismo gesto que la gota de la rueda: encoge y baja) y
   * mide lo mismo que el disco. El cuello lo publica `skinShapes` como una
   * cápsula: con el blend de render en 0 el campo no funde huecos solo.
   * `pointer-events: none` lo hereda de `.p-skin`. Va más específico que
   * `.p-skin > i` (display: block) para centrar adentro.
   */
  .p-skin > i.p-skin-drop {
    position: absolute;
    top: calc(var(--pill-bar) + var(--rec-drop-gap));
    left: 50%;
    display: grid;

    /* Igual de ancha que el disco: la gota no puede verse más grande que él. */
    width: calc(var(--pill-bar) - var(--goo-grow) * 2);
    height: var(--rec-drop);
    padding: 0 0.25rem;
    place-items: center;
    border-radius: 999px;
    opacity: 0;
    transform: translate(-50%, -60%) scale(0.4);
    transition:
      transform var(--island-open-dur) var(--ease-liquid),
      opacity var(--island-open-dur) var(--ease-liquid);
  }

  .p-skin > i.p-skin-drop.is-in {
    opacity: 1;
    transform: translateX(-50%);
  }

  .p-wave-hang {
    display: block;
    width: 100%;
  }

  /* La onda crece hacia ABAJO desde el borde de arriba: el envión «cae». */
  .p-wave-hang :global(.rb-wave) {
    flex: 1 1 auto;
    width: 100%;
    min-width: 0;
    align-items: flex-start;
  }

  .p-wave-hang :global(.rb-wave-voice) {
    gap: 1px;
    height: 1.05rem;
    justify-content: space-between;
  }

  .p-wave-hang :global(.rb-wave-bar) {
    transform-origin: top;
  }

  /* ─── Barra ─────────────────────────────────────────────────────────── */

  /* Una sola piel para todos los estados. Antes la barra y la tira de cola
     declaraban la misma superficie por separado y se desincronizaban. */
  .p-shell {
    /* Encima de la piel: un hijo posicionado pinta por arriba del contenido
       estático, así que sin esto `.p-skin` taparía la barra entera. */
    position: relative;
    z-index: 1;
    display: flex;

    /* max-content, no 100%: si la ventana todavía no encogió, con 100% la
       barra se estiraba a lo ancho y se veía una pastilla larga con la marca
       pegada a la izquierda. Abrazando el contenido, la forma es correcta
       aunque la ventana venga atrasada. */
    width: max-content;
    max-width: 100%;
    height: var(--pill-bar);
    flex-shrink: 0;
    align-items: center;
    overflow: hidden;
    border-radius: 999px;

    /* Sin fondo: la superficie la pinta `.p-skin`. Acá el radio y el overflow
       siguen haciendo falta, pero solo para recortar el CONTENIDO a la forma
       de la pastilla. */
    color: var(--text);
    transition:
      border-radius var(--morph-close-dur) var(--morph-close-ease),
      transform var(--morph-close-dur) var(--morph-close-ease);
  }

  /* max-content: el ancho lo fija el contenido, no la ventana. Es lo que hace
     que medir la barra no se realimente con el resize. */
  .p-bar {
    display: grid;
    width: max-content;
    min-width: var(--pill-bar);
    height: 100%;

    /* NO encoger. `.p-shell` está topado con `max-width: 100%`, o sea el ancho
       de la VENTANA, así que sin esto la barra se comprimía hasta su min-width
       (el disco) y eso era lo que medíamos: ventana, tope y barra iguales.
       Un abrazo mortal donde la pill no podía crecer sola.
       Se notaba al dictar con el atajo —quedaba redonda con las ondas
       recortadas adentro— pero no con la rueda, porque ahí la ventana se
       redimensiona antes por otro camino y destraba el tope.
       Overflow lo tapa `.p-shell` durante el frame que la ventana tarda. */
    flex-shrink: 0;
    align-items: center;
    padding: 0 12px 0 10px;
    white-space: nowrap;
  }

  /*
   * Un slot por estado, apilados en la misma celda: el outro no suma anchos
   * (la pastilla mediría viejo+nuevo y saltaría). `opacityFade` corre al
   * montar Y al desmontar; el keyframe `p-in` solo existía de ida.
   */
  .p-bar-slot {
    grid-area: 1 / 1;
    display: flex;
    height: 100%;
    min-width: 0;
    align-items: center;
    gap: 8px;
  }

  /* Reposo: disco exacto. Con el padding de la barra quedaba elipse. */
  .p-bar.is-disc-only {
    width: var(--pill-bar);
    justify-content: center;
    padding: 0;
  }

  /*
   * Consola al lado izquierdo del disco: invertir el flex mantiene marca y
   * chip en el DOM (mark → agent) pero pinta agent | mark.
   */
  .p-bar.is-console-start {
    padding: 0 10px 0 12px;
  }

  .p-bar.is-console-start .p-bar-slot {
    flex-direction: row-reverse;
  }

  .p-mark {
    display: inline-flex;
    flex-shrink: 0;
    color: var(--text);
    line-height: 0;
    overflow: visible;
  }

  .p-mark.is-disc {
    border: 0;
    padding: 0;
    border-radius: 999px;
    background: transparent;
    cursor: pointer;
  }

  .p-mark.is-disc:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .p-label {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    color: var(--text);
    font-family: var(--font-sans);
    font-size: 0.625rem;
    font-weight: 500;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .p-timer {
    min-width: 2.4rem;
    color: var(--text);
    font-family: var(--font-sans);
    font-size: 0.6875rem;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.06em;
  }

  .p-chip {
    overflow: hidden;
    max-width: 3.5rem;
    color: var(--muted);
    font-family: var(--font-sans);
    font-size: 0.5625rem;
    font-weight: 500;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .p-chip.is-error {
    color: var(--rec);
  }

  .p-chip.is-warn {
    color: var(--warn);
  }

  .p-wave {
    display: flex;
    min-width: 0;
    align-items: center;
  }

  /* Las ondas SON el control mientras dicta: sin chrome de botón, solo el
     área clickeable. Sin esto habría que dejar el ícono al lado y volvíamos a
     tener dos cosas donde alcanza una. */

  /* ─── Botones ───────────────────────────────────────────────────────── */
  .p-icon,
  .p-dict {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    border: 0;
    margin: 0;
    padding: 0;
    cursor: pointer;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .p-icon {
    /* Hit area del chrome denso: el alto de la barra, sin pseudo que se solape. */
    width: var(--pill-bar);
    height: var(--pill-bar);
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
  }

  .p-icon:hover {
    color: var(--text);
    background: color-mix(in sRGB, var(--text) 8%, transparent);
  }

  .p-dict {
    width: 32px;
    height: 32px;
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
  }

  .p-dict.is-busy {
    color: var(--warn);
  }

  .p-dict.is-ok {
    color: var(--ok);
  }

  .p-dict.is-error {
    color: var(--rec);
  }

  .p-icon:active:not(:disabled),
  .p-dict:active:not(:disabled) {
    transform: scale(0.96);
  }

  .p-icon:disabled,
  .p-dict:disabled {
    opacity: 0.45;
    cursor: default;
  }

  /* El anillo va por dentro y no como `outline`: la pill vive sobre una
     silueta redondeada y un contorno exterior se sale del contorno fundido. */
  .p-icon:focus-visible,
  .p-dict:focus-visible,
  .p-queue-btn:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--accent);
  }

  /* ─── Cola de pegado ────────────────────────────────────────────────── */
  .p-queue-count {
    flex-shrink: 0;
    color: var(--faint);
    font-size: 0.625rem;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.06em;
  }

  .p-queue-text {
    max-width: 10rem;
    overflow: hidden;
    color: var(--muted);
    font-size: 0.6875rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .p-queue-btn {
    display: inline-flex;
    min-height: 1.65rem;
    flex-shrink: 0;
    align-items: center;
    border: 0;
    border-radius: 999px;
    padding: 0 0.6rem;
    background: color-mix(in sRGB, var(--text) 8%, transparent);
    color: var(--text);
    font-size: 0.5625rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    cursor: pointer;
    transition: transform var(--duration-quick) var(--ease-smooth-out);
  }

  .p-queue-btn:active:not(:disabled) {
    transform: scale(0.96);
  }

  .p-queue-btn:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .p-agent-stack {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    flex-shrink: 0;
  }

  .p-bar.is-console-start .p-agent-stack {
    align-items: flex-end;
  }

  /* ─── Aviso de agente ───────────────────────────────────────────────── */
  .p-agent {
    position: relative;
    display: inline-flex;
    min-height: 1.35rem;
    max-width: 9.5rem;
    flex-shrink: 0;
    align-items: center;
    gap: 0.18rem;
    border: 0;
    border-radius: 999px;
    padding: 0 0.34rem 0 0.28rem;
    background: color-mix(in sRGB, var(--accent) 12%, transparent);
    color: var(--accent);
    cursor: pointer;
    transition:
      transform var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      color var(--duration-quick) var(--ease-smooth-out);
  }

  /* Hit ≥40px sin inflar la cápsula visible. */
  .p-agent::after {
    content: "";
    position: absolute;
    top: 50%;
    left: 50%;
    width: max(40px, 100%);
    height: 40px;
    transform: translate(-50%, -50%);
  }

  .p-agent:active {
    transform: scale(0.96);
  }

  .p-agent-ico {
    display: grid;
    place-items: center;
    width: 0.85rem;
    height: 0.85rem;
    flex-shrink: 0;
    opacity: 0.92;
  }

  .p-agent-ico.is-row {
    display: flex;
    width: auto;
    height: auto;
    gap: 0.2rem;
  }

  /*
   * Pestaña achicada: texto de la barra, no una cápsula dentro de otra.
   * El estadio de la piel ya es la silueta; un fondo propio se leía como
   * gota colgando. Espera / listo / conteo sí pintan cápsula (aviso).
   */
  .p-agent.is-dock {
    min-height: 1.5rem;
    padding: 0 0.08rem 0 0;
    gap: 0.34rem;
    background: transparent;
    font-family: var(--font-sans);
  }

  .p-island-cue.p-agent.is-dock {
    min-height: 0;
    padding: 0;
    gap: 0;
  }

  .p-agent.is-dock .p-agent-msg {
    max-width: 8rem;
    font-size: 0.6875rem;
    letter-spacing: 0.02em;
  }

  .p-agent.is-dock.is-working {
    background: transparent;
  }

  /* Solo número: cápsula mínima, sin aire de “pill anidada”. */
  .p-agent.is-count {
    gap: 0.12rem;
    padding: 0 0.3rem 0 0.26rem;
    max-width: none;
  }

  /* Espera una decisión: es lo único que de verdad bloquea al agente, así que
     es lo único que usa el color de alerta. */
  .p-agent.is-waiting {
    background: color-mix(in sRGB, var(--rec) 16%, transparent);
    color: var(--rec);
  }

  /* Trabajando / contestando: la gota de la piel respira y el logo late
     suave; el chip no parpadea entero (el texto tiene que leerse). */
  .p-agent.is-working {
    background: transparent;
    color: var(--muted);
  }

  .p-agent.is-working .p-agent-ico {
    animation: p-agent-pulse 1.8s var(--ease-liquid) infinite;
  }

  /* Listo / respuesta sin leer: affordance clara, no solo un número. */
  .p-agent.is-ready {
    background: color-mix(in sRGB, var(--ok) 14%, transparent);
    color: var(--ok);
    animation: p-agent-ready-in var(--duration-very-slow, 250ms) var(--ease-smooth-out)
      both;
  }

  .p-agent-count,
  .p-agent-msg {
    font-size: 0.625rem;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    line-height: 1;
  }

  .p-agent-msg {
    /* Techo: sin él, el preview del agente estiraba la pill hasta donde
       llegara el texto. El mensaje entero vive en el globo. */
    max-width: 18ch;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Sin nada que abrir: es un aviso, no un control. */
  .p-agent.is-inert {
    cursor: default;
  }

  /* Aviso de actualización: la misma cápsula que el chip de agentes, en el
     color de información. Comparte forma a propósito — son los dos avisos que
     la pill sabe dar, y leerlos como la misma cosa es lo correcto. */
  .p-update {
    position: relative;
    display: inline-flex;
    min-height: 1.35rem;
    flex-shrink: 0;
    align-items: center;
    gap: 0.18rem;
    border: 0;
    border-radius: 999px;
    padding: 0 0.34rem 0 0.28rem;
    background: color-mix(in sRGB, var(--info) 14%, transparent);
    color: var(--info);
    cursor: pointer;
    transition:
      transform var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      color var(--duration-quick) var(--ease-smooth-out);
  }

  /* Mismo blanco de clic que `.p-agent`: la cápsula mide menos que un dedo,
     así que el área viva se estira sin mover el dibujo. */
  .p-update::after {
    content: "";
    position: absolute;
    top: 50%;
    left: 50%;
    width: max(40px, 100%);
    height: 40px;
    transform: translate(-50%, -50%);
  }

  .p-update:active {
    transform: scale(0.96);
  }

  /* Bajando o instalando: no se puede volver a apretar, y el cursor lo dice. */
  .p-update:disabled {
    cursor: progress;
  }

  /*
   * El aviso del equipo no es una novedad agradable: tono de atención de la
   * casa —el mismo del permiso de agentes— y respira. Un chip quieto, del
   * mismo tamaño que el del update, se lee como información; esto es un
   * pedido de atención y tiene que distinguirse sin leerlo.
   */
  .p-update.is-alert {
    border-color: color-mix(in sRGB, var(--warn) 55%, transparent);
    background: color-mix(in sRGB, var(--warn) 14%, transparent);
    color: var(--warn);
    animation: p-alert-breathe 2.4s ease-in-out infinite;
  }

  @keyframes p-alert-breathe {
    0%,
    100% {
      border-color: color-mix(in sRGB, var(--warn) 55%, transparent);
    }

    50% {
      border-color: color-mix(in sRGB, var(--warn) 100%, transparent);
    }
  }

  /* El volumen es información, no un aviso: tono normal y sin puntero. */
  .p-update.is-volume {
    cursor: default;
  }

  /* Ya está en disco: el próximo clic instala y reinicia. */
  .p-update.is-ready {
    background: color-mix(in sRGB, var(--ok) 14%, transparent);
    color: var(--ok);
  }

  .p-update.is-busy {
    background: color-mix(in sRGB, var(--text) 7%, transparent);
    color: var(--muted);
  }

  .p-update-ico {
    display: grid;
    place-items: center;
    width: 0.85rem;
    height: 0.85rem;
    flex-shrink: 0;
    opacity: 0.92;
  }

  /* Tabular: el porcentaje cuenta de 9% a 10% sin que la cápsula tironee. */
  .p-update-text {
    font-size: 0.625rem;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    line-height: 1;
  }

  /* La respiración vive en el Skin del overlay, no en este fill. */

  /*
   * Auth: mismo `.float-emerge` que clipboard/agentes (nace/vuelve a la pill).
   * Solo alarga la apertura; el viaje y el scale viven en `app.css`.
   */
  .p-auth-host {
    --float-open-dur: var(--duration-medium);

    position: absolute;
    z-index: 6;
  }

  @keyframes p-agent-pulse {
    0%,
    100% {
      opacity: 0.55;
    }

    50% {
      opacity: 1;
    }
  }

  @keyframes p-agent-ready-in {
    from {
      opacity: 0;
      transform: translateY(var(--distance-micro, 4px));
      filter: blur(var(--blur-small, 2px));
    }

    to {
      opacity: 1;
      transform: translateY(0);
      filter: blur(0);
    }
  }

  @keyframes p-liquid-alive {
    0%,
    100% {
      filter: brightness(1);
    }

    50% {
      filter: brightness(1.08);
    }
  }

  /* La pill entera es inseleccionable: es una superficie que se arrastra. */
  .p-root,
  .p-root * {
    user-select: none !important;
  }

  @media (prefers-reduced-motion: reduce) {
    .p-update.is-alert {
      animation: none;
    }

    .p-root.is-flying,
    .p-root.is-docked,
    .p-wheel,
    .p-wheel.is-open,
    .p-stack,
    .p-shell,
    .p-liquid,
    .p-skin-tail,
    .p-skin-drop,
    .p-icon,
    .p-dict,
    .p-queue-btn,
    .p-agent,
    .p-update,
    .p-auth-host,
    .p-island-tool,
    .p-island-along,
    .p-side,
    .p-agent-ico,
    .p-root.is-seating,
    .p-island.is-detach-bounce .p-island-body,
    .p-island-mark,
    .p-island-cue,
    .p-island-update,
    .p-island-cue-logo,
    .p-island-agent-badge,
    .p-live-row,
    .p-live-act,
    .p-live-act i,
    .p-mark.is-lead {
      transition: none !important;
      animation: none !important;
    }

    /* La isla salta entre cerrada y abierta sin recorrido. No se fuerza el
       estado abierto: las reglas de `.is-open` siguen mandando, y forzarlo
       dejaría las gotas separadas también con la isla cerrada. */

    .p-agent.is-working {
      opacity: 0.8;
    }
  }
</style>
