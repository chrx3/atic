<script lang="ts">
  /**
   * Las herramientas del catálogo real: `TOOLS` se importa del core de la app,
   * así que si una herramienta cambia su bajada, el sitio la cuenta igual.
   */
  import { TOOLS } from "$atic/lib/core/tools";
  import ToolIcon from "$lib/atic/ToolIcon.svelte";

  const SHORTCUTS = [
    { keys: "Ctrl+Shift+R", label: "Iniciar / detener grabación" },
    { keys: "Ctrl+Shift+D", label: "Dictado" },
    { keys: "Ctrl+Shift+V", label: "Historial de portapapeles" },
    { keys: "Ctrl+Shift+S", label: "Textos" },
    { keys: "Ctrl+Shift+4", label: "Captura" },
    { keys: "Ctrl+Shift+X", label: "Pizarra" },
    { keys: "Ctrl+Shift+C", label: "Color" },
    { keys: "Ctrl+Shift+A", label: "Agentes" },
    { keys: "Ctrl+Space", label: "Launcher de apps" },
    { keys: "Ctrl+Shift+Space", label: "Rueda de herramientas" },
  ];
</script>

<section class="features" id="herramientas">
  <div class="wrap">
    <header class="head">
      <span class="eyebrow">Qué hace</span>
      <h2>Nueve herramientas, una sola presencia</h2>
      <p>
        No es otra ventana de chat ni un índice de todo el disco. Son las herramientas
        del SO y de tus CLIs, sin interrumpir lo que estabas haciendo.
      </p>
    </header>

    <ul class="grid">
      {#each TOOLS as tool (tool.id)}
        <li class="card">
          <span class="card-icon"><ToolIcon id={tool.id} size={18} /></span>
          <h3>{tool.label}</h3>
          <p>{tool.blurb}</p>
        </li>
      {/each}
    </ul>

    <div class="shortcuts">
      <div class="shortcuts-head">
        <h3>Atajos globales</h3>
        <p>En Mac, Ctrl es Cmd. Todos se pueden cambiar en Ajustes.</p>
      </div>
      <ul class="keys">
        {#each SHORTCUTS as item (item.keys)}
          <li>
            <kbd>{item.keys}</kbd>
            <span>{item.label}</span>
          </li>
        {/each}
      </ul>
    </div>
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
    margin-bottom: 28px;
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

  .grid {
    display: grid;
    margin: 0;
    padding: 0;
    gap: 10px;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
    list-style: none;
  }

  .card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    padding: 16px;
    background: color-mix(in srgb, var(--surface) 55%, transparent);
    transition:
      border-color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out);
  }

  .card:hover {
    border-color: var(--line-strong);
    background: var(--surface);
  }

  .card-icon {
    display: grid;
    width: 34px;
    height: 34px;
    place-items: center;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background: var(--skin);
    color: var(--text);
  }

  .card h3 {
    margin: 2px 0 0;
    font-size: var(--text-md);
    font-weight: var(--font-weight-semibold);
  }

  .card p {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.55;
  }

  .shortcuts {
    display: grid;
    margin-top: 34px;
    gap: 22px;
    border-top: 1px solid var(--line);
    padding-top: 26px;
    grid-template-columns: minmax(220px, 300px) 1fr;
  }

  .shortcuts-head h3 {
    margin: 0 0 6px;
    font-size: var(--text-md);
    font-weight: var(--font-weight-semibold);
  }

  .shortcuts-head p {
    margin: 0;
    color: var(--faint);
    font-size: var(--text-sm);
    line-height: 1.5;
  }

  .keys {
    display: grid;
    margin: 0;
    padding: 0;
    gap: 2px 22px;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    list-style: none;
  }

  .keys li {
    display: flex;
    align-items: center;
    gap: 10px;
    border-bottom: 1px solid var(--line);
    padding: 8px 0;
    color: var(--muted);
    font-size: var(--text-sm);
  }

  kbd {
    min-width: 8.5rem;
    border: 1px solid var(--line);
    border-radius: var(--radius-xs);
    padding: 3px 7px;
    background: var(--surface);
    color: var(--text);
    font-family: var(--font-mono);
    font-size: 0.65625rem;
    text-align: center;
  }

  @media (max-width: 720px) {
    .shortcuts {
      grid-template-columns: 1fr;
    }
  }
</style>
