<script lang="ts">
  /**
   * Dictado: hablar y que el texto se pegue donde estabas.
   *
   * Lo que se usa es el atajo global, no esta pantalla. Por eso la barra de
   * arriba es el interruptor —el mismo botón que la rueda de la pill— con el
   * medidor mientras escucha, y el cuerpo muestra lo único que no se ve desde
   * otro lado: la tecla, cómo se aprieta y el último resultado.
   *
   * El motor y el modo viven en Ajustes; acá queda una línea de estado y el
   * camino para cambiarlos, sin repetir el formulario.
   */
  import { formatShortcut } from "$core/format";
  import { config } from "$domain/config.svelte";
  import { dictation } from "$domain/dictation.svelte";
  import { t } from "$domain/i18n.svelte";
  import { paste } from "$domain/paste.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import type { SettingsSectionId } from "$features/settings/settingsSections";
  import { writeSystemClipboardText } from "$ipc/clipboard";
  import ToolPage from "$patterns/ToolPage.svelte";
  import Toolbar from "$patterns/Toolbar.svelte";
  import Button from "$ui/Button.svelte";
  import Chip from "$ui/Chip.svelte";
  import Kbd from "$ui/Kbd.svelte";
  import Meter from "$ui/Meter.svelte";

  let { onOpenSettings }: { onOpenSettings?: (section?: SettingsSectionId) => void } =
    $props();

  const PHASE = $derived({
    idle: { label: t("page.dictation.idle"), tone: "neutral" as const },
    listening: { label: t("page.dictation.listening"), tone: "rec" as const },
    transcribing: { label: t("page.dictation.transcribing"), tone: "info" as const },
    pasted: { label: t("page.dictation.pasted"), tone: "ok" as const },
    error: { label: t("page.dictation.error"), tone: "danger" as const },
  });

  const shortcut = $derived(config.current?.dictation_shortcut ?? "");
  const pushToTalk = $derived(config.current?.dictation_mode === "push_to_talk");
  /** Instrucción según el modo: qué hacer con la tecla. */
  const how = $derived(
    pushToTalk ? t("page.dictation.hold") : t("page.dictation.toggle"),
  );
  /** Nombre del modo, el mismo que muestra Ajustes. */
  const mode = $derived(
    pushToTalk ? t("settings.dictation.hold") : t("settings.dictation.toggle"),
  );

  /**
   * Lo que se muestra: lo que espera en la cola manda, porque es lo que
   * todavía no llegó a destino. Si no hay cola, el último dictado.
   */
  const shown = $derived(
    paste.count > 0 ? (paste.front?.text ?? null) : dictation.lastText,
  );
  const queued = $derived(paste.count > 0);

  async function copy(text: string) {
    try {
      await writeSystemClipboardText(text);
      toasts.push(t("page.dictation.copied"));
    } catch (error) {
      toastError(error);
    }
  }

  async function toggle() {
    await dictation.toggle().catch(toastError);
  }
</script>

<ToolPage
  title={t("tools.dictation.label")}
  icon="dictation"
  kicker={t("tools.dictation.short")}
  blurb={t("tools.dictation.blurb")}
>
  <div class="flex h-full min-h-0 flex-col">
    <Toolbar label={t("page.dictation.actions")}>
      <Button
        variant={dictation.active ? "danger-solid" : "primary"}
        size="md"
        loading={dictation.phase === "transcribing"}
        onclick={() => void toggle()}
      >
        {dictation.phase === "transcribing"
          ? t("page.dictation.transcribing")
          : dictation.active
            ? t("tools.dictation.stop")
            : t("tools.dictation.start")}
      </Button>

      {#snippet end()}
        {#if dictation.active}
          <span class="flex items-center gap-2.5">
            <Chip tone={PHASE[dictation.phase].tone}
              >{PHASE[dictation.phase].label}</Chip
            >
            {#if dictation.phase === "listening"}
              <div class="w-32">
                <Meter
                  value={dictation.mic}
                  tone="mic"
                  label={t("page.dictation.mic")}
                />
              </div>
            {/if}
          </span>
        {/if}
      {/snippet}
    </Toolbar>

    <div class="min-h-0 flex-1 overflow-y-auto">
      <div class="flex min-h-full flex-col items-center justify-center gap-3 px-4 py-6">
        {#if shortcut}
          <p
            class="flex flex-wrap items-center justify-center gap-1.5 text-xs text-muted"
          >
            {t("page.dictation.press")}
            <Kbd combo={formatShortcut(shortcut)} separator="+" size="md" />
          </p>
          <p class="max-w-sm text-sm text-muted text-pretty text-center">{how}</p>
        {:else}
          <p class="max-w-sm text-sm text-muted text-pretty text-center">
            {t("page.dictation.noShortcut")}
          </p>
          <Button
            variant="soft"
            size="sm"
            onclick={() => onOpenSettings?.("shortcuts")}
          >
            {t("page.dictation.assignShortcut")}
          </Button>
        {/if}

        <p class="max-w-xs text-xs text-faint text-pretty text-center">
          {t("page.dictation.hereNote")}
        </p>

        {#if dictation.phase === "error" && dictation.message}
          <p class="max-w-sm text-sm text-danger text-pretty text-center" role="status">
            {dictation.message}
          </p>
        {/if}

        <!--
          Lo dictado, en la pantalla que lo dictó. Si no hubo dónde pegarlo
          —o sea: no había otra app con el foco— el texto sigue acá adentro y
          es esta la superficie desde donde se despacha.
        -->
        {#if shown}
          <section
            class="flex w-full max-w-sm flex-col gap-1.5 rounded-md bg-surface-2 p-3 text-left"
          >
            <div class="flex items-center gap-2">
              <span class="text-micro text-muted uppercase">
                {queued ? t("page.dictation.queued") : t("page.dictation.last")}
              </span>
              <div class="ml-auto flex shrink-0 items-center gap-1">
                {#if queued}
                  <Button
                    variant="soft"
                    size="sm"
                    disabled={paste.busy}
                    onclick={() => void paste.paste()}
                  >
                    {t("page.common.paste")}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    disabled={paste.busy}
                    onclick={() => void paste.dismiss()}
                  >
                    {t("chrome.dismiss")}
                  </Button>
                {/if}
                <Button variant="ghost" size="sm" onclick={() => void copy(shown)}>
                  {t("page.common.copy")}
                </Button>
              </div>
            </div>
            <p class="text-sm whitespace-pre-wrap text-text">{shown}</p>
            {#if dictation.phase !== "error" && dictation.message}
              <p class="text-xs text-faint" role="status">{dictation.message}</p>
            {/if}
          </section>
        {/if}

        <div class="mt-2 flex items-center gap-1.5">
          <span class="text-micro text-faint uppercase">
            {t("settings.dictation.mode")}
          </span>
          <span class="text-xs text-muted">
            {mode}
          </span>
          <Button
            variant="ghost"
            size="sm"
            onclick={() => onOpenSettings?.("dictation")}
          >
            {t("page.dictation.settings")}
          </Button>
        </div>
      </div>
    </div>
  </div>
</ToolPage>
