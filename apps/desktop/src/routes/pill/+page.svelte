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
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type {
    ClipboardItem,
    DictationPhase,
    Levels,
    PasteQueueItem,
    Snippet as TextSnippet,
  } from "$lib/types";
  import Waveform from "$lib/Waveform.svelte";
  import AticMark from "$lib/AticMark.svelte";
  import ToolIcon from "$lib/ToolIcon.svelte";
  import ClipboardHistoryList from "$lib/ClipboardHistoryList.svelte";
  import SnippetsList from "$lib/SnippetsList.svelte";
  import ParticleWheel from "$lib/ParticleWheel.svelte";
  import { TOOLS, type ToolId } from "$lib/tools";
  import { formatShortcut } from "$lib/format";
  import { PILL, createStage, growsFirst, windowFor, type Size } from "$lib/pillStage";
  import { MOTION, ms, wait } from "$lib/motion";
  import {
    listPasteQueue,
    pasteQueueItemNow,
    dismissPasteQueueItem,
    onPasteQueueChanged,
    onPasteQueued,
    startRecording,
    stopRecording,
    isRecording,
    toggleDictation,
    dictationPhase,
    showMainWindow,
    startCaptureSession,
    getConfig,
    listClipboardHistory,
    listSnippets,
    getScratchpad,
    setScratchpad,
    onLevels,
    onStatus,
    onCaptureWarn,
    onDictationStatus,
    onLiveTranscriptError,
    onLiveTranscriptFinal,
    prepareClipboardPill,
    prepareSnippetsPill,
    snapPillToCursor,
    restorePillPosition,
    onPillClipboardToggle,
    onPillClipboardClose,
    onPillSnippetsToggle,
    onPillSnippetsClose,
    onPillReset,
    onPillRadialPress,
    onPillRadialRelease,
    onClipboardHistoryChanged,
    onSnippetsChanged,
    openDataDir,
  } from "$lib/api";

  type Surface = "none" | "wheel" | "clipboard" | "snippets";

  // ─── Eje 1: actividad ────────────────────────────────────────────────────
  let recording = $state(false);
  let elapsed = $state(0);
  let levels = $state<Levels>({ mic: 0, system: 0 });
  let dictation = $state<DictationPhase>("idle");
  let dictationMessage = $state<string | null>(null);
  let liveActive = $state(false);
  let liveError = $state<string | null>(null);
  let btWarning = $state<string | null>(null);
  let busy = $state(false);

  const dictating = $derived(dictation !== "idle");
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

  const panelOpen = $derived(surface === "clipboard" || surface === "snippets");

  // ─── Eje 3: cola de pegado ───────────────────────────────────────────────
  let queue = $state<PasteQueueItem[]>([]);
  let queueBusy = $state(false);
  const hasQueue = $derived(queue.length > 0);

  // ─── Datos de los paneles ────────────────────────────────────────────────
  let clipboardItems = $state<ClipboardItem[]>([]);
  let clipboardLoading = $state(false);
  let snippetItems = $state<TextSnippet[]>([]);
  let snippetsLoading = $state(false);
  let snippetsTab = $state<"list" | "scratchpad">("list");
  let scratchBody = $state("");
  let scratchLoading = $state(false);
  let scratchSaving = $state(false);
  let scratchTimer: ReturnType<typeof setTimeout> | null = null;

  let pasting = $state(false);
  let windowDragging = $state(false);
  let wheelShortcut = $state("");

  // ─── Geometría ───────────────────────────────────────────────────────────
  const stage = createStage("pill");
  /** Ancho real de la barra, medido del DOM. Sin esto habría que mantener una
   *  tabla de anchos mágicos por estado — la fuente original del desajuste. */
  let barW = $state<number>(PILL.bar);
  let barEl = $state<HTMLElement | null>(null);


  const content = $derived.by((): Size => {
    if (surface === "wheel") {
      const side = PILL.wheel - PILL.pad * 2;
      return { w: side, h: side };
    }
    if (panelOpen) return { w: PILL.panelW, h: PILL.bar + PILL.panelH };
    return { w: Math.max(barW, PILL.bar), h: PILL.bar };
  });

  const target = $derived(windowFor(content));

  const pivot = $derived(
    surface === "wheel" ? "center" : panelOpen ? "panel" : "center",
  );

  /** `openWheel` reencuadra por su cuenta; el reconciliador no debe pisarlo. */
  let opening = $state(false);
  /** Solo el colapso de la rueda necesita que la ventana espere. */
  let leavingWheel = false;

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
    const outcome = await stage.resize(next, pivot);
    if (outcome.ok) panelUp = outcome.up;
  }

  $effect(() => {
    const next = target;
    void reconcile(next);
  });

  // Medir la barra: `max-content` la hace independiente del ancho de ventana,
  // así que no puede realimentarse con el resize.
  $effect(() => {
    const el = barEl;
    if (!el) return;
    const observer = new ResizeObserver(() => {
      barW = el.offsetWidth;
    });
    observer.observe(el);
    barW = el.offsetWidth;
    return () => observer.disconnect();
  });

  // ─── Temporizador de grabación ───────────────────────────────────────────
  let timer: ReturnType<typeof setInterval> | null = null;
  let startedAt = 0;

  function startTimer() {
    if (timer) clearInterval(timer);
    startedAt = Date.now();
    elapsed = 0;
    timer = setInterval(
      () => (elapsed = Math.floor((Date.now() - startedAt) / 1000)),
      500,
    );
  }
  function stopTimer() {
    if (timer) clearInterval(timer);
    timer = null;
  }

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
    if (surface === "wheel") return;
    await closePanels({ silent: true });
    wheelQuick = false;
    // Sin selección inicial: un toque accidental del atajo no debe disparar
    // ninguna acción al soltar.
    wheelTool = null;
    opening = true;
    try {
      // ORDEN, no paralelo. Redimensionar y centrar a la vez hacía que el
      // centrado usara el tamaño viejo (48) sobre una ventana ya de 232: la
      // rueda aparecía ~92 px abajo-derecha del puntero.
      const side = PILL.wheel - PILL.pad * 2;
      await stage.resize(windowFor({ w: side, h: side }), "center");
      await snapPillToCursor();
      await getCurrentWindow().setFocus();
    } catch (err) {
      console.warn("pill wheel summon", err);
    } finally {
      opening = false;
    }
    surface = "wheel";
    wheelShown = true;
  }

  /** Cierra la rueda y devuelve la pill a su hogar. No activa nada. */
  async function closeWheel() {
    if (surface !== "wheel") return;
    leavingWheel = true;
    wheelShown = false;
    wheelTool = null;
    surface = "none";
    await goHome();
  }

  /** Soltar la tecla: activa lo apuntado si la rueda llegó a mostrarse. */
  function onWheelRelease() {
    if (surface !== "wheel") return;
    if (wheelShown && wheelTool) void activateTool(wheelTool);
    else void closeWheel();
  }

  /** Mueve la selección un paso (teclado y rueda del ratón). */
  function stepWheel(direction: 1 | -1) {
    const index = TOOLS.findIndex((tool) => tool.id === wheelTool);
    const next =
      index < 0
        ? direction === 1
          ? 0
          : TOOLS.length - 1
        : (index + direction + TOOLS.length) % TOOLS.length;
    wheelTool = TOOLS[next].id;
  }

  /** Teclado con la rueda abierta. No preselecciona: enfocar un nodo al abrir
   *  dejaría una herramienta armada y soltar la tecla la dispararía. */
  function onWheelKey(event: KeyboardEvent): boolean {
    if (surface !== "wheel" || !wheelShown) return false;
    const key = event.key;
    if (key === "ArrowRight" || key === "ArrowDown") stepWheel(1);
    else if (key === "ArrowLeft" || key === "ArrowUp") stepWheel(-1);
    else if (key === "Tab") stepWheel(event.shiftKey ? -1 : 1);
    else if (key === "Enter" || key === " ") {
      if (wheelTool) void activateTool(wheelTool);
      else void closeWheel();
    } else return false;
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
    const wasShown = wheelShown;
    wheelQuick = true;
    leavingWheel = wasShown;
    wheelShown = false;
    wheelTool = null;

    if (id === "meetings") void toggleRecord();
    else if (id === "dictation") void toggleDictate();

    try {
      if (id === "clipboard" || id === "snippets") {
        // La pill ya está en el cursor: el panel se abre acá mismo.
        surface = id;
        surfaceOpenedAt = Date.now();
        await loadSurface(id);
        return;
      }
      surface = "none";
      // Dejar correr el cierre acelerado antes de mover la ventana.
      if (wasShown) await wait(ms(MOTION.morphQuick));
      await goHome();
      if (id === "captures") await startCaptureSession();
    } catch (err) {
      console.warn("acción de la rueda", err);
    } finally {
      wheelQuick = false;
    }
  }

  // ─── Paneles ─────────────────────────────────────────────────────────────
  async function loadSurface(kind: "clipboard" | "snippets") {
    if (kind === "clipboard") {
      await refreshClipboard();
      return;
    }
    await Promise.all([refreshSnippets(), loadScratchpad()]);
  }

  async function openPanel(kind: "clipboard" | "snippets", fly: boolean) {
    try {
      if (kind === "clipboard") await prepareClipboardPill(fly);
      else await prepareSnippetsPill(fly);
    } catch (err) {
      console.warn("prepare pill", err);
    }
    surface = kind;
    surfaceOpenedAt = Date.now();
    try {
      await getCurrentWindow().setFocus();
    } catch {
      // best-effort
    }
    await loadSurface(kind);
  }

  /** Cierra cualquier panel/rueda. `silent` evita el viaje de vuelta al hogar. */
  async function closePanels({ silent = false } = {}) {
    if (surface === "none") return;
    if (scratchTimer) {
      clearTimeout(scratchTimer);
      scratchTimer = null;
    }
    if (surface === "wheel") leavingWheel = true;
    surface = "none";
    wheelShown = false;
    pasting = false;
    if (!silent) await goHome();
  }

  /** Devuelve la pill al hogar guardado (si el summon la había movido). */
  async function goHome() {
    try {
      await restorePillPosition();
    } catch (err) {
      console.warn("restore pill position", err);
    }
  }

  /** Atajo de panel: si ya está abierto, lo reabre en el cursor. */
  async function onPanelHotkey(kind: "clipboard" | "snippets") {
    if (surface === kind) {
      await closePanels();
    }
    await openPanel(kind, true);
  }

  // ─── Datos ───────────────────────────────────────────────────────────────
  async function refreshClipboard() {
    clipboardLoading = true;
    try {
      clipboardItems = await listClipboardHistory();
    } catch {
      clipboardItems = [];
    } finally {
      clipboardLoading = false;
    }
  }

  async function refreshSnippets() {
    snippetsLoading = true;
    try {
      snippetItems = await listSnippets();
    } catch {
      snippetItems = [];
    } finally {
      snippetsLoading = false;
    }
  }

  async function loadScratchpad() {
    scratchLoading = true;
    try {
      scratchBody = (await getScratchpad()).body;
    } catch {
      scratchBody = "";
    } finally {
      scratchLoading = false;
    }
  }

  function scheduleScratchSave() {
    if (scratchTimer) clearTimeout(scratchTimer);
    scratchTimer = setTimeout(() => void persistScratchpad(), 500);
  }

  async function persistScratchpad() {
    if (scratchSaving) return;
    scratchSaving = true;
    try {
      await setScratchpad(scratchBody);
    } catch (err) {
      console.warn("scratchpad save", err);
    } finally {
      scratchSaving = false;
    }
  }

  async function refreshQueue() {
    try {
      queue = await listPasteQueue();
    } catch {
      queue = [];
    }
  }

  async function queueAction(run: (id: string) => Promise<void>) {
    const front = queue[0];
    if (!front || queueBusy) return;
    queueBusy = true;
    try {
      await run(front.id);
      await refreshQueue();
    } catch (err) {
      console.warn("cola de pegado", err);
    } finally {
      queueBusy = false;
    }
  }

  // ─── Acciones ────────────────────────────────────────────────────────────
  async function toggleRecord() {
    if (busy || dictating) return;
    // Detener siempre se puede; empezar no, si hay un panel ocupando la barra.
    if (!recording && panelOpen) return;
    busy = true;
    try {
      if (recording) await stopRecording();
      else await startRecording();
    } catch (e) {
      liveError = String(e);
    } finally {
      busy = false;
    }
  }

  async function toggleDictate() {
    if (busy || recording || panelOpen) return;
    busy = true;
    try {
      await toggleDictation();
    } catch (e) {
      dictationMessage = String(e);
      dictation = "error";
    } finally {
      busy = false;
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
   * El arrastre solo empieza tras superar un umbral. Llamar a `startDragging()`
   * en el propio pointerdown metía la ventana en el loop modal de Windows, que
   * se comía el clic: ni el clic simple ni el doble llegaban nunca.
   */
  const DRAG_THRESHOLD = 4;
  let dragOrigin: { x: number; y: number } | null = null;
  let dragMoved = false;

  function beginDrag(event: PointerEvent) {
    const el = event.target as HTMLElement | null;
    if (!el || event.button !== 0) return;
    if (el.closest("button, a, input, textarea, [data-no-drag], .clip-item, .clip-items")) {
      return;
    }
    dragOrigin = { x: event.clientX, y: event.clientY };
    dragMoved = false;
    window.addEventListener("pointermove", onDragMove);
    window.addEventListener("pointerup", endDrag);
    window.addEventListener("pointercancel", endDrag);
  }

  function onDragMove(event: PointerEvent) {
    if (!dragOrigin) return;
    const moved = Math.hypot(
      event.clientX - dragOrigin.x,
      event.clientY - dragOrigin.y,
    );
    if (moved <= DRAG_THRESHOLD) return;
    dragMoved = true;
    stopDragWatch();
    windowDragging = true;
    void getCurrentWindow().startDragging();
    window.setTimeout(() => (windowDragging = false), 1500);
  }

  function stopDragWatch() {
    dragOrigin = null;
    window.removeEventListener("pointermove", onDragMove);
    window.removeEventListener("pointerup", endDrag);
    window.removeEventListener("pointercancel", endDrag);
  }

  /** Soltar sin haber movido = clic. En reposo, abre la rueda. */
  function endDrag() {
    const wasClick = dragOrigin !== null && !dragMoved;
    stopDragWatch();
    if (wasClick && activity === "idle" && surface === "none" && !hasQueue) {
      void openWheel();
    }
  }

  // ─── Ciclo de vida ───────────────────────────────────────────────────────
  onMount(() => {
    const unlisteners: Promise<UnlistenFn>[] = [];

    (async () => {
      recording = await isRecording();
      if (recording) startTimer();
      try {
        dictation = await dictationPhase();
      } catch {
        dictation = "idle";
      }
      await refreshQueue();
      try {
        wheelShortcut = (await getConfig()).pill_radial_shortcut;
      } catch {
        // Sin config, el tooltip solo omite el atajo.
      }
    })();

    unlisteners.push(
      onStatus((s) => {
        recording = s.active;
        if (s.active) {
          startTimer();
          liveActive = false;
          liveError = null;
        } else {
          stopTimer();
          liveActive = false;
          btWarning = null;
        }
      }),
      onLevels((l) => (levels = l)),
      onCaptureWarn((message) => (btWarning = message)),
      onLiveTranscriptFinal(() => {
        liveActive = true;
        liveError = null;
      }),
      onLiveTranscriptError((message) => (liveError = message)),
      onDictationStatus((s) => {
        dictation = s.phase;
        dictationMessage = s.message;
      }),
      onPillClipboardToggle(() => void onPanelHotkey("clipboard")),
      onPillSnippetsToggle(() => void onPanelHotkey("snippets")),
      onPillClipboardClose(() => void closePanels()),
      onPillSnippetsClose(() => void closePanels()),
      onPillRadialPress(() => void openWheel()),
      onPillRadialRelease(() => onWheelRelease()),
      onPillReset(() => void closePanels({ silent: true })),
      onClipboardHistoryChanged(() => {
        if (surface === "clipboard") void refreshClipboard();
      }),
      onSnippetsChanged(() => {
        if (surface === "snippets") void refreshSnippets();
      }),
      onPasteQueueChanged(() => void refreshQueue()),
      onPasteQueued(() => void refreshQueue()),
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
      // Bloquea chrome del WebView (Imprimir, Buscar, DevTools, zoom…).
      const mod = event.ctrlKey || event.metaKey;
      const key = event.key.toLowerCase();
      if (mod && ["p", "f", "g", "u", "j", "i", "r", "=", "+", "-", "0"].includes(key)) {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
      if (event.key === "F3" || event.key === "F5" || event.key === "F12") {
        event.preventDefault();
        event.stopPropagation();
      }
    };
    const onBlur = () => {
      if (pasting || windowDragging) return;
      // El margen evita que el propio setFocus de la apertura la cierre.
      if (surface !== "none" && Date.now() - surfaceOpenedAt > 400) {
        if (surface === "wheel") void closeWheel();
        else void closePanels();
      }
    };
    const onFocus = () => (windowDragging = false);

    window.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", onBlur);
    window.addEventListener("focus", onFocus);

    return () => {
      stopTimer();
      stopDragWatch();
      if (scratchTimer) clearTimeout(scratchTimer);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("focus", onFocus);
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
        items={clipboardItems}
        loading={clipboardLoading}
        compact
        onRefresh={refreshClipboard}
        onPasteStart={() => (pasting = true)}
        onPasted={() => void closePanels()}
        onError={() => (pasting = false)}
      />
    {:else}
      <div class="p-tabs" role="tablist" aria-label="Vistas de fragmentos">
        <button
          type="button"
          role="tab"
          class="p-tab"
          class:active={snippetsTab === "list"}
          aria-selected={snippetsTab === "list"}
          onclick={() => (snippetsTab = "list")}
        >
          Lista
        </button>
        <button
          type="button"
          role="tab"
          class="p-tab"
          class:active={snippetsTab === "scratchpad"}
          aria-selected={snippetsTab === "scratchpad"}
          onclick={() => (snippetsTab = "scratchpad")}
        >
          Bloc
        </button>
      </div>
      {#if snippetsTab === "list"}
        <SnippetsList
          items={snippetItems}
          loading={snippetsLoading}
          compact
          onRefresh={refreshSnippets}
          onPasteStart={() => (pasting = true)}
          onPasted={() => void closePanels()}
          onError={() => (pasting = false)}
        />
      {:else if scratchLoading}
        <p class="p-empty">Cargando bloc…</p>
      {:else}
        <textarea
          class="p-scratch"
          bind:value={scratchBody}
          oninput={scheduleScratchSave}
          placeholder="Notas temporales…"
          aria-label="Bloc de notas"
          data-no-drag
        ></textarea>
      {/if}
    {/if}
  </div>
{/snippet}

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="p-root"
  class:is-wheel={surface === "wheel"}
  class:is-panel={panelOpen}
  class:is-up={panelUp && panelOpen}
  class:is-quick={wheelQuick}
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

  <div class="p-stack" class:is-dim={surface === "wheel"}>
    <div class="p-shell">
      <!-- La barra se mide sola (`max-content`): no hay tabla de anchos. -->
      <div
        class="p-bar"
        class:is-disc-only={!panelOpen && activity === "idle" && !hasQueue}
        bind:this={barEl}
      >
        {#if panelOpen}
          <span class="p-mark"><AticMark size={15} strokeWidth={1.5} /></span>
          <span class="p-label">
            {surface === "clipboard" ? "Clipboard" : "Fragmentos"}
          </span>
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
        {:else if activity === "dictating"}
          <button
            type="button"
            class="p-dict"
            class:is-live={dictation === "listening"}
            class:is-busy={dictation === "transcribing"}
            class:is-ok={dictation === "pasted"}
            class:is-error={dictation === "error"}
            data-no-drag
            onclick={toggleDictate}
            disabled={busy || dictation === "transcribing"}
            aria-label={dictation === "listening" ? "Detener dictado" : "Dictado"}
            title={dictationLabel(dictation)}
          >
            <ToolIcon id="dictation" size={16} strokeWidth={1.5} />
          </button>
          <span class="p-label" aria-live="polite">{dictationLabel(dictation)}</span>
          {#if dictation === "listening"}
            <div class="p-wave">
              <Waveform mic={levels.mic} system={0} bars={8} variant="quiet" />
            </div>
          {/if}
        {:else if hasQueue}
          <!-- La cola es un badge sobre el disco, no un reemplazo: antes borraba
               la pill entera y con ella el acceso a la rueda. -->
          <span class="p-mark is-disc"><AticMark size={20} strokeWidth={1.4} /></span>
          <span class="p-queue-count">{queue.length}</span>
          <span class="p-queue-text" title={queue[0]?.text}>{queue[0]?.text ?? ""}</span>
          <button
            type="button"
            class="p-queue-btn"
            data-no-drag
            disabled={queueBusy}
            onclick={() => void queueAction(pasteQueueItemNow)}
          >
            Pegar
          </button>
          {@render iconBtn("Descartar", "M6 6l12 12M18 6L6 18", () =>
            void queueAction(dismissPasteQueueItem),
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
        {/if}
      </div>
    </div>

    {#if panelOpen}
      {@render panelBody()}
    {/if}
  </div>
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: transparent;
  }

  .p-root {
    position: relative;
    display: flex;
    width: 100vw;
    height: 100vh;
    box-sizing: border-box;
    flex-direction: column;
    padding: 4px;
    overflow: hidden;
    cursor: grab;
  }
  .p-root:active {
    cursor: grabbing;
  }
  .p-root.is-wheel {
    padding: 0;
    cursor: default;
  }
  /* El panel hacia arriba invierte el orden visual sin tocar el DOM. */
  .p-root.is-up .p-stack {
    flex-direction: column-reverse;
  }

  /* Cierre acelerado: al elegir herramienta la rueda ya cumplió su función. */
  .p-root.is-quick {
    --morph-close-dur: var(--morph-quick-dur);
    --morph-fade-dur: var(--morph-quick-dur);
  }

  .p-stack {
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

  /* Solo el disco escala. Si escalara la rueda entera, la marca del centro
     crecería con ella y dejaría de ser el punto fijo del morph. */
  .p-wheel::before {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background: color-mix(in srgb, var(--rb-bg0) 92%, transparent);
    transform: scale(0.19);
    transition: transform var(--morph-close-dur) var(--morph-close-ease);
  }
  .p-wheel.is-open::before {
    transform: scale(1);
    transition: transform var(--morph-open-dur) var(--morph-ease);
  }

  /* ─── Barra ─────────────────────────────────────────────────────────── */
  /* Una sola piel para todos los estados. Antes la barra y la tira de cola
     declaraban la misma superficie por separado y se desincronizaban. */
  .p-shell {
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
    background: color-mix(in srgb, var(--rb-surface) 97%, transparent);
    color: var(--rb-text);
    transition:
      border-radius var(--morph-close-dur) var(--morph-close-ease),
      transform var(--morph-close-dur) var(--morph-close-ease);
  }
  /* Con panel la barra sí ocupa el ancho: es la cabecera del panel. */
  .p-root.is-panel .p-shell {
    width: 100%;
    border-radius: 16px 16px 0 0;
  }
  .p-root.is-panel.is-up .p-shell {
    border-radius: 0 0 16px 16px;
  }

  /* max-content: el ancho lo fija el contenido, no la ventana. Es lo que hace
     que medir la barra no se realimente con el resize. */
  .p-bar {
    display: flex;
    width: max-content;
    min-width: 40px;
    height: 100%;
    align-items: center;
    gap: 8px;
    padding: 0 12px 0 10px;
    white-space: nowrap;
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
  .p-dict.is-live {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 10%, transparent);
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

  /* ─── Panel ─────────────────────────────────────────────────────────── */
  .p-panel {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    border-radius: 0 0 16px 16px;
    padding: 0.45rem 0.5rem 0.55rem;
    background: color-mix(in srgb, var(--rb-surface) 97%, transparent);
    color: var(--rb-text);
    overflow: hidden;
    cursor: default;
  }
  .p-root.is-up .p-panel {
    border-radius: 16px 16px 0 0;
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

  .p-empty {
    margin: 0.35rem 0 0;
    color: var(--rb-muted);
    font-size: 0.75rem;
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
    .p-wheel::before,
    .p-wheel.is-open::before,
    .p-stack,
    .p-shell,
    .p-icon,
    .p-rec,
    .p-dict,
    .p-tab {
      transition: none !important;
    }
    .p-stack.is-dim {
      filter: none;
    }
  }
</style>
