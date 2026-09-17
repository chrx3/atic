<script lang="ts">
  /**
   * El hero: la promesa y nada más. El demo vive justo abajo, así que acá no
   * hace falta explicar de más — la prueba está a un scroll.
   */
  import AticMark from "$lib/atic/AticMark.svelte";
  import Icon from "$lib/atic/Icon.svelte";
  import GithubIcon from "$lib/atic/GithubIcon.svelte";
  import { Download } from "$lib/atic/icons";
</script>

<section class="hero" id="top">
  <div class="wrap">
    <span class="eyebrow">
      <span class="dot"></span>
      Caja de herramientas de escritorio · local-first
    </span>

    <div class="mark"><AticMark size={72} alive={true} strokeWidth={1.15} /></div>

    <h1>
      Tus herramientas,<br />
      <em>flotando</em> sobre el escritorio.
    </h1>

    <p class="sub">
      Atic vive en una pill, no en una ventana que hay que ir a buscar: conversa con
      los agentes CLI que ya tienes, lanza apps, dicta, captura y guarda — sin salir
      de lo que estabas haciendo.
    </p>

    <div class="actions">
      <a class="primary" href="#demo">
        Probar el demo
        <span class="arrow">↓</span>
      </a>
      <a class="secondary" href="https://github.com/chrx3/atic/releases/latest" target="_blank" rel="noreferrer">
        <Icon icon={Download} size={14} /> Descargar para Windows
      </a>
      <a class="secondary" href="https://github.com/chrx3/atic" target="_blank" rel="noreferrer">
        <GithubIcon size={14} /> Código
      </a>
    </div>

    <p class="note">MIT · sin cuenta · sin telemetría · Windows y macOS</p>
  </div>
</section>

<style>
  .wrap {
    display: flex;
    max-width: 760px;
    flex-direction: column;
    gap: 18px;
    align-items: center;
    margin: 0 auto;
    padding: clamp(48px, 9vh, 96px) 20px 24px;
    text-align: center;
  }

  .eyebrow {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    border: 1px solid var(--line);
    border-radius: var(--radius-pill);
    padding: 6px 13px;
    color: var(--muted);
    font-size: var(--text-micro);
    font-weight: var(--font-weight-medium);
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--ok);
  }

  .mark {
    display: grid;
    width: 108px;
    height: 108px;
    place-items: center;
    border: 1px solid var(--line);
    border-radius: 30px;
    background: var(--skin);
    box-shadow: var(--shadow-pop);
  }

  h1 {
    margin: 4px 0 0;
    font-family: var(--font-display);
    font-size: clamp(2rem, 5.4vw, 3.125rem);
    font-weight: var(--font-weight-semibold);
    line-height: 1.08;
    letter-spacing: -0.035em;
    text-wrap: balance;
  }

  h1 em {
    font-style: normal;
    color: var(--muted);
  }

  .sub {
    max-width: 54ch;
    margin: 0;
    color: var(--muted);
    font-size: clamp(0.875rem, 1.6vw, 1rem);
    line-height: 1.65;
    text-wrap: pretty;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    justify-content: center;
    padding-top: 4px;
  }

  .primary,
  .secondary {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    border-radius: var(--radius-sm);
    padding: 11px 18px;
    font-size: var(--text-base);
    font-weight: var(--font-weight-medium);
    text-decoration: none;
    transition:
      transform var(--duration-quick) var(--ease-smooth-out),
      opacity var(--duration-quick) var(--ease-smooth-out);
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

  .primary:hover,
  .secondary:hover {
    transform: translateY(-1px);
  }

  .primary:active,
  .secondary:active {
    transform: translateY(0) scale(0.99);
  }

  .arrow {
    font-size: 0.8em;
    opacity: 0.7;
  }

  .note {
    margin: 0;
    color: var(--faint);
    font-size: var(--text-micro);
    letter-spacing: 0.02em;
  }
</style>
