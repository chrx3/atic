<script lang="ts">
  /**
   * El lanzador: atajo global, escribir, Enter.
   *
   * Se cierra al perder el foco. Apareció encima de lo que estabas haciendo, así
   * que en cuanto mirás a otro lado ya cumplió o ya no hace falta; obligar a
   * cerrarlo a mano sería pedir un paso de más por algo que dura dos segundos.
   *
   * La versión anterior era vidrio —`backdrop-filter: blur(18px)` y un brillo
   * blanco en el borde—. Acá es una superficie opaca: la dirección visual es
   * papel y tinta, y encima el desenfoque de fondo obliga al compositor a
   * releer lo que hay debajo en cada cuadro de una ventana que aparece y
   * desaparece todo el tiempo.
   */
  import type { LauncherHit } from "$core/types";
  import {
    hideLauncher,
    launcherRun,
    launcherSearch,
    onLauncherOpened,
  } from "$ipc/search";
  import { onWindowFocus } from "$ipc/windows";
  import Kbd from "$ui/Kbd.svelte";
  import { tick } from "svelte";
  import LauncherIcon from "./LauncherIcon.svelte";

  let query = $state("");
  let hits = $state<LauncherHit[]>([]);
  let selected = $state(0);
  let searching = $state(false);
  let error = $state("");
  let input = $state<HTMLInputElement | null>(null);

  /**
   * Cada búsqueda lleva número.
   *
   * Se consulta en cada tecla y las respuestas pueden volver desordenadas: sin
   * comparar contra el número actual, «not» podría pisar los resultados de
   * «notepad».
   */
  let generation = 0;

  async function search(text: string) {
    const mine = ++generation;
    searching = true;
    error = "";
    try {
      const next = await launcherSearch(text);
      if (mine !== generation) return;
      hits = next;
      selected = 0;
    } catch (failure) {
      if (mine !== generation) return;
      error = failure instanceof Error ? failure.message : String(failure);
      hits = [];
    } finally {
      if (mine === generation) searching = false;
    }
  }

  /** Vuelve al estado inicial: es lo que se espera al reabrirlo. */
  async function reset(select = false) {
    query = "";
    await search("");
    await tick();
    input?.focus();
    if (select) input?.select();
  }

  async function run() {
    const hit = hits[selected];
    if (!hit) return;
    try {
      await launcherRun(hit.id);
    } catch (failure) {
      error = failure instanceof Error ? failure.message : String(failure);
    }
  }

  $effect(() => {
    const pending = Promise.all([
      onLauncherOpened(() => void reset(true)),
      onWindowFocus((focused) => {
        if (!focused) void hideLauncher();
      }),
    ]);
    void reset(true);
    return () => void pending.then((offs) => offs.forEach((off) => off()));
  });

  async function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      await hideLauncher();
    } else if (event.key === "ArrowDown" && hits.length > 0) {
      event.preventDefault();
      selected = (selected + 1) % hits.length;
    } else if (event.key === "ArrowUp" && hits.length > 0) {
      event.preventDefault();
      selected = (selected - 1 + hits.length) % hits.length;
    } else if (event.key === "Enter") {
      event.preventDefault();
      await run();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="h-screen w-screen p-3" role="dialog" aria-label="Buscar">
  <div
    class="flex h-full flex-col overflow-hidden rounded-lg border border-line
           bg-elevated text-text shadow-float"
  >
    <div class="flex items-center gap-2.5 border-b border-line px-4 py-3">
      <span class="shrink-0 text-muted" aria-hidden="true">
        <LauncherIcon id="" kind="" />
      </span>

      <input
        bind:this={input}
        bind:value={query}
        oninput={() => void search(query)}
        type="text"
        placeholder="Buscar apps y acciones…"
        aria-label="Buscar apps y acciones"
        autocomplete="off"
        spellcheck="false"
        class="min-w-0 flex-1 bg-transparent text-lg text-text outline-none
               placeholder:text-faint"
      />

      {#if searching}
        <span class="font-mono text-xs text-faint" data-numeric aria-hidden="true"
          >…</span
        >
      {:else if query}
        <button
          type="button"
          class="grid size-6 shrink-0 place-items-center rounded-xs bg-surface-2 text-muted
                 transition-colors duration-(--duration-quick) ease-calm hover:text-text"
          aria-label="Limpiar la búsqueda"
          onclick={() => void reset()}
        >
          <svg
            width="12"
            height="12"
            viewBox="0 0 24 24"
            fill="none"
            aria-hidden="true"
          >
            <path
              d="M6 6l12 12M18 6L6 18"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
            />
          </svg>
        </button>
      {/if}
    </div>

    {#if error}
      <p class="bg-danger-soft px-4 py-2 text-xs text-danger" role="alert">{error}</p>
    {/if}

    <ul
      class="flex-1 list-none overflow-auto p-2"
      role="listbox"
      aria-label="Resultados"
    >
      {#each hits as hit, i (hit.id)}
        <li>
          <button
            type="button"
            role="option"
            aria-selected={i === selected}
            class="flex w-full items-center gap-3 rounded-sm px-3 py-2 text-left
                   transition-colors duration-(--duration-quick) ease-calm
                   active:scale-[0.99]
                   {i === selected ? 'bg-surface-2' : ''}"
            onmouseenter={() => (selected = i)}
            onclick={() => {
              selected = i;
              void run();
            }}
          >
            <!-- Las acciones de Atic se distinguen de las apps del sistema por
                 el color del cuadrito: son lo único que el lanzador hace por sí
                 mismo en vez de delegarlo. -->
            <span
              class="grid size-8 shrink-0 place-items-center rounded-sm
                     {hit.kind === 'action'
                ? 'bg-ok-soft text-ok'
                : 'bg-surface-2 text-muted'}"
              aria-hidden="true"
            >
              <LauncherIcon id={hit.id} kind={hit.kind} />
            </span>

            <span class="flex min-w-0 flex-col gap-px">
              <span class="truncate text-md font-semibold">{hit.title}</span>
              <span class="truncate text-xs text-muted">{hit.subtitle}</span>
            </span>
          </button>
        </li>
      {:else}
        <li
          class="flex flex-col items-center gap-2 px-3 py-9 text-center text-sm text-faint"
        >
          <span
            class="grid size-10 place-items-center rounded-sm bg-surface-2"
            aria-hidden="true"
          >
            <LauncherIcon id="" kind="" />
          </span>
          Sin resultados
        </li>
      {/each}
    </ul>

    <footer
      class="flex gap-3.5 border-t border-line px-4 pt-2 pb-2.5 text-micro text-faint"
    >
      <span class="inline-flex items-center gap-1.5"><Kbd combo="↑+↓" /> navegar</span>
      <span class="inline-flex items-center gap-1.5"><Kbd combo="Enter" /> abrir</span>
      <span class="inline-flex items-center gap-1.5"><Kbd combo="Esc" /> cerrar</span>
    </footer>
  </div>
</div>

<style>
  /* Ventana sin marco y transparente: el panel de adentro es todo lo que se ve,
     y el margen de 12px es lo que deja respirar a su sombra. */
  :global(html),
  :global(body) {
    overflow: hidden;
    margin: 0;
    background: transparent;
  }
</style>
