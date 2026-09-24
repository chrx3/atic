<script lang="ts" module>
  /**
   * Los agentes arrancan de a uno. Varios CLI a la vez (Claude Code) se pisan
   * al escribir su config en `~/.claude.json`: queda anotado como «murió al
   * arrancar» uno que arrancó bien, y tras dos de esos apaga su pantalla
   * completa. Con un respiro entre uno y otro, esas escrituras no coinciden.
   */
  const SPAWN_GAP_MS = 1500;
  let spawnChain: Promise<void> = Promise.resolve();

  function spawnTurn(): Promise<void> {
    const turn = spawnChain;
    spawnChain = turn.then(
      () => new Promise((r) => window.setTimeout(r, SPAWN_GAP_MS)),
    );
    return turn;
  }
</script>

<script lang="ts">
  /**
   * Una PTY en un xterm, para la ventana de agentes.
   *
   * Hace una sola cosa: engancha una sesión —una ya viva, repintando su
   * scrollback, o una nueva que abre ella misma— a un terminal que se ajusta
   * a su caja. Sin nada del overlay (modo texto, OLE, traspasos): en una
   * ventana normal el foco y el teclado ya funcionan solos.
   *
   * Rust es el dueño del proceso. Desmontar la vista suelta la sesión, no la
   * mata; cerrarla es decisión de quien la contiene.
   */
  import { onMount, untrack } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import { consoleOpen, consoleResize, consoleTail, consoleWrite } from "$ipc/agents";
  import { t } from "$domain/i18n.svelte";
  import {
    CONSOLE_VIEW_ID,
    claim,
    consoleBusReady,
    release,
    subscribeConsole,
  } from "./consoleBus";
  import { terminalTheme } from "./terminalTheme";

  let {
    sessionId = null,
    command = null,
    cwd = null,
    fontZoom = 0,
    focused = false,
    onSession,
    onExit,
    onTitle,
  }: {
    /** PTY viva a mostrar. `null` = abrir una con `command` en `cwd`. */
    sessionId?: string | null;
    command?: string | null;
    cwd?: string | null;
    /** Puntos sobre el tamaño base. */
    fontZoom?: number;
    /** Tomar el foco al montar y cuando pasa a `true`. */
    focused?: boolean;
    onSession?: (id: string) => void;
    onExit?: (code: number | null) => void;
    onTitle?: (title: string) => void;
  } = $props();

  /** Abrir con menos deja al TUI del agente dibujado en miniatura para siempre. */
  const MIN_COLS = 80;
  const MIN_ROWS = 24;
  const TAIL_BYTES = 128 * 1024;
  const BASE_FONT = 12.5;

  let host = $state<HTMLElement | null>(null);
  let term: Terminal | null = null;
  let fit: FitAddon | null = null;
  let live: string | null = null;
  let booting = $state(true);
  let ended = $state<number | null | undefined>(undefined);

  function fitNow() {
    if (!term || !fit || !host) return;
    if (host.clientWidth < 40 || host.clientHeight < 40) return;
    fit.fit();
    if (live && term.cols >= 2 && term.rows >= 2) {
      void consoleResize(live, term.cols, term.rows).catch(() => {});
    }
  }

  let fitTimer = 0;
  /** Un arrastre de borde manda decenas de tamaños: al PTY solo el último. */
  function scheduleFit() {
    window.clearTimeout(fitTimer);
    fitTimer = window.setTimeout(fitNow, 90);
  }

  /** Espera a que la caja tenga tamaño real: abrir a 0×0 rompe el TUI. */
  async function settledSize() {
    for (let i = 0; i < 120; i++) {
      if (host && host.clientWidth >= 200 && host.clientHeight >= 120) return;
      await new Promise((r) => requestAnimationFrame(r));
    }
  }

  function attach(id: string) {
    live = id;
    claim(id);
    return subscribeConsole(
      id,
      (data) => {
        booting = false;
        term?.write(data);
      },
      (code) => {
        ended = code;
        live = null;
        term?.write(
          `\r\n\x1b[2m[${t("page.agents.window.ended", { code: code ?? "?" })}]\x1b[0m\r\n`,
        );
        onExit?.(code);
      },
    );
  }

  onMount(() => {
    const instance = new Terminal({
      cursorBlink: true,
      fontSize: BASE_FONT + untrack(() => fontZoom),
      fontFamily: "Cascadia Mono, SFMono-Regular, Menlo, Consolas, monospace",
      lineHeight: 1.12,
      minimumContrastRatio: 4.5,
      drawBoldTextInBrightColors: true,
      theme: terminalTheme(),
      rightClickSelectsWord: false,
    });
    const addon = new FitAddon();
    instance.loadAddon(addon);
    term = instance;
    fit = addon;
    instance.open(host!);

    instance.onData((data) => {
      if (live) void consoleWrite(live, data).catch(() => {});
    });
    instance.onTitleChange((title) => onTitle?.(title));
    // Ctrl+V lo pega el evento `paste` nativo; mandarlo también como tecla
    // lo duplicaba. Ctrl+C con selección copia en vez de interrumpir.
    instance.attachCustomKeyEventHandler((ev) => {
      if (ev.type !== "keydown") return true;
      const mod = ev.ctrlKey || ev.metaKey;
      if (mod && (ev.key === "v" || ev.key === "V")) return false;
      if (mod && (ev.key === "c" || ev.key === "C") && instance.hasSelection()) {
        void navigator.clipboard.writeText(instance.getSelection()).catch(() => {});
        return false;
      }
      return true;
    });

    const observer = new ResizeObserver(scheduleFit);
    observer.observe(host!);
    // El tema sigue a la app: la paleta se cambia en caliente.
    const themeWatch = new MutationObserver(() => {
      instance.options.theme = terminalTheme();
    });
    themeWatch.observe(document.documentElement, { attributeFilter: ["data-theme"] });

    let unsubscribe: (() => void) | null = null;
    let disposed = false;
    const start = untrack(() => ({ sessionId, command, cwd }));

    void (async () => {
      await consoleBusReady();
      await settledSize();
      if (disposed) return;
      fitNow();
      if (start.sessionId) {
        // Ya viva: se repinta lo que dijo mientras nadie la miraba.
        unsubscribe = attach(start.sessionId);
        const tail = await consoleTail(start.sessionId, TAIL_BYTES).catch(() => "");
        if (disposed) return;
        if (tail) instance.write(tail);
        booting = false;
        fitNow();
        return;
      }
      if (start.command) {
        await spawnTurn();
        if (disposed) return;
      }
      try {
        const id = await consoleOpen({
          kind: "local",
          command: start.command,
          cwd: start.cwd,
          cols: Math.max(MIN_COLS, instance.cols),
          rows: Math.max(MIN_ROWS, instance.rows),
          view: CONSOLE_VIEW_ID,
        });
        if (disposed) {
          release(id);
          return;
        }
        unsubscribe = attach(id);
        onSession?.(id);
        // Sin comando es una shell: no hay TUI que esperar.
        if (!start.command) booting = false;
        requestAnimationFrame(fitNow);
        window.setTimeout(fitNow, 400);
      } catch (err) {
        booting = false;
        instance.write(`\x1b[31m${String(err)}\x1b[0m\r\n`);
      }
    })();

    return () => {
      disposed = true;
      window.clearTimeout(fitTimer);
      observer.disconnect();
      themeWatch.disconnect();
      unsubscribe?.();
      if (live) release(live);
      instance.dispose();
      term = null;
      fit = null;
    };
  });

  /** Lo seleccionado en la terminal, para encargárselo a otro agente. */
  export function selection(): string {
    return term?.getSelection() ?? "";
  }

  $effect(() => {
    const size = BASE_FONT + fontZoom;
    if (!term || term.options.fontSize === size) return;
    term.options.fontSize = size;
    scheduleFit();
  });

  $effect(() => {
    if (focused) requestAnimationFrame(() => term?.focus());
  });
</script>

<div class="terminal">
  <div class="host" bind:this={host}></div>
  {#if booting && ended === undefined}
    <p class="boot" role="status">{t("page.agents.window.booting")}</p>
  {/if}
</div>

<style>
  .terminal {
    position: relative;
    height: 100%;
    min-height: 0;
    overflow: hidden;
    background: var(--rb-bg0);
  }

  .host {
    position: absolute;
    inset: 8px 4px 4px 12px;
  }

  .boot {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    margin: 0;
    color: var(--rb-muted);
    font-size: 12px;
    pointer-events: none;
  }
</style>
