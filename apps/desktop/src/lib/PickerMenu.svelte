<script lang="ts">
  /**
   * Pastilla con menú, al estilo de los compositores de agentes.
   *
   * Existe en vez de un `<select>` por dos motivos: el nativo pinta con el
   * tema del sistema y rompe el tono de la consola, y sobre todo abre hacia
   * abajo — dentro de una burbuja anclada al borde de la pantalla eso queda
   * fuera de la ventana. Este abre hacia ARRIBA, que es donde hay sitio cuando
   * el control vive en el compositor.
   */
  interface Option {
    id: string;
    label: string;
    /** Qué cambia si eliges esto. Sin ella, elegir es adivinar. */
    note?: string;
    disabled?: boolean;
  }

  let {
    label,
    options,
    value,
    open,
    onToggle,
    onPick,
  }: {
    label: string;
    options: Option[];
    value: string;
    open: boolean;
    onToggle: () => void;
    onPick: (id: string) => void;
  } = $props();
</script>

<div class="pm">
  {#if open}
    <ul class="pm-list" role="listbox">
      {#each options as o (o.id)}
        <li>
          <button
            type="button"
            role="option"
            aria-selected={o.id === value}
            class="pm-opt"
            class:active={o.id === value}
            disabled={o.disabled}
            onclick={() => onPick(o.id)}
          >
            <span class="pm-opt-l">{o.label}</span>
            {#if o.note}
              <span class="pm-opt-n">{o.note}</span>
            {/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <button
    type="button"
    class="pm-chip"
    class:is-open={open}
    aria-haspopup="listbox"
    aria-expanded={open}
    onclick={onToggle}
  >
    <span class="pm-label">{label}</span>
    <span class="pm-caret" aria-hidden="true">⌄</span>
  </button>
</div>

<style>
  .pm {
    position: relative;
  }

  .pm-chip {
    display: inline-flex;
    max-width: 10rem;
    align-items: center;
    gap: 0.25rem;
    border: 1px solid var(--line, #332e2b);
    border-radius: 999px;
    padding: 0.2rem 0.55rem;
    background: transparent;
    color: var(--dim, #8d827a);
    font-family: inherit;
    font-size: 0.6875rem;
    cursor: pointer;
  }
  .pm-chip:hover,
  .pm-chip.is-open {
    color: var(--text, #e7e2dd);
  }

  .pm-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pm-caret {
    flex-shrink: 0;
    font-size: 0.625rem;
    line-height: 1;
  }

  .pm-list {
    position: absolute;
    bottom: calc(100% + 0.35rem);
    left: 0;
    z-index: 20;
    min-width: 100%;
    margin: 0;
    border: 1px solid var(--line, #332e2b);
    border-radius: 0.6rem;
    padding: 0.2rem;
    background: #262120;
    box-shadow: 0 10px 26px rgb(0 0 0 / 45%);
    list-style: none;
  }

  .pm-opt {
    display: block;
    width: 100%;
    border: 0;
    border-radius: 0.4rem;
    padding: 0.3rem 0.55rem;
    background: transparent;
    color: var(--dim, #8d827a);
    font-family: inherit;
    font-size: 0.6875rem;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .pm-opt:hover:not(:disabled) {
    background: #332e2b;
    color: var(--text, #e7e2dd);
  }
  .pm-opt.active .pm-opt-l {
    color: var(--coral, #d97757);
  }

  .pm-opt-l {
    display: block;
    color: var(--text, #e7e2dd);
  }

  /* El rasgo, más pequeño y apagado: se lee al dudar, no compite con el
     nombre cuando ya sabes cuál quieres. */
  .pm-opt-n {
    display: block;
    margin-top: 0.05rem;
    color: var(--faint, #6b615a);
    font-size: 0.625rem;
  }
  .pm-opt:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
