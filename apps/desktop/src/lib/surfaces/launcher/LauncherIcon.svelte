<script lang="ts">
  /**
   * El icono de cada resultado del lanzador.
   *
   * Un solo componente con un `switch` en vez de ocho archivos: son glifos de
   * 16px que no se usan en ningún otro lado, y tenerlos juntos hace evidente
   * que la lista de acciones y la de iconos tienen que coincidir.
   */
  let { id, kind }: { id: string; kind: string } = $props();

  const which = $derived(
    id.startsWith("action:")
      ? id.slice("action:".length)
      : kind === "app"
        ? "app"
        : "search",
  );
</script>

<svg
  width="16"
  height="16"
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.7"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if which === "dictation"}
    <rect x="9" y="3" width="6" height="11" rx="3" />
    <path d="M5 11a7 7 0 0 0 14 0M12 18v3" />
  {:else if which === "capture"}
    <path d="M7 3v14a1 1 0 0 0 1 1h14M3 7h14a1 1 0 0 1 1 1v14" />
  {:else if which === "clipboard"}
    <rect x="5" y="4" width="14" height="17" rx="2" />
    <path d="M9 4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2H9z" />
  {:else if which === "snippets"}
    <path d="M6 3h8l4 4v14H6z" />
    <path d="M14 3v4h4M9 12h6M9 16h4" />
  {:else if which === "agents"}
    <path d="m8 8-4 4 4 4M16 8l4 4-4 4M13 5l-2 14" />
  {:else if which === "settings"}
    <circle cx="12" cy="12" r="3" />
    <path
      d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6 7 7M17 17l1.4 1.4M18.4 5.6 17 7M7 17l-1.4 1.4"
    />
  {:else if which === "app"}
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M3 9h18" />
  {:else}
    <circle cx="11" cy="11" r="6" />
    <path d="m20 20-3.5-3.5" />
  {/if}
</svg>
