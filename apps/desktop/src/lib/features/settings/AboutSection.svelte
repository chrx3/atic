<script lang="ts">
  /**
   * Quiénes somos, qué versión corre y si hay un instalador nuevo en GitHub.
   */
  import { getVersion } from "@tauri-apps/api/app";
  import { appUpdate } from "$domain/appUpdate.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import { GITHUB_RELEASES_URL, GITHUB_REPO_URL } from "$ipc/updates";
  import { openExternalUrl } from "$ipc/config";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Banner from "$ui/Banner.svelte";
  import Button from "$ui/Button.svelte";
  import { t } from "$domain/i18n.svelte";
  import Chip from "$ui/Chip.svelte";
  import ProgressBar from "$ui/ProgressBar.svelte";

  let version = $state("");

  const busy = $derived(appUpdate.busy);
  const buildLabel = $derived(
    version
      ? `${navigator.userAgent.includes("Mac") ? "macOS" : "Windows"} · v${version}`
      : "…",
  );

  $effect(() => {
    void getVersion()
      .then((value) => {
        version = value;
      })
      .catch(toastError);
  });
</script>

<div class="flex flex-col gap-5">
  <div
    class="flex flex-col items-center gap-1.5 rounded-sm border border-line px-4 py-5
           text-center"
  >
    <p class="text-lg font-semibold text-text">Atic</p>
    <p class="max-w-[40ch] text-xs leading-relaxed text-muted">
      {t("about.tagline")}
    </p>
    {#if version}
      <Chip tone="neutral">v{version}</Chip>
    {/if}
  </div>

  <SettingsGroup title={t("about.details")}>
    <SettingsRow label={t("about.build")}>
      {#snippet control()}
        <p class="font-mono text-sm text-text" data-numeric>{buildLabel}</p>
      {/snippet}
    </SettingsRow>
    <SettingsRow label={t("about.id")}>
      {#snippet control()}
        <p class="font-mono text-sm text-text">com.ciat.atic</p>
      {/snippet}
    </SettingsRow>
    <SettingsRow label={t("about.license")}>
      {#snippet control()}
        <p class="text-sm text-text">MIT</p>
      {/snippet}
    </SettingsRow>
    <SettingsRow label={t("about.code")} hint={t("about.codeHint")}>
      {#snippet control()}
        <Button
          variant="soft"
          size="sm"
          full
          onclick={() => void openExternalUrl(GITHUB_REPO_URL).catch(toastError)}
        >
          {t("about.github")}
        </Button>
      {/snippet}
    </SettingsRow>
  </SettingsGroup>

  {#if appUpdate.installing}
    <Banner tone="info" title={t("about.installingTitle", { version: appUpdate.version ?? "" })}>
      {t("about.installingBody")}
    </Banner>
  {:else if appUpdate.downloaded && !appUpdate.downloading}
    <Banner tone="info" title={t("about.readyTitle", { version: appUpdate.version ?? "" })}>
      {t("about.readyBody")}
    </Banner>
  {:else if appUpdate.pending && !appUpdate.downloading}
    <Banner tone="info" title={t("about.availableTitle", { version: appUpdate.version ?? "" })}>
      {t("about.availableBody")}
    </Banner>
  {:else if appUpdate.error}
    <Banner tone="warn" title={t("about.checkFailed")}>
      {appUpdate.error}
    </Banner>
  {:else if appUpdate.checked && !appUpdate.checking && !appUpdate.pending}
    <Banner tone="info" title={t("about.upToDate")} />
  {/if}

  <SettingsGroup title={t("about.updates")} hint={t("about.updatesHint")}>
    <SettingsRow bare>
      {#snippet control()}
        <div class="flex flex-col gap-2">
          {#if appUpdate.downloaded && !appUpdate.installing}
            <Button
              variant="primary"
              size="sm"
              full
              disabled={busy}
              onclick={() => void appUpdate.apply()}
            >
              {t("about.install", { version: appUpdate.version ?? "" })}
            </Button>
          {:else if appUpdate.pending && !appUpdate.downloading && !appUpdate.installing}
            <Button
              variant="primary"
              size="sm"
              full
              disabled={busy}
              onclick={() => void appUpdate.download()}
            >
              {t("about.download", { version: appUpdate.version ?? "" })}
            </Button>
          {/if}
          <Button
            variant={appUpdate.pending ? "soft" : "primary"}
            size="sm"
            full
            loading={appUpdate.checking}
            disabled={busy || appUpdate.checking}
            onclick={() => void appUpdate.check({ force: true })}
          >
            {appUpdate.checking ? t("about.checking") : t("about.check")}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            full
            onclick={() => void openExternalUrl(GITHUB_RELEASES_URL).catch(toastError)}
          >
            {t("about.releases")}
          </Button>
        </div>
      {/snippet}
    </SettingsRow>

    {#if appUpdate.downloading}
      <SettingsRow bare>
        {#snippet control()}
          <ProgressBar
            value={(appUpdate.percent ?? 0) / 100}
            indeterminate={appUpdate.percent === null}
            label={t("about.downloading", { version: appUpdate.version ?? "" })}
            tone="ok"
          />
        {/snippet}
      </SettingsRow>
    {:else if appUpdate.installing}
      <SettingsRow bare>
        {#snippet control()}
          <ProgressBar
            value={1}
            indeterminate
            label={t("about.installing", { version: appUpdate.version ?? "" })}
            tone="ok"
          />
        {/snippet}
      </SettingsRow>
    {/if}
  </SettingsGroup>
</div>
