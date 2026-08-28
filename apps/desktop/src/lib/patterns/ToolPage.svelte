<script lang="ts">
  /**
   * El marco de una herramienta: encabezado y cuerpo.
   *
   * A diferencia del `ToolPageShell` que reemplaza, no sabe nada de carpetas de
   * datos ni de abrir el explorador. Eso es dominio y sube a la feature; acá
   * entra por el snippet `actions` como cualquier otra acción.
   *
   * `embedded` (o chrome `modal` por contexto) oculta título/blurb/icono cuando
   * el modal padre ya los muestra. Meta y acciones siguen disponibles en una
   * franja compacta.
   */
  import type { Snippet } from "svelte";
  import type { ToolId } from "$core/tools";
  import ToolIcon from "$lib/ToolIcon.svelte";
  import { useToolPageChrome } from "./toolPageChrome";

  let {
    title,
    blurb,
    kicker,
    meta,
    actions,
    icon,
    embedded = false,
    children,
  }: {
    title: string;
    /** Una línea que explica para qué sirve. Se lee una vez y no molesta más. */
    blurb?: string;
    /** Etiqueta chica encima del título. */
    kicker?: string;
    /** Contadores, estado: lo que describe a la herramienta ahora mismo. */
    meta?: Snippet;
    actions?: Snippet;
    /** Icono de la herramienta, a la izquierda del título. */
    icon?: ToolId;
    /**
     * Sin título/blurb/icono propios. También activo si el ancestro provee
     * chrome `modal` (p.ej. ToolWorkspace).
     */
    embedded?: boolean;
    children: Snippet;
  } = $props();

  const chrome = useToolPageChrome();
  const hideIdentity = $derived(embedded || chrome === "modal");
  const showMetaBar = $derived(hideIdentity && Boolean(meta || actions));
  const showFullHeader = $derived(!hideIdentity);
</script>

<div class="flex h-full min-h-0 flex-col overflow-hidden">
  {#if showFullHeader}
    <header class="flex shrink-0 items-start gap-3 border-b border-line px-4 pt-3 pb-3">
      {#if icon}
        <div
          class="flex size-9 shrink-0 items-center justify-center rounded-md
                 bg-surface-2 text-muted"
        >
          <ToolIcon id={icon} size={18} />
        </div>
      {/if}

      <div class="flex min-w-0 flex-1 flex-col gap-0.5">
        {#if kicker}
          <p class="text-micro text-muted uppercase">{kicker}</p>
        {/if}
        <h2 class="truncate text-lg font-semibold text-text">{title}</h2>
        {#if blurb}
          <p class="text-xs text-muted">{blurb}</p>
        {/if}
        {#if meta}
          <div class="mt-1 flex flex-wrap items-center gap-1.5">{@render meta()}</div>
        {/if}
      </div>

      {#if actions}
        <div class="flex shrink-0 items-center gap-1.5">{@render actions()}</div>
      {/if}
    </header>
  {:else if showMetaBar}
    <div
      class="flex shrink-0 items-center gap-2 border-b border-line px-3 py-1"
      aria-label={title}
    >
      {#if meta}
        <div class="flex min-w-0 flex-1 flex-wrap items-center gap-1.5">
          {@render meta()}
        </div>
      {/if}
      {#if actions}
        <div class="ml-auto flex shrink-0 items-center gap-1.5">
          {@render actions()}
        </div>
      {/if}
    </div>
  {/if}

  <div class="flex min-h-0 flex-1 flex-col overflow-hidden">
    {@render children()}
  </div>
</div>
