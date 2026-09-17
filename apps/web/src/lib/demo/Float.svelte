<script lang="ts">
  /**
   * El marco de una ventana flotante del demo.
   *
   * Mismo comportamiento que los floats de la app: nacen de la pill con
   * `float-emerge` — escala 0.55, 18 px de viaje, 150 ms y curva calmada — y
   * al cerrar hacen el camino inverso. El CSS es el de `app.css` de la app,
   * ya que ese archivo no se importa entero en el sitio.
   */
  import type { Snippet } from "svelte";
  import type { AppIconId } from "$lib/atic/icons";
  import ToolIcon from "$lib/atic/ToolIcon.svelte";
  import { X } from "$lib/atic/icons";
  import Icon from "$lib/atic/Icon.svelte";

  let {
    title,
    icon,
    shown = true,
    wide = false,
    tall = false,
    onClose,
    children,
    footer,
  }: {
    title: string;
    icon: AppIconId;
    shown?: boolean;
    wide?: boolean;
    /** Ocupa todo el alto disponible (consola de agentes). */
    tall?: boolean;
    onClose?: () => void;
    children: Snippet;
    footer?: Snippet;
  } = $props();

  /**
   * La entrada se dispara recién en el segundo cuadro: el float se monta ya
   * en su estado final y una transición necesita un estado previo del que
   * partir. Sin esto, el panel aparece de golpe.
   */
  let entered = $state(false);
  $effect(() => {
    const raf = requestAnimationFrame(() => (entered = true));
    return () => cancelAnimationFrame(raf);
  });
</script>

<section
  class="float"
  class:is-shown={shown && entered}
  class:is-wide={wide}
  class:is-tall={tall}
  data-side="bottom"
  aria-label={title}
>
  <header class="head">
    <span class="head-icon"><ToolIcon id={icon} size={15} /></span>
    <h3 class="head-title">{title}</h3>
    <button type="button" class="head-x" aria-label="Cerrar" onclick={onClose}>
      <Icon icon={X} size={15} />
    </button>
  </header>
  <div class="body">
    {@render children()}
  </div>
  {#if footer}
    <footer class="foot">{@render footer()}</footer>
  {/if}
</section>

<style>
  .float {
    display: flex;
    /* El panel real mide 312 (`PILL.panelW`); el ancho lo pone el slot. */
    width: min(312px, calc(100vw - 24px));
    max-height: 100%;
    flex-direction: column;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: var(--surface);
    box-shadow: var(--shadow-float);
    color: var(--text);

    /* `float-emerge` de app.css, en versión scoped. */
    opacity: 0;
    transform: translateY(var(--float-travel, 18px)) scale(var(--float-scale, 0.55));
    transform-origin: 50% 100%;
    pointer-events: none;
    transition:
      opacity var(--float-close-dur, 100ms) var(--float-ease, cubic-bezier(0.22, 1, 0.36, 1)),
      transform var(--float-close-dur, 100ms) var(--float-ease, cubic-bezier(0.22, 1, 0.36, 1));
  }

  .float.is-shown {
    opacity: 1;
    transform: none;
    pointer-events: auto;
    transition:
      opacity var(--float-open-dur, 150ms) var(--float-ease, cubic-bezier(0.22, 1, 0.36, 1)),
      transform var(--float-open-dur, 150ms) var(--float-ease, cubic-bezier(0.22, 1, 0.36, 1));
  }

  .float.is-wide {
    width: min(560px, calc(100vw - 24px));
  }

  /* El alto lo pone el slot; los paneles que quieren scrollear adentro (la
     consola) estiran el float a todo el espacio. */
  .float.is-tall {
    height: 100%;
  }

  .head {
    display: flex;
    height: 44px;
    flex-shrink: 0;
    align-items: center;
    gap: 8px;
    padding: 0 8px 0 12px;
    border-bottom: 1px solid var(--line);
  }

  .head-icon {
    display: grid;
    place-items: center;
    color: var(--muted);
  }

  .head-title {
    flex: 1;
    margin: 0;
    font-size: var(--text-micro);
    font-weight: var(--font-weight-semibold);
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
    color: var(--muted);
  }

  .head-x {
    display: grid;
    width: 28px;
    height: 28px;
    place-items: center;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--faint);
    cursor: pointer;
  }

  .head-x:hover {
    background: color-mix(in srgb, var(--text) 8%, transparent);
    color: var(--text);
  }

  .body {
    min-height: 0;
    flex: 1;
    overflow: auto;
  }

  .foot {
    flex-shrink: 0;
    border-top: 1px solid var(--line);
    padding: 8px 12px;
    color: var(--faint);
    font-size: var(--text-micro);
  }

  @media (prefers-reduced-motion: reduce) {
    .float,
    .float.is-shown {
      transition: none;
      transform: none;
    }
  }
</style>
