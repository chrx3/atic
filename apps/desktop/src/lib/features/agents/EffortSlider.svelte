<script lang="ts">
  /**
   * Esfuerzo del modelo como una escalera: cada nivel es un escalón más alto
   * y subir se ve. El color es el del agente (`--agent-accent`).
   *
   * Los niveles son los que informa el modelo, en su orden. Arrastrar por la
   * escalera solo previsualiza; el cambio va a la sesión al soltar, para no
   * mandar un `setModel` por cada escalón que cruza el puntero. El popover
   * queda abierto: se ajusta mirando el resultado.
   */
  import Icon from "$ui/Icon.svelte";
  import { RotateCcw } from "$lib/icons";
  import { t } from "$domain/i18n.svelte";
  import ChatPopover from "./ChatPopover.svelte";

  type Level = { id: string; label: string; note?: string };

  let {
    levels,
    value,
    defaultValue,
    modelName,
    open,
    onToggle,
    onPick,
  }: {
    levels: Level[];
    value: string;
    defaultValue: string;
    modelName: string;
    open: boolean;
    onToggle: (open: boolean) => void;
    onPick: (id: string) => void;
  } = $props();

  let stairsEl = $state<HTMLElement | null>(null);
  /** Escalón bajo el puntero mientras se arrastra; `null` en reposo. */
  let preview = $state<number | null>(null);

  const committed = $derived(
    Math.max(
      0,
      levels.findIndex((l) => l.id === value),
    ),
  );
  const index = $derived(preview ?? committed);
  const level = $derived(levels[index]);
  const canReset = $derived(!!defaultValue && value !== defaultValue);

  /** Alto de cada escalón, del 28 % al 100 %. */
  function stepHeight(i: number): number {
    return levels.length > 1 ? 0.28 + (0.72 * i) / (levels.length - 1) : 1;
  }

  function indexAt(clientX: number): number {
    if (!stairsEl || levels.length < 2) return 0;
    const rect = stairsEl.getBoundingClientRect();
    const x = (clientX - rect.left) / Math.max(1, rect.width);
    return Math.min(levels.length - 1, Math.max(0, Math.floor(x * levels.length)));
  }

  function commit(next: number) {
    const id = levels[next]?.id;
    if (id && id !== value) onPick(id);
  }

  function onPointerDown(event: PointerEvent) {
    if (event.button !== 0) return;
    stairsEl?.setPointerCapture(event.pointerId);
    preview = indexAt(event.clientX);
  }

  function onPointerMove(event: PointerEvent) {
    if (preview === null) return;
    preview = indexAt(event.clientX);
  }

  function onPointerUp() {
    if (preview === null) return;
    const next = preview;
    preview = null;
    commit(next);
  }

  function onKeydown(event: KeyboardEvent) {
    const last = levels.length - 1;
    const next =
      event.key === "ArrowRight" || event.key === "ArrowUp"
        ? Math.min(last, committed + 1)
        : event.key === "ArrowLeft" || event.key === "ArrowDown"
          ? Math.max(0, committed - 1)
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? last
              : null;
    if (next === null) return;
    event.preventDefault();
    commit(next);
  }
</script>

