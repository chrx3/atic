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
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { agents } from "$lib/agentSessions.svelte";
  import {
    agentBackends,
    getConfig,
    hideAgentsWindow,
    toggleDictation,
    onAgentsBubbleAnchor,
    onAgentsBubbleDismiss,
  } from "$lib/api";
  import { applyTheme, readCachedTheme } from "$lib/theme";
  import { MOTION, ms } from "$lib/motion";
  import McpServersModal from "$lib/McpServersModal.svelte";
  import PickerMenu from "$lib/PickerMenu.svelte";
  import ClaudeMark from "$lib/ClaudeMark.svelte";
  import AgentMessage from "$lib/AgentMessage.svelte";
  import AgentToolCard from "$lib/AgentToolCard.svelte";
  import AgentIcons from "$lib/AgentIcons.svelte";
  import type {
    AgentBackendInfo,
    McpServerConfig,
    PermissionDecision,
  } from "$lib/types";

  /**
   * Modelos, con lo que hace falta para elegir.
   *
   * Se usan ALIAS (`opus`, `sonnet`) y no identificadores completos: el alias
   * sigue apuntando al último de su familia, así que esta lista no envejece con
   * cada versión. El nombre y el rasgo van aparte porque «opus» a secas no dice
   * nada a quien no vive en el CLI, y elegir modelo sin saber qué cambia es
   * elegir a ciegas.
   */
  const MODELS = [
    { id: "", label: "El de tu CLI", note: "Lo que tengas configurado allá" },
    { id: "fable", label: "Fable 5", note: "El más capaz · 1M · gasta rápido" },
    { id: "opus", label: "Opus 5", note: "Muy capaz · 1M de contexto" },
    { id: "sonnet", label: "Sonnet 5", note: "Equilibrado · 1M · más barato" },
    { id: "haiku", label: "Haiku 4.5", note: "El más rápido · 200K" },
  ];

  /** `manual` primero a propósito: con alguien mirando, preguntar cuesta poco
   *  y equivocarse cuesta caro. Los permisivos existen para tareas largas. */
  const MODES = [
    { id: "manual", label: "Preguntar siempre" },
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

  const CONTEXT_WINDOW = 1_000_000;

  /**
   * Cómo salió la burbuja, ya en píxeles de CSS.
   *
   * Rust razona en píxeles FÍSICOS —es lo que usa Win32— y el CSS en lógicos.
   * A escala 100% son el mismo número y la diferencia no se ve; a 125% el
   * contenido quedaba un 25% más ancho que su ventana y se recortaba por los
   * dos lados. La conversión vive acá, una sola vez, y no repartida por reglas
   * de estilo.
   */
  let anchor = $state<{
    side: string;
    offset: number;
    w: number;
    h: number;
  } | null>(null);
  let shown = $state(false);
  /** Hay un diálogo del sistema abierto. */
  let picking = $state(false);
  /**
   * La arrastraste: la punta deja de dibujarse.
   *
   * Movida de sitio ya no sale de la pill, y una punta que apunta a donde la
   * pill no está es peor que ninguna. Al volver a abrirla se re-ancla y vuelve.
   */
  let detached = $state(false);
  /** Duración del vuelo desde la pill, para fundir el contenido a la par. */
  let flight = $state(0);
  /** Antes de este instante, los movimientos son del vuelo y no del usuario. */
  let settledAt = 0;

  let backends = $state<AgentBackendInfo[]>([]);
  let picked = $state("");
  let model = $state("");
  let mode = $state("manual");
  let cwd = $state("");
  let starting = $state(false);
  let error = $state<string | null>(null);
  let draft = $state("");
  let logEl = $state<HTMLElement | null>(null);
  let inputEl = $state<HTMLTextAreaElement | null>(null);
  let mcpOpen = $state(false);
  let mcpServers = $state<McpServerConfig[]>([]);
  let menu = $state<"model" | "mode" | "agent" | null>(null);

  let activeId = $state<string | null>(null);
  const active = $derived(agents.byId(activeId));
  const ready = $derived(
    backends.find((b) => b.id === picked)?.available ?? false,
  );
  const enabledMcp = $derived(mcpServers.filter((s) => s.enabled));
  const modelLabel = $derived(
    MODELS.find((m) => m.id === model)?.label ?? "Modelo",
  );
  const modeLabel = $derived(MODES.find((m) => m.id === mode)?.label ?? "Permisos");
  const ctxPct = $derived(
    Math.min(100, ((active?.contextTokens ?? 0) / CONTEXT_WINDOW) * 100),
  );

  /**
   * Los items de la conversación, en orden.
   *
   * Antes acá vivía una derivación que recorría el registro plano emparejando
   * cada `toolCall` con su `toolResult` por id, porque llegaban como eventos
   * separados y con lo que hubiera en medio. Ya no hace falta: el backend
   * manda items con identidad y el store los mantiene actualizados, así que
   * esto es aplanar turnos y nada más.
   */
  const items = $derived((active?.turns ?? []).flatMap((t) => t.items));

  /**
   * El agente está escribiendo ahora mismo.
   *
   * Solo los items de texto tienen `streaming`; una herramienta corriendo no
   * cuenta, porque ahí lo que se muestra es su tarjeta latiendo y no un cursor.
   */
  const writing = $derived.by(() => {
    const last = items.at(-1);
    return !!last && "streaming" in last && last.streaming;
  });

  /** El cierre de cada turno, para dibujar la regla con el costo. */
  const turnEnds = $derived(
    new Map(
      (active?.turns ?? [])
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
    const un = onAgentsBubbleAnchor((a) => {
      const dpr = window.devicePixelRatio || 1;
      anchor = {
        side: a.side,
        offset: a.offset / dpr,
        w: a.w / dpr,
        h: a.h / dpr,
      };
      detached = false;
      // La ventana está volando desde la pill: el contenido se funde durante
      // ese mismo tiempo, así que crecer y aparecer son un solo gesto.
      flight = a.flight;
      shown = false;
      // El vuelo empieza YA; ignorar los `moved` que provoca, o la burbuja se
      // daría por arrastrada antes de terminar de abrirse.
      settledAt = Date.now() + a.flight + 120;
      void tick().then(() => requestAnimationFrame(() => (shown = true)));
    });

    // Moverla a mano la desancla. Se detecta por el evento de la ventana y no
    // por el arrastre en sí: Windows también la mueve al acomodarla contra un
    // borde, y eso cuenta igual.
    let unMoved: (() => void) | null = null;
    void (async () => {
      try {
        unMoved = await getCurrentWindow().onMoved(() => {
          if (Date.now() < settledAt) return;
          detached = true;
        });
      } catch {
        // Sin ventana nativa no hay nada que seguir.
      }
    })();

    // La rueda pide cerrarla (segunda pulsación sobre «Agentes»).
    const unDismiss = onAgentsBubbleDismiss(() => void close());

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
        picked = backends.find((b) => b.available)?.id ?? backends[0]?.id ?? "";
      } catch (err) {
        error = String(err);
      }
      try {
        mcpServers = parseMcp((await getConfig()).agent_mcp_servers);
      } catch {
        // Sin config se arranca sin servidores extra.
      }
    })();

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !mcpOpen && !menu) {
        event.preventDefault();
        void close();
      }
    };
    window.addEventListener("keydown", onKeyDown);

    return () => {
      window.removeEventListener("keydown", onKeyDown);
      void un.then((fn) => fn());
      void unDismiss.then((fn) => fn());
      unMoved?.();
      agents.watch(null);
    };
  });

  function parseMcp(raw: string | undefined): McpServerConfig[] {
    if (!raw) return [];
    try {
      const parsed = JSON.parse(raw);
      return Array.isArray(parsed) ? parsed : [];
    } catch {
      return [];
    }
  }

  $effect(() => {
    if (activeId && agents.byId(activeId)) return;
    activeId = agents.sessions[0]?.id ?? null;
  });

  $effect(() => {
    agents.watch(activeId);
  });

  $effect(() => {
    // También con cada trozo del streaming: si no, el texto crece por debajo
    // del borde y hay que perseguirlo con la rueda mientras se escribe.
    // También con el último trozo: si no, el texto crece por debajo del borde
    // y hay que perseguirlo con la rueda mientras se escribe.
    const last = items.at(-1);
    const n = items.length + (last && "text" in last ? last.text.length : 0);
    if (!logEl || n === 0) return;
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
    if (!shown) return;
    shown = false;
    try {
      await hideAgentsWindow();
    } catch {
      // Sin ventana nativa (preview web) no hay nada que replegar.
    }
  }

  function mcpConfig(): string | undefined {
    if (enabledMcp.length === 0) return undefined;
    const servers: Record<string, unknown> = {};
    for (const server of enabledMcp) {
      try {
        servers[server.name] = JSON.parse(server.json);
      } catch {
        // Un servidor con JSON roto se salta: mejor arrancar sin él que no
        // arrancar. El aviso ya está donde se edita.
      }
    }
    return JSON.stringify({ mcpServers: servers });
  }

  async function pickFolder() {
    picking = true;
    try {
      const chosen = await openDialog({ directory: true, multiple: false });
      if (typeof chosen === "string") cwd = chosen;
    } catch (err) {
      error = String(err);
    } finally {
      picking = false;
    }
  }

  /**
   * Dicta dentro del compositor.
   *
   * El foco va PRIMERO: el dictado de Atic pega en el control que tenga el
   * foco del sistema cuando termina, así que sin esto el texto acabaría en la
   * app que estuviera detrás de la burbuja.
   */
  async function dictate() {
    inputEl?.focus();
    try {
      await toggleDictation();
    } catch (err) {
      error = String(err);
    }
  }

  async function attach() {
    picking = true;
    try {
      const chosen = await openDialog({ multiple: true });
      const paths = Array.isArray(chosen) ? chosen : chosen ? [chosen] : [];
      if (paths.length === 0) return;
      // Se manda la RUTA, no el contenido: el agente sabe leer archivos, y
      // volcarlos acá gastaría contexto en algo que él abre cuando le sirva.
      draft = [draft.trim(), ...paths].filter(Boolean).join("\n");
      inputEl?.focus();
    } catch (err) {
      error = String(err);
    } finally {
      picking = false;
    }
  }

  async function start() {
    if (starting || !picked) return;
    starting = true;
    error = null;
    try {
      activeId = await agents.start(picked, {
        cwd: cwd || undefined,
        model: model || undefined,
        permissionMode: mode,
        mcpConfig: mcpConfig(),
      });
    } catch (err) {
      error = String(err);
    } finally {
      starting = false;
    }
  }

  async function send(override?: string) {
    const text = (override ?? draft).trim();
    if (!text || !activeId) return;
    if (!override) draft = "";
    slashOpen = false;
    try {
      await agents.send(activeId, text);
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
  async function switchModel(id: string) {
    model = id;
    menu = null;
    if (!activeId || !id) return;
    await send(`/model ${id}`);
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
      if (active) void send();
      else void start();
    }
  }

  function shortNumber(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${Math.round(n / 1000)}k`;
    return String(n);
  }

</script>

<div
  class="bub"
  class:is-shown={shown}
  class:is-loose={detached}
  data-side={anchor?.side ?? "top"}
  style="--tail: {anchor?.offset ?? 40}px; --fade: {flight || 200}ms; --w: {anchor?.w ??
    580}px; --h: {anchor?.h ?? 520}px"
>
  <span class="bub-tail" aria-hidden="true"></span>

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

    <button
      type="button"
      class="shut"
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

    {#if agents.sessions.length > 1}
      <div class="tabs" role="tablist" aria-label="Sesiones">
        {#each agents.sessions as s (s.id)}
          <button
            type="button"
            role="tab"
            class="tab"
            class:active={s.id === activeId}
            aria-selected={s.id === activeId}
            onclick={() => (activeId = s.id)}
          >
            {#if s.pending.length > 0}
              <span class="dot is-wait"></span>
            {:else if s.status === "working"}
              <span class="dot is-busy"></span>
            {:else if s.unread > 0}
              <span class="dot is-new"></span>
            {:else}
              <span class="dot"></span>
            {/if}
            {s.backendName}
          </button>
        {/each}
      </div>
    {/if}

    <div class="log" bind:this={logEl} role="log">
      {#if !active}
        <!-- Caja de bienvenida con el título en el borde, como el CLI. El
             bicho identifica al backend: cuando sea Codex o Gemini irá su
             marca, no esta. -->
        <section class="card card-hi">
          <h2 class="card-title">Atic · agentes</h2>
          <span class="hi-mark" class:is-off={!ready}>
            <ClaudeMark size={54} />
          </span>
          <div class="hi-copy">
            <p class="card-line">
              Lanza el agente que ya tienes instalado, con tu sesión, tus
              herramientas y tus skills. Atic solo le pone cara.
            </p>
            <p class="card-line dim">
              Elige abajo con qué arrancar. Escribe y Enter para empezar; toca
              fuera y sigue trabajando, que la pill te avisa.
            </p>
          </div>
        </section>

        {#if backends.length > 0 && !ready}
          <p class="warn">
            No se encontró el ejecutable. Instálalo y ábrelo una vez en la
            consola para iniciar sesión; Atic usa esa misma cuenta.
          </p>
        {/if}
      {:else}
        {#each items as item (item.id)}
          {#if item.kind === "message" && item.role === "user"}
            <!-- El turno del usuario. Antes no se dibujaba en ninguna parte:
                 el registro solo tenía lo que venía del backend, así que la
                 conversación se leía como un monólogo del agente. -->
            <div class="mine">
              <span class="mine-who">tú</span>
              <AgentMessage text={item.text} />
            </div>
          {:else if item.kind === "message"}
            <div class:wip={item.streaming}>
              <AgentMessage text={item.text} />
              {#if item.streaming}
                <span class="caret" aria-hidden="true"></span>
              {/if}
            </div>
          {:else if item.kind === "tool"}
            <AgentToolCard
              name={item.name}
              title={item.title}
              toolKind={item.toolKind}
              input={item.input}
              output={item.output}
              status={item.status}
              locations={item.locations}
            />
          {:else if item.kind === "reasoning"}
            <!-- El razonamiento es trabajo previo, no la respuesta: se ofrece
                 plegado para que no compita con lo que el agente dice. -->
            <details class="think">
              <summary>pensando</summary>
              <p>{item.text}</p>
            </details>
          {:else if item.kind === "plan"}
            <div class="plan">
              <div class="plan-h">plan</div>
              {#each item.entries as e, i (i)}
                <div class="plan-e" data-s={e.status}>
                  <span class="plan-b"
                    >{e.status === "completed"
                      ? "✓"
                      : e.status === "in_progress"
                        ? "▸"
                        : "○"}</span
                  >
                  <span class="plan-t">{e.text}</span>
                </div>
              {/each}
            </div>
          {:else if item.kind === "notice"}
            <p class="warn">{item.text}</p>
          {/if}

          <!-- Cierre de turno: una regla con el costo al medio. Separa las
               respuestas entre sí, que sin esto se leían como una sola. -->
          {#if turnEnds.has(item.id)}
            <p class="turn">
              <span class="turn-t"
                >{turnEnds.get(item.id) !== null
                  ? `$${turnEnds.get(item.id)?.toFixed(4)}`
                  : "fin del turno"}</span
              >
            </p>
          {/if}
        {/each}

        {#if active.status === "working" && !writing}
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

      <div class="cmp-box">
        <textarea
          class="cmp-in"
          bind:this={inputEl}
          bind:value={draft}
          onkeydown={onKey}
          rows="2"
          placeholder={active
            ? "Escribe · Enter envía, Shift+Enter salta línea"
            : "Describe lo que quieres y Enter para empezar…"}
          aria-label="Mensaje para el agente"
        ></textarea>

        <div class="cmp-row">
          {#if !active}
            <!-- Estas tres solo se pueden fijar al arrancar, así que están a la
                 vista con su valor puesto y no escondidas en un ajuste. -->
            <PickerMenu
              label={backends.find((b) => b.id === picked)?.displayName ??
                "Agente"}
              open={menu === "agent"}
              options={backends.map((b) => ({
                id: b.id,
                label: b.displayName,
                disabled: !b.available,
              }))}
              value={picked}
              onToggle={() => (menu = menu === "agent" ? null : "agent")}
              onPick={(id) => {
                picked = id;
                menu = null;
              }}
            />
            <PickerMenu
              label={modelLabel}
              open={menu === "model"}
              options={MODELS}
              value={model}
              onToggle={() => (menu = menu === "model" ? null : "model")}
              onPick={(id) => {
                model = id;
                menu = null;
              }}
            />
            <PickerMenu
              label={modeLabel}
              open={menu === "mode"}
              options={MODES}
              value={mode}
              onToggle={() => (menu = menu === "mode" ? null : "mode")}
              onPick={(id) => {
                mode = id;
                menu = null;
              }}
            >
              {#snippet icon()}
                <AgentIcons name={SHIELDS[mode] ?? "shield-manual"} />
              {/snippet}
            </PickerMenu>
            <button type="button" class="chip" onclick={() => void pickFolder()}>
              <AgentIcons name="folder" />
              {cwd ? cwd.split(/[\\/]/).pop() : "Carpeta"}
            </button>
            <button type="button" class="chip" onclick={() => (mcpOpen = true)}>
              MCP{enabledMcp.length > 0 ? ` · ${enabledMcp.length}` : ""}
            </button>
          {:else}
            <!-- El modelo sigue vivo con la sesión abierta: `/model <alias>`
                 lo cambia sin reiniciar. -->
            <PickerMenu
              label={modelLabel}
              open={menu === "model"}
              options={MODELS.filter((m) => m.id)}
              value={model}
              onToggle={() => (menu = menu === "model" ? null : "model")}
              onPick={(id) => void switchModel(id)}
            />
            <span class="chip is-static">
              <AgentIcons name={SHIELDS[mode] ?? "shield-manual"} />
              {modeLabel}
            </span>
            <button type="button" class="chip" onclick={() => void attach()}>
              Adjuntar
            </button>
            <button type="button" class="chip" onclick={() => void stop()}>
              Terminar
            </button>
          {/if}

          <span class="cmp-gap"></span>

          <!-- Dictar al agente. Es lo que ninguna GUI de agentes tiene, y acá
               sale gratis: el dictado ya pega en el control con el foco, así
               que enfocar el compositor antes es todo lo que hace falta. -->
          <button
            type="button"
            class="chip is-icon"
            onclick={() => void dictate()}
            title="Dictar · Ctrl+Shift+D"
            aria-label="Dictar"
          >
            <AgentIcons name="mic" size={12} />
          </button>

          {#if active}
            <!-- El contexto es el recurso que se agota sin avisar: anillo
                 siempre visible, no un comando que haya que recordar. -->
            <span
              class="ring"
              title="Contexto: {shortNumber(active.contextTokens)} tokens"
              style="--pct: {ctxPct}"
            >
              <span class="ring-n">{shortNumber(active.contextTokens)}</span>
            </span>
            {#if active.costUsd > 0}
              <span class="cost">${active.costUsd.toFixed(3)}</span>
            {/if}
          {/if}

          <button
            type="button"
            class="go"
            onclick={() => (active ? void send() : void start())}
            disabled={starting || (!active && !ready) || (!!active && !draft.trim())}
            aria-label={active ? "Enviar" : "Iniciar sesión"}
          >
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
          </button>
        </div>
      </div>
    </div>
  </div>
</div>

{#if mcpOpen}
  <McpServersModal
    servers={mcpServers}
    onSave={(next) => {
      mcpServers = next;
      mcpOpen = false;
    }}
    onClose={() => (mcpOpen = false)}
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
    --inset: 28px;
    --coral: #da7756;
    --ink: #17151400;
    --shell: #1c1917;
    --line: #332e2b;
    --text: #e7e2dd;
    --dim: #8d827a;
    --faint: #6b615a;

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
  .bub.is-loose .bub-tail {
    display: none;
  }

  .bub-body {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    border: 1px solid var(--line);
    border-radius: 18px;
    background: var(--shell);
    /* Cabe entera en `--inset` (6 + 20 = 26 < 28), así que la ventana no la
       recorta y el globo queda flotando de verdad. */
    box-shadow: 0 6px 20px rgb(0 0 0 / 42%);
    color: var(--text);
    overflow: hidden;
  }

  /* La punta: un cuadrado girado 45°, con el mismo borde y fondo que el
     cuerpo. Un triángulo con `border` no puede llevar borde propio, y sin él
     se ve como un pegote suelto al lado de la burbuja. */
  .bub-tail {
    position: absolute;
    width: 16px;
    height: 16px;
    border: 1px solid var(--line);
    background: var(--shell);
    transform: rotate(45deg);
  }
  .bub[data-side="top"] .bub-tail {
    top: calc(var(--inset) - 8px);
    left: calc(var(--tail) - 8px);
    border-right: 0;
    border-bottom: 0;
    border-top-left-radius: 4px;
  }
  .bub[data-side="bottom"] .bub-tail {
    bottom: calc(var(--inset) - 8px);
    left: calc(var(--tail) - 8px);
    border-left: 0;
    border-top: 0;
    border-bottom-right-radius: 4px;
  }
  .bub[data-side="left"] .bub-tail {
    top: calc(var(--tail) - 8px);
    left: calc(var(--inset) - 8px);
    border-top: 0;
    border-right: 0;
    border-bottom-left-radius: 4px;
  }
  .bub[data-side="right"] .bub-tail {
    top: calc(var(--tail) - 8px);
    right: calc(var(--inset) - 8px);
    border-bottom: 0;
    border-left: 0;
    border-top-right-radius: 4px;
  }

  /* ─── Sesiones ──────────────────────────────────────────────────────── */
  .tabs {
    display: flex;
    min-width: 0;
    flex-shrink: 0;
    gap: 0.2rem;
    padding: 0.5rem 0.7rem 0;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .tab {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    border: 0;
    border-radius: 999px;
    padding: 0.15rem 0.55rem;
    background: transparent;
    color: var(--dim);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.6875rem;
    white-space: nowrap;
    cursor: pointer;
  }
  .tab.active {
    background: #2a2522;
    color: var(--text);
  }

  .dot {
    width: 0.35rem;
    height: 0.35rem;
    border-radius: 999px;
    background: var(--faint);
  }
  .dot.is-wait {
    background: var(--coral);
  }
  .dot.is-busy {
    background: var(--dim);
    animation: pulse 1.6s ease-in-out infinite;
  }
  .dot.is-new {
    background: #7dd3a0;
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

  /* Caja con el título incrustado en el borde: el gesto que define la consola
     de Claude Code y lo que hace que un bloque se lea como una unidad. */
  .card {
    position: relative;
    margin-top: 0.5rem;
    border: 1px solid var(--line);
    border-radius: 10px;
    padding: 0.75rem 0.85rem 0.7rem;
  }

  .card-title {
    position: absolute;
    top: -0.55rem;
    left: 0.7rem;
    margin: 0;
    padding: 0 0.35rem;
    background: var(--shell);
    color: var(--coral);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.6875rem;
    font-weight: 400;
  }

  .card-line {
    margin: 0 0 0.35rem;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.75rem;
    line-height: 1.55;
  }
  .card-line:last-child {
    margin-bottom: 0;
  }
  .card-line.dim {
    color: var(--dim);
  }

  /* La marca a la izquierda y el texto al lado, como el saludo del CLI. */
  .card-hi {
    display: flex;
    align-items: flex-start;
    gap: 0.85rem;
  }

  .hi-mark {
    flex-shrink: 0;
    color: var(--coral);
    line-height: 0;
  }
  /* Sin el agente instalado, la marca se apaga: el estado se ve antes de leer
     el aviso de abajo. */
  .hi-mark.is-off {
    color: var(--faint);
  }

  .hi-copy {
    min-width: 0;
    flex: 1;
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

  /* Texto en vivo: el cursor parpadeante es la señal de que sigue escribiendo,
     y evita tener que poner un «trabajando…» encima de su propia respuesta. */
  .wip {
    position: relative;
  }

  .caret {
    display: inline-block;
    width: 0.45rem;
    height: 0.85rem;
    margin-left: 0.15rem;
    background: var(--coral);
    vertical-align: text-bottom;
    animation: blink 1s steps(2, start) infinite;
  }

  @keyframes blink {
    50% {
      opacity: 0;
    }
  }

  /* El turno del usuario: barra al costado, sin caja. Es la voz propia
     dentro del registro, no una tarjeta más. */
  .mine {
    border-left: 2px solid var(--coral);
    padding-left: 0.55rem;
  }
  .mine-who {
    display: block;
    color: var(--faint);
    font-size: 0.6875rem;
    letter-spacing: 0.03em;
  }

  /* Plan propuesto por el agente, con el estado de cada paso. */
  .plan {
    border: 1px solid var(--line);
    border-radius: 9px;
    padding: 0.45rem 0.6rem;
    background: #1f1b19;
  }
  .plan-h {
    color: var(--faint);
    font-size: 0.6875rem;
    letter-spacing: 0.03em;
  }
  .plan-e {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    color: var(--dim);
    font-size: 0.71875rem;
  }
  .plan-b {
    flex-shrink: 0;
    width: 1.1em;
    color: var(--faint);
  }
  .plan-e[data-s="in_progress"] {
    color: var(--text);
  }
  .plan-e[data-s="in_progress"] .plan-b {
    color: var(--coral);
  }
  .plan-e[data-s="completed"] .plan-b {
    color: #7dd3a0;
  }
  .plan-e[data-s="completed"] .plan-t {
    text-decoration: line-through;
    text-decoration-color: var(--line);
  }

  .think {
    color: var(--faint);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.6875rem;
  }
  .think summary {
    cursor: pointer;
  }
  .think p {
    margin: 0.35rem 0 0;
    border-left: 2px solid var(--line);
    padding-left: 0.7rem;
    color: var(--dim);
    line-height: 1.5;
    white-space: pre-wrap;
  }

  /* Cierre de turno: una regla fina que corta el ancho. Sin ella, dos
     respuestas seguidas se leían como una sola parrafada. */
  .turn {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    margin: 0.15rem 0;
    color: var(--faint);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.625rem;
  }
  .turn::before,
  .turn::after {
    height: 1px;
    flex: 1;
    background: var(--line);
    content: "";
  }
  .turn-t {
    flex-shrink: 0;
  }

  /* Franja de arrastre: ocupa el ancho de arriba y solo se insinúa al pasar
     por encima. Ocupar sitio con una barra de título completa contradiría que
     esto es un globo y no una ventana. */
  .grip {
    position: absolute;
    top: var(--inset);
    right: var(--inset);
    left: var(--inset);
    z-index: 4;
    display: flex;
    height: 1.5rem;
    align-items: center;
    justify-content: center;
    border-radius: 18px 18px 0 0;
  }

  .grip-bar {
    width: 2.2rem;
    height: 3px;
    border-radius: 999px;
    background: var(--line);
    opacity: 0;
    transition: opacity 140ms ease;
  }
  .grip:hover .grip-bar {
    opacity: 1;
  }

  /* Flotando sobre el registro y en gris: está para cuando se busca, no para
     competir con lo que el agente está diciendo. */
  .shut {
    position: absolute;
    top: calc(var(--inset) + 3px);
    right: calc(var(--inset) + 6px);
    z-index: 5;
    display: flex;
    border: 0;
    border-radius: 999px;
    padding: 0.25rem;
    background: transparent;
    color: var(--faint);
    cursor: pointer;
  }
  .shut:hover {
    background: #2a2522;
    color: var(--text);
  }

  .warn {
    margin: 0;
    color: var(--coral);
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
    border-top: 1px solid color-mix(in srgb, var(--coral) 45%, transparent);
    padding: 0.6rem 0.95rem;
    background: color-mix(in srgb, var(--coral) 12%, transparent);
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
    border: 0;
    padding: 0.1rem 0.15rem 0.4rem;
    background: transparent;
    color: var(--text);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.78125rem;
    line-height: 1.55;
    resize: none;
  }
  .cmp-in:focus {
    outline: none;
  }
  .cmp-in::placeholder {
    color: var(--faint);
  }

  .cmp-row {
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }

  .cmp-gap {
    flex: 1;
  }

  /* Pastilla con el valor puesto. Un `<select>` acá escondía el valor actual,
     que es justo lo que uno mira antes de mandar. */
  .chip.is-icon {
    padding-right: 0.4rem;
    padding-left: 0.4rem;
  }

  .chip {
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
  .chip.is-static {
    cursor: default;
  }

  /* Anillo de contexto: el relleno es un cono cónico recortado con máscara. */
  .ring {
    position: relative;
    display: inline-flex;
    width: 1.75rem;
    height: 1.75rem;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    border-radius: 999px;
    background: conic-gradient(
      var(--coral) calc(var(--pct) * 1%),
      #35302c 0
    );
  }
  .ring::after {
    position: absolute;
    border-radius: 999px;
    background: #211d1b;
    content: "";
    inset: 2px;
  }
  .ring-n {
    position: relative;
    z-index: 1;
    color: var(--dim);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.5625rem;
  }

  .cost {
    color: var(--faint);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.625rem;
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
</style>
