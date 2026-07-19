<script lang="ts">
  import type { Recording } from "$lib/types";
  import { formatDate, formatDuration, statusLabel } from "$lib/format";

  let {
    recordings,
    selectedId,
    progress,
    onSelect,
  }: {
    recordings: Recording[];
    selectedId: string | null;
    progress: Record<string, number>;
    onSelect: (recording: Recording) => void;
  } = $props();
</script>

<section class="rb-recording-list" aria-label="Grabaciones">
  <header class="rb-pane-header">
    <h2>Grabaciones</h2>
    <span>{recordings.length}</span>
  </header>

  {#if recordings.length === 0}
    <div class="rb-list-empty">
      <p class="font-medium">Aún no hay grabaciones</p>
      <p>Inicia una grabación para verla aquí.</p>
    </div>
  {:else}
    <ul>
      {#each recordings as recording (recording.id)}
        {@const currentProgress = progress[recording.id]}
        <li>
          <button
            class="rb-recording-row"
            class:is-selected={selectedId === recording.id}
            onclick={() => onSelect(recording)}
            aria-pressed={selectedId === recording.id}
          >
            <span class="rb-recording-row-title">{recording.title}</span>
            <span class="rb-recording-row-meta">
              {formatDate(recording.started_at)} · {formatDuration(recording.duration_secs)}
            </span>
            <span
              class="rb-recording-row-status"
              class:is-error={recording.status === "error"}
              class:is-working={recording.status === "transcribing" ||
                recording.status === "summarizing"}
            >
              {#if currentProgress !== undefined}
                Transcribiendo · {Math.round(currentProgress * 100)}%
              {:else}
                {statusLabel(recording.status)}
              {/if}
            </span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .rb-recording-list {
    min-height: 0;
    overflow: hidden;
    border: 0;
    border-radius: var(--rb-radius);
    background: var(--rb-surface);
  }
  .rb-pane-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.75rem 0.875rem;
    border-bottom: 0;
  }
  .rb-pane-header h2 {
    font-size: 0.8125rem;
    font-weight: 600;
  }
  .rb-pane-header span {
    color: var(--rb-faint);
    font-size: 0.6875rem;
    font-variant-numeric: tabular-nums;
  }
  ul {
    max-height: 100%;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  li {
    content-visibility: auto;
    contain-intrinsic-size: auto 74px;
  }
  li + li {
    border-top: 0;
  }
  .rb-recording-row {
    display: grid;
    width: 100%;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 0.25rem 0.75rem;
    padding: 0.75rem 0.875rem;
    text-align: left;
    transition:
      background 0.14s ease,
      color 0.14s ease;
  }
  .rb-recording-row:hover {
    background: color-mix(in srgb, var(--rb-text) 4%, transparent);
  }
  .rb-recording-row.is-selected {
    background: color-mix(in srgb, var(--rb-accent) 10%, transparent);
  }
  .rb-recording-row:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }
  .rb-recording-row-title {
    min-width: 0;
    overflow: hidden;
    color: var(--rb-text);
    font-size: 0.8125rem;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rb-recording-row-meta {
    grid-column: 1 / -1;
    overflow: hidden;
    color: var(--rb-faint);
    font-size: 0.6875rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rb-recording-row-status {
    grid-column: 2;
    grid-row: 1;
    align-self: center;
    color: var(--rb-muted);
    font-size: 0.625rem;
    white-space: nowrap;
  }
  .rb-recording-row-status.is-working {
    color: var(--rb-warn);
  }
  .rb-recording-row-status.is-error {
    color: var(--rb-record);
  }

  @media (max-width: 760px) {
    .rb-recording-list {
      max-height: min(38dvh, 21rem);
      border-radius: var(--rb-radius);
    }
  }

  @media (max-width: 36rem) {
    .rb-recording-row {
      min-height: 4.5rem;
      padding: 0.75rem;
    }

    .rb-recording-row-title {
      font-size: 0.875rem;
    }

    .rb-recording-row-meta,
    .rb-recording-row-status,
    .rb-list-empty {
      font-size: 0.75rem;
    }
  }
  .rb-list-empty {
    display: flex;
    min-height: 12rem;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.35rem;
    padding: 1.5rem;
    color: var(--rb-muted);
    text-align: center;
    font-size: 0.75rem;
  }
</style>
