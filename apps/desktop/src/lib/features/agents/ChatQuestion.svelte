<script lang="ts">
  /**
   * Las preguntas del agente, una por página, donde el agente se detuvo.
   *
   * Antes llegaban como un permiso de «AskUserQuestion» con el JSON crudo:
   * se podía aprobar o rechazar, pero no contestar. Acá se elige (o se
   * escribe) cada respuesta y se envían todas juntas.
   */
  import { t } from "$domain/i18n.svelte";
  import type { AgentQuestion } from "./chatQuestions";

  let {
    questions,
    agentName,
    busy = false,
    allowOwn = true,
    onAnswer,
    onSkip,
  }: {
    questions: AgentQuestion[];
    agentName: string;
    busy?: boolean;
    /** Cursor solo acepta sus opciones: ahí no se ofrece escribir una propia. */
    allowOwn?: boolean;
    onAnswer: (picked: string[][], written: string[]) => void;
    onSkip: () => void;
  } = $props();

  let page = $state(0);
  let picked = $state<string[][]>([]);
  let written = $state<string[]>([]);

  const question = $derived(questions[page]);
  const last = $derived(page === questions.length - 1);
  const answered = $derived(!!written[page]?.trim() || (picked[page]?.length ?? 0) > 0);

  function toggle(label: string) {
    const current = picked[page] ?? [];
    const next = question.multiSelect
      ? current.includes(label)
        ? current.filter((l) => l !== label)
        : [...current, label]
      : [label];
    picked[page] = next;
    // Una sola opción y no es la última: elegir ya es avanzar.
    if (!question.multiSelect && !last) page += 1;
  }

  function next() {
    if (last) onAnswer(picked, written);
    else page += 1;
  }
</script>

<section class="ask" aria-label={t("page.agents.chat.askTitle", { name: agentName })}>
  <header class="head">
    {#if question.header}<span class="chip">{question.header}</span>{/if}
    {#if questions.length > 1}
      <span class="pager">{page + 1} / {questions.length}</span>
    {/if}
  </header>
  <p class="question">{question.question}</p>

  <div class="options" role={question.multiSelect ? "group" : "radiogroup"}>
    {#each question.options as option (option.label)}
      {@const on = picked[page]?.includes(option.label) ?? false}
      <button
        type="button"
        class="option"
        class:is-on={on}
        role={question.multiSelect ? "checkbox" : "radio"}
        aria-checked={on}
        disabled={busy}
        onclick={() => toggle(option.label)}
      >
        <span class="mark" class:is-multi={question.multiSelect} aria-hidden="true"
        ></span>
        <span class="text">
          <span class="label">{option.label}</span>
          {#if option.description}<span class="desc">{option.description}</span>{/if}
        </span>
      </button>
    {/each}
  </div>

  {#if allowOwn}
    <input
      class="own"
      type="text"
      placeholder={t("page.agents.chat.askOwn")}
      bind:value={written[page]}
      disabled={busy}
      onkeydown={(e) => {
        if (e.key === "Enter" && answered) {
          e.preventDefault();
          next();
        }
      }}
    />
  {/if}

  <div class="actions">
    <button type="button" class="btn is-ghost" disabled={busy} onclick={onSkip}>
      {t("page.agents.chat.askSkip")}
    </button>
    <span class="grow"></span>
    {#if page > 0}
      <button type="button" class="btn" disabled={busy} onclick={() => (page -= 1)}>
        {t("page.agents.chat.askBack")}
      </button>
    {/if}
    <button
      type="button"
      class="btn is-primary"
      disabled={busy || !answered}
      onclick={next}
    >
      {last ? t("page.agents.chat.askSend") : t("page.agents.chat.askNext")}
    </button>
  </div>
</section>

<style>
  .ask {
    display: flex;
    flex-direction: column;
    gap: 10px;
    border-radius: 14px;
    padding: 12px;
    background: color-mix(in sRGB, var(--accent) 7%, var(--rb-surface));
    box-shadow:
      inset 0 0 0 1px color-mix(in sRGB, var(--accent) 28%, transparent),
      0 8px 24px -16px rgb(0 0 0 / 50%);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 18px;
  }

  .chip {
    border-radius: 999px;
    padding: 1px 8px;
    background: color-mix(in sRGB, var(--accent) 18%, transparent);
    color: var(--rb-text);
    font-size: 11px;
    font-weight: 600;
  }

  .pager {
    margin-left: auto;
    color: var(--rb-faint);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .question {
    margin: 0;
    font-size: 13.5px;
    font-weight: 600;
    text-wrap: pretty;
  }

  .options {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .option {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    border: 0;
    border-radius: 10px;
    padding: 8px 10px;
    background: color-mix(in sRGB, var(--rb-text) 5%, transparent);
    color: var(--rb-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      background-color 120ms ease,
      box-shadow 120ms ease;
  }

  .option:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--rb-text) 9%, transparent);
  }

  .option.is-on {
    background: color-mix(in sRGB, var(--accent) 14%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--accent) 45%, transparent);
  }

  /* Círculo si es una sola; cuadrado si son varias: lo que dice cada control. */
  .mark {
    flex-shrink: 0;
    width: 14px;
    height: 14px;
    margin-top: 2px;
    border-radius: 999px;
    box-shadow: inset 0 0 0 1.5px var(--rb-faint);
    transition:
      box-shadow 120ms ease,
      background-color 120ms ease;
  }

  .mark.is-multi {
    border-radius: 4px;
  }

  .is-on .mark {
    background: var(--accent);
    box-shadow: inset 0 0 0 3px color-mix(in sRGB, var(--rb-surface) 80%, transparent);
  }

  .is-on .mark.is-multi {
    box-shadow: none;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .label {
    font-size: 12.5px;
    font-weight: 560;
  }

  .desc {
    color: var(--rb-muted);
    font-size: 11.5px;
    text-wrap: pretty;
  }

  .own {
    box-sizing: border-box;
    width: 100%;
    border: 0;
    border-radius: 8px;
    padding: 7px 10px;
    background: color-mix(in sRGB, var(--rb-text) 5%, transparent);
    color: var(--rb-text);
    font: inherit;
    font-size: 12.5px;
    outline: 0;
  }

  .own:focus {
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--accent) 45%, transparent);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .grow {
    flex: 1;
  }

  .btn {
    min-height: 30px;
    border: 0;
    border-radius: 8px;
    padding: 0 12px;
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
    color: var(--rb-text);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition:
      background-color 120ms ease,
      scale 120ms ease;
  }

  .btn:active:not(:disabled) {
    scale: 0.96;
  }

  .btn.is-ghost {
    background: transparent;
    color: var(--rb-muted);
  }

  .btn.is-primary {
    background: var(--accent);
    color: var(--rb-on-accent, var(--rb-bg0));
  }

  .btn:disabled {
    opacity: 0.45;
    cursor: default;
  }
</style>
