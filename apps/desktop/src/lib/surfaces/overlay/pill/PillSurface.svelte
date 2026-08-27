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
  import { onMount, tick } from "svelte";
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
  import type { Rect } from "$lib/liquid/geometry";
  import { RectTracker } from "$lib/liquid/measure.svelte";
  import { gapBetween, pillShape } from "$lib/liquid/geometry";
  import { INFLUENCE, REACH } from "$lib/liquid/constants";
  import ToolIcon from "$lib/ToolIcon.svelte";
  import { agents } from "$lib/agentSessions.svelte";
  import { presence } from "$lib/agentPresence.svelte";
  import ParticleWheel from "$lib/ParticleWheel.svelte";
  import {
    WHEEL_TOOLS,
    AGENTS_ENABLED,
    AGENT_PAGER_ENABLED,
    type ToolId,
  } from "$lib/tools";
  import { localizeTool, t } from "$domain/i18n.svelte";
  import { formatShortcut } from "$lib/format";
  import Icon from "$ui/Icon.svelte";
  import { Check, Download, X } from "$lib/icons";
  import type { IconNode } from "morphicons/svelte";
  import { PILL, windowFor, type Size } from "$surfaces/overlay/pillStage";
  import { createCssStage } from "$surfaces/overlay/pillCssStage";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";
  import {
    blocksBrowserChrome,
    consoleSideFor,
    contentFor,
    discJoinsTail,
    FLIGHT_SKIP_PX,
    isDiscOnly,
    islandLiveSlots,
    morphsInPlace,
    pivotFor,
    stepWheel as nextWheelTool,
    undockForSummon,
    wheelChromeActive,
    wheelKeyAction,
    wheelOpenFlight,
    type Dock,
    type Surface,
  } from "$surfaces/overlay/pill/pillPlan";
  import { agentChip } from "$surfaces/overlay/pill/pillAgentChip";
  import {
    dockAxis,
    dockCandidate,
    dockedEdgeAt,
    edgeWallsFor,
    shouldUndock,
    type DockEdge,
  } from "$surfaces/overlay/edgeDock";
  import AgentAuthCard from "$surfaces/overlay/pill/AgentAuthCard.svelte";
  import { afterTransition, MOTION, ms, wait } from "$lib/motion";
  import { playWheelTick } from "$core/uiSound";
  import type { PermissionDecision } from "$core/types";
  // Lo que queda son los comandos DE LA PILL: su geometría, sus atajos y las
  // ventanas que abre. El estado de la app lo traen los stores.
  import {
    agentsAlwaysOnTop,
    agentsWindowVisible,
    hideAgentsWindow,
    onAgentsBubbleAnchor,
    onAgentsBubbleDismiss,
    showAgentsWindow,
    agentPresenceFocus,
    agentPresenceBind,
    revealAgentsConsole,
  } from "$ipc/agents";
  import {
    clipboardAlwaysOnTop,
    hideClipboardWindow,
    onClipboardBubbleDismiss,
  } from "$ipc/clipboard";
  import { getConfig } from "$ipc/config";
  import { on } from "$ipc/events";
  import { hideLauncher, onLauncherBubbleDismiss } from "$ipc/search";
  import {
    hideSnippetsWindow,
    onSnippetsBubbleDismiss,
    snippetsAlwaysOnTop,
  } from "$ipc/snippets";
  import { executeToolAction } from "$core/toolActions";
  import { isSpatialTool, resolveSlot, slotForTool } from "$surfaces/overlay/toolSlots";
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
  import { armOpenDismissGrace } from "$surfaces/overlay/openDismissGrace";
  import {
    onOverlayDismiss,
    onOverlayYieldMain,
    onOverlayReady,
    onPillRadialPress,
    onPillRadialRelease,
    onPillReset,
    overlayCursor,
    overlayCursorOverHit,
    overlayPrimaryDown,
    overlayActiveAnchor,
    pillHome,
    pillTrace,
    savePillHome,
    setOverlayPointerGesture,
  } from "$ipc/overlay";

  /*
   * Los tipos y las decisiones viven en `pill/pillPlan.ts`, que es TS puro y
   * está testeado. Acá queda la ejecución: el estado, los efectos y los viajes
   * a Rust.
   *
   * Clipboard, snippets y agentes abren ventanas flotantes propias; la pill solo
   * los invoca desde la rueda o muestra el aviso de agente en la barra.
   */

  /**
   * El estado de la app no es de la pill.
   *
   * Grabación, dictado y cola vivían acá con sus propias copias y sus propios
   * oyentes, duplicados con la ventana principal: dos cronómetros contando lo
   * mismo. Ahora se declara una vez qué necesita esta ventana y el resto se lee.
   */
  $effect(() => sessionEffect(["config", "capture", "dictation", "paste"]));

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
  let wheelTool = $state<ToolId | null>(null);
  /** Cierre acelerado: al elegir herramienta la rueda ya cumplió su función. */
  let wheelQuick = $state(false);
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
  const chip = $derived(
    agentChip({
      chat: {
        unread: agents.unread,
        working: agents.working,
        waiting: agents.waiting,
        readyLabel: agents.readyLabel,
        providerSessions: agents.sessions.map((s) => s.providerSession),
      },
      presence: presence.view,
      chatEnabled: AGENTS_ENABLED,
      pagerEnabled: AGENT_PAGER_ENABLED,
    }),
  );
  const agentAlert = $derived(chip.tone !== "off");
  const agentWorking = $derived(chip.tone === "working" || chip.tone === "count");
  const agentReady = $derived(chip.tone === "ready");
  const agentReadyLabel = $derived(chip.label ?? t("pill.ready"));
  /**
   * Aviso de actualización: qué muestra el chip y qué dice al pasar el mouse.
   *
   * Un solo botón para las cuatro fases porque `appUpdate.advance()` ya es un
   * solo camino: si no está descargada, baja; si ya está, instala y reinicia.
   * Los textos son los mismos que la gota de la ventana principal — es el
   * mismo aviso, no otro dialecto.
   */
  const updateChip = $derived.by(() => {
    if (!appUpdate.pending) return null;
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

  const agentChipAria = $derived.by(() => {
    const target = chip.target;
    if (target.kind === "focus") {
      const name =
        presence.list.find((p) => p.id === target.presenceId)?.backendName ??
        t("pill.agentFallback");
      return t("pill.goToAgent", { name });
    }
    if (target.kind === "console") return t("pill.openConsole");
    if (target.kind === "none") {
      return t("pill.unbound");
    }
    if (chip.tone === "working") return t("pill.working");
    return agentReadyLabel;
  });
  const agentChipTitle = $derived.by(() => {
    if (chip.target.kind === "none") {
      return t("pill.unboundTitle");
    }
    const base =
      chip.tone === "waiting"
        ? t("pill.waiting")
        : chip.tone === "ready"
          ? agentReadyLabel
          : chip.tone === "count"
            ? t("pill.unread", { label: chip.label ?? "" })
            : t("pill.working");
    if (chip.target.kind === "focus") {
      return t("pill.rebind", { base });
    }
    return base;
  });
  const authRequest = $derived(agents.primaryPending);
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
  const discOnly = $derived(isDiscOnly({ surface, activity, hasQueue, agentAlert }));

  let wheelShortcut = $state("");

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
  /** La isla acoplada: llena la caja, así que se anima con ella. */
  const trackIsland = (el: HTMLElement) => tracker.track("island", el);
  const trackLive = (el: HTMLElement) => tracker.track("live", el);
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
  const wheelTools = $derived(WHEEL_TOOLS.map(localizeTool));
  const islandAttachers = WHEEL_TOOLS.map(
    (_, i) => (el: HTMLElement) => tracker.track(`island-${i}`, el),
  );

  /** Margen tras la coreografía, por si el último cuadro llega tarde. */
  const ISLAND_SETTLE_MS = 120;

  /**
   * Mantener despierto al tracker durante TODA la transición de la isla.
   *
   * `RectTracker` deja de mirar tras 3 cuadros sin cambios, y su propio
   * comentario avisa del riesgo: «una transición puede tener un cuadro sin
   * cambio visible en el medio». La de la isla tiene muchos más que uno —el
   * escalonado mete hasta 52 ms de `transition-delay`, que a 60 Hz son 3
   * cuadros enteros en los que nada se mueve—, así que se dormía a mitad de
   * camino y la silueta quedaba congelada donde la hubiera agarrado: un cuerpo
   * chico bajo unos iconos ya desplegados, hasta que otra cosa lo despertaba.
   *
   * `wake()` es idempotente y solo reinicia el contador de quietud, así que
   * bombearlo por cuadro durante el tramo cuesta nada y garantiza que la
   * silueta siga la animación hasta el final.
   */
  $effect(() => {
    void dock;
    void surface;
    void recording;
    if (surface !== "edge" && !beadsAlive) return;
    let raf = 0;
    const until = performance.now() + ms(MOTION.islandOpen) + ISLAND_SETTLE_MS;
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
    // Con el chrome de la rueda activo (abierta o colapsando) la silueta la
    // pintan las gotas de ParticleWheel. Publicar la barra en ese tramo deja
    // un disco fantasma arriba-izquierda del cuadrado: el stack vive anclado
    // al top-left del root, no al centro donde está la marca de la rueda.
    if (wheelChrome) return [];
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
    // publicarla dibujaba el disco de 40 px sobre una zona viva de 48×18 —la
    // pill se veía sin cambiar y no respondía, porque lo que recibe el puntero
    // es la caja y no lo dibujado—.
    if (surface === "edge") {
      const shapes = [];
      if (r.island) shapes.push(pillShape(at(r.island)));
      if (r.live) shapes.push(pillShape(at(r.live)));
      shapes.push(...wallFor(r.island));
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
    liquid.publish("pill", skinShapes);
  });

  /**
   * El hogar: reposo persistido. Abrir la rueda vuela al cursor; al cerrar
   * (Esc / fuera / soltar sin tool) vuelve acá. Summon y arrastre sí lo
   * reescriben a propósito.
   */
  let home = $state({ x: 0, y: 0 });
  /** El vuelo lo hace una transición CSS; esto la enciende solo cuando toca. */
  let flying = $state(false);
  /** Generación del vuelo: Esc/reabrir invalidan un `flyTo` en curso. */
  let flightEpoch = 0;

  function cancelFlight() {
    flightEpoch += 1;
    flying = false;
  }

  /**
   * Mueve la pill con transición CSS hasta `p`.
   *
   * Hay que pintar `.is-flying` *antes* de cambiar left/top: si clase y
   * destino llegan en el mismo frame, el navegador salta sin animar.
   * Devuelve la duración usada, o `-1` si el vuelo fue cancelado.
   */
  async function flyTo(
    p: { x: number; y: number },
    opts: { skipIfNear?: number } = {},
  ): Promise<number> {
    const epoch = ++flightEpoch;
    const from = stage.at();
    const dist = Math.hypot(p.x - from.x, p.y - from.y);
    const skip = opts.skipIfNear ?? 0;
    const dur = ms(MOTION.flight);

    if (dist < skip || dur <= 0) {
      stage.moveTo(p);
      at = stage.at();
      surfaces.schedule();
      return 0;
    }

    flying = true;
    await tick();
    if (epoch !== flightEpoch) {
      flying = false;
      return -1;
    }
    // Fuerza layout con la transición ya activa y la posición vieja.
    void rootEl?.offsetWidth;

    stage.moveTo(p);
    at = stage.at();
    await wait(dur);

    if (epoch !== flightEpoch) {
      flying = false;
      return -1;
    }

    flying = false;
    // Republicar al aterrizar: durante el vuelo getBoundingClientRect()
    // reporta la posición animada; sin esto Rust arma el sitio de origen.
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

  /** Monitor activo: mouse, o la ventana con foco si está en otra pantalla. */
  async function activeAnchorPoint(): Promise<{ x: number; y: number } | null> {
    try {
      return await overlayActiveAnchor();
    } catch {
      return cursorPoint();
    }
  }
  /** Ancho real de la barra, medido del DOM. Sin esto habría que mantener una
   *  tabla de anchos mágicos por estado — la fuente original del desajuste. */
  let barW = $state<number>(PILL.bar);
  let barEl = $state<HTMLElement | null>(null);

  /**
   * Acoplada a un borde. `null` = flotando, que es el estado de siempre.
   *
   * No se persiste aparte: `pill_home` guarda un punto y al arrancar se deduce
   * con `dockedEdgeAt` si ese punto estaba a ras de un borde exterior. Evita
   * migrar el formato por un booleano.
   */
  let dock = $state<Dock | null>(null);

  const target = $derived(windowFor(contentFor(surface, barW, dock, activity)));
  const islandSlots = $derived(WHEEL_TOOLS.length + islandLiveSlots(activity));

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

  /** La tira está desplegada (o se está desplegando). */
  const islandOpen = $derived(surface === "edge" && dock?.expanded === true);

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

  async function openAgentsConsole() {
    try {
      const visible = await agentsWindowVisible();
      // `show_agents_window` es un toggle: si el lanzador ya está a la vista,
      // no cerrarlo. Pedir la consola viva y, si hacía falta, abrir el float.
      revealAgentsConsole();
      if (!visible) await showAgentsWindow();
      agentsConsoleOpen = true;
    } catch (err) {
      console.warn("abrir consola de agentes", err);
    }
  }

  function activateAgentChip(preferBindFromGesture = false) {
    const target = chip.target;
    if (target.kind === "console") {
      void openAgentsConsole();
      return;
    }
    const id = target.presenceId;
    if (!id) return;
    const preferBind = preferBindFromGesture || target.kind === "none";
    void (async () => {
      try {
        let result = preferBind
          ? await agentPresenceBind(id)
          : await agentPresenceFocus(id);
        if (result.kind === "none" && !preferBind) {
          result = await agentPresenceBind(id);
        }
        if (result.kind === "focused") presence.markSeen(id);
        else if (result.kind === "console") await openAgentsConsole();
      } catch (err) {
        console.warn("enfocar terminal del agente", err);
      }
    })();
  }

  function onAgentChipClick(event: MouseEvent) {
    if (suppressAgentChipClick && event.detail > 0) {
      event.preventDefault();
      suppressAgentChipClick = false;
      return;
    }
    activateAgentChip(event.ctrlKey || event.metaKey);
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
    const w = 320;
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

  /** Invalida un encoger/vuelo en curso. */
  function cancelPendingCollapse() {
    collapseEpoch += 1;
    cancelFlight();
    opening = false;
  }
  /** El estado del que dependen las decisiones de `pillPlan`. */
  function plan() {
    return { surface, collapsingFrom, dock };
  }

  /**
   * Único reconciliador. La regla de crecer-antes / encoger-después es lo que
   * reemplaza a `chromeHidden`, `radialClosing` y `quickClose` como banderas:
   * la ventana es siempre la unión de origen y destino mientras algo se anima.
   */
  async function reconcile(next: Size) {
    if (opening) return;
    const from = stage.applied();
    const outcome = await stage.resize(
      next,
      pivotFor(plan()),
      morphsInPlace({ ...plan(), from }),
    );
    if (outcome.ok) {
      collapsingFrom = null;
      at = stage.at();
      box = next;
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
    void reconcile(next);
  });

  // La zona viva: sin esto Rust deja el overlay en click-through y la pill no
  // recibe ni un clic.
  $effect(() => (rootEl ? surfaces.add("pill", rootEl) : undefined));

  /**
   * La SILUETA, aparte de la zona viva.
   *
   * No son lo mismo: la zona incluye el respiro que la pill deja alrededor
   * para que quepa la rueda, y de ahí cuelga la burbuja de agentes. Anclada a
   * la zona, el globo quedaría a `gap` más ese respiro de distancia y el
   * cuello no llegaría a cruzarlo.
   */
  /*
   * Acoplada no se publica: el stack sigue midiendo la barra de 40 px aunque
   * esté oculto —`overflow: hidden` recorta lo que se ve, no lo que mide—, así
   * que dejaba una zona viva fantasma más grande que la isla. Ahí el overlay se
   * armaba sobre pantalla que la isla no ocupa y se comía clics ajenos.
   */
  $effect(() =>
    liquidEl && surface !== "edge" ? surfaces.add("pill-skin", liquidEl) : undefined,
  );

  /** La tarjeta de auth también tiene que armar hit-rects o queda click-through. */
  $effect(() => (authEl && authAlive ? surfaces.add("agent-auth", authEl) : undefined));

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
      if (surface !== "none") return;
      // max(offset, scroll): si alguna regla vuelve a topar la barra, el ancho
      // real del contenido sigue estando en scrollWidth. Sin esto, un clamp
      // aguas arriba deja la medición mintiendo y la ventana no crece nunca.
      barW = Math.max(el.offsetWidth, el.scrollWidth);
    };
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    measure();
    return () => observer.disconnect();
  });

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
    void agentReadyLabel;
    void consoleSide;
    if (surface !== "none") return;
    // Un frame después: en este tick el DOM todavía tiene el contenido viejo.
    const frame = requestAnimationFrame(() => {
      // max(offset, scroll): si alguna regla vuelve a topar la barra, el ancho
      // real del contenido sigue estando en scrollWidth. Sin esto, un clamp
      // aguas arriba deja la medición mintiendo y la ventana no crece nunca.
      barW = Math.max(el.offsetWidth, el.scrollWidth);
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
   */
  function settleDock(): boolean {
    // La rueda manda: mientras esté abierta o colapsando, la pill no es una
    // isla aunque esté parada sobre el canto.
    if (surface === "wheel" || collapsingFrom === "wheel") return false;
    const size = stage.applied() ?? windowFor({ w: PILL.bar, h: PILL.bar });
    const rect = { x: at.x, y: at.y, w: size.w, h: size.h };
    const found = dockCandidate(rect, stage.workAreas());
    if (!found) {
      dock = null;
      if (surface === "edge") surface = "none";
      return false;
    }
    dock = { edge: found.edge, expanded: false };
    surface = "edge";
    // Pegada al canto, sin transición: el vuelo ya terminó y esto es el
    // último ajuste de milímetros.
    stage.moveTo(found.at);
    at = stage.at();
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
    let alive = true;
    const look = async () => {
      // Arrastrando no: agrandar la caja a mitad del gesto mueve el suelo bajo
      // el puntero, y encima el destino de acople se calcula con ese tamaño.
      if (dragOrigin) return;
      const over = await overlayCursorOverHit("pill").catch(() => null);
      if (alive && !dragOrigin && over !== null) setIslandExpanded(over);
    };
    void look();
    const timer = setInterval(() => void look(), ISLAND_HOVER_MS);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  });

  /**
   * Dictar acoplada DESACOP.LA la pill mientras dura el dictado.
   *
   * Acoplada, la barra con las ondas está oculta (`.p-root.is-docked .p-stack`)
   * y la pestaña no tiene dónde mostrarlas: sin esto, dictar en un borde no
   * muestra señal alguna. La pill vuelve a ser la barra normal —mic + ondas— y
   * al terminar intenta volver a su canto: `settleDock` re-evalúa el candidato,
   * así que si el usuario la arrastró lejos durante el dictado no se acopla de
   * golpe en un sitio que ya no le corresponde.
   */
  let dictDockedEdge: DockEdge | null = null;
  $effect(() => {
    if (dictating) {
      if (surface === "edge" && !dragOrigin) {
        dictDockedEdge = dock?.edge ?? null;
        surface = "none";
        dock = null;
      }
      return;
    }
    if (dictDockedEdge === null) return;
    dictDockedEdge = null;
    // Solo tiene sentido re-acoplar si sigue flotando libre cerca del canto:
    // la rueda o un arrastre mandan sobre este regreso.
    if (surface === "none") settleDock();
  });

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
  async function openWheel() {
    // `surface` recién vale "wheel" al final. En esa ventana el auto-repeat del
    // atajo (Windows reenvía `Pressed` mientras la tecla está sostenida) podía
    // reentrar acá: el segundo pase cancelaba el tween del primero.
    if (surface === "wheel" || openingWheel) return;
    openingWheel = true;
    try {
      await openWheelInner();
    } finally {
      // Pase lo que pase: una bandera trabada acá dejaría la rueda muerta para
      // el resto de la sesión.
      openingWheel = false;
    }
  }

  async function openWheelInner() {
    trace("openWheel");
    // Cancela un encoger pendiente: si el morph de cierre aún corre, el resize
    // chico no debe llegar después de que ya volvimos a crecer.
    cancelPendingCollapse();
    await closeWheel();
    wheelQuick = false;
    // Sin selección inicial: un toque accidental del atajo no debe disparar
    // ninguna acción al soltar.
    wheelTool = null;
    opening = true;
    const openEpoch = collapseEpoch;
    try {
      // 1) Guardar el hogar ANTES de tocar la geometría.
      home = { ...at };
      // 2) Volar al sitio donde la rueda cabe: cursor si está lejos (atajo),
      //    o el centro actual clampeado si el clic es sobre la pill.
      await stage.loadAreas();
      const size = stage.applied() ?? windowFor({ w: PILL.bar, h: PILL.bar });
      const wheel = windowFor({
        w: PILL.wheel - PILL.pad * 2,
        h: PILL.wheel - PILL.pad * 2,
      });
      const dest = wheelOpenFlight({
        cursor: await cursorPoint(),
        pill: { x: at.x, y: at.y, w: size.w, h: size.h },
        wheel,
        areas: stage.workAreas(),
        skipIfNear: FLIGHT_SKIP_PX,
      });
      const flew = await flyTo(dest, { skipIfNear: 2 });
      if (flew < 0 || openEpoch !== collapseEpoch) {
        surface = "none";
        wheelShown = false;
        await flyTo(home, { skipIfNear: 2 });
        return;
      }
      if (openEpoch !== collapseEpoch) return;

      // 3) Stack apagado + chrome de rueda colapsado ANTES de crecer: un solo
      //    "a" (ParticleWheel) fijo en el centro.
      collapsingFrom = null;
      surface = "wheel";
      wheelShown = false;
      // 4) Crecer hit-box al instante (pivot center). El morph visual es el anillo.
      const side = PILL.wheel - PILL.pad * 2;
      const next = windowFor({ w: side, h: side });
      await stage.resize(next, pivotFor({ surface: "wheel", collapsingFrom: null }));
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
    await stage.resize(next, pivotFor({ surface: "none", collapsingFrom: "wheel" }));
    at = stage.at();
    // El escenario ya encogió: el DOM tiene que seguirlo YA, antes de soltar
    // `collapsingFrom`. Si `box` se queda en tamaño rueda, el handoff al stack
    // pinta la marca compacta arriba-izquierda del cuadrado fantasma.
    box = next;
    collapsingFrom = null;
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
      settleDock();
    }
    return true;
  }

  /** Cierra la rueda. Por defecto vuelve al hogar; un atajo de tool no debe. */
  async function closeWheel(opts: { returnHome?: boolean } = {}) {
    if (surface !== "wheel") return;
    trace("closeWheel");
    const epoch = ++collapseEpoch;
    opening = true;
    // Orden importa: `collapsingFrom` ANTES de `surface = "none"`, para que el
    // stack y la piel líquida no asomen un frame en el top-left del root grande
    // mientras ParticleWheel aún colapsa en el centro.
    collapsingFrom = "wheel";
    wheelTool = null;
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
    if (wheelShown && wheelTool) activateTool(wheelTool);
    else void closeWheel();
  }

  /** Teclado con la rueda abierta. No preselecciona: enfocar un nodo al abrir
   *  dejaría una herramienta armada y soltar la tecla la dispararía. */
  function onWheelKey(event: KeyboardEvent): boolean {
    if (surface !== "wheel" || !wheelShown) return false;
    const action = wheelKeyAction(event.key, event.shiftKey);
    if (!action) return false;
    if (action === "activate") {
      if (wheelTool) activateTool(wheelTool);
      else void closeWheel();
    } else {
      const next = nextWheelTool(wheelTool, action === "next" ? 1 : -1, WHEEL_TOOLS);
      if (next !== wheelTool) playWheelTick();
      wheelTool = next;
    }
    return true;
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
    const [clipPinned, snipPinned, agentsPinned] = await Promise.all([
      clipboardAlwaysOnTop().catch(() => false),
      snippetsAlwaysOnTop().catch(() => false),
      agentsAlwaysOnTop().catch(() => false),
    ]);
    const targets = spatialDismissTargets(keep, {
      clipboard: clipPinned,
      snippets: snipPinned,
      agents: agentsPinned,
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
      case "agents":
        await hideAgentsWindow();
        return;
      default:
        return;
    }
  }

  /** Float espacial ya visible (hit-rect registrado). */
  function spatialToolOpen(id: ToolId): boolean {
    return isSpatialTool(id) && surfaces.live[id] != null;
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
   * 3840×1080) mientras Windows asienta las pantallas, y `pill_home` se
   * restaura clampeado contra ESE espacio: la pill quedaba a mitad del
   * escritorio cuando el viewport llegaba al tamaño real, hasta que alguna
   * interacción la volvía a volar. Lo mismo si la topología flapea (2→1→2
   * monitores) con la app abierta. Rust ya reencuadra la ventana solo; esta es
   * la mitad del frontend: recargar áreas y devolver la pill a su hogar, solo
   * si está en reposo.
   */
  let resettleTimer = 0;
  let homeRestored = false;
  /** Corta el sondeo de actualizaciones al desmontar. */
  let stopUpdatePolling: (() => void) | null = null;

  function queueResettle() {
    if (!homeRestored) return;
    window.clearTimeout(resettleTimer);
    // Los resize del boot llegan en ráfaga (1551→3072→3840): coalescer.
    resettleTimer = window.setTimeout(() => void resettleAfterGeometry(), 150);
  }

  async function resettleAfterGeometry() {
    await stage.loadAreas();
    if (flying || openingWheel || slotBusy || returnHomeSuppressed) return;
    if (surfaces.dragging || dragOrigin || anySpatialOpen()) return;
    if (surface !== "none" && surface !== "edge") return;
    if (dock?.expanded) return;
    const before = { ...at };
    stage.moveTo(home);
    at = stage.at();
    if (Math.hypot(before.x - at.x, before.y - at.y) >= 2) {
      // Mismo criterio que al restaurar el hogar en el arranque: si el punto
      // queda a ras de un borde exterior, estaba acoplada ahí.
      const size = stage.applied() ?? windowFor({ w: PILL.bar, h: PILL.bar });
      const edge = dockedEdgeAt(
        { x: at.x, y: at.y, w: size.w, h: size.h },
        stage.workAreas(),
      );
      if (edge) {
        dock = { edge, expanded: false };
        surface = "edge";
      } else if (surface === "edge") {
        dock = null;
        surface = "none";
      }
    }
    surfaces.schedule();
  }

  /** Vuelve al hogar si la pill quedó en un slot de acción. */
  async function maybeReturnHome() {
    if (returnHomeSuppressed || slotBusy || spatialIntent) return;
    if (anySpatialOpen()) return;
    if (Math.hypot(home.x - at.x, home.y - at.y) < 2) return;
    try {
      await flyTo(home);
      // Mismo motivo que en `playCloseMorph`: si el hogar es un canto, al
      // llegar vuelve a ser isla.
      settleDock();
    } catch (err) {
      console.warn("return-to-home", err);
    }
  }

  /** Dismiss de tool con reverse liquid: esperar a que el float se funda. */
  async function maybeReturnHomeAfterFloat(id: "launcher" | "clipboard" | "snippets") {
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
   * Vuela la pill al slot de la tool (work area del monitor actual).
   * Clipboard / textos no tienen slot fijo: vuelan al cursor (atajo / catálogo).
   * No redefine `home`: el destino es posición de acción, no reposo.
   *
   * `anchored` = el pedido viene de la rueda. Los slots fijos igual se vuelan
   * (dictado abajo, Apps al costado), pero el vuelo al cursor se saltea: la
   * rueda YA se abrió centrada en el cursor, y el punto que toca el puntero al
   * elegir está sobre el anillo, no en el centro. Sin esto, elegir Clipboard
   * mandaba la pill los ~56 px del radio hasta el gajo, que es ruido: el ancla
   * natural de un menú radial es su centro, no dónde cayó el dedo.
   */
  async function flyToToolSlot(
    id: ToolId,
    opts: { anchored?: boolean } = {},
  ): Promise<void> {
    // Al terminar dictado no hace falta volar.
    if (id === "dictation" && dictationStore.active) return;

    if (isSpatialTool(id) || id === "dictation") await dismissSpatialTools(id);

    const size = target;
    const slot = slotForTool(id);

    if (slot) {
      await stage.loadAreas();
      const areas = stage.workAreas();
      const pillCenter = { x: at.x + size.w / 2, y: at.y + size.h / 2 };
      // Clic en la rueda/pill: quedarse en ESTA pantalla. El atajo sí vuela
      // al mouse/foco (otra app puede estar en el otro monitor).
      const cursor = opts.anchored ? null : await activeAnchorPoint();
      const anchor = cursor ?? pillCenter;
      const dest = resolveSlot(slot, areas, size, anchor);
      if (Math.hypot(dest.x - at.x, dest.y - at.y) < 2) return;
      await flyTo(dest);
      return;
    }

    if (opts.anchored) return;

    // Clipboard / textos: el atajo trae la pill al mouse y abre desde ahí.
    if (id === "clipboard" || id === "snippets") {
      await stage.loadAreas();
      const cursor = (await cursorPoint()) ?? (await activeAnchorPoint());
      if (!cursor) return;
      const dest = {
        x: cursor.x - size.w / 2,
        y: cursor.y - size.h / 2,
      };
      if (Math.hypot(dest.x - at.x, dest.y - at.y) < 2) return;
      await flyTo(dest);
    }
  }

  /**
   * Camino ÚNICO de activación: catálogo, ToolRail, atajo global y rueda.
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
      cancelPendingCollapse();
      await closeWheel({ returnHome: false });
      // El cierre acelerado ya se consumió; el resto del acto usa las curvas
      // normales. Dejarlo puesto no rompe nada, pero miente sobre el estado.
      wheelQuick = false;
      surfaces.resetInteraction();

      const intent = slotIntent(
        id,
        spatialToolOpen(id),
        // Con `force` la distancia no decide nada: ahorrarse el IPC del cursor.
        force ? 0 : await cursorMovePx(id),
        FLIGHT_SKIP_PX,
        { force },
      );

      if (intent === "close") {
        if (spatialIntent === id) spatialIntent = null;
        await dismissSpatialTool(id).catch(() => {});
        if (slotPending) return;
        if (id === "launcher" || id === "clipboard" || id === "snippets") {
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

      if (isSpatialTool(id)) spatialIntent = id;
      else if (id === "dictation") spatialIntent = null;

      // Desacoplar SIEMPRE, no solo tools con slot. Clipboard/textos no
      // tienen slot fijo: sin esto la pestaña (~10 px) se queda, el morph a
      // barra no corre y el float ancla contra esa geometría (o no abre).
      if (surface === "edge") {
        const undocked = undockForSummon({ surface, dock });
        surface = undocked.surface;
        dock = undocked.dock;
        await tick();
        await reconcile(target);
      }

      await flyToToolSlot(id, { anchored: force });
      if (gen !== slotGen) return;
      if (!shouldCommitShow(slotPending)) return;
      // Hit-rect fresco: sin esto el float ancla contra la geometría vieja
      // (la caja de la rueda, o la posición previa al vuelo).
      await surfaces.flush();
      await executeToolAction(id);
      // El float publica su zona un tick después del evento de ancla.
      await tick();
      await surfaces.flush();
    } catch (err) {
      console.warn("activate-tool-slot", err);
    } finally {
      if (hold) void setOverlayPointerGesture(false).catch(() => {});
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
      cancelPendingCollapse();
      await closeWheel({ returnHome: false });
      await flyToToolSlot(id);
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
   * La posición se lee con `overlayCursor` (Rust), no solo con `pointermove`:
   * cerca del borde la barra de tareas se queda con el mouse y el webview deja
   * de recibir eventos; sin el cursor global el arrastre se cortaba a mitad.
   */
  const DRAG_THRESHOLD = 4;
  let dragOrigin: {
    /** Null hasta el primer tick: lo siembra el cursor de Rust, no el evento. */
    cx: number | null;
    cy: number | null;
    ox: number;
    oy: number;
    pointerId: number;
  } | null = null;
  let dragMoved = false;
  let dragRaf = 0;
  /** Ya se vio el botón apretado: recién ahí un `false` significa "soltó". */
  let dragSawDown = false;
  /** Icono de la isla donde arrancó el gesto, si arrancó en uno. */
  let islandPressTool: ToolId | null = null;
  /** El gesto arrancó sobre el aviso/botón de consola de agentes. */
  let agentChipPressed = false;
  let agentChipPreferBind = false;
  /** Un drag sobre el chip no debe terminar convertido en click. */
  let suppressAgentChipClick = false;
  let suppressAgentChipClickTimer = 0;
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
    // Abierta, la rueda se agarra por el núcleo y SOLO por ahí.
    //
    // Al revés que la isla: ahí todo es asa porque no queda hueco donde
    // apuntar, pero acá cada gajo es una elección, y un umbral de 4 px sobre
    // un menú radial convertiría cualquier clic con pulso en un arrastre. El
    // núcleo mide 58 px y es donde ya está el puntero cuando la rueda acaba de
    // abrirse, así que no hay que ir a buscarlo. El trato es el del disco en
    // reposo: clic cierra, arrastre mueve.
    const onWheelCore = el.closest(".pw-core") !== null;
    if (
      !onIsland &&
      !onAgentChip &&
      !onWheelCore &&
      el.closest("button, a, input, textarea, [data-no-drag]")
    ) {
      return;
    }
    agentChipPressed = onAgentChip;
    agentChipPreferBind = onAgentChip && (event.ctrlKey || event.metaKey);
    wheelCorePressed = onWheelCore;
    // El origen NO sale del evento del DOM: `clientX` mide contra la ventana, y
    // traducirlo obliga a confiar en dónde cree el CSS que está `.ov`, que es
    // un dato que llega por evento desde Rust y se atrasa justo cuando la
    // ventana se acaba de reencuadrar. Lo siembra el primer tick, con el mismo
    // cursor con el que se sigue el resto del gesto.
    dragOrigin = {
      cx: null,
      cy: null,
      ox: at.x,
      oy: at.y,
      pointerId: event.pointerId,
    };
    dragMoved = false;
    dragSawDown = false;
    // La ventana ya no se estira al escritorio entero durante el arrastre, así
    // que el puntero puede salirse de ella. Sin capturarlo, el `pointerup` de
    // afuera no llega y el gesto queda pegado.
    try {
      rootEl?.setPointerCapture(event.pointerId);
    } catch {
      // Puntero ya liberado: el oyente de `window` alcanza.
    }
    window.addEventListener("pointerup", endDrag, true);
    window.addEventListener("pointercancel", endDrag, true);
    // Armar el overlay YA: esperar el umbral de 4px dejaba un hueco donde
    // Rust desarma y el pointerup se pierde.
    surfaces.dragging = true;
    if (!dragRaf) dragRaf = requestAnimationFrame(() => void tickDrag());
  }

  async function tickDrag() {
    dragRaf = 0;
    const origin = dragOrigin;
    if (!origin) return;

    const [cur, down] = await Promise.all([
      overlayCursor().catch(() => null),
      overlayPrimaryDown().catch(() => null),
    ]);
    // Fin del gesto por Win32 y no por el DOM: soltar sobre la barra de tareas
    // no manda `pointerup` acá, y el arrastre quedaba colgado con el hit-rect
    // a pantalla completa. Se exige haberlo visto apretado antes, para que un
    // cuadro madrugador no aborte el arrastre apenas empieza.
    if (down === true) dragSawDown = true;
    if (dragSawDown && down === false) {
      endDrag();
      return;
    }
    if (cur && dragOrigin === origin) {
      // Primer cuadro: es la semilla, no un movimiento.
      if (origin.cx === null || origin.cy === null) {
        origin.cx = cur.x;
        origin.cy = cur.y;
      } else {
        const dx = cur.x - origin.cx;
        const dy = cur.y - origin.cy;
        if (!dragMoved && Math.hypot(dx, dy) > DRAG_THRESHOLD) {
          dragMoved = true;
        }
        if (dragMoved) {
          stage.moveTo({ x: origin.ox + dx, y: origin.oy + dy });
          at = stage.at();
          // Despegar en cuanto se aleja de verdad: si esperáramos a soltar, la
          // isla arrastraría su forma de pestaña por toda la pantalla.
          releaseDockIfFar(cur);
        }
      }
    }

    if (dragOrigin) {
      dragRaf = requestAnimationFrame(() => void tickDrag());
    }
  }

  function stopDragWatch() {
    const pointerId = dragOrigin?.pointerId;
    dragOrigin = null;
    if (pointerId !== undefined && rootEl?.hasPointerCapture(pointerId)) {
      rootEl.releasePointerCapture(pointerId);
    }
    if (dragRaf) {
      cancelAnimationFrame(dragRaf);
      dragRaf = 0;
    }
    surfaces.dragging = false;
    window.removeEventListener("pointerup", endDrag, true);
    window.removeEventListener("pointercancel", endDrag, true);
  }

  /**
   * Hogar nuevo después de arrastrar la rueda abierta.
   *
   * `at` es la esquina del cuadrado de la rueda, no la del disco. Al cerrar,
   * `wheelCollapse` encoge con pivote al centro, así que la pill queda
   * centrada donde estaba el núcleo: ese es el hogar. Guardar `at` tal cual
   * mandaría el disco ~100 px arriba y a la izquierda al volver a casa —y el
   * cierre, que vuela al hogar, desharía el arrastre a la vista.
   */
  function rehomeFromWheel(): void {
    const size = stage.applied() ?? box;
    // El mismo tamaño que va a pedir `wheelCollapse`: `barW` no se re-mide
    // mientras la rueda manda, así que ambos leen la barra en reposo.
    const rest = windowFor(contentFor("none", barW));
    home = {
      x: Math.round(at.x + (size.w - rest.w) / 2),
      y: Math.round(at.y + (size.h - rest.h) / 2),
    };
    void savePillHome(home.x, home.y);
  }

  /** Soltar sin haber movido = clic. Abre la rueda aunque haya cola o grabación. */
  function endDrag() {
    const wasClick = dragOrigin !== null && !dragMoved;
    const moved = dragMoved;
    const pressedTool = islandPressTool;
    const pressedAgentChip = agentChipPressed;
    const preferAgentBind = agentChipPreferBind;
    const pressedWheelCore = wheelCorePressed;
    islandPressTool = null;
    agentChipPressed = false;
    agentChipPreferBind = false;
    wheelCorePressed = false;
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
      // La rueda abierta se mueve y se queda abierta: ni se acopla (no es una
      // isla mientras manda) ni cambia de sitio al cerrar.
      if (surface === "wheel") {
        rehomeFromWheel();
        return;
      }
      // Arrastrar redefine el hogar: la pill se queda donde la dejaste —o
      // pegada al canto, si la soltaste cerca de uno. Si el gesto arrancó sobre
      // un icono, mover cancela la elección: querías moverla, no abrirla.
      settleDock();
      home = { ...at };
      void savePillHome(at.x, at.y);
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
      activateAgentChip(preferAgentBind);
      return;
    }
    // Soltar el núcleo sin haber movido sigue siendo cerrar. Se decide acá y
    // no por el click nativo del botón porque el arrastre captura el puntero:
    // dejarlo en manos del click sería confiar en cómo cada motor lo re-apunta.
    // Un segundo cierre no molesta: `closeWheel` sale de una si ya no está.
    if (wasClick && pressedWheelCore) {
      void closeWheel();
      return;
    }
    if (wasClick && pressedTool) {
      activateFromIsland(pressedTool);
      return;
    }
    // Acoplada, el clic abre la rueda igual: la isla es la pill, no otro
    // control. Sale del canto a volar como siempre y vuelve al soltar.
    if (wasClick && (surface === "none" || surface === "edge")) {
      void openWheel();
    }
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

    (async () => {
      // Los monitores y el hogar, antes de nada: el primer reencuadre ya los
      // necesita para clampear, y sin hogar la pill arrancaría en 0,0.
      await stage.loadAreas();
      const saved = await pillHome().catch(() => null);
      if (saved) {
        home = saved;
        stage.moveTo(saved);
        at = stage.at();
        // El hogar guardado es solo un punto: si quedó a ras de un borde
        // exterior, es que estaba acoplada ahí. Se deduce en vez de guardarse
        // para no migrar el formato de `pill_home`.
        const size = stage.applied() ?? windowFor({ w: PILL.bar, h: PILL.bar });
        const edge = dockedEdgeAt(
          { x: at.x, y: at.y, w: size.w, h: size.h },
          stage.workAreas(),
        );
        if (edge) {
          dock = { edge, expanded: false };
          surface = "edge";
        }
      }
      // Recién ahora hay hogar de verdad: antes de esto, un resize temprano
      // re-asentaría la pill sobre el {0,0} inicial.
      homeRestored = true;
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
      onLauncherBubbleDismiss(() => void maybeReturnHomeAfterFloat("launcher")),
      on("activate-tool-slot", (tool) => requestActivateAtSlot(tool)),
      on("overlay-session-started", () => {
        surfaces.resetInteraction();
        void closeWheel({ returnHome: false });
      }),
      on("fly-tool-slot", (tool) => void flySlotOnly(tool)),
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
        // El summon fija un hogar nuevo: la pill se queda donde la llamaste.
        home = { ...at };
        void savePillHome(at.x, at.y);
      }),
    );

    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && (surface === "wheel" || openingWheel)) {
        event.preventDefault();
        event.stopPropagation();
        if (openingWheel && surface !== "wheel") {
          // Aborta el vuelo; `openWheelInner` vuelve al hogar al ver el epoch.
          cancelFlight();
          collapseEpoch += 1;
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
      if (dragMoved) {
        // Solo el clic que terminó el arrastre. La bandera vive hasta el
        // próximo `beginDrag`, y sin consumirla acá un arrastre de la rueda
        // dejaba sordo el cierre por clic afuera de ahí en adelante.
        dragMoved = false;
        return;
      }
      if (openingWheel && surface !== "wheel") {
        cancelFlight();
        collapseEpoch += 1;
        return;
      }
      if (surface === "wheel") void closeWheel();
    };

    window.addEventListener("keydown", onKey, true);
    // El viewport que crece tarde (boot tras hibernar) y el reencuadre de
    // Rust tras un cambio de monitores: ambos re-asientan el hogar.
    window.addEventListener("resize", queueResettle);
    unlisteners.push(onOverlayReady(() => queueResettle()));
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
      window.clearTimeout(suppressAgentChipClickTimer);
      window.clearTimeout(suppressWheelCoreClickTimer);
      window.clearTimeout(resettleTimer);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("resize", queueResettle);
      unlisteners.forEach((u) => u.then((fn) => fn()));
    };
  });
