<script lang="ts">
  /**
   * La única entrada de texto de la pizarra. No es de ninguna consola: le
   * escribe a la que está enfocada, y dice a cuál para que no haya sorpresas.
   * Enter manda; Shift+Enter baja de línea.
   *
   * Recuerda lo mandado, como una shell: flecha arriba desde el principio
   * del texto trae lo anterior, flecha abajo desde el final vuelve. Lo que se
   * estaba escribiendo no se pierde al recorrer el historial.
   */
  import { t } from "$domain/i18n.svelte";
  import Icon from "$ui/Icon.svelte";
  import { ArrowUp } from "$lib/icons";
  import AgentLogo from "./AgentLogo.svelte";
  import { pushHistory } from "./agentBoard";

  const HISTORY_KEY = "atic.agents.board.history";

  let {
    target,
    onSend,
    onFocusChange,
  }: {
    /** La consola que recibe; `null` si no hay ninguna a quien escribirle. */
    target: { label: string; cli: string | null } | null;
    onSend: (text: string) => void;
    onFocusChange: (focused: boolean) => void;
  } = $props();

  let draft = $state("");
  let field = $state<HTMLTextAreaElement | null>(null);
  let history = $state<string[]>(readHistory());
  /** Posición al recorrer el historial; `null` = escribiendo lo propio. */
  let browsing = $state<number | null>(null);
  /** Lo que se estaba escribiendo antes de subir por el historial. */
  let stash = "";
  const canSend = $derived(!!target && draft.trim().length > 0);

  /**
   * Inserta donde está el cursor (o al final) y deja el foco en la entrada:
   * lo pegado o soltado desde el historial del portapapeles cae acá.
   */
  export function insertText(text: string) {
    const at =
      field && document.activeElement === field ? field.selectionStart : draft.length;
    const end =
      field && document.activeElement === field ? field.selectionEnd : draft.length;
    draft = draft.slice(0, at) + text + draft.slice(end);
    browsing = null;
    const caret = at + text.length;
    requestAnimationFrame(() => {
      field?.focus();
      field?.setSelectionRange(caret, caret);
    });
  }

  function readHistory(): string[] {
    try {
      const raw = JSON.parse(localStorage.getItem(HISTORY_KEY) ?? "[]");
      return Array.isArray(raw) ? raw.filter((x) => typeof x === "string") : [];
    } catch {
      return [];
    }
  }

  function send() {
    if (!canSend) return;
    onSend(draft);
    history = pushHistory(history, draft);
    try {
      localStorage.setItem(HISTORY_KEY, JSON.stringify(history));
    } catch {
      /* queda en memoria */
    }
    draft = "";
    browsing = null;
  }

  /** `-1` = más viejo, `1` = más nuevo. Devuelve si se movió. */
  function step(dir: -1 | 1): boolean {
    if (history.length === 0) return false;
    if (browsing === null) {
      if (dir === 1) return false;
      stash = draft;
      browsing = history.length - 1;
    } else {
      const next = browsing + dir;
      if (next < 0) return true;
      if (next >= history.length) {
        browsing = null;
        draft = stash;
        return true;
      }
      browsing = next;
    }
    draft = history[browsing];
    return true;
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      send();
      return;
    }
    const field = event.currentTarget as HTMLTextAreaElement;
    // Solo desde el borde: en un texto de varias líneas, las flechas mueven
    // el cursor como siempre. Recorriendo uno de una línea, siguen navegando.
    const oneLine = browsing !== null && !field.value.includes("\n");
    const atStart = oneLine || (field.selectionStart === 0 && field.selectionEnd === 0);
    const atEnd = oneLine || field.selectionStart === field.value.length;
    if (event.key === "ArrowUp" && atStart && step(-1)) event.preventDefault();
    else if (event.key === "ArrowDown" && atEnd && browsing !== null && step(1))
      event.preventDefault();
  }
</script>

<form
  class="composer"
  class:is-idle={!target}
  onsubmit={(event) => {
    event.preventDefault();
    send();
  }}
>
  {#if target}
    <span class="target" title={target.label}>
      <AgentLogo agent={target.cli} size={13} />
      <span class="target-name">{target.label}</span>
    </span>
  {/if}
  <textarea
    bind:this={field}
    bind:value={draft}
    rows="1"
    disabled={!target}
    placeholder={target
      ? t("page.agents.board.placeholder", { name: target.label })
      : t("page.agents.board.noTarget")}
    onkeydown={onKeydown}
    oninput={() => (browsing = null)}
    onfocus={() => onFocusChange(true)}
    onblur={() => onFocusChange(false)}></textarea>
  <button
    type="submit"
    class="send"
    aria-label={t("page.agents.chat.send")}
    disabled={!canSend}
  >
    <Icon icon={ArrowUp} size={15} />
  </button>
</form>

<style>
  .composer {
    display: flex;
    align-items: flex-end;
    gap: 8px;
    width: min(420px, calc(100% - 32px));
    border-radius: 18px;
    padding: 8px 8px 8px 10px;
    background: color-mix(in sRGB, var(--rb-surface) 82%, transparent);
    box-shadow:
      0 0 0 1px color-mix(in sRGB, var(--rb-text) 11%, transparent),
      0 18px 40px -16px rgb(0 0 0 / 55%);
    backdrop-filter: blur(18px) saturate(1.2);
    transition:
      box-shadow var(--duration-fast) ease,
      width var(--duration-medium) var(--ease-smooth-out);
  }

  /* En reposo y vacía ocupa poco; al escribir se abre a su ancho completo. */
  .composer:focus-within,
  .composer:has(textarea:not(:placeholder-shown)) {
    width: min(680px, calc(100% - 32px));
  }

  .composer:focus-within {
    box-shadow:
      0 0 0 1px color-mix(in sRGB, var(--accent) 55%, transparent),
      0 18px 40px -16px rgb(0 0 0 / 55%);
  }

  .target {
    display: inline-flex;
    flex-shrink: 0;
    align-items: center;
    gap: 6px;
    max-width: 180px;
    height: 28px;
    border-radius: 999px;
    padding: 0 10px 0 8px;
    background: color-mix(in sRGB, var(--rb-text) 7%, transparent);
    color: var(--rb-text);
    font-size: 12px;
    font-weight: 560;
  }

  .target-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  textarea {
    flex: 1;
    min-width: 0;
    max-height: 160px;
    field-sizing: content;
    min-height: 28px;
    resize: none;
    border: 0;
    outline: 0;
    padding: 4px 0;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 13.5px;
    line-height: 1.45;
  }

  textarea::placeholder {
    color: var(--rb-faint);
  }

  .send {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 30px;
    height: 30px;
    border: 0;
    border-radius: 50%;
    background: var(--accent);
    color: var(--rb-on-accent, var(--rb-bg0));
    cursor: pointer;
    transition:
      background-color var(--duration-fast) ease,
      scale var(--duration-fast) ease;
  }

  .send:active:not(:disabled) {
    scale: 0.94;
  }

  .send:disabled {
    background: color-mix(in sRGB, var(--rb-text) 12%, transparent);
    color: var(--rb-muted);
    cursor: default;
  }
</style>
