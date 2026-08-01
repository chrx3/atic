<script lang="ts">
  import AppNavIcon from "$lib/AppNavIcon.svelte";
  import AticMark from "$lib/AticMark.svelte";
  import ToolIcon from "$lib/ToolIcon.svelte";
  import { APP_SECTIONS, type AppSectionId } from "$lib/tools";
  import { formatDate, formatDuration, formatShortcut } from "$lib/format";
  import type { DictationPhase, Recording } from "$lib/types";

  let {
    recording = false,
    elapsed = 0,
    dictation = "idle",
    modelReady = false,
    recordings = [],
    pillShortcut = "",
    onToggleRecording,
    onToggleDictation,
    onSearch,
    onNavigate,
  }: {
    recording?: boolean;
    elapsed?: number;
    dictation?: DictationPhase;
    modelReady?: boolean;
    recordings?: Recording[];
    pillShortcut?: string;
    onToggleRecording: () => void;
    onToggleDictation: () => void;
    onSearch: () => void;
    onNavigate: (section: AppSectionId) => void;
  } = $props();

  const recent = $derived(recordings.slice(0, 3));
  const dictating = $derived(
    dictation === "listening" || dictation === "transcribing",
  );
  const library = APP_SECTIONS.filter(
    (section) => section.id !== "home" && section.id !== "settings",
  );
</script>

