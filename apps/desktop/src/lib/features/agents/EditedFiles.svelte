<script lang="ts">
  /**
   * Lo que un tramo de actividad dejó cambiado, archivo por archivo.
   *
   * La pregunta al terminar un turno es «¿qué tocó?», y la respuesta estaba
   * repartida entre tarjetas de herramientas plegadas. Acá va junta: el
   * nombre del archivo, su carpeta y cuántas líneas entraron y salieron.
   */
  import { t } from "$domain/i18n.svelte";
  import type { EditedFile } from "./chatThread";

  let { files }: { files: EditedFile[] } = $props();

  /** Con más de esto la lista se pliega: el resto no cabe de un vistazo. */
  const FOLD_FROM = 5;

  let expanded = $state(false);

  const shown = $derived(expanded ? files : files.slice(0, FOLD_FROM));
  const totals = $derived(
    files.reduce((sum, f) => ({ add: sum.add + f.add, del: sum.del + f.del }), {
      add: 0,
      del: 0,
    }),
  );
  /** Cinco casillas repartidas entre altas y bajas, como proporción. */
  const squares = $derived.by(() => {
    const all = totals.add + totals.del;
    const add = all === 0 ? 0 : Math.round((totals.add / all) * 5);
    return Array.from({ length: 5 }, (_, i) => (i < add ? "add" : "del"));
  });

  function split(path: string): { name: string; dir: string } {
    const parts = path.replace(/[/\\]+$/, "").split(/[/\\]/);
    const name = parts.pop() ?? path;
    return { name, dir: parts.slice(-2).join("/") };
  }
</script>

<div class="files">
  <div class="head">
    <span class="squares" aria-hidden="true">
      {#each squares as tone, i (i)}
        <span class="square is-{tone}"></span>
      {/each}
    </span>
    <span class="label">
      {files.length === 1
        ? t("page.agents.chat.act.filesOne")
        : t("page.agents.chat.act.filesMany", { n: files.length })}
    </span>
    <span class="delta">
      <span class="add">+{totals.add}</span>
      <span class="del">−{totals.del}</span>
    </span>
  </div>

  <ul class="list">
    {#each shown as file (file.path)}
      {@const parts = split(file.path)}
      <li class="row" title={file.path}>
        <span class="name">{parts.name}</span>
        {#if parts.dir}<span class="dir">{parts.dir}</span>{/if}
        <span class="delta">
          {#if file.add}<span class="add">+{file.add}</span>{/if}
          {#if file.del}<span class="del">−{file.del}</span>{/if}
        </span>
      </li>
    {/each}
  </ul>

  {#if files.length > FOLD_FROM}
    <button type="button" class="more" onclick={() => (expanded = !expanded)}>
      {expanded
        ? t("page.agents.chat.act.filesLess")
        : t("page.agents.chat.act.filesMore", { n: files.length - FOLD_FROM })}
    </button>
  {/if}
</div>

<style>
  .files {
    overflow: hidden;
    border-radius: 12px;
    background: color-mix(in sRGB, var(--rb-text) 3%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--rb-text) 8%, transparent);
    font-size: 12px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-bottom: 1px solid color-mix(in sRGB, var(--rb-text) 6%, transparent);
  }

  .squares {
    display: flex;
    gap: 2px;
  }

  .square {
    width: 7px;
    height: 7px;
    border-radius: 2px;
  }

  .square.is-add {
    background: var(--rb-ok);
  }

  .square.is-del {
    background: color-mix(in sRGB, var(--rb-record) 75%, transparent);
  }

  .label {
    flex: 1;
    color: var(--rb-text);
    font-weight: 560;
  }

  .delta {
    display: flex;
    flex-shrink: 0;
    gap: 6px;
    margin-left: auto;
    font-family: var(--rb-mono);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .add {
    color: var(--rb-ok);
  }

  .del {
    color: var(--rb-record);
  }

  .list {
    margin: 0;
    padding: 4px 0;
    list-style: none;
  }

  .row {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
    padding: 4px 12px;
  }

  .name {
    flex-shrink: 0;
    max-width: 60%;
    overflow: hidden;
    color: var(--rb-text);
    font-family: var(--rb-mono);
    font-size: 11.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dir {
    min-width: 0;
    overflow: hidden;
    color: var(--rb-faint);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }

  .more {
    width: 100%;
    border: 0;
    border-top: 1px solid color-mix(in sRGB, var(--rb-text) 6%, transparent);
    padding: 6px 12px;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 11.5px;
    text-align: left;
    cursor: pointer;
    transition: color var(--duration-fast) ease;
  }

  .more:hover {
    color: var(--rb-text);
  }
</style>
