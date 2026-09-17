<script lang="ts">
  /**
   * La consola de agentes: Claude Code, Codex, OpenCode y Cursor en pestañas.
   *
   * La conversación está guionizada —es un demo— pero el ritmo es el real:
   * el texto llega a pedazos y las herramientas aparecen mientras corren y
   * se asientan con su resultado. Atic no autentica agentes: se cuelga del CLI
   * que ya tienes; eso también se dice acá.
   */
  import Float from "../Float.svelte";
  import Icon from "$lib/atic/Icon.svelte";
  import { tick } from "svelte";
  import { ArrowRight, FileText, Search, SquareTerminal, Sparkles } from "$lib/atic/icons";
  import { AGENTS, AGENT_SCRIPTS, type AgentId, type AgentStep } from "../data";
  import { demo, type AgentMsg } from "../state.svelte";

  let { onClose }: { onClose?: () => void } = $props();

  let agent = $state<AgentId>("claude");
  let prompt = $state("");
  let scroller = $state<HTMLDivElement | null>(null);

  const def = $derived(AGENTS.find((item) => item.id === agent) ?? AGENTS[0]);
  const thread = $derived(demo.agentThreads[agent] ?? []);
  const running = $derived(demo.agentRunning[agent] ?? false);

  let seq = 0;

  /** El hilo se busca por id, no por `agent`: si el usuario cambia de pestaña
   *  mientras corre, la respuesta tiene que seguir cayendo en su hilo.
   *
   *  Devuelve el mensaje YA proxeado (leyéndolo de vuelta del array): mutar
   *  el objeto crudo que se acaba de empujar no dispara nada, y el stream se
   *  quedaría congelado en el primer cuadro. */
  function pushTo(thread: AgentMsg[], message: Omit<AgentMsg, "id">): AgentMsg {
    thread.push({ id: ++seq, ...message });
    return thread[thread.length - 1];
  }

  const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

  /** Baja el hilo después de que el DOM aplique lo último que llegó: sin
   *  `tick`, el scroll calcula sobre la altura vieja y se queda a mitad. */
  function scrollEnd() {
    void tick().then(() => {
      requestAnimationFrame(() => {
        if (scroller) scroller.scrollTop = scroller.scrollHeight;
      });
    });
  }

  async function runSteps(thread: AgentMsg[], steps: AgentStep[]) {
    for (const step of steps) {
      if (step.role === "user") continue;

      if (step.role === "text") {
        const message = pushTo(thread, { kind: "text", text: "" });
        // A pedazos, con una pausa más larga después de punto: el mismo ritmo
        // de lectura que un stream real.
        for (let i = 0; i < step.text.length; i += 3) {
          message.text += step.text.slice(i, i + 3);
          scrollEnd();
          await sleep(step.text[i] === "." ? 90 : 14);
        }
        await sleep(180);
        continue;
      }

      if (step.role === "tool") {
        const message = pushTo(thread, {
          kind: "tool",
          tool: { ...step.tool, out: null, status: "run" },
        });
        scrollEnd();
        await sleep(680 + Math.random() * 520);
        if (message.tool) {
          message.tool.out = step.tool.out;
          message.tool.status = "ok";
        }
        scrollEnd();
        await sleep(140);
        continue;
      }

      if (step.role === "done") {
        // El turno termina sin resumen de costo: en la app eso vive en el
        // modal Uso, no en el hilo.
        pushTo(thread, { kind: "done" });
        scrollEnd();
      }
    }
  }

  async function send(text: string) {
    const id = agent;
    const clean = text.trim();
    if (!clean || demo.agentRunning[id]) return;
    if (!demo.agentThreads[id]) demo.agentThreads[id] = [];
    const thread = demo.agentThreads[id];
    demo.agentRunning[id] = true;
    prompt = "";
    pushTo(thread, { kind: "user", text: clean });
    scrollEnd();
    await sleep(420);
    await runSteps(thread, AGENT_SCRIPTS[id].steps);
    demo.agentRunning[id] = false;
    scrollEnd();
  }
