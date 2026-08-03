<script lang="ts">
  import { onMount } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import ClipboardHistoryList from "$lib/ClipboardHistoryList.svelte";
  import SnippetsList from "$lib/SnippetsList.svelte";
  import ToolIcon from "$lib/ToolIcon.svelte";
  import FloatingAgents from "$lib/floating-v2/FloatingAgents.svelte";
  import { TOOLS, toolById, type ToolId } from "$lib/tools";
  import type { CaptureItem, ClipboardItem, DictationPhase, Snippet as TextSnippet } from "$lib/types";
  import {
    activateCapture,
    beginFloatingDrag,
    closeFloatingTool,
    detachFloatingObject,
    dictationPhase,
    getFloatingWorld,
    getScratchpad,
    isRecording,
    listClipboardHistory,
    listRecentCaptures,
    listSnippets,
    onClipboardHistoryChanged,
    onDictationStatus,
    onFloatingWorldChanged,
    onSnippetsChanged,
    onStatus,
    setScratchpad,
    startCaptureSession,
    startRecording,
    stopRecording,
    toggleDictation,
    type FloatingWorld,
  } from "$lib/api";

  let tool = $state<ToolId>("meetings");
  let recording = $state(false);
  let dictation = $state<DictationPhase>("idle");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let clipboardItems = $state<ClipboardItem[]>([]);
  let snippets = $state<TextSnippet[]>([]);
  let captures = $state<CaptureItem[]>([]);
  let loading = $state(false);
  let snippetsTab = $state<"texts" | "scratchpad">("texts");
  let scratchpad = $state("");
  let scratchSaving = $state(false);
  let floatingWorld = $state<FloatingWorld | null>(null);
  let scratchTimer: ReturnType<typeof setTimeout> | null = null;

  function moveSnippetsTab(event: KeyboardEvent) {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    snippetsTab = event.key === "ArrowRight" || event.key === "End" ? "scratchpad" : "texts";
    requestAnimationFrame(() => {
      (event.currentTarget as HTMLElement)
        .closest("[role='tablist']")
        ?.querySelector<HTMLButtonElement>(`[data-tab='${snippetsTab}']`)
        ?.focus();
    });
  }

  const definition = $derived(toolById(tool));
  const group = $derived(floatingWorld?.groups.find((item) => item.members.includes(tool)) ?? null);
  const grouped = $derived(Boolean(group));
  const status = $derived(
    grouped
      ? `Unido a ${Math.max(1, (group?.members.length ?? 1) - 1)} superficie${(group?.members.length ?? 1) > 2 ? "s" : ""}`
      : tool === "meetings" && recording
      ? "Grabando"
      : tool === "dictation" && dictation !== "idle"
        ? dictation === "listening" ? "Escuchando" : "Procesando"
        : "Superficie persistente",
  );

  function toolFromWindowLabel(label: string): ToolId {
    const candidate = label.replace(/^pearl-tool-/, "") as ToolId;
    return TOOLS.some((item) => item.id === candidate) ? candidate : "meetings";
  }

  async function refreshClipboard() {
    loading = true;
    try {
      clipboardItems = await listClipboardHistory();
    } catch (cause) {
      error = String(cause);
    } finally {
      loading = false;
    }
  }

  async function refreshSnippets() {
    loading = true;
    try {
      [snippets, scratchpad] = await Promise.all([
        listSnippets(),
        getScratchpad().then((value) => value.body),
      ]);
    } catch (cause) {
      error = String(cause);
    } finally {
      loading = false;
    }
  }

  async function refreshCaptures() {
    loading = true;
    try {
      captures = await listRecentCaptures();
    } catch (cause) {
      error = String(cause);
    } finally {
      loading = false;
    }
  }

  async function run(action: () => Promise<unknown>) {
    if (busy) return;
    busy = true;
    error = null;
    try {
      await action();
    } catch (cause) {
      error = String(cause);
    } finally {
      busy = false;
    }
  }

  async function startToolDrag(event: PointerEvent) {
    if (event.button !== 0 || (event.target as HTMLElement).closest("button")) return;
    event.preventDefault();
    try {
      await beginFloatingDrag(tool, event.altKey);
      await getCurrentWindow().startDragging();
    } catch (cause) {
      error = String(cause);
    }
  }

  async function detachSurface() {
    try {
      await detachFloatingObject(tool);
      floatingWorld = await getFloatingWorld();
    } catch (cause) {
      error = String(cause);
    }
  }

  function toggleMeeting() {
    void run(async () => {
      if (recording) await stopRecording();
      else await startRecording();
    });
  }

  function scheduleScratchSave() {
    if (scratchTimer) clearTimeout(scratchTimer);
    scratchTimer = setTimeout(() => {
      scratchTimer = null;
      scratchSaving = true;
      void setScratchpad(scratchpad)
        .catch((cause) => (error = String(cause)))
        .finally(() => (scratchSaving = false));
    }, 500);
  }

  function labelForCapture(item: CaptureItem) {
    if (item.label?.trim()) return item.label;
    return (item.path.split(/[/\\]/).pop() ?? item.path).replace(/^Atic_/, "").replace(/\.png$/i, "");
  }

  onMount(() => {
    tool = toolFromWindowLabel(getCurrentWindow().label);
    const unlisteners: Promise<UnlistenFn>[] = [];
    void getFloatingWorld().then((world) => (floatingWorld = world));
    unlisteners.push(onFloatingWorldChanged((world) => (floatingWorld = world)));

    if (tool === "meetings") {
      void isRecording().then((value) => (recording = value));
      unlisteners.push(onStatus((value) => (recording = value.active)));
    } else if (tool === "dictation") {
      void dictationPhase().then((value) => (dictation = value));
      unlisteners.push(onDictationStatus((value) => (dictation = value.phase)));
    } else if (tool === "clipboard") {
      void refreshClipboard();
      unlisteners.push(onClipboardHistoryChanged(() => void refreshClipboard()));
    } else if (tool === "snippets") {
      void refreshSnippets();
      unlisteners.push(onSnippetsChanged(() => void refreshSnippets()));
    } else if (tool === "captures") {
      void refreshCaptures();
    }

    return () => {
      for (const pending of unlisteners) void pending.then((unlisten) => unlisten());
      if (scratchTimer) {
        clearTimeout(scratchTimer);
        void setScratchpad(scratchpad);
      }
    };
  });
