<script lang="ts">
  /**
   * Un archivo abierto en la pizarra: lo que suelte el usuario o lo que generó
   * una sesión. Imágenes, video, audio y PDF los pinta el webview; el texto se
   * lee y se muestra tal cual. Lo que no sabe mostrar, lo ofrece afuera.
   *
   * Cada vez que monta pide permiso para leerlo: el scope de assets vive en
   * memoria de Rust y no sobrevive a reiniciar la app.
   */
  import { onMount } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import Icon from "$ui/Icon.svelte";
  import AgentMessage from "$lib/AgentMessage.svelte";
  import { ExternalLink, Folder, RotateCw } from "$lib/icons";
  import { t } from "$domain/i18n.svelte";
  import {
    boardAllowFile,
    boardOpenExternal,
    boardReveal,
    type BoardFile,
  } from "$ipc/agents";

  let { path }: { path: string } = $props();

  /** Tope de lo que se lee de un archivo de texto para mostrar. */
  const TEXT_MAX = 400 * 1024;

  let file = $state<BoardFile | null>(null);
  let text = $state<string | null>(null);
  let error = $state<string | null>(null);
  const src = $derived(file ? convertFileSrc(file.path) : "");
  /** Markdown se lee renderizado, como en el chat; el resto, tal cual. */
  const isMarkdown = $derived(/\.(md|markdown)$/i.test(path));
  /** Imagen: ajustada a la tarjeta o a tamaño real (con scroll). */
  let actualSize = $state(false);
  let loop = $state(false);

  onMount(() => {
    let alive = true;
    void (async () => {
      try {
        const info = await boardAllowFile(path);
        if (!alive) return;
        file = info;
        if (info.kind === "text") {
          const body = await (await fetch(convertFileSrc(info.path))).text();
          if (!alive) return;
          text =
            body.length > TEXT_MAX
              ? `${body.slice(0, TEXT_MAX)}\n\n… ${t("page.agents.board.fileTruncated")}`
              : body;
        }
      } catch (err) {
        if (alive) error = err instanceof Error ? err.message : String(err);
      }
    })();
    return () => {
      alive = false;
    };
  });

  function act(action: (path: string) => Promise<void>) {
    void action(path).catch((err) => {
      error = err instanceof Error ? err.message : String(err);
    });
  }
</script>

<div class="file-view">
  <div class="bar">
    <span class="path" title={path}>{path}</span>
    {#if file?.kind === "video"}
      <button
        type="button"
        class="bar-btn"
        class:is-on={loop}
        aria-pressed={loop}
        title={t("page.agents.board.fileLoop")}
        aria-label={t("page.agents.board.fileLoop")}
        onclick={() => (loop = !loop)}
      >
        <Icon icon={RotateCw} size={12} />
      </button>
    {/if}
    <button
      type="button"
      class="bar-btn"
      title={t("page.agents.board.fileOpen")}
      aria-label={t("page.agents.board.fileOpen")}
      onclick={() => act(boardOpenExternal)}
    >
      <Icon icon={ExternalLink} size={12} />
    </button>
    <button
      type="button"
      class="bar-btn"
      title={t("page.agents.board.fileReveal")}
      aria-label={t("page.agents.board.fileReveal")}
      onclick={() => act(boardReveal)}
    >
      <Icon icon={Folder} size={12} />
    </button>
  </div>

  <div class="body" class:is-actual={actualSize}>
    {#if error}
      <p class="note" role="alert">{error}</p>
    {:else if !file}
      <p class="note">{t("page.agents.board.fileLoading")}</p>
    {:else if file.kind === "image"}
      <button
        type="button"
        class="zoom"
        title={actualSize ? t("page.agents.board.fileFit") : t("page.agents.board.fileActual")}
        onclick={() => (actualSize = !actualSize)}
      >
        <img class="media" {src} alt={file.name} draggable="false" />
      </button>
    {:else if file.kind === "video"}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video class="media" {src} controls preload="metadata" {loop}></video>
    {:else if file.kind === "audio"}
      <audio class="audio" {src} controls preload="metadata"></audio>
    {:else if file.kind === "pdf"}
      <iframe class="frame" {src} title={file.name}></iframe>
    {:else if file.kind === "text" && isMarkdown}
      <div class="markdown"><AgentMessage text={text ?? ""} /></div>
    {:else if file.kind === "text"}
      <pre class="text">{text ?? ""}</pre>
    {:else}
      <div class="note">
        <p>{t("page.agents.board.fileUnknown")}</p>
        <button type="button" class="open" onclick={() => act(boardOpenExternal)}>
          {t("page.agents.board.fileOpen")}
        </button>
      </div>
    {/if}
  </div>
</div>

<style>
  .file-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--rb-bg0);
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 6px 4px 10px;
    border-bottom: 1px solid color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--rb-muted);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }

  .bar-btn {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
  }

  .bar-btn.is-on,
  .bar-btn:hover {
    background: color-mix(in sRGB, var(--rb-text) 10%, transparent);
    color: var(--rb-text);
  }

  .body {
    position: relative;
    display: grid;
    flex: 1;
    min-height: 0;
    place-items: center;
    overflow: auto;
  }

  .media {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }

  .zoom {
    display: grid;
    place-items: center;
    width: 100%;
    height: 100%;
    border: 0;
    padding: 0;
    background: transparent;
    cursor: zoom-in;
  }

  .zoom .media {
    transition: transform 280ms var(--ease-island);
  }

  .zoom:active .media {
    transform: scale(0.98);
  }

  /* Tamaño real: la imagen manda y la tarjeta hace scroll. */
  .body.is-actual {
    place-items: start;
  }

  .body.is-actual .zoom {
    width: max-content;
    height: max-content;
    cursor: zoom-out;
  }

  .body.is-actual .media {
    max-width: none;
    max-height: none;
  }

  .markdown {
    align-self: stretch;
    justify-self: stretch;
    padding: 12px 16px;
    color: var(--rb-text);
  }

  .audio {
    width: min(90%, 420px);
  }

  .frame {
    width: 100%;
    height: 100%;
    border: 0;
    background: #fff;
  }

  .text {
    align-self: stretch;
    justify-self: stretch;
    margin: 0;
    padding: 10px 12px;
    color: var(--rb-text);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 12px;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
  }

  .note {
    display: grid;
    gap: 8px;
    place-items: center;
    margin: 0;
    padding: 16px;
    color: var(--rb-muted);
    font-size: 12px;
    text-align: center;
  }

  .note p {
    margin: 0;
  }

  .open {
    border: 1px solid color-mix(in sRGB, var(--rb-text) 16%, transparent);
    border-radius: 6px;
    padding: 4px 10px;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    cursor: pointer;
  }
</style>