</script>

<Float title="Agentes" icon="agents" wide tall onClose={onClose}>
  <div class="console">
    <div class="tabs" role="tablist" aria-label="Agentes">
      {#each AGENTS as item (item.id)}
        <button
          type="button"
          role="tab"
          class="tab"
          class:is-active={item.id === agent}
          aria-selected={item.id === agent}
          onclick={() => (agent = item.id)}
        >
          <span class="tab-mark" data-agent={item.id}>{item.mark}</span>
          {item.name}
          {#if demo.agentRunning[item.id]}
            <span class="tab-dot" aria-label="Trabajando"></span>
          {/if}
        </button>
      {/each}
    </div>

    <div class="thread" bind:this={scroller}>
      <div class="where">
        <span>{def.cwd}</span>
        <span class="sep">·</span>
        <span>CLI del sistema, sin cuenta Atic</span>
      </div>

      {#if thread.length === 0}
        <div class="empty">
          <p class="empty-title">{def.name} está listo.</p>
          <button type="button" class="suggestion" onclick={() => send(AGENT_SCRIPTS[agent].suggestion)}>
            <Icon icon={Sparkles} size={14} />
            {AGENT_SCRIPTS[agent].suggestion}
          </button>
          <p class="empty-note">Atajo del demo con un hilo de ejemplo guionizado.</p>
        </div>
      {/if}

      {#each thread as message (message.id)}
        {#if message.kind === "user"}
          <div class="msg is-user">
            <p>{message.text}</p>
          </div>
        {:else if message.kind === "text"}
          <div class="msg is-agent">
            <p>{message.text}{#if running && message.id === thread[thread.length - 1]?.id}<span class="caret"></span>{/if}</p>
          </div>
        {:else if message.kind === "tool" && message.tool}
          <div class="tool" class:is-run={message.tool.status === "run"}>
            <div class="tool-head">
              <span class="tool-icon">
                <Icon icon={message.tool.name === "Bash" ? SquareTerminal : message.tool.name === "Grep" ? Search : FileText} size={13} />
              </span>
              <span class="tool-name">{message.tool.name}</span>
              <span class="tool-arg">{message.tool.arg}</span>
              <span class="tool-out">{message.tool.out ?? "corriendo…"}</span>
            </div>
            {#if message.tool.status === "ok" && message.tool.lines}
              <pre class="tool-lines">{message.tool.lines.join("\n")}</pre>
            {/if}
          </div>
        {:else if message.kind === "done"}
          <div class="done" aria-hidden="true">
            <span class="done-line"></span>
          </div>
        {/if}
      {/each}
    </div>

    <form
      class="composer"
      onsubmit={(event) => {
        event.preventDefault();
        void send(prompt);
      }}
    >
      <input
        bind:value={prompt}
        placeholder={running ? "Trabajando…" : `Escribe para ${def.name}…`}
        aria-label="Mensaje para el agente"
        disabled={running}
        spellcheck="false"
        autocomplete="off"
      />
      <button type="submit" class="send" disabled={running || !prompt.trim()} aria-label="Enviar">
        <Icon icon={ArrowRight} size={15} />
      </button>
    </form>
  </div>
</Float>

<style>
  .console {
    display: flex;
    height: 100%;
    min-height: 320px;
    flex-direction: column;
  }

  .tabs {
    display: flex;
    flex-shrink: 0;
    flex-wrap: wrap;
    gap: 2px;
    border-bottom: 1px solid var(--line);
    padding: 6px 8px 0;
  }

  .tab {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 0;
    border-bottom: 2px solid transparent;
    border-radius: var(--radius-xs) var(--radius-xs) 0 0;
    padding: 7px 10px 8px;
    background: transparent;
    color: var(--faint);
    font: inherit;
    font-size: var(--text-xs);
    cursor: pointer;
  }

  .tab:hover {
    color: var(--muted);
  }

  .tab.is-active {
    border-bottom-color: var(--accent);
    color: var(--text);
  }

  .tab-mark {
    display: grid;
    width: 18px;
    height: 18px;
    place-items: center;
    border-radius: 5px;
    background: color-mix(in srgb, var(--text) 9%, transparent);
    color: var(--muted);
    font-size: 10px;
    font-weight: var(--font-weight-semibold);
  }

  .tab-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--warn);
    animation: pulse 1.2s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      opacity: 0.3;
    }
  }

  .thread {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 10px;
    overflow: auto;
    padding: 12px 14px;
  }

  .where {
    display: flex;
    gap: 6px;
    color: var(--faint);
    font-size: var(--text-micro);
    letter-spacing: 0.02em;
    font-variant-numeric: tabular-nums;
  }

  .sep {
    opacity: 0.5;
  }

  .empty {
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: flex-start;
    padding: 8px 0;
  }

  .empty-title {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-base);
  }

  .suggestion {
    display: inline-flex;
    align-items: flex-start;
    gap: 8px;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    padding: 8px 11px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--text-sm);
    line-height: 1.45;
    text-align: left;
    cursor: pointer;
  }

  .suggestion:hover {
    border-color: var(--line-strong);
  }

  .empty-note {
    margin: 0;
    color: var(--faint);
    font-size: var(--text-micro);
  }

  .msg {
    max-width: 92%;
    border-radius: var(--radius-sm);
    padding: 8px 11px;
    font-size: var(--text-sm);
    line-height: 1.5;
  }

  .msg p {
    margin: 0;
    white-space: pre-wrap;
  }

  .msg.is-user {
    align-self: flex-end;
    background: color-mix(in srgb, var(--text) 9%, transparent);
    color: var(--text);
  }

  .msg.is-agent {
    align-self: flex-start;
    color: var(--muted);
  }

  .caret {
    display: inline-block;
    width: 2px;
    height: 0.95em;
    margin-left: 2px;
    background: currentColor;
    vertical-align: text-bottom;
    animation: pulse 0.9s steps(2) infinite;
  }

  .tool {
    align-self: stretch;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    padding: 7px 10px;
  }

  .tool.is-run {
    border-color: color-mix(in srgb, var(--warn) 40%, transparent);
  }

  .tool-head {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: var(--text-xs);
  }

  .tool-icon {
    display: grid;
    place-items: center;
    color: var(--faint);
  }

  .tool-name {
    color: var(--text);
    font-weight: var(--font-weight-semibold);
  }

  .tool-arg {
    overflow: hidden;
    flex: 1;
    color: var(--muted);
    font-family: var(--font-mono);
    font-size: 0.6875rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tool-out {
    flex-shrink: 0;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }

  .tool.is-run .tool-out {
    color: var(--warn);
  }

  .tool-lines {
    margin: 7px 0 1px;
    padding: 7px 9px;
    border-radius: var(--radius-xs);
    background: color-mix(in srgb, var(--bg) 60%, transparent);
    color: var(--muted);
    font-family: var(--font-mono);
    font-size: 0.65625rem;
    line-height: 1.6;
    overflow-x: auto;
    white-space: pre;
  }

  .done {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--faint);
    font-size: var(--text-micro);
    font-variant-numeric: tabular-nums;
  }

  .done-line {
    height: 1px;
    flex: 1;
    background: var(--line);
  }

  .composer {
    display: flex;
    flex-shrink: 0;
    gap: 6px;
    border-top: 1px solid var(--line);
    padding: 8px;
  }

  .composer input {
    min-width: 0;
    flex: 1;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    padding: 8px 10px;
    background: var(--surface-2);
    color: var(--text);
    font: inherit;
    font-size: var(--text-sm);
    outline: none;
  }

  .composer input:focus {
    border-color: var(--line-strong);
  }

  .send {
    display: grid;
    width: 34px;
    place-items: center;
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    border-radius: var(--radius-sm);
    background: var(--accent);
    color: var(--on-accent);
    cursor: pointer;
  }

  .send:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
