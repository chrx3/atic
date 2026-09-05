<script lang="ts">
  /**
   * El preview visible del portapapeles. Montado UNA vez por ventana.
   *
   * El porqué está en `clipPreview.svelte.ts`. Acá solo queda la ubicación: al
   * COSTADO de la fila, no arriba ni abajo.
   *
   * El lado importa. La lista es una columna angosta de filas apiladas: un
   * panel encima taparía las filas vecinas, que es justo lo que el usuario está
   * recorriendo. Al costado, la columna queda entera a la vista y el preview
   * acompaña. Se prefiere la derecha y se cae a la izquierda si no entra.
   */
  import { clipPreviewState } from "./clipPreview.svelte";

  /** Aire entre la fila y el panel. */
  const GAP = 10;
  /** Margen mínimo contra el borde de la ventana. */
  const EDGE = 8;

  let el = $state<HTMLElement | null>(null);
  let x = $state(0);
  let y = $state(0);
  /**
   * Hasta no estar medido se pinta transparente: al primer cuadro el panel
   * está en 0,0 y sin esto se ve saltar desde la esquina.
   */
  let placed = $state(false);

  $effect(() => {
    const anchor = clipPreviewState.anchor;
    // Dependencias explícitas: el contenido cambia el tamaño medido.
    void clipPreviewState.text;
    void clipPreviewState.src;
    void clipPreviewState.color;
    void clipPreviewState.kind;
    if (!clipPreviewState.open || !anchor || !el) {
      placed = false;
      return;
    }
    const box = el.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    const right = anchor.x + anchor.w + GAP;
    const left = anchor.x - box.width - GAP;
    const fitsRight = right + box.width + EDGE <= vw;
    const fitsLeft = left >= EDGE;
    x = fitsRight ? right : fitsLeft ? left : Math.max(EDGE, vw - box.width - EDGE);

    // Centrado con la fila, y adentro de la ventana.
    const top = anchor.y + anchor.h / 2 - box.height / 2;
    y = Math.min(Math.max(top, EDGE), Math.max(EDGE, vh - box.height - EDGE));
    placed = true;
  });
</script>

{#if clipPreviewState.open}
  <!-- `aria-hidden`: es un apoyo visual del hover. El contenido ya lo anuncia
       la fila, y un lector de pantalla no llega acá con el mouse. -->
  <div
    class="cp"
    class:is-placed={placed}
    bind:this={el}
    style:left="{x}px"
    style:top="{y}px"
    aria-hidden="true"
  >
    {#if clipPreviewState.kind === "image"}
      <img class="cp-img" src={clipPreviewState.src} alt="" draggable="false" />
      {#if clipPreviewState.label}
        <p class="cp-label">{clipPreviewState.label}</p>
      {/if}
    {:else if clipPreviewState.kind === "color"}
      <span class="cp-color" style:background={clipPreviewState.color}></span>
      {#if clipPreviewState.label}
        <p class="cp-value" data-numeric>{clipPreviewState.label}</p>
      {/if}
    {:else}
      <p class="cp-text">{clipPreviewState.text}</p>
    {/if}
    {#if clipPreviewState.hint}
      <p class="cp-hint">{clipPreviewState.hint}</p>
    {/if}
  </div>
{/if}

<style>
  .cp {
    position: fixed;
    z-index: var(--z-toast, 100);
    display: flex;
    max-width: 26rem;
    flex-direction: column;
    gap: 0.4rem;
    border: 1px solid color-mix(in sRGB, var(--line) 80%, transparent);
    border-radius: 0.6rem;
    padding: 0.5rem 0.6rem;

    /* Traslúcido con desenfoque detrás: es un apoyo momentáneo, no una
       ventana. Deja ver que abajo sigue estando la lista. */
    background: color-mix(in sRGB, var(--surface) 88%, transparent);
    backdrop-filter: blur(10px);
    box-shadow: 0 12px 32px color-mix(in sRGB, rgb(0 0 0) 38%, transparent);
    color: var(--text);
    opacity: 0;

    /* Duro: el overlay es click-through salvo en sus hit-rects, y un panel que
       reciba el mouse taparía el escritorio de abajo. Tampoco se publica como
       zona viva, por lo mismo. */
    pointer-events: none;
    transition: opacity var(--duration-fast, 125ms) var(--ease-smooth-out, ease-out);
  }

  .cp.is-placed {
    opacity: 1;
  }

  /*
   * El texto completo, con sus saltos de línea.
   *
   * `pre-wrap` porque lo que se guarda suele ser código, rutas o logs, donde la
   * sangría es información. `-webkit-line-clamp` corta lo muy largo con puntos
   * suspensivos en vez de dar scroll: el panel no recibe el mouse, así que un
   * scroll ahí adentro sería inalcanzable.
   */
  .cp-text {
    display: -webkit-box;
    overflow: hidden;
    margin: 0;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 18;
    line-clamp: 18;
    font-size: 0.72rem;
    font-weight: 450;
    line-height: 1.45;
    white-space: pre-wrap;

    /* Una URL o un base64 no tienen dónde cortar: sin esto estiran el panel. */
    overflow-wrap: anywhere;
  }

  .cp-img {
    display: block;
    max-width: 100%;
    max-height: 17rem;
    border-radius: 0.35rem;

    /* El damero se ve por debajo de un PNG con transparencia; sin esto una
       captura con fondo alfa se lee como un recorte roto. */
    background: repeating-conic-gradient(
        color-mix(in sRGB, var(--text) 8%, transparent) 0% 25%,
        transparent 0% 50%
      )
      50% / 12px 12px;
    object-fit: contain;
  }

  /*
   * El color, en grande.
   *
   * Es el equivalente del preview de imagen: la fila ya dice el valor, y lo
   * que no cabe en 28 px es el color mismo. El aro va en dos tonos porque un
   * blanco y un negro puros son de lo más copiado y cada uno desaparece contra
   * uno de los dos temas.
   */
  .cp-color {
    display: block;
    width: 13rem;
    height: 5rem;
    border-radius: 0.35rem;
    box-shadow:
      inset 0 0 0 1px rgb(255 255 255 / 22%),
      inset 0 0 0 1px rgb(0 0 0 / 18%);
  }

  .cp-value {
    margin: 0;
    font-family: var(--rb-mono, monospace);
    font-size: 0.72rem;
    font-weight: 600;
    line-height: 1.2;
  }

  .cp-label,
  .cp-hint {
    margin: 0;
    color: var(--faint);
    font-size: 0.625rem;
    font-weight: 500;
    line-height: 1.2;
  }

  .cp-hint {
    border-top: 1px solid color-mix(in sRGB, var(--line) 55%, transparent);
    padding-top: 0.36rem;
  }

  @media (prefers-reduced-motion: reduce) {
    .cp {
      transition: none;
    }
  }
</style>
