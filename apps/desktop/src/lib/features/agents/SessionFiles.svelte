<script lang="ts">
  /**
   * Lo que cambió en la carpeta de una consola desde que se abrió: lo que
   * generó o tocó la sesión, sea por edición del agente o por un comando.
   * Clic lo abre en la pizarra; la carpeta lo muestra en el Explorador.
   */
  import { onMount } from "svelte";
  import { fly } from "svelte/transition";
  import { backOut } from "svelte/easing";
  import { prefersReducedMotion } from "$lib/motion";
  import Icon from "$ui/Icon.svelte";
  import { FileImage, FileText, FileType, Folder, X } from "$lib/icons";
  import { t } from "$domain/i18n.svelte";
  import { boardReveal, consoleChangedFiles, type ChangedFile } from "$ipc/agents";

  let {
    session,
    onOpen,
    onClose,
  }: {
    /** La consola (PTY), no la sesión del agente. */
    session: string;
    onOpen: (path: string) => void;
    onClose: () => void;
  } = $props();

  /** Mientras está abierto se vuelve a mirar: el agente sigue escribiendo. */
  const REFRESH_MS = 3000;

  let files = $state<ChangedFile[] | null>(null);
  let error = $state<string | null>(null);
  let now = $state(Date.now());
  /** Solo lo que nació en la sesión, sin lo que ya existía y se tocó. */
  let onlyNew = $state(false);
  /** Agrupado por carpeta en vez de por fecha. */
  let byFolder = $state(false);

  const shown = $derived.by(() => {
    const list = (files ?? []).filter((f) => !onlyNew || f.created);
    if (!byFolder) return list;
    return [...list].sort(
      (a, b) =>
        dirOf(a.path).localeCompare(dirOf(b.path)) || b.modifiedMs - a.modifiedMs,
    );
  });
  const reduced = prefersReducedMotion();

  async function load() {
    try {
      files = await consoleChangedFiles(session);
      error = null;
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    }
    now = Date.now();
  }

  onMount(() => {
    void load();
    const timer = window.setInterval(() => void load(), REFRESH_MS);
    return () => window.clearInterval(timer);
  });

  function ago(ms: number): string {
    const s = Math.max(0, Math.round((now - ms) / 1000));
    if (s < 60) return t("page.agents.board.filesSeconds", { n: s });
    const m = Math.round(s / 60);
    if (m < 60) return t("page.agents.board.filesMinutes", { n: m });
    return t("page.agents.board.filesHours", { n: Math.round(m / 60) });
  }

  function iconOf(kind: ChangedFile["kind"]) {
    if (kind === "image" || kind === "video") return FileImage;
    if (kind === "text") return FileText;
    return FileType;
  }

  /** La carpeta del archivo, sin el nombre: ubica sin repetirlo. */
  function dirOf(path: string): string {
    const cut = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
    return cut > 0 ? path.slice(0, cut) : "";
  }
</script>

<aside
  class="session-files"
  aria-label={t("page.agents.board.filesTitle")}
  in:fly={{ x: 28, duration: reduced ? 0 : 320, easing: backOut, opacity: 0 }}
  out:fly={{ x: 20, duration: reduced ? 0 : 140, opacity: 0 }}
