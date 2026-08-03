<script lang="ts">
  /**
   * De dónde salen los resúmenes.
   *
   * Es la única parte de Atic que manda datos fuera de la máquina, así que la
   * pantalla lo dice en vez de darlo por sentado. Ollama es la excepción: corre
   * local y por eso no pide clave.
   *
   * Las claves se escriben pero NO se leen: Rust las guarda en el llavero del
   * sistema y solo informa si hay una puesta. Un campo que muestre la clave
   * guardada es una filtración esperando a que alguien comparta pantalla.
   */
  import type { SummaryProvider } from "$core/types";
  import { config } from "$domain/config.svelte";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import { secretsStatus, setSecret } from "$ipc/config";
  import { listSummaryProviders, ollamaAvailable } from "$ipc/summaries";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Banner from "$ui/Banner.svelte";
  import Button from "$ui/Button.svelte";
  import Input from "$ui/Input.svelte";
  import Select from "$ui/Select.svelte";

  const cfg = $derived(config.current);

  let providers = $state<SummaryProvider[]>([]);
  let hasKey = $state<Record<string, boolean>>({});
  let ollamaUp = $state<boolean | null>(null);
  let key = $state("");
  let savingKey = $state(false);

  async function load() {
    try {
      const [list, status] = await Promise.all([
        listSummaryProviders(),
        secretsStatus(),
      ]);
      providers = list;
      hasKey = status.providers ?? {};
    } catch (error) {
      toastError(error);
    }
  }

  $effect(() => {
    void load();
  });

  // Solo se pregunta si el proveedor elegido es Ollama: es una consulta a un
  // servidor local que puede no estar, y hacerla siempre agregaría una espera
  // por nada.
  $effect(() => {
    if (cfg?.summary_backend !== "ollama") {
      ollamaUp = null;
      return;
    }
    void ollamaAvailable()
      .then((up) => (ollamaUp = up))
      .catch(() => (ollamaUp = false));
  });

  const provider = $derived(providers.find((p) => p.id === cfg?.summary_backend));

  function patch(changes: Parameters<typeof config.patch>[0]) {
    void config.patch(changes).catch(toastError);
  }

  async function saveKey() {
    const kind = provider?.secret_kind;
    if (!kind || !key.trim()) return;
    savingKey = true;
    try {
      await setSecret(kind, key.trim());
      key = "";
      toasts.push("Clave guardada en el llavero del sistema");
      await load();
    } catch (error) {
      toastError(error);
    } finally {
      savingKey = false;
    }
  }
</script>

{#if cfg}
  <div class="flex flex-col gap-5">
    {#if provider?.needs_api_key && !hasKey[provider.id]}
      <Banner tone="warn" title="Falta la clave de {provider.display_name}" />
    {:else if ollamaUp === false}
      <Banner tone="warn" title="Ollama no está respondiendo">
        Arrancalo o elegí otro proveedor.
      </Banner>
    {/if}

    <SettingsGroup
      title="Proveedor"
      hint="El resumen es lo único que sale de tu máquina. Ollama corre local."
    >
      <SettingsRow label="Quién resume">
        {#snippet control({ id })}
          <Select
            {id}
            value={cfg.summary_backend}
            options={providers.map((p) => ({ value: p.id, label: p.display_name }))}
            onchange={(e: Event) => {
              const next = providers.find(
                (p) => p.id === (e.currentTarget as HTMLSelectElement).value,
              );
              if (!next) return;
              // Cambiar de proveedor arrastra su modelo y su URL por defecto:
              // dejar los del anterior produce una combinación que no existe.
              patch({
                summary_backend: next.id,
                summary_model: next.default_model,
                summary_base_url: next.default_base_url,
              });
            }}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow label="Modelo">
        {#snippet control({ id })}
          {#if provider && provider.suggested_models.length > 0}
            <Select
              {id}
              value={cfg.summary_model}
              options={provider.suggested_models.map((m) => ({ value: m, label: m }))}
              onchange={(e: Event) =>
                patch({ summary_model: (e.currentTarget as HTMLSelectElement).value })}
            />
          {:else}
            <Input
              {id}
              mono
              value={cfg.summary_model}
              oninput={(e: Event) =>
                patch({ summary_model: (e.currentTarget as HTMLInputElement).value })}
            />
          {/if}
        {/snippet}
      </SettingsRow>

      {#if provider?.base_url_editable}
        <SettingsRow label="URL" hint="Dónde escucha el proveedor.">
          {#snippet control({ id })}
            <Input
              {id}
              mono
              value={cfg.summary_base_url}
              oninput={(e: Event) =>
                patch({
                  summary_base_url: (e.currentTarget as HTMLInputElement).value,
                })}
            />
          {/snippet}
        </SettingsRow>
      {/if}
    </SettingsGroup>

    {#if provider?.needs_api_key}
      <SettingsGroup
        title="Clave"
        hint="Se guarda en el llavero del sistema, no en un archivo de la app."
      >
        <SettingsRow
          label={hasKey[provider.id] ? "Reemplazar la clave" : "Poner la clave"}
          hint={hasKey[provider.id]
            ? "Hay una guardada. No se puede leer, solo cambiar."
            : undefined}
        >
          {#snippet control({ id })}
            <div class="flex gap-1.5">
              <Input
                {id}
                type="password"
                mono
                bind:value={key}
                placeholder="sk-…"
                autocomplete="off"
              />
              <Button
                variant="soft"
                size="sm"
                loading={savingKey}
                disabled={!key.trim()}
                onclick={() => void saveKey()}
              >
                Guardar
              </Button>
            </div>
          {/snippet}
        </SettingsRow>
      </SettingsGroup>
    {/if}
  </div>
{/if}
