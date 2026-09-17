<script lang="ts">
  /**
   * Descarga: el instalador de Windows del último release y el camino de
   * macOS, que hoy se compila desde el source.
   */
  import Icon from "$lib/atic/Icon.svelte";
  import AticMark from "$lib/atic/AticMark.svelte";
  import { Download, ExternalLink } from "$lib/atic/icons";
</script>

<section class="download" id="descargar">
  <div class="wrap">
    <div class="panel">
      <div class="copy">
        <span class="mark"><AticMark size={40} alive={true} /></span>
        <h2>Pruébalo en tu escritorio</h2>
        <p>
          La app completa vive en GitHub. Los modelos de Whisper se descargan en el
          primer uso; no van dentro del instalador.
        </p>
        <div class="actions">
          <a class="primary" href="https://github.com/chrx3/atic/releases/latest" target="_blank" rel="noreferrer">
            <Icon icon={Download} size={15} /> Último release
          </a>
          <a class="secondary" href="https://github.com/chrx3/atic/blob/main/docs/MACOS.md" target="_blank" rel="noreferrer">
            Compilar en macOS <Icon icon={ExternalLink} size={13} />
          </a>
        </div>
      </div>

      <ul class="platforms">
        <li>
          <h3>Windows 11</h3>
          <p>Plataforma completa: micrófono y audio del sistema, transcripción local y resúmenes.</p>
          <span class="tag">Instalador NSIS</span>
        </li>
        <li>
          <h3>macOS 11+</h3>
          <p>Audio del sistema con ScreenCaptureKit; firma y notarización pendientes.</p>
          <span class="tag">Compilar desde el source</span>
        </li>
      </ul>
    </div>

    <p class="note">
      Windows puede mostrar SmartScreen: la app todavía no lleva firma Authenticode.
    </p>
  </div>
</section>

<style>
  .wrap {
    max-width: 1080px;
    margin: 0 auto;
    padding: clamp(48px, 8vh, 84px) 20px 0;
  }

  .panel {
    display: grid;
    gap: 26px;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    padding: clamp(22px, 4vw, 34px);
    background:
      radial-gradient(520px 300px at 100% 0%, color-mix(in srgb, var(--surface-2) 80%, transparent) 0%, transparent 70%),
      var(--surface);
    grid-template-columns: 1.1fr 1fr;
    align-items: center;
  }

  .mark {
    display: grid;
    width: 62px;
    height: 62px;
    place-items: center;
    margin-bottom: 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: var(--skin);
  }

  h2 {
    margin: 0 0 10px;
    font-family: var(--font-display);
    font-size: clamp(1.375rem, 3vw, 1.875rem);
    font-weight: var(--font-weight-semibold);
    letter-spacing: -0.025em;
  }

  .copy p {
    max-width: 46ch;
    margin: 0 0 18px;
    color: var(--muted);
    font-size: var(--text-md);
    line-height: 1.6;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 9px;
  }

  .primary,
  .secondary {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    border-radius: var(--radius-sm);
    padding: 10px 16px;
    font-size: var(--text-base);
    font-weight: var(--font-weight-medium);
    text-decoration: none;
  }

  .primary {
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    background: var(--accent);
    color: var(--on-accent);
  }

  .secondary {
    border: 1px solid var(--line-strong);
    color: var(--text);
  }

  .secondary:hover {
    border-color: var(--text);
  }

  .platforms {
    display: flex;
    margin: 0;
    flex-direction: column;
    gap: 10px;
    padding: 0;
    list-style: none;
  }

  .platforms li {
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    padding: 14px 16px;
    background: var(--bg);
  }

  .platforms h3 {
    margin: 0 0 5px;
    font-size: var(--text-base);
    font-weight: var(--font-weight-semibold);
  }

  .platforms p {
    margin: 0 0 9px;
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.5;
  }

  .tag {
    display: inline-block;
    border: 1px solid var(--line);
    border-radius: var(--radius-pill);
    padding: 3px 9px;
    color: var(--faint);
    font-size: var(--text-micro);
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
  }

  .note {
    margin: 14px 0 0;
    color: var(--faint);
    font-size: var(--text-xs);
    text-align: center;
  }

  @media (max-width: 820px) {
    .panel {
      grid-template-columns: 1fr;
    }
  }
</style>
