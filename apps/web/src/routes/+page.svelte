<script lang="ts">
  /**
   * La página: hero, el demo interactivo, herramientas, videos, privacidad y
   * descarga. El demo va temprano a propósito — es la mejor explicación.
   */
  import Nav from "$lib/site/Nav.svelte";
  import Hero from "$lib/site/Hero.svelte";
  import DemoDesk from "$lib/demo/DemoDesk.svelte";
  import Features from "$lib/site/Features.svelte";
  import VideoSlot from "$lib/site/VideoSlot.svelte";
  import Privacy from "$lib/site/Privacy.svelte";
  import Download from "$lib/site/Download.svelte";
  import Footer from "$lib/site/Footer.svelte";
</script>

<svelte:head>
  <title>Atic — caja de herramientas de escritorio</title>
  <meta
    name="description"
    content="Atic vive en una pill flotante: agentes CLI, launcher, clipboard, capturas, pizarra y dictado, sin salir de lo que estabas haciendo. Local-first, open source."
  />
  <meta property="og:title" content="Atic — caja de herramientas de escritorio" />
  <meta
    property="og:description"
    content="La pill flotante con tus herramientas de escritorio. Demo interactivo en el navegador."
  />
  <meta property="og:type" content="website" />
</svelte:head>

<Nav />

<main>
  <Hero />

  <section class="demo" id="demo">
    <div class="wrap">
      <header class="head">
        <span class="eyebrow">Demo</span>
        <h2>El escritorio, acá mismo</h2>
        <p>
          Es la pill de Atic con la rueda y sus herramientas. Clic en la pill —o
          <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Espacio</kbd>— para abrir la rueda;
          <kbd>Ctrl</kbd>+<kbd>Espacio</kbd> trae el launcher. Los datos son de
          ejemplo; el portapapeles copia de verdad.
        </p>
      </header>
      <DemoDesk />
    </div>
  </section>

  <Features />
  <VideoSlot />
  <Privacy />
  <Download />
</main>

<Footer />

<style>
  .demo .wrap {
    max-width: 1080px;
    margin: 0 auto;
    padding: clamp(40px, 7vh, 72px) 20px 0;
  }

  .head {
    max-width: 620px;
    margin-bottom: 22px;
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

  kbd {
    border: 1px solid var(--line);
    border-radius: var(--radius-xs);
    padding: 1px 5px;
    background: var(--surface);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.65625rem;
  }
</style>
