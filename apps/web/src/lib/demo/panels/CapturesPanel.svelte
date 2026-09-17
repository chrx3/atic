<script lang="ts">
  /**
   * Capturas y su shelf.
   *
   * Como en la app, el shelf es efímero: muestra la última captura y se
   * descarta solo a los 20 segundos. Desde acá se copia, se guarda o se abre
   * en la pizarra para dibujar encima.
   *
   * Las imágenes se dibujan (`captureArt.ts`): en el navegador no hay pantalla
   * que recortar, pero el flujo —tomar, copiar, dibujar, guardar— es el de la
   * app. Lo que no existe acá (extraer texto, arrastrar a otra app) se dice en
   * el pie en vez de simularse.
   */
  import { onDestroy } from "svelte";
  import Float from "../Float.svelte";
  import Icon from "$lib/atic/Icon.svelte";
  import { Crop, Download, Image, Pencil, Trash2 } from "$lib/atic/icons";
  import { CAPTURE_VARIANTS, makeCapture, type CaptureVariant } from "../captureArt";
  import { copyText, demo } from "../state.svelte";

  let { onClose }: { onClose?: () => void } = $props();

  type Capture = {
    id: number;
    title: string;
    size: string;
    url: string;
  };

  /** El shelf real se descarta solo a los 20 s. */
  const SHELF_TTL_MS = 20_000;

  const LABEL: Record<CaptureVariant, string> = {
    code: "editor",
    chart: "gráfico Q3",
    article: "notas",
  };

  let capture = $state<Capture | null>(null);
  let variantIndex = 0;
  let seq = 0;
  let ttlTimer = 0;
  let ttlKey = $state(0);

  function clearTtl() {
    window.clearTimeout(ttlTimer);
  }

  function armTtl() {
    clearTtl();
    ttlKey += 1;
    ttlTimer = window.setTimeout(() => {
      capture = null;
      demo.toast("Captura descartada", "info");
    }, SHELF_TTL_MS);
  }

  function take() {
    const variant = CAPTURE_VARIANTS[variantIndex++ % CAPTURE_VARIANTS.length];
    const art = makeCapture(variant, 960, 600, Date.now() % 99991);
    capture = {
      id: ++seq,
      title: `Captura — ${LABEL[variant]}`,
      size: "1440 × 900",
      url: art.url,
    };
    armTtl();
    demo.toast("Captura al shelf", "ok");
  }

  async function copyImage() {
    if (!capture) return;
    try {
      const blob = await (await fetch(capture.url)).blob();
      await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
      demo.toast("Imagen copiada al portapapeles", "ok");
    } catch {
      demo.toast("El navegador no dejó copiar la imagen; usa Guardar", "info");
    }
  }

  function save() {
    if (!capture) return;
    const link = document.createElement("a");
    link.href = capture.url;
    link.download = "atic-captura.png";
    link.click();
    demo.toast("Captura guardada");
  }

  function draw() {
    if (!capture) return;
    demo.openBoard(capture.url);
  }

  function discard() {
    clearTtl();
    capture = null;
  }

  onDestroy(clearTtl);
</script>

<Float title="Capturas" icon="captures" onClose={onClose}>
  <div class="captures">
    {#if capture}
      {#key capture.id}
        <figure class="card">
          <img src={capture.url} alt={capture.title} />
          <span class="ttl" aria-hidden="true"><i></i></span>
          <figcaption>
            <span class="meta">{capture.title} · {capture.size}</span>
            <span class="actions">
              <button type="button" class="btn" onclick={copyImage}>Copiar</button>
              <button type="button" class="btn" onclick={draw}>
                <Icon icon={Pencil} size={12} /> Dibujar
              </button>
              <button type="button" class="btn" onclick={save}>
                <Icon icon={Download} size={12} /> Guardar
              </button>
              <button
                type="button"
                class="btn is-danger"
                aria-label="Descartar"
                onclick={discard}
              >
                <Icon icon={Trash2} size={12} />
              </button>
            </span>
          </figcaption>
        </figure>
      {/key}
    {:else}
      <div class="empty">
        <span class="empty-icon"><Icon icon={Image} size={20} /></span>
        <p class="empty-title">Todavía no hay capturas</p>
        <p class="empty-sub">Ventana, región o monitor — al shelf flotante.</p>
        <button type="button" class="btn is-primary" onclick={take}>
          <Icon icon={Crop} size={12} /> Capturar ahora
        </button>
      </div>
    {/if}
  </div>

  {#snippet footer()}
    <div class="foot">
      {#if capture}
        <span>El shelf es efímero: se descarta solo</span>
        <button type="button" class="btn" onclick={take}>
          <Icon icon={Crop} size={12} /> Otra
        </button>
      {:else}
        <span>En la app también extraes texto o la arrastras a otra app</span>
      {/if}
    </div>
  {/snippet}
</Float>

<style>
  .captures {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
  }

  .card {
    margin: 0;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background: var(--bg);
  }

  .card img {
    display: block;
    width: 100%;
    height: auto;
  }

  /* La barra del TTL: el shelf se va solo, como en la app. */
  .ttl {
    display: block;
    height: 2px;
    background: color-mix(in srgb, var(--text) 10%, transparent);
  }

  .ttl i {
    display: block;
    height: 100%;
    background: var(--faint);
    transform-origin: 0 50%;
    animation: ttl linear forwards;
    animation-duration: 20s;
  }

  @keyframes ttl {
    from {
      transform: scaleX(1);
    }
    to {
      transform: scaleX(0);
    }
  }

  .card figcaption {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    justify-content: space-between;
    border-top: 1px solid var(--line);
    padding: 7px 9px;
  }

  .meta {
    color: var(--faint);
    font-size: var(--text-micro);
    font-variant-numeric: tabular-nums;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
  }

  .empty {
    display: flex;
    flex-direction: column;
    gap: 6px;
    align-items: center;
    padding: 26px 18px;
    text-align: center;
  }

  .empty-icon {
    display: grid;
    width: 42px;
    height: 42px;
    place-items: center;
    border-radius: 50%;
    background: color-mix(in srgb, var(--text) 7%, transparent);
    color: var(--muted);
  }

  .empty-title {
    margin: 4px 0 0;
    color: var(--text);
    font-size: var(--text-md);
  }

  .empty-sub {
    margin: 0 0 6px;
    color: var(--faint);
    font-size: var(--text-xs);
  }

  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    padding: 4px 9px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--text-micro);
    cursor: pointer;
  }

  .btn:hover {
    border-color: var(--line-strong);
    color: var(--text);
  }

  .btn.is-danger:hover {
    border-color: color-mix(in srgb, var(--danger) 50%, transparent);
    color: var(--danger);
  }

  .btn.is-primary {
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    background: var(--accent);
    color: var(--on-accent);
  }

  @media (prefers-reduced-motion: reduce) {
    .ttl i {
      animation: none;
    }
  }
</style>
