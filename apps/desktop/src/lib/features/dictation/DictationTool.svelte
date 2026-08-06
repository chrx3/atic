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

  const PHASE = {
    idle: { label: "En reposo", tone: "neutral" },
    listening: { label: "Escuchando", tone: "rec" },
    transcribing: { label: "Transcribiendo", tone: "info" },
    pasted: { label: "Pegado", tone: "ok" },
    error: { label: "Error", tone: "danger" },
  } as const;

  const shortcut = $derived(config.current?.dictation_shortcut ?? "");
  const pushToTalk = $derived(config.current?.dictation_mode === "push_to_talk");
</script>

<ToolPage
  title="Dictado"
  icon="dictation"
  kicker="Voz a texto"
  blurb="Hablá y el texto se pega en la app donde estabas."
>
  {#snippet meta()}
    <Chip tone={PHASE[dictation.phase].tone}>{PHASE[dictation.phase].label}</Chip>
  {/snippet}

  <div class="flex flex-col gap-4 p-4">
    {#if dictation.message}
      <Banner
        tone={dictation.phase === "error" ? "danger" : "info"}
        title={dictation.message}
      />
    {/if}

    <div class="flex flex-col gap-2">
      <p class="text-sm text-muted">
        {#if shortcut}
          Apretá <Kbd combo={formatShortcut(shortcut)} separator="+" /> en cualquier app.
          {#if pushToTalk}
            Mantenelo apretado mientras hablás y soltá para pegar.
          {:else}
            Una vez para empezar, otra para terminar.
          {/if}
        {:else}
          No hay atajo de dictado configurado. Se define en Ajustes.
        {/if}
      </p>

      <div>
        <Button
          variant={dictation.active ? "danger-solid" : "primary"}
          onclick={() => void dictation.toggle().catch(toastError)}
        >
          {dictation.active ? "Terminar" : "Probar acá"}
        </Button>
      </div>
      <p class="text-xs text-faint">
        Desde acá el texto se pega en esta ventana, no en otra app.
      </p>
    </div>
  </div>
</ToolPage>
