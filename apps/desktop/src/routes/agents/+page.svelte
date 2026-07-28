<script lang="ts">
  /**
   * La consola de agentes: una burbuja que sale de la pill.
   *
   * # Por qué burbuja y no ventana suelta
   *
   * Una ventana más sería una app aparte que además tiene una pill. La punta
   * apuntando a la pill dice que es la misma cosa desplegada, y hace obvio de
   * dónde salió y adónde vuelve al cerrarse. Rust decide de qué lado va la
   * punta (es quien ve los monitores) y esta vista solo la dibuja.
   *
   * # De dónde sale el aspecto
   *
   * De dos sitios, a propósito:
   *
   *  - **La consola de Claude Code** para el registro: monoespaciada, fondo
   *    casi negro cálido, un acento coral, cajas con el título incrustado en el
   *    borde y avisos con barra vertical. Quien ya usa el CLI reconoce lo que
   *    está mirando; inventar un lenguaje propio acá solo habría hecho que
   *    hubiera que aprender dos.
   *  - **El compositor de las GUI de agentes (T3 Code y parecidas)** para lo de
   *    abajo: caja redondeada, controles en pastillas con su valor a la vista
   *    —modelo, permisos, carpeta—, anillo de contexto y botón circular de
   *    enviar. Un `<select>` de formulario ahí abajo rompía el tono y encima
   *    escondía el valor actual, que es lo que uno mira antes de mandar.
   *
   * El estado no vive acá: vive en `agents`, que escucha desde que arranca la
   * app. Cerrar la burbuja es dejar de mirar, no terminar la sesión.
   */
  import { onMount, tick } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { agents } from "$lib/agentSessions.svelte";
  import { Bubble } from "$lib/bubble.svelte";
  import {
    agentBackends,
    agentThread,
    agentThreadDelete,
    agentThreads,
    capturePrimaryMonitor,
    resizeAgentsBubble,
    hideAgentsWindow,
    toggleDictation,
    onDictationStatus,
    onAgentsBubbleAnchor,
    onAgentsBubbleDismiss,
    onAgentsComposerInsert,
    readClipboardDragText,
    agentListModels,
    type AgentsComposerInsert,
  } from "$lib/api";
  import {
    effortLabelFor,
    effortShortLabel,
    filterVisibleModels,
    isFilterableBackend,
    modelLabelFor,
    rememberBackend,
    rememberCwd,
    rememberEffort,
    rememberFast,
    rememberMode,
    rememberModel,
    rememberedBackend,
    rememberedCwd,
    rememberedEffort,
    rememberedFast,
    rememberedMode,
    rememberedModel,
    resolveModelChoice,
    setVisibleModelIds,
  } from "$lib/agentModels";
  import { formatDate, formatListWhen } from "$lib/format";
  import { applyTheme, readCachedTheme } from "$lib/theme";
  import { MOTION, ms } from "$lib/motion";
  import AgentModelsModal from "$lib/AgentModelsModal.svelte";
  import AgentToolsModal from "$lib/AgentToolsModal.svelte";
  import PickerMenu from "$lib/PickerMenu.svelte";
  import AgentMark from "$lib/AgentMark.svelte";
  import AgentConversation from "$lib/AgentConversation.svelte";
  import AgentIcons from "$lib/AgentIcons.svelte";
  import type {
    AgentBackendInfo,
    AgentModel,
    AgentOrigin,
    PermissionDecision,
    StoredThread,
  } from "$lib/types";

  /** `manual` primero a propósito: con alguien mirando, preguntar cuesta poco
   *  y equivocarse cuesta caro. Los permisivos existen para tareas largas. */
  const MODES = [
    { id: "manual", label: "Preguntar" },
    { id: "acceptEdits", label: "Aceptar ediciones" },
    { id: "plan", label: "Solo planificar" },
    { id: "bypassPermissions", label: "Acceso total" },
  ];

  /** El escudo que corresponde al modo. La forma dice el nivel de guardia. */
  const SHIELDS: Record<string, "shield-manual" | "shield-edits" | "shield-plan" | "shield-open"> = {
    manual: "shield-manual",
    acceptEdits: "shield-edits",
    plan: "shield-plan",
    bypassPermissions: "shield-open",
  };

  /**
   * Con qué ventana se dibuja el anillo cuando el agente no dice la suya.
   *
   * Es la de Claude, que es de donde salió el número. Los que la informan
   * —ACP y Codex— pisan esto con la de verdad: la de Codex ronda los 258K, así
   * que con la constante el anillo mostraba un cuarto de lo consumido.
   */
  const CONTEXT_WINDOW = 1_000_000;

  /** Dónde cae el globo y cuándo se puede pintar. Vive en `$lib/bubble`. */
  const bubble = new Bubble();
  /** Hay un diálogo del sistema abierto. */
  let picking = $state(false);

  let backends = $state<AgentBackendInfo[]>([]);
  let picked = $state("");
  let model = $state("");
  let mode = $state("manual");
  /** Cuánto tiene que pensar. Vacío = lo que traiga el backend. */
  let effort = $state("");
  /** Variante rápida (Cursor). Independiente del nivel. */
  let fast = $state(false);
  let cwd = $state("");
  let starting = $state(false);
  let error = $state<string | null>(null);
  /** Catálogo descubierto por backend (`agent_list_models`). */
  let discoveredModels = $state<Record<string, AgentModel[]>>({});
  /** Backends ya consultados (también si la lista vino vacía). */
  let discoveredTried = $state<Record<string, boolean>>({});
  let modelsLoading = $state(false);
  let draft = $state("");
  /** Rutas absolutas de imágenes pendientes de mandar como content. */
  let attachments = $state<string[]>([]);
  let previewPath = $state<string | null>(null);
  let logEl = $state<HTMLElement | null>(null);
  let inputEl = $state<HTMLTextAreaElement | null>(null);
  let toolsOpen = $state(false);
  let modelsConfigOpen = $state(false);
  /** Invalida el derivado de modelos visibles tras guardar el filtro. */
  let modelFilterTick = $state(0);
  let menu = $state<"model" | "mode" | "effort" | "plus" | null>(null);

  let activeId = $state<string | null>(null);

  /**
   * El historial, y lo que se está leyendo de él.
   *
   * `null` en `history` es «el historial está cerrado»; una lista vacía es «no
   * hay nada guardado», que es otra cosa y se dice con otras palabras.
   *
   * La persistencia existía desde la fase 0 y no se podía ver sin abrir el
   * `atic.db3` a mano. Claude Code y Codex pueden reanudarla; en ACP todavía no
   * se ofrece un botón que arrancaría una sesión nueva fingiendo que sigue la
   * vieja.
   */
  let history = $state<StoredThread[] | null>(null);
  let reading = $state<StoredThread | null>(null);
  /** Filtro del listado de conversaciones guardadas. */
  let histQuery = $state("");
  let histLoading = $state(false);
  /** Hilo con el borrado a medias: se pide confirmar en el propio botón. */
  let forgetting = $state<string | null>(null);

  const active = $derived(agents.byId(activeId));
  const ready = $derived(
    backends.find((b) => b.id === picked)?.available ?? false,
  );
  /**
   * Los modelos del backend que está en juego.
   *
   * Con sesión abierta manda el suyo; sin ella, el que se eligió para arrancar.
   * Antes era una lista fija de alias de Claude para los cuatro agentes, así
   * que elegir «Opus 5» en Codex le pasaba un modelo que no existe.
   */
  /**
   * El acento de cada agente, para la pestaña.
   *
   * Duplica los valores del CSS a propósito: acá se necesitan como dato —van a
   * una variable en línea por pestaña— y allá como regla. Leerlos del CSS
   * obligaría a consultar el estilo computado en cada render.
   */
  const ACCENTS: Record<string, string> = {
    "claude-code": "#da7756",
    opencode: "#7fae86",
    codex: "#8fa9b8",
    cursor: "#a88fc4",
  };

  /**
   * Redimensionado en curso: de dónde salió el arrastre y con qué tamaño.
   *
   * Se miden las coordenadas de PANTALLA y no las de la ventana: el borde
   * anclado se queda quieto pero el opuesto se mueve mientras estirás, así que
   * el marco de referencia de la ventana cambia bajo el cursor y las
   * coordenadas locales darían un arrastre que se acelera solo.
   */
  let rz = $state<{
    axis: "h" | "v" | "both";
    x: number;
    y: number;
    w: number;
    h: number;
  } | null>(null);

  function startResize(event: PointerEvent, axis: "h" | "v" | "both") {
    if (!bubble.anchor) return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    rz = {
      axis,
      x: event.screenX,
      y: event.screenY,
      w: bubble.anchor.w,
      h: bubble.anchor.h,
    };
  }

  function moveResize(event: PointerEvent) {
    if (!rz) return;
    const g = bubble.grips;
    // El signo lo da de qué lado se agarra: por la izquierda, alejarse del
    // centro es restar coordenada y sumar ancho.
    const dx = (event.screenX - rz.x) * (g.h === "right" ? 1 : -1);
    const dy = (event.screenY - rz.y) * (g.v === "bottom" ? 1 : -1);
    const w = Math.max(544, rz.axis === "v" ? rz.w : rz.w + dx);
    const h = Math.max(464, rz.axis === "h" ? rz.h : rz.h + dy);
    bubble.resized(Math.round(w), Math.round(h));
    void resizeAgentsBubble(
      Math.round(w),
      Math.round(h),
      bubble.anchor?.side ?? "top",
      false,
    );
  }

  function endResize(event: PointerEvent) {
    if (!rz) return;
    moveResize(event);
    rz = null;
    // El último tamaño, ahora sí, al disco.
    void resizeAgentsBubble(
      Math.round(bubble.anchor?.w ?? 704),
      Math.round(bubble.anchor?.h ?? 644),
      bubble.anchor?.side ?? "top",
      true,
    );
  }

  const backendForModels = $derived(active?.backendId ?? picked);

  /**
   * Catálogo crudo (con efforts) del backend activo o descubierto.
   *
   * Con sesión abierta mandan los que informó el agente. Sin ella, el de
   * `agent_list_models` (cacheado en Rust, precargado al arrancar).
   */
  const RAW_MODELS = $derived.by((): AgentModel[] => {
    if (active && active.models.length > 0) return active.models;
    return discoveredModels[backendForModels] ?? [];
  });

  /**
   * Los modelos que se pueden elegir ahora mismo (forma del picker).
   */
  const MODELS = $derived.by(() =>
    RAW_MODELS.map((m) => ({
      id: m.id,
      label: m.name,
      note: m.description,
      efforts: m.efforts,
      defaultEffort: m.defaultEffort,
      supportsFast: m.supportsFast,
    })),
  );
  /** Catálogo filtrado para el picker (Cursor / OpenCode). */
  const DISPLAY_MODELS = $derived.by(() => {
    void modelFilterTick;
    return filterVisibleModels(backendForModels, MODELS);
  });
  const modelLabel = $derived(
    modelsLoading && MODELS.length === 0
      ? "Modelos…"
      : (DISPLAY_MODELS.find((m) => m.id === model)?.label ??
        MODELS.find((m) => m.id === model)?.label ??
        modelLabelFor(model, MODELS)),
  );
  const modelsBackendLabel = $derived(
    backends.find((b) => b.id === backendForModels)?.displayName ??
      backendForModels,
  );

  /** Los esfuerzos del modelo elegido. Vacío = este modelo no los ofrece. */
  const EFFORTS = $derived.by(() => {
    const m = RAW_MODELS.find((x) => x.id === model);
    const rank: Record<string, number> = {
      none: 0,
      minimal: 0,
      low: 1,
      default: 2,
      medium: 3,
      high: 4,
      xhigh: 5,
      max: 6,
    };
    return [...(m?.efforts ?? [])]
      .sort((a, b) => (rank[a.id] ?? 50) - (rank[b.id] ?? 50))
      .map((e) => ({
        id: e.id,
        label: effortShortLabel(e.id),
        note: e.description,
      }));
  });
  const effortLabel = $derived(effortLabelFor(RAW_MODELS, model, effort));
  const supportsFast = $derived(
    !!RAW_MODELS.find((m) => m.id === model)?.supportsFast,
  );
  const modeLabel = $derived(MODES.find((m) => m.id === mode)?.label ?? "Permisos");
  const ctxPct = $derived(
    Math.min(
      100,
      ((active?.contextTokens ?? 0) / (active?.contextSize || CONTEXT_WINDOW)) *
        100,
    ),
  );

  /**
   * De dónde sale la conversación que se dibuja.
   *
   * Un hilo guardado tiene la misma forma que uno vivo —turnos con items—, así
   * que se pinta con el mismo código y no con una vista de solo lectura aparte.
   * Lo único que cambia es que el guardado ya no crece.
   */
  const viewTurns = $derived(reading?.turns ?? active?.turns ?? []);

  /**
   * Los items de la conversación, en orden.
   *
   * Antes acá vivía una derivación que recorría el registro plano emparejando
   * cada `toolCall` con su `toolResult` por id, porque llegaban como eventos
   * separados y con lo que hubiera en medio. Ya no hace falta: el backend
   * manda items con identidad y el store los mantiene actualizados, así que
   * esto es aplanar turnos y nada más.
   */
  const items = $derived(viewTurns.flatMap((t) => t.items));

  /**
   * El agente está escribiendo ahora mismo.
   *
   * Solo los items de texto tienen `streaming`; una herramienta corriendo no
   * cuenta, porque ahí lo que se muestra es su tarjeta latiendo y no un cursor.
   */
  const writing = $derived.by(() => {
    return items.some((it) => "streaming" in it && !!it.streaming);
  });
  const sendBlocked = $derived(
    !!active &&
      (active.status === "working" || active.pending.length > 0 || writing),
  );
  type ComposerState = "idle" | "streaming" | "awaitingPermission" | "starting";
  const composerState = $derived.by<ComposerState>(() => {
    if (starting) return "starting";
    if (active?.pending.length) return "awaitingPermission";
    if (active?.status === "working" || writing) return "streaming";
    return "idle";
  });
  const singlePending = $derived(
    active?.pending.length === 1 ? active.pending[0] : null,
  );
  const composerActionLabel = $derived(
    composerState === "streaming"
      ? "Detener"
      : composerState === "awaitingPermission"
        ? "Aprobar"
        : active
          ? "Enviar"
          : "Iniciar",
  );
  const composerActionAria = $derived(
    composerState === "streaming" ? "Espera a que termine" : composerActionLabel,
  );
  const composerDisabled = $derived(
    composerState === "starting" ||
      composerState === "streaming" ||
      (composerState === "awaitingPermission" && !singlePending) ||
      (composerState === "idle" &&
        (active ? !draft.trim() && attachments.length === 0 : !ready)),
  );

  /** El cierre de cada turno, para dibujar la regla con el costo. */
  const turnEnds = $derived(
    new Map(
      viewTurns
        .filter((t) => t.status !== "running")
        .map((t) => [t.items.at(-1)?.id ?? t.id, t.costUsd]),
    ),
  );

  onMount(() => {
    applyTheme(readCachedTheme());
    void agents.init();
    // Al abrir la ventana y no al escribir `/`: leer el disco toma lo suyo, y
    // hacerlo con la lista ya desplegada la dejaría llenándose sola debajo del
    // cursor justo mientras se elige.
    void agents.loadSkills(active?.cwd || undefined);

    // La burbuja no se pinta hasta saber dónde va la punta: al abrirse ya tiene
    // que estar bien, no acomodarse a la vista.
    const un = onAgentsBubbleAnchor((a) => bubble.place(a));

    // Quién es dueño del próximo pegado. `pasted` dura un momento largo antes
    // de volver a `idle`, así que cubre de sobra al evento del DOM y da lo
    // mismo cuál de los dos llegue primero.
    const unDict = onDictationStatus(({ phase }) => {
      dictating =
        phase === "listening" || phase === "transcribing" || phase === "pasted";
      if (phase === "pasted") origin = { via: "dictado" };
    });

    // Moverla a mano la desancla. Se detecta por el evento de la ventana y no
    // por el arrastre en sí: Windows también la mueve al acomodarla contra un
    // borde, y eso cuenta igual.
    let unMoved: (() => void) | null = null;
    void (async () => {
      try {
        unMoved = await getCurrentWindow().onMoved(() => bubble.moved());
      } catch {
        // Sin ventana nativa no hay nada que seguir.
      }
    })();

    // La rueda pide cerrarla (segunda pulsación sobre «Agentes»).
    const unDismiss = onAgentsBubbleDismiss(() => void close());

    // Clipboard → compositor (clic con agentes abierto; no Ctrl+V externo).
    const unInsert = onAgentsComposerInsert((payload) => {
      void acceptComposerInsert(payload);
    });

    // NO se cierra al perder el foco.
    //
    // Lo hacía, por simetría con la rueda y el historial, y estaba mal: esos
    // aparecen para una acción y se van. Una sesión de agente es una
    // conversación en curso — abrir el clipboard para copiar algo que le vas a
    // pegar la mataba justo cuando hacía falta. La rueda la abre y la cierra;
    // perder el foco solo la manda atrás.

    void (async () => {
      try {
        backends = await agentBackends();
        picked = rememberedBackend(backends);
        if (picked) {
          mode = rememberedMode(picked);
          const folder = rememberedCwd(picked);
          if (folder) cwd = folder;
        }
      } catch (err) {
        error = String(err);
      }
    })();

    // Drop nativo (OLE / Explorador / arrastre desde el clipboard de la pill).
    let unDrop: (() => void) | null = null;
    void (async () => {
      try {
        unDrop = await getCurrentWindow().onDragDropEvent((event) => {
          if (event.payload.type !== "drop") return;
          void acceptDroppedPaths(event.payload.paths);
        });
      } catch {
        // Sin ventana nativa no hay drop OS.
      }
    })();

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (previewPath) {
        previewPath = null;
        event.preventDefault();
        return;
      }
      if (toolsOpen || modelsConfigOpen || menu) return;
      event.preventDefault();
      // Esc deshace un paso a la vez: primero el historial, y recién con la
      // conversación a la vista cierra la burbuja.
      if (history !== null) {
        backFromHistory();
        return;
      }
      void close();
    };
    window.addEventListener("keydown", onKeyDown);

    return () => {
      window.removeEventListener("keydown", onKeyDown);
      void un.then((fn) => fn());
      void unDict.then((fn) => fn());
      void unDismiss.then((fn) => fn());
      void unInsert.then((fn) => fn());
      unMoved?.();
      unDrop?.();
      agents.watch(null);
    };
  });

  $effect(() => {
    if (activeId && agents.byId(activeId)) return;
    // Hay sesión del agente elegido: adopta esa, no la primera de la lista.
    const forPicked = agents.sessions.find((s) => s.backendId === picked);
    if (forPicked) {
      activeId = forPicked.id;
      return;
    }
    // Elegiste un agente sin sesión todavía: no robes el foco a otra
    // conversación viva. Así se pueden tener varias en paralelo.
    if (picked) {
      activeId = null;
      return;
    }
    activeId = agents.sessions[0]?.id ?? null;
  });

  $effect(() => {
    agents.watch(activeId);
  });

  // Cambiar de agente (o lista vacía) puede dejar un id inválido. Se restaura
  // el recordado para ese backend, o el primero de la lista. También resuelve
  // slugs wire viejos (`…-high`) al id de grupo + effort + fast.
  $effect(() => {
    void modelFilterTick;
    const catalog = MODELS.filter((m) => m.id);
    const list = filterVisibleModels(backendForModels, catalog);
    if (list.length === 0) {
      if (model) model = "";
      if (effort) effort = "";
      if (fast) fast = false;
      return;
    }
    if (!model || !list.some((m) => m.id === model)) {
      const remembered = rememberedModel(backendForModels, list);
      const resolved = resolveModelChoice(list, remembered || model);
      model = resolved.modelId;
      if (resolved.effortId) {
        effort = resolved.effortId;
      } else {
        effort = rememberedEffort(backendForModels, model, list);
      }
      fast = list.find((m) => m.id === model)?.supportsFast
        ? rememberedFast(backendForModels, model, list) || resolved.fast
        : false;
      return;
    }
    const m = list.find((x) => x.id === model);
    if (m?.efforts?.length) {
      if (!effort || !m.efforts.some((e) => e.id === effort)) {
        effort = rememberedEffort(backendForModels, model, list);
      }
    } else if (effort) {
      effort = "";
    }
    if (m?.supportsFast) {
      // no pisar un fast ya elegido en esta sesión si el modelo no cambió
    } else if (fast) {
      fast = false;
    }
  });

  // Catálogo del proveedor: precarga todos los disponibles al montar, y
  // refresca el elegido si aún no está. El cache de Rust (5 min) hace que
  // las llamadas tras el arranque sean baratas.
  $effect(() => {
    const ids = backends.filter((b) => b.available).map((b) => b.id);
    if (ids.length === 0) return;
    let cancelled = false;
    const pending = ids.filter((id) => !discoveredTried[id]);
    if (pending.length === 0) return;
    modelsLoading = true;
    void Promise.all(
      pending.map((backend) =>
        agentListModels(backend)
          .then((list) => {
            if (cancelled) return;
            discoveredModels = { ...discoveredModels, [backend]: list };
            discoveredTried = { ...discoveredTried, [backend]: true };
          })
          .catch((err) => {
            if (cancelled) return;
            discoveredTried = { ...discoveredTried, [backend]: true };
            // Solo surface el error del backend que el usuario está mirando.
            if (backend === picked) error = String(err);
          }),
      ),
    ).finally(() => {
      if (!cancelled) modelsLoading = false;
    });
    return () => {
      cancelled = true;
    };
  });

  // El textarea crece con el texto en vez de hacer scroll interno.
  $effect(() => {
    draft;
    void tick().then(resizeComposer);
  });

  // Con sesión viva, el backend manda el modo. Sin sesión, el de la UI es el
  // que recordamos para ese agente (escudo Opus/High/etc. al reabrir).
  $effect(() => {
    if (active?.mode) {
      if (active.mode !== mode) mode = active.mode;
      return;
    }
    if (!picked) return;
    const remembered = rememberedMode(picked);
    if (mode !== remembered) mode = remembered;
  });

  // Sesión viva: el patch puede traer slug wire; lo partimos a grupo+effort+fast.
  $effect(() => {
    const sessionModel = active?.model;
    if (!sessionModel || !active) return;
    const models = active.models.length > 0 ? active.models : RAW_MODELS;
    const resolved = resolveModelChoice(models, sessionModel);
    if (resolved.modelId && resolved.modelId !== model) {
      model = resolved.modelId;
    }
    const sessionEffort = active.effort ?? resolved.effortId;
    if (sessionEffort && sessionEffort !== effort) {
      effort = sessionEffort;
    }
    if (active.fast !== null && active.fast !== undefined) {
      if (active.fast !== fast) fast = active.fast;
    } else if (resolved.fast !== fast && supportsFast) {
      fast = resolved.fast;
    }
  });

  $effect(() => {
    // También con cada trozo del streaming: si no, el texto crece por debajo
    // del borde y hay que perseguirlo con la rueda mientras se escribe.
    const last = items.at(-1);
    const n = items.length + (last && "text" in last ? last.text.length : 0);
    // Sobre la lista del historial no: ahí abajo puede haber una sesión viva
    // creciendo, y el salto caería justo mientras se lee la lista.
    if (!logEl || n === 0 || (history !== null && !reading)) return;
    void tick().then(() => {
      if (logEl) logEl.scrollTop = logEl.scrollHeight;
    });
  });

  /**
   * Cierra replegándose sobre la pill.
   *
   * Quien mueve y oculta la ventana es Rust: acá solo se apaga el contenido. Si
   * el ocultado viviera en esta vista, un cuelgue del webview a mitad de la
   * animación dejaría una ventana muerta en pantalla sin forma de cerrarla.
   */
  async function close() {
    if (!bubble.shown) return;
    bubble.hide();
    try {
      await hideAgentsWindow();
    } catch {
      // Sin ventana nativa (preview web) no hay nada que replegar.
    }
  }

  function openTools() {
    toolsOpen = true;
    void agents.loadSkills(cwd || active?.cwd);
  }

  async function pickFolder() {
    picking = true;
    try {
      const chosen = await openDialog({ directory: true, multiple: false });
      if (typeof chosen === "string") {
        cwd = chosen;
        if (picked) rememberCwd(picked, chosen);
      }
    } catch (err) {
      error = String(err);
    } finally {
      picking = false;
    }
  }

  function selectBackend(id: string, sessionId: string | null) {
    picked = id;
    rememberBackend(id);
    if (sessionId) {
      activeId = sessionId;
      return;
    }
    activeId = null;
    mode = rememberedMode(id);
    cwd = rememberedCwd(id);
  }

  function selectMode(id: string) {
    mode = id;
    if (picked) rememberMode(picked, id);
    menu = null;
  }

  /**
   * Dicta dentro del compositor.
   *
   * El foco va PRIMERO: el dictado de Atic pega en el control que tenga el
   * foco del sistema cuando termina, así que sin esto el texto acabaría en la
   * app que estuviera detrás de la burbuja.
   */
  /**
   * De dónde salió lo que hay ahora en el compositor.
   *
   * Se marca cuando lo puso un puente de Atic —el dictado, una captura, el
   * portapapeles— y viaja con el mensaje. Se limpia al enviar y al vaciar la
   * caja: si borraste todo y escribiste a mano, ya no vino de ahí.
   */
  let origin = $state<AgentOrigin | null>(null);

  /**
   * El dictado está en curso y le pertenece el próximo pegado.
   *
   * Hace falta porque el dictado entrega su texto **pegándolo** —lo pone en el
   * portapapeles y manda Ctrl+V—, así que en la caja llega como un `paste`
   * idéntico al que harías vos. Sin distinguirlos, hablarle al agente quedaría
   * registrado como si lo hubieras pegado.
   */
  let dictating = $state(false);

  /* Caja vacía es caja sin procedencia: si borraste lo dictado y escribiste a
     mano, el próximo envío no tiene por qué decir que lo dictaste. Con el
     dictado en curso no se toca, que ahí está vacía justamente porque la voz
     todavía no llegó. Con adjuntos pendientes el origen se conserva. */
  $effect(() => {
    if (!draft.trim() && !dictating && attachments.length === 0) origin = null;
  });

  function isImagePath(path: string): boolean {
    return /\.(png|jpe?g|gif|webp)$/i.test(path);
  }

  function fileName(path: string): string {
    return path.split(/[\\/]/).pop() || path;
  }

  async function dictate() {
    inputEl?.focus();
    try {
      await toggleDictation();
    } catch (err) {
      error = String(err);
    }
  }

  /**
   * Una captura de pantalla para el agente.
   *
   * Va como content de imagen (base64 en el backend), no como ruta en el texto:
   * el modelo la ve; no tiene que adivinar un path y abrirla con Read.
   */
  async function capture() {
    try {
      const path = await capturePrimaryMonitor();
      attachments = [...attachments, path];
      origin = { via: "captura", file: fileName(path), files: [...attachments] };
      inputEl?.focus();
    } catch (err) {
      error = String(err);
    }
  }

  async function attach() {
    picking = true;
    try {
      const chosen = await openDialog({
        multiple: true,
        filters: [
          { name: "Imágenes", extensions: ["png", "jpg", "jpeg", "gif", "webp"] },
          { name: "Todos", extensions: ["*"] },
        ],
      });
      const paths = Array.isArray(chosen) ? chosen : chosen ? [chosen] : [];
      if (paths.length === 0) return;
      const images = paths.filter(isImagePath);
      const others = paths.filter((p) => !isImagePath(p));
      if (images.length > 0) {
        attachments = [...attachments, ...images];
        origin = {
          via: origin?.via === "captura" ? "captura" : "archivo",
          file: fileName(images[0]),
          files: [...attachments],
        };
      }
      // Lo que no es imagen sigue como ruta: el agente lo abre cuando le sirva.
      if (others.length > 0) {
        draft = [draft.trim(), ...others].filter(Boolean).join("\n");
      }
      inputEl?.focus();
    } catch (err) {
      error = String(err);
    } finally {
      picking = false;
    }
  }

  function removeAttachment(path: string) {
    if (previewPath === path) previewPath = null;
    attachments = attachments.filter((p) => p !== path);
    if (attachments.length === 0) {
      if (
        origin?.via === "captura" ||
        origin?.via === "archivo" ||
        origin?.via === "portapapeles"
      ) {
        if (!draft.trim()) origin = null;
      }
    } else if (origin) {
      origin = {
        ...origin,
        file: fileName(attachments[0]),
        files: [...attachments],
      };
    }
  }

  function addImageAttachments(paths: string[], via = "portapapeles") {
    const images = paths.filter(isImagePath);
    if (images.length === 0) return;
    const next = [...attachments];
    for (const path of images) {
      if (!next.includes(path)) next.push(path);
    }
    attachments = next;
    origin = {
      via,
      file: fileName(images[0]),
      files: [...attachments],
    };
  }

  function appendDraftText(text: string, via = "portapapeles") {
    const t = text.trimEnd();
    if (!t) return;
    draft = draft.trim() ? `${draft.replace(/\s+$/, "")}\n${t}` : t;
    origin = {
      via: dictating ? "dictado" : via,
      file: origin?.file,
      files: attachments,
    };
  }

  async function acceptComposerInsert(payload: AgentsComposerInsert) {
    if (history !== null) return;
    if (payload.kind === "image" && payload.imagePath) {
      addImageAttachments([payload.imagePath], "portapapeles");
    } else if (payload.kind === "text" && payload.text) {
      appendDraftText(payload.text);
    }
    await tick();
    inputEl?.focus();
  }

  function isClipboardDragText(path: string): boolean {
    const name = fileName(path);
    return name.startsWith(".atic-drag-") && name.endsWith(".txt");
  }

  async function acceptDroppedPaths(paths: string[]) {
    if (history !== null || paths.length === 0) return;
    const images: string[] = [];
    const others: string[] = [];
    for (const path of paths) {
      if (isClipboardDragText(path)) {
        try {
          appendDraftText(await readClipboardDragText(path));
        } catch (err) {
          error = String(err);
        }
      } else if (isImagePath(path)) {
        images.push(path);
      } else {
        others.push(path);
      }
    }
    if (images.length > 0) addImageAttachments(images, "archivo");
    if (others.length > 0) {
      draft = [draft.trim(), ...others].filter(Boolean).join("\n");
      origin = origin ?? { via: "archivo" };
    }
    await tick();
    inputEl?.focus();
  }

  function onComposerDragOver(event: DragEvent) {
    if (!event.dataTransfer) return;
    const types = [...event.dataTransfer.types];
    if (types.includes("text/plain") || types.includes("Files")) {
      event.preventDefault();
      event.dataTransfer.dropEffect = "copy";
    }
  }

  async function onComposerDrop(event: DragEvent) {
    event.preventDefault();
    if (history !== null) return;
    const text = event.dataTransfer?.getData("text/plain")?.trim();
    if (text) appendDraftText(text);
    const files = event.dataTransfer?.files;
    if (files && files.length > 0) {
      // En Tauri el drop OS llega por onDragDropEvent; esto cubre HTML5.
      const named = [...files]
        .map((f) => (f as File & { path?: string }).path)
        .filter(Boolean) as string[];
      if (named.length > 0) await acceptDroppedPaths(named);
    }
    await tick();
    inputEl?.focus();
  }

  /** `updated_at` viene en segundos; `formatDate` habla ISO. */
  const when = (secs: number) => formatDate(new Date(secs * 1000).toISOString());
  const whenList = (secs: number) => formatListWhen(secs);

  const historyVisible = $derived.by(() => {
    if (!history) return [];
    const q = histQuery.trim().toLowerCase();
    if (!q) return history;
    return history.filter((t) => {
      const hay = `${t.preview} ${t.cwd} ${t.backendName} ${t.model}`.toLowerCase();
      return hay.includes(q);
    });
  });

  function folderTail(path: string): string {
    return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
  }

  async function openHistory() {
    menu = null;
    slashOpen = false;
    reading = null;
    forgetting = null;
    histQuery = "";
    error = null;
    histLoading = true;
    history = [];
    try {
      history = await agentThreads();
    } catch (err) {
      error = String(err);
      history = [];
    } finally {
      histLoading = false;
    }
  }

  /** Un paso atrás: del hilo a la lista, y de la lista a la conversación. */
  function backFromHistory() {
    forgetting = null;
    if (reading) {
      reading = null;
      return;
    }
    histQuery = "";
    history = null;
  }

  async function readThread(id: string) {
    error = null;
    forgetting = null;
    try {
      reading = await agentThread(id);
    } catch (err) {
      error = String(err);
    }
  }

  async function resumeThread() {
    const thread = reading;
    if (
      starting ||
      !thread?.providerSession ||
      (thread.backendId !== "claude-code" && thread.backendId !== "codex")
    ) {
      return;
    }
    starting = true;
    error = null;
    try {
      activeId = await agents.start(thread.backendId, {
        cwd: thread.cwd,
        resume: thread.providerSession,
        model: thread.model || undefined,
        permissionMode: mode,
      });
      picked = thread.backendId;
      rememberBackend(thread.backendId);
      cwd = thread.cwd;
      model = thread.model;
      reading = null;
      history = null;
    } catch (err) {
      error = String(err);
    } finally {
      starting = false;
    }
  }

  /**
   * Borra un hilo, con confirmación en el propio botón.
   *
   * Un clic solo sería demasiado poco para algo que no se deshace: acá lo que
   * se pierde es la copia de Atic, y aunque el CLI conserve la suya en su
   * propio almacén, esta es la única que la app sabe encontrar.
   */
  async function forget(id: string) {
    if (forgetting !== id) {
      forgetting = id;
      return;
    }
    error = null;
    forgetting = null;
    try {
      await agentThreadDelete(id);
      history = (history ?? []).filter((t) => t.id !== id);
      if (reading?.id === id) reading = null;
    } catch (err) {
      error = String(err);
    }
  }

  async function start() {
    if (starting || !picked || !ready) return;
    starting = true;
    error = null;
    const pendingText = draft.trim();
    const pendingAttachments = [...attachments];
    const pendingOrigin = origin;
    try {
      activeId = await agents.start(picked, {
        cwd: cwd || undefined,
        model: model || undefined,
        effort: effort || undefined,
        fast: supportsFast ? fast : undefined,
        permissionMode: mode,
      });
      // Un solo clic: arrancar y mandar lo que ya está en el compositor.
      // Antes solo abría la sesión y había que volver a pulsar Enviar.
      if (pendingText || pendingAttachments.length > 0) {
        const from =
          pendingAttachments.length > 0
            ? {
                via: pendingOrigin?.via ?? "archivo",
                file: pendingOrigin?.file ?? fileName(pendingAttachments[0]),
                files: [...pendingAttachments],
              }
            : (pendingOrigin ?? undefined);
        try {
          await agents.send(
            activeId,
            pendingText || "Mira esta imagen.",
            from,
          );
          draft = "";
          origin = null;
          attachments = [];
          previewPath = null;
          slashOpen = false;
        } catch (sendErr) {
          draft = pendingText;
          attachments = pendingAttachments;
          origin = pendingOrigin;
          throw sendErr;
        }
      }
    } catch (err) {
      error = String(err);
    } finally {
      starting = false;
    }
  }

  async function send(override?: string) {
    const text = (override ?? draft).trim();
    if ((!text && attachments.length === 0) || !activeId || sendBlocked) return;
    // Un `/model haiku` es de la interfaz, no algo que dictaste: el origen
    // acompaña a lo que escribiste vos, y `override` nunca es eso.
    const from = override
      ? undefined
      : attachments.length > 0
        ? {
            via: origin?.via ?? "archivo",
            file: origin?.file ?? fileName(attachments[0]),
            files: [...attachments],
          }
        : (origin ?? undefined);
    if (!override) {
      draft = "";
      origin = null;
      attachments = [];
      previewPath = null;
    }
    slashOpen = false;
    try {
      await agents.send(activeId, text || "Mira esta imagen.", from);
    } catch (err) {
      error = String(err);
    }
  }

  /**
   * Cambia el modelo sin reiniciar la sesión.
   *
   * Comprobado contra el CLI: `/model haiku` en modo headless responde «Set
   * model to Haiku 4.5 for this session only». Por eso el selector sigue vivo
   * con la sesión abierta en vez de quedar bloqueado hasta la siguiente.
   */
  /**
   * Cambia modelo o esfuerzo en una sesión viva.
   *
   * Por un comando propio y no mandando `/model x` como si lo hubieras escrito:
   * eso funcionaba de casualidad en Claude Code —su CLI lo interpreta— y en
   * Codex habría llegado al modelo como un mensaje más. Cada adaptador sabe
   * cómo se hace en su protocolo; acá solo se pide.
   */
  function resizeComposer() {
    const el = inputEl;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${el.scrollHeight}px`;
  }

  async function switchModel(id: string) {
    const resolved = resolveModelChoice(RAW_MODELS, id);
    model = resolved.modelId;
    if (resolved.effortId) {
      effort = resolved.effortId;
    } else if (EFFORTS.length > 0 && !EFFORTS.some((e) => e.id === effort)) {
      effort =
        RAW_MODELS.find((m) => m.id === resolved.modelId)?.defaultEffort ?? "";
    } else if (EFFORTS.length === 0) {
      effort = "";
    }
    const canFast = !!RAW_MODELS.find((m) => m.id === model)?.supportsFast;
    fast = canFast
      ? rememberedFast(backendForModels, model, RAW_MODELS) || resolved.fast
      : false;
    rememberModel(backendForModels, model);
    if (effort) rememberEffort(backendForModels, model, effort);
    if (canFast) rememberFast(backendForModels, model, fast);
    menu = null;
    if (!activeId || !model) return;
    try {
      await agents.setModel(
        activeId,
        model,
        effort || undefined,
        canFast ? fast : undefined,
      );
    } catch (err) {
      error = String(err);
    }
  }

  async function switchEffort(id: string) {
    effort = id;
    rememberEffort(backendForModels, model, id);
    menu = null;
    if (!activeId || !model) return;
    try {
      await agents.setModel(
        activeId,
        model,
        id,
        supportsFast ? fast : undefined,
      );
    } catch (err) {
      error = String(err);
    }
  }

  async function switchFast(next: boolean) {
    fast = next;
    rememberFast(backendForModels, model, next);
    if (!activeId || !model) return;
    try {
      await agents.setModel(activeId, model, effort || undefined, next);
    } catch (err) {
      error = String(err);
    }
  }

  /** Los comandos que encajan con lo que llevas escrito. */
  const slashQuery = $derived.by(() => {
    const m = /^\/(\S*)$/.exec(draft);
    return m ? m[1].toLowerCase() : null;
  });

  /**
   * Los comandos, con las skills completadas desde el disco.
   *
   * Claude Code ya ofrece las skills como comandos de barra, así que no hacen
   * falta un prefijo ni una lista aparte: hacen falta las descripciones, que el
   * agente no manda y solo están en el `SKILL.md`. Una skill que el agente no
   * llegó a listar se suma igual — de eso se trata poder descubrirlas.
   */
  const commands = $derived.by(() => {
    const list = active?.commands.length
      ? active.commands
      : (agents.catalog[active?.backendId ?? picked] ?? []);
    const bySkill = new Map(agents.skills.map((s) => [s.name, s]));
    const merged = list.map((c) => {
      const skill = bySkill.get(c.name);
      if (!skill) return c;
      bySkill.delete(c.name);
      return c.description ? c : { ...c, description: skill.description };
    });
    const rest = [...bySkill.values()].map((s) => ({
      name: s.name,
      description: s.description,
      argumentHint: "",
    }));
    return [...merged, ...rest];
  });

  const slashHits = $derived.by(() => {
    if (slashQuery === null) return [];
    // Sin sesión abierta se usan los del backend elegido, vistos la última vez.
    // Ofrecer comandos solo con una sesión viva dejaba el primer `/` mudo.
    return commands
      .filter((c) => c.name.toLowerCase().startsWith(slashQuery))
      .slice(0, 8);
  });

  let slashOpen = $state(false);
  let slashPick = $state(0);

  // Se abre sola al escribir `/` y se cierra en cuanto deja de encajar. Que sea
  // derivado y no un estado aparte evita el caso clásico: la lista abierta
  // sobre un texto que ya no es un comando.
  $effect(() => {
    slashOpen = slashHits.length > 0;
    slashPick = 0;
  });

  function takeSlash(name: string) {
    const cmd = commands.find((c) => c.name === name);
    // Con argumento, se deja el cursor listo para escribirlo en vez de enviar:
    // `/model` a secas no hace lo que quien lo eligió esperaba.
    draft = cmd?.argumentHint ? `/${name} ` : `/${name}`;
    slashOpen = false;
    inputEl?.focus();
  }

  async function stop() {
    if (!activeId) return;
    try {
      await agents.stop(activeId);
    } catch (err) {
      error = String(err);
    }
  }

  async function decide(permissionId: string, decision: PermissionDecision) {
    if (!activeId) return;
    try {
      await agents.decide(activeId, permissionId, decision);
    } catch (err) {
      error = String(err);
    }
  }

  function runComposerAction() {
    if (composerState === "awaitingPermission" && singlePending) {
      void decide(singlePending.id, "allow");
    } else if (composerState === "idle") {
      if (active) void send();
      else void start();
    }
  }

  function onKey(event: KeyboardEvent) {
    // Con la lista de comandos abierta, las flechas y Enter son suyas: es lo
    // que espera cualquiera que haya usado un autocompletado.
    if (slashOpen && slashHits.length > 0) {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        slashPick = (slashPick + 1) % slashHits.length;
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        slashPick = (slashPick - 1 + slashHits.length) % slashHits.length;
        return;
      }
      if (event.key === "Tab" || event.key === "Enter") {
        event.preventDefault();
        takeSlash(slashHits[slashPick].name);
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        slashOpen = false;
        return;
      }
    }
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      if (active) {
        if (!sendBlocked) void send();
      } else if (!starting && ready) {
        void start();
      }
    }
  }

  function shortNumber(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${Math.round(n / 1000)}k`;
    return String(n);
  }

