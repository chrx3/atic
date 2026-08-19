<script lang="ts">
  /** Tema, arranque, tutorial y retención. */
  import { config } from "$domain/config.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import { openDataDir } from "$ipc/config";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import { useMainUi } from "$surfaces/main/mainUi.svelte";
  import Button from "$ui/Button.svelte";
  import Input from "$ui/Input.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import Switch from "$ui/Switch.svelte";
  import { applyTheme, type UiTheme } from "$lib/theme";

  const cfg = $derived(config.current);
  const ui = useMainUi();

  function patch(changes: Parameters<typeof config.patch>[0]) {
    void config.patch(changes).catch(toastError);
  }

  function setTheme(theme: UiTheme) {
    // Se aplica al DOM en el acto y se guarda después: esperar el viaje a Rust
    // para repintar hace que el tema se sienta pegajoso.
    applyTheme(theme);
    patch({ ui_theme: theme });
  }
</script>

{#if cfg}
  <div class="flex flex-col gap-5">
    <SettingsGroup title="Apariencia">
      <SettingsRow label="Tema" hint="El claro se deriva del oscuro.">
        {#snippet control()}
          <SegmentedControl
            value={(cfg.ui_theme || "system") as UiTheme}
            label="Tema de la interfaz"
            options={[
              { value: "system" as UiTheme, label: "Sistema" },
              { value: "light" as UiTheme, label: "Claro" },
              { value: "dark" as UiTheme, label: "Oscuro" },
            ]}
            onchange={setTheme}
            full
          />
        {/snippet}
      </SettingsRow>
    </SettingsGroup>

    <SettingsGroup title="Arranque">
      <SettingsRow bare>
        {#snippet control()}
          <Switch
            checked={cfg.autostart}
            label="Arrancar con el sistema"
            hint="Atic queda en la bandeja, listo para los atajos."
            onchange={(v) => patch({ autostart: v })}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow bare>
        {#snippet control()}
          <Switch
            checked={cfg.show_pill}
            label="Mostrar la pill"
            hint="La superficie flotante desde donde se graba y se pega."
            onchange={(v) => patch({ show_pill: v })}
          />
        {/snippet}
      </SettingsRow>
    </SettingsGroup>

    <SettingsGroup
      title="Tutorial"
      hint="Consentimiento, Groq o local, atajos y práctica junto a la pill."
    >
      <SettingsRow label="Primer uso">
        {#snippet control()}
          <Button
            variant="soft"
            size="sm"
            full
            onclick={() => void ui.replayOnboarding().catch(toastError)}
          >
            Repetir tutorial
          </Button>
        {/snippet}
      </SettingsRow>
    </SettingsGroup>

    <SettingsGroup
      title="Datos"
      hint="Todo se guarda en tu disco. Nada sale de la máquina salvo el resumen, si lo configuraste."
    >
      <SettingsRow
        label="Conservar grabaciones"
        hint="En días. 0 las guarda para siempre."
      >
        {#snippet control({ id })}
          <Input
            {id}
            type="number"
            min="0"
            value={String(cfg.retention_days)}
            oninput={(e: Event) =>
              patch({
                retention_days:
                  Number((e.currentTarget as HTMLInputElement).value) || 0,
              })}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow bare>
        {#snippet control()}
          <Switch
            checked={cfg.retention_auto_cleanup}
            label="Borrar las vencidas solas"
            onchange={(v) => patch({ retention_auto_cleanup: v })}
          />
        {/snippet}
      </SettingsRow>

      <SettingsRow label="Carpeta de datos" hint="Grabaciones, textos y capturas.">
        {#snippet control()}
          <Button
            variant="soft"
            size="sm"
            full
            onclick={() => void openDataDir("data").catch(toastError)}
          >
            Abrir carpeta
          </Button>
        {/snippet}
      </SettingsRow>
    </SettingsGroup>
  </div>
{/if}
