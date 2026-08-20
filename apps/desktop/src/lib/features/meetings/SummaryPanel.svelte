<script lang="ts">
  /**
   * Generar, revisar y enviar el resumen de una reunión.
   *
   * Son tres pasos y no una sola pantalla porque el resumen sale de un modelo:
   * lo que llega hay que leerlo antes de mandarlo a nadie. Los pasos se pueden
   * saltear hacia atrás pero no hacia adelante — no hay nada que revisar sin
   * texto, ni a quién mandárselo sin revisar.
   *
   * El borrador vive en el store, no acá: los tokens llegan por evento y cerrar
   * esta pantalla a media generación no tiene que perderlos.
   */
  import type { Recording } from "$core/types";
  import { config } from "$domain/config.svelte";
  import { summaries } from "$domain/summaries.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import { t } from "$domain/i18n.svelte";
  import { sendSummaryEmail } from "$ipc/summaries";
  import Banner from "$ui/Banner.svelte";
  import Button from "$ui/Button.svelte";
  import ConfirmDialog from "$ui/ConfirmDialog.svelte";
  import Field from "$ui/Field.svelte";
  import Input from "$ui/Input.svelte";
  import Modal from "$ui/Modal.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import Select from "$ui/Select.svelte";
  import TextArea from "$ui/TextArea.svelte";
  import SummaryDocument from "./SummaryDocument.svelte";

  let {
    recording,
    onClose,
    onOpenSettings,
  }: {
    recording: Recording;
    onClose: () => void;
    /** Para el aviso de proveedor mal configurado, que no se arregla acá. */
    onOpenSettings?: () => void;
  } = $props();

  type Phase = "generate" | "review" | "send";

  let phase = $state<Phase>("generate");
  let reviewMode = $state<"preview" | "edit">("preview");
  let confirm = $state<"discard" | "regenerate" | null>(null);
  let recipients = $state("");
  let saving = $state(false);
  let sending = $state(false);
  let loading = $state(true);
  let loadError = $state<string | null>(null);

  $effect(() => {
    const id = recording.id;
    const title = recording.title;
    loading = true;
    loadError = null;
    summaries
      .open(id, title)
      .then(() => {
        // Si ya había un resumen guardado, el paso útil es revisarlo.
        if (summaries.byId[id]) phase = "review";
      })
      .catch((error) => (loadError = t("page.summary.openFail", { error: String(error) })))
      .finally(() => (loading = false));
  });

  const generating = $derived(summaries.generating === recording.id);
  const saved = $derived(summaries.byId[recording.id] ?? null);
  const hasText = $derived(summaries.draft.trim().length > 0);
  const canReview = $derived(hasText || generating);
  const canSend = $derived(hasText && !generating);
  const mailBackend = $derived(config.current?.mail_backend ?? "mailto");

  const templateLabel = $derived(t(`page.summary.tpl.${summaries.template}`));

  /** El título que se le pone al documento cuando el texto no trae ninguno. */
  const documentTitle = $derived(
      summaries.template === "followup_email"
        ? t("page.summary.docMessage")
        : summaries.template === "action_items"
          ? t("page.summary.docActions")
          : t("page.summary.docSummary"),
  );

  const STEPS = $derived([
    { value: "generate" as const, label: t("page.summary.stepGenerate") },
    { value: "review" as const, label: t("page.summary.stepReview"), disabled: !canReview },
    { value: "send" as const, label: t("page.summary.stepSend"), disabled: !canSend },
  ]);

  async function generate() {
    confirm = null;
    try {
      await summaries.generate(recording.id);
    } catch (error) {
      toastError(error);
    }
  }

  function requestGeneration() {
    // Regenerar pisa lo que haya: si hay algo que perder, se pregunta.
    if (summaries.dirty || saved || hasText) confirm = "regenerate";
    else void generate();
  }

  async function save(): Promise<boolean> {
    saving = true;
    try {
      const result = await summaries.save(recording.id, recording.title);
      if (result) toasts.push(t("toast.summarySaved"));
      return result !== null;
    } catch (error) {
      toastError(error);
      return false;
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
      toasts.push(t("toast.needRecipient"));
      return;
    }
    if (!summaries.subject.trim()) {
      toasts.push(t("toast.needSubject"));
      return;
    }

    sending = true;
    try {
      // Se manda lo guardado, no lo que hay en pantalla: si difieren, primero
      // se guarda, para que el archivo y el correo digan lo mismo.
      if ((summaries.dirty || !saved) && !(await save())) return;
      const result = await sendSummaryEmail(
        recording.id,
        to,
        summaries.subject.trim(),
        summaries.draft,
      );
      toasts.push(
        result.backend === "mailto"
          ? t("page.summary.mailtoOpened")
          : result.message,
      );
    } catch (error) {
      toastError(error);
    } finally {
      sending = false;
    }
  }

  function requestClose() {
    if (summaries.dirty) confirm = "discard";
    else onClose();
  }
</script>

