<script lang="ts">
  import { onMount } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import type { Recording, Summary, TemplateInfo } from "$lib/types";
  import {
    getConfig,
    getSummary,
    listSummaryTemplates,
    onSummarizeDelta,
    onSummarizeError,
    onSummaryReady,
    saveSummary,
    sendSummaryEmail,
    summarizeRecording,
  } from "$lib/api";
  import ConfirmModal from "$lib/ConfirmModal.svelte";
  import ModalShell from "$lib/ModalShell.svelte";
  import SummaryDocument from "$lib/SummaryDocument.svelte";

  let {
    recording,
    onClose,
    onToast,
    onNeedSetup,
  }: {
    recording: Recording;
    onClose: () => void;
    onToast: (message: string) => void;
    onNeedSetup?: () => void;
  } = $props();

  type Phase = "generate" | "review" | "send";
  type ReviewMode = "preview" | "edit";
  type ConfirmIntent = "discard" | "regenerate" | null;

  let phase = $state<Phase>("generate");
  let reviewMode = $state<ReviewMode>("preview");
  let confirmIntent = $state<ConfirmIntent>(null);
  let summary = $state<Summary | null>(null);
  let templates = $state<TemplateInfo[]>([]);
  let selectedTemplate = $state("summary_key_points");
  let mailBackend = $state("mailto");

  let loading = $state(true);
  let loadError = $state<string | null>(null);
  let generating = $state(false);
  let saving = $state(false);
  let sending = $state(false);

  let body = $state("");
  let subject = $state("");
  let recipients = $state("");
  let dirty = $state(false);
  let savedBody = "";
  let savedSubject = "";

  const selectedTemplateLabel = $derived(
    templates.find((template) => template.id === selectedTemplate)?.label ??
      selectedTemplate,
  );
  const canReview = $derived(Boolean(body.trim()) || generating);
  const canSend = $derived(Boolean(body.trim()) && !generating);
  const documentTitle = $derived(
    selectedTemplate === "followup_email"
      ? "Mensaje"
      : selectedTemplate === "action_items"
        ? "Acuerdos"
        : "Resumen",
  );
  const phaseLabel = $derived(
    phase === "generate"
      ? "Generar resumen"
      : phase === "review"
        ? "Revisar resumen"
        : "Enviar seguimiento",
  );

  function applySummary(next: Summary | null) {
    summary = next;
    body = next?.body ?? "";
    subject = next?.subject ?? `Seguimiento: ${recording.title}`;
    selectedTemplate = next?.template ?? templates[0]?.id ?? selectedTemplate;
    savedBody = body;
    savedSubject = subject;
    dirty = false;
  }

  function rollbackGeneration() {
    body = savedBody;
    subject = savedSubject;
    dirty = false;
    generating = false;
  }

  async function reloadSummary() {
    applySummary(await getSummary(recording.id));
  }

  async function beginGeneration() {
    confirmIntent = null;
    generating = true;
    loadError = null;
    body = "";
    try {
      await summarizeRecording(recording.id, selectedTemplate);
    } catch (error) {
      rollbackGeneration();
      const message = String(error);
      onToast(message);
      if (
        message.toLowerCase().includes("api key") ||
        message.toLowerCase().includes("ollama")
      ) {
        onNeedSetup?.();
      }
    }
  }

  function requestGeneration() {
    if (dirty || summary || body.trim()) {
      confirmIntent = "regenerate";
    } else {
      void beginGeneration();
    }
  }

  async function save(): Promise<Summary | null> {
    if (!body.trim()) return null;
    saving = true;
    try {
      const next: Summary = {
        template: selectedTemplate,
        title: summary?.title ?? `Resumen — ${recording.title}`,
        body,
        subject: subject.trim() || null,
        backend: summary?.backend || "manual",
        created_at: summary?.created_at ?? new Date().toISOString(),
      };
      await saveSummary(recording.id, next);
      applySummary(next);
      onToast("Resumen guardado");
      return next;
    } catch (error) {
      onToast(String(error));
      return null;
    } finally {
      saving = false;
    }
  }

  async function send() {
    const to = recipients
      .split(/[,;\s]+/)
      .map((address) => address.trim())
      .filter(Boolean);

    if (to.length === 0) {
      onToast("Escribe al menos un destinatario.");
      return;
    }
    if (!subject.trim()) {
      onToast("Escribe el asunto del correo.");
      return;
    }

    sending = true;
    try {
      if (dirty || !summary) {
        const saved = await save();
        if (!saved) return;
      }
      const result = await sendSummaryEmail(
        recording.id,
        to,
        subject.trim(),
        body,
      );
      onToast(
        result.backend === "mailto"
          ? "Se abrió un borrador en tu cliente de correo."
          : result.message,
      );
    } catch (error) {
      onToast(String(error));
    } finally {
      sending = false;
    }
  }

  function requestClose() {
    if (dirty) confirmIntent = "discard";
    else onClose();
  }

  onMount(() => {
    const unlisteners: Promise<UnlistenFn>[] = [];

    (async () => {
      try {
        const [availableTemplates, appConfig] = await Promise.all([
          listSummaryTemplates(),
          getConfig(),
        ]);
        templates = availableTemplates;
        mailBackend = appConfig.mail_backend;
        if (templates[0]) selectedTemplate = templates[0].id;
        await reloadSummary();
        if (summary) phase = "review";
      } catch (error) {
        loadError = `No se pudo abrir el resumen: ${String(error)}`;
      } finally {
        loading = false;
      }
    })();

    unlisteners.push(
      onSummarizeDelta((id, delta) => {
        if (id !== recording.id) return;
        body += delta;
      }),
      onSummaryReady(async (id) => {
        if (id !== recording.id) return;
        generating = false;
        try {
          await reloadSummary();
          phase = "review";
        } catch (error) {
          loadError = `El resumen terminó, pero no se pudo abrir: ${String(error)}`;
        }
      }),
      onSummarizeError((id, message) => {
        if (id !== recording.id) return;
        rollbackGeneration();
        onToast(message);
        if (
          message.toLowerCase().includes("api key") ||
          message.toLowerCase().includes("ollama")
        ) {
          onNeedSetup?.();
        }
      }),
    );

    return () => {
      unlisteners.forEach((listener) => listener.then((unlisten) => unlisten()));
    };
  });
