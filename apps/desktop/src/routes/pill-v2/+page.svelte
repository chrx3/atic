<script lang="ts">
  import { onMount } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import ClipboardHistoryList from "$lib/ClipboardHistoryList.svelte";
  import SnippetsList from "$lib/SnippetsList.svelte";
  import FloatingExperience from "$lib/floating-v2/FloatingExperience.svelte";
  import { envelopeFor, opensLargeSurface, pivotForTransition } from "$lib/floating-v2/geometry";
  import type { FloatingActivity, FloatingSurface } from "$lib/floating-v2/types";
  import { createStage, growsFirst } from "$lib/pillStage";
  import type { ClipboardItem, DictationPhase, Snippet as TextSnippet } from "$lib/types";
  import {
    dictationPhase,
    isRecording,
    listClipboardHistory,
    listSnippets,
    onClipboardHistoryChanged,
    onDictationStatus,
    onPillClipboardClose,
    onPillClipboardToggle,
    onPillRadialPress,
    onPillReset,
    onPillSnippetsClose,
    onPillSnippetsToggle,
    onSnippetsChanged,
    onStatus,
    showAgentsWindow,
    showMainWindow,
    startCaptureSession,
    startRecording,
    stopRecording,
    summonPillHere,
    toggleDictation,
  } from "$lib/api";

  const stage = createStage("pill");
  const MORPH_MS = 280;

  let surface = $state<FloatingSurface>({ kind: "pill" });
  let activity = $state<FloatingActivity>("idle");
  let dictation = $state<DictationPhase>("idle");
  let openedUp = $state(false);
  let transitioning = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let clipboardItems = $state<ClipboardItem[]>([]);
  let clipboardLoading = $state(false);
  let snippetItems = $state<TextSnippet[]>([]);
  let snippetsLoading = $state(false);

  function sameSurface(a: FloatingSurface, b: FloatingSurface): boolean {
    if (a.kind !== b.kind) return false;
    if (a.kind === "tool" && b.kind === "tool") return a.tool === b.tool;
    return true;
  }

  async function refreshClipboard() {
    clipboardLoading = true;
    try {
      clipboardItems = await listClipboardHistory();
    } catch (cause) {
      error = String(cause);
    } finally {
      clipboardLoading = false;
    }
  }

  async function refreshSnippets() {
    snippetsLoading = true;
    try {
      snippetItems = await listSnippets();
    } catch (cause) {
      error = String(cause);
    } finally {
      snippetsLoading = false;
    }
  }

  async function prepareSurface(next: FloatingSurface) {
    if (next.kind !== "tool") return;
    if (next.tool === "clipboard") await refreshClipboard();
    if (next.tool === "snippets") await refreshSnippets();
  }

  async function setSurface(next: FloatingSurface) {
    if (transitioning || sameSurface(surface, next)) return;
    transitioning = true;
    error = null;

    const from = surface;
    const fromSize = envelopeFor(from);
    const nextSize = envelopeFor(next);
    const pivot = pivotForTransition(from, next, openedUp);

    try {
      await prepareSurface(next);
      if (growsFirst(fromSize, nextSize)) {
        const outcome = await stage.resize(nextSize, pivot);
        if (!outcome.ok) return;
        openedUp = outcome.up;
        surface = next;
      } else {
        surface = next;
        await new Promise((resolve) => window.setTimeout(resolve, MORPH_MS));
        const outcome = await stage.resize(nextSize, pivot);
        if (outcome.ok && opensLargeSurface(from)) openedUp = false;
      }
    } catch (cause) {
      error = String(cause);
    } finally {
      transitioning = false;
    }
  }

  async function changeActivity(next: FloatingActivity) {
    if (busy || next === activity) return;
    busy = true;
    error = null;
    try {
      if (next === "recording") await startRecording();
      else if (activity === "recording") await stopRecording();
      else await toggleDictation();
    } catch (cause) {
      error = String(cause);
    } finally {
      busy = false;
    }
  }

  async function runCapture() {
    await setSurface({ kind: "pill" });
    try {
      await startCaptureSession();
    } catch (cause) {
      error = String(cause);
    }
  }

  async function openAgents() {
    await setSurface({ kind: "pill" });
    try {
      await showAgentsWindow();
    } catch (cause) {
      error = String(cause);
    }
  }

  onMount(() => {
    const unlisteners: Promise<UnlistenFn>[] = [];
    let dragOrigin: { x: number; y: number } | null = null;

    void stage.resize(envelopeFor(surface), "topCenter");
    void Promise.all([isRecording(), dictationPhase()]).then(([recording, phase]) => {
      dictation = phase;
      activity = recording ? "recording" : phase === "idle" ? "idle" : "dictating";
    });

    unlisteners.push(
      onStatus((status) => {
        activity = status.active ? "recording" : dictation === "idle" ? "idle" : "dictating";
      }),
      onDictationStatus((status) => {
        dictation = status.phase;
        if (activity !== "recording") activity = status.phase === "idle" ? "idle" : "dictating";
      }),
      onClipboardHistoryChanged(() => {
        if (surface.kind === "tool" && surface.tool === "clipboard") void refreshClipboard();
      }),
      onSnippetsChanged(() => {
        if (surface.kind === "tool" && surface.tool === "snippets") void refreshSnippets();
      }),
      onPillClipboardToggle(() => void setSurface({ kind: "tool", tool: "clipboard" })),
      onPillSnippetsToggle(() => void setSurface({ kind: "tool", tool: "snippets" })),
      onPillClipboardClose(() => void setSurface({ kind: "pill" })),
      onPillSnippetsClose(() => void setSurface({ kind: "pill" })),
      onPillRadialPress(() => void setSurface({ kind: "wheel" })),
      onPillReset(async () => {
        await setSurface({ kind: "pill" });
        await summonPillHere().catch(() => {});
      }),
    );

    const onPointerDown = (event: PointerEvent) => {
      if (event.button !== 0 || !(event.target as HTMLElement).closest(".pill-control")) return;
      dragOrigin = { x: event.screenX, y: event.screenY };
    };
    const onPointerMove = (event: PointerEvent) => {
      if (!dragOrigin || Math.hypot(event.screenX - dragOrigin.x, event.screenY - dragOrigin.y) < 4) return;
      dragOrigin = null;
      void getCurrentWindow().startDragging();
    };
    const clearDrag = () => (dragOrigin = null);
    const onDoubleClick = (event: MouseEvent) => {
      if ((event.target as HTMLElement).closest(".pill-control")) void showMainWindow();
    };

    window.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", clearDrag);
    window.addEventListener("pointercancel", clearDrag);
    window.addEventListener("dblclick", onDoubleClick);

    return () => {
      for (const pending of unlisteners) void pending.then((unlisten) => unlisten());
      window.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", clearDrag);
      window.removeEventListener("pointercancel", clearDrag);
      window.removeEventListener("dblclick", onDoubleClick);
    };
  });
