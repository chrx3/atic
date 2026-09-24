<script lang="ts">
  /**
   * El contenido de la lista de la pizarra: las consolas abiertas y las
   * sesiones guardadas.
   *
   * Tocar una consola la encuadra. «Guardar» anota las consolas abiertas con
   * un nombre; tocar una sesión guardada las vuelve a abrir.
   */
  import { tick } from "svelte";
  import { t } from "$domain/i18n.svelte";
  import { formatListWhen } from "$core/format";
  import Icon from "$ui/Icon.svelte";
  import { History, Save, Trash2, X } from "$lib/icons";
  import AgentLogo from "./AgentLogo.svelte";
  import ConsoleStateDot from "./ConsoleStateDot.svelte";
  import type { ConsoleState } from "./consoleStatus";
  import type { SavedSpace } from "./agentBoard";

  type Entry = {
    key: string;
    label: string;
    cli: string | null;
    status: ConsoleState;
    attention: boolean;
  };

  let {
    entries,
    spaces,
    active,
    defaultName,
    onSelect,
    onClose,
    onSaveSpace,
    onOpenSpace,
    onDeleteSpace,
  }: {
    entries: Entry[];
    spaces: SavedSpace[];
    active: string | null;
    /** El nombre que se propone al guardar: la carpeta de inicio. */
    defaultName: string;
    onSelect: (key: string) => void;
    onClose: (key: string) => void;
    onSaveSpace: (name: string) => void;
    onOpenSpace: (space: SavedSpace) => void;
    onDeleteSpace: (space: SavedSpace) => void;
  } = $props();

  /** `null` = sin el campo de nombre a la vista. */
  let spaceName = $state<string | null>(null);
  let nameInput = $state<HTMLInputElement | null>(null);

  async function startSave() {
    spaceName = defaultName;
    await tick();
    nameInput?.select();
  }

  function save() {
    const clean = spaceName?.trim();
    if (!clean || entries.length === 0) return;
    onSaveSpace(clean);
    spaceName = null;
  }

  function summary(space: SavedSpace): string {
    const n = space.consoles.length;
    const count =
      n === 1
        ? t("page.agents.board.spaceCountOne")
        : t("page.agents.board.spaceCount", { n });
    return space.savedAt ? `${count} · ${formatListWhen(space.savedAt / 1000)}` : count;
  }
</script>

