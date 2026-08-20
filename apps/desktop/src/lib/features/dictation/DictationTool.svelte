<script lang="ts">
  /**
   * Dictado: hablar y que el texto se pegue donde estabas.
   *
   * La herramienta es casi toda explicación, y está bien: lo que se usa es el
   * atajo global, no esta pantalla. Sirve para saber qué atajo es, en qué
   * estado está y probarlo sin salir de la app.
   */
  import { formatShortcut } from "$core/format";
  import { config } from "$domain/config.svelte";
  import { dictation } from "$domain/dictation.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import ToolPage from "$patterns/ToolPage.svelte";
  import Banner from "$ui/Banner.svelte";
  import Button from "$ui/Button.svelte";
  import Chip from "$ui/Chip.svelte";
  import Kbd from "$ui/Kbd.svelte";
  import { t } from "$domain/i18n.svelte";

  const PHASE = $derived({
    idle: { label: t("page.dictation.idle"), tone: "neutral" as const },
    listening: { label: t("page.dictation.listening"), tone: "rec" as const },
    transcribing: { label: t("page.dictation.transcribing"), tone: "info" as const },
    pasted: { label: t("page.dictation.pasted"), tone: "ok" as const },
    error: { label: t("page.dictation.error"), tone: "danger" as const },
  });

  const shortcut = $derived(config.current?.dictation_shortcut ?? "");
  const pushToTalk = $derived(config.current?.dictation_mode === "push_to_talk");
</script>

<ToolPage
  title={t("tools.dictation.label")}
  icon="dictation"
  kicker={t("tools.dictation.short")}
  blurb={t("tools.dictation.blurb")}
>
  {#snippet meta()}
    <Chip tone={PHASE[dictation.phase].tone}>{PHASE[dictation.phase].label}</Chip>
  {/snippet}

  <div class="flex h-full min-h-0 flex-col gap-3 overflow-y-auto p-3">
    {#if dictation.message}
      <Banner
        tone={dictation.phase === "error" ? "danger" : "info"}
        title={dictation.message}
      />
    {/if}

    <div class="flex flex-col gap-2">
      <p class="text-sm text-muted text-pretty">
        {#if shortcut}
          {t("page.dictation.press")} <Kbd combo={formatShortcut(shortcut)} separator="+" />
          {#if pushToTalk}
            {t("page.dictation.hold")}
          {:else}
            {t("page.dictation.toggle")}
          {/if}
        {:else}
          {t("page.dictation.noShortcut")}
        {/if}
      </p>

      <div>
        <Button
          variant={dictation.active ? "danger-solid" : "primary"}
          size="sm"
          onclick={() => void dictation.toggle().catch(toastError)}
        >
          {dictation.active ? t("tools.dictation.stop") : t("page.dictation.tryHere")}
        </Button>
      </div>
      <p class="text-xs text-faint">
        {t("page.dictation.hereNote")}
      </p>
    </div>
  </div>
</ToolPage>
