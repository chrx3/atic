<script lang="ts">
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
  import { sessionEffect } from "$domain/session";
  import Waveform from "$lib/Waveform.svelte";
  import AticMark from "$lib/AticMark.svelte";
  import { liquid } from "$surfaces/overlay/group.svelte";
  import { publishEmergeSkin } from "$surfaces/overlay/floatEmergeSkin";
  import type { Rect } from "$lib/liquid/geometry";
  import { RectTracker } from "$lib/liquid/measure.svelte";
  import { boxShape, gapBetween, pillShape } from "$lib/liquid/geometry";
  import { REACH } from "$lib/liquid/constants";
  import ToolIcon from "$lib/ToolIcon.svelte";
  import { agents } from "$lib/agentSessions.svelte";
  import ParticleWheel from "$lib/ParticleWheel.svelte";
  import { TOOLS, AGENTS_ENABLED, type ToolId } from "$lib/tools";
  import { formatShortcut } from "$lib/format";
  import Icon from "$ui/Icon.svelte";
  import { X } from "$lib/icons";
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
    morphsInPlace,
    pivotFor,
    stepWheel as nextWheelTool,
    wheelChromeActive,
    wheelKeyAction,
    type Surface,
  } from "$surfaces/overlay/pill/pillPlan";
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
  } from "$ipc/agents";
  import {
    clipboardAlwaysOnTop,
    hideClipboardWindow,
    onClipboardBubbleDismiss,
  } from "$ipc/clipboard";
  import { startCaptureSession } from "$ipc/captures";
  import { getConfig, showMainWindow } from "$ipc/config";
  import { on } from "$ipc/events";
  import { hideLauncher, onLauncherBubbleDismiss } from "$ipc/search";
  import {
    hideSnippetsWindow,
    onSnippetsBubbleDismiss,
    snippetsAlwaysOnTop,
  } from "$ipc/snippets";
  import { executeToolAction } from "$core/toolActions";
  import {
    isSpatialTool,
    resolveSlot,
    slotForTool,
  } from "$surfaces/overlay/toolSlots";
  import {
    onOverlayDismiss,
    onPillRadialPress,
    onPillRadialRelease,
    onPillReset,
    overlayCursor,
    pillHome,
    pillTrace,
    savePillHome,
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
   * Es lo que hace que «corre en segundo plano» signifique algo: la consola
   * puede estar cerrada y la sesión sigue viva, así que la pill tiene que ser
   * el lugar donde te enteras de que respondió o de que te está esperando.
   */
  const agentAlert = $derived(
    AGENTS_ENABLED &&
      (agents.unread > 0 || agents.working || agents.waiting > 0),
  );
  const agentWorking = $derived(agents.working && agents.waiting === 0);
  const agentReady = $derived(
    agents.unread > 0 && agents.waiting === 0 && !agents.working,
  );
  const agentReadyLabel = $derived(agents.readyLabel ?? "Listo");
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

  // Cada cambio de estado arranca una animación de CSS: hay que volver a mirar.
  // La posición también cuenta: un vuelo no toca los estados de arriba, y sin
  // `at` la silueta quedaba en el sitio del que la pill se fue.
  $effect(() => {
    void surface;
    void discOnly;
    void barW;
    void at.x;
    void at.y;
    tracker.wake();
  });

  onMount(() => () => tracker.stop());

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
    const shapes = [];
    // La gota, si está. El disco solo mientras la gota no lo cubra: ver
    // `discJoinsTail` — publicar ambos en reposo engordaba el lado izquierdo.
    if (r.tail) shapes.push(pillShape(at(r.tail)));
    if (r.bar && (!r.tail || discJoinsTail(r.bar, r.tail))) {
      shapes.push(pillShape(at(r.bar)));
    }
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
  /** Ancho real de la barra, medido del DOM. Sin esto habría que mantener una
   *  tabla de anchos mágicos por estado — la fuente original del desajuste. */
  let barW = $state<number>(PILL.bar);
  let barEl = $state<HTMLElement | null>(null);

  const target = $derived(windowFor(contentFor(surface, barW)));

  /**
   * Lado del chip de consola: opuesto al borde horizontal más cercano.
   * Usa el centro de la caja de la pill (no solo el disco) para no saltar
   * al expandirse el aviso.
   */
  const consoleSide = $derived(
    consoleSideFor(stage.workAreas(), at, box),
  );

  /** Traza al log de Rust. Fire-and-forget: no debe alterar el flujo ni fallar. */
  function trace(msg: string) {
    void pillTrace(msg).catch(() => {});
  }

  async function openAgentsConsole() {
    try {
      await showAgentsWindow();
      agentsConsoleOpen = true;
    } catch (err) {
      console.warn("abrir consola de agentes", err);
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
    const w = 320;
    // Hueco corto: tiene que quedar dentro de REACH para que nazca el cuello.
    const gap = 8;
    const areas = stage.workAreas();
    // `consoleSide` ya es el lado libre: "right" crece a la derecha, "left" a la izquierda.
    let x =
      consoleSide === "right" ? at.x : at.x + box.w - w;
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
    return { surface, collapsingFrom };
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
      | typeof MOTION.morphOpen
      | typeof MOTION.morphClose
      | typeof MOTION.morphQuick,
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
  $effect(() => (liquidEl ? surfaces.add("pill-skin", liquidEl) : undefined));

  /** La tarjeta de auth también tiene que armar hit-rects o queda click-through. */
  $effect(() =>
    authEl && authAlive ? surfaces.add("agent-auth", authEl) : undefined,
  );

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
        return "Dictando…";
      case "transcribing":
        return "Transcribiendo…";
      case "pasted":
        return dictationMessage ?? "Pegado";
      case "error":
        return dictationMessage ?? "Error";
      default:
        return "Dictar";
    }
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
      // 2) Volar al cursor; cerca de la pill se omite (solo latencia).
      const cursor = await cursorPoint();
      if (cursor) {
        const size = stage.applied() ?? windowFor({ w: PILL.bar, h: PILL.bar });
        const flew = await flyTo(
          {
            x: cursor.x - size.w / 2,
            y: cursor.y - size.h / 2,
          },
          { skipIfNear: FLIGHT_SKIP_PX },
        );
        if (flew < 0 || openEpoch !== collapseEpoch) {
          surface = "none";
          wheelShown = false;
          await flyTo(home, { skipIfNear: 2 });
          return;
        }
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
      await stage.resize(
        next,
        pivotFor({ surface: "wheel", collapsingFrom: null }),
      );
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
    await stage.resize(
      next,
      pivotFor({ surface: "none", collapsingFrom: "wheel" }),
    );
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
    }
    return true;
  }

  /** Cierra la rueda y vuelve al hogar previo al open. No activa nada. */
  async function closeWheel() {
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
      await playCloseMorph(epoch, { returnHome: true });
    } finally {
      if (epoch === collapseEpoch) opening = false;
    }
  }

  /** Soltar la tecla: activa lo apuntado si la rueda llegó a mostrarse. */
  function onWheelRelease() {
    if (surface !== "wheel") return;
    if (wheelShown && wheelTool) void activateTool(wheelTool);
    else void closeWheel();
  }

  /** Teclado con la rueda abierta. No preselecciona: enfocar un nodo al abrir
   *  dejaría una herramienta armada y soltar la tecla la dispararía. */
  function onWheelKey(event: KeyboardEvent): boolean {
    if (surface !== "wheel" || !wheelShown) return false;
    const action = wheelKeyAction(event.key, event.shiftKey);
    if (!action) return false;
    if (action === "activate") {
      if (wheelTool) void activateTool(wheelTool);
      else void closeWheel();
    } else {
      const next = nextWheelTool(wheelTool, action === "next" ? 1 : -1, TOOLS);
      if (next !== wheelTool) playWheelTick();
      wheelTool = next;
    }
    return true;
  }

  /**
   * La rueda ejecuta la herramienta, no navega la app.
   *
   * Grabar arranca en paralelo al cierre (sin slot). Tools con slot vuelan
   * y recién ahí se ejecutan (dictado, Apps, clipboard, textos, agentes).
   */
  async function activateTool(id: ToolId) {
    if (id === "agents" && !AGENTS_ENABLED) return;
    if (surface !== "wheel") return;
    wheelQuick = true;
    const epoch = ++collapseEpoch;
    opening = true;
    wheelTool = null;

    if (id === "meetings") void toggleRecord();

    try {
      // Mismo orden que `closeWheel`: chrome de rueda activo hasta encoger.
      collapsingFrom = "wheel";
      surface = "none";
      const collapsed = await playCloseMorph(epoch);
      if (!collapsed) return;
      // Hit-rect fresco: sin esto el float ancla contra la geometría de la rueda.
      await surfaces.flush();

      if (slotForTool(id)) {
        returnHomeSuppressed = true;
        try {
          // Solo launcher / agentes / dictado vuelan al slot.
          // Clipboard/textos desde la rueda abren junto a la pill (ya en el cursor).
          await flyToToolSlot(id);
          await surfaces.flush();
          await executeToolAction(id);
        } finally {
          returnHomeSuppressed = false;
        }
        return;
      }

      if (isSpatialTool(id)) {
        returnHomeSuppressed = true;
        try {
          await dismissSpatialTools();
          await surfaces.flush();
          await executeToolAction(id);
        } finally {
          returnHomeSuppressed = false;
        }
        return;
      }

      if (id === "captures") await startCaptureSession();
    } catch (err) {
      console.warn("acción de la rueda", err);
    } finally {
      if (epoch === collapseEpoch) opening = false;
      wheelQuick = false;
    }
  }

  /**
   * Exclusive espacial: al abrir otra tool, cierra los floats no fijados
   * (clipboard / textos / agentes / launcher). El pin (“siempre arriba”)
   * mantiene el panel abierto — p. ej. agentes fijado + clipboard para pegar.
   * `returnHomeSuppressed` evita que los dismiss disparen flyTo(home).
   */
  let returnHomeSuppressed = false;
  async function dismissSpatialTools() {
    returnHomeSuppressed = true;
    const [clipPinned, snipPinned, agentsPinned] = await Promise.all([
      clipboardAlwaysOnTop().catch(() => false),
      snippetsAlwaysOnTop().catch(() => false),
      agentsAlwaysOnTop().catch(() => false),
    ]);
    await Promise.all([
      clipPinned ? Promise.resolve() : hideClipboardWindow().catch(() => {}),
      snipPinned ? Promise.resolve() : hideSnippetsWindow().catch(() => {}),
      agentsPinned ? Promise.resolve() : hideAgentsWindow().catch(() => {}),
      hideLauncher().catch(() => {}),
    ]);
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

  /** Vuelve al hogar si la pill quedó en un slot de acción. */
  async function maybeReturnHome() {
    if (returnHomeSuppressed || slotBusy) return;
    if (Math.hypot(home.x - at.x, home.y - at.y) < 2) return;
    try {
      await flyTo(home);
    } catch (err) {
      console.warn("return-to-home", err);
    }
  }

  /** Dismiss de tool con reverse liquid: esperar a que el float se funda. */
  async function maybeReturnHomeAfterFloat(id: "launcher" | "clipboard" | "snippets") {
    await waitSpatialSurfaceGone(id);
    await maybeReturnHome();
  }

  /**
   * Vuela la pill al slot de la tool (work area del monitor actual).
   * Clipboard / textos no tienen slot fijo: vuelan al cursor (atajo / catálogo).
   * No redefine `home`: el destino es posición de acción, no reposo.
   */
  async function flyToToolSlot(id: ToolId): Promise<void> {
    // Al terminar dictado no hace falta volar.
    if (id === "dictation" && dictationStore.active) return;

    if (isSpatialTool(id) || id === "dictation") await dismissSpatialTools();

    const size = stage.applied() ?? windowFor({ w: PILL.bar, h: PILL.bar });
    const slot = slotForTool(id);

    if (slot) {
      const areas = stage.workAreas();
      const anchor = { x: at.x + size.w / 2, y: at.y + size.h / 2 };
      const target = resolveSlot(slot, areas, size, anchor);
      if (Math.hypot(target.x - at.x, target.y - at.y) < 2) return;
      await flyTo(target);
      return;
    }

    // Clipboard / textos: el atajo trae la pill al mouse y abre desde ahí.
    if (id === "clipboard" || id === "snippets") {
      const cursor = await cursorPoint();
      if (!cursor) return;
      const target = {
        x: cursor.x - size.w / 2,
        y: cursor.y - size.h / 2,
      };
      if (Math.hypot(target.x - at.x, target.y - at.y) < 2) return;
      await flyTo(target);
    }
  }

  /**
   * Catálogo / ToolRail / atajo: fly → ejecutar.
   * Si el float espacial ya está abierto, segunda activación = cerrar (toggle).
   */
  let slotBusy = false;
  async function activateAtSlot(id: ToolId) {
    if (slotBusy) return;
    slotBusy = true;
    returnHomeSuppressed = true;
    try {
      cancelPendingCollapse();
      await closeWheel();
      surfaces.resetInteraction();

      if (spatialToolOpen(id)) {
        await dismissSpatialTool(id).catch(() => {});
        // Liberar locks antes del vuelo: `maybeReturnHome` respeta `slotBusy`
        // (y los dismiss IPC pueden haber intentado volver a casa en vano).
        returnHomeSuppressed = false;
        slotBusy = false;
        // Launcher / clipboard / snippets cierran con reverse liquid: esperar
        // a que el hit-rect se vaya antes de volar a casa.
        if (id === "launcher" || id === "clipboard" || id === "snippets") {
          await waitSpatialSurfaceGone(id);
        }
        await maybeReturnHome();
        return;
      }

      await flyToToolSlot(id);
      await surfaces.flush();
      await executeToolAction(id);
    } catch (err) {
      console.warn("activate-tool-slot", err);
    } finally {
      slotBusy = false;
      returnHomeSuppressed = false;
    }
  }

  /** Solo vuelo (PTT: en paralelo al start de Rust). */
  async function flySlotOnly(id: ToolId) {
    returnHomeSuppressed = true;
    try {
      cancelPendingCollapse();
      await closeWheel();
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

  async function openMain() {
    try {
      await showMainWindow();
    } catch {
      // best-effort
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

  function beginDrag(event: PointerEvent) {
    const el = event.target as HTMLElement | null;
    if (!el || event.button !== 0) return;
    if (el.closest("button, a, input, textarea, [data-no-drag]")) {
      return;
    }
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
    // La ventana ya no se estira al escritorio entero durante el arrastre, así
    // que el puntero puede salirse de ella. Sin capturarlo, el `pointerup` de
    // afuera no llega y el gesto queda pegado.
    try {
      rootEl?.setPointerCapture(event.pointerId);
    } catch {
      // Puntero ya liberado: el oyente de `window` alcanza.
    }
    window.addEventListener("pointerup", endDrag);
    window.addEventListener("pointercancel", endDrag);
    if (!dragRaf) dragRaf = requestAnimationFrame(() => void tickDrag());
  }

  async function tickDrag() {
    dragRaf = 0;
    const origin = dragOrigin;
    if (!origin) return;

    const cur = await overlayCursor().catch(() => null);
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
          surfaces.dragging = true;
        }
        if (dragMoved) {
          stage.moveTo({ x: origin.ox + dx, y: origin.oy + dy });
          at = stage.at();
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
    window.removeEventListener("pointerup", endDrag);
    window.removeEventListener("pointercancel", endDrag);
  }

  /** Soltar sin haber movido = clic. En reposo, abre la rueda. */
  function endDrag() {
    const wasClick = dragOrigin !== null && !dragMoved;
    const moved = dragMoved;
    stopDragWatch();
    if (moved) {
      // Arrastrar redefine el hogar: la pill se queda donde la dejaste.
      home = { ...at };
      void savePillHome(at.x, at.y);
      return;
    }
    if (wasClick && activity === "idle" && surface === "none" && !hasQueue) {
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

    (async () => {
      // Los monitores y el hogar, antes de nada: el primer reencuadre ya los
      // necesita para clampear, y sin hogar la pill arrancaría en 0,0.
      await stage.loadAreas();
      const saved = await pillHome().catch(() => null);
      if (saved) {
        home = saved;
        stage.moveTo(saved);
        at = stage.at();
      }
      try {
        wheelShortcut = (await getConfig()).pill_radial_shortcut;
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
      on("activate-tool-slot", (tool) => void activateAtSlot(tool)),
      on("fly-tool-slot", (tool) => void flySlotOnly(tool)),
      onPillRadialPress(() => void openWheel()),
      onPillRadialRelease(() => onWheelRelease()),
      onPillReset(async () => {
        trace("pill-reset (summon)");
        // Misma cancelación que al reabrir: un encoger a medias no debe
        // reescribir la posición después del vuelo al cursor.
        cancelPendingCollapse();
        await closeWheel();
        const cursor = await cursorPoint();
        if (!cursor) return;
        const size = stage.applied() ?? windowFor({ w: PILL.bar, h: PILL.bar });
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
      if (dragMoved) return;
      if (openingWheel && surface !== "wheel") {
        cancelFlight();
        collapseEpoch += 1;
        return;
      }
      if (surface === "wheel") void closeWheel();
    };

    window.addEventListener("keydown", onKey, true);
    unlisteners.push(onOverlayDismiss(onOutside));
    trace(`listeners registrados n=${unlisteners.length}`);

    return () => {
      stopDragWatch();
      window.removeEventListener("keydown", onKey, true);
      unlisteners.forEach((u) => u.then((fn) => fn()));
    };
  });
</script>

<!-- Testigo de grabación: vive fuera de los modos, por eso sobrevive a que se
     abra la rueda encima. Antes desaparecía y con él el botón de detener. -->
{#snippet recDot(label: string)}
  <button
    type="button"
    class="p-rec"
    data-no-drag
    onclick={toggleRecord}
    disabled={busy}
    aria-label="Detener grabación"
    title={btWarning ?? label}
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
    title={label}
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
  style="left: {at.x}px; top: {at.y}px; width: {box.w}px; height: {box.h}px"
  bind:this={rootEl}
  onpointerdown={beginDrag}
>
  <!-- La rueda vive siempre montada. Durante el colapso sigue opaca (aunque
       `revealed` ya sea false) hasta que el root encoge: el handoff al stack
       ocurre en el mismo centro, no con un fundido top-left ↔ centro. -->
  <div class="p-wheel" class:is-open={wheelChrome} data-no-drag>
    <ParticleWheel
      compact
      wheelNav
      particles={false}
      revealed={wheelShown}
      bind:activeId={wheelTool}
      caption="Herramientas"
      onSelect={(id) => void activateTool(id)}
      onCenter={() => {
        void closeWheel();
        void openMain();
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
        class:is-working={agentWorking && activity === "idle" && !hasQueue}
        class:is-ready={agentReady && activity === "idle" && !hasQueue}
        class:is-waiting={agents.waiting > 0 && activity === "idle" && !hasQueue}
      >
        <div
          class="p-bar"
          class:is-disc-only={discOnly}
          class:is-console-start={consoleSide === "left" && agentAlert && activity === "idle" && !hasQueue}
          bind:this={barEl}
        >
          {#if activity === "recording"}
            {@render recDot("Detener grabación")}
            <span class="p-timer">{fmt(elapsed)}</span>
            {#if liveError}
              <span class="p-chip is-error" role="status">Error</span>
            {:else if btWarning}
              <span class="p-chip is-warn" role="status" title={btWarning}>BT</span>
            {:else if liveActive}
              <span class="p-chip" role="status">En vivo</span>
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
              aria-label="Detener dictado"
              title="Dictando · clic para detener"
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
              aria-label="Dictado"
              title={dictationLabel(dictation)}
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
              <span class="p-mark is-disc"><AticMark size={20} strokeWidth={1.4} /></span>
            {/if}
            <span class="p-queue-count">{paste.count}</span>
            <span class="p-queue-text" title={paste.front?.text}>
              {paste.front?.text ?? ""}
            </span>
            <button
              type="button"
              class="p-queue-btn"
              data-no-drag
              disabled={paste.busy}
              onclick={() => void paste.paste()}
            >
              Pegar
            </button>
            {@render iconBtn("Descartar", X, () => void paste.dismiss(), 13)}
          {:else}
            <!-- Reposo: disco con la marca. Un clic abre la rueda; el centro de
               la rueda abre la app. El doble clic ya no hace falta.
               Con la rueda abierta/colapsando no se monta: el único «a» visible
               es el de ParticleWheel (centro). El stack sigue midiendo el
               disco vía `.p-bar.is-disc-only` (40px fijos). -->
            {#if !wheelChrome}
              <span
                class="p-mark is-disc"
                title={[
                  wheelShortcut ? `${formatShortcut(wheelShortcut)} · herramientas` : "",
                  "Clic para las herramientas",
                  "Arrastra para mover",
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
                class:is-waiting={agents.waiting > 0}
                class:is-working={agentWorking && agents.unread === 0}
                class:is-ready={agentReady}
                class:is-count={!agents.waiting && !agentReady && agents.unread > 0}
                data-no-drag
                onclick={() => void openAgentsConsole()}
                title={agents.waiting > 0
                  ? "El agente espera tu permiso"
                  : agentReady
                    ? agentReadyLabel
                    : agents.unread > 0
                      ? `${agents.unread} sin leer`
                      : "El agente está trabajando"}
                aria-label="Abrir la consola de agentes"
              >
                <span class="p-agent-ico" aria-hidden="true">
                  <ToolIcon id="agents" size={11} strokeWidth={1.7} />
                </span>
                {#if agents.waiting > 0}
                  <span class="p-agent-count">permiso</span>
                {:else if agentReady}
                  <span class="p-agent-msg">{agentReadyLabel}</span>
                {:else if agents.unread > 0}
                  <span class="p-agent-count">{agents.unread}</span>
                {/if}
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
     seis gotas de `ParticleWheel`, que escalan y viajan por su cuenta. La
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
   * hueco supera los 8.6 px de alcance— y se expande hacia él hasta fundirse.
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
    animation: p-agent-ready-in var(--duration-very-slow, 500ms)
      var(--ease-smooth-out) both;
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

  /* Pastilla viva mientras el agente trabaja (anillo + brillo suave). */
  .p-shell.is-working {
    animation: p-shell-alive 2.2s linear infinite;
  }

  .p-liquid.is-working {
    animation: p-liquid-alive 2.2s linear infinite;
  }

  .p-shell.is-ready {
    box-shadow: inset 0 0 0 1.5px color-mix(in sRGB, var(--ok) 40%, transparent);
  }

  .p-shell.is-waiting {
    box-shadow: inset 0 0 0 1.5px color-mix(in sRGB, var(--rec) 45%, transparent);
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

  @keyframes p-shell-alive {
    0%,
    100% {
      box-shadow: inset 0 0 0 0 transparent;
    }

    50% {
      box-shadow: inset 0 0 0 1.5px
        color-mix(in sRGB, var(--accent) 42%, transparent);
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
    .p-auth-host {
      transition: none !important;
      animation: none !important;
    }

    .p-agent.is-working {
      opacity: 0.8;
    }

    .p-shell.is-working {
      box-shadow: inset 0 0 0 1px
        color-mix(in sRGB, var(--accent) 35%, transparent);
    }
  }
</style>
