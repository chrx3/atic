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
  import { onMount } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import type { DictationPhase } from "$core/types";
  import { capture } from "$domain/capture.svelte";
  import { dictation as dictationStore } from "$domain/dictation.svelte";
  import { paste } from "$domain/paste.svelte";
  import { sessionEffect } from "$domain/session";
  import Waveform from "$lib/Waveform.svelte";
  import AticMark from "$lib/AticMark.svelte";
  import { liquid } from "$surfaces/overlay/group.svelte";
  import type { Rect } from "$lib/liquid/geometry";
  import { RectTracker } from "$lib/liquid/measure.svelte";
  import { pillShape } from "$lib/liquid/geometry";
  import ToolIcon from "$lib/ToolIcon.svelte";
  import { agents } from "$lib/agentSessions.svelte";
  import ParticleWheel from "$lib/ParticleWheel.svelte";
  import { TOOLS, type ToolId } from "$lib/tools";
  import { formatShortcut } from "$lib/format";
  import { PILL, growsFirst, windowFor, type Size } from "$surfaces/overlay/pillStage";
  import { createCssStage } from "$surfaces/overlay/pillCssStage";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";
  import {
    blocksBrowserChrome,
    contentFor,
    isDiscOnly,
    morphsInPlace,
    pivotFor,
    stepWheel as nextWheelTool,
    wheelKeyAction,
    type Surface,
  } from "$surfaces/overlay/pill/pillPlan";
  import { MOTION, ms, wait } from "$lib/motion";
  import { playWheelTick } from "$core/uiSound";
  // Lo que queda son los comandos DE LA PILL: su geometría, sus atajos y las
  // ventanas que abre. El estado de la app lo traen los stores.
  import { showAgentsWindow } from "$ipc/agents";
  import { showClipboardWindow } from "$ipc/clipboard";
  import { startCaptureSession } from "$ipc/captures";
  import { getConfig, showMainWindow } from "$ipc/config";
  import { showSnippetsWindow } from "$ipc/snippets";
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
  // La posición también cuenta: un vuelo o un arrastre no tocan ninguno de los
  // estados de arriba, y sin `at` la silueta quedaba dibujada en el sitio del
  // que la pill se fue.
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
    // Con la rueda abierta la silueta la pintan las gotas de ParticleWheel.
    // Seguir publicando la barra deja un disco fantasma arriba-izquierda del
    // cuadrado: el stack está en opacity 0, pero Skin lee medidas, no estilos.
    if (surface === "wheel") return [];
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
    if (r.bar) shapes.push(pillShape(at(r.bar)));
    if (r.tail) shapes.push(pillShape(at(r.tail)));
    return shapes;
  });

  /**
   * La pill no traza su contorno: lo publica.
   *
   * Quien traza es el overlay, con TODO el grupo en un solo campo. Es lo único
   * que hace que el cuello hacia la consola exista sin dibujarlo: dos campos
   * separados no se pueden fundir por definición.
   */
  $effect(() => liquid.publish("pill", skinShapes));

  /**
   * El hogar: dónde queda la pill en reposo (y se persiste).
   *
   * Ya no “vuelve” tras la rueda: colapsa in-situ y ese punto pasa a ser el
   * hogar. Solo el summon (`pill-reset`) y el arrastre la reubican a propósito.
   */
  let home = $state({ x: 0, y: 0 });
  /** El vuelo lo hace una transición CSS; esto la enciende solo cuando toca. */
  let flying = $state(false);

  /** Duración del vuelo. Token `--flight-dur` (= `--duration-fast`). */
  const FLIGHT_MS = ms(MOTION.flight);

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
      // sin esto Rust se queda armando el sitio de donde la pill se fue.
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
   * para cuando el reconciliador corre, `surface` ya vale `"none"` y el
   * colapso de la rueda es indistinguible del reposo. Sin este dato el colapso
   * usaba `center` cuando no correspondía.
   */
  let collapsingFrom: "wheel" | null = null;

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
    if (from && !growsFirst(from, next) && leavingWheel) {
      // La rueda se colapsa hacia el centro: la ventana tiene que seguir
      // grande hasta que termine. El resto de los estados no anima tamaño,
      // así que esperar ahí solo dejaba la barra estirada en pantalla.
      leavingWheel = false;
      await wait(ms(wheelQuick ? MOTION.morphQuick : MOTION.morphClose));
    }
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
    await closeWheel();
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
   * Colapsa la rueda in-situ: de (cursor, grande) a (mismo centro, barra).
   *
   * No vuela al hogar previo. El centro de la rueda se conserva (pivot
   * center) y esa posición pasa a ser el nuevo hogar persistido.
   */
  async function wheelCollapse() {
    const next = target;
    opening = true;
    try {
      await stage.resize(next, "center");
      at = stage.at();
      home = { ...at };
      void savePillHome(at.x, at.y);
      surfaces.schedule();
      trace(`wheelCollapse -> ${next.w}x${next.h} @ ${at.x},${at.y}`);
    } finally {
      opening = false;
    }
  }

  /** Cierra la rueda y deja la pill donde estaba. No activa nada. */
  async function closeWheel() {
    if (surface !== "wheel") return;
    trace("closeWheel");
    opening = true;
    wheelShown = false;
    wheelTool = null;
    collapsingFrom = "wheel";
    surface = "none";
    leavingWheel = false;
    await wheelCollapse();
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
      collapsingFrom = "wheel";
      opening = true;
      surface = "none";
      leavingWheel = false;
      await wheelCollapse();
      if (id === "captures") await startCaptureSession();
      else if (id === "agents") await showAgentsWindow();
      else if (id === "clipboard") await showClipboardWindow();
      else if (id === "snippets") await showSnippetsWindow();
    } catch (err) {
      console.warn("acción de la rueda", err);
    } finally {
      wheelQuick = false;
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
    })();

    // Lo que queda acá son los eventos DE LA PILL: los atajos que la abren y la
    // cierran, y el clic fuera. La actividad, los datos y la cola los escuchan
    // sus stores.
    unlisteners.push(
      onPillRadialPress(() => void openWheel()),
      onPillRadialRelease(() => onWheelRelease()),
      onPillReset(async () => {
        trace("pill-reset (summon)");
        await closeWheel();
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
      if (event.key === "Escape" && surface === "wheel") {
        event.preventDefault();
        event.stopPropagation();
        void closeWheel();
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
     * Es más correcto que el blur, y se llevó puesto el margen de 400 ms que
     * existía solo para tragarse el blur que causaba el propio `setFocus` de la
     * apertura.
     */
    const onOutside = () => {
      if (dragMoved) return;
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

{#snippet iconBtn(label: string, path: string, onClick: () => void, size = 15)}
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

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="p-root"
  class:is-wheel={surface === "wheel"}
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

  <!-- El stack es la REFERENCIA de medida y nada más: la silueta ya no se
       dibuja acá, se publica al grupo del overlay. Sigue estando fuera de
       `.p-liquid` porque ese recorta con `overflow: hidden` para contener el
       sobrepaso del morph, y el recorte se comería lo que se mide. -->
  <div class="p-stack" class:is-dim={surface === "wheel"} bind:this={stackEl}>
    <div class="p-liquid" bind:this={liquidEl}>
      <div class="p-skin" aria-hidden="true">
        <i class="p-skin-bar" {@attach trackBar}></i>
        {#if !discOnly}
          <i class="p-skin-tail" {@attach trackTail}></i>
        {/if}
      </div>

      <div class="p-shell">
        <div class="p-bar" class:is-disc-only={discOnly} bind:this={barEl}>
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
            {@render iconBtn(
              "Descartar",
              "M6 6l12 12M18 6L6 18",
              () => void paste.dismiss(),
              13,
            )}
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
            <!-- Aviso del agente: aparece solo si hay algo que decir. Es un chip
               junto al disco y no un reemplazo, porque el disco sigue siendo la
               puerta a la rueda. -->
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

  @keyframes p-skin-arrive {
    from {
      inset: 7px 4px 7px calc(100% - 28px);
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
    display: inline-flex;
    min-height: 1.65rem;
    flex-shrink: 0;
    align-items: center;
    gap: 0.3rem;
    border: 0;
    border-radius: 999px;
    padding: 0 0.5rem;
    background: color-mix(in sRGB, var(--accent) 16%, transparent);
    color: var(--accent);
    cursor: pointer;
    transition: transform var(--duration-quick) var(--ease-smooth-out);
  }

  .p-agent:active {
    transform: scale(0.96);
  }

  /* Espera una decisión: es lo único que de verdad bloquea al agente, así que
     es lo único que usa el color de alerta. */
  .p-agent.is-waiting {
    background: color-mix(in sRGB, var(--rec) 20%, transparent);
    color: var(--rec);
  }

  /* Trabajando sin nada nuevo que leer: presente, pero sin reclamar atención.
     El número es la señal fuerte; esto es solo "sigue vivo". */
  .p-agent.is-working {
    background: color-mix(in sRGB, var(--text) 8%, transparent);
    color: var(--muted);
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
    .p-skin-tail,
    .p-icon,
    .p-rec,
    .p-dict,
    .p-queue-btn,
    .p-agent {
      transition: none !important;
      animation: none !important;
    }

    .p-stack.is-dim {
      filter: none;
    }
  }
</style>
