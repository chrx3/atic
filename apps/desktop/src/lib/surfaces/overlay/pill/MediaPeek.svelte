<script lang="ts">
  /**
   * El reproductor, como vistazo de la isla.
   *
   * El botón de la tira solo pausa o reanuda; acá está el resto: qué suena,
   * de qué app, los saltos y cuánto va. Horizontal arriba y abajo (carátula
   * al lado del texto); al costado, en columna (carátula arriba, a lo ancho).
   */
  import Icon from "$ui/Icon.svelte";
  import {
    AudioLines,
    Pause,
    Play,
    RotateCcw,
    RotateCw,
    SkipBack,
    SkipForward,
    Volume2,
    VolumeX,
  } from "$lib/icons";
  import OverlaySlider from "$surfaces/overlay/OverlaySlider.svelte";
  import { t } from "$domain/i18n.svelte";
  import { media } from "$domain/media.svelte";
  import { mediaClock, mediaPosition } from "$domain/mediaTime";
  import { lyricIndex } from "$domain/lyrics";

  let { vertical = false }: { vertical?: boolean } = $props();

  const SEEK_STEP_MS = 10_000;

  /** Reloj de la barra: la posición se extrapola entre muestras. */
  let at = $state(Date.now());
  /** Mientras se arrastra la barra: dónde está la mano, aún sin mandar. */
  let scrub = $state<number | null>(null);

  $effect(() => media.watch());
  $effect(() => media.watchVolume());

  $effect(() => {
    if (!media.now?.playing) return;
    // La letra cambia de verso al segundo: con ella, el reloj va más fino.
    const timer = setInterval(() => (at = Date.now()), media.lyrics ? 250 : 500);
    return () => clearInterval(timer);
  });

  const now = $derived(media.now);
  const position = $derived(scrub ?? (now ? mediaPosition(now, at) : null));
  const duration = $derived(now?.duration_ms ?? 0);
  /**
   * El nombre de la app, si se puede leer. Los navegadores se registran con un
   * id opaco (`F0DC299D809B9700`): mostrarlo sería peor que no decir nada.
   */
  const appName = $derived(
    now && now.app && !/^[0-9a-f]{12,}$/i.test(now.app) ? now.app : "",
  );
  /**
   * Los saltos, solo si la app los ofrece. Un video del navegador no los tiene
   * y dos botones siempre apagados al lado del play solo estorban.
   */
  const canSkip = $derived(Boolean(now?.can_prev || now?.can_next));
  const canSeek = $derived(Boolean(now?.can_seek));
  const openLabel = $derived(
    appName ? t("pill.peek.mediaOpen", { app: appName }) : t("pill.peek.mediaOpenPlayer"),
  );
  /** El verso que suena y sus vecinos, si el tema tiene letra sincronizada. */
  const lyric = $derived.by(() => {
    const lines = media.lyrics;
    if (!lines || position == null) return null;
    const i = lyricIndex(lines, position);
    return {
      prev: lines[i - 1]?.text ?? "",
      current: i < 0 ? "" : lines[i].text,
      next: lines[i + 1]?.text ?? "",
    };
  });
  const volume = $derived(media.volume);
  /** Dice de quién es el volumen: de la app si se la pudo identificar. */
  const volumeLabel = $derived(
    volume?.app && appName
      ? t("pill.peek.mediaVolumeApp", { app: appName })
      : t("pill.peek.mediaVolumeSystem"),
  );
</script>

