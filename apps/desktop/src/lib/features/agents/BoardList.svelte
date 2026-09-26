<script lang="ts">
  /**
   * Lo que flota abajo a la izquierda de la pizarra: abrir una consola, la
   * lista de lo abierto y lo guardado (`BoardTree`), la carpeta donde nacen
   * y los ajustes. Anclada abajo crece hacia arriba, y sus menús tienen lugar
   * para abrirse sin cortarse contra el borde de la ventana.
   *
   * Achicada queda como una barra de íconos: deja la pizarra casi entera y
   * cada consola se sigue reconociendo por el logo de su agente.
   */
  import { untrack } from "svelte";
  import { prefersReducedMotion } from "$lib/motion";
  import { t } from "$domain/i18n.svelte";
  import Icon from "$ui/Icon.svelte";
  import {
    Folder,
    PanelLeftClose,
    PanelLeftOpen,
    Plus,
    Settings,
    SquareTerminal,
  } from "$lib/icons";
  import AgentLogo from "./AgentLogo.svelte";
  import ConsoleStateDot from "./ConsoleStateDot.svelte";
  import type { ConsoleState } from "./consoleStatus";
  import ChatPopover from "./ChatPopover.svelte";
  import BoardTree from "./BoardTree.svelte";
  import type { AgentDef } from "./agentCatalog";
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
    currentSpace = null,
    active,
    choices,
    cwd,
    newOpen,
    collapsed,
    onCollapse,
    onNewToggle,
    onNew,
    onSelect,
    onClose,
    onPickFolder,
    onSettings,
    onSaveSpace,
    onOpenSpace,
    onDeleteSpace,
  }: {
    entries: Entry[];
    spaces: SavedSpace[];
    /** El espacio que esta pizarra va guardando sola. */
    currentSpace?: string | null;
    active: string | null;
    choices: AgentDef[];
    cwd: string;
    newOpen: boolean;
    collapsed: boolean;
    onCollapse: (collapsed: boolean) => void;
    onNewToggle: (open: boolean) => void;
    /** `null` = la terminal del sistema. */
    onNew: (agent: AgentDef | null) => void;
    onSelect: (key: string) => void;
    onClose: (key: string) => void;
    onPickFolder: () => void;
    onSettings: () => void;
    onSaveSpace: (name: string) => void;
    onOpenSpace: (space: SavedSpace) => void;
    onDeleteSpace: (space: SavedSpace) => void;
  } = $props();

  function folderName(path: string): string {
    if (!path) return t("page.agents.window.home");
    return (
      path
        .replace(/[/\\]+$/, "")
        .split(/[/\\]/)
        .pop() || path
    );
  }

  /**
   * Abrir y cerrar en dos tiempos, con el mismo ritmo en los dos sentidos.
   *
   * Abrir: primero sube (el alto de la lista), después se ensancha. Cerrar:
   * al revés, primero se angosta y después baja. El alto cambia con el
   * contenido y eso CSS no lo anima, así que se mide antes y después del
   * cambio y se anima entre las dos cajas.
   */
  const MORPH_MS = 620;
  /** Dónde termina el primer tramo y empieza el segundo. */
  const MORPH_SPLIT = 0.48;
  const EASE_STEP = "cubic-bezier(0.45, 0, 0.2, 1)";

  let listEl = $state<HTMLElement | null>(null);
  let before: DOMRect | null = null;
  let shownCollapsed = untrack(() => collapsed);
  let running: Animation[] = [];

  // Antes de que el DOM cambie: la caja de donde parte.
  $effect.pre(() => {
    const next = collapsed;
    if (next !== shownCollapsed && listEl) before = listEl.getBoundingClientRect();
  });

  // Con el DOM ya cambiado: la caja a la que llega, y la animación.
  $effect(() => {
    const next = collapsed;
    if (next === shownCollapsed) return;
    shownCollapsed = next;
    const from = before;
    before = null;
    if (!from || !listEl || prefersReducedMotion()) return;
    morph(listEl, from, listEl.getBoundingClientRect(), next);
  });

  function morph(el: HTMLElement, from: DOMRect, to: DOMRect, closing: boolean) {
    for (const a of running) a.cancel();
    const box = (w: number, h: number) => ({ width: `${w}px`, height: `${h}px` });
    // Primer tramo: abriendo cambia el alto; cerrando, el ancho.
    const mid = closing ? box(to.width, from.height) : box(from.width, to.height);
    el.classList.add("is-morphing");
    const shape = el.animate(
      [
        { ...box(from.width, from.height), easing: EASE_STEP },
        { ...mid, offset: MORPH_SPLIT, easing: EASE_STEP },
        box(to.width, to.height),
      ],
      { duration: MORPH_MS },
    );
    // El contenido nuevo aparece cuando la caja ya casi llegó: antes se
    // vería apretado o cortado.
    const fades = [...el.children].map((child) =>
      child.animate(
        [
          { opacity: 0, transform: "translateY(4px)" },
          { opacity: 0, transform: "translateY(4px)", offset: 0.62 },
          { opacity: 1, transform: "none" },
        ],
        { duration: MORPH_MS, easing: "ease-out" },
      ),
    );
    running = [shape, ...fades];
    void shape.finished
      .then(() => el.classList.remove("is-morphing"))
      .catch(() => {});
  }
