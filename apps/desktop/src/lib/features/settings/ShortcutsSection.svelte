<script lang="ts">
  /**
   * Los atajos globales.
   *
   * Se registran en el SO, así que otra app puede tenerlos tomados. Cuando eso
   * pasa Rust avisa y acá se marca cuál: un atajo que no funciona y no dice por
   * qué es de las cosas más frustrantes que puede hacer una app de escritorio.
   */
  import { config } from "$domain/config.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Banner from "$ui/Banner.svelte";
  import HotkeyCapture from "$ui/HotkeyCapture.svelte";
  import { AGENTS_ENABLED } from "$core/tools";

  const cfg = $derived(config.current);

  function patch(changes: Parameters<typeof config.patch>[0]) {
    void config.patch(changes).catch(toastError);
  }

  /** Los valores de fábrica, para el botón de restablecer. */
  const ALL_SHORTCUTS = [
    {
      key: "global_shortcut",
      label: "Grabar / parar",
      hint: "Empieza y termina una grabación desde cualquier app.",
      fallback: "CmdOrCtrl+Shift+R",
    },
    {
      key: "dictation_shortcut",
      label: "Dictar",
      hint: "Habla y el texto se pega donde estabas.",
      fallback: "CmdOrCtrl+Shift+D",
    },
    {
      key: "summon_pill_shortcut",
      label: "Traer la pill",
      hint: "La acerca al cursor.",
      fallback: "CmdOrCtrl+Shift+P",
    },
    {
      key: "pill_radial_shortcut",
      label: "Rueda de herramientas",
      hint: "Mantenelo apretado y soltá sobre la que quieras.",
      fallback: "CmdOrCtrl+Shift+Space",
    },
    {
      key: "clipboard_shortcut",
      label: "Historial del portapapeles",
      fallback: "CmdOrCtrl+Shift+V",
    },
    {
      key: "snippets_shortcut",
      label: "Textos guardados",
      fallback: "CmdOrCtrl+Shift+S",
    },
    {
      key: "agents_shortcut",
      label: "Consola de agentes",
      hint: "Abre o cierra el chat de agentes junto a la pill.",
      fallback: "CmdOrCtrl+Shift+A",
    },
    {
      key: "screenshot_shortcut",
      label: "Captura de pantalla",
      fallback: "CmdOrCtrl+Shift+4",
    },
    {
      key: "board_shortcut",
      label: "Dibujar en pantalla",
      hint: "Congela la pantalla y deja marcarla. Esc la saca.",
      fallback: "CmdOrCtrl+Shift+X",
    },
    {
      key: "launcher_shortcut",
      label: "Launcher",
      hint: "Buscar y abrir apps, como Spotlight.",
      fallback: "CmdOrCtrl+Space",
    },
  ] as const;

  const SHORTCUTS = AGENTS_ENABLED
    ? ALL_SHORTCUTS
    : ALL_SHORTCUTS.filter((item) => item.key !== "agents_shortcut");

  /** Rust manda los nombres tal como los registró. */
  const conflicts = $derived(new Set(config.conflicts));
</script>

{#if cfg}
  <div class="flex flex-col gap-5">
    {#if config.conflicts.length > 0}
      <Banner
        tone="warn"
        title={config.conflicts.length === 1
          ? "Un atajo ya lo tenía tomado otra app"
          : `${config.conflicts.length} atajos ya los tenía tomados otra app`}
      >
        Elegí otra combinación para los marcados.
      </Banner>
    {/if}

    <SettingsGroup
      title="Atajos globales"
      hint="Funcionan en cualquier app, no solo con Atic al frente."
    >
      {#each SHORTCUTS as item (item.key)}
        <SettingsRow
          label={conflicts.has(item.key) ? `${item.label} · en conflicto` : item.label}
          hint={"hint" in item ? item.hint : undefined}
        >
          {#snippet control()}
            <HotkeyCapture
              value={cfg[item.key]}
              defaultValue={item.fallback}
              ariaLabel="Cambiar el atajo de {item.label}"
              onChange={(sc) => patch({ [item.key]: sc })}
            />
          {/snippet}
        </SettingsRow>
      {/each}
    </SettingsGroup>
  </div>
{/if}
