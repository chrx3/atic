<script lang="ts">
  import HotkeyCapture from "$lib/HotkeyCapture.svelte";
  import ToolPageShell from "$lib/ToolPageShell.svelte";
  import type { DictationPhase } from "$lib/types";
  import { toolById } from "$lib/tools";

  let {
    phase,
    message,
    shortcut,
    pillShortcut = "CmdOrCtrl+Shift+P",
    mode = "toggle",
    busy = false,
    onToggle,
    onShortcutChange,
    onPillShortcutChange,
    onModeChange,
    onOpenSettings,
  }: {
    phase: DictationPhase;
    message: string | null;
    shortcut: string;
    pillShortcut?: string;
    mode?: string;
    busy?: boolean;
    onToggle: () => void;
    onShortcutChange: (shortcut: string) => void | Promise<void>;
    onPillShortcutChange: (shortcut: string) => void | Promise<void>;
    onModeChange: (mode: string) => void | Promise<void>;
    onOpenSettings: () => void;
  } = $props();

  const tool = toolById("dictation");

  const statusLabel = $derived.by(() => {
    switch (phase) {
      case "listening":
        return "Escuchando…";
      case "transcribing":
        return "Transcribiendo…";
      case "pasted":
        return message ?? "Texto pegado";
      case "error":
        return message ?? "Error de dictado";
      default:
        return "Listo";
    }
  });

  const live = $derived(
    phase === "listening" ||
      phase === "transcribing" ||
      phase === "pasted" ||
      phase === "error",
  );
</script>

<ToolPageShell {tool} dataDir="data">
  {#snippet actions()}
    <button type="button" class="rb-btn rb-btn-soft" onclick={onOpenSettings}>
      Ajustes de dictado
    </button>
  {/snippet}

  {#snippet prefs()}
    <div class="atic-shortcut-row">
      <div>
        <p class="atic-shortcut-label">Atajo de dictado</p>
        <p class="atic-shortcut-hint">
          {#if mode === "push_to_talk"}
            Mantén para hablar.
          {:else}
            Pulsa para iniciar o parar.
          {/if}
        </p>
      </div>
      <HotkeyCapture
        value={shortcut || "CmdOrCtrl+Shift+D"}
        defaultValue="CmdOrCtrl+Shift+D"
        ariaLabel="Cambiar atajo de dictado"
        onChange={onShortcutChange}
      />
    </div>

    <div class="atic-shortcut-row">
      <label class="atic-shortcut-label" for="dict-mode">Modo</label>
      <select
        id="dict-mode"
        class="rb-field dict-mode-select"
        value={mode}
        onchange={(e) => onModeChange(e.currentTarget.value)}
      >
        <option value="push_to_talk">Push-to-talk (mantener)</option>
        <option value="toggle">Toggle (pulsar para iniciar/parar)</option>
      </select>
    </div>

    <div class="atic-shortcut-row">
      <p class="atic-shortcut-label">Traer pill al cursor</p>
      <HotkeyCapture
        value={pillShortcut || "CmdOrCtrl+Shift+P"}
        defaultValue="CmdOrCtrl+Shift+P"
        ariaLabel="Cambiar atajo para traer la pill al cursor"
        onChange={onPillShortcutChange}
      />
    </div>
  {/snippet}

  <div class="dict-card" class:is-live={live}>
    <div class="dict-status">
      <span
        class="dict-dot"
        class:on={phase === "listening"}
        aria-hidden="true"
      ></span>
      <div>
        <strong>{statusLabel}</strong>
        <p>Habla y pega texto en la app enfocada.</p>
      </div>
    </div>

    <button
      type="button"
      class="rb-btn rb-btn-primary"
      class:is-active={phase === "listening"}
      disabled={busy || phase === "transcribing"}
      onclick={onToggle}
    >
      {phase === "listening" ? "Detener" : "Dictar ahora"}
    </button>
  </div>

  <ul class="dict-notes">
    <li>Modelo y micrófono se configuran en Ajustes → Dictado.</li>
    <li>La pill flotante siempre dicta en modo toggle.</li>
  </ul>
</ToolPageShell>

<style>
  .dict-mode-select {
    min-width: 12rem;
    max-width: 100%;
  }

  .dict-card {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.85rem;
    padding: 1rem 1.05rem;
    border: 0;
    border-radius: var(--rb-radius);
    background: var(--rb-surface);
  }

  .dict-card.is-live {
    border-color: color-mix(in srgb, var(--rb-record) 35%, var(--rb-border));
    background: color-mix(in srgb, var(--rb-record-soft) 55%, var(--rb-surface));
  }

  .dict-status {
    display: flex;
    min-width: 0;
    align-items: flex-start;
    gap: 0.7rem;
  }

  .dict-status strong {
    display: block;
    font-size: 0.9375rem;
  }

  .dict-status p {
    margin: 0.2rem 0 0;
    color: var(--rb-muted);
    font-size: 0.8125rem;
  }

  .dict-dot {
    width: 0.55rem;
    height: 0.55rem;
    margin-top: 0.35rem;
    border-radius: 999px;
    background: var(--rb-faint);
  }

  .dict-dot.on {
    background: var(--rb-record);
    box-shadow: 0 0 0 4px color-mix(in srgb, var(--rb-record) 18%, transparent);
  }

  .dict-notes {
    margin: 0;
    padding-left: 1.1rem;
    color: var(--rb-muted);
    font-size: 0.8125rem;
    line-height: 1.5;
  }

  .dict-notes li + li {
    margin-top: 0.25rem;
  }

  :global(.dict-card .rb-btn-primary.is-active) {
    background: var(--rb-record);
  }

  @container atic-main (max-width: 36.999rem) {
    .dict-card {
      flex-direction: column;
      align-items: stretch;
      padding: 0.85rem 0.9rem;
    }

    .dict-card :global(.rb-btn) {
      width: 100%;
    }
  }
</style>