</script>

<aside
  bind:this={listEl}
  class="list"
  class:is-collapsed={collapsed}
  aria-label={t("page.agents.board.list")}
>
  <div class="top">
    <ChatPopover
      open={newOpen}
      onToggle={onNewToggle}
      label={t("page.agents.board.newTip")}
      width={240}
    >
      {#snippet trigger()}
        <Icon icon={Plus} size={14} />
        {#if !collapsed}
          <span class="new-label">{t("page.agents.board.new")}</span>
        {/if}
      {/snippet}

      {#each choices as agent (agent.cli)}
        <button type="button" class="pick" onclick={() => onNew(agent)}>
          <AgentLogo agent={agent.cli} size={15} />
          {agent.name}
        </button>
      {/each}
      <button type="button" class="pick is-shell" onclick={() => onNew(null)}>
        <Icon icon={SquareTerminal} size={15} />
        {t("page.agents.window.shell")}
      </button>
    </ChatPopover>
    <button
      type="button"
      class="fold"
      aria-label={collapsed
        ? t("page.agents.board.expand")
        : t("page.agents.board.collapse")}
      title={collapsed
        ? t("page.agents.board.expand")
        : t("page.agents.board.collapse")}
      onclick={() => onCollapse(!collapsed)}
    >
      <Icon icon={collapsed ? PanelLeftOpen : PanelLeftClose} size={15} />
    </button>
  </div>

  {#if collapsed}
    <!-- Achicada: solo el logo de cada una, con su estado encima. -->
    <nav class="entries">
      {#each entries as entry (entry.key)}
        <div
          class="row"
          class:is-active={entry.key === active}
        >
          <button
            type="button"
            class="entry"
            aria-current={entry.key === active ? "true" : undefined}
            title={entry.label}
            onclick={() => onSelect(entry.key)}
          >
            <span class="logo">
              <AgentLogo agent={entry.cli} size={16} />
              <span class="badge">
                <ConsoleStateDot status={entry.status} attention={entry.attention} />
              </span>
            </span>
          </button>
        </div>
      {/each}
    </nav>
  {:else}
    <div class="tree-wrap">
    <BoardTree
      {entries}
      {spaces}
      {currentSpace}
      {active}
      defaultName={currentSpace ?? (cwd ? folderName(cwd) : "")}
      {onSelect}
      {onClose}
      {onSaveSpace}
      {onOpenSpace}
      {onDeleteSpace}
    />
    </div>
  {/if}

  <footer class="foot">
    {#if !collapsed}
      <button
        type="button"
        class="folder"
        title={`${t("page.agents.window.pickFolder")}: ${cwd || t("page.agents.window.home")}`}
        onclick={onPickFolder}
      >
        <Icon icon={Folder} size={13} />
        <span>{folderName(cwd)}</span>
      </button>
    {/if}
    <button
      type="button"
      class="gear"
      aria-label={t("settings.agents.modalTitle")}
      title={t("settings.agents.modalTitle")}
      onclick={onSettings}
    >
      <Icon icon={Settings} size={14} />
    </button>
  </footer>
</aside>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 232px;
    max-height: 100%;
    box-sizing: border-box;
    border-radius: 16px;
    padding: 8px;
    background: color-mix(in sRGB, var(--rb-surface) 82%, transparent);
    box-shadow:
      0 0 0 1px color-mix(in sRGB, var(--rb-text) 10%, transparent),
      0 18px 40px -18px rgb(0 0 0 / 55%);
    backdrop-filter: blur(18px) saturate(1.2);
  }

  /* Mientras cambia de forma, lo que no cabe se corta en vez de asomarse. */
  .list:global(.is-morphing) {
    overflow: hidden;
  }

  /* El scroll de adentro también se corta durante el morph: el árbol queda
     más chico que su contenido mientras la caja crece y su barra asomaba toda
     la transición, para desaparecer al llegar al tamaño final. */
  .list:global(.is-morphing) :global(.tree),
  .list:global(.is-morphing) .entries {
    overflow: hidden;
  }

  .list.is-collapsed {
    width: 52px;
    padding: 6px;
  }

  .top {
    display: flex;
    gap: 4px;
  }

  .list.is-collapsed .top {
    flex-direction: column;
  }

  .top :global(.anchor) {
    flex: 1;
    min-width: 0;
  }

  .fold {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 34px;
    height: 34px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
    transition:
      background-color var(--duration-fast) ease,
      color var(--duration-fast) ease;
  }

  .fold:hover {
    background: color-mix(in sRGB, var(--rb-text) 7%, transparent);
    color: var(--rb-text);
  }

  .list.is-collapsed .top :global(.trigger) {
    justify-content: center;
    padding: 0;
  }

  .list.is-collapsed .entry {
    justify-content: center;
    padding: 8px 0;
  }

  .list.is-collapsed .foot {
    flex-direction: column;
    align-items: center;
  }

  .foot :global(.trigger) {
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
  }

  .logo {
    position: relative;
    display: grid;
    flex-shrink: 0;
    place-items: center;
  }

  .badge {
    position: absolute;
    right: -4px;
    bottom: -3px;
    display: grid;
    border-radius: 50%;
    padding: 1px;
    background: var(--rb-surface);
  }

  .top :global(.trigger) {
    width: 100%;
    height: 34px;
    justify-content: flex-start;
    gap: 8px;
    background: color-mix(in sRGB, var(--rb-text) 7%, transparent);
    color: var(--rb-text);
    font-size: 13px;
    font-weight: 600;
  }

  .new-label {
    flex: 1;
    text-align: left;
  }

  .pick {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    border: 0;
    border-radius: 8px;
    padding: 7px 8px;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }

  .pick:hover {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .pick.is-shell {
    margin-top: 4px;
    border-top: 1px solid color-mix(in sRGB, var(--rb-text) 7%, transparent);
    border-radius: 0 0 8px 8px;
    color: var(--rb-muted);
  }

  .entries {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-height: 0;
    overflow-y: auto;
  }

  .row {
    position: relative;
    display: flex;
    align-items: center;
    border-radius: 10px;
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
    gap: 9px;
    min-width: 0;
    border: 0;
    padding: 7px 8px;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }

  .foot {
    display: flex;
    gap: 2px;
    padding-top: 6px;
    border-top: 1px solid color-mix(in sRGB, var(--rb-text) 7%, transparent);
  }

  .folder,
  .gear {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    border: 0;
    border-radius: 8px;
    padding: 0 8px;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
    transition:
      background-color var(--duration-fast) ease,
      color var(--duration-fast) ease;
  }

  .folder {
    flex: 1;
    min-width: 0;
  }

  .folder span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .gear {
    flex-shrink: 0;
    justify-content: center;
    width: 30px;
    padding: 0;
  }

  .folder:hover,
  .gear:hover {
    background: color-mix(in sRGB, var(--rb-text) 7%, transparent);
    color: var(--rb-text);
  }

  /* Lleva la transición del árbol; deja que su scroll siga funcionando. */
  .tree-wrap {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

</style>
