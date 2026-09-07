<script lang="ts">
  /**
   * Tapa 3D sobre una ventana ajena: el frente es una captura, el reverso un bloc.
   *
   * El overlay es más grande que la ventana y transparente. La tarjeta ocupa
   * solo el marco visible; al girar se ve el escritorio a los lados.
   */
  import { onMount } from "svelte";
  import { t } from "$domain/i18n.svelte";
  import {
    closeWindowFlip,
    onWindowFlipOpen,
    onWindowFlipRequestClose,
    refreshWindowFlipPreview,
    saveWindowFlipNote,
    windowFlipPreviewSrc,
    windowFlipState,
    type WindowFlipView,
  } from "$ipc/windowFlip";

  const FLIP_MS = 720;
  const RESPALDO_MS = FLIP_MS + 200;
  const FOTO_ESPERA_MS = 280;

  let view = $state<WindowFlipView | null>(null);
  let note = $state("");
  let showBack = $state(false);
  let previewSrc = $state("");
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let closeTimer: ReturnType<typeof setTimeout> | undefined;
  let closing = false;

  function pct(value: number | undefined, fallback: number): string {
    return `${((value ?? fallback) * 100).toFixed(4)}%`;
  }

  function srcDe(path: string): string {
    return path ? `${windowFlipPreviewSrc(path)}?t=${Date.now()}` : "";
  }

  function esperarFoto(src: string): Promise<void> {
    if (!src) return Promise.resolve();
    return new Promise((resolve) => {
      const img = new Image();
      let done = false;
      const finish = () => {
        if (done) return;
        done = true;
        resolve();
      };
      img.onload = finish;
      img.onerror = finish;
      img.src = src;
      setTimeout(finish, FOTO_ESPERA_MS);
    });
  }

  function apply(next: WindowFlipView) {
    closing = false;
    if (closeTimer) clearTimeout(closeTimer);
    view = next;
    note = next.note;
    previewSrc = srcDe(next.previewPath);
    showBack = false;
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (!closing) showBack = true;
      });
    });
  }

  function persist(body: string) {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      void saveWindowFlipNote(body);
    }, 350);
  }

  function sinMovimiento(): boolean {
    return (
      typeof window !== "undefined" &&
      window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true
    );
  }

  async function beginClose() {
    if (closing) return;
    closing = true;
    if (saveTimer) clearTimeout(saveTimer);
    void saveWindowFlipNote(note);

    if (!sinMovimiento()) {
      try {
        const fresh = await refreshWindowFlipPreview();
        if (fresh.previewPath) {
          const src = srcDe(fresh.previewPath);
          await esperarFoto(src);
          if (closing) previewSrc = src;
        }
      } catch {
        // Sin foto nueva se gira con la de apertura.
      }
    }

    if (sinMovimiento()) {
      terminarCierre();
      return;
    }
    showBack = false;
    closeTimer = setTimeout(terminarCierre, RESPALDO_MS);
  }

  function terminarCierre() {
    if (!closing) return;
    if (closeTimer) clearTimeout(closeTimer);
    closeTimer = undefined;
    void closeWindowFlip();
  }

  function alTerminarGiro(event: TransitionEvent) {
    if (event.propertyName !== "transform") return;
    if (!(event.target as HTMLElement).classList.contains("card")) return;
    if (closing && !showBack) terminarCierre();
  }

  onMount(() => {
    void windowFlipState().then((current) => {
      if (current) apply(current);
    });
    const stopOpen = onWindowFlipOpen(apply);
    const stopClose = onWindowFlipRequestClose(() => {
      void beginClose();
    });
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        void beginClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      void stopOpen.then((unlisten) => unlisten());
      void stopClose.then((unlisten) => unlisten());
      window.removeEventListener("keydown", onKey);
      if (saveTimer) clearTimeout(saveTimer);
      if (closeTimer) clearTimeout(closeTimer);
    };
  });
</script>

<div class="root">
  <div class="stage">
    <div
      class="card"
      class:flipped={showBack}
      style:left={pct(view?.cardLeft, 0)}
      style:top={pct(view?.cardTop, 0)}
      style:width={pct(view?.cardWidth, 1)}
      style:height={pct(view?.cardHeight, 1)}
      ontransitionend={alTerminarGiro}
    >
      <div class="face front">
        {#if previewSrc}
          <img src={previewSrc} alt="" />
        {:else}
          <div class="blank"></div>
        {/if}
      </div>
      <div class="face back">
        <header>
          <p class="kicker">
            {t("overlay.windowFlip.kicker", { exe: view?.exe ?? "" })}
          </p>
          <h1>{view?.title || t("overlay.windowFlip.untitled")}</h1>
          <p class="hint">{t("overlay.windowFlip.hint")}</p>
        </header>
        <textarea
          value={note}
          placeholder={t("overlay.windowFlip.placeholder")}
          oninput={(event) => {
            note = event.currentTarget.value;
            persist(note);
          }}
        ></textarea>
        <button type="button" onclick={() => void beginClose()}>
          {t("overlay.windowFlip.close")}
        </button>
      </div>
    </div>
  </div>
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    height: 100%;
    overflow: hidden;
    background: transparent;
  }

  .root {
    width: 100%;
    height: 100%;
    background: transparent;
  }

  .stage {
    position: relative;
    width: 100%;
    height: 100%;
    perspective: 1600px;
    background: transparent;
  }

  .card {
    position: absolute;
    transform-style: preserve-3d;
    transform: translateZ(0);
    transition: transform 0.72s cubic-bezier(0.22, 1, 0.36, 1);
    transform-origin: center center;
    box-shadow: 0 18px 50px rgba(0, 0, 0, 0.35);
  }

  @media (prefers-reduced-motion: reduce) {
    .card {
      transition: none;
    }
  }

  .card.flipped {
    transform: rotateY(180deg) translateZ(0);
  }

  .face {
    position: absolute;
    inset: 0;
    -webkit-backface-visibility: hidden;
    backface-visibility: hidden;
    transform: translateZ(1px);
  }

  .front img,
  .blank {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: fill;
    background: #2a2623;
  }

  .back {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 20px 22px 16px;
    background: #1c1917;
    color: #f5f0eb;
    transform: rotateY(180deg) translateZ(1px);
    box-sizing: border-box;
  }

  header h1 {
    margin: 4px 0 0;
    font-size: 18px;
    font-weight: 600;
    line-height: 1.25;
  }

  .kicker,
  .hint {
    margin: 0;
    font-size: 12px;
    letter-spacing: 0.02em;
  }

  .kicker {
    color: #d6b48a;
    text-transform: lowercase;
  }

  .hint {
    color: #a89f96;
    margin-top: 6px;
  }

  textarea {
    flex: 1;
    min-height: 0;
    resize: none;
    border: 1px solid #3f3a36;
    border-radius: 10px;
    padding: 12px;
    background: #2a2623;
    color: inherit;
    font: inherit;
    font-size: 14px;
    line-height: 1.45;
  }

  textarea:focus {
    outline: 2px solid #d6b48a;
    outline-offset: 1px;
  }

  button {
    align-self: flex-start;
    border: 0;
    border-radius: 8px;
    padding: 8px 12px;
    background: #3f3a36;
    color: inherit;
    cursor: pointer;
    font: inherit;
    font-size: 13px;
  }

  button:hover {
    background: #524c46;
  }
</style>
