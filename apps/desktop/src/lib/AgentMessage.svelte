<script lang="ts">
  /**
   * El texto del agente, con su markdown puesto.
   *
   * Sin esto la respuesta era un párrafo plano con asteriscos y comillas
   * invertidas a la vista — el motivo principal de que la consola se viera
   * cruda. Los bloques vienen ya tipados de `agentMarkdown`, así que acá no se
   * inserta HTML de nadie: cada trozo se pinta con su etiqueta.
   */
  import { parse, type Inline } from "$lib/agentMarkdown";

  let { text }: { text: string } = $props();

  const blocks = $derived(parse(text));
</script>

{#snippet spans(items: Inline[])}
  {#each items as s, j (j)}
    {#if s.kind === "code"}<code class="md-tick">{s.text}</code
    >{:else if s.kind === "strong"}<strong>{s.text}</strong
    >{:else}{s.text}{/if}
  {/each}
{/snippet}

<div class="md">
  {#each blocks as b, i (i)}
    {#if b.kind === "code"}
      <pre class="md-code"><code>{b.text}</code></pre>
    {:else if b.kind === "hr"}
      <hr class="md-hr" />
    {:else if b.kind === "h"}
      <p class="md-h" data-level={b.level}>
        {@render spans(b.spans)}
      </p>
    {:else if b.kind === "li"}
      <p class="md-li">
        <span class="md-marker">{b.marker}</span>
        <span>
          {@render spans(b.spans)}
        </span>
      </p>
    {:else if b.kind === "table"}
      <div class="md-table-wrap">
        <table class="md-table">
          <thead>
            <tr>
              {#each b.headers as cell, ci (ci)}
                <th>{@render spans(cell)}</th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each b.rows as row, ri (ri)}
              <tr>
                {#each row as cell, ci (ci)}
                  <td>{@render spans(cell)}</td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else}
      <p class="md-p">
        {@render spans(b.spans)}
      </p>
    {/if}
  {/each}
</div>

<style>
  .md {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    font-size: 0.8125rem;
    line-height: 1.62;
    user-select: text;
    -webkit-user-select: text;
    cursor: text;
  }

  .md-p,
  .md-li,
  .md-h {
    margin: 0;
  }

  /* Los saltos de línea se respetan: la salida de `/usage` y compañía viene
     alineada por líneas, y colapsarlas la vuelve ilegible. */
  .md-p {
    white-space: pre-wrap;
  }

  .md-h {
    color: var(--text);
    font-weight: 650;
  }
  .md-h[data-level="1"] {
    font-size: 0.9375rem;
  }

  .md-li {
    display: flex;
    gap: 0.5rem;
    padding-left: 0.3rem;
  }

  .md-marker {
    flex-shrink: 0;
    color: var(--coral);
    font-variant-numeric: tabular-nums;
  }

  /* El código en línea no lleva fondo de caja: dentro de una consola ya todo
     es monoespaciado, así que basta el color para separarlo de la prosa. */
  .md-tick {
    color: var(--coral);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.9em;
  }

  .md-code {
    max-height: 16rem;
    margin: 0.15rem 0;
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 0.55rem 0.7rem;
    background: var(--code);
    color: var(--text);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.75rem;
    line-height: 1.55;
    overflow: auto;
  }

  .md-hr {
    margin: 0.2rem 0;
    border: 0;
    border-top: 1px solid var(--line);
  }

  .md-table-wrap {
    max-width: 100%;
    margin: 0.1rem 0;
    overflow-x: auto;
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--code);
  }

  .md-table {
    width: max-content;
    min-width: 100%;
    border-collapse: collapse;
    font-size: 0.75rem;
    line-height: 1.45;
  }

  .md-table th,
  .md-table td {
    padding: 0.35rem 0.55rem;
    border-bottom: 1px solid var(--line);
    border-right: 1px solid color-mix(in srgb, var(--line) 70%, transparent);
    text-align: left;
    vertical-align: top;
    white-space: nowrap;
  }

  .md-table th:last-child,
  .md-table td:last-child {
    border-right: 0;
  }

  .md-table thead th {
    color: var(--text);
    font-weight: 650;
    background: color-mix(in srgb, var(--coral) 10%, transparent);
  }

  .md-table tbody tr:last-child td {
    border-bottom: 0;
  }

  .md-table td {
    color: var(--dim);
  }
</style>
