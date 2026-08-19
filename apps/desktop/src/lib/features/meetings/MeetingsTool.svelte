<script lang="ts">
  /**
   * Reuniones: grabar, ver lo grabado, transcribir.
   *
   * Es la primera pantalla que conecta los stores de dominio con las
   * primitivas, y por eso no tiene ni un `onMount` con suscripciones ni una
   * variable de estado propia que venga de Rust: todo eso vive en `domain/` y
   * acá solo se lee. Lo único local es lo que no sale de la ventana — qué
   * confirmación está abierta.
   */
  import { formatDate, formatDuration, statusLabel } from "$core/format";
  import type { Recording } from "$core/types";
  import { capture } from "$domain/capture.svelte";
  import { models } from "$domain/models.svelte";
  import { recordings } from "$domain/recordings.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import { pickAudioFiles } from "$ipc/dialogs";
  import { openRecordingDir } from "$ipc/recordings";
  import ListDetail from "$patterns/ListDetail.svelte";
  import ToolPage from "$patterns/ToolPage.svelte";
  import Toolbar from "$patterns/Toolbar.svelte";
  import Button from "$ui/Button.svelte";
  import Banner from "$ui/Banner.svelte";
  import Chip from "$ui/Chip.svelte";
  import ConfirmDialog from "$ui/ConfirmDialog.svelte";
  import EmptyState from "$ui/EmptyState.svelte";
  import Meter from "$ui/Meter.svelte";
  import ProgressBar from "$ui/ProgressBar.svelte";
  import LiveTranscript from "./LiveTranscript.svelte";
  import RecordingPlayer from "./RecordingPlayer.svelte";
  import SummaryPanel from "./SummaryPanel.svelte";
  import TranscribeModelSelect from "./TranscribeModelSelect.svelte";
  import TranscriptPanel from "./TranscriptPanel.svelte";
  import TranscriptView from "./TranscriptView.svelte";
  import { summaries } from "$domain/summaries.svelte";

  let { onOpenSettings }: { onOpenSettings?: () => void } = $props();

  let toDelete = $state<Recording | null>(null);
  let deleting = $state(false);
  let importing = $state(false);
  let transcriptFor = $state<Recording | null>(null);
  let summaryFor = $state<Recording | null>(null);
  let openingFolder = $state(false);

  const TONE = {
    recorded: "neutral",
    transcribing: "info",
    transcribed: "ok",
    summarizing: "info",
    summarized: "ok",
    error: "danger",
  } as const;

  async function toggle() {
    try {
      await capture.toggle();
    } catch (error) {
      toastError(error);
    }
  }

  async function transcribe(id: string) {
    try {
      await recordings.transcribe(id);
    } catch (error) {
      toastError(error);
    }
  }

  async function openThisRecording(id: string) {
    if (openingFolder) return;
    openingFolder = true;
    try {
      await openRecordingDir(id);
    } catch (error) {
      toastError(error);
    } finally {
      openingFolder = false;
    }
  }

  /**
   * Traer audio que ya existe.
   *
   * No se puede importar mientras se graba: Rust tiene un solo pipeline de
   * captura y meterle archivos en el medio le cambiaría la lista debajo.
   */
  async function importAudio() {
    if (capture.active) return;
    importing = true;
    try {
      const paths = await pickAudioFiles();
      if (paths.length === 0) return;
      const imported = await recordings.importFiles(paths);
      if (imported[0]) recordings.select(imported[0].id);
      toasts.push(
        imported.length === 1
          ? `Importado: ${imported[0].title}`
          : `Importados ${imported.length} archivos`,
      );
    } catch (error) {
      toastError(error);
    } finally {
      importing = false;
    }
  }

  async function confirmDelete() {
    const target = toDelete;
    if (!target) return;
    deleting = true;
    try {
      await recordings.remove(target.id);
      toasts.push(`Borrada: ${target.title}`);
      toDelete = null;
    } catch (error) {
      toastError(error);
    } finally {
      deleting = false;
    }
  }
</script>