</script>

<!-- Testigo de grabación (barra flotante). En la rueda y la isla acoplada
     la parada es una gota líquida colgada, no este botón. -->
{#snippet recDot(label: string)}
  <button
    type="button"
    class="p-rec"
    data-no-drag
    onclick={toggleRecord}
    disabled={busy}
    aria-label={label}
    use:tip={btWarning ?? label}
  >
    <span class="p-rec-square" aria-hidden="true"></span>
  </button>
{/snippet}

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
  data-edge={surface === "edge" ? dock?.edge : undefined}
  style="left: {at.x}px; top: {at.y}px; width: {box.w}px; height: {box.h}px; --island-tool: {PILL.islandTool}px; --island-gap: {PILL.islandGap}px; --rec-drop: {PILL.recDrop}px; --rec-drop-gap: {PILL.recDropGap}px"
  bind:this={rootEl}
  onpointerdown={beginDrag}
>
  <!-- Acoplada al borde. `.p-island-skin` llena el cuerpo de la tira y es
       lo que se mide: la silueta líquida sale de ahí. Si hay grabación, una
       gota cuelga hacia adentro, fundida, con el stop encima. -->
  <!-- Montada mientras haya isla O gotas vivas. Las gotas NO se montan con la
       apertura: si nacieran ya abiertas no habría estado inicial desde el cual
       transicionar y el CSS pintaría el final directo. Existen desde que la
       pill se acopla, cerradas, y la clase las abre. -->
  {#if surface === "edge" || beadsAlive}
    <div class="p-island" class:is-open={islandOpen}>
      <div class="p-island-body">
        <i class="p-island-skin" {@attach trackIsland} aria-hidden="true"></i>
        <div
          class="p-island-tools"
          class:is-open={islandOpen}
          class:is-column={peekEdgeAxis === "x"}
          style="--n: {islandSlots}"
        >
          {#each wheelTools as tool, i (tool.id)}
            {@const slot = i + islandLiveSlots(activity)}
            <button
              type="button"
              class="p-island-tool"
              style="--i: {slot}; --s: {Math.abs((islandSlots - 1) / 2 - slot)}"
              use:tip={`${tool.label} — ${tool.short}`}
              aria-label="{tool.label}. {tool.short}"
              {@attach islandAttachers[i]}
              onpointerdown={() => (islandPressTool = tool.id)}
            >
              <ToolIcon id={tool.id} size={18} strokeWidth={1.6} />
            </button>
          {/each}
        </div>
      </div>
      {#if recording}
        <button
          type="button"
          class="p-live-drop"
          {@attach trackLive}
          data-no-drag
          use:tip={btWarning ?? t("pill.stopRecord")}
          aria-label={t("pill.stopRecord")}
          disabled={busy}
          onpointerdown={(e) => e.stopPropagation()}
          onclick={toggleRecord}
        >
          <span class="p-rec-square" aria-hidden="true"></span>
        </button>
      {/if}
    </div>
  {/if}

  <!-- La rueda vive siempre montada. Durante el colapso sigue opaca (aunque
       `revealed` ya sea false) hasta que el root encoge: el handoff al stack
       ocurre en el mismo centro, no con un fundido top-left ↔ centro. -->
  <div class="p-wheel" class:is-open={wheelChrome} data-no-drag>
    <ParticleWheel
      compact
      wheelNav
      particles={false}
      revealed={wheelShown}
      tools={wheelTools}
      bind:activeId={wheelTool}
      caption={t("tools.wheelCaption")}
      centerLabel={t("tools.wheelClose")}
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
      onSelect={(id) => activateTool(id)}
      onCenter={() => {
        // Arrastrarla por el núcleo no debe además cerrarla: el click nativo
        // llega igual después del pointerup.
        if (suppressWheelCoreClick) return;
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
        {#if !discOnly}
          <i
            class="p-skin-tail"
            class:is-from-start={consoleSide === "left"}
            {@attach trackTail}
          ></i>
        {/if}
      </div>

      <div
        class="p-shell"
      >
        <div
          class="p-bar"
          class:is-disc-only={discOnly}
          class:is-console-start={consoleSide === "left" &&
            agentAlert &&
            activity === "idle" &&
            !hasQueue}
          bind:this={barEl}
        >
          {#if activity === "recording"}
            {@render recDot(t("pill.stopRecord"))}
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
          {:else if dictation === "listening"}
            <!-- Escuchando: micrófono + ondas, sin texto. El ícono dice QUÉ está
               pasando (el mic está abierto) y las ondas dicen que te oye; la
               palabra "Dictando" no agregaba nada sobre esos dos. Todo el
               conjunto es el botón de parada. Los otros estados sí necesitan
               texto: "Transcribiendo…" y "Error" no se pueden mostrar con una
               animación. -->
            <button
              type="button"
              class="p-dict-wave"
              data-no-drag
              onclick={toggleDictate}
              disabled={busy}
              aria-label={t("pill.stopDictate")}
              use:tip={t("pill.dictatingHint")}
            >
              <ToolIcon id="dictation" size={16} strokeWidth={1.5} />
              <Waveform mic={levels.mic} bars={18} variant="voice" live />
            </button>
          {:else if activity === "dictating"}
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
            <span class="p-label" aria-live="polite">{dictationLabel(dictation)}</span>
          {:else if hasQueue}
            <!-- La cola es un badge sobre el disco, no un reemplazo: antes borraba
               la pill entera y con ella el acceso a la rueda. -->
            <!-- Sin marca mientras la rueda manda: el stack queda en el
               top-left del root grande y una segunda «a» fantasma se veía ahí. -->
            {#if !wheelChrome}
              <span class="p-mark is-disc"
                ><AticMark size={20} strokeWidth={1.4} /></span
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
          {:else}
            <!-- Reposo: disco con la marca. Un clic abre la rueda; el centro de
               la rueda la cierra. El doble clic ya no hace falta.
               Con la rueda abierta/colapsando no se monta: el único «a» visible
               es el de ParticleWheel (centro). El stack sigue midiendo el
               disco vía `.p-bar.is-disc-only` (40px fijos). -->
            {#if !wheelChrome}
              <span
                class="p-mark is-disc"
                use:tip={[
                  wheelShortcut
                    ? t("pill.toolsWithShortcut", {
                        shortcut: formatShortcut(wheelShortcut),
                      })
                    : "",
                  t("pill.clickTools"),
                  t("pill.dragMove"),
                ]
                  .filter(Boolean)
                  .join(" · ")}
              >
                <AticMark size={22} strokeWidth={1.4} />
              </span>
            {/if}
            <!-- Aviso del agente: aparece solo si hay algo que decir. Es un chip
               junto al disco y no un reemplazo, porque el disco sigue siendo la
               puerta a la rueda. -->
            {#if agentAlert && !wheelChrome}
              <button
                type="button"
                class="p-agent"
                class:is-waiting={chip.tone === "waiting"}
                class:is-working={chip.tone === "working"}
                class:is-ready={chip.tone === "ready"}
                class:is-count={chip.tone === "count"}
                onclick={(e) => onAgentChipClick(e)}
                use:tip={agentChipTitle}
                aria-label={agentChipAria}
              >
                <span class="p-agent-ico" aria-hidden="true">
                  <ToolIcon id="agents" size={11} strokeWidth={1.7} />
                </span>
                {#if chip.tone === "waiting"}
                  <span class="p-agent-count">{t("pill.permission")}</span>
                {:else if chip.tone === "ready"}
                  <span class="p-agent-msg">{agentReadyLabel}</span>
                {:else if chip.tone === "count"}
                  <span class="p-agent-count">{chip.label}</span>
                {/if}
              </button>
            {/if}
            <!-- Hay versión nueva. Mismo sitio y misma cápsula que el aviso de
               agentes: el disco sigue siendo la puerta a la rueda, y esto es
               algo que la pill cuenta, no algo que la reemplace. -->
            {#if updateChip && !wheelChrome}
              <button
                type="button"
                class="p-update"
                class:is-ready={updateChip.tone === "ready"}
                class:is-busy={updateChip.tone === "busy"}
                disabled={appUpdate.busy}
                onclick={() => void appUpdate.advance()}
                use:tip={updateChip.label}
                aria-label={updateChip.label}
              >
                <span class="p-update-ico" aria-hidden="true">
                  <Icon icon={updateChip.icon} size={11} strokeWidth={1.9} />
                </span>
                <span class="p-update-text">{updateChip.text}</span>
              </button>
            {/if}
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
    <AgentAuthCard
      permission={authView.permission}
      busy={authBusy}
      onOpenConsole={() => void openAgentsConsole()}
      onDecide={(d) => void decideAuth(d)}
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

  /* El vuelo al hogar y al cursor. Solo mientras dura: si la transición
     quedara siempre puesta, cada reencuadre de la barra compacta —el timer
     tictaqueando, el badge de la cola— se arrastraría con cada tick. */
  .p-root.is-flying {
    transition:
      left var(--flight-dur) var(--ease-smooth-out),
      top var(--flight-dur) var(--ease-smooth-out);
  }

  .p-root:active {
    cursor: grabbing;
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
    transition:
      width var(--island-open-dur) var(--ease-liquid),
      height var(--island-open-dur) var(--ease-liquid),
      left var(--island-open-dur) var(--ease-liquid),
      top var(--island-open-dur) var(--ease-liquid);
  }

  /* Arrastrando no: la posición tiene que seguir al dedo sin inercia. */
  .p-root.is-docked.is-dragging {
    transition: none;
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
    flex-direction: column;
  }

  .p-root[data-edge="bottom"] .p-island {
    flex-direction: column-reverse;
  }

  .p-root[data-edge="left"] .p-island {
    flex-direction: row;
  }

  .p-root[data-edge="right"] .p-island {
    flex-direction: row-reverse;
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

  .p-island-skin {
    position: absolute;
    inset: 0;
    display: block;
    border-radius: 999px;
  }

  .p-live-drop {
    display: grid;
    flex: 0 0 auto;
    width: var(--rec-drop);
    height: var(--rec-drop);
    border: 0;
    margin: 0;
    padding: 0;
    border-radius: 999px;
    background: transparent;
    color: var(--rec);
    cursor: pointer;
    place-items: center;
    transition: transform var(--duration-quick) var(--ease-smooth-out);
  }

  .p-live-drop .p-rec-square {
    width: 10px;
    height: 10px;
    animation: p-island-rec-pulse 1.6s linear infinite;
  }

  .p-live-drop:hover:not(:disabled),
  .p-live-drop:focus-visible {
    transform: scale(1.04);
  }

  .p-live-drop:active:not(:disabled) {
    transform: scale(0.96);
  }

  .p-live-drop:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .p-live-drop:disabled {
    cursor: default;
    opacity: 0.55;
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
    width: var(--island-tool);
    height: var(--island-tool);
    border: 0;
    border-radius: 999px;
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

  .p-island-tool:hover,
  .p-island-tool:focus-visible {
    background: color-mix(in sRGB, var(--text) 14%, transparent);
    color: var(--text);
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

  /* El disco de fondo se fue: ahora la superficie la ponen el núcleo y las
     gotas de `ParticleWheel`, que escalan y viajan por su cuenta. La
     marca del centro sigue sin escalar — es el punto fijo del morph. */

  /* ─── Cuerpo líquido (barra) ─────────────────────────────────────────── */
  .p-liquid {
    /* `--goo-grow` viene de `app.css`. Sin compensarlo, el disco de reposo
       saldría de 43 en vez de 40 y se comería casi la mitad del respiro que
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
     * Las medidas de acá abajo están escritas como `40px - goo-grow * 2`
     * porque el endurecido del filtro devolvía 2.8 px por lado. El contorno
     * trazado NO engorda: pasa por la geometría pedida. Con el descuento
     * puesto, el disco de 40 se medía de 37.2.
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
     por los dos lados, así que el borde final cae exactamente en los 40. */
  .p-skin-bar {
    height: calc(40px - var(--goo-grow) * 2);
    width: calc(40px - var(--goo-grow) * 2);
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
    inset: 0;
    border-radius: 999px;
    animation: p-skin-arrive var(--panel-dur) var(--morph-ease);
  }

  /* Consola al inicio (cerca del borde derecho): la gota llega desde la izquierda. */
  .p-skin-tail.is-from-start {
    animation-name: p-skin-arrive-start;
  }

  @keyframes p-skin-arrive {
    from {
      inset: 7px 4px 7px calc(100% - 28px);
    }
  }

  @keyframes p-skin-arrive-start {
    from {
      inset: 7px calc(100% - 28px) 7px 4px;
    }
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
    height: 40px;
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
    display: flex;
    width: max-content;
    min-width: 40px;
    height: 100%;

    /* NO encoger. `.p-shell` está topado con `max-width: 100%`, o sea el ancho
       de la VENTANA, así que sin esto la barra se comprimía hasta su min-width
       de 40 px y eso era lo que medíamos: la ventana quedaba en 48, el tope en
       48, la barra en 40. Un abrazo mortal donde la pill no podía crecer sola.
       Se notaba al dictar con el atajo —quedaba redonda con las ondas
       recortadas adentro— pero no con la rueda, porque ahí la ventana se
       redimensiona antes por otro camino y destraba el tope.
       Overflow lo tapa `.p-shell` durante el frame que la ventana tarda. */
    flex-shrink: 0;
    align-items: center;
    gap: 8px;
    padding: 0 12px 0 10px;
    white-space: nowrap;
  }

  /* Lo que entra a la barra se funde en vez de aparecer de golpe.
     Va sobre los hijos DIRECTOS porque son los que Svelte monta y desmonta al
     cambiar de estado; lo que sobrevive al cambio —el timer, que solo cambia su
     texto— no se re-anima y no parpadea.
     Solo opacidad: `transform` acá pisaría el `scale` de hover de los botones,
     porque una animación gana sobre una transición. */
  .p-bar > * {
    animation: p-in var(--morph-fade-dur) var(--morph-close-ease);
  }

  @keyframes p-in {
    from {
      opacity: 0;
    }
  }

  /* Reposo: disco exacto. Con el padding de la barra quedaba elipse. */
  .p-bar.is-disc-only {
    width: 40px;
    justify-content: center;
    padding: 0;
  }

  /*
   * Consola al lado izquierdo del disco: invertir el flex mantiene marca y
   * chip en el DOM (mark → agent) pero pinta agent | mark.
   */
  .p-bar.is-console-start {
    flex-direction: row-reverse;
    padding: 0 10px 0 12px;
  }

  .p-mark {
    display: inline-flex;
    flex-shrink: 0;
    color: var(--text);
    line-height: 0;
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
  .p-dict-wave {
    display: flex;
    height: 100%;
    align-items: center;
    gap: 8px;
    border: 0;
    background: none;
    color: var(--text);
    cursor: pointer;
    padding: 0 2px;
    margin: 0;
  }

  /* El micrófono no se comprime: es el que da el contexto de la animación. */
  .p-dict-wave :global(svg) {
    flex-shrink: 0;
  }

  .p-dict-wave:disabled {
    cursor: default;
  }

  /* ─── Botones ───────────────────────────────────────────────────────── */
  .p-icon,
  .p-rec,
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
    /* 40×40: hit area de chrome denso sin pseudo que se solape con vecinos. */
    width: 40px;
    height: 40px;
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
  }

  .p-icon:hover {
    color: var(--text);
    background: color-mix(in sRGB, var(--text) 8%, transparent);
  }

  .p-rec,
  .p-dict {
    width: 32px;
    height: 32px;
    border-radius: 999px;
  }

  .p-rec {
    background: color-mix(in sRGB, var(--rec) 24%, transparent);
    color: var(--rec);
  }

  .p-rec-square {
    width: 8px;
    height: 8px;
    background: currentColor;
  }

  .p-dict {
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
  .p-rec:active:not(:disabled),
  .p-dict:active:not(:disabled) {
    transform: scale(0.96);
  }

  .p-icon:disabled,
  .p-rec:disabled,
  .p-dict:disabled {
    opacity: 0.45;
    cursor: default;
  }

  /* El anillo va por dentro y no como `outline`: la pill vive sobre una
     silueta redondeada y un contorno exterior se sale del contorno fundido. */
  .p-icon:focus-visible,
  .p-rec:focus-visible,
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

  /* Trabajando: presente, pulso linear continuo (alive, no noisy). */
  .p-agent.is-working {
    background: color-mix(in sRGB, var(--text) 7%, transparent);
    color: var(--muted);
    animation: p-agent-pulse 2s linear infinite;
  }

  /* Listo / respuesta sin leer: affordance clara, no solo un número. */
  .p-agent.is-ready {
    background: color-mix(in sRGB, var(--ok) 14%, transparent);
    color: var(--ok);
    animation: p-agent-ready-in var(--duration-very-slow, 500ms) var(--ease-smooth-out)
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
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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

  /* Pastilla viva mientras el agente trabaja: brillo del fill, no anillo.
     Un inset se leía como borde y cortaba el cuello fundido con el float. */
  .p-liquid.is-working {
    animation: p-liquid-alive 2.2s linear infinite;
  }

  /*
   * Auth: mismo `.float-emerge` que clipboard/agentes (nace/vuelve a la pill).
   * Solo alarga la apertura; el viaje y el scale viven en `app.css`.
   */
  .p-auth-host {
    --float-open-dur: 150ms;
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
    .p-root.is-flying,
    .p-wheel,
    .p-wheel.is-open,
    .p-stack,
    .p-shell,
    .p-liquid,
    .p-skin-tail,
    .p-icon,
    .p-rec,
    .p-dict,
    .p-queue-btn,
    .p-agent,
    .p-update,
    .p-auth-host,
    .p-island-tool,
    .p-live-drop,
    /* La animación vive en los hijos, no en la gota: sin ellos acá el
       pulso seguiría con reduce activo. */
    .p-live-drop .p-rec-square,
    .p-live-drop .p-dict-glyph {
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
