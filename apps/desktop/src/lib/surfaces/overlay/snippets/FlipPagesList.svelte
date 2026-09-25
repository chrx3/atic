<script lang="ts">
  /**
   * Las páginas del tablero (window flip), listadas dentro de Textos.
   *
   * Clic en una página = voltear la ventana del frente y abrir el tablero justo
   * en esa página. El tablero es uno solo para todas las ventanas, así que la
   * lista no depende de qué ventana esté debajo.
   *
   * Las miniaturas salen de la misma cuenta que la tira del tablero
   * (`miniaturasTablero`): lo que se ve acá es lo que se ve allá.
   */
  import { onMount } from "svelte";
  import { t } from "$domain/i18n.svelte";
  import {
    openWindowFlipPage,
    windowFlipAssetSrc,
    windowFlipBoard,
  } from "$ipc/windowFlip";
  import type { NoteBlock } from "$core/types";
  import {
    anchoParaBloques,
    bloqueEnPagina,
    cantidadPaginas,
    miniaturasTablero,
    PAGINA_H,
    PAGINA_W,
  } from "$surfaces/windowFlip/flipLayout";

  let { onOpened }: { onOpened?: () => void } = $props();

  let bloques = $state.raw<NoteBlock[]>([]);
  let assetsDir = $state("");
  let cargado = $state(false);
  let error = $state("");

  const paginas = $derived(cantidadPaginas(anchoParaBloques(bloques)));
  const miniaturas = $derived(miniaturasTablero(bloques, paginas));

  /** Primera línea de texto de la página: lo que la hace reconocible en la lista. */
  function resumen(indice: number): string {
    for (const b of bloques) {
      if (!bloqueEnPagina(b, indice)) continue;
      const texto =
        b.kind === "text"
          ? b.body
          : b.kind === "check"
            ? b.items.map((item) => item.text).join(" · ")
            : "";
      const linea = texto
        .split("\n")
        .map((l) => l.trim())
        .find(Boolean);
      if (linea) return linea;
    }
    return "";
  }

  async function abrir(indice: number) {
    error = "";
    try {
      await openWindowFlipPage(indice);
      onOpened?.();
    } catch (err) {
      error = String(err);
    }
  }

  onMount(() => {
    void windowFlipBoard()
      .then((board) => {
        bloques = board.blocks;
        assetsDir = board.assetsDir;
      })
      .catch((err) => (error = String(err)))
      .finally(() => (cargado = true));
  });
</script>

<div class="fp">
  {#if cargado && bloques.length === 0}
    <p class="fp-vacio">{t("overlay.boardEmpty")}</p>
  {/if}
  {#if bloques.length > 0}
    <ul class="fp-lista">
      {#each miniaturas as pagina (pagina.indice)}
        {@const texto = resumen(pagina.indice)}
        <li>
          <button
            type="button"
            class="fp-item"
            title={t("overlay.boardOpenPage", { n: String(pagina.indice + 1) })}
            onclick={() => void abrir(pagina.indice)}
          >
            <span class="mini" aria-hidden="true">
              {#each pagina.piezas as pieza (pieza.id)}
                {#if pieza.tipo === "image"}
                  <img
                    class="mini-foto"
                    src={windowFlipAssetSrc(assetsDir, pieza.asset)}
                    alt=""
                    draggable="false"
                    style:left={`${pieza.x}%`}
                    style:top={`${pieza.y}%`}
                    style:width={`${pieza.w}%`}
                    style:height={`${pieza.h}%`}
                  />
                {:else}
                  <span
                    class="mini-texto"
                    style:left={`${pieza.x}%`}
                    style:top={`${pieza.y}%`}
                    style:width={`${pieza.w}%`}
                    style:height={`${pieza.h}%`}
                  >
                    {#each Array.from({ length: Math.max(pieza.renglones, pieza.filas) }, (_, i) => i) as renglon (renglon)}
                      <span class="mini-renglon"></span>
                    {/each}
                  </span>
                {/if}
              {/each}
              {#if pagina.trazos.length > 0}
                <svg
                  class="mini-tinta"
                  viewBox={`${pagina.indice * PAGINA_W} 0 ${PAGINA_W} ${PAGINA_H}`}
                  preserveAspectRatio="none"
                >
                  {#each pagina.trazos as trazo, k (k)}
                    <polyline
                      points={trazo.puntos}
                      fill="none"
                      stroke={trazo.color}
                      stroke-width={trazo.ancho}
                      stroke-linecap="round"
                      stroke-linejoin="round"
                    />
                  {/each}
                </svg>
              {/if}
            </span>
            <span class="fp-textos">
              <span class="fp-titulo">
                {t("overlay.windowFlip.pageN", { n: String(pagina.indice + 1) })}
              </span>
              <span class="fp-resumen">{texto || t("overlay.boardPageEmpty")}</span>
            </span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
  {#if error}
    <p class="fp-error" role="alert">{error}</p>
  {/if}
</div>

<style>
  .fp {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    overflow-y: auto;
    padding: 6px;
  }

  .fp-lista {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .fp-item {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 10px;
    padding: 6px;
    border: 0;
    border-radius: var(--rb-radius-sm);
    background: transparent;
    color: inherit;
    cursor: pointer;
    text-align: left;
    transition: background var(--duration-fast) var(--ease-smooth-out);
  }

  .fp-item:hover {
    background: var(--rb-bg0);
  }

  .fp-item:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .fp-textos {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    gap: 2px;
  }

  .fp-titulo {
    font-size: 12px;
    font-weight: 600;
  }

  .fp-resumen {
    overflow: hidden;
    color: var(--rb-muted);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .fp-vacio,
  .fp-error {
    margin: 0;
    padding: 12px 8px;
    color: var(--rb-muted);
    font-size: 12px;
    text-align: center;
  }

  .fp-error {
    color: var(--rb-record);
  }

  /* Miniatura: mismas proporciones y trazos que la tira del tablero. */
  .mini {
    position: relative;
    display: block;
    width: 64px;
    flex: none;
    aspect-ratio: 4 / 3;
    border: 1px solid var(--rb-hairline);
    border-radius: var(--rb-radius-xs);
    background: var(--rb-surface-elevated);
    overflow: hidden;
  }

  .mini-foto,
  .mini-texto {
    position: absolute;
  }

  .mini-foto {
    border-radius: 1px;
    object-fit: fill;
  }

  .mini-texto {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 2px;
    overflow: hidden;
  }

  .mini-renglon {
    display: block;
    height: 2px;
    flex: none;
    border-radius: 1px;
    background: color-mix(in sRGB, var(--rb-text) 38%, transparent);
  }

  .mini-renglon:last-child {
    width: 55%;
  }

  .mini-tinta {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
</style>