<section class="home-control" aria-labelledby="home-title">
  <header class="home-intro">
    <div class="home-mark" aria-hidden="true"><AticMark size={26} /></div>
    <div>
      <h1 id="home-title">Tu espacio de trabajo</h1>
      <p>Inicia una acción o continúa donde la dejaste.</p>
    </div>
    <button type="button" class="home-search" onclick={onSearch}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true">
        <circle cx="10.5" cy="10.5" r="6.5" />
        <path d="m16 16 4 4" />
      </svg>
      Buscar
      <kbd>Ctrl K</kbd>
    </button>
  </header>

  <div class="home-flow" class:is-live={recording || dictating}>
    <div class="home-flow-state">
      <span class="home-orb" class:is-recording={recording} class:is-dictating={dictating}>
        {#if recording}
          <ToolIcon id="meetings" size={22} />
        {:else if dictating}
          <ToolIcon id="dictation" size={22} />
        {:else}
          <AticMark size={22} />
        {/if}
      </span>
      <div>
        <strong>
          {recording
            ? `Grabando · ${formatDuration(elapsed)}`
            : dictating
              ? dictation === "listening"
                ? "Escuchando dictado"
                : "Transcribiendo dictado"
              : "Atic está listo"}
        </strong>
        <p>
          {recording || dictating
            ? "La pill muestra el estado mientras sigues trabajando."
            : modelReady
              ? "Transcripción local preparada."
              : "Configura el modelo local para transcribir sin salir del equipo."}
        </p>
      </div>
    </div>

    <div class="home-flow-actions">
      <button
        type="button"
        class="home-action"
        class:is-stop={recording}
        onclick={onToggleRecording}
      >
        <ToolIcon id="meetings" size={18} />
        {recording ? "Detener" : "Grabar reunión"}
      </button>
      <button
        type="button"
        class="home-action"
        class:is-stop={dictating}
        onclick={onToggleDictation}
      >
        <ToolIcon id="dictation" size={18} />
        {dictating ? "Detener dictado" : "Dictar texto"}
      </button>
    </div>
  </div>

  <div class="home-columns">
    <section class="home-library" aria-labelledby="library-title">
      <div class="home-section-head">
        <h2 id="library-title">Biblioteca</h2>
        <span>Todo permanece en este equipo</span>
      </div>
      <div class="home-library-list">
        {#each library as section (section.id)}
          <button type="button" onclick={() => onNavigate(section.id)}>
            <span class="home-library-icon"><AppNavIcon id={section.id} /></span>
            <span class="home-library-copy">
              <strong>{section.label}</strong>
              <small>{section.blurb}</small>
            </span>
            <span class="home-arrow" aria-hidden="true">→</span>
          </button>
        {/each}
      </div>
    </section>

    <aside class="home-aside">
      <section aria-labelledby="recent-title">
        <div class="home-section-head">
          <h2 id="recent-title">Recientes</h2>
          <button type="button" onclick={() => onNavigate("meetings")}>Ver todas</button>
        </div>
        {#if recent.length === 0}
          <p class="home-empty">Las reuniones aparecerán aquí cuando grabes la primera.</p>
        {:else}
          <ul class="home-recents">
            {#each recent as item (item.id)}
              <li>
                <button type="button" onclick={() => onNavigate("meetings")}>
                  <span>{item.title}</span>
                  <time>{formatDate(item.started_at)}</time>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </section>

      <section class="home-pill-note" aria-label="Pill flotante">
        <span class="home-mini-pill"><AticMark size={16} /></span>
        <div>
          <strong>Pill flotante activa</strong>
          <p>
            {pillShortcut
              ? `Tráela al cursor con ${formatShortcut(pillShortcut)}.`
              : "Disponible sobre cualquier aplicación."}
          </p>
        </div>
      </section>
    </aside>
  </div>
</section>

<style>
  .home-control {
    display: flex;
    width: min(68rem, 100%);
    min-height: 100%;
    margin: 0 auto;
    flex-direction: column;
    gap: 1.25rem;
    padding: 1.2rem 1.35rem 1.5rem;
  }

  .home-intro {
    display: flex;
    align-items: center;
    gap: 0.8rem;
  }

  .home-mark,
  .home-orb,
  .home-library-icon,
  .home-mini-pill {
    display: grid;
    flex: 0 0 auto;
    place-items: center;
    border-radius: 999px;
    color: var(--rb-text);
    background: var(--rb-surface);
  }

  .home-mark {
    width: 2.75rem;
    height: 2.75rem;
  }

  .home-intro h1 {
    margin: 0;
    font-size: 1.125rem;
    font-weight: 650;
    letter-spacing: -0.025em;
  }

  .home-intro p,
  .home-flow-state p,
  .home-pill-note p {
    margin: 0.18rem 0 0;
    color: var(--rb-muted);
    font-size: 0.75rem;
    line-height: 1.4;
  }

  .home-search {
    display: flex;
    min-height: 2.4rem;
    margin-left: auto;
    align-items: center;
    gap: 0.5rem;
    border: 0;
    border-radius: var(--rb-radius-sm);
    padding: 0 0.7rem;
    color: var(--rb-muted);
    background: var(--rb-surface);
    font: inherit;
    font-size: 0.75rem;
    cursor: pointer;
  }

  .home-search kbd {
    margin-left: 0.4rem;
    color: var(--rb-faint);
    font-family: var(--rb-mono);
    font-size: 0.625rem;
  }

  .home-flow {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 1rem 1.1rem;
    border-radius: var(--rb-radius);
    background: var(--rb-surface);
  }

  .home-flow.is-live {
    background: color-mix(in oklch, var(--rb-record-soft) 54%, var(--rb-surface));
  }

  .home-flow-state,
  .home-flow-actions,
  .home-pill-note {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .home-orb {
    width: 3rem;
    height: 3rem;
    background: var(--rb-bg0);
  }

  .home-orb.is-recording {
    color: var(--rb-record);
  }

  .home-orb.is-dictating {
    color: var(--rb-mic);
  }

  .home-flow-state strong,
  .home-pill-note strong {
    font-size: 0.8125rem;
    font-weight: 650;
  }

  .home-action {
    display: inline-flex;
    min-height: 2.45rem;
    align-items: center;
    gap: 0.45rem;
    border: 0;
    border-radius: 999px;
    padding: 0 0.8rem;
    color: var(--rb-text);
    background: var(--rb-bg0);
    font: inherit;
    font-size: 0.75rem;
    font-weight: 600;
    cursor: pointer;
  }

  .home-action.is-stop {
    color: var(--rb-record);
  }

  .home-search:hover,
  .home-action:hover,
  .home-library-list button:hover,
  .home-recents button:hover {
    color: var(--rb-text);
    background: var(--rb-surface-2);
  }

  .home-search:focus-visible,
  .home-action:focus-visible,
  .home-library-list button:focus-visible,
  .home-recents button:focus-visible,
  .home-section-head button:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .home-columns {
    display: grid;
    min-height: 0;
    flex: 1;
    grid-template-columns: minmax(0, 1.55fr) minmax(15rem, 0.85fr);
    gap: 1.4rem;
  }

  .home-library,
  .home-aside,
  .home-aside > section {
    min-width: 0;
  }

  .home-aside {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .home-section-head {
    display: flex;
    min-height: 2rem;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
  }

  .home-section-head h2 {
    margin: 0;
    font-size: 0.8125rem;
    font-weight: 650;
  }

  .home-section-head > span,
  .home-section-head button {
    border: 0;
    padding: 0;
    color: var(--rb-faint);
    background: transparent;
    font: inherit;
    font-size: 0.6875rem;
  }

  .home-section-head button {
    cursor: pointer;
  }

  .home-library-list {
    display: flex;
    flex-direction: column;
  }

  .home-library-list button {
    display: grid;
    min-height: 4.25rem;
    grid-template-columns: 2.5rem minmax(0, 1fr) auto;
    align-items: center;
    gap: 0.75rem;
    border: 0;
    padding: 0.5rem 0.55rem;
    color: var(--rb-text);
    background: transparent;
    text-align: left;
    cursor: pointer;
  }

  .home-library-list button + button {
    border-top: 1px solid var(--rb-hairline);
  }

  .home-library-icon {
    width: 2.5rem;
    height: 2.5rem;
  }

  .home-library-copy {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 0.16rem;
  }

  .home-library-copy strong {
    font-size: 0.8125rem;
    font-weight: 620;
  }

  .home-library-copy small {
    overflow: hidden;
    color: var(--rb-muted);
    font-size: 0.6875rem;
    line-height: 1.35;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .home-arrow {
    color: var(--rb-faint);
    font-size: 0.875rem;
  }

  .home-empty {
    margin: 0.4rem 0 0;
    padding: 0.8rem;
    border-radius: var(--rb-radius-sm);
    color: var(--rb-muted);
    background: var(--rb-surface);
    font-size: 0.75rem;
    line-height: 1.45;
  }

  .home-recents {
    margin: 0.35rem 0 0;
    padding: 0;
    list-style: none;
  }

  .home-recents button {
    display: flex;
    width: 100%;
    min-height: 2.5rem;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    border: 0;
    border-radius: var(--rb-radius-xs);
    padding: 0.35rem 0.45rem;
    color: var(--rb-text);
    background: transparent;
    text-align: left;
    cursor: pointer;
  }

  .home-recents span {
    min-width: 0;
    overflow: hidden;
    font-size: 0.75rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .home-recents time {
    flex: 0 0 auto;
    color: var(--rb-faint);
    font-size: 0.625rem;
  }

  .home-pill-note {
    margin-top: auto;
    padding: 0.8rem;
    border-radius: var(--rb-radius);
    background: color-mix(in oklch, var(--rb-surface) 82%, var(--rb-bg1));
  }

  .home-mini-pill {
    width: 2.25rem;
    height: 2.25rem;
    background: var(--rb-bg0);
  }

  @container atic-main (max-width: 43rem) {
    .home-columns {
      grid-template-columns: 1fr;
    }

    .home-pill-note {
      margin-top: 0;
    }
  }

  @container atic-main (max-width: 31rem) {
    .home-control {
      gap: 0.85rem;
      padding: 0.8rem 0.75rem 1rem;
    }

    .home-intro {
      align-items: flex-start;
    }

    .home-search {
      width: 2.5rem;
      margin-left: auto;
      justify-content: center;
      padding: 0;
      font-size: 0;
    }

    .home-search kbd {
      display: none;
    }

    .home-flow {
      align-items: stretch;
      flex-direction: column;
    }

    .home-flow-actions {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .home-action {
      justify-content: center;
    }

    .home-section-head > span {
      display: none;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .home-search,
    .home-action,
    .home-library-list button,
    .home-recents button {
      transition: none;
    }
  }
</style>
