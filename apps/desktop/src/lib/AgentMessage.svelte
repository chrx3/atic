<script lang="ts">
  /**
   * El texto del agente, con su markdown puesto.
   *
   * Sin esto la respuesta era un párrafo plano con asteriscos y comillas
   * invertidas a la vista — el motivo principal de que la consola se viera
   * cruda. Los bloques vienen ya tipados de `agentMarkdown`, así que acá no se
   * inserta HTML de nadie: cada trozo se pinta con su etiqueta.
   */
  import { parse } from "$lib/agentMarkdown";

  let { text }: { text: string } = $props();

  const blocks = $derived(parse(text));
</script>

<div class="md">
  {#each blocks as b, i (i)}
    {#if b.kind === "code"}
      <pre class="md-code"><code>{b.text}</code></pre>
    {:else if b.kind === "hr"}
      <hr class="md-hr" />
    {:else if b.kind === "h"}
      <p class="md-h" data-level={b.level}>
        {#each b.spans as s, j (j)}
          {#if s.kind === "code"}<code class="md-tick">{s.text}</code
            >{:else if s.kind === "strong"}<strong>{s.text}</strong
            >{:else}{s.text}{/if}
        {/each}
      </p>
    {:else if b.kind === "li"}
      <p class="md-li">
        <span class="md-marker">{b.marker}</span>
        <span>
          {#each b.spans as s, j (j)}
            {#if s.kind === "code"}<code class="md-tick">{s.text}</code
              >{:else if s.kind === "strong"}<strong>{s.text}</strong
              >{:else}{s.text}{/if}
          {/each}
        </span>
      </p>
    {:else}
      <p class="md-p">
        {#each b.spans as s, j (j)}
          {#if s.kind === "code"}<code class="md-tick">{s.text}</code
            >{:else if s.kind === "strong"}<strong>{s.text}</strong
            >{:else}{s.text}{/if}
        {/each}
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
  }

  .md-p,
  .md-li,
  .md-h {
    margin: 0;
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
    background: #16130f;
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
</style>
