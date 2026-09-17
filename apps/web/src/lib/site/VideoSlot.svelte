<script lang="ts">
  /**
   * La videoteca del sitio.
   *
   * Los videos se producen con Remotion y se embeben acá. Mientras no haya
   * ninguno, la sección lo dice sin disimularlo: el marco ya tiene la
   * proporción y el lugar donde va el `iframe` o el `video`.
   */
  import Icon from "$lib/atic/Icon.svelte";
  import { Play } from "$lib/atic/icons";

  type Video = {
    id: string;
    title: string;
    /** Ruta del render de Remotion o URL embebible. */
    src: string;
  };

  const VIDEOS: Video[] = [];

  let selected = $state<Video | null>(VIDEOS[0] ?? null);
</script>

<section class="videos" id="videos">
  <div class="wrap">
    <header class="head">
      <span class="eyebrow">Videos</span>
      <h2>El demo, en movimiento</h2>
      <p>
        Los recorridos de cada herramienta se producen con Remotion y se embeben
        acá, sobre el mismo demo interactivo.
      </p>
    </header>

    {#if selected}
      <div class="player">
        <iframe src={selected.src} title={selected.title} allowfullscreen></iframe>
      </div>
    {:else}
      <div class="player is-empty">
        <div class="empty">
          <span class="play"><Icon icon={Play} size={20} /></span>
          <p class="empty-title">El video llega pronto</p>
          <p class="empty-sub">
            Composición de Remotion en preparación. Mientras tanto, el demo de arriba
            ya se puede usar.
          </p>
          <span class="empty-path">apps/web/src/lib/site/VideoSlot.svelte → VIDEOS</span>
        </div>
      </div>
    {/if}
  </div>
</section>

<style>
  .wrap {
    max-width: 1080px;
    margin: 0 auto;
    padding: clamp(48px, 8vh, 84px) 20px 0;
  }

  .head {
    max-width: 620px;
    margin-bottom: 24px;
  }

  .eyebrow {
    color: var(--faint);
    font-size: var(--text-micro);
    font-weight: var(--font-weight-semibold);
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
  }

  h2 {
    margin: 10px 0 10px;
    font-family: var(--font-display);
    font-size: clamp(1.5rem, 3.4vw, 2.125rem);
    font-weight: var(--font-weight-semibold);
    letter-spacing: -0.025em;
  }

  .head p {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-md);
    line-height: 1.6;
  }

  .player {
    position: relative;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: #0d1016;
    aspect-ratio: 16 / 9;
  }

  .player iframe {
    width: 100%;
    height: 100%;
    border: 0;
  }

  .player.is-empty {
    display: grid;
    place-items: center;
    border-style: dashed;
    background:
      radial-gradient(600px 320px at 50% 0%, rgb(47 58 99 / 55%) 0%, transparent 70%),
      linear-gradient(160deg, #14161f 0%, #0d1016 60%, #090b10 100%);
  }

  .empty {
    display: flex;
    max-width: 420px;
    flex-direction: column;
    gap: 8px;
    align-items: center;
    padding: 24px;
    text-align: center;
  }

  .play {
    display: grid;
    width: 52px;
    height: 52px;
    place-items: center;
    border: 1px solid rgb(255 255 255 / 22%);
    border-radius: 50%;
    background: rgb(255 255 255 / 8%);
    color: #fff;
  }

  .empty-title {
    margin: 6px 0 0;
    color: #fff;
    font-size: var(--text-md);
    font-weight: var(--font-weight-semibold);
  }

  .empty-sub {
    margin: 0;
    color: rgb(255 255 255 / 66%);
    font-size: var(--text-sm);
    line-height: 1.55;
  }

  .empty-path {
    margin-top: 6px;
    color: rgb(255 255 255 / 38%);
    font-family: var(--font-mono);
    font-size: 0.625rem;
  }
</style>