<div class="tree">
  <section class="section">
    <div class="head">
      <span class="head-label">{t("page.agents.board.list")}</span>
      {#if entries.length > 0 && spaceName === null}
        <button
          type="button"
          class="head-action"
          title={t("page.agents.board.spaceSaveTip")}
          onclick={() => void startSave()}
        >
          <Icon icon={Save} size={12} />
          {t("page.agents.board.spaceSave")}
        </button>
      {/if}
    </div>

    {#if spaceName !== null}
      <form
        class="save"
        onsubmit={(e) => {
          e.preventDefault();
          save();
        }}
        onkeydown={(e) => {
          if (e.key === "Escape") {
            e.stopPropagation();
            spaceName = null;
          }
        }}
      >
        <input
          bind:this={nameInput}
          bind:value={spaceName}
          placeholder={t("page.agents.board.spaceName")}
          aria-label={t("page.agents.board.spaceName")}
          onblur={() => {
            if (!spaceName?.trim()) spaceName = null;
          }}
        />
        <button type="submit" disabled={!spaceName.trim()}>
          {t("page.agents.board.spaceSave")}
        </button>
      </form>
    {/if}

    {#each entries as entry, i (entry.key)}
      <div class="row" class:is-active={entry.key === active}>
        <button
          type="button"
          class="entry"
          aria-current={entry.key === active ? "true" : undefined}
          title={i < 9 ? `${entry.label} · Ctrl+${i + 1}` : entry.label}
          onclick={() => onSelect(entry.key)}
        >
          <AgentLogo agent={entry.cli} size={15} />
          <span class="name">{entry.label}</span>
          <ConsoleStateDot status={entry.status} attention={entry.attention} />
        </button>
        <button
          type="button"
          class="hover-btn"
          aria-label={t("page.agents.window.close", { name: entry.label })}
          onclick={() => onClose(entry.key)}
        >
          <Icon icon={X} size={11} />
        </button>
      </div>
    {:else}
      <p class="quiet">{t("page.agents.window.noSessions")}</p>
    {/each}
  </section>

  {#if spaces.length > 0}
    <section class="section">
      <div class="head">
        <span class="head-label">{t("page.agents.board.spaces")}</span>
      </div>
      {#each spaces as space (space.name)}
        <div class="row">
          <button
            type="button"
            class="entry is-space"
            title={t("page.agents.board.spaceOpen", {
              consoles: space.consoles.map((c) => c.label).join(", "),
            })}
            onclick={() => onOpenSpace(space)}
          >
            <Icon icon={History} size={14} />
            <span class="stack">
              <span class="name">{space.name}</span>
              <span class="meta">{summary(space)}</span>
            </span>
          </button>
          <button
            type="button"
            class="hover-btn is-danger"
            aria-label={t("page.agents.board.spaceDelete", { name: space.name })}
            title={t("page.agents.board.spaceDelete", { name: space.name })}
            onclick={() => onDeleteSpace(space)}
          >
            <Icon icon={Trash2} size={11} />
          </button>
        </div>
      {/each}
    </section>
  {/if}
</div>

<style>
  .tree {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
    overflow-y: auto;
    scrollbar-width: thin;
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 24px;
    padding: 0 4px 0 8px;
    color: var(--rb-faint);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.02em;
    text-transform: uppercase;
  }

  .head-label {
    flex: 1;
  }

  .head-action {
    display: flex;
    align-items: center;
    gap: 5px;
    height: 22px;
    border: 0;
    border-radius: 6px;
    padding: 0 7px;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 11px;
    font-weight: 560;
    letter-spacing: 0;
    text-transform: none;
    cursor: pointer;
    transition:
      background-color var(--duration-fast) ease,
      color var(--duration-fast) ease;
  }

  .head-action:hover {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
    color: var(--rb-text);
  }

  .row {
    position: relative;
    display: flex;
    align-items: center;
    border-radius: 9px;
    transition: background-color var(--duration-fast) ease;
  }

  .row:hover {
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
  }

  .row.is-active {
    background: color-mix(in sRGB, var(--rb-text) 10%, transparent);
  }

  .entry {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 8px;
    min-width: 0;
    border: 0;
    padding: 6px 8px;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }

  .entry.is-space {
    color: var(--rb-muted);
  }

  .row:hover .entry.is-space {
    color: var(--rb-text);
  }

  .stack {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    overflow: hidden;
    color: var(--rb-faint);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hover-btn {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 22px;
    height: 22px;
    margin-right: 3px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--rb-faint);
    cursor: pointer;
    opacity: 0;
    transition: opacity var(--duration-fast) ease;
  }

  .row:hover .hover-btn,
  .hover-btn:focus-visible {
    opacity: 1;
  }

  .hover-btn:hover {
    background: color-mix(in sRGB, var(--rb-text) 9%, transparent);
    color: var(--rb-text);
  }

  .hover-btn.is-danger:hover {
    background: color-mix(in sRGB, var(--rb-record) 16%, transparent);
    color: var(--rb-record);
  }

  .quiet {
    margin: 3px 8px;
    color: var(--rb-faint);
    font-size: 11.5px;
  }

  .save {
    display: flex;
    gap: 6px;
    margin: 2px 2px 4px;
  }

  .save input {
    flex: 1;
    min-width: 0;
    height: 28px;
    border: 0;
    border-radius: 8px;
    padding: 0 9px;
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
    color: var(--rb-text);
    font: inherit;
    font-size: 12px;
    outline: none;
  }

  .save input::placeholder {
    color: var(--rb-faint);
  }

  .save input:focus {
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--accent) 55%, transparent);
  }

  .save button {
    flex-shrink: 0;
    height: 28px;
    border: 0;
    border-radius: 8px;
    padding: 0 10px;
    background: var(--accent);
    color: var(--rb-on-accent, var(--rb-bg0));
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
  }

  .save button:disabled {
    background: color-mix(in sRGB, var(--rb-text) 10%, transparent);
    color: var(--rb-muted);
    cursor: default;
  }
</style>