{#if now}
  <div class="mp" class:is-vertical={vertical}>
    <button
      type="button"
      class="mp-cover"
      title={openLabel}
      aria-label={openLabel}
      onclick={() => void media.focus()}
    >
      {#if now.thumbnail}
        <img src={now.thumbnail} alt="" draggable="false" />
      {:else}
        <Icon icon={AudioLines} size={vertical ? 28 : 20} />
      {/if}
    </button>

    <div class="mp-main">
      <button
        type="button"
        class="mp-text"
        title={openLabel}
        onclick={() => void media.focus()}
      >
        <span class="mp-title">{now.title || t("pill.peek.mediaUntitled")}</span>
        <span class="mp-sub">
          {now.artist || (appName ? t("pill.peek.mediaFrom", { app: appName }) : "")}
        </span>
      </button>

      <div class="mp-controls">
        {#if canSkip}
          <button
            type="button"
            class="mp-btn"
            disabled={!now.can_prev}
            aria-label={t("pill.peek.mediaPrev")}
            onclick={() => void media.control("prev")}
          >
            <Icon icon={SkipBack} size={15} />
          </button>
        {/if}
        {#if canSeek}
          <button
            type="button"
            class="mp-btn"
            aria-label={t("pill.peek.mediaBack")}
            title={t("pill.peek.mediaBack")}
            onclick={() => void media.seekBy(-SEEK_STEP_MS)}
          >
            <Icon icon={RotateCcw} size={14} />
          </button>
        {/if}
        <button
          type="button"
          class="mp-btn is-main"
          disabled={!now.can_toggle}
          aria-label={now.playing
            ? t("pill.peek.mediaPause")
            : t("pill.peek.mediaPlay")}
          onclick={() => void media.control("toggle")}
        >
          <Icon icon={now.playing ? Pause : Play} size={16} />
        </button>
        {#if canSeek}
          <button
            type="button"
            class="mp-btn"
            aria-label={t("pill.peek.mediaForward")}
            title={t("pill.peek.mediaForward")}
            onclick={() => void media.seekBy(SEEK_STEP_MS)}
          >
            <Icon icon={RotateCw} size={14} />
          </button>
        {/if}
        {#if canSkip}
          <button
            type="button"
            class="mp-btn"
            disabled={!now.can_next}
            aria-label={t("pill.peek.mediaNext")}
            onclick={() => void media.control("next")}
          >
            <Icon icon={SkipForward} size={15} />
          </button>
        {/if}
      </div>
    </div>

    {#if position != null && duration > 0}
      <div class="mp-time">
        <span data-numeric>{mediaClock(position)}</span>
        {#if canSeek}
          <OverlaySlider
            label={t("pill.peek.mediaSeek")}
            value={position}
            max={duration}
            step={1000}
            onValue={(v) => (scrub = v)}
            onCommit={(v) => {
              scrub = null;
              void media.seek(v);
            }}
          />
        {:else}
          <span class="mp-track" aria-hidden="true">
            <span class="mp-fill" style:width="{(position / duration) * 100}%"></span>
          </span>
        {/if}
        <span data-numeric>{mediaClock(duration)}</span>
      </div>
    {/if}

    {#if lyric}
      <div class="mp-lyrics" aria-live="off">
        <span class="mp-lyric is-side">{lyric.prev}</span>
        {#key lyric.current}
          <span class="mp-lyric is-current">{lyric.current || "♪"}</span>
        {/key}
        <span class="mp-lyric is-side">{lyric.next}</span>
      </div>
    {/if}

    {#if volume}
      <label class="mp-volume" title={volumeLabel}>
        <Icon icon={volume.level <= 0.001 ? VolumeX : Volume2} size={14} />
        <OverlaySlider
          label={volumeLabel}
          value={volume.level}
          onValue={(v) => void media.setVolume(v)}
        />
      </label>
    {/if}
  </div>
{/if}

<style>
  /* Carátula a un lado y texto + controles al otro; la barra, a lo ancho. */
  .mp {
    display: grid;
    align-items: center;
    grid-template-columns: 3.5rem minmax(0, 1fr);
    gap: 0.5rem 0.7rem;
  }

  .mp-cover {
    display: grid;
    border: 0;
    padding: 0;
    cursor: pointer;
    width: 3.5rem;
    height: 3.5rem;
    overflow: hidden;
    place-items: center;
    border-radius: 10px;
    background: color-mix(in sRGB, var(--text) 8%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--text) 10%, transparent);
    color: var(--muted);
  }

  .mp-cover:focus-visible,
  .mp-text:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .mp-cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .mp-main {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 0.3rem;
  }

  .mp-text {
    min-width: 0;
    border: 0;
    border-radius: 6px;
    padding: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: inherit;
    cursor: pointer;
  }

  .mp-text:hover .mp-title {
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .mp-title,
  .mp-sub {
    display: block;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mp-title {
    color: var(--text);
    font-weight: 600;
  }

  .mp-sub {
    color: var(--muted);
    font-size: 0.6875rem;
  }

  .mp-controls {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }

  .mp-btn {
    display: grid;
    width: 1.75rem;
    height: 1.75rem;
    place-items: center;
    border: 0;
    border-radius: 999px;
    padding: 0;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    transition:
      background var(--duration-quick, 75ms) var(--ease-smooth-out, ease-out),
      color var(--duration-quick, 75ms) var(--ease-smooth-out, ease-out),
      transform var(--duration-quick, 75ms) var(--ease-smooth-out, ease-out);
  }

  .mp-btn:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--text) 10%, transparent);
    color: var(--text);
  }

  .mp-btn:active:not(:disabled) {
    transform: scale(0.94);
  }

  .mp-btn:disabled {
    cursor: default;
    opacity: 0.35;
  }

  .mp-btn:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  /* Play/pausa manda: lleno, como el botón principal de cualquier reproductor. */
  .mp-btn.is-main {
    width: 2rem;
    height: 2rem;
    background: var(--text);
    color: var(--bg);
  }

  .mp-btn.is-main:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--text) 85%, transparent);
    color: var(--bg);
  }

  .mp-time {
    display: grid;
    align-items: center;
    grid-column: 1 / -1;
    grid-template-columns: auto minmax(0, 1fr) auto;
    gap: 0.45rem;
    color: var(--faint);
    font-size: 0.625rem;
    font-variant-numeric: tabular-nums;
  }

  /*
   * Tres versos de alto fijo (la caja no cambia al avanzar), a lo Apple Music:
   * todos del mismo peso, el actual grande y blanco, los vecinos apagados.
   */
  .mp-lyrics {
    display: grid;
    grid-column: 1 / -1;
    gap: 0.15rem;
    padding: 0.2rem 0;
    font-family: var(--font-sans);
    font-weight: 700;
    letter-spacing: -0.01em;
    text-align: center;
  }

  .mp-lyric {
    min-height: 1.25em;
    overflow: hidden;
    line-height: 1.25;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mp-lyric.is-side {
    color: color-mix(in sRGB, var(--text) 30%, transparent);
    font-size: 0.8125rem;
  }

  /* El verso actual no se corta: parte en dos renglones parejos, con el alto
     de dos reservado para que la caja no salte entre versos cortos y largos. */
  .mp-lyric.is-current {
    display: flex;
    min-height: 2.5em;
    align-items: center;
    justify-content: center;
    color: var(--text);
    font-size: 1rem;
    white-space: normal;
    text-wrap: balance;
    animation: mp-lyric-in var(--duration-slow, 200ms) var(--ease-smooth-out, ease-out);
  }

  @keyframes mp-lyric-in {
    from {
      opacity: 0.3;
      transform: translateY(0.5em);
    }
  }

  .mp-volume {
    display: grid;
    align-items: center;
    grid-column: 1 / -1;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 0.45rem;
    color: var(--muted);
  }

  .mp-track {
    position: relative;
    height: 0.25rem;
    overflow: hidden;
    border-radius: 999px;
    background: color-mix(in sRGB, var(--text) 12%, transparent);
  }

  .mp-fill {
    position: absolute;
    inset: 0 auto 0 0;
    border-radius: inherit;
    background: var(--text);
    transition: width 500ms linear;
  }

  /* Al costado: carátula arriba a lo ancho, todo lo demás centrado debajo. */
  .mp.is-vertical {
    grid-template-columns: minmax(0, 1fr);
    justify-items: stretch;
  }

  .mp.is-vertical .mp-cover {
    width: 100%;
    height: auto;
    aspect-ratio: 1;
  }

  .mp.is-vertical .mp-main {
    align-items: center;
    text-align: center;
  }

  .mp.is-vertical .mp-text {
    width: 100%;
  }

  @media (prefers-reduced-motion: reduce) {
    .mp-btn,
    .mp-fill {
      transition: none;
    }

    .mp-lyric.is-current {
      animation: none;
    }

    .mp-btn:active:not(:disabled) {
      transform: none;
    }
  }
</style>