>
  <header class="head">
    <span class="title">{t("page.agents.board.filesTitle")}</span>
    <button
      type="button"
      class="chip"
      class:is-on={onlyNew}
      aria-pressed={onlyNew}
      onclick={() => (onlyNew = !onlyNew)}
    >
      {t("page.agents.board.filesOnlyNew")}
    </button>
    <button
      type="button"
      class="chip"
      class:is-on={byFolder}
      aria-pressed={byFolder}
      onclick={() => (byFolder = !byFolder)}
    >
      {t("page.agents.board.filesByFolder")}
    </button>
    <button
      type="button"
      class="icon-btn"
      aria-label={t("page.agents.board.filesClose")}
      title={t("page.agents.board.filesClose")}
      onclick={onClose}
    >
      <Icon icon={X} size={11} />
    </button>
  </header>

  {#if error}
    <p class="note" role="alert">{error}</p>
  {:else if files === null}
    <p class="note">{t("page.agents.board.fileLoading")}</p>
  {:else if shown.length === 0}
    <p class="note">{t("page.agents.board.filesEmpty")}</p>
  {:else}
    <ul class="list">
      {#each shown as file, i (file.path)}
        {#if byFolder && (i === 0 || dirOf(shown[i - 1].path) !== dirOf(file.path))}
          <li class="group" title={dirOf(file.path)}>{dirOf(file.path) || "·"}</li>
        {/if}
        <li class="row" style:--i={Math.min(i, 12)}>
          <button
            type="button"
            class="entry"
            title={file.path}
            onclick={() => onOpen(file.path)}
          >
            <Icon icon={iconOf(file.kind)} size={13} />
            <span class="stack">
              <span class="name">
                {file.name}
                {#if file.created}<span class="badge"
                    >{t("page.agents.board.filesNew")}</span
                  >{/if}
              </span>
              <span class="meta">
                {byFolder
                  ? ago(file.modifiedMs)
                  : `${dirOf(file.path)} · ${ago(file.modifiedMs)}`}
              </span>
            </span>
          </button>
          <button
            type="button"
            class="icon-btn"
            aria-label={t("page.agents.board.fileReveal")}
            title={t("page.agents.board.fileReveal")}
            onclick={() => void boardReveal(file.path).catch(() => {})}
          >
            <Icon icon={Folder} size={11} />
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</aside>

<style>
  .session-files {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    z-index: 3;
    display: flex;
    flex-direction: column;
    width: min(280px, 70%);
    border-left: 1px solid color-mix(in sRGB, var(--rb-text) 10%, transparent);
    background: var(--rb-bg1, var(--rb-bg0));
    box-shadow: -8px 0 24px rgb(0 0 0 / 25%);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 6px 6px 10px;
    border-bottom: 1px solid color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .title {
    flex: 1;
    color: var(--rb-text);
    font-size: 12px;
    font-weight: 600;
  }

  .list {
    flex: 1;
    min-height: 0;
    margin: 0;
    padding: 4px;
    overflow-y: auto;
    list-style: none;
  }

  /* Las filas llegan en cascada; las que ya estaban no se vuelven a animar
     porque la lista va con clave. */
  .row {
    display: flex;
    align-items: center;
    border-radius: 6px;
    animation: row-in 260ms var(--ease-smooth-out) both;
    animation-delay: calc(var(--i, 0) * 20ms);
  }

  @keyframes row-in {
    from {
      opacity: 0;
      transform: translateX(8px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .row {
      animation: none;
    }
  }

  .group {
    overflow: hidden;
    padding: 8px 6px 2px;
    color: var(--rb-faint, var(--rb-muted));
    font-size: 10.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }

  .chip {
    border: 1px solid color-mix(in sRGB, var(--rb-text) 12%, transparent);
    border-radius: 999px;
    padding: 1px 7px;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 10.5px;
    cursor: pointer;
    transition:
      background-color var(--duration-fast) ease,
      color var(--duration-fast) ease;
  }

  .chip.is-on {
    border-color: transparent;
    background: color-mix(in sRGB, var(--accent) 22%, transparent);
    color: var(--rb-text);
  }

  .row:hover {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .entry {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 8px;
    min-width: 0;
    border: 0;
    padding: 5px 6px;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .row:hover .entry {
    color: var(--rb-text);
  }

  .stack {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .name,
  .meta {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .name {
    color: var(--rb-text);
    font-size: 12px;
  }

  .meta {
    color: var(--rb-muted);
    font-size: 10.5px;
  }

  .badge {
    margin-left: 4px;
    border-radius: 4px;
    padding: 0 4px;
    background: color-mix(in sRGB, var(--coral, var(--accent)) 22%, transparent);
    color: var(--coral, var(--accent));
    font-size: 9.5px;
  }

  .icon-btn {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 22px;
    height: 22px;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
  }

  .icon-btn:hover {
    background: color-mix(in sRGB, var(--rb-text) 10%, transparent);
    color: var(--rb-text);
  }

  .note {
    margin: 0;
    padding: 14px 12px;
    color: var(--rb-muted);
    font-size: 12px;
  }
</style>
