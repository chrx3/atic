<script lang="ts">
  import AticMark from "$lib/atic/AticMark.svelte";

  const YEAR = new Date().getFullYear();
</script>

<footer class="foot">
  <div class="wrap">
    <div class="brand">
      <span class="mark"><AticMark size={22} alive={true} /></span>
      <div class="who">
        <span class="name">Atic</span>
        <span class="meta">MIT · © {YEAR} chrx3</span>
      </div>
    </div>

    <nav class="links" aria-label="Enlaces">
      <a href="https://github.com/chrx3/atic" target="_blank" rel="noreferrer">GitHub</a>
      <a href="https://github.com/chrx3/atic/releases/latest" target="_blank" rel="noreferrer">Releases</a>
      <a href="https://github.com/chrx3/atic/issues" target="_blank" rel="noreferrer">Issues</a>
      <a href="https://github.com/chrx3/atic/blob/main/LICENSE" target="_blank" rel="noreferrer">Licencia</a>
    </nav>

    <p class="craft">
      Sitio y demo con SvelteKit. Los tokens del diseño se importan de la app:
      si la app cambia, esta página cambia con ella.
    </p>
  </div>
</footer>

<style>
  .foot {
    margin-top: clamp(56px, 9vh, 96px);
    border-top: 1px solid var(--line);
  }

  .wrap {
    display: flex;
    max-width: 1080px;
    flex-wrap: wrap;
    gap: 18px 30px;
    align-items: center;
    margin: 0 auto;
    padding: 26px 20px 40px;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .mark {
    display: grid;
    width: 36px;
    height: 36px;
    place-items: center;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background: var(--skin);
  }

  .who {
    display: flex;
    flex-direction: column;
  }

  .name {
    font-size: var(--text-base);
    font-weight: var(--font-weight-semibold);
  }

  .meta {
    color: var(--faint);
    font-size: var(--text-micro);
  }

  .links {
    display: flex;
    flex: 1;
    flex-wrap: wrap;
    gap: 4px;
    justify-content: center;
  }

  .links a {
    border-radius: var(--radius-sm);
    padding: 6px 10px;
    color: var(--muted);
    font-size: var(--text-xs);
    text-decoration: none;
  }

  .links a:hover {
    background: color-mix(in srgb, var(--text) 7%, transparent);
    color: var(--text);
  }

  .craft {
    flex-basis: 100%;
    margin: 0;
    border-top: 1px solid var(--line);
    padding-top: 16px;
    color: var(--faint);
    font-size: var(--text-micro);
    line-height: 1.6;
    text-align: center;
  }

  @media (max-width: 720px) {
    .links {
      justify-content: flex-start;
    }
  }
</style>