<ChatPopover {open} {onToggle} label={t("page.agents.chat.effort")} width={320}>
  {#snippet trigger()}
    <span class="meter" aria-hidden="true">
      {#each levels as l, i (l.id)}
        <span class="meter-bar" class:is-lit={i <= committed} style:--h={stepHeight(i)}
        ></span>
      {/each}
    </span>
    <span class="trigger-text"
      >{levels[committed]?.label ?? t("page.agents.chat.effort")}</span
    >
  {/snippet}

  <div class="effort">
    <div class="head">
      <div class="title">
        <p class="kicker">
          <span>{t("page.agents.chat.effort")}</span>
          {#if modelName}
            <span class="model">· {modelName}</span>
          {/if}
        </p>
        {#key level?.id}
          <p class="level">{level?.label}</p>
        {/key}
      </div>
      <button
        type="button"
        class="reset"
        aria-label={t("page.agents.chat.effortReset")}
        title={t("page.agents.chat.effortReset")}
        disabled={!canReset}
        onclick={() => commit(levels.findIndex((l) => l.id === defaultValue))}
      >
        <Icon icon={RotateCcw} size={14} />
      </button>
    </div>

    <div
      class="stairs"
      class:is-dragging={preview !== null}
      bind:this={stairsEl}
      role="slider"
      tabindex="0"
      aria-label={t("page.agents.chat.effort")}
      aria-valuemin={0}
      aria-valuemax={levels.length - 1}
      aria-valuenow={index}
      aria-valuetext={level?.label}
      onpointerdown={onPointerDown}
      onpointermove={onPointerMove}
      onpointerup={onPointerUp}
      onpointercancel={() => (preview = null)}
      onkeydown={onKeydown}
    >
      {#each levels as l, i (l.id)}
        <span
          class="step"
          class:is-lit={i <= index}
          class:is-current={i === index}
          class:is-default={l.id === defaultValue}
          style:--h={stepHeight(i)}
          style:--i={i}
          title={l.label}
        ></span>
      {/each}
    </div>

    <p class="note">
      {level?.note ||
        (level?.id === defaultValue ? t("page.agents.chat.effortDefault") : " ")}
    </p>
  </div>
</ChatPopover>

<style>
  .meter,
  .effort {
    --tone: var(--agent-accent, var(--accent));
  }

  /* Mini escalera en el botón: se lee el nivel sin abrir nada. */
  .meter {
    display: flex;
    align-items: flex-end;
    gap: 1.5px;
    height: 11px;
  }

  .meter-bar {
    width: 2.5px;
    height: calc(var(--h) * 100%);
    border-radius: 1px;
    background: color-mix(in sRGB, currentColor 30%, transparent);
    transition: background-color var(--duration-medium) ease;
  }

  .meter-bar.is-lit {
    background: var(--tone);
  }

  .trigger-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .effort {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 8px 8px 4px;
  }

  .head {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }

  .title {
    flex: 1;
    min-width: 0;
  }

  .kicker {
    overflow: hidden;
    margin: 0;
    color: var(--rb-muted);
    font-size: 11.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .model {
    color: var(--rb-faint, var(--rb-muted));
  }

  .level {
    margin: 2px 0 0;
    color: var(--rb-text);
    font-size: 20px;
    font-weight: 650;
    line-height: 1.15;
    letter-spacing: -0.015em;
    animation: level-in var(--duration-slow) var(--ease-calm);
  }

  @keyframes level-in {
    from {
      opacity: 0;
      translate: 0 3px;
    }
  }

  .reset {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 28px;
    height: 28px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
    transition:
      background-color var(--duration-fast) ease,
      color var(--duration-fast) ease,
      opacity var(--duration-fast) ease;
  }

  .reset:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
    color: var(--rb-text);
  }

  .reset:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .stairs {
    display: flex;
    align-items: flex-end;
    gap: 5px;
    height: 56px;
    border-radius: 10px;
    padding: 0 2px;
    cursor: pointer;
    touch-action: none;
    outline: none;
  }

  .stairs:focus-visible {
    box-shadow: 0 0 0 2px color-mix(in sRGB, var(--tone) 60%, transparent);
  }

  /* Cada escalón se enciende un poco más que el anterior: el color sube
     con la escalera en vez de ser un bloque parejo. */
  .step {
    position: relative;
    flex: 1;
    height: calc(var(--h) * 100%);
    border-radius: 6px 6px 4px 4px;
    background: color-mix(in sRGB, var(--rb-text) 9%, transparent);
    transition:
      background-color var(--duration-slow) ease,
      scale var(--duration-slow) var(--ease-calm);
    transition-delay: calc(var(--i) * var(--duration-stagger));
    transform-origin: bottom;
  }

  .step.is-lit {
    background: color-mix(in sRGB, var(--tone) calc(45% + var(--h) * 55%), transparent);
  }

  .step.is-current {
    scale: 1 1.06;
  }

  /* El nivel por defecto lleva una marca al pie, para saber a dónde vuelve. */
  .step.is-default::after {
    position: absolute;
    bottom: -7px;
    left: 50%;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--rb-muted);
    content: "";
    translate: -50% 0;
  }

  .stairs.is-dragging .step {
    transition-delay: 0s;
    transition-duration: var(--duration-quick);
  }

  .note {
    min-height: 1.4em;
    margin: 4px 0 0;
    color: var(--rb-muted);
    font-size: 12px;
    text-wrap: pretty;
  }

  @media (prefers-reduced-motion: reduce) {
    .level {
      animation: none;
    }

    .step,
    .meter-bar {
      transition: none;
    }
  }
</style>