<ToolPage
  title="Reuniones"
  icon="meetings"
  blurb="Audio del PC, transcripción local y resúmenes editables."
  kicker="Grabar y resumir"
>
  {#snippet meta()}
    {#if capture.active}
      <Chip tone="rec">grabando · {formatDuration(capture.elapsed)}</Chip>
    {:else}
      <Chip>{recordings.items.length} grabaciones</Chip>
    {/if}
    {#if capture.meeting?.active}
      <Chip tone="info">
        reunión detectada{capture.meeting.provider
          ? ` · ${capture.meeting.provider}`
          : ""}
      </Chip>
    {/if}
  {/snippet}

  <div class="flex h-full min-h-0 flex-col">
    <Toolbar label="Acciones de grabación">
      <Button
        variant={capture.active ? "danger-solid" : "primary"}
        size="sm"
        loading={capture.busy}
        onclick={toggle}
      >
        {capture.active ? "Parar" : "Grabar"}
      </Button>

      <Button
        variant="soft"
        size="sm"
        loading={importing}
        disabled={capture.active}
        onclick={() => void importAudio()}
      >
        Importar
      </Button>

      <Button
        variant="ghost"
        size="sm"
        loading={openingFolder}
        disabled={!recordings.selected}
        onclick={() => {
          const id = recordings.selectedId;
          if (id) void openThisRecording(id);
        }}
      >
        Carpeta
      </Button>

      {#snippet end()}
        {#if capture.active}
          <!-- Los niveles solo tienen sentido mientras entra audio. -->
          <div class="flex w-40 flex-col gap-0.5">
            <Meter value={capture.levels.mic} tone="mic" label="mic" />
            <Meter value={capture.levels.system} tone="sys" label="sis" />
          </div>
        {/if}
      {/snippet}
    </Toolbar>

    {#if models.missing.length > 0}
      <div class="px-4 pt-3">
        <Banner
          tone="warn"
          title={models.missing.length === 1
            ? `Falta descargar ${models.missing[0].display_name}`
            : `Faltan ${models.missing.length} modelos`}
        >
          {#snippet action()}
            <Button
              variant="soft"
              size="sm"
              loading={models.downloading !== null}
              onclick={() => void models.download(models.missing[0].id)}
            >
              Descargar
            </Button>
          {/snippet}
          Sin ellos no se puede transcribir.
        </Banner>
      </div>
    {/if}

    {#if models.downloading}
      <div class="px-4 pt-3">
        <ProgressBar
          value={models.downloading.downloaded / Math.max(models.downloading.total, 1)}
          label="Descargando modelo"
        />
      </div>
    {/if}

    {#if capture.note}
      <div class="px-4 pt-3">
        <Banner tone="warn" title={capture.note} />
      </div>
    {/if}

    <LiveTranscript />

    <div class="min-h-0 flex-1">
      <ListDetail
        hasSelection={recordings.selected !== null}
        listLabel="Grabaciones"
        listCount={recordings.items.length}
      >
        {#snippet list()}
          {#if recordings.items.length === 0}
            <EmptyState
              compact
              icon="meetings"
              title="Todavía no hay grabaciones"
              hint="Apretá Grabar, o importá audio que ya tengas."
            />
          {:else}
            <ul class="flex flex-col">
              {#each recordings.items as item (item.id)}
                <li>
                  <button
                    type="button"
                    class="flex w-full flex-col gap-0.5 px-3 py-1.5
                           text-left transition-colors duration-(--duration-quick)
                           hover:bg-surface-2
                           {recordings.selectedId === item.id ? 'bg-surface-2' : ''}"
                    aria-current={recordings.selectedId === item.id
                      ? "true"
                      : undefined}
                    onclick={() => recordings.select(item.id)}
                  >
                    <span class="truncate text-sm text-text">{item.title}</span>
                    <span class="font-mono text-xs text-faint" data-numeric>
                      {formatDuration(item.duration_secs)} · {formatDate(
                        item.started_at,
                      )}
                    </span>
                    {#if recordings.progress[item.id] !== undefined}
                      <div class="mt-1 w-full">
                        <ProgressBar value={recordings.progress[item.id]} />
                      </div>
                    {/if}
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        {/snippet}

        {#snippet detail()}
          {@const item = recordings.selected}
          {#if item}
            <!-- Sin padding propio: el panel de ListDetail ya lo trae. -->
            <div class="flex flex-col gap-3">
              <div class="flex items-start gap-2">
                <div class="flex min-w-0 flex-1 flex-col gap-1">
                  <h3 class="truncate text-md font-semibold text-text">{item.title}</h3>
                  <p class="font-mono text-xs text-faint" data-numeric>
                    {formatDuration(item.duration_secs)} · {formatDate(item.started_at)}
                  </p>
                </div>
                <Chip tone={TONE[item.status]}>{statusLabel(item.status)}</Chip>
              </div>

              {#if item.mic_path || item.system_path}
                <RecordingPlayer recording={item} />
              {/if}

              {#if recordings.progress[item.id] !== undefined}
                <ProgressBar
                  value={recordings.progress[item.id]}
                  label={`Transcribiendo · ${models.meetingProgressLabel}`}
                  tone="ok"
                />
              {/if}

              <div class="flex flex-wrap items-center gap-1.5">
                <div class="min-w-52 max-w-72 flex-1">
                  <TranscribeModelSelect
                    disabled={recordings.progress[item.id] !== undefined}
                  />
                </div>
                {#if !models.meetingCanTranscribe}
                  <Button
                    variant="soft"
                    size="sm"
                    loading={models.downloading !== null}
                    onclick={() => {
                      const id = models.meetingModel?.id;
                      if (id) void models.download(id).catch(toastError);
                    }}
                  >
                    Descargar
                  </Button>
                {:else}
                  <Button
                    variant="soft"
                    size="sm"
                    disabled={recordings.progress[item.id] !== undefined}
                    onclick={() => void transcribe(item.id)}
                  >
                    {item.status === "recorded" || item.status === "error"
                      ? "Transcribir"
                      : "Re-transcribir"}
                  </Button>
                {/if}
                <Button variant="soft" size="sm" onclick={() => (transcriptFor = item)}>
                  Ver y corregir
                </Button>
                <Button variant="primary" size="sm" onclick={() => (summaryFor = item)}>
                  {summaries.byId[item.id] ? "Resumen" : "Resumir"}
                </Button>
                <Button variant="danger" size="sm" onclick={() => (toDelete = item)}>
                  Borrar
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  loading={openingFolder}
                  onclick={() => void openThisRecording(item.id)}
                >
                  Carpeta
                </Button>
              </div>

              <div class="border-t border-line pt-3">
                <TranscriptView recordingId={item.id} />
              </div>
            </div>
          {/if}
        {/snippet}

        {#snippet empty()}
          <EmptyState
            compact
            icon="meetings"
            title="Elegí una grabación"
            hint="El detalle se muestra acá."
          />
        {/snippet}
      </ListDetail>
    </div>
  </div>
</ToolPage>

{#if transcriptFor}
  <!-- Se fija en una constante para que el cierre de `onRetranscribe` no
       dependa de una variable que puede volverse nula. -->
  {@const item = transcriptFor}
  <TranscriptPanel
    recording={item}
    canTranscribe={models.meetingCanTranscribe}
    onRetranscribe={() => transcribe(item.id)}
    onClose={() => (transcriptFor = null)}
  />
{/if}

{#if summaryFor}
  <SummaryPanel
    recording={summaryFor}
    {onOpenSettings}
    onClose={() => (summaryFor = null)}
  />
{/if}

{#if toDelete}
  <ConfirmDialog
    title="Borrar «{toDelete.title}»"
    body="Se borran el audio, la transcripción y el resumen. No se puede deshacer."
    confirmLabel="Borrar"
    tone="danger"
    busy={deleting}
    onConfirm={() => void confirmDelete()}
    onCancel={() => (toDelete = null)}
  />
{/if}