</script>

<ModalShell
  title="Resumen"
  subtitle={recording.title}
  size="xl"
  onClose={requestClose}
>
  <div class="rb-settings-layout rb-summary-layout">
    <nav
      class="rb-settings-sidebar rb-summary-sidebar"
      aria-label="Etapas del resumen"
    >
      <button
        type="button"
        class="rb-settings-nav-item rb-summary-nav-item"
        class:active={phase === "generate"}
        onclick={() => (phase = "generate")}
        aria-current={phase === "generate" ? "step" : undefined}
      >
        <span aria-hidden="true">1</span>
        Generar
      </button>
      <button
        type="button"
        class="rb-settings-nav-item rb-summary-nav-item"
        class:active={phase === "review"}
        onclick={() => (phase = "review")}
        disabled={!canReview}
        aria-current={phase === "review" ? "step" : undefined}
      >
        <span aria-hidden="true">2</span>
        Revisar
      </button>
      <button
        type="button"
        class="rb-settings-nav-item rb-summary-nav-item"
        class:active={phase === "send"}
        onclick={() => (phase = "send")}
        disabled={!canSend}
        aria-current={phase === "send" ? "step" : undefined}
      >
        <span aria-hidden="true">3</span>
        Enviar
      </button>
    </nav>

    <div class="rb-settings-content">
      <div class="rb-settings-panel rb-summary-panel">
        <h3 class="rb-settings-panel-title">{phaseLabel}</h3>

        {#if loading}
          <div class="rb-summary-state" role="status">Cargando resumen…</div>
        {:else if loadError}
          <div class="rb-summary-state is-error" role="alert">{loadError}</div>
        {:else if phase === "generate"}
          <section class="rb-summary-phase">
      <div>
        <h3>Genera un borrador útil</h3>
        <p>
          El audio ya transcrito se procesa con el proveedor configurado. Puedes
          revisar y editar el resultado antes de enviarlo.
        </p>
      </div>

      <label class="rb-label">
        Formato
        <select
          class="rb-field"
          bind:value={selectedTemplate}
          disabled={generating}
        >
          {#each templates as template (template.id)}
            <option value={template.id}>{template.label}</option>
          {/each}
        </select>
      </label>

      {#if generating}
        <div class="rb-generation" role="status">
          <div>
            <strong>Generando {selectedTemplateLabel}</strong>
            <span>Organizando el contenido mientras llega.</span>
          </div>
          <div class="rb-generation-preview">
            <SummaryDocument
              content={body}
              defaultTitle={documentTitle}
              streaming
              emptyMessage="Preparando la estructura…"
            />
          </div>
        </div>
      {:else if summary}
        <div class="rb-existing-summary">
          <strong>Ya existe un resumen guardado</strong>
          <span>{selectedTemplateLabel} · {summary.backend}</span>
        </div>
      {/if}
          </section>
        {:else if phase === "review"}
          <section class="rb-summary-phase">
      <div class="rb-review-heading">
        <div>
          <h3>Resumen estructurado</h3>
          <p>Lee el resultado organizado o cambia a edición cuando lo necesites.</p>
        </div>
        <div class="rb-review-tools">
          <span class:has-changes={dirty}
            >{dirty ? "Cambios sin guardar" : "Guardado"}</span
          >
          <div class="rb-review-switch" role="group" aria-label="Modo de revisión">
            <button
              type="button"
              class:is-active={reviewMode === "preview"}
              onclick={() => (reviewMode = "preview")}
              aria-pressed={reviewMode === "preview"}
            >
              Vista
            </button>
            <button
              type="button"
              class:is-active={reviewMode === "edit"}
              onclick={() => (reviewMode = "edit")}
              aria-pressed={reviewMode === "edit"}
            >
              Editar
            </button>
          </div>
        </div>
      </div>

      <label class="rb-label">
        Asunto
        <input
          class="rb-field"
          name="summary-subject"
          bind:value={subject}
          oninput={() => (dirty = true)}
          autocomplete="off"
          disabled={generating}
        />
      </label>
      {#if reviewMode === "preview"}
        <SummaryDocument
          content={body}
          defaultTitle={documentTitle}
          streaming={generating}
        />
      {:else}
        <label class="rb-label">
          Contenido en Markdown
          <textarea
            class="rb-field rb-summary-editor"
            name="summary-body"
            bind:value={body}
            oninput={() => (dirty = true)}
            readonly={generating}
            spellcheck="true"
          ></textarea>
          <span class="rb-hint">
            Usa encabezados con ## y guiones para mantener la vista organizada.
          </span>
        </label>
      {/if}
          </section>
        {:else}
          <section class="rb-summary-phase">
      <div>
        <h3>Envía el seguimiento</h3>
        <p>
          {mailBackend === "smtp"
            ? "El correo se enviará directamente por SMTP."
            : "Se abrirá un borrador en tu cliente de correo."}
        </p>
      </div>

      <label class="rb-label">
        Destinatarios
        <input
          class="rb-field"
          name="summary-recipients"
          bind:value={recipients}
          placeholder="ana@empresa.com, luis@empresa.com…"
          autocomplete="off"
          spellcheck="false"
        />
        <span class="rb-hint">Separa varias direcciones con coma.</span>
      </label>

      <div class="rb-mail-preview">
        <span>Asunto</span>
        <strong>{subject || "Sin asunto"}</strong>
        <span>Contenido</span>
        <div>
          <SummaryDocument
            content={body}
            defaultTitle={documentTitle}
            compact
          />
        </div>
      </div>
          </section>
        {/if}
      </div>
    </div>
  </div>

  {#snippet actions()}
    {#if !loading && !loadError}
      {#if phase === "generate"}
        <button
          class="rb-btn rb-btn-primary"
          onclick={requestGeneration}
          disabled={generating}
        >
          {generating ? "Generando…" : summary ? "Regenerar" : "Generar resumen"}
        </button>
        {#if canReview}
          <button class="rb-btn rb-btn-soft" onclick={() => (phase = "review")}>
            Revisar
          </button>
        {/if}
      {:else if phase === "review"}
        <button
          class="rb-btn rb-btn-soft"
          onclick={save}
          disabled={saving || generating || !dirty}
        >
          {saving ? "Guardando…" : "Guardar"}
        </button>
        <button
          class="rb-btn rb-btn-primary"
          onclick={() => (phase = "send")}
          disabled={!canSend}
        >
          Continuar a envío
        </button>
      {:else}
        <button class="rb-btn rb-btn-ghost" onclick={() => (phase = "review")}>
          Volver
        </button>
        <button
          class="rb-btn rb-btn-primary"
          onclick={send}
          disabled={sending || !canSend}
        >
          {sending
            ? "Preparando…"
            : mailBackend === "smtp"
              ? "Enviar correo"
              : "Abrir borrador"}
        </button>
      {/if}
    {/if}
  {/snippet}
</ModalShell>

{#if confirmIntent === "discard"}
  <ConfirmModal
    title="Descartar cambios"
    message="Hay cambios sin guardar. Si cierras ahora, se perderán."
    confirmLabel="Descartar"
    onConfirm={onClose}
    onCancel={() => (confirmIntent = null)}
  />
{:else if confirmIntent === "regenerate"}
  <ConfirmModal
    title="Regenerar resumen"
    message="Se reemplazará el borrador actual por una nueva generación."
    confirmLabel="Regenerar"
    danger={false}
    onConfirm={beginGeneration}
    onCancel={() => (confirmIntent = null)}
  />
{/if}

<style>
  .rb-summary-nav-item {
    display: flex;
    align-items: center;
    gap: 0.55rem;
  }
  .rb-summary-nav-item > span {
    display: inline-flex;
    width: 1.25rem;
    height: 1.25rem;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    border-radius: 999px;
    color: var(--rb-faint);
    background: color-mix(in srgb, var(--rb-text) 6%, transparent);
    font-size: 0.625rem;
    font-variant-numeric: tabular-nums;
  }
  .rb-summary-nav-item.active > span {
    color: #fbfbf8;
    background: var(--rb-accent);
  }
  .rb-summary-nav-item:disabled {
    opacity: 0.42;
    cursor: not-allowed;
  }
  .rb-summary-nav-item:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }
  .rb-summary-panel {
    min-height: 0;
  }
  .rb-summary-phase {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }
  .rb-summary-phase h3 {
    color: var(--rb-text);
    font-size: 0.875rem;
    font-weight: 600;
  }
  .rb-summary-phase > div > p {
    max-width: 65ch;
    margin-top: 0.25rem;
    color: var(--rb-muted);
    font-size: 0.75rem;
    line-height: 1.5;
  }
  .rb-generation {
    overflow: hidden;
    border: 0;
    border-radius: var(--rb-radius-sm);
    background: color-mix(in srgb, var(--rb-text) 3.5%, transparent);
  }
  .rb-generation > div:first-child {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.65rem 0.75rem;
    border-bottom: 0;
    font-size: 0.6875rem;
  }
  .rb-generation > div:first-child span {
    color: var(--rb-muted);
  }
  .rb-generation-preview {
    max-height: 18rem;
    overflow-y: auto;
    padding: 0.65rem;
  }
  .rb-existing-summary {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.75rem;
    border-radius: var(--rb-radius-sm);
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-accent) 9%, transparent);
    font-size: 0.75rem;
  }
  .rb-existing-summary span {
    color: var(--rb-muted);
    font-size: 0.6875rem;
  }
  .rb-review-heading {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 1rem;
  }
  .rb-review-tools {
    display: flex;
    align-items: center;
    gap: 0.65rem;
  }
  .rb-review-tools > span {
    color: var(--rb-muted);
    font-size: 0.6875rem;
    white-space: nowrap;
  }
  .rb-review-tools > span.has-changes {
    color: var(--rb-warn);
  }
  .rb-review-switch {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: var(--rb-radius-sm);
    background: color-mix(in srgb, var(--rb-text) 6%, transparent);
  }
  .rb-review-switch button {
    min-height: 1.75rem;
    border: 0;
    border-radius: var(--rb-radius-xs);
    padding: 0.25rem 0.55rem;
    color: var(--rb-muted);
    background: transparent;
    font-size: 0.6875rem;
    font-weight: 550;
  }
  .rb-review-switch button:hover {
    color: var(--rb-text);
  }
  .rb-review-switch button.is-active {
    color: var(--rb-text);
    background: var(--rb-surface-2);
  }
  .rb-review-switch button:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }
  .rb-summary-editor {
    min-height: 20rem;
    resize: vertical;
    font-family: var(--rb-font);
    font-size: 0.8125rem;
    line-height: 1.6;
  }
  .rb-mail-preview {
    display: grid;
    grid-template-columns: 4rem minmax(0, 1fr);
    gap: 0.5rem 0.75rem;
    padding: 0.85rem;
    border: 0;
    border-radius: var(--rb-radius-sm);
    background: color-mix(in srgb, var(--rb-text) 3.5%, transparent);
  }
  .rb-mail-preview span {
    color: var(--rb-faint);
    font-size: 0.6875rem;
  }
  .rb-mail-preview strong {
    min-width: 0;
    overflow: hidden;
    color: var(--rb-text);
    font-size: 0.75rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rb-mail-preview > div {
    grid-column: 2;
    min-width: 0;
    max-height: 14rem;
    overflow-y: auto;
  }
  .rb-summary-state {
    display: flex;
    min-height: 18rem;
    align-items: center;
    justify-content: center;
    color: var(--rb-muted);
    font-size: 0.8125rem;
  }
  .rb-summary-state.is-error {
    color: var(--rb-record);
  }

  @media (max-width: 36rem) {
    .rb-summary-phase > div > p,
    .rb-existing-summary,
    .rb-generation-preview,
    .rb-summary-editor,
    .rb-mail-preview strong {
      font-size: 0.875rem;
    }

    .rb-existing-summary,
    .rb-review-heading {
      align-items: flex-start;
      flex-direction: column;
    }

    .rb-review-tools {
      width: 100%;
      justify-content: space-between;
    }

    .rb-review-switch button {
      min-height: 2.25rem;
      padding-inline: 0.75rem;
      font-size: 0.75rem;
    }

    .rb-mail-preview {
      grid-template-columns: 3.25rem minmax(0, 1fr);
      padding: 0.75rem;
    }
  }
</style>
