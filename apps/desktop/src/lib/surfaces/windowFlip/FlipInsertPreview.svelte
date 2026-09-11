<script lang="ts">
  /**
   * Antes de pegar: se ve lo que va al papel. El clic del cajón no inserta.
   */
  import type { CaptureItem, ClipboardItem, Recording, Snippet } from "$core/types";
  import { MOTION, ms, prefersReducedMotion } from "$lib/motion";
  import { t } from "$domain/i18n.svelte";
  import { captureSrc } from "$ipc/captures";
  import { windowFlipPreviewSrc } from "$ipc/windowFlip";
  import SummaryDocument from "$features/meetings/SummaryDocument.svelte";

  export type FlipPreview =
    | { tipo: "clip"; item: ClipboardItem }
    | { tipo: "snip"; item: Snippet }
    | { tipo: "cap"; item: CaptureItem }
    | { tipo: "meet"; item: Recording; etiqueta: string; body: string | null };

  let {
    preview,
    oncancel,
    onconfirm,
  }: {
    preview: FlipPreview;
    oncancel: () => void;
    onconfirm: () => void;
  } = $props();

  let cerrando = $state(false);
  let timerCierre: ReturnType<typeof setTimeout> | undefined;

  /**
   * Cierra con la salida ya andando.
   *
   * El padre desmonta la carta en cuanto recibe la acción, así que si se avisara
   * enseguida la animación de salida no llegaría a verse. Es el mismo trato que
   * hace `Modal.svelte`: se espera lo que dura el cierre y recién ahí se avisa.
   */
  function cerrar(accion: () => void) {
    if (cerrando) return;
    if (prefersReducedMotion()) {
      accion();
      return;
    }
    cerrando = true;
    timerCierre = setTimeout(accion, ms(MOTION.quick));
  }

  $effect(() => () => {
    if (timerCierre) clearTimeout(timerCierre);
  });
</script>

<div class="velo" class:cerrando role="presentation">
  <div class="carta" role="dialog" aria-modal="true" aria-labelledby="flip-prev-titulo">
    <p id="flip-prev-titulo" class="kicker">{t("overlay.windowFlip.previewKicker")}</p>
    <div class="cuerpo">
      {#if preview.tipo === "clip"}
        {#if preview.item.kind === "image" && preview.item.imagePath}
          <img src={windowFlipPreviewSrc(preview.item.imagePath)} alt="" />
        {:else}
          <pre>{preview.item.text || preview.item.preview}</pre>
        {/if}
      {:else if preview.tipo === "snip"}
        <p class="nombre">{preview.item.name}</p>
        <pre>{preview.item.body}</pre>
      {:else if preview.tipo === "cap"}
        <img src={captureSrc(preview.item.path)} alt="" />
      {:else if preview.body === null}
        <p class="hueco">{t("overlay.windowFlip.previewLoading")}</p>
      {:else if !preview.body.trim()}
        <p class="hueco">{t("overlay.windowFlip.previewMeetEmpty")}</p>
      {:else}
        <p class="nombre">{preview.etiqueta}</p>
        <p class="sub">{preview.item.title}</p>
        {#if preview.body.trim()}
          <SummaryDocument
            content={preview.body}
            defaultTitle={preview.etiqueta}
            compact
          />
        {:else}
          <pre>{preview.body}</pre>
        {/if}
      {/if}
    </div>
    <div class="actos">
      <button
        type="button"
        class="rb-btn rb-btn-ghost"
        onclick={() => cerrar(oncancel)}
      >
        {t("overlay.windowFlip.previewCancel")}
      </button>
      <button
        type="button"
        class="rb-btn rb-btn-soft"
        disabled={preview.tipo === "meet" &&
          (preview.body === null || !preview.body.trim())}
        onclick={() => cerrar(onconfirm)}
      >
        {t("overlay.windowFlip.previewAdd")}
      </button>
    </div>
  </div>
</div>

<style>
  /*
   * Abrir: `--duration-fast` + `--scale-large` (0.96) + blur chico, todo con
   * smooth-out. Cerrar: `--duration-quick`, misma escala y sin rebote —un cierre
   * no se celebra—. Es el par que ya usa Modal.svelte.
   */
  .velo {
    position: absolute;
    inset: 0;
    z-index: 20;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
    background: color-mix(in sRGB, var(--rb-bg1) 55%, transparent);
    animation: velo-entra var(--duration-fast) var(--ease-smooth-out) both;
  }

  .velo.cerrando {
    pointer-events: none;
    animation: velo-sale var(--duration-quick) var(--ease-smooth-out) both;
  }

  @keyframes velo-entra {
    from {
      opacity: 0;
    }
  }

  @keyframes velo-sale {
    to {
      opacity: 0;
    }
  }

  .carta {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: min(420px, 100%);
    max-height: min(520px, 100%);
    padding: 12px;
    border: 1px solid var(--rb-hairline);
    border-radius: var(--rb-radius-sm);
    background: var(--rb-surface);
    box-shadow: 0 12px 40px rgb(0 0 0 / 28%);
    transform-origin: 50% 50%;
    animation: carta-entra var(--duration-fast) var(--ease-smooth-out) both;
  }

  .velo.cerrando .carta {
    animation: carta-sale var(--duration-quick) var(--ease-smooth-out) both;
  }

  @keyframes carta-entra {
    from {
      opacity: 0;
      transform: scale(var(--scale-large, 0.96));
      filter: blur(var(--blur-small, 2px));
    }

    to {
      opacity: 1;
      transform: scale(1);
      filter: blur(0);
    }
  }

  @keyframes carta-sale {
    from {
      opacity: 1;
      transform: scale(1);
    }

    to {
      opacity: 0;
      transform: scale(var(--scale-large, 0.96));
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .velo,
    .velo.cerrando,
    .carta,
    .velo.cerrando .carta {
      animation: none;
    }
  }

  .kicker {
    margin: 0;
    color: var(--rb-muted);
    font-size: 11px;
    font-weight: 600;
  }

  .cuerpo {
    min-height: 0;
    overflow: auto;
    flex: 1;
  }

  .nombre {
    margin: 0 0 4px;
    font-size: 13px;
    font-weight: 600;
  }

  .sub {
    margin: 0 0 8px;
    color: var(--rb-muted);
    font-size: 12px;
  }

  .hueco {
    margin: 0;
    color: var(--rb-muted);
    font-size: 12.5px;
  }

  pre {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font: inherit;
    font-size: 12.5px;
    line-height: 1.45;
  }

  img {
    display: block;
    width: 100%;
    height: auto;
    max-height: 280px;
    object-fit: contain;
    border-radius: var(--rb-radius-xs);
  }

  .actos {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }
</style>
