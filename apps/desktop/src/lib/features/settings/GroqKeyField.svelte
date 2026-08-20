<script lang="ts">
  /**
   * Clave de Groq para dictado, reuniones y resúmenes si el proveedor es Groq.
   *
   * Se escribe y no se lee: Rust la guarda en el llavero. El enlace abre la
   * consola de Groq en el navegador, no dentro de Atic.
   */
  import { toastError, toasts } from "$domain/toasts.svelte";
  import { GROQ_KEYS_URL, openExternalUrl, secretsStatus, setSecret } from "$ipc/config";
  import Banner from "$ui/Banner.svelte";
  import Button from "$ui/Button.svelte";
  import Input from "$ui/Input.svelte";
  import { t } from "$domain/i18n.svelte";

  let {
    missingHint,
  }: { missingHint?: string } = $props();
  const hint = $derived(missingHint ?? t("settings.dictation.groqKeyHint"));
  let hasKey = $state(false);
  let key = $state("");
  let saving = $state(false);

  async function load() {
    try {
      const status = await secretsStatus();
      hasKey = Boolean(status.providers?.groq);
    } catch (error) {
      toastError(error);
    }
  }

  $effect(() => {
    void load();
  });

  async function save() {
    if (!key.trim()) return;
    saving = true;
    try {
      await setSecret("groq_api_key", key.trim());
      key = "";
      hasKey = true;
      toasts.push(t("settings.groq.saved"));
    } catch (error) {
      toastError(error);
    } finally {
      saving = false;
    }
  }
</script>

<div class="flex flex-col gap-2">
  {#if hasKey}
    <Banner tone="info" title={t("settings.groq.hasKey")} />
  {:else}
    <Banner tone="warn" title={t("settings.groq.missingKey")}>
      {hint}
    </Banner>
  {/if}

  <div class="flex gap-1.5">
    <Input
      type="password"
      mono
      bind:value={key}
      placeholder={hasKey ? "••••••••" : "gsk_…"}
      autocomplete="off"
      aria-label={hasKey ? t("settings.groq.replaceAria") : t("settings.groq.keyAria")}
    />
    <Button
      variant="soft"
      size="sm"
      loading={saving}
      disabled={!key.trim()}
      onclick={() => void save()}
    >
      {t("settings.groq.save")}
    </Button>
  </div>

  <p class="text-xs leading-relaxed text-faint">
    {t("settings.groq.freeBefore")}
    <a
      href={GROQ_KEYS_URL}
      class="text-accent underline-offset-2 hover:underline"
      onclick={(event) => {
        event.preventDefault();
        void openExternalUrl(GROQ_KEYS_URL).catch(toastError);
      }}
    >
      console.groq.com/keys
    </a>.
  </p>
</div>
