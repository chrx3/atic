<script lang="ts">
  /**
   * Cómo se usa Atic: la pill y los tres atajos que hay que recordar.
   *
   * Vive aparte porque tiene dos casas. Sale en el último paso del onboarding,
   * y también desde Ajustes para quien se lo saltó —que es justo quien más lo
   * necesita— sin obligarlo a repetir consentimiento ni descargas.
   */
  import type { AppConfig } from "$lib/types";
  import { formatShortcut } from "$lib/format";

  let { config }: { config: AppConfig } = $props();
</script>

<p class="font-medium">Atic vive fuera de esta ventana</p>
<p class="leading-relaxed" style="color: var(--rb-muted)">
  Esta ventana es para revisar lo que grabaste. El día a día pasa en la
  <strong>pill</strong>: una pastilla flotante que queda sobre lo que estés
  haciendo, y que puedes arrastrar adonde te sirva.
</p>

<!-- Tres atajos, no siete. Los demás se descubren desde la rueda; una lista
     completa acá sería imposible de recordar. -->
<ul class="ug-keys">
  <li>
    <kbd class="ug-key">{formatShortcut(config.pill_radial_shortcut)}</kbd>
    <span class="ug-key-copy">
      <strong>Rueda de herramientas.</strong>
      Mantén la tecla y aparece en el cursor. Elige con la rueda del ratón y
      suelta para activar. Un clic en la pill también abre la rueda.
    </span>
  </li>
  <li>
    <kbd class="ug-key">{formatShortcut(config.dictation_shortcut)}</kbd>
    <span class="ug-key-copy">
      <strong>Dictar.</strong>
      Habla y el texto se pega donde tengas el cursor, en cualquier app.
    </span>
  </li>
  <li>
    <kbd class="ug-key">{formatShortcut(config.clipboard_shortcut)}</kbd>
    <span class="ug-key-copy">
      <strong>Historial del portapapeles.</strong>
      Todo lo que copiaste, para volver a pegarlo.
    </span>
  </li>
</ul>

<p class="rb-hint">
  Se cambian en Ajustes → Atajos. Si alguno ya lo usa otra app, Atic te avisa
  para que elijas otro.
</p>

<style>
  .ug-keys {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .ug-keys li {
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
  }

  /* El atajo primero y con ancho fijo: la columna de teclas se lee de un
     vistazo, que es lo que alguien vuelve a mirar cuando olvida uno. */
  .ug-key {
    flex-shrink: 0;
    min-width: 7.5rem;
    border-radius: 0.4rem;
    padding: 0.2rem 0.45rem;
    background: var(--rb-bg0);
    color: var(--rb-text);
    font-family: inherit;
    font-size: 0.75rem;
    font-weight: 650;
    text-align: center;
  }

  .ug-key-copy {
    color: var(--rb-muted);
    font-size: 0.8125rem;
    line-height: 1.45;
  }

  .ug-key-copy strong {
    color: var(--rb-text);
  }
</style>