</script>

<svelte:head><title>Atic · {definition.label}</title></svelte:head>

<main class:agent-surface={tool === "agents"} class:grouped class="floating-tool own-skin" data-tool={tool}>
  <header
    class="tool-header"
    role="group"
    aria-label={`Mover ${definition.label}`}
    onpointerdown={startToolDrag}
    title="Arrastra para mover · Alt+arrastrar separa"
  >
    <span class="tool-mark"><ToolIcon id={tool} size={18} /></span>
    <div class="tool-heading">
      <strong>{definition.label}</strong>
      <span>{status}</span>
    </div>
    {#if grouped}
      <button
        class="detach-button"
        type="button"
        aria-label={`Separar ${definition.label} del grupo`}
        title="Separar del grupo"
        onclick={detachSurface}
      ><i></i></button>
    {/if}
    <button
      class="close-button"
      type="button"
      aria-label={`Cerrar ${definition.label}`}
      title={`Cerrar ${definition.label}`}
      onclick={() => void closeFloatingTool(tool)}
    ><i></i></button>
  </header>

  <section class="tool-body">
    {#if tool === "meetings"}
      <div class="quick-state">
        <span class:live={recording} class="state-orb"><i></i></span>
        <div><strong>{recording ? "Grabación en curso" : "Reunión lista"}</strong><p>Audio y transcripción guardados localmente.</p></div>
        <button class:danger={recording} type="button" disabled={busy} onclick={toggleMeeting}>
          {recording ? "Detener" : "Empezar"}
        </button>
      </div>
    {:else if tool === "dictation"}
      <div class="quick-state dictation-state">
        <span class:live={dictation === "listening"} class="state-orb voice"><i></i></span>
        <div><strong>{dictation === "idle" ? "Dictado listo" : dictation === "listening" ? "Te escucho…" : "Convirtiendo voz…"}</strong><p>El texto se pega en la aplicación activa.</p></div>
        <button type="button" disabled={busy || dictation === "transcribing"} onclick={() => void run(toggleDictation)}>
          {dictation === "listening" ? "Terminar" : "Dictar"}
        </button>
      </div>
    {:else if tool === "clipboard"}
      <ClipboardHistoryList
        items={clipboardItems}
        {loading}
        compact
        onRefresh={refreshClipboard}
        onPasted={() => {}}
        onError={(message) => (error = message)}
      />
    {:else if tool === "snippets"}
      <div class="tabs" role="tablist" aria-label="Textos y notas">
        <button id="floating-snippets-tab-texts" data-tab="texts" class:active={snippetsTab === "texts"} type="button" role="tab" aria-selected={snippetsTab === "texts"} aria-controls="floating-snippets-panel" tabindex={snippetsTab === "texts" ? 0 : -1} onclick={() => (snippetsTab = "texts")} onkeydown={moveSnippetsTab}>Textos</button>
        <button id="floating-snippets-tab-scratchpad" data-tab="scratchpad" class:active={snippetsTab === "scratchpad"} type="button" role="tab" aria-selected={snippetsTab === "scratchpad"} aria-controls="floating-snippets-panel" tabindex={snippetsTab === "scratchpad" ? 0 : -1} onclick={() => (snippetsTab = "scratchpad")} onkeydown={moveSnippetsTab}>Bloc</button>
      </div>
      <div id="floating-snippets-panel" role="tabpanel" aria-labelledby={`floating-snippets-tab-${snippetsTab}`} class="snippets-panel">
      {#if snippetsTab === "texts"}
        <SnippetsList items={snippets} {loading} compact onRefresh={refreshSnippets} onPasted={() => {}} onError={(message) => (error = message)} />
      {:else}
        <label class="scratchpad">
          <span>{scratchSaving ? "Guardando…" : "Se guarda automáticamente"}</span>
          <textarea bind:value={scratchpad} oninput={scheduleScratchSave} placeholder="Escribe una nota temporal…"></textarea>
        </label>
      {/if}
      </div>
    {:else if tool === "captures"}
      <div class="capture-actions">
        <button type="button" disabled={busy} onclick={() => void run(startCaptureSession)}><ToolIcon id="captures" size={18} /><span><strong>Nueva captura</strong><small>Área, ventana o monitor</small></span></button>
        <div class="recent-captures" aria-label="Capturas recientes">
          {#if loading}<p>Cargando…</p>
          {:else if captures.length === 0}<p>Aún no hay capturas.</p>
          {:else}
            {#each captures.slice(0, 3) as item (item.path)}
              <button type="button" title={labelForCapture(item)} onclick={() => void activateCapture(item.path).catch((cause) => (error = String(cause)))}>
                <img src={convertFileSrc(item.path)} alt="" /><span>{labelForCapture(item)}</span>
              </button>
            {/each}
          {/if}
        </div>
      </div>
    {:else if tool === "agents"}
      <FloatingAgents />
    {/if}
  </section>

  {#if error}<p class="surface-error" role="alert">{error}</p>{/if}
</main>

<style>
  :global(html), :global(body) {
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: transparent;
  }

  .floating-tool {
    --pearl-ink: oklch(0.22 0.012 255);
    --pearl-muted: oklch(0.48 0.014 255);
    --pearl-line: oklch(0.22 0.012 255 / 0.1);
    --pearl-soft: oklch(0.22 0.012 255 / 0.055);
    --pearl-signal: oklch(0.62 0.18 252);
    --pearl-record: oklch(0.61 0.2 28);
    /* Mismos tonos que LiquidSkin, para que el fondo propio y el del
       compositor sean el mismo pearl. */
    --pearl-top: oklch(1 0 0);
    --pearl-mid: oklch(0.965 0.006 255);
    --pearl-deep: oklch(0.915 0.009 255);
    --pearl-highlight: oklch(1 0 0 / 0.72);
    position: relative;
    display: grid;
    width: 100%;
    height: 100%;
    min-width: 0;
    grid-template-rows: 58px minmax(0, 1fr);
    color: var(--pearl-ink);
    font: 13px/1.4 "Aptos", "Segoe UI Variable", system-ui, sans-serif;
  }

  /* El cuerpo vive siempre junto al contenido. El compositor global dibuja
     únicamente los cuellos que atraviesan el espacio entre ventanas. */
  .floating-tool.own-skin {
    overflow: hidden;
    border-radius: 15px;
    background: linear-gradient(
      to bottom right,
      var(--pearl-top),
      var(--pearl-mid) 58%,
      var(--pearl-deep)
    );
    box-shadow: inset 0 0 0 1px var(--pearl-highlight);
  }

  /* Coincide con `radius_for` en liquid_world.rs. */
  .floating-tool.own-skin.agent-surface {
    border-radius: 16px;
  }

  /* La piel global aporta el borde continuo cuando forma parte de un grupo. */
  .floating-tool.own-skin.grouped {
    box-shadow: none;
  }

  .tool-header {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 10px;
    padding: 10px 12px 8px 16px;
    cursor: grab;
  }

  .tool-header:active { cursor: grabbing; }
  .tool-mark { display: grid; width: 34px; height: 34px; flex: 0 0 auto; place-items: center; border-radius: 50%; background: var(--pearl-soft); }
  .tool-heading { display: flex; min-width: 0; flex: 1; flex-direction: column; }
  .tool-heading strong { font-size: 13px; font-weight: 680; }
  .tool-heading span { margin-top: 1px; overflow: hidden; color: var(--pearl-muted); font-size: 10px; text-overflow: ellipsis; white-space: nowrap; }

  .close-button {
    position: relative;
    width: 36px;
    height: 36px;
    flex: 0 0 auto;
    border: 0;
    border-radius: 50%;
    color: var(--pearl-muted);
    background: transparent;
    cursor: pointer;
  }
  .detach-button {
    position: relative;
    display: grid;
    width: 36px;
    height: 36px;
    flex: 0 0 auto;
    place-items: center;
    border: 0;
    border-radius: 50%;
    color: var(--pearl-muted);
    background: transparent;
    cursor: pointer;
  }
  .detach-button:hover { color: var(--pearl-ink); background: var(--pearl-soft); }
  .detach-button:focus-visible { outline: 2px solid var(--pearl-signal); outline-offset: -3px; }
  .detach-button i, .detach-button i::after { width: 9px; height: 9px; border: 1.5px solid currentColor; border-radius: 50%; content: ""; }
  .detach-button i { transform: translate(-3px, -2px); }
  .detach-button i::after { display: block; transform: translate(7px, 5px); background: var(--pearl-soft); }
  .close-button:hover { color: var(--pearl-ink); background: var(--pearl-soft); }
  .close-button:focus-visible { outline: 2px solid var(--pearl-signal); outline-offset: -3px; }
  .close-button i::before, .close-button i::after { position: absolute; top: 17px; left: 11px; width: 14px; height: 1.5px; border-radius: 2px; background: currentColor; content: ""; }
  .close-button i::before { transform: rotate(45deg); }
  .close-button i::after { transform: rotate(-45deg); }

  .tool-body { min-width: 0; min-height: 0; overflow: auto; padding: 8px 16px 16px; }
  .agent-surface .tool-body { overflow: hidden; padding-top: 4px; }
  .agent-surface .tool-body :global(.floating-agents) { height: 100%; }

  .quick-state { display: grid; min-height: 100%; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 12px; }
  .quick-state strong { display: block; font-size: 14px; }
  .quick-state p { margin: 3px 0 0; color: var(--pearl-muted); font-size: 11px; }
  .quick-state button, .capture-actions > button { min-height: 38px; border: 0; border-radius: 999px; padding: 0 15px; color: white; background: var(--pearl-ink); font: inherit; font-weight: 650; cursor: pointer; }
  .quick-state button.danger { background: var(--pearl-record); }
  button:disabled { opacity: 0.48; cursor: not-allowed; }
  .state-orb { display: grid; width: 44px; height: 44px; place-items: center; border-radius: 50%; background: var(--pearl-soft); }
  .state-orb i { width: 11px; height: 11px; border: 2px solid var(--pearl-muted); border-radius: 50%; }
  .state-orb.live { background: color-mix(in oklch, var(--pearl-record) 15%, transparent); }
  .state-orb.live i { border-color: var(--pearl-record); background: var(--pearl-record); }

  .tabs { display: flex; gap: 4px; margin-bottom: 9px; padding: 3px; border-radius: 10px; background: var(--pearl-soft); }
  .tabs button { min-height: 32px; flex: 1; border: 0; border-radius: 7px; color: var(--pearl-muted); background: transparent; font: inherit; cursor: pointer; }
  .tabs button.active { color: var(--pearl-ink); background: oklch(1 0 0 / 0.52); }
  .tabs button:focus-visible { outline: 2px solid var(--pearl-signal); }
  .snippets-panel { display: flex; height: calc(100% - 47px); min-height: 0; flex-direction: column; }
  .scratchpad { display: grid; height: 100%; grid-template-rows: auto 1fr; gap: 6px; color: var(--pearl-muted); font-size: 10px; }
  .scratchpad textarea { min-height: 180px; resize: none; border: 0; border-radius: 10px; padding: 12px; color: var(--pearl-ink); background: var(--pearl-soft); font: 13px/1.5 inherit; outline: none; }
  .scratchpad textarea:focus { box-shadow: inset 0 0 0 2px var(--pearl-signal); }

  .capture-actions { display: grid; gap: 12px; }
  .capture-actions > button { display: flex; width: 100%; justify-content: center; align-items: center; gap: 9px; border-radius: 10px; }
  .capture-actions > button span { display: flex; flex-direction: column; text-align: left; }
  .capture-actions small { font-size: 9px; font-weight: 450; opacity: 0.72; }
  .recent-captures { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 7px; }
  .recent-captures > button { min-width: 0; border: 0; border-radius: 8px; padding: 4px; color: var(--pearl-muted); background: var(--pearl-soft); cursor: pointer; }
  .recent-captures img { display: block; width: 100%; aspect-ratio: 16 / 9; border-radius: 5px; object-fit: cover; }
  .recent-captures span { display: block; margin-top: 4px; overflow: hidden; font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
  .recent-captures p { grid-column: 1 / -1; margin: 8px 0; color: var(--pearl-muted); text-align: center; }

  .surface-error { position: absolute; right: 12px; bottom: 10px; left: 12px; margin: 0; border-radius: 8px; padding: 7px 10px; color: oklch(0.34 0.18 28); background: oklch(0.93 0.05 28 / 0.96); font-size: 10px; }

  :global(:root[data-theme="dark"]) .floating-tool {
    --pearl-ink: oklch(0.92 0.008 255);
    --pearl-muted: oklch(0.7 0.012 255);
    --pearl-line: oklch(1 0 0 / 0.1);
    --pearl-soft: oklch(1 0 0 / 0.07);
    --pearl-top: oklch(0.3 0.01 255);
    --pearl-mid: oklch(0.245 0.012 255);
    --pearl-deep: oklch(0.19 0.012 255);
    --pearl-highlight: oklch(1 0 0 / 0.16);
  }

  :global(:root[data-theme="dark"]) .tabs button.active {
    background: oklch(1 0 0 / 0.1);
  }

  :global(:root[data-theme="dark"]) .quick-state button,
  :global(:root[data-theme="dark"]) .capture-actions > button {
    color: oklch(0.17 0.01 255);
    background: oklch(0.91 0.008 255);
  }

  @media (prefers-reduced-motion: reduce) {
    *, *::before, *::after { scroll-behavior: auto !important; transition-duration: 0.1ms !important; animation-duration: 0.1ms !important; }
  }
</style>
