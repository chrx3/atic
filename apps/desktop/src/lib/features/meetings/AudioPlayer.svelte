<script lang="ts">
  /**
   * El reproductor. No tiene estado propio: todo sale del controlador, que es
   * único para la app.
   *
   * Eso es lo que hace que elegir un fragmento en la transcripción y esta barra
   * sean la misma cosa sin que ninguno de los dos conozca al otro.
   */
  import { playback } from "$domain/playback.svelte";
  import Icon from "$ui/Icon.svelte";
  import { Pause, Play, X } from "$lib/icons";

  let {
    placeholder = "Elegí un fragmento para escucharlo",
    alwaysVisible = false,
    dismissible = true,
    onEmptyPlay,
  }: {
    placeholder?: string;
    /** Ocupa su lugar aunque no haya nada cargado. Para barras fijas. */
    alwaysVisible?: boolean;
    dismissible?: boolean;
    /** Si todavía no hay pista cargada, el play arranca por acá. */
    onEmptyPlay?: () => void | Promise<void>;
  } = $props();

  function stamp(seconds: number): string {
    if (!Number.isFinite(seconds) || seconds < 0) return "0:00";
    const m = Math.floor(seconds / 60);
    const s = Math.floor(seconds % 60);
    return `${m}:${s.toString().padStart(2, "0")}`;
  }

  const percent = $derived(
    playback.duration > 0
      ? Math.min(100, (playback.currentTime / playback.duration) * 100)
      : 0,
  );
</script>

{#if alwaysVisible || playback.label}
  <div class="flex w-full items-center gap-3" aria-label="Reproductor de audio">
    <button
      type="button"
      class="grid size-8 shrink-0 place-items-center rounded-pill bg-surface-2 text-text
             transition-colors duration-(--duration-quick) ease-calm
             hover:bg-elevated disabled:opacity-45"
      onclick={() => {
        if (playback.label) void playback.toggle();
        else void onEmptyPlay?.();
      }}
      disabled={(!playback.label && !onEmptyPlay) || playback.loading}
      aria-label={playback.playing ? "Pausar" : "Reproducir"}
    >
      {#if playback.loading}
        <span class="spinner" aria-hidden="true"></span>
      {:else}
        <Icon icon={playback.playing ? Pause : Play} size={12} />
      {/if}
    </button>

    <div class="flex min-w-0 flex-1 flex-col gap-1">
      <p class="truncate text-xs font-medium text-text">
        {playback.label ?? placeholder}
      </p>

      <input
        type="range"
        class="scrub"
        min="0"
        max={playback.duration || 0}
        step="0.1"
        value={playback.currentTime}
        oninput={(event) => playback.seek(Number(event.currentTarget.value))}
        style="--played: {percent}%"
        aria-label="Posición"
        disabled={!playback.label}
      />

      <div class="flex justify-between font-mono text-micro text-muted" data-numeric>
        <span>{stamp(playback.currentTime)}</span>
        <span class="text-faint">{stamp(playback.duration)}</span>
      </div>

      {#if playback.error}
        <p class="text-xs text-danger" role="alert">{playback.error}</p>
      {/if}
    </div>

    {#if dismissible && playback.label}
      <button
        type="button"
        class="grid size-6 shrink-0 place-items-center rounded-xs text-muted
               transition-colors duration-(--duration-quick) ease-calm
               hover:bg-surface-2 hover:text-text"
        onclick={() => playback.stop()}
        aria-label="Cerrar el reproductor"
      >
        <Icon icon={X} size={12} />
      </button>
    {/if}
  </div>
{/if}

<style>
  /* El `<input type="range">` no se puede pintar con utilidades: el relleno y
     la perilla viven en pseudo-elementos con prefijo de motor. */
  .scrub {
    width: 100%;
    height: 4px;
    border-radius: var(--radius-pill);
    background: linear-gradient(
      to right,
      var(--accent) 0%,
      var(--accent) var(--played),
      var(--line-strong) var(--played),
      var(--line-strong) 100%
    );
    outline: none;
    appearance: none;
    cursor: pointer;
  }

  .scrub:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .scrub::-webkit-slider-thumb {
    width: 12px;
    height: 12px;
    border: none;
    border-radius: var(--radius-pill);
    background: var(--text);
    box-shadow: var(--shadow-card);
    appearance: none;
  }

  .scrub::-moz-range-thumb {
    width: 12px;
    height: 12px;
    border: none;
    border-radius: var(--radius-pill);
    background: var(--text);
  }

  .spinner {
    width: 12px;
    height: 12px;
    border: 2px solid var(--line-strong);
    border-top-color: currentColor;
    border-radius: var(--radius-pill);
    animation: spin var(--duration-spin) linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation: none;
    }
  }
</style>
