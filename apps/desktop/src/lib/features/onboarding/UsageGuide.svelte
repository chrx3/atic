<script lang="ts">
  /**
   * Cómo se usa Atic: la pill y los tres atajos que hay que recordar.
   *
   * Vive aparte porque tiene dos casas. Sale en el último paso del onboarding,
   * y también desde Ajustes para quien se lo salteó —que es justo quien más lo
   * necesita— sin obligarlo a repetir consentimiento ni descargas.
   */
  import { formatShortcut } from "$core/format";
  import { config } from "$domain/config.svelte";
  import Kbd from "$ui/Kbd.svelte";

  const cfg = $derived(config.current);

  /**
   * Tres atajos, no siete.
   *
   * Los demás se descubren desde la rueda; una lista completa acá sería
   * imposible de recordar y por eso no serviría para nada.
   */
  const KEYS = $derived(
    cfg
      ? [
          {
            combo: cfg.pill_radial_shortcut,
            title: "Rueda de herramientas",
            body: "Mantené la tecla y aparece en el cursor. Elegís con la rueda del mouse y soltás para activar.",
          },
          {
            combo: cfg.dictation_shortcut,
            title: "Dictar",
            body: "Hablás y el texto se pega donde tengas el cursor, en cualquier app.",
          },
          {
            combo: cfg.clipboard_shortcut,
            title: "Historial del portapapeles",
            body: "Todo lo que copiaste, para volver a pegarlo.",
          },
        ]
      : [],
  );
</script>

<div class="flex flex-col gap-3">
  <div class="flex flex-col gap-1">
    <p class="text-sm font-medium text-text">Atic vive fuera de esta ventana</p>
    <p class="max-w-[60ch] text-sm leading-relaxed text-muted">
      Esta ventana es para revisar lo que grabaste. El día a día pasa en la
      <strong class="font-medium text-text">pill</strong>: una pastilla flotante que
      queda sobre lo que estés haciendo, y que podés arrastrar adonde te sirva.
    </p>
  </div>

  <!-- El atajo primero y con ancho fijo: la columna de teclas se lee de un
       vistazo, que es lo que alguien vuelve a mirar cuando olvida uno. -->
  <ul class="flex list-none flex-col gap-2.5">
    {#each KEYS as key (key.combo)}
      <li class="grid grid-cols-[8rem_minmax(0,1fr)] items-baseline gap-3">
        <Kbd combo={formatShortcut(key.combo)} separator="+" />
        <span class="text-sm leading-relaxed text-muted">
          <strong class="font-medium text-text">{key.title}.</strong>
          {key.body}
        </span>
      </li>
    {/each}
  </ul>

  <p class="text-xs text-faint">
    Se cambian en Ajustes → Atajos. Si alguno ya lo usa otra app, Atic te avisa para que
    elijas otro.
  </p>
</div>
