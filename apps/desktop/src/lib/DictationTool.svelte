<script lang="ts">
  import HotkeyCapture from "$lib/HotkeyCapture.svelte";
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

<section class="dict-tool" aria-label="Dictado">
  <header class="dict-head">
    <p class="dict-kicker">Herramienta</p>
    <h2>{tool.label}</h2>
    <p class="dict-blurb">{tool.blurb}</p>
  </header>

  <div class="dict-card" class:is-live={live}>
    <div class="dict-status">
      <span class="dict-dot" class:on={phase === "listening"} aria-hidden="true"></span>
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

  <div class="dict-prefs">
    <div class="dict-pref">
      <p class="dict-pref-label">Atajo de dictado</p>
      <HotkeyCapture
        value={shortcut || "CmdOrCtrl+Shift+D"}
        defaultValue="CmdOrCtrl+Shift+D"
        ariaLabel="Cambiar atajo de dictado"
        onChange={onShortcutChange}
      />
      <p class="dict-pref-hint">
        {#if mode === "push_to_talk"}
          Mantén el atajo para hablar; al soltar, transcribe y pega.
        {:else}
          Pulsa para empezar, pulsa otra vez para transcribir y pegar.
        {/if}
      </p>
    </div>

    <label class="dict-pref">
      <span class="dict-pref-label">Modo</span>
      <select
        class="rb-field"
        value={mode}
        onchange={(e) => onModeChange(e.currentTarget.value)}
      >
        <option value="push_to_talk">Push-to-talk (mantener)</option>
        <option value="toggle">Toggle (pulsar para iniciar/parar)</option>
      </select>
    </label>

    <div class="dict-pref">
      <p class="dict-pref-label">Traer pastilla al cursor</p>
      <HotkeyCapture
        value={pillShortcut || "CmdOrCtrl+Shift+P"}
        defaultValue="CmdOrCtrl+Shift+P"
        ariaLabel="Cambiar atajo para traer la pastilla al cursor"
        onChange={onPillShortcutChange}
      />
    </div>
  </div>

  <ul class="dict-notes">
    <li>Modelo y micrófono se configuran en Ajustes → Dictado.</li>
    <li>La pastilla flotante siempre dicta en modo toggle.</li>
  </ul>

  <button type="button" class="rb-btn rb-btn-soft" onclick={onOpenSettings}>
    Ajustes de dictado
  </button>
</section>

<style>
  .dict-tool {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    padding: 1.1rem 1.15rem 1.25rem;
  }

  .dict-kicker {
    margin: 0;
    color: var(--rb-muted);
    font-size: 0.6875rem;
    font-weight: 650;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .dict-head h2 {
    margin: 0.15rem 0 0.35rem;
    font-family: var(--rb-display);
    font-size: 1.35rem;
    font-weight: 650;
    letter-spacing: -0.03em;
  }

  .dict-blurb {
    margin: 0;
    max-width: 34rem;
    color: var(--rb-muted);
    font-size: 0.875rem;
    line-height: 1.45;
  }

  .dict-card {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.85rem;
    padding: 1rem 1.05rem;
    border: 1px solid var(--rb-border);
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

  .dict-prefs {
    display: flex;
    flex-direction: column;
    gap: 0.85rem;
    max-width: 28rem;
    padding: 0.9rem 1rem;
    border: 1px solid var(--rb-border);
    border-radius: var(--rb-radius);
    background: var(--rb-surface);
  }

  .dict-pref {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    min-width: 0;
  }

  .dict-pref-label {
    margin: 0;
    color: var(--rb-muted);
    font-size: 0.75rem;
    font-weight: 600;
  }

  .dict-pref-hint {
    margin: 0;
    color: var(--rb-faint);
    font-size: 0.75rem;
    line-height: 1.4;
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
</style>