<Modal title={t("page.summary.title")} subtitle={recording.title} size="lg" onClose={requestClose}>
  {#if loading}
    <p class="py-12 text-center text-sm text-muted" role="status">{t("page.summary.loading")}</p>
  {:else if loadError}
    <p class="py-12 text-center text-sm text-danger" role="alert">{loadError}</p>
  {:else}
    <div class="flex flex-col gap-4">
      <SegmentedControl
        bind:value={phase}
        options={STEPS}
        label={t("page.summary.stepsAria")}
        full
      />

      {#if summaries.needsSetup}
        <Banner tone="warn" title={t("page.summary.notReady")}>
          {#snippet action()}
            {#if onOpenSettings}
              <Button variant="soft" size="sm" onclick={onOpenSettings}
                >{t("chrome.settings")}</Button
              >
            {/if}
          {/snippet}
          {t("page.summary.notReadyBody")}
        </Banner>
      {/if}

      {#if phase === "generate"}
        <p class="max-w-[65ch] text-sm leading-relaxed text-muted">
          {t("page.summary.generateBlurb")}
        </p>

        <Field label={t("page.summary.format")}>
          {#snippet children({ id })}
            <Select
              {id}
              bind:value={summaries.template}
              options={summaries.templates.map((tpl) => ({
                value: tpl.id,
                label: t(`page.summary.tpl.${tpl.id}`),
              }))}
              disabled={generating}
            />
          {/snippet}
        </Field>

        {#if generating}
          <div class="flex flex-col gap-2" role="status">
            <p class="text-xs text-muted">
              {t("page.summary.generating", { label: templateLabel })}
            </p>
            <div class="max-h-72 overflow-y-auto">
              <SummaryDocument
                content={summaries.draft}
                defaultTitle={documentTitle}
                streaming
                emptyMessage={t("page.summary.preparing")}
              />
            </div>
          </div>
        {:else if saved}
          <Banner tone="info" title={t("page.summary.alreadySaved")}>
            {t("page.summary.alreadySavedBody", {
              label: templateLabel,
              backend: saved.backend,
            })}
          </Banner>
        {/if}
      {:else if phase === "review"}
        <div class="flex items-center justify-between gap-3">
          <span class="text-xs {summaries.dirty ? 'text-warn' : 'text-faint'}">
            {summaries.dirty ? t("page.summary.unsaved") : t("page.summary.saved")}
          </span>
          <SegmentedControl
            bind:value={reviewMode}
            options={[
              { value: "preview" as const, label: t("page.summary.preview") },
              { value: "edit" as const, label: t("page.summary.edit") },
            ]}
            size="sm"
            label={t("page.summary.reviewMode")}
          />
        </div>

        <Field label={t("page.summary.subject")}>
          {#snippet children({ id })}
            <Input
              {id}
              bind:value={summaries.subject}
              oninput={() => summaries.touch()}
              autocomplete="off"
              disabled={generating}
            />
          {/snippet}
        </Field>

        {#if reviewMode === "preview"}
          <SummaryDocument
            content={summaries.draft}
            defaultTitle={documentTitle}
            streaming={generating}
          />
        {:else}
          <Field
            label={t("page.summary.markdown")}
            hint={t("page.summary.markdownHint")}
          >
            {#snippet children({ id, describedBy })}
              <TextArea
                {id}
                aria-describedby={describedBy}
                bind:value={summaries.draft}
                oninput={() => summaries.touch()}
                readonly={generating}
                spellcheck="true"
                rows={16}
              />
            {/snippet}
          </Field>
        {/if}
      {:else}
        <p class="max-w-[65ch] text-sm leading-relaxed text-muted">
          {mailBackend === "smtp"
            ? t("page.summary.smtpBlurb")
            : t("page.summary.mailtoBlurb")}
        </p>

        <Field label={t("page.summary.recipients")} hint={t("page.summary.recipientsHint")}>
          {#snippet children({ id, describedBy })}
            <Input
              {id}
              aria-describedby={describedBy}
              bind:value={recipients}
              placeholder={t("page.summary.recipientsPlaceholder")}
              autocomplete="off"
              spellcheck="false"
            />
          {/snippet}
        </Field>

        <div class="flex flex-col gap-1.5">
          <span class="text-xs font-medium text-muted">{t("page.summary.subject")}</span>
          <p class="truncate text-sm text-text">
            {summaries.subject || t("page.summary.noSubject")}
          </p>
        </div>

        <div class="flex flex-col gap-1.5">
          <span class="text-xs font-medium text-muted">{t("page.summary.content")}</span>
          <div class="max-h-56 overflow-y-auto">
            <SummaryDocument
              content={summaries.draft}
              defaultTitle={documentTitle}
              compact
            />
          </div>
        </div>
      {/if}
    </div>
  {/if}

  {#snippet actions()}
    {#if !loading && !loadError}
      {#if phase === "generate"}
        {#if canReview}
          <Button variant="soft" onclick={() => (phase = "review")}>{t("page.summary.review")}</Button>
        {/if}
        <Button variant="primary" loading={generating} onclick={requestGeneration}>
          {saved ? t("page.summary.regenerate") : t("page.summary.generate")}
        </Button>
      {:else if phase === "review"}
        <Button
          variant="soft"
          loading={saving}
          disabled={generating || !summaries.dirty}
          onclick={() => void save()}
        >
          {t("page.snippets.save")}
        </Button>
        <Button variant="primary" disabled={!canSend} onclick={() => (phase = "send")}>
          {t("page.summary.continue")}
        </Button>
      {:else}
        <Button variant="ghost" onclick={() => (phase = "review")}>{t("page.summary.back")}</Button>
        <Button
          variant="primary"
          loading={sending}
          disabled={!canSend}
          onclick={() => void send()}
        >
          {mailBackend === "smtp" ? t("page.summary.sendMail") : t("page.summary.openDraft")}
        </Button>
      {/if}
    {/if}
  {/snippet}
</Modal>

{#if confirm === "discard"}
  <ConfirmDialog
    title={t("page.summary.discardTitle")}
    body={t("page.summary.discardBody")}
    confirmLabel={t("page.summary.discard")}
    tone="danger"
    onConfirm={onClose}
    onCancel={() => (confirm = null)}
  />
{:else if confirm === "regenerate"}
  <ConfirmDialog
    title={t("page.summary.regenTitle")}
    body={t("page.summary.regenBody")}
    confirmLabel={t("page.summary.regenerate")}
    onConfirm={() => void generate()}
    onCancel={() => (confirm = null)}
  />
{/if}
