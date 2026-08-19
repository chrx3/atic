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

  let {
    missingHint = "Sin ella el dictado cae a Whisper local.",
  }: { missingHint?: string } = $props();
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
      toasts.push("Clave de Groq guardada en el llavero");
    } catch (error) {
      toastError(error);
    } finally {
      saving = false;
    }
  }
</script>

<div class="flex flex-col gap-2">
  {#if hasKey}
    <Banner tone="info" title="Hay una clave de Groq en el llavero" />
  {:else}
    <Banner tone="warn" title="Falta la clave de Groq">
      {missingHint}
    </Banner>
  {/if}

  <div class="flex gap-1.5">
    <Input
      type="password"
      mono
      bind:value={key}
      placeholder={hasKey ? "••••••••" : "gsk_…"}
      autocomplete="off"
      aria-label={hasKey ? "Reemplazar la clave de Groq" : "Clave de Groq"}
    />
    <Button
      variant="soft"
      size="sm"
      loading={saving}
      disabled={!key.trim()}
      onclick={() => void save()}
    >
      Guardar
    </Button>
  </div>

  <p class="text-xs leading-relaxed text-faint">
    Gratis. Creá una cuenta y copiá la clave en
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