</script>

{#snippet modelListFooter()}
  <button
    type="button"
    onclick={() => {
      menu = null;
      modelsConfigOpen = true;
    }}
  >
    Configurar lista…
  </button>
{/snippet}

<!-- Filtro goo solo para el cuello de la burbuja (Liquid UI). No se aplica
     al contenido: el blur+threshold borraría el texto. -->
<svg class="liquid-defs" width="0" height="0" aria-hidden="true" focusable="false">
  <defs>
    <filter id="bub-liquid-goo" x="-50%" y="-50%" width="200%" height="200%">
      <feGaussianBlur in="SourceGraphic" stdDeviation="5" result="blur" />
      <feColorMatrix
        in="blur"
        mode="matrix"
        values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 18 -7"
        result="goo"
      />
      <feComposite in="SourceGraphic" in2="goo" operator="atop" />
    </filter>
  </defs>
</svg>

<div
  class="bub"
  class:is-shown={bubble.shown}
  class:is-loose={bubble.detached}
  data-side={bubble.anchor?.side ?? "top"}
  data-prov={active?.backendId ?? picked}
  style={bubble.vars}
>
  <!-- Cuello líquido hacia la pill: dos blobs fusionados con filtro goo. -->
  <span class="bub-neck" aria-hidden="true">
    <i class="bub-neck-blob is-root"></i>
    <i class="bub-neck-blob is-tip"></i>
  </span>

  <!-- Agarraderas. Viven en el margen de la sombra, del lado opuesto a la
       punta, así que estirar nunca despega el globo de la pill. La de la
       esquina lleva una marca visible: sin ella nadie sabe que se puede. -->
  <button
    type="button"
    class="rz rz-h"
    class:on={rz !== null}
    data-h={bubble.grips.h}
    aria-label="Cambiar el ancho"
    onpointerdown={(e) => startResize(e, "h")}
    onpointermove={moveResize}
    onpointerup={endResize}
    onlostpointercapture={endResize}
  ></button>
  <button
    type="button"
    class="rz rz-v"
    class:on={rz !== null}
    data-v={bubble.grips.v}
    aria-label="Cambiar el alto"
    onpointerdown={(e) => startResize(e, "v")}
    onpointermove={moveResize}
    onpointerup={endResize}
    onlostpointercapture={endResize}
  ></button>
  <button
    type="button"
    class="rz rz-c"
    class:on={rz !== null}
    data-h={bubble.grips.h}
    data-v={bubble.grips.v}
    aria-label="Cambiar el tamaño"
    onpointerdown={(e) => startResize(e, "both")}
    onpointermove={moveResize}
    onpointerup={endResize}
    onlostpointercapture={endResize}
  ></button>

  <div class="bub-body">
    <!-- Sin barra de título: es un modal, no una ventana. Pero como YA NO se
         cierra al perder el foco, tiene que haber una forma visible de
         cerrarlo; si no, la única salida sería recordar el atajo. -->
    <!-- Franja de arrastre. La burbuja nace pegada a la pill, pero eso no
         siempre cae donde te sirve: si tapa lo que estás mirando, se mueve. Al
         moverla se desancla y la punta desaparece, porque ya no sale de ahí. -->
    <div class="grip" data-tauri-drag-region title="Arrastra para mover">
      <span class="grip-bar" data-tauri-drag-region></span>
    </div>

    <!-- Las pestañas SON el selector de agente, no solo un conmutador entre
         sesiones abiertas.
         Antes el agente se elegía en una pastilla del compositor y las pestañas
         aparecían recién con dos sesiones vivas, así que con una sola no había
         nada arriba y la fila entera se materializaba de golpe al abrir la
         segunda. Ahora están siempre y siempre dicen lo mismo: con quién estás
         hablando y con quién más podrías. Tocar una sin sesión la deja elegida;
         tocarla con sesión abierta cambia a ella. -->
    <div class="tabs" role="tablist" aria-label="Agentes" data-tauri-drag-region>
      {#each backends as b (b.id)}
        {@const open = agents.sessions.find((s) => s.backendId === b.id)}
        <button
          type="button"
          role="tab"
          class="tab"
          class:active={open ? open.id === activeId : !active && picked === b.id}
          class:is-off={!b.available}
          aria-selected={open ? open.id === activeId : !active && picked === b.id}
          disabled={!b.available && !open}
          title={b.available ? b.displayName : `${b.displayName} · no instalado`}
          data-tauri-drag-region="false"
          onclick={() => selectBackend(b.id, open?.id ?? null)}
          style="--tv: {ACCENTS[b.id] ?? 'var(--coral)'}"
        >
          <span class="tab-mark"><AgentMark backend={b.id} size={13} /></span>
          {#if open}
            {#if open.pending.length > 0}
              <span class="dot is-wait"></span>
            {:else if open.status === "working"}
              <span class="dot is-busy"></span>
            {:else if open.unread > 0}
              <span class="dot is-new"></span>
            {:else}
              <span class="dot"></span>
            {/if}
          {/if}
          {b.displayName}
        </button>
      {/each}

      <!-- Al final de la fila y no en el compositor: terminar y releer son
           cosas de la SESIÓN, y esta fila pasó a ser la de las sesiones. En el
           compositor competían con los ajustes del mensaje, que es lo que el
           artifact deja ahí y nada más. -->
      <div class="tabs-end" data-tauri-drag-region="false">
        {#if active}
          <button
            type="button"
            class="tab-act"
            onclick={() => void stop()}
            title="Terminar esta sesión"
            aria-label="Terminar esta sesión"
          >
            <svg viewBox="0 0 24 24" width="11" height="11" aria-hidden="true">
              <path
                d="M6 6l12 12M18 6L6 18"
                fill="none"
                stroke="currentColor"
                stroke-width="2.2"
                stroke-linecap="round"
              />
            </svg>
          </button>
        {/if}
        <button
          type="button"
          class="tab-act"
          class:active={history !== null}
          onclick={() => (history === null ? void openHistory() : backFromHistory())}
          title="Conversaciones guardadas"
          aria-label="Conversaciones guardadas"
        >
          <svg
            viewBox="0 0 24 24"
            width="11"
            height="11"
            fill="none"
            stroke="currentColor"
            stroke-width="1.9"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <path d="M12 3a9 9 0 1 1-8.5 12 M12 7v5l3.5 2 M3 3v5h5" />
          </svg>
        </button>

        <!-- Cerrar, al final de la misma fila. Estaba en posición absoluta
             sobre la esquina y desde que la fila de pestañas ocupa todo el
             ancho se pisaban: dos controles en el mismo pixel, y el de arriba
             ganaba el clic. En el flujo no pueden chocar. -->
        <button
          type="button"
          class="tab-act"
          onclick={() => void close()}
          aria-label="Cerrar (Esc)"
          title="Cerrar · Esc"
        >
          <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true">
            <path
              d="M6 6l12 12M18 6L6 18"
              fill="none"
              stroke="currentColor"
              stroke-width="2.2"
              stroke-linecap="round"
            />
          </svg>
        </button>
      </div>
    </div>

    <div class="log" bind:this={logEl} role="log">
      {#if history !== null && !reading}
        <!-- El historial. Se guardaba desde la fase 0 y no había forma de
             mirarlo sin abrir el `atic.db3` a mano. -->
        <div class="hist-head">
          <div class="hist-title-row">
            <h2 class="hist-h">Conversaciones</h2>
            {#if history.length > 0}
              <span class="hist-count"
                >{histQuery.trim()
                  ? `${historyVisible.length} de ${history.length}`
                  : history.length}</span
              >
            {/if}
          </div>
          {#if history.length > 0}
            <input
              class="hist-search"
              type="search"
              placeholder="Buscar por texto, carpeta o agente…"
              bind:value={histQuery}
              aria-label="Filtrar conversaciones"
              spellcheck="false"
              autocomplete="off"
            />
          {/if}
        </div>
        {#if histLoading}
          <ul class="hist-list" aria-busy="true" aria-label="Cargando conversaciones">
            {#each [1, 2, 3, 4] as n (n)}
              <li class="hist hist-skel" aria-hidden="true">
                <div class="hist-skel-line is-meta"></div>
                <div class="hist-skel-line is-preview"></div>
                <div class="hist-skel-line is-cwd"></div>
              </li>
            {/each}
          </ul>
        {:else if history.length === 0}
          <div class="hist-empty">
            <p class="hist-empty-t">Todavía no hay conversaciones</p>
            <p class="hist-empty-d">
              Se guardan solas al terminar cada turno. Abrí una sesión, hablá, y
              volverán a aparecer acá.
            </p>
          </div>
        {:else if historyVisible.length === 0}
          <p class="hist-empty-d">Ninguna coincide con «{histQuery.trim()}».</p>
        {:else}
          <ul class="hist-list" aria-label="Conversaciones guardadas">
            {#each historyVisible as t (t.id)}
              {@const resumable =
                !!t.providerSession &&
                (t.backendId === "claude-code" || t.backendId === "codex")}
              {@const readOnly = !!t.providerSession && !resumable}
              <li class="hist">
                <button
                  type="button"
                  class="hist-o"
                  onclick={() => void readThread(t.id)}
                >
                  <span class="hist-top">
                    <span
                      class="hist-mark"
                      style="--tv: {ACCENTS[t.backendId] ?? 'var(--coral)'}"
                      aria-hidden="true"
                    >
                      <AgentMark backend={t.backendId} size={12} />
                    </span>
                    <span
                      class="hist-who"
                      style="color: {ACCENTS[t.backendId] ?? 'var(--dim)'}"
                      >{t.backendName}</span
                    >
                    {#if resumable}
                      <span class="hist-badge is-resume">Reanudable</span>
                    {:else if readOnly}
                      <span class="hist-badge">Solo lectura</span>
                    {/if}
                    <span class="hist-when">{whenList(t.updatedAt)}</span>
                  </span>
                  <span class="hist-p">{t.preview || "Sin texto todavía"}</span>
                  <span class="hist-cwd" title={t.cwd}>{folderTail(t.cwd)}</span>
                </button>
                <button
                  type="button"
                  class="hist-x"
                  class:is-sure={forgetting === t.id}
                  onclick={() => void forget(t.id)}
                  aria-label={forgetting === t.id
                    ? "Confirmar borrado"
                    : `Borrar conversación de ${t.backendName}`}
                  title={forgetting === t.id ? "¿Seguro?" : "Borrar"}
                >
                  {#if forgetting === t.id}
                    ¿Seguro?
                  {:else}
                    <svg
                      viewBox="0 0 24 24"
                      width="12"
                      height="12"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="1.8"
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      aria-hidden="true"
                    >
                      <path d="M5 7h14 M9 7V5h6v2 M7 7l1 13h8l1-13" />
                    </svg>
                  {/if}
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      {:else if !active && !reading}
        <section class="empty">
          <span class="empty-mark" class:is-off={!ready}>
            <AgentMark backend={picked} size={48} />
          </span>
          <p>¿En qué trabajamos?</p>
        </section>

        {#if backends.length > 0 && !ready}
          <p class="warn">
            No se encontró el ejecutable. Instálalo y ábrelo una vez en la
            consola para iniciar sesión; Atic usa esa misma cuenta.
          </p>
        {/if}
      {:else}
        <!-- Que es guardada tiene que decirse: si no, una conversación vieja
             se lee igual que una viva y uno le escribe esperando respuesta. -->
        {#if reading}
          <p class="hist-r">
            Guardada · {reading.backendName} · {when(reading.updatedAt)}
          </p>
        {/if}
        <AgentConversation {items} {turnEnds} />

        {#if active?.status === "working" && !writing}
          <p class="meta live">trabajando…</p>
        {/if}
      {/if}
    </div>

    <!-- El permiso va pegado al compositor: ahí están los ojos mientras el
         agente trabaja, y es una decisión, no una línea más del registro. -->
    {#each active?.pending ?? [] as p (p.id)}
      <div class="perm" role="alertdialog" aria-label="Permiso pendiente">
        <div class="perm-copy">
          <p class="perm-t">Quiere usar <strong>{p.tool}</strong></p>
          <p class="perm-w">{p.description}</p>
        </div>
        <div class="perm-acts">
          <button type="button" class="btn" onclick={() => void decide(p.id, "deny")}>
            Denegar
          </button>
          <!-- «Siempre» graba la regla que sugiere el propio agente, por esta
               sesión. Sin este botón, la única salida a contestar veinte veces
               lo mismo es arrancar en `acceptEdits`, que renuncia a preguntar
               por todo lo demás. -->
          <button
            type="button"
            class="btn"
            onclick={() => void decide(p.id, "allowAlways")}
          >
            Siempre
          </button>
          <button
            type="button"
            class="btn is-go"
            onclick={() => void decide(p.id, "allow")}
          >
            Permitir
          </button>
        </div>
      </div>
    {/each}

    {#if error || active?.error}
      <p class="warn warn-bar" role="alert">{error ?? active?.error}</p>
    {/if}

    {#if active && history === null}
      <div
        class="usage"
        title="Contexto: {shortNumber(active.contextTokens)} de {shortNumber(
          active.contextSize || CONTEXT_WINDOW,
        )} tokens"
      >
        <div class="usage-meta">
          <span>
            {shortNumber(active.contextTokens)} / {shortNumber(
              active.contextSize || CONTEXT_WINDOW,
            )}
            · {Math.round(ctxPct)}%
          </span>
          {#if active.costUsd > 0}
            <span class="usage-cost">${active.costUsd.toFixed(3)}</span>
          {/if}
        </div>
        <div class="usage-track" aria-hidden="true">
          <div
            class="usage-fill"
            style="transform: scaleX({Math.max(0, Math.min(1, ctxPct / 100))})"
          ></div>
        </div>
      </div>
    {/if}

    <div class="cmp">
      <!-- Comandos del agente, con su descripción. La lista la da él mismo por
           el canal de control, así que las skills y los plugins que tengas
           instalados aparecen sin que Atic sepa nada de ellos. -->
      {#if slashOpen && slashHits.length > 0}
        <ul class="slash" role="listbox" aria-label="Comandos">
          {#each slashHits as c, i (c.name)}
            <li>
              <button
                type="button"
                role="option"
                aria-selected={i === slashPick}
                class="slash-o"
                class:active={i === slashPick}
                onclick={() => takeSlash(c.name)}
              >
                <span class="slash-n">
                  /{c.name}{c.argumentHint ? ` ${c.argumentHint}` : ""}
                </span>
                <span class="slash-d">{c.description}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}

      <div
        class="cmp-box"
        ondragover={onComposerDragOver}
        ondrop={(e) => void onComposerDrop(e)}
        role="presentation"
      >
        <!-- Mirando el historial no hay a quién escribirle: el compositor se va
             entero en vez de quedarse desactivado ofreciendo algo que no pasa. -->
        {#if history === null}
          {#if attachments.length > 0}
            <ul class="att-list" aria-label="Imágenes adjuntas">
              {#each attachments as path (path)}
                <li class="att">
                  <button
                    type="button"
                    class="att-thumb"
                    onclick={() => (previewPath = path)}
                    title={fileName(path)}
                    aria-label="Ver {fileName(path)}"
                  >
                    <img src={convertFileSrc(path)} alt={fileName(path)} />
                  </button>
                  <button
                    type="button"
                    class="att-x"
                    onclick={(e) => {
                      e.stopPropagation();
                      removeAttachment(path);
                    }}
                    aria-label="Quitar {fileName(path)}"
                    title="Quitar"
                  >
                    ×
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
          <textarea
            class="cmp-in"
            bind:this={inputEl}
            bind:value={draft}
            onkeydown={onKey}
            oninput={resizeComposer}
            onpaste={() => {
              origin = { via: "portapapeles" };
              void tick().then(resizeComposer);
            }}
            rows="1"
            placeholder={active
              ? "Escribe, o dicta con Ctrl+Shift+D…"
              : "Describe lo que quieres y Enter para empezar…"}
            aria-label="Mensaje para el agente"
          ></textarea>
        {/if}

        <!-- Dos grupos y no una fila suelta: los ajustes se comprimen y, si
             aun así no caben, bajan de línea; las acciones no se encogen nunca.
             Con un solo flex sin `wrap`, a 580px el anillo salía cortado y el
             botón de enviar quedaba fuera del panel — el control más importante
             de los tres era el único que no se veía. -->
        <div class="cmp-row">
          <div class="cmp-set">
            {#if history !== null}
              <button type="button" class="chip" onclick={backFromHistory}>
                {reading ? "Volver a la lista" : "Cerrar historial"}
              </button>
              {#if reading}
                {@const rid = reading.id}
                {#if reading.providerSession &&
                  (reading.backendId === "claude-code" || reading.backendId === "codex")}
                  <button
                    type="button"
                    class="chip is-resume"
                    onclick={() => void resumeThread()}
                    disabled={starting}
                  >
                    Continuar
                  </button>
                {:else if reading.providerSession}
                  <span
                    class="chip is-static"
                    title="OpenCode y Cursor todavía no reanudan desde Atic"
                  >
                    Solo lectura
                  </span>
                {/if}
                <button
                  type="button"
                  class="chip"
                  class:is-sure={forgetting === rid}
                  onclick={() => void forget(rid)}
                >
                  {forgetting === rid ? "¿Seguro?" : "Borrar"}
                </button>
                <span class="chip is-static">{reading.cwd}</span>
              {/if}
            {:else if !active}
              <!-- El agente ya lo dicen las pestañas de arriba; acá quedan solo
                   los ajustes que describen la sesión que vas a abrir. -->
              {#if modelsLoading || MODELS.length > 0}
                <PickerMenu
                  label={modelLabel}
                  open={menu === "model"}
                  options={DISPLAY_MODELS}
                  value={model}
                  loading={modelsLoading && MODELS.length === 0}
                  loadingMessage="Consultando modelos del proveedor…"
                  footer={isFilterableBackend(backendForModels)
                    ? modelListFooter
                    : undefined}
                  onToggle={() => (menu = menu === "model" ? null : "model")}
                  onPick={(id) => {
                    const resolved = resolveModelChoice(RAW_MODELS, id);
                    model = resolved.modelId;
                    effort =
                      resolved.effortId ||
                      rememberedEffort(picked, resolved.modelId, RAW_MODELS);
                    const canFast = !!RAW_MODELS.find(
                      (m) => m.id === model,
                    )?.supportsFast;
                    fast = canFast
                      ? rememberedFast(picked, model, RAW_MODELS) ||
                        resolved.fast
                      : false;
                    rememberModel(picked, model);
                    if (effort) rememberEffort(picked, model, effort);
                    if (canFast) rememberFast(picked, model, fast);
                    menu = null;
                  }}
                />
              {/if}
              {#if EFFORTS.length > 0}
                <PickerMenu
                  label={effortLabel}
                  open={menu === "effort"}
                  options={EFFORTS}
                  value={effort}
                  onToggle={() => (menu = menu === "effort" ? null : "effort")}
                  onPick={(id) => {
                    effort = id;
                    rememberEffort(picked, model, id);
                    menu = null;
                  }}
                />
              {/if}
              {#if supportsFast}
                <button
                  type="button"
                  class="chip"
                  class:is-on={fast}
                  onclick={() => {
                    fast = !fast;
                    rememberFast(picked, model, fast);
                  }}
                  aria-pressed={fast}
                  title={fast ? "Fast activo" : "Fast desactivado"}
                >
                  Fast
                </button>
              {/if}
              <div class="mode-chip" class:is-risk={mode === "bypassPermissions"}>
                <PickerMenu
                  label={modeLabel}
                  iconOnly
                  title={modeLabel}
                  ariaLabel={modeLabel}
                  open={menu === "mode"}
                  options={MODES}
                  value={mode}
                  onToggle={() => (menu = menu === "mode" ? null : "mode")}
                  onPick={(id) => selectMode(id)}
                >
                  {#snippet icon()}
                    <AgentIcons name={SHIELDS[mode] ?? "shield-manual"} />
                  {/snippet}
                </PickerMenu>
              </div>
              <button
                type="button"
                class="chip is-icon"
                onclick={() => void pickFolder()}
                title={cwd || "Carpeta"}
                aria-label={cwd || "Carpeta"}
              >
                <AgentIcons name="folder" />
              </button>
              <button
                type="button"
                class="chip is-icon"
                onclick={openTools}
                title="MCP y skills"
                aria-label="MCP y skills"
              >
                <AgentIcons name="mcp" />
              </button>
            {:else}
              <!-- El modelo sigue vivo con la sesión abierta: `/model <alias>`
                   lo cambia sin reiniciar. -->
              {#if modelsLoading || MODELS.length > 0}
                <PickerMenu
                  label={modelLabel}
                  open={menu === "model"}
                  options={DISPLAY_MODELS.filter((m) => m.id)}
                  value={model}
                  loading={modelsLoading && MODELS.length === 0}
                  loadingMessage="Consultando modelos del proveedor…"
                  footer={isFilterableBackend(backendForModels)
                    ? modelListFooter
                    : undefined}
                  onToggle={() => (menu = menu === "model" ? null : "model")}
                  onPick={(id) => void switchModel(id)}
                />
              {/if}
              <!-- Cuánto piensa. Solo aparece si ESTE modelo lo acepta: no
                   todos los tienen, y un selector con una sola opción es
                   ruido. -->
              {#if EFFORTS.length > 0}
                <PickerMenu
                  label={effortLabel}
                  open={menu === "effort"}
                  options={EFFORTS}
                  value={effort}
                  onToggle={() => (menu = menu === "effort" ? null : "effort")}
                  onPick={(id) => void switchEffort(id)}
                />
              {/if}
              {#if supportsFast}
                <button
                  type="button"
                  class="chip"
                  class:is-on={fast}
                  onclick={() => void switchFast(!fast)}
                  aria-pressed={fast}
                  title={fast ? "Fast activo" : "Fast desactivado"}
                >
                  Fast
                </button>
              {/if}
              <span
                class="chip is-static is-icon"
                class:is-risk={mode === "bypassPermissions"}
                title={modeLabel}
              >
                <AgentIcons name={SHIELDS[mode] ?? "shield-manual"} />
              </span>
              <!-- La carpeta: el ícono basta; la ruta completa va en el title. -->
              <span class="chip is-static is-icon" title={active.cwd}>
                <AgentIcons name="folder" />
              </span>
              <button
                type="button"
                class="chip is-icon"
                onclick={openTools}
                title="MCP y skills"
                aria-label="MCP y skills"
              >
                <AgentIcons name="mcp" />
              </button>
            {/if}
          </div>

          {#if history === null}
            <div class="cmp-acts">
              <div class="plus-wrap">
                {#if menu === "plus"}
                  <ul class="plus-menu" role="menu" aria-label="Agregar al mensaje">
                    <li>
                      <button
                        type="button"
                        role="menuitem"
                        onclick={() => {
                          menu = null;
                          void capture();
                        }}
                      >
                        Capturar imagen
                      </button>
                    </li>
                    <li>
                      <button
                        type="button"
                        role="menuitem"
                        onclick={() => {
                          menu = null;
                          void attach();
                        }}
                      >
                        Adjuntar archivo
                      </button>
                    </li>
                    <li>
                      <button
                        type="button"
                        role="menuitem"
                        disabled={!!active}
                        title={active ? "Se elige al iniciar una sesión" : undefined}
                        onclick={() => selectMode("plan")}
                      >
                        Modo plan
                      </button>
                    </li>
                    {#if EFFORTS.length > 0}
                      <li class="plus-label" role="presentation">Esfuerzo</li>
                      {#each EFFORTS as option (option.id)}
                        <li>
                          <button
                            type="button"
                            role="menuitem"
                            class:active={option.id === effort}
                            onclick={() =>
                              active
                                ? void switchEffort(option.id)
                                : (rememberEffort(picked, model, option.id),
                                  (effort = option.id),
                                  (menu = null))}
                          >
                            {option.label}
                          </button>
                        </li>
                      {/each}
                    {/if}
                  </ul>
                {/if}
                <button
                  type="button"
                  class="chip is-icon plus"
                  class:is-open={menu === "plus"}
                  onclick={() => (menu = menu === "plus" ? null : "plus")}
                  aria-haspopup="menu"
                  aria-expanded={menu === "plus"}
                  aria-label="Agregar"
                  title="Agregar"
                >
                  <span aria-hidden="true">+</span>
                </button>
              </div>

              <!-- Dictar al agente. Es lo que ninguna GUI de agentes tiene, y
                   acá sale gratis: el dictado ya pega en el control con el foco,
                   así que enfocar el compositor antes es todo lo que hace falta. -->
              <button
                type="button"
                class="chip is-icon"
                onclick={() => void dictate()}
                title="Dictar · Ctrl+Shift+D"
                aria-label="Dictar"
              >
                <AgentIcons name="mic" size={12} />
              </button>

              <button
                type="button"
                class="go"
                class:is-stop={composerState === "streaming"}
                class:is-approve={composerState === "awaitingPermission"}
                onclick={runComposerAction}
                disabled={composerDisabled}
                aria-label={composerActionAria}
                title={composerActionLabel}
              >
                <span class="sr-only">{composerActionLabel}</span>
                {#if composerState === "streaming"}
                  <svg viewBox="0 0 24 24" width="13" height="13" aria-hidden="true">
                    <rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor" />
                  </svg>
                {:else if composerState === "awaitingPermission"}
                  <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true">
                    <path
                      d="m5 12 4 4L19 6"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="2"
                      stroke-linecap="round"
                      stroke-linejoin="round"
                    />
                  </svg>
                {:else}
                  <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true">
                    <path
                      d="M12 19V5M5 12l7-7 7 7"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="2"
                      stroke-linecap="round"
                      stroke-linejoin="round"
                    />
                  </svg>
                {/if}
              </button>
            </div>
          {/if}
        </div>
      </div>
    </div>
  </div>
</div>

{#if previewPath}
  <div
    class="att-preview"
    role="dialog"
    aria-modal="true"
    aria-label="Vista previa"
    tabindex="-1"
  >
    <button
      type="button"
      class="att-preview-backdrop"
      aria-label="Cerrar vista previa"
      onclick={() => (previewPath = null)}
    ></button>
    <button
      type="button"
      class="att-preview-close"
      aria-label="Cerrar vista previa"
      onclick={() => (previewPath = null)}
    >
      ×
    </button>
    <button
      type="button"
      class="att-preview-img-btn"
      aria-label="Cerrar vista previa"
      onclick={() => (previewPath = null)}
    >
      <img
        class="att-preview-img"
        src={convertFileSrc(previewPath)}
        alt={fileName(previewPath)}
      />
    </button>
  </div>
{/if}

{#if toolsOpen}
  <AgentToolsModal
    mcpServers={active?.mcpServers ?? []}
    skills={agents.skills}
    hasSession={!!active}
    onClose={() => (toolsOpen = false)}
  />
{/if}

{#if modelsConfigOpen}
  <AgentModelsModal
    backendId={backendForModels}
    backendLabel={modelsBackendLabel}
    models={MODELS}
    onSave={(ids) => {
      setVisibleModelIds(backendForModels, ids);
      modelFilterTick += 1;
      modelsConfigOpen = false;
    }}
    onClose={() => (modelsConfigOpen = false)}
  />
{/if}

<style>
  :global(html),
  :global(body) {
    margin: 0;
    height: 100%;
    background: transparent;
    overflow: hidden;
  }

  /* ─── La burbuja ────────────────────────────────────────────────────────
   *
   * El marco transparente (`--inset`) NO es decorativo: es el sitio donde la
   * sombra tiene que caber. Antes había 14 px de hueco y una sombra de 48 px de
   * difuminado, así que la ventana la cortaba en seco y lo que se veía era una
   * banda oscura con bordes rectos alrededor del globo — la «sombra fea».
   *
   * La regla es: desplazamiento + difuminado <= inset. Cambiar una obliga a
   * mirar la otra, y Rust usa el mismo número para descontarlo al colocarla.
   */
  .bub {
    --inset: 62px;
    --coral: #da7756;
    --shell: #1c1917;
    --line: #332e2b;
    --text: #e7e2dd;
    --dim: #8d827a;
    --faint: #6b615a;
    /* Superficies y señales. Estaban escritas a mano en cada componente; acá
       se declaran una vez y bajan por herencia, que es de donde
       `AgentConversation` y `AgentToolCard` ya decían sacarlas. */
    --card: var(--card);
    --code: var(--code);
    --hover: var(--hover);
    --add: var(--add);
    --del: var(--del);
    /* Ámbar: «esto espera tu decisión».
     *
     * Deliberadamente FUERA del acento. El acento cambia con el agente, así
     * que un permiso pintado con él sería verde en OpenCode y violeta en
     * Cursor, y dejaría de reconocerse de un vistazo. Esto tiene que
     * significar lo mismo en los cuatro. */
    --wait: #d4a24c;
    --wait-text: #d9bd85;

    /* Tamaño FIJO, no `100vw/100vh`.
     *
     * La ventana arranca del porte de la pill y crece: con medidas relativas,
     * el contenido se recompondría en cada frame del vuelo —la barra de abajo
     * plegándose, el texto reflowing— y lo que se vería es una interfaz
     * histérica, no un globo desplegándose. Fijo, simplemente se va
     * descubriendo a medida que la ventana lo destapa.
     *
     * El tamaño lo dicta Rust (`--w`/`--h`), no una constante escrita acá: es
     * quien sabe el rectángulo real de la ventana, y a escalas distintas de
     * 100% ese número no coincide con el declarado en la config.
     */
    position: absolute;
    display: flex;
    width: var(--w);
    height: var(--h);
    box-sizing: border-box;
    padding: var(--inset);

    /* Lo único que anima acá es la opacidad: quien crece es la VENTANA, con el
       mismo tween que la rueda. Escalar además el contenido sería animar dos
       veces la misma idea y se notaría como un rebote doble. */
    opacity: 0;
    transition: opacity var(--fade, 200ms) ease;
  }
  .bub.is-shown {
    opacity: 1;
  }

  /* El acento sigue al proveedor: es la única pieza de color que cambia entre
     los cuatro agentes, y por eso es la prueba visible de que abajo hay un solo
     modelo. Sin sesión abierta manda el que elegiste para arrancar, así que el
     globo ya se pinta del color del agente antes del primer mensaje. */
  .bub[data-prov="opencode"] {
    --coral: #7fae86;
  }
  .bub[data-prov="codex"] {
    --coral: #8fa9b8;
  }
  .bub[data-prov="cursor"] {
    --coral: #a88fc4;
  }

  /* Anclado al borde por el que sale, y centrado en el otro eje: durante el
     vuelo la ventana está centrada en la pill, así que la punta se queda sobre
     ella todo el recorrido. Con `margin` y no `transform`, que acá no hay
     ninguno que pisar. */
  .bub[data-side="top"] {
    top: 0;
    left: 50%;
    margin-left: calc(var(--w) / -2);
  }
  .bub[data-side="bottom"] {
    bottom: 0;
    left: 50%;
    margin-left: calc(var(--w) / -2);
  }
  .bub[data-side="left"] {
    top: 50%;
    left: 0;
    margin-top: calc(var(--h) / -2);
  }
  .bub[data-side="right"] {
    top: 50%;
    right: 0;
    margin-top: calc(var(--h) / -2);
  }

  /* Movida de sitio ya no sale de la pill: la punta sobra. */
  .bub.is-loose .bub-neck {
    display: none;
  }

  /* ─── Agarraderas ─────────────────────────────────────────────────────
   *
   * Viven DENTRO del margen de la sombra, así que no le quitan sitio al globo
   * ni pisan la franja de arrastre o el botón de cerrar. Son invisibles hasta
   * que las buscás; la de la esquina no, porque si nada indica que se puede
   * estirar, nadie lo intenta. */
  .rz {
    position: absolute;
    z-index: 6;
    border: 0;
    border-radius: 3px;
    background: transparent;
    padding: 0;
    transition: background-color 120ms ease;
  }
  .rz:hover,
  .rz.on {
    background: color-mix(in srgb, var(--coral) 55%, transparent);
  }
  .rz:focus-visible {
    outline: 2px solid var(--coral);
    outline-offset: 1px;
  }

  /* Barra vertical, en el borde izquierdo o derecho según de qué lado salga. */
  .rz-h {
    top: var(--inset);
    bottom: var(--inset);
    width: 15px;
    cursor: ew-resize;
  }
  .rz-h[data-h="right"] {
    right: calc(var(--inset) - 15px);
  }
  .rz-h[data-h="left"] {
    left: calc(var(--inset) - 15px);
  }

  .rz-v {
    right: var(--inset);
    left: var(--inset);
    height: 15px;
    cursor: ns-resize;
  }
  .rz-v[data-v="bottom"] {
    bottom: calc(var(--inset) - 15px);
  }
  .rz-v[data-v="top"] {
    top: calc(var(--inset) - 15px);
  }

  .rz-c {
    width: 22px;
    height: 22px;
  }
  .rz-c[data-h="right"] {
    right: calc(var(--inset) - 14px);
  }
  .rz-c[data-h="left"] {
    left: calc(var(--inset) - 14px);
  }
  .rz-c[data-v="bottom"] {
    bottom: calc(var(--inset) - 14px);
  }
  .rz-c[data-v="top"] {
    top: calc(var(--inset) - 14px);
  }
  .rz-c[data-h="right"][data-v="bottom"],
  .rz-c[data-h="left"][data-v="top"] {
    cursor: nwse-resize;
  }
  .rz-c[data-h="right"][data-v="top"],
  .rz-c[data-h="left"][data-v="bottom"] {
    cursor: nesw-resize;
  }

  /* La marca de esquina: dos trazos, siempre visibles. */
  .rz-c::after {
    position: absolute;
    width: 8px;
    height: 8px;
    border-color: var(--faint);
    content: "";
  }
  .rz-c[data-h="right"]::after {
    right: 4px;
    border-right: 2px solid;
  }
  .rz-c[data-h="left"]::after {
    left: 4px;
    border-left: 2px solid;
  }
  .rz-c[data-v="bottom"]::after {
    bottom: 4px;
    border-bottom: 2px solid;
  }
  .rz-c[data-v="top"]::after {
    top: 4px;
    border-top: 2px solid;
  }
  .rz-c:hover::after,
  .rz-c.on::after {
    border-color: #fff;
  }

  .bub-body {
    position: relative;
    z-index: 1;
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    border: 1px solid var(--line);
    /* 26 y no 18: es el mismo número que `BUBBLE_CORNER` en Rust, que lo usa
       para que la punta no caiga sobre la curva. Estaban desfasados. */
    border-radius: 26px;
    background: var(--shell);
    /* Cabe entera en `--inset` (18 + 44 = 62 <= 62), así que la ventana no la
       recorta y el globo queda flotando de verdad. */
    box-shadow: 0 18px 44px rgb(0 0 0 / 42%);
    color: var(--text);
    overflow: hidden;
  }

  .liquid-defs {
    position: absolute;
    width: 0;
    height: 0;
    overflow: hidden;
  }

  /* Cuello líquido hacia la pill: dos blobs + filtro goo (estilo Liquid UI).
     Solo envuelve formas vacías, nunca el texto del globo. */
  .bub-neck {
    position: absolute;
    z-index: 0;
    display: grid;
    place-items: center;
    width: 28px;
    height: 36px;
    filter: url(#bub-liquid-goo);
    pointer-events: none;
  }
  .bub-neck-blob {
    display: block;
    border-radius: 999px;
    background: var(--shell);
    box-shadow: 0 0 0 1px var(--line);
  }
  .bub-neck-blob.is-root {
    width: 22px;
    height: 18px;
  }
  .bub-neck-blob.is-tip {
    width: 14px;
    height: 14px;
    margin-top: -6px;
  }
  .bub[data-side="top"] .bub-neck {
    top: calc(var(--inset) - 28px);
    left: calc(var(--tail) - 14px);
  }
  .bub[data-side="bottom"] .bub-neck {
    bottom: calc(var(--inset) - 28px);
    left: calc(var(--tail) - 14px);
    transform: rotate(180deg);
  }
  .bub[data-side="left"] .bub-neck {
    top: calc(var(--tail) - 18px);
    left: calc(var(--inset) - 28px);
    width: 36px;
    height: 28px;
    transform: rotate(-90deg);
  }
  .bub[data-side="right"] .bub-neck {
    top: calc(var(--tail) - 18px);
    right: calc(var(--inset) - 28px);
    width: 36px;
    height: 28px;
    transform: rotate(90deg);
  }

  @media (prefers-reduced-motion: reduce) {
    .bub-neck {
      filter: none;
    }
    .bub-neck-blob.is-tip {
      margin-top: -4px;
    }
  }

  /* ─── Sesiones ──────────────────────────────────────────────────────── */
  /* Subrayado y no pastilla: la pestaña activa se marca con una línea del
     acento del agente, así el color dice con quién hablás antes de leer el
     nombre. Una pastilla con fondo compite con las del compositor, que son
     otra cosa. */
  .tabs {
    display: flex;
    min-width: 0;
    flex-shrink: 0;
    gap: 0.1rem;
    border-bottom: 1px solid var(--line);
    padding: 0 0.7rem;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .tab {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    border: 0;
    border-bottom: 2px solid transparent;
    padding: 0.3rem 0.5rem 0.35rem;
    background: transparent;
    color: var(--dim);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.6875rem;
    white-space: nowrap;
    cursor: pointer;
    transition:
      color 120ms ease,
      border-color 120ms ease;
  }
  .tab:hover:not(:disabled) {
    color: var(--text);
  }
  .tab.active {
    border-bottom-color: var(--tv);
    color: var(--text);
  }
  /* La marca lleva el acento SIEMPRE, también sin estar activa: es lo que
     permite reconocer al agente de un vistazo en la fila. */
  .tab-mark {
    display: inline-flex;
    flex-shrink: 0;
    align-items: center;
    color: var(--tv);
  }
  /* No instalado: se muestra igual, apagado. Ocultarlo dejaría la duda de si
     Atic no lo soporta o si falta instalarlo. */
  .tab.is-off {
    cursor: default;
    opacity: 0.38;
  }

  /* Empujadas al extremo: son de la fila, no de ninguna pestaña. */
  .tabs-end {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    margin-left: auto;
    gap: 0.1rem;
  }
  .tab-act {
    display: inline-flex;
    /* 24px de área táctil sobre un ícono de 11: en una barra tan angosta,
       apuntarle a 11px es pelearse con el ratón. */
    width: 1.5rem;
    height: 1.5rem;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--faint);
    cursor: pointer;
    transition:
      color 120ms ease,
      background-color 120ms ease;
  }
  .tab-act:hover {
    background: var(--hover);
    color: var(--text);
  }
  .tab-act:active {
    scale: 0.96;
  }
  .tab-act.active {
    color: var(--coral);
  }

  .dot {
    width: 0.35rem;
    height: 0.35rem;
    border-radius: 999px;
    background: var(--faint);
  }
  /* Esperando tu decisión: ámbar, el mismo del permiso, y no el acento. */
  .dot.is-wait {
    background: var(--wait);
  }
  /* Trabajando: late en el acento. El latido es la señal; en gris se leía
     como apagado. */
  .dot.is-busy {
    background: var(--coral);
    animation: pulse 1.6s ease-in-out infinite;
  }
  .dot.is-new {
    background: var(--add);
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 0.35;
    }
    50% {
      opacity: 1;
    }
  }

  /* ─── Registro ──────────────────────────────────────────────────────── */
  .log {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.6rem;
    /* Arriba, sitio para la franja de arrastre y el botón de cerrar. */
    padding: 1.6rem 0.95rem 0.9rem;
    overflow: auto;
  }

  .empty {
    display: flex;
    flex: 1;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    color: var(--coral);
    gap: 0.7rem;
    text-align: center;
  }
  .empty-mark {
    display: inline-flex;
    line-height: 0;
  }
  .empty-mark.is-off {
    color: var(--faint);
  }
  .empty p {
    margin: 0;
    color: var(--text);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.875rem;
  }

  .meta {
    margin: 0;
    color: var(--faint);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.6875rem;
  }
  .live {
    animation: pulse 1.6s ease-in-out infinite;
  }

  /* ─── Historial ─────────────────────────────────────────────────────
     Lista densa, no tarjetas: el preview manda; agente, hora y carpeta
     orientan. Búsqueda arriba cuando la lista crece. */
  .hist-head {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    margin-bottom: 0.35rem;
  }

  .hist-title-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .hist-h {
    margin: 0;
    color: var(--text);
    font-family: inherit;
    font-size: 0.8125rem;
    font-weight: 600;
    letter-spacing: 0;
    text-transform: none;
    text-wrap: balance;
  }

  .hist-count {
    color: var(--faint);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.625rem;
    font-variant-numeric: tabular-nums;
  }

  .hist-search {
    box-sizing: border-box;
    width: 100%;
    border: 1px solid var(--line);
    border-radius: 0.45rem;
    padding: 0.35rem 0.5rem;
    background: #1c1918;
    color: var(--text);
    font-family: inherit;
    font-size: 0.6875rem;
    outline: none;
  }
  .hist-search::placeholder {
    color: var(--faint);
  }
  .hist-search:focus-visible {
    border-color: color-mix(in srgb, var(--coral) 55%, var(--line));
  }

  .hist-list {
    display: flex;
    margin: 0;
    flex-direction: column;
    gap: 0;
    padding: 0;
    list-style: none;
  }

  .hist {
    display: flex;
    align-items: stretch;
    border-bottom: 1px solid var(--line);
    background: transparent;
    gap: 0;
  }
  .hist:last-child {
    border-bottom: 0;
  }
  .hist:hover {
    background: color-mix(in srgb, var(--hover) 70%, transparent);
  }

  .hist-o {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    border: 0;
    padding: 0.55rem 0.35rem 0.55rem 0.15rem;
    background: transparent;
    color: var(--text);
    font-family: inherit;
    gap: 0.2rem;
    text-align: left;
    cursor: pointer;
  }
  .hist-o:focus-visible {
    outline: 2px solid var(--coral);
    outline-offset: -2px;
    border-radius: 0.35rem;
  }

  .hist-top {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    color: var(--dim);
    font-size: 0.625rem;
  }
  .hist-mark {
    display: inline-flex;
    flex-shrink: 0;
    line-height: 0;
    color: var(--tv, var(--dim));
  }
  .hist-who {
    min-width: 0;
    overflow: hidden;
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hist-badge {
    flex-shrink: 0;
    border: 1px solid var(--line);
    border-radius: 999px;
    padding: 0.05rem 0.35rem;
    color: var(--faint);
    font-size: 0.5625rem;
    line-height: 1.3;
  }
  .hist-badge.is-resume {
    border-color: color-mix(in srgb, var(--coral) 45%, var(--line));
    color: var(--coral);
  }
  .hist-when {
    margin-left: auto;
    flex-shrink: 0;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }

  .hist-skel {
    flex-direction: column;
    gap: 0.35rem;
    padding: 0.55rem 0.15rem;
    pointer-events: none;
  }
  .hist-skel-line {
    border-radius: 0.25rem;
    background: color-mix(in srgb, var(--line) 80%, transparent);
    opacity: 0.7;
  }
  .hist-skel-line.is-meta {
    width: 42%;
    height: 0.45rem;
  }
  .hist-skel-line.is-preview {
    width: 88%;
    height: 0.7rem;
  }
  .hist-skel-line.is-cwd {
    width: 28%;
    height: 0.4rem;
  }

  .hist-p {
    display: -webkit-box;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    font-size: 0.75rem;
    line-height: 1.35;
    color: var(--text);
  }

  .hist-cwd {
    color: var(--faint);
    font-size: 0.625rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hist-x {
    display: inline-flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    min-width: 2rem;
    border: 0;
    border-radius: 0.35rem;
    margin: 0.35rem 0.1rem;
    padding: 0 0.45rem;
    background: transparent;
    color: var(--faint);
    font-family: inherit;
    font-size: 0.625rem;
    cursor: pointer;
  }
  .hist-x:hover {
    background: #2a2522;
    color: var(--text);
  }
  .hist-x:focus-visible {
    outline: 2px solid var(--coral);
    outline-offset: 1px;
  }
  /* Confirmar en el propio botón: un diálogo aparte para borrar una línea es
     más ceremonia que la que merece, y un clic solo es demasiado poco. */
  .hist-x.is-sure,
  .chip.is-sure {
    border: 1px solid var(--coral);
    color: var(--coral);
  }

  .hist-empty {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    padding: 1.25rem 0.25rem;
  }
  .hist-empty-t {
    margin: 0;
    color: var(--text);
    font-size: 0.8125rem;
  }
  .hist-empty-d {
    margin: 0;
    color: var(--faint);
    font-size: 0.6875rem;
    line-height: 1.45;
    text-wrap: pretty;
  }

  .hist-r {
    margin: 0;
    border-left: 2px solid var(--line);
    padding-left: 0.7rem;
    color: var(--faint);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.625rem;
  }

  /* Franja de arrastre: en el flujo, encima de las pestañas. Antes iba
     absolute con z-index y tapaba la fila de agentes (clics y título). */
  .grip {
    display: flex;
    flex-shrink: 0;
    height: 0.85rem;
    align-items: center;
    justify-content: center;
    cursor: grab;
  }
  .grip:active {
    cursor: grabbing;
  }

  .grip-bar {
    width: 2.2rem;
    height: 3px;
    border-radius: 999px;
    background: var(--line);
    opacity: 0.55;
    transition: opacity 140ms ease;
  }
  .grip:hover .grip-bar {
    opacity: 1;
  }

  /* Flotando sobre el registro y en gris: está para cuando se busca, no para
     competir con lo que el agente está diciendo. */

  .warn {
    margin: 0;
    border-left: 2px solid var(--wait);
    padding-left: 0.55rem;
    color: var(--wait-text);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.71875rem;
    line-height: 1.5;
  }
  .warn-bar {
    flex-shrink: 0;
    padding: 0 0.95rem 0.5rem;
  }

  /* ─── Permiso ───────────────────────────────────────────────────────── */
  .perm {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 0.85rem;
    border-top: 1px solid color-mix(in srgb, var(--wait) 40%, var(--line));
    padding: 0.6rem 0.95rem;
    background: color-mix(in srgb, var(--wait) 9%, var(--shell));
  }
  .perm-copy {
    min-width: 0;
    flex: 1;
  }
  .perm-t {
    margin: 0;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.75rem;
  }
  .perm-w {
    margin: 0.1rem 0 0;
    overflow: hidden;
    color: var(--dim);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.6875rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .perm-acts {
    display: flex;
    flex-shrink: 0;
    gap: 0.35rem;
  }

  .btn {
    border: 1px solid var(--line);
    border-radius: 999px;
    padding: 0.25rem 0.7rem;
    background: transparent;
    color: var(--text);
    font-family: inherit;
    font-size: 0.71875rem;
    cursor: pointer;
  }
  .btn.is-go {
    border-color: var(--coral);
    background: var(--coral);
    color: #1c1917;
    font-weight: 600;
  }

  /* ─── Compositor ────────────────────────────────────────────────────── */
  .cmp {
    position: relative;
    flex-shrink: 0;
    padding: 0 0.7rem 0.7rem;
  }

  /* Encima del compositor y no debajo: abajo no hay sitio, la burbuja termina
     ahí. Es el mismo motivo por el que los selectores abren hacia arriba. */
  .slash {
    position: absolute;
    right: 0.7rem;
    bottom: calc(100% - 0.2rem);
    left: 0.7rem;
    z-index: 15;
    max-height: 13rem;
    margin: 0;
    border: 1px solid var(--line);
    border-radius: 10px;
    padding: 0.25rem;
    background: #262120;
    box-shadow: 0 -8px 24px rgb(0 0 0 / 45%);
    list-style: none;
    overflow: auto;
  }

  .slash-o {
    display: block;
    width: 100%;
    border: 0;
    border-radius: 6px;
    padding: 0.3rem 0.5rem;
    background: transparent;
    font-family: inherit;
    text-align: left;
    cursor: pointer;
  }
  .slash-o.active,
  .slash-o:hover {
    background: #332e2b;
  }

  .slash-n {
    display: block;
    color: var(--coral);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.71875rem;
  }

  .slash-d {
    display: block;
    overflow: hidden;
    color: var(--dim);
    font-size: 0.6875rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cmp-box {
    border: 1px solid var(--line);
    border-radius: 14px;
    padding: 0.55rem 0.6rem 0.5rem;
    background: #211d1b;
  }
  .cmp-box:focus-within {
    border-color: color-mix(in srgb, var(--coral) 55%, var(--line));
  }

  .cmp-in {
    display: block;
    width: 100%;
    box-sizing: border-box;
    min-height: calc(1.55em + 0.2rem);
    max-height: min(40vh, 14rem);
    border: 0;
    padding: 0.1rem 0.15rem 0.4rem;
    background: transparent;
    color: var(--text);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.78125rem;
    line-height: 1.55;
    overflow-y: auto;
    resize: none;
    field-sizing: content;
  }
  .cmp-in:focus {
    outline: none;
  }
  .cmp-in::placeholder {
    color: var(--faint);
  }

  .att-list {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
    margin: 0 0 0.35rem;
    padding: 0;
    list-style: none;
  }
  .att {
    position: relative;
    display: inline-flex;
    align-items: flex-start;
  }
  .att-thumb {
    display: block;
    border: 1px solid var(--line);
    border-radius: 4px;
    padding: 0;
    background: #2a2523;
    overflow: hidden;
    cursor: pointer;
    line-height: 0;
    transition: border-color 120ms ease;
  }
  .att-thumb:hover {
    border-color: #3d3733;
  }
  .att-thumb img {
    display: block;
    width: 28px;
    height: 22px;
    object-fit: cover;
  }
  .att-x {
    position: absolute;
    top: -4px;
    right: -4px;
    width: 0.85rem;
    height: 0.85rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    padding: 0;
    background: #2a2523;
    color: var(--dim);
    font: inherit;
    font-size: 0.625rem;
    line-height: 1;
    cursor: pointer;
  }
  .att-x:hover {
    color: var(--text);
    background: #3a342f;
    border-color: #3d3733;
  }

  .att-preview {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .att-preview-backdrop {
    position: absolute;
    inset: 0;
    border: 0;
    padding: 0;
    background: rgb(0 0 0 / 72%);
    cursor: pointer;
  }
  .att-preview-close {
    position: absolute;
    top: 0.85rem;
    right: 0.85rem;
    z-index: 2;
    width: 1.75rem;
    height: 1.75rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    padding: 0;
    background: #2a2523;
    color: var(--dim);
    font: inherit;
    font-size: 1rem;
    line-height: 1;
    cursor: pointer;
  }
  .att-preview-close:hover {
    color: var(--text);
    background: #3a342f;
  }
  .att-preview-img-btn {
    position: relative;
    z-index: 1;
    border: 0;
    padding: 0;
    background: transparent;
    cursor: pointer;
  }
  .att-preview-img {
    display: block;
    max-width: 90vw;
    max-height: 90vh;
    object-fit: contain;
    border: 1px solid var(--line);
    border-radius: 8px;
  }

  .cmp-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.35rem 0.3rem;
  }

  /* Los ajustes ceden espacio; las acciones no. `min-width: 0` es lo que
     permite que las pastillas se recorten en vez de empujar la fila. */
  .cmp-set {
    display: flex;
    min-width: 0;
    flex: 1 1 auto;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.3rem;
  }

  .cmp-acts {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    margin-left: auto;
    gap: 0.3rem;
  }

  .plus-wrap {
    position: relative;
  }
  .plus {
    justify-content: center;
    min-width: 1.65rem;
    font-size: 1rem;
    line-height: 1;
  }
  .plus.is-open {
    border-color: var(--coral);
    color: var(--text);
  }
  .plus-menu {
    position: absolute;
    right: 0;
    bottom: calc(100% + 0.35rem);
    z-index: 20;
    min-width: 10.5rem;
    margin: 0;
    border: 1px solid var(--line);
    border-radius: 0.6rem;
    padding: 0.2rem;
    background: #262120;
    box-shadow: 0 10px 26px rgb(0 0 0 / 45%);
    list-style: none;
  }
  .plus-menu button {
    display: block;
    width: 100%;
    border: 0;
    border-radius: 0.4rem;
    padding: 0.32rem 0.55rem;
    background: transparent;
    color: var(--text);
    font-family: inherit;
    font-size: 0.6875rem;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .plus-menu button:hover:not(:disabled) {
    background: var(--line);
  }
  .plus-menu button.active {
    color: var(--coral);
  }
  .plus-menu button:disabled {
    color: var(--faint);
    cursor: default;
  }
  .plus-label {
    margin-top: 0.15rem;
    border-top: 1px solid var(--line);
    padding: 0.3rem 0.55rem 0.08rem;
    color: var(--faint);
    font-size: 0.625rem;
    text-transform: uppercase;
  }

  /* Pastilla con el valor puesto. Un `<select>` acá escondía el valor actual,
     que es justo lo que uno mira antes de mandar. */
  .chip.is-icon {
    max-width: none;
    padding-right: 0.4rem;
    padding-left: 0.4rem;
  }

  /* `inline-flex` y no texto suelto: las que llevan ícono lo tenían como
     bloque, así que «Preguntar siempre» bajaba de línea y la pastilla quedaba
     del doble de alto que sus vecinas. */
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    padding: 0.2rem 0.6rem;
    background: transparent;
    color: var(--dim);
    font-family: inherit;
    font-size: 0.6875rem;
    max-width: 10rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }
  .chip:hover {
    color: var(--text);
  }
  .chip.is-resume {
    border-color: var(--coral);
    color: var(--coral);
  }
  .chip.is-on {
    border-color: var(--text);
    background: color-mix(in srgb, var(--text) 12%, transparent);
    color: var(--text);
  }
  .chip.is-risk,
  .mode-chip.is-risk :global(.pm-chip) {
    border-color: #d8893f;
    color: #e0a15d;
  }
  .chip.is-static {
    cursor: default;
  }

  /* Uso de contexto: franja fina encima del compositor, sin el anillo duplicado. */
  .usage {
    flex-shrink: 0;
    padding: 0 0.95rem 0.35rem;
  }
  .usage-meta {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
    margin-bottom: 0.25rem;
    color: var(--faint);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.625rem;
  }
  .usage-cost {
    color: var(--dim);
  }
  .usage-track {
    height: 2px;
    border-radius: 999px;
    background: var(--line);
    overflow: hidden;
  }
  .usage-fill {
    width: 100%;
    height: 100%;
    border-radius: inherit;
    background: color-mix(in srgb, var(--coral) 70%, transparent);
    transform: scaleX(0);
    transform-origin: left center;
    transition: transform 400ms cubic-bezier(0.2, 0, 0, 1);
  }

  @media (prefers-reduced-motion: reduce) {
    .usage-fill {
      transition: none;
    }
    .live {
      animation: none;
    }
  }

  .go {
    display: inline-flex;
    width: 1.75rem;
    height: 1.75rem;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: 999px;
    background: var(--coral);
    color: #1c1917;
    cursor: pointer;
  }
  .go:disabled {
    background: #35302c;
    color: var(--faint);
    cursor: default;
  }
  .go.is-stop:disabled {
    background: color-mix(in srgb, var(--coral) 24%, #35302c);
    color: var(--coral);
  }
  .go.is-approve {
    background: var(--wait);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    padding: 0;
    clip: rect(0 0 0 0);
    border: 0;
    overflow: hidden;
    white-space: nowrap;
  }
</style>
