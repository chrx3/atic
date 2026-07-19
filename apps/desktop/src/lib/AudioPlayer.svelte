<script lang="ts">
  import { playback } from "$lib/playback.svelte";

  let {
    placeholder = "Selecciona una pista para escuchar",
    alwaysVisible = false,
    dismissible = true,
  }: {
    placeholder?: string;
    alwaysVisible?: boolean;
    dismissible?: boolean;
  } = $props();

  function fmt(secs: number): string {
    if (!Number.isFinite(secs) || secs < 0) return "0:00";
    const m = Math.floor(secs / 60);
    const s = Math.floor(secs % 60);
    return `${m}:${s.toString().padStart(2, "0")}`;
  }

  function seek(e: Event) {
    playback.seek(Number((e.currentTarget as HTMLInputElement).value));
  }

  const pct = $derived(
    playback.duration > 0
      ? Math.min(100, (playback.currentTime / playback.duration) * 100)
      : 0,
  );
</script>

<div
  class="rb-player"
  class:is-hidden={!alwaysVisible && !playback.label}
  aria-label="Reproductor de audio"
>
  {#if alwaysVisible || playback.label}
    <button
      class="rb-player-toggle"
      onclick={() => playback.toggle()}
      disabled={!playback.label || playback.loading}
      aria-label={playback.playing ? "Pausar" : "Reproducir"}
    >
      {#if playback.loading}
        <span class="rb-player-spinner" aria-hidden="true"></span>
      {:else if playback.playing}
        <span class="rb-player-icon-pause" aria-hidden="true"></span>
      {:else}
        <span class="rb-player-icon-play" aria-hidden="true"></span>
      {/if}
    </button>

    <div class="rb-player-meta min-w-0 flex-1">
      <div class="truncate text-xs font-medium" style="color: var(--rb-text)">
        {playback.label ?? placeholder}
      </div>
      <div class="rb-player-scrub">
        <input
          type="range"
          class="rb-player-range"
          min="0"
          max={playback.duration || 0}
          step="0.1"
          value={playback.currentTime}
          oninput={seek}
          style="--pct: {pct}%"
          aria-label="Posición"
          disabled={!playback.label}
        />
        <div class="rb-player-times tabular-nums">
          <span>{fmt(playback.currentTime)}</span>
          <span style="color: var(--rb-faint)">{fmt(playback.duration)}</span>
        </div>
      </div>
      {#if playback.error}
        <p class="rb-player-error" role="alert">{playback.error}</p>
      {/if}
    </div>
    {#if dismissible && playback.label}
      <button
        class="rb-player-close"
        onclick={() => playback.stop()}
        aria-label="Cerrar reproductor"
      >
        <span aria-hidden="true">×</span>
      </button>
    {/if}
  {/if}
</div>

<style>
  .rb-player {
    display: flex;
    align-items: center;
    gap: 0.85rem;
    width: 100%;
  }
  .rb-player.is-hidden {
    display: none;
  }

  .rb-player-toggle {
    display: flex;
    height: 2.25rem;
    width: 2.25rem;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    border-radius: 999px;
    background: color-mix(in srgb, var(--rb-text) 10%, transparent);
    color: var(--rb-text);
    transition: background 0.15s ease;
  }
  .rb-player-toggle:hover {
    background: color-mix(in srgb, var(--rb-text) 16%, transparent);
  }
  .rb-player-toggle:disabled {
    cursor: default;
    opacity: 0.45;
  }

  .rb-player-icon-play {
    width: 0;
    height: 0;
    margin-left: 2px;
    border-style: solid;
    border-width: 5px 0 5px 8px;
    border-color: transparent transparent transparent currentColor;
  }
  .rb-player-icon-pause {
    display: block;
    width: 8px;
    height: 10px;
    background:
      linear-gradient(currentColor, currentColor) left / 2.5px 100% no-repeat,
      linear-gradient(currentColor, currentColor) right / 2.5px 100% no-repeat;
  }
  .rb-player-spinner {
    width: 12px;
    height: 12px;
    border: 2px solid color-mix(in srgb, currentColor 25%, transparent);
    border-top-color: currentColor;
    border-radius: 999px;
    animation: rb-player-spin 0.8s linear infinite;
  }

  .rb-player-scrub {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    margin-top: 0.2rem;
  }

  .rb-player-range {
    -webkit-appearance: none;
    appearance: none;
    width: 100%;
    height: 4px;
    border-radius: 999px;
    background: linear-gradient(
      to right,
      var(--rb-accent) 0%,
      var(--rb-accent) var(--pct),
      color-mix(in srgb, var(--rb-text) 12%, transparent) var(--pct),
      color-mix(in srgb, var(--rb-text) 12%, transparent) 100%
    );
    outline: none;
    cursor: pointer;
  }
  .rb-player-range:disabled {
    cursor: default;
    opacity: 0.45;
  }
  .rb-player-range::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 12px;
    height: 12px;
    border-radius: 999px;
    background: var(--rb-text);
    border: none;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.35);
  }
  .rb-player-range::-moz-range-thumb {
    width: 12px;
    height: 12px;
    border-radius: 999px;
    background: var(--rb-text);
    border: none;
  }

  .rb-player-times {
    display: flex;
    justify-content: space-between;
    font-size: 0.625rem;
    color: var(--rb-muted);
  }
  .rb-player-error {
    margin-top: 0.25rem;
    color: var(--rb-record);
    font-size: 0.6875rem;
  }
  .rb-player-close {
    width: 1.75rem;
    height: 1.75rem;
    flex: 0 0 auto;
    border-radius: var(--rb-radius-xs);
    color: var(--rb-muted);
    background: transparent;
    font-size: 1rem;
  }
  .rb-player-close:hover {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 7%, transparent);
  }
  @keyframes rb-player-spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (max-width: 36rem) {
    .rb-player {
      gap: 0.65rem;
    }

    .rb-player-toggle,
    .rb-player-close {
      width: 2.75rem;
      height: 2.75rem;
    }

    .rb-player-times,
    .rb-player-error {
      font-size: 0.75rem;
    }

    .rb-player-range::-webkit-slider-thumb {
      width: 16px;
      height: 16px;
    }

    .rb-player-range::-moz-range-thumb {
      width: 16px;
      height: 16px;
    }
  }
</style>
