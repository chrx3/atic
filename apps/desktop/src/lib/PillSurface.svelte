<script lang="ts">
  /**
   * La pill: barra flotante siempre visible con la rueda de herramientas.
   *
   * Modelo: tres ejes ORTOGONALES en vez de un enum de prioridad.
   *
   *   activity  qué está haciendo la app   (idle | recording | dictating)
   *   surface   qué hay desplegado          (none | wheel | clipboard | snippets)
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
  import { onMount } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import type { DictationPhase } from "$core/types";
  import { capture } from "$domain/capture.svelte";
  import { clipboard } from "$domain/clipboard.svelte";
  import { dictation as dictationStore } from "$domain/dictation.svelte";
  import { paste } from "$domain/paste.svelte";
  import { sessionEffect } from "$domain/session";
  import { snippets } from "$domain/snippets.svelte";
  import Waveform from "$lib/Waveform.svelte";
  import AticMark from "$lib/AticMark.svelte";
  import GooFilter from "$lib/GooFilter.svelte";
  import Skin from "$lib/liquid/Skin.svelte";
  import { RectTracker } from "$lib/liquid/measure.svelte";
  import { boxShape, pillShape } from "$lib/liquid/geometry";
  import ToolIcon from "$lib/ToolIcon.svelte";
  import ClipboardHistoryList from "$lib/ClipboardHistoryList.svelte";
  import SnippetsList from "$lib/SnippetsList.svelte";
  import { agents } from "$lib/agentSessions.svelte";
  import ParticleWheel from "$lib/ParticleWheel.svelte";
  import { TOOLS, type ToolId } from "$lib/tools";
  import { formatShortcut } from "$lib/format";
  import {
    PILL,
    growsFirst,
    windowFor,
    type Size,
    type Pivot,
  } from "$surfaces/overlay/pillStage";
  import { createCssStage } from "$surfaces/overlay/pillCssStage";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";
  import {
    blocksBrowserChrome,
    contentFor,
    isDiscOnly,
    isPanel,
    morphsInPlace,
    pivotFor,
    stepWheel as nextWheelTool,
    wheelKeyAction,
    type PanelKind,
    type Surface,
  } from "$surfaces/overlay/pill/pillPlan";
  import { MOTION, ms, wait } from "$lib/motion";
  // Lo que queda son los comandos DE LA PILL: su geometría, sus atajos y las
  // ventanas que abre. El estado de la app lo traen los stores.
  import { showAgentsWindow } from "$ipc/agents";
  import { startCaptureSession } from "$ipc/captures";
  import { getConfig, openDataDir, showMainWindow } from "$ipc/config";
  import {
    onOverlayDismiss,
    onPillClipboardClose,
    onPillClipboardToggle,
    onPillRadialPress,
    onPillRadialRelease,
    onPillReset,
    onPillSnippetsClose,
    onPillSnippetsToggle,
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
   * Nota sobre los paneles: los agentes NO son uno, y no es un olvido. Un panel
   * de la pill sirve para «elegí y listo» —mirás una lista, tocás una cosa, se
   * cierra—. Una sesión de agente es lo contrario, así que pide una ventana que
   * se queda. La pill se queda con el aviso, que sí es su trabajo.
   */

  /**
   * El estado de la app no es de la pill.
   *
   * Grabación, dictado, portapapeles, textos y cola vivían acá con sus propias
   * copias y sus propios oyentes, duplicados con la ventana principal: dos
   * cronómetros contando lo mismo. Ahora se declara una vez qué necesita esta
   * ventana y el resto se lee.
   *
   * La pill los necesita TODOS desde el arranque, aunque los paneles estén
   * cerrados: se abren por atajo global y tienen que salir con el contenido ya
   * puesto, no vacíos esperando un viaje a Rust.
   */
  $effect(() =>
    sessionEffect(["config", "capture", "dictation", "clipboard", "snippets", "paste"]),
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
  const activity = $derived(
    recording ? "recording" : dictating ? "dictating" : "idle",
  );

  // ─── Eje 2: superficie ───────────────────────────────────────────────────
  let surface = $state<Surface>("none");
  /** Visual, separado del lógico: la rueda se revela recién con la ventana ya
   *  reencuadrada, para que el morph nunca se pinte a mitad del resize. */
  let wheelShown = $state(false);
  let wheelTool = $state<ToolId | null>(null);
  /** Cierre acelerado: al elegir herramienta la rueda ya cumplió su función. */
  let wheelQuick = $state(false);
  /** El panel abrió hacia arriba (lo decide Rust: es quien ve los monitores). */
  let panelUp = $state(false);
  let surfaceOpenedAt = 0;

  const panelOpen = $derived(isPanel(surface));

  /**
   * Aviso de agente en la barra compacta.
   *
   * Es lo que hace que «corre en segundo plano» signifique algo: la consola
   * puede estar cerrada y la sesión sigue viva, así que la pill tiene que ser
   * el lugar donde te enteras de que respondió o de que te está esperando.
   */
  const agentAlert = $derived(agents.unread > 0 || agents.working);

  // ─── Eje 3: cola de pegado ───────────────────────────────────────────────
  const hasQueue = $derived(paste.count > 0);

  /**
   * Reposo: la barra es SOLO el disco.
   *
   * Estaba escrito en línea en la clase de `.p-bar`; ahora lo mira también la
   * piel, que monta la gota que llega justo cuando esto deja de valer.
   */
  const discOnly = $derived(isDiscOnly({ surface, activity, hasQueue, agentAlert }));

  // ─── Datos de los paneles ────────────────────────────────────────────────
  // Las listas y el bloc salen de los stores; lo único local es qué pestaña se
  // está mirando, que es estado de esta ventana y de ninguna otra.
  let snippetsTab = $state<"list" | "scratchpad">("list");

  let pasting = $state(false);
  let windowDragging = $state(false);
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
   * El filtro SVG sigue disponible con `Ctrl+Alt+P`: esta superficie se usa
   * todos los días y conviene poder volver en un gesto si algo se ve mal.
   *
   * El CSS no cambia. Sigue decidiendo la geometría y las animaciones —cómo
   * se derrama el panel, cómo llega la gota, cómo se invierte todo al abrir
   * hacia arriba— y lo único que cambia es quién dibuja el contorno.
   */
  let sdf = $state(true);
  const tracker = new RectTracker();

  $effect(() => {
    tracker.origin = stackEl;
  });

  /**
   * Las tres altas, creadas UNA vez.
   *
   * No pueden ser flechas escritas en el markup: ahí se recrean en cada
   * render, Svelte las trata como un adjunto distinto y desmonta y vuelve a
   * montar el anterior. Y como la baja borra el rectángulo del que depende
   * este mismo template, el resultado era un bucle: medir, redibujar, dar de
   * baja, volver a medir. La pill quedaba sin piel y la pestaña colgada.
   */
  const trackBar = (el: HTMLElement) => tracker.track("bar", el);
  const trackTail = (el: HTMLElement) => tracker.track("tail", el);
  const trackPanel = (el: HTMLElement) => tracker.track("panel", el);

  // Cada cambio de estado arranca una animación de CSS: hay que volver a mirar.
  $effect(() => {
    void surface;
    void panelOpen;
    void discOnly;
    void panelUp;
    void barW;
    tracker.wake();
  });

  onMount(() => {
    sdf = localStorage.getItem("atic-pill-goo") !== "1";
    const onToggle = (event: KeyboardEvent) => {
      if (!event.ctrlKey || !event.altKey || event.key.toLowerCase() !== "p") return;
      sdf = !sdf;
      if (sdf) localStorage.removeItem("atic-pill-goo");
      else localStorage.setItem("atic-pill-goo", "1");
      tracker.wake();
    };
    window.addEventListener("keydown", onToggle);
    return () => {
      window.removeEventListener("keydown", onToggle);
      tracker.stop();
    };
  });

  /**
   * Las siluetas medidas, como formas del campo.
   *
   * Los radios se repiten acá porque el campo los necesita como número y el
   * CSS los declara como estilo. Son los mismos tres de siempre: la barra y la
   * gota son pastillas, el panel tiene esquina de 18.
   */
  const skinShapes = $derived.by(() => {
    const r = tracker.rects;
    const shapes = [];
    if (r.bar) shapes.push(pillShape(r.bar));
    if (r.tail) shapes.push(pillShape(r.tail));
    if (r.panel) shapes.push(boxShape(r.panel, 18));
    return shapes;
  });

  /**
   * El hogar: dónde vuelve la pill cuando se cierra lo que haya abierto.
   *
   * Era una posición guardada en Rust (`stash_pill_home` / `morph_pill_home` /
   * `restore_pill_position`), porque la ventana la movía Rust y el frontend no
   * tenía forma de recordar un punto en coordenadas de pantalla. Acá es una
   * variable: la pill vive en el overlay y su posición nunca sale de CSS.
   */
  let home = $state({ x: 0, y: 0 });
  /** El vuelo lo hace una transición CSS; esto la enciende solo cuando toca. */
  let flying = $state(false);


  /** Duración del vuelo. Es la misma curva que usaba el tween de Rust. */
  const FLIGHT_MS = 190;

  /** Mueve la pill animando, y avisa cuánto va a tardar. */
  function flyTo(p: { x: number; y: number }): number {
    flying = true;
    stage.moveTo(p);
    at = stage.at();
    window.setTimeout(() => {
      flying = false;
      // Republicar al aterrizar, y no solo al despegar.
      //
      // La zona viva se mide con `getBoundingClientRect()`, que durante una
      // transición CSS devuelve la posición ANIMADA. El publish que dispara el
      // cambio de `at` sale con la pill todavía en el origen del vuelo, así que
      // sin esto Rust se queda armando el sitio de donde la pill se fue: se
      // veía como que después de cerrar un panel quedaba inmóvil.
      surfaces.schedule();
    }, FLIGHT_MS);
    return FLIGHT_MS;
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

  /** Traza al log de Rust. Fire-and-forget: no debe alterar el flujo ni fallar. */
  function trace(msg: string) {
    void pillTrace(msg).catch(() => {});
  }

  /**
   * Hay una transición dueña de la geometría corriendo (morph de la rueda).
   * Mientras esté puesta, el reconciliador no toca la ventana: un resize suyo
   * cancelaría el morph a mitad de camino y la dejaría donde llegó.
   */
  let opening = $state(false);
  /** Una apertura de rueda ya está en vuelo. Corta el auto-repeat del atajo. */
  let openingWheel = false;
  /** Solo el colapso de la rueda necesita que la ventana espere. */
  let leavingWheel = false;
  /**
   * Qué superficie se está cerrando.
   *
   * El pivote del colapso depende de **qué se cierra**, no del estado destino:
   * para cuando el reconciliador corre, `surface` ya vale `"none"` y los dos
   * colapsos —rueda y panel— son indistinguibles. Sin este dato ambos usaban
   * `center`, y ese era el "punto C": el panel se encogía hacia su propio
   * centro (~130 px arriba y ~80 a la izquierda de la barra) y recién desde
   * ese punto arrancaba el vuelo al hogar.
   */
  let collapsingFrom: "wheel" | "panel" | null = null;

  /** El estado del que dependen las decisiones de `pillPlan`. */
  function plan() {
    return { surface, collapsingFrom, panelUp };
  }

  /**
   * Único reconciliador. La regla de crecer-antes / encoger-después es lo que
   * reemplaza a `chromeHidden`, `radialClosing` y `quickClose` como banderas:
   * la ventana es siempre la unión de origen y destino mientras algo se anima.
   */
  async function reconcile(next: Size) {
    if (opening) return;
    const from = stage.applied();
    if (from && !growsFirst(from, next) && leavingWheel) {
      // La rueda se colapsa hacia el centro: la ventana tiene que seguir
      // grande hasta que termine. El resto de los estados no anima tamaño,
      // así que esperar ahí solo dejaba la barra estirada en pantalla.
      leavingWheel = false;
      await wait(ms(wheelQuick ? MOTION.morphQuick : MOTION.morphClose));
    }
    // `pivotFor` lee `panelUp` (hacia dónde había abierto) y `stage.resize` lo
    // sobrescribe con el resultado nuevo: en ese orden, no al revés.
    const outcome = await stage.resize(
      next,
      pivotFor(plan()),
      morphsInPlace({ ...plan(), from }),
    );
    if (outcome.ok) {
      panelUp = outcome.up;
      collapsingFrom = null;
      at = stage.at();
      box = next;
    }
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

  /**
   * Republicar cuando la pill se MUEVE.
   *
   * El `ResizeObserver` del registro solo ve cambios de tamaño, y mover algo
   * con `left`/`top` no es uno. Sin esto, Rust arma el overlay donde la pill
   * estaba en el primer frame —(0,0), antes de leer su hogar— y la pill queda
   * dibujada en su sitio pero sin recibir un solo evento.
   */
  $effect(() => {
    void at.x;
    void at.y;
    void box.w;
    void box.h;
    surfaces.schedule();
  });


  // Medir la barra SOLO en reposo.
  //
  // `max-content` la hace independiente del ancho de ventana, pero
  // `.p-root.is-panel .p-bar { width: 100% }` pisa esa regla: con un panel
  // abierto la barra mide lo que mide la ventana. Medir ahí cierra un lazo
  // —ventana define barra define ventana— y el colapso terminaba emitiendo
  // tres reencuadres, oscilando entre 48 y 320 px de ancho, con un final no
  // determinista: a veces la pill volvía al hogar convertida en barra ancha.
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
  // ondas recortada adentro. Por la rueda no se notaba porque ahí `wheelHome()`
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
   * Abre la rueda EN EL CURSOR. Antes crecía donde la pill estuviera, así que
   * con la tecla sostenida había que cruzar la pantalla con el mouse hasta
   * donde la habías dejado. El hogar se guarda para volver al soltar.
   */
  async function openWheel() {
    // `surface` recién vale "wheel" al final, después de tres IPC. En esa
    // ventana el auto-repeat del atajo (Windows reenvía `Pressed` mientras la
    // tecla está sostenida) podía reentrar acá: el segundo pase cancelaba el
    // tween del primero y lo reiniciaba desde donde hubiera llegado, así que
    // sostener la tecla dejaba la rueda creciendo a los tirones.
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
    await closePanels({ silent: true });
    wheelQuick = false;
    // Sin selección inicial: un toque accidental del atajo no debe disparar
    // ninguna acción al soltar.
    wheelTool = null;
    opening = true;
    try {
      // 1) Guardar el hogar ANTES de tocar la geometría. Si se guarda después,
      //    lo que queda grabado es la posición que ya movió el reencuadre.
      home = { ...at };
      // 2) Viajar al cursor Y crecer como UN solo movimiento. Antes esto pedía
      //    que Rust interpolara el rectángulo completo, porque eran dos
      //    escrituras de posición y entre medio se pintaba la rueda sobre la
      //    pill vieja — la "tercera posición". Acá el tamaño y la posición se
      //    escriben en el mismo frame, así que el problema no puede ocurrir.
      const side = PILL.wheel - PILL.pad * 2;
      await stage.resize(windowFor({ w: side, h: side }), "cursor");
      at = stage.at();
    } catch (err) {
      console.warn("pill wheel summon", err);
    } finally {
      opening = false;
    }
    surface = "wheel";
    wheelShown = true;
  }

  /**
   * Cierra la rueda: de (cursor, grande) a (hogar, chica) en UN movimiento.
   *
   * Rust interpola el rectángulo entero, así que la rueda se achica mientras
   * viaja. El camino viejo eran dos pasos con un salto visible entre medio:
   * implosionaba al centro (+115 px) y desde ahí volaba al hogar.
   */
  async function wheelHome() {
    const next = target;
    opening = true;
    try {
      // Encoger y volver, en el mismo frame. El tamaño lo escribe el escenario
      // y la posición la anima el CSS: se achica mientras viaja, que es lo que
      // el tween de Rust conseguía interpolando el rectángulo entero.
      await stage.resize(next, "topLeft");
      flyTo(home);
      trace(`wheelHome -> ${next.w}x${next.h}`);
    } finally {
      opening = false;
    }
  }

  /** Cierra la rueda y devuelve la pill a su hogar. No activa nada. */
  async function closeWheel() {
    if (surface !== "wheel") return;
    trace("closeWheel");
    // Tomar la geometría ANTES de cambiar de superficie. Si no, el
    // reconciliador ve el estado nuevo y encoge en el acto —con el salto al
    // centro— mientras todavía estamos esperando a que salga el contenido.
    opening = true;
    wheelShown = false;
    wheelTool = null;
    collapsingFrom = "wheel";
    surface = "none";
    leavingWheel = false;
    // Sin espera: el morph arranca en el MISMO frame en que el contenido
    // empieza a cambiar, igual que en la apertura.
    //
    // Esperar acá era la tercera posición del cierre. Durante esos 250 ms la
    // barra de 48 px ya estaba dibujada dentro de la ventana de 290×290
    // todavía en el cursor, y como el stack es un flex column al tope, se veía
    // pegada arriba-izquierda del área de la rueda. Recién después la ventana
    // se movía. Ahora encoge y viaja mientras el contenido se cruza.
    await wheelHome();
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
      wheelTool = nextWheelTool(wheelTool, action === "next" ? 1 : -1, TOOLS);
    }
    return true;
  }

  /**
   * La rueda ejecuta la herramienta, no navega la app.
   *
   * Grabar y dictar no dependen de la ventana: arrancan ya, en paralelo al
   * cierre del morph, así que la respuesta se percibe inmediata.
   */
  async function activateTool(id: ToolId) {
    if (surface !== "wheel") return;
    wheelQuick = true;
    wheelShown = false;
    wheelTool = null;

    if (id === "meetings") void toggleRecord();
    else if (id === "dictation") void toggleDictate();

    try {
      if (id === "clipboard" || id === "snippets") {
        // La pill ya está en el cursor: el panel se abre acá mismo, y con el
        // contenido ya puesto porque los stores están montados desde el
        // arranque.
        surface = id;
        surfaceOpenedAt = Date.now();
        return;
      }
      collapsingFrom = "wheel";
      // Igual que en `closeWheel`: la geometría se toma antes de cambiar de
      // superficie, para que el reconciliador no encoja por su cuenta.
      opening = true;
      surface = "none";
      // Mismo cierre que `closeWheel`: el morph corre junto al contenido, sin
      // espera previa. `wheelQuick` ya acelera las curvas CSS de salida.
      leavingWheel = false;
      await wheelHome();
      if (id === "captures") await startCaptureSession();
      // La consola es una ventana aparte: la rueda la abre y se va, igual que
      // con la ventana principal.
      else if (id === "agents") await showAgentsWindow();
    } catch (err) {
      console.warn("acción de la rueda", err);
    } finally {
      wheelQuick = false;
    }
  }

  // ─── Paneles ─────────────────────────────────────────────────────────────
  async function openPanel(kind: PanelKind, fly: boolean) {
    // Guardar el hogar y, si hace falta, volar al cursor. Antes eran dos
    // comandos de Rust (`prepare_clipboard_pill` / `prepare_snippets_pill`)
    // que hacían exactamente esto sobre la ventana.
    home = { ...at };
    let flight = 0;
    if (fly) {
      const cursor = await cursorPoint();
      if (cursor) {
        const size = stage.applied() ?? windowFor({ w: PILL.bar, h: PILL.bar });
        flight = flyTo({ x: cursor.x - size.w / 2, y: cursor.y - size.h / 2 });
      }
    }
    // Esperar el aterrizaje antes de expandir. Volar y crecer a la vez eran dos
    // escritores de la posición: el panel se anclaba donde estuviera la barra en
    // ese frame y el vuelo seguía empujando después, así que terminaba en un
    // punto intermedio del recorrido y no en el cursor.
    trace(`openPanel ${kind} fly=${fly} vuelo=${flight}ms`);
    if (flight > 0) await wait(flight);

    trace(`openPanel ${kind} expande`);
    surface = kind;
    surfaceOpenedAt = Date.now();
  }

  /** Cierra cualquier panel/rueda. `silent` evita el viaje de vuelta al hogar. */
  async function closePanels({ silent = false } = {}) {
    if (surface === "none") return;
    // Guardar lo pendiente del bloc, NO descartarlo. El autoguardado espera
    // tras la última tecla; cerrar el panel dentro de esa ventana —Escape, clic
    // afuera, pegar un fragmento— tiraba lo último que escribiste sin avisar.
    snippets.flushScratchpad();
    trace(`closePanels desde=${surface} silent=${silent}`);
    collapsingFrom = surface === "wheel" ? "wheel" : "panel";
    if (surface === "wheel") leavingWheel = true;
    surface = "none";
    wheelShown = false;
    pasting = false;
    // Encoger PRIMERO, volar DESPUÉS. Al revés el vuelo salía con el tamaño
    // del panel todavía puesto: el hogar se clampeaba contra el borde del
    // monitor usando 312×380 en vez de 48×48, así que la pill no volvía al
    // punto del que había salido. Además el reencuadre llegaba a mitad del
    // vuelo y lo cancelaba, dejándola donde estuviera en ese instante.
    await collapse();
    if (!silent) await goHome();
  }

  /**
   * Aplica el encogimiento pendiente y espera a que la ventana quede en su
   * tamaño final.
   *
   * El `$effect` también lo dispara, pero de forma diferida: para cuando corre,
   * el vuelo al hogar ya salió. Llamarlo en línea vuelve determinista el orden;
   * el efecto que llega después encuentra el tamaño ya aplicado y `stage.resize`
   * lo descarta sin IPC.
   */
  async function collapse() {
    // La barra venía estirada al ancho del panel, así que su última medición no
    // sirve para elegir el tamaño compacto: apuntaría a 320 px de ancho. Volver
    // a la base y dejar que el observador la ensanche después si hace falta
    // (timer de grabación, chip de la cola). Así el colapso es un solo paso.
    if (collapsingFrom === "panel") barW = PILL.bar;
    const next = target;
    trace(
      `collapse from=${collapsingFrom} panelUp=${panelUp} ` +
        `pivot=${pivotFor(plan())} -> ${next.w}x${next.h}`,
    );
    await reconcile(next);
    trace("collapse listo");
  }

  /** Devuelve la pill al hogar guardado (si el summon la había movido). */
  async function goHome() {
    trace("goHome");
    flyTo(home);
  }

  /** Título de la barra con un panel abierto. */
  function panelTitle(kind: Surface): string {
    return kind === "clipboard" ? "Clipboard" : "Textos";
  }

  /** Atajo de panel: si ya está abierto, lo reabre en el cursor. */
  async function onPanelHotkey(kind: PanelKind) {
    if (surface === kind) {
      await closePanels();
    }
    await openPanel(kind, true);
  }

  // ─── Acciones ────────────────────────────────────────────────────────────
  async function toggleRecord() {
    if (busy || dictating) return;
    // Detener siempre se puede; empezar no, si hay un panel ocupando la barra.
    if (!recording && panelOpen) return;
    try {
      await capture.toggle();
    } catch (err) {
      console.warn("grabación", err);
    }
  }

  async function toggleDictate() {
    if (busy || recording || panelOpen) return;
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
   */
  const DRAG_THRESHOLD = 4;
  let dragOrigin: { x: number; y: number; ox: number; oy: number } | null = null;
  let dragMoved = false;

  function beginDrag(event: PointerEvent) {
    const el = event.target as HTMLElement | null;
    if (!el || event.button !== 0) return;
    if (el.closest("button, a, input, textarea, [data-no-drag], .clip-item, .clip-items")) {
      return;
    }
    dragOrigin = { x: event.clientX, y: event.clientY, ox: at.x, oy: at.y };
    dragMoved = false;
    window.addEventListener("pointermove", onDragMove);
    window.addEventListener("pointerup", endDrag);
    window.addEventListener("pointercancel", endDrag);
  }

  function onDragMove(event: PointerEvent) {
    if (!dragOrigin) return;
    const dx = event.clientX - dragOrigin.x;
    const dy = event.clientY - dragOrigin.y;
    if (!dragMoved && Math.hypot(dx, dy) <= DRAG_THRESHOLD) return;
    if (!dragMoved) {
      dragMoved = true;
      // Todo el overlay pasa a recibir el mouse mientras dura el gesto: si no,
      // el primer movimiento rápido saca el puntero de la pill, Rust desarma la
      // ventana y el arrastre se corta solo.
      surfaces.dragging = true;
    }
    stage.moveTo({ x: dragOrigin.ox + dx, y: dragOrigin.oy + dy });
    at = stage.at();
  }

  function stopDragWatch() {
    dragOrigin = null;
    surfaces.dragging = false;
    window.removeEventListener("pointermove", onDragMove);
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

    // Escuchar a los agentes desde el arranque, no al abrir el panel: una
    // sesión que responde con la pill cerrada tiene que dejar el aviso puesto.
    // Si empezáramos a escuchar al abrir, el aviso no existiría nunca.
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
    })();

    // Lo que queda acá son los eventos DE LA PILL: los atajos que la abren y la
    // cierran, y el clic fuera. La actividad, los datos y la cola los escuchan
    // sus stores.
    unlisteners.push(
      onPillClipboardToggle(() => {
        trace("RX pill-clipboard-toggle");
        void onPanelHotkey("clipboard");
      }),
      onPillSnippetsToggle(() => void onPanelHotkey("snippets")),
      onPillClipboardClose(() => void closePanels()),
      onPillSnippetsClose(() => void closePanels()),
      onPillRadialPress(() => void openWheel()),
      onPillRadialRelease(() => onWheelRelease()),
      // Colapsar y RECIÉN AHÍ volar al cursor. Rust emite el reset pero no
      // mueve: solo acá se sabe cuándo la ventana terminó de encoger, y el
      // ancla del cursor se calcula con el tamaño que tenga en ese momento.
      // Colapsar y RECIÉN AHÍ volar al cursor: el ancla se calcula con el
      // tamaño que la pill tenga en ese momento, no con el del panel abierto.
      onPillReset(async () => {
        trace("pill-reset (summon)");
        await closePanels({ silent: true });
        const cursor = await cursorPoint();
        if (!cursor) return;
        const size = stage.applied() ?? windowFor({ w: PILL.bar, h: PILL.bar });
        flyTo({ x: cursor.x - size.w / 2, y: cursor.y - size.h / 2 });
        // El summon fija un hogar nuevo: la pill se queda donde la llamaste.
        home = { ...at };
        void savePillHome(at.x, at.y);
      }),
    );

    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && surface !== "none") {
        event.preventDefault();
        event.stopPropagation();
        if (surface === "wheel") void closeWheel();
        else void closePanels();
        return;
      }
      if (onWheelKey(event)) {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
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
     * Es más correcto que el blur, y se llevó puestos dos parches: el margen de
     * 400 ms (existía solo para tragarse el blur que causaba el propio
     * `setFocus` de la apertura) y la excepción del bloc de notas (el blur se
     * disparaba con una notificación, que no es el usuario yéndose).
     */
    const onOutside = () => {
      if (pasting || dragMoved) return;
      if (surface === "wheel") void closeWheel();
      else if (surface !== "none") void closePanels();
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
     abra un panel encima. Antes desaparecía y con él el botón de detener. -->
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

{#snippet iconBtn(
  label: string,
  path: string,
  onClick: () => void,
  size = 15,
)}
  <button
    type="button"
    class="p-icon"
    data-no-drag
    onclick={onClick}
    aria-label={label}
    title={label}
  >
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.5"
      stroke-linecap="butt"
      aria-hidden="true"
    >
      <path d={path} />
    </svg>
  </button>
{/snippet}

{#snippet panelBody()}
  <div class="p-panel" data-no-drag>
    {#if surface === "clipboard"}
      <ClipboardHistoryList
        items={clipboard.items}
        loading={false}
        compact
        onRefresh={() => void clipboard.hydrate()}
        onPasteStart={() => (pasting = true)}
        onPasted={() => void closePanels()}
        onError={() => (pasting = false)}
      />
    {:else}
      <!-- Las pestañas nombran el contenido, no la vista: "Lista" no decía
           lista de qué, y era lo único que distinguía los textos reusables del
           bloc de notas. -->
      <div class="p-tabs" role="tablist" aria-label="Textos y notas">
        <button
          type="button"
          role="tab"
          class="p-tab"
          class:active={snippetsTab === "list"}
          aria-selected={snippetsTab === "list"}
          onclick={() => (snippetsTab = "list")}
        >
          Textos
        </button>
        <button
          type="button"
          role="tab"
          class="p-tab"
          class:active={snippetsTab === "scratchpad"}
          aria-selected={snippetsTab === "scratchpad"}
          onclick={() => (snippetsTab = "scratchpad")}
        >
          Notas
        </button>
      </div>
      {#if snippetsTab === "list"}
        <SnippetsList
          items={snippets.items}
          loading={false}
          compact
          onRefresh={() => void snippets.hydrate()}
          onPasteStart={() => (pasting = true)}
          onPasted={() => void closePanels()}
          onError={() => (pasting = false)}
        />
      {:else}
        <textarea
          class="p-scratch"
          value={snippets.scratchpad?.body ?? ""}
          oninput={(event) => snippets.editScratchpad(event.currentTarget.value)}
          placeholder="Notas temporales…"
          aria-label="Bloc de notas"
          data-no-drag
        ></textarea>
      {/if}
    {/if}
  </div>
{/snippet}

<GooFilter id="pill-goo" />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="p-root"
  class:is-wheel={surface === "wheel"}
  class:is-panel={panelOpen}
  class:is-up={panelUp && panelOpen}
  class:is-quick={wheelQuick}
  class:is-flying={flying}
  style="left: {at.x}px; top: {at.y}px; width: {box.w}px; height: {box.h}px"
  bind:this={rootEl}
  onpointerdown={beginDrag}
>
  <!-- La rueda vive siempre montada y se cruza en fundido con el resto del
       chrome; montarla y desmontarla hacía que la pill reapareciera opaca
       encima mientras la rueda aún salía. -->
  <div class="p-wheel" class:is-open={wheelShown} data-no-drag>
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

  <!-- La piel medida cuelga del stack y no del cuerpo líquido: `.p-liquid`
       recorta con `overflow: hidden` para contener el sobrepaso del morph, y
       ese recorte se comería el bulto de la silueta fundida. -->
  <div class="p-stack" class:is-dim={surface === "wheel"} bind:this={stackEl}>
    {#if sdf}
      <Skin shapes={skinShapes} />
    {/if}

    <!-- Cuerpo líquido: con panel, barra + cuerpo son DOS siluetas que se
         funden (join tipo Liquid UI). Sin panel, es solo la barra. -->
    <div class="p-liquid" class:is-fused={panelOpen} bind:this={liquidEl}>
      <!-- La piel va aparte del contenido: acá no puede haber ni texto ni
           iconos.

           Con el campo de distancia estos `<i>` dejan de pintarse y quedan
           como REFERENCIAS DE MEDIDA: el CSS sigue decidiendo la geometría y
           las animaciones —que es donde está todo el conocimiento de cómo se
           derrama el panel y cómo llega la gota— y `Skin` dibuja el contorno
           a partir de lo medido. -->
      <div class="p-skin" class:is-sdf={sdf} aria-hidden="true">
        <i class="p-skin-bar" {@attach trackBar}></i>
        {#if !discOnly && !panelOpen}
          <i class="p-skin-tail" {@attach trackTail}></i>
        {/if}
        {#if panelOpen}
          <i class="p-skin-panel" {@attach trackPanel}></i>
        {/if}
      </div>

    <div class="p-shell">
      <!-- La barra se mide sola (`max-content`): no hay tabla de anchos. -->
      <div class="p-bar" class:is-disc-only={discOnly} bind:this={barEl}>
        {#if panelOpen}
          <span class="p-mark"><AticMark size={15} strokeWidth={1.5} /></span>
          <span class="p-label">{panelTitle(surface)}</span>
          {#if recording}
            {@render recDot(`Grabando ${fmt(elapsed)} · clic para detener`)}
          {/if}
          {@render iconBtn("Abrir carpeta", "M4 19V6h6l2 2.5h8V19H4Z", () =>
            void openDataDir(surface === "clipboard" ? "clipboard" : "snippets").catch(
              console.warn,
            ),
          16)}
          {@render iconBtn("Cerrar (Esc)", "M6 6l12 12M18 6L6 18", () =>
            void closePanels(),
          )}
        {:else if activity === "recording"}
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
            <Waveform mic={levels.mic} system={levels.system} bars={10} variant="quiet" />
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
          <span class="p-mark is-disc"><AticMark size={20} strokeWidth={1.4} /></span>
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
          {@render iconBtn("Descartar", "M6 6l12 12M18 6L6 18", () =>
            void paste.dismiss(),
          13)}
        {:else}
          <!-- Reposo: disco con la marca. Un clic abre la rueda; el centro de
               la rueda abre la app. El doble clic ya no hace falta. -->
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
          <!-- Aviso del agente: aparece solo si hay algo que decir, y se va al
               abrir el panel. Es un chip junto al disco y no un reemplazo,
               porque el disco sigue siendo la puerta a la rueda. -->
          {#if agentAlert}
            <button
              type="button"
              class="p-agent"
              class:is-waiting={agents.waiting > 0}
              class:is-working={agents.waiting === 0 &&
                agents.working &&
                agents.unread === 0}
              data-no-drag
              onclick={() => void showAgentsWindow()}
              title={agents.waiting > 0
                ? "El agente espera tu permiso"
                : agents.unread > 0
                  ? `${agents.unread} respuesta(s) sin leer`
                  : "El agente está trabajando"}
              aria-label="Abrir la consola de agentes"
            >
              <ToolIcon id="agents" size={13} strokeWidth={1.6} />
              {#if agents.waiting > 0}
                <span class="p-agent-count">permiso</span>
              {:else if agents.unread > 0}
                <span class="p-agent-count">{agents.unread}</span>
              {/if}
            </button>
          {/if}
        {/if}
      </div>
    </div>

    {#if panelOpen}
      {@render panelBody()}
    {/if}
    </div>
  </div>
</div>

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
    display: flex;
    box-sizing: border-box;
    flex-direction: column;
    padding: 4px;
    overflow: hidden;
    cursor: grab;
  }
  /* El vuelo al hogar y al cursor. Solo mientras dura: si la transición
     quedara siempre puesta, cada reencuadre de la barra compacta —el timer
     tictaqueando, el badge de la cola— se arrastraría 190 ms. */
  .p-root.is-flying {
    transition:
      left 190ms cubic-bezier(0.22, 1, 0.36, 1),
      top 190ms cubic-bezier(0.22, 1, 0.36, 1);
  }
  .p-root:active {
    cursor: grabbing;
  }
  .p-root.is-wheel {
    padding: 0;
    cursor: default;
  }
  /* El panel hacia arriba invierte barra/cuerpo dentro del blob líquido. */
  .p-root.is-up .p-liquid.is-fused {
    flex-direction: column-reverse;
  }
  /* Abriendo hacia arriba la piel se da vuelta sola (`flex-direction: inherit`),
     así que acá ya no hace falta invertir nada. */

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
    transition:
      opacity var(--morph-fade-dur) var(--morph-close-ease),
      filter var(--morph-fade-dur) var(--morph-close-ease);
  }
  .p-stack.is-dim {
    opacity: 0;
    filter: blur(var(--morph-blur));
    pointer-events: none;
    /* Cada estado declara la curva de su dirección; si no, el chrome se iría
       con la del cierre y rompería el espejo. */
    transition:
      opacity var(--morph-fade-dur) var(--morph-ease),
      filter var(--morph-fade-dur) var(--morph-ease);
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

  /* ─── Cuerpo líquido (barra sola o barra+panel fusionados) ───────────── */
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
  .p-liquid.is-fused {
    width: 100%;
    flex: 1;
    overflow: hidden;
    border-radius: 18px;
    /* Ya no pinta la superficie —eso lo hace `.p-skin`—, solo proyecta la
       sombra. El brillo de 1px que había acá se fue con el fondo: era una
       línea recta arriba, y lo que ahora dibuja la unión es el filete cóncavo
       del hombro, que es justamente el gesto que la línea tapaba. */
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.14);
    animation: p-liquid-in var(--panel-dur) var(--morph-ease);
  }

  /*
   * La piel: las siluetas, fundidas entre sí, sin nada de contenido adentro.
   *
   * `flex-direction: inherit` no es un atajo: `.p-root.is-up` invierte
   * `.p-liquid` a `column-reverse` cuando el panel abre hacia arriba, y así la
   * piel se da vuelta con él sin repetir la regla.
   */
  .p-skin {
    position: absolute;
    z-index: 0;
    inset: var(--goo-grow);
    display: flex;
    flex-direction: inherit;
    pointer-events: none;
    filter: url(#pill-goo);
  }
  .p-skin > i {
    display: block;
    background: var(--skin);
  }

  /* Con el campo de distancia los `<i>` no se pintan ni se filtran: solo se
     miden. La silueta la dibuja `Skin` a partir de sus rectángulos. */
  .p-skin.is-sdf {
    filter: none;

    /*
     * Y se anula el descuento del engorde.
     *
     * Todas las medidas de la piel están escritas como `40px - goo-grow * 2`
     * porque el endurecido del filtro devolvía 2.8 px por lado. El contorno
     * trazado NO engorda: pasa por la geometría pedida. Dejando el descuento,
     * el disco de 40 se dibujaba de 37.2.
     *
     * Se apaga con la variable y no regla por regla: así vale para las tres
     * siluetas y para el `inset` de una sola vez.
     */
    --goo-grow: 0px;

    inset: 0;
  }
  .p-skin.is-sdf > i {
    background: transparent;
  }

  /* Y la sombra la proyecta el path ya trazado, no la caja: dejar las dos
     dibujaba un rectángulo por detrás de una silueta redondeada. */
  .p-liquid.is-fused:has(.p-skin.is-sdf) {
    box-shadow: none;
  }
  /* La pastilla de siempre. El alto descuenta lo que el filtro va a devolver
     por los dos lados, así que el borde final cae exactamente en los 40. */
  .p-skin-bar {
    height: calc(40px - var(--goo-grow) * 2);
    flex-shrink: 0;
    border-radius: 999px;
  }
  /* Sin panel, el blob base es SOLO el disco: el resto del ancho lo trae la
     gota, que es la que hace visible que la barra creció absorbiendo algo. */
  .p-liquid:not(.is-fused) .p-skin-bar {
    width: calc(40px - var(--goo-grow) * 2);
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
  @keyframes p-skin-arrive {
    from {
      top: 7px;
      right: 4px;
      bottom: 7px;
      left: calc(100% - 28px);
    }
  }
  .p-skin-panel {
    min-height: 0;
    flex: 1;
    border-radius: 18px;
    /*
     * El panel se DERRAMA de la barra en vez de aparecer entero.
     *
     * El origen es el borde que toca la barra, así que la silueta crece desde
     * ahí; y como el filtro las mantiene fundidas durante todo el camino, lo
     * que se ve es líquido saliendo y no una caja apareciendo. El sobrepaso de
     * `--morph-ease` lo recorta el `overflow: hidden` de `.p-liquid`.
     */
    transform-origin: 50% 0;
    animation: p-skin-pour var(--panel-dur) var(--morph-ease);
  }
  .p-root.is-up .p-liquid.is-fused .p-skin-panel {
    transform-origin: 50% 100%;
  }
  @keyframes p-skin-pour {
    from {
      transform: scale(0.34, 0.02);
    }
  }
  @keyframes p-liquid-in {
    from {
      opacity: 0.88;
      filter: blur(2px);
      transform: scale(0.975);
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
    color: var(--rb-text);
    transition:
      border-radius var(--morph-close-dur) var(--morph-close-ease),
      transform var(--morph-close-dur) var(--morph-close-ease);
  }
  /* Fusionado: la barra es solo cabecera.
     Sin línea divisoria: abarcaba todo el ancho, y la silueta en el hombro se
     mete para adentro, así que los dos extremos quedaban colgando sobre la
     ventana transparente. La división ahora la hace el filete del hombro. */
  .p-liquid.is-fused .p-shell {
    width: 100%;
    border-radius: 0;
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
  .p-root.is-panel .p-bar {
    width: 100%;
    padding-right: 8px;
  }

  .p-mark {
    display: inline-flex;
    flex-shrink: 0;
    color: var(--rb-text);
    line-height: 0;
  }

  .p-label {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    color: var(--rb-text);
    font-family: var(--rb-font);
    font-size: 0.625rem;
    font-weight: 500;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .p-timer {
    min-width: 2.4rem;
    color: var(--rb-text);
    font-family: var(--rb-font);
    font-size: 0.6875rem;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.06em;
  }

  .p-chip {
    overflow: hidden;
    max-width: 3.5rem;
    color: var(--rb-muted);
    font-family: var(--rb-font);
    font-size: 0.5625rem;
    font-weight: 500;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .p-chip.is-error {
    color: var(--rb-record);
  }
  .p-chip.is-warn {
    color: var(--rb-warn);
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
    color: var(--rb-text);
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
    width: 1.5rem;
    height: 1.5rem;
    border-radius: 999px;
    background: transparent;
    color: var(--rb-muted);
  }
  .p-icon:hover {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 8%, transparent);
  }
  .p-rec,
  .p-dict {
    width: 26px;
    height: 26px;
    border-radius: 999px;
  }
  .p-rec {
    background: color-mix(in srgb, var(--rb-record) 24%, transparent);
    color: var(--rb-record);
  }
  .p-rec-square {
    width: 8px;
    height: 8px;
    background: currentColor;
  }
  .p-dict {
    background: transparent;
    color: var(--rb-muted);
  }
  .p-dict.is-busy {
    color: var(--rb-warn);
  }
  .p-dict.is-ok {
    color: var(--rb-ok);
  }
  .p-dict.is-error {
    color: var(--rb-record);
  }
  .p-icon:active:not(:disabled),
  .p-rec:active:not(:disabled),
  .p-dict:active:not(:disabled) {
    transform: scale(0.94);
  }
  .p-icon:disabled,
  .p-rec:disabled,
  .p-dict:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .p-icon:focus-visible,
  .p-rec:focus-visible,
  .p-dict:focus-visible,
  .p-queue-btn:focus-visible,
  .p-tab:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  /* ─── Cola de pegado ────────────────────────────────────────────────── */
  .p-queue-count {
    flex-shrink: 0;
    color: var(--rb-faint);
    font-size: 0.625rem;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.06em;
  }
  .p-queue-text {
    max-width: 10rem;
    overflow: hidden;
    color: var(--rb-muted);
    font-size: 0.6875rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .p-queue-btn {
    display: inline-flex;
    height: 1.65rem;
    flex-shrink: 0;
    align-items: center;
    border: 0;
    border-radius: 999px;
    padding: 0 0.6rem;
    background: color-mix(in srgb, var(--rb-text) 8%, transparent);
    color: var(--rb-text);
    font-size: 0.5625rem;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    cursor: pointer;
  }
  .p-queue-btn:disabled {
    opacity: 0.45;
    cursor: default;
  }

  /* ─── Aviso de agente ───────────────────────────────────────────────── */
  .p-agent {
    display: inline-flex;
    height: 1.65rem;
    flex-shrink: 0;
    align-items: center;
    gap: 0.3rem;
    border: 0;
    border-radius: 999px;
    padding: 0 0.5rem;
    background: color-mix(in srgb, var(--rb-accent) 16%, transparent);
    color: var(--rb-accent);
    cursor: pointer;
  }
  /* Espera una decisión: es lo único que de verdad bloquea al agente, así que
     es lo único que usa el color de alerta. */
  .p-agent.is-waiting {
    background: color-mix(in srgb, var(--rb-record) 20%, transparent);
    color: var(--rb-record);
  }
  /* Trabajando sin nada nuevo que leer: presente, pero sin reclamar atención.
     El número es la señal fuerte; esto es solo "sigue vivo". */
  .p-agent.is-working {
    background: color-mix(in srgb, var(--rb-text) 8%, transparent);
    color: var(--rb-muted);
    animation: p-agent-pulse 1.8s ease-in-out infinite;
  }
  .p-agent-count {
    font-size: 0.6875rem;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
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

  @media (prefers-reduced-motion: reduce) {
    .p-agent.is-working {
      animation: none;
      opacity: 0.8;
    }
  }

  /* ─── Panel ─────────────────────────────────────────────────────────── */
  .p-panel {
    /* Encima de la piel, igual que `.p-shell`. */
    position: relative;
    z-index: 1;
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    border-radius: 0;
    padding: 0.45rem 0.5rem 0.55rem;
    background: transparent;
    color: var(--rb-text);
    overflow: hidden;
    cursor: default;
  }

  .p-tabs {
    display: flex;
    flex-shrink: 0;
    gap: 0.3rem;
    margin-bottom: 0.35rem;
  }
  .p-tab {
    border: 0;
    border-radius: 999px;
    padding: 0.2rem 0.55rem;
    background: transparent;
    color: var(--rb-muted);
    font-size: 0.6875rem;
    font-weight: 600;
    cursor: pointer;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out);
  }
  .p-tab.active {
    background: color-mix(in srgb, var(--rb-accent) 12%, transparent);
    color: var(--rb-accent);
  }

  .p-scratch {
    width: 100%;
    min-height: 0;
    flex: 1;
    border: 0;
    border-radius: 0.45rem;
    padding: 0.4rem 0.5rem;
    background: color-mix(in srgb, var(--rb-bg0) 80%, transparent);
    color: var(--rb-text);
    font-family: inherit;
    font-size: 0.75rem;
    resize: none;
    outline: none;
  }

  .p-root,
  .p-root * {
    user-select: none !important;
    -webkit-user-select: none !important;
  }
  .p-scratch {
    user-select: text !important;
    -webkit-user-select: text !important;
  }

  @media (prefers-reduced-motion: reduce) {
    .p-wheel,
    .p-wheel.is-open,
    .p-stack,
    .p-shell,
    .p-liquid.is-fused,
    .p-liquid.is-fused .p-skin-panel,
    .p-skin-tail,
    .p-icon,
    .p-rec,
    .p-dict,
    .p-tab {
      transition: none !important;
      animation: none !important;
    }
    .p-stack.is-dim {
      filter: none;
    }
  }
</style>