</script>

<svelte:head><title>Atic · Pearl Flow</title></svelte:head>

{#snippet productionContent(current: FloatingSurface)}
  {#if current.kind === "tool" && current.tool === "meetings"}
    <div class="quick-action">
      <span class:recording={activity === "recording"} class="action-orb"><i></i></span>
      <strong>{activity === "recording" ? "Grabación en curso" : "Reunión lista"}</strong>
      <p>{activity === "recording" ? "Atic está registrando el audio y la transcripción." : "El audio y la transcripción se guardan localmente."}</p>
      <button class:danger={activity === "recording"} type="button" disabled={busy} onclick={() => void changeActivity(activity === "recording" ? "idle" : "recording")}>
        {activity === "recording" ? "Detener grabación" : "Empezar reunión"}
      </button>
    </div>
  {:else if current.kind === "tool" && current.tool === "dictation"}
    <div class="quick-action">
      <span class:listening={activity === "dictating"} class="action-orb voice"><i></i><i></i><i></i></span>
      <strong>{activity === "dictating" ? "Te escucho…" : "Dictado listo"}</strong>
      <p>El resultado se pega en la aplicación que estabas usando.</p>
      <button type="button" disabled={busy || activity === "recording"} onclick={() => void changeActivity(activity === "dictating" ? "idle" : "dictating")}>
        {activity === "dictating" ? "Terminar dictado" : "Empezar a dictar"}
      </button>
    </div>
  {:else if current.kind === "tool" && current.tool === "clipboard"}
    <ClipboardHistoryList
      items={clipboardItems}
      loading={clipboardLoading}
      compact
      onRefresh={refreshClipboard}
      onPasted={() => void setSurface({ kind: "pill" })}
      onError={(message) => (error = message)}
    />
  {:else if current.kind === "tool" && current.tool === "snippets"}
    <SnippetsList
      items={snippetItems}
      loading={snippetsLoading}
      compact
      onRefresh={refreshSnippets}
      onPasted={() => void setSurface({ kind: "pill" })}
      onError={(message) => (error = message)}
    />
  {:else if current.kind === "tool" && current.tool === "captures"}
    <div class="quick-action">
      <span class="action-orb capture"><i></i></span>
      <strong>Capturar pantalla</strong>
      <p>Selecciona un área, una ventana o un monitor en el overlay.</p>
      <button type="button" onclick={() => void runCapture()}>Elegir área</button>
    </div>
  {:else if current.kind === "agents"}
    <div class="quick-action agents-action">
      <span class="action-orb agent"><i>&gt;_</i></span>
      <strong>Espacio de agentes</strong>
      <p>La conversación real sigue disponible mientras migramos su superficie al renderer compartido.</p>
      <button type="button" onclick={() => void openAgents()}>Abrir agentes</button>
    </div>
  {/if}
{/snippet}

<main class:opened-up={openedUp} class="pill-v2-host">
  <FloatingExperience
    {surface}
    {activity}
    layout="window"
    interactive={!transitioning}
    content={productionContent}
    onSurfaceChange={(next) => void setSurface(next)}
    onActivityChange={(next) => void changeActivity(next)}
  />
  {#if error}<p class="host-error" role="status">{error}</p>{/if}
</main>

<style>
  :global(html), :global(body) {
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: transparent;
  }

  .pill-v2-host {
    position: relative;
    width: 100%;
    height: 100%;
  }

  .host-error {
    position: fixed;
    right: 18px;
    bottom: 12px;
    left: 18px;
    z-index: 5;
    margin: 0;
    padding: 7px 10px;
    border: 1px solid color-mix(in oklch, var(--rb-record) 28%, transparent);
    border-radius: 9px;
    color: var(--rb-record);
    background: color-mix(in oklch, var(--rb-record-soft) 92%, transparent);
    font-size: 10px;
    text-align: center;
  }

  .quick-action {
    display: flex;
    height: 100%;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
  }

  .quick-action strong {
    margin-top: 14px;
    color: oklch(0.22 0.012 255);
    font-size: 17px;
    letter-spacing: -0.025em;
  }

  .quick-action p {
    max-width: 270px;
    margin: 7px 0 18px;
    color: oklch(0.48 0.014 255);
    font-size: 11px;
    line-height: 1.45;
  }

  .quick-action button {
    min-height: 38px;
    padding: 0 17px;
    border: 0;
    border-radius: 999px;
    color: white;
    background: oklch(0.22 0.012 255);
    font: inherit;
    font-size: 11px;
    font-weight: 650;
    cursor: pointer;
  }

  .quick-action button.danger { background: oklch(0.61 0.2 28); }
  .quick-action button:disabled { opacity: 0.45; cursor: wait; }

  .action-orb {
    display: grid;
    width: 70px;
    height: 70px;
    place-items: center;
    border-radius: 50%;
    color: oklch(0.22 0.012 255);
    background: oklch(0.915 0.009 255);
  }

  .action-orb > i {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: currentColor;
  }

  .action-orb.recording { color: oklch(0.61 0.2 28); }
  .action-orb.recording > i { border-radius: 3px; }

  .action-orb.voice {
    display: flex;
    gap: 4px;
    align-items: center;
    justify-content: center;
  }
  .action-orb.voice i { width: 2px; height: 18px; border-radius: 2px; }
  .action-orb.voice i:nth-child(2) { height: 30px; }
  .action-orb.voice.listening { color: oklch(0.62 0.18 252); }
  .action-orb.voice.listening i { animation: listening 800ms ease-in-out infinite alternate; }

  .action-orb.capture > i { width: 30px; height: 24px; border: 1.5px dashed currentColor; border-radius: 4px; background: transparent; }
  .action-orb.agent > i { width: auto; height: auto; border-radius: 0; background: transparent; font: 13px var(--rb-mono, monospace); }

  @keyframes listening { to { transform: scaleY(0.45); } }

  @media (prefers-reduced-motion: reduce) {
    .action-orb.voice.listening i { animation: none; }
  }
</style>
