<script lang="ts">
  /**
   * Una llamada a herramienta, como tarjeta.
   *
   * Antes era una línea suelta y el resultado otra, separadas por lo que
   * hubiera en medio: había que emparejarlas a ojo para saber si algo había
   * fallado. Unidas en una tarjeta con estado, la respuesta a «qué hizo y cómo
   * le fue» se lee de un vistazo, que es el patrón de todas las interfaces de
   * agente que funcionan (Zed, opcode, Claudia).
   *
   * Plegada por defecto: la salida de una herramienta suele ser larga y casi
   * nunca es lo que se está leyendo. Se despliega sola cuando falló, porque ahí
   * sí es lo único que importa.
   */
  import { editDiff } from "$lib/agentMarkdown";
  import type { ToolKind, ToolStatus } from "$lib/types";

  let {
    name,
    title,
    toolKind,
    input,
    output,
    status,
    locations = [],
  }: {
    name: string;
    /** Texto legible. Lo arma el backend; la vista ya no lo deduce. */
    title: string;
    toolKind: ToolKind;
    input: unknown;
    output?: string;
    status: ToolStatus;
    /** Archivos que toca. */
    locations?: string[];
  } = $props();

  let open = $state(false);

  const diff = $derived(editDiff(input));
  const isError = $derived(status === "failed");
  const done = $derived(status === "completed" || status === "failed");
  const isBash = $derived(toolKind === "execute");

  /** Cuánto cambia, para decirlo sin abrir la tarjeta. */
  const counts = $derived.by(() => {
    if (!diff) return null;
    let add = 0;
    let del = 0;
    for (const line of diff) {
      if (line.sign === "+") add += 1;
      else if (line.sign === "-") del += 1;
    }
    return { add, del };
  });

  const body = $derived(
    typeof input === "object" && input !== null
      ? JSON.stringify(input, null, 2)
      : String(input ?? ""),
  );

  // Un fallo se abre solo: es lo único que hace falta leer entero.
  $effect(() => {
    if (isError) open = true;
  });
</script>

<div class="tc" class:is-error={isError} class:is-open={open}>
  <button
    type="button"
    class="tc-head"
    onclick={() => (open = !open)}
    aria-expanded={open}
  >
    <span class="tc-st" class:is-run={!done} class:is-bad={isError}></span>
    <span class="tc-name">{name}</span>
    <span class="tc-arg">{title}</span>

    {#if counts}
      <span class="tc-num">
        <span class="add">+{counts.add}</span>
        <span class="del">−{counts.del}</span>
      </span>
    {/if}
    <span class="tc-caret" aria-hidden="true">{open ? "⌃" : "⌄"}</span>
  </button>

  {#if open}
    <div class="tc-body">
      {#if diff}
        <div class="tc-diff">
          {#each diff as line, i (i)}
            <div class="dl" data-sign={line.sign}>
              <span class="dl-s">{line.sign}</span><span class="dl-t"
                >{line.text}</span
              >
            </div>
          {/each}
        </div>
      {:else if isBash}
        <pre class="tc-cmd">$ {title}</pre>
      {:else}
        <pre class="tc-json">{body}</pre>
      {/if}

      {#if output}
        <pre class="tc-out" class:is-error={isError}>{output}</pre>
      {:else if !done}
        <p class="tc-wait">ejecutando…</p>
      {/if}

      {#if locations.length > 0}
        <div class="tc-loc">
          {#each locations as l (l)}<span>{l}</span>{/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .tc {
    border: 1px solid var(--line);
    border-radius: 9px;
    background: #1f1b19;
    overflow: hidden;
  }
  .tc.is-error {
    border-color: color-mix(in srgb, var(--coral) 55%, var(--line));
  }

  .tc-head {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 0.5rem;
    border: 0;
    padding: 0.42rem 0.6rem;
    background: transparent;
    color: var(--text);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.75rem;
    text-align: left;
    cursor: pointer;
  }
  .tc-head:hover {
    background: #26211e;
  }

  /* Punto de estado: relleno cuando terminó, latiendo mientras corre. Es la
     respuesta a «sigue en eso o ya está» sin leer nada. */
  .tc-st {
    width: 0.4rem;
    height: 0.4rem;
    flex-shrink: 0;
    border-radius: 999px;
    background: var(--dim);
  }
  .tc-st.is-run {
    background: var(--coral);
    animation: tc-pulse 1.4s ease-in-out infinite;
  }
  .tc-st.is-bad {
    background: var(--coral);
    animation: none;
  }

  @keyframes tc-pulse {
    0%,
    100% {
      opacity: 0.3;
    }
    50% {
      opacity: 1;
    }
  }

  .tc-name {
    flex-shrink: 0;
    color: var(--text);
  }

  .tc-arg {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    color: var(--dim);
    text-overflow: ellipsis;
    white-space: nowrap;
    /* La cola es lo informativo de una ruta larga, no la cabeza. */
    direction: rtl;
    text-align: left;
  }

  .tc-num {
    display: flex;
    flex-shrink: 0;
    gap: 0.3rem;
    font-size: 0.6875rem;
  }
  .tc-num .add {
    color: #7dd3a0;
  }
  .tc-num .del {
    color: #e08a7a;
  }

  .tc-caret {
    flex-shrink: 0;
    color: var(--faint);
    font-size: 0.625rem;
  }

  .tc-body {
    border-top: 1px solid var(--line);
    padding: 0.45rem 0.6rem 0.55rem;
  }

  .tc-diff {
    max-height: 15rem;
    border-radius: 6px;
    background: #16130f;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.71875rem;
    line-height: 1.5;
    overflow: auto;
  }

  .dl {
    display: flex;
    gap: 0.5rem;
    padding: 0 0.5rem;
    white-space: pre;
  }
  .dl[data-sign="+"] {
    background: color-mix(in srgb, #7dd3a0 12%, transparent);
  }
  .dl[data-sign="-"] {
    background: color-mix(in srgb, #e08a7a 12%, transparent);
  }

  .dl-s {
    flex-shrink: 0;
    color: var(--faint);
  }
  .dl[data-sign="+"] .dl-s {
    color: #7dd3a0;
  }
  .dl[data-sign="-"] .dl-s {
    color: #e08a7a;
  }

  .dl-t {
    color: var(--text);
  }

  .tc-cmd,
  .tc-json,
  .tc-out {
    max-height: 12rem;
    margin: 0;
    border-radius: 6px;
    padding: 0.4rem 0.5rem;
    background: #16130f;
    color: var(--dim);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.71875rem;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow: auto;
  }

  .tc-cmd {
    color: var(--text);
  }

  .tc-out {
    margin-top: 0.4rem;
  }
  .tc-out.is-error {
    color: #e8a496;
  }

  /* Archivos que la herramienta toca. Llegan del backend en `locations`. */
  .tc-loc {
    display: flex;
    flex-wrap: wrap;
    margin-top: 0.4rem;
    gap: 0.25rem;
  }
  .tc-loc span {
    border-radius: 4px;
    padding: 0.05rem 0.35rem;
    background: #16130f;
    color: var(--faint);
    font-size: 0.625rem;
  }

  .tc-wait {
    margin: 0.4rem 0 0;
    color: var(--faint);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.6875rem;
  }
</style>
