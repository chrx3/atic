<script lang="ts">
  /**
   * Icono de cada resultado del lanzador (Lucide).
   */
  import Icon from "$ui/Icon.svelte";
  import { LAUNCHER_ICONS, Search } from "$lib/icons";

  let { id, kind }: { id: string; kind: string } = $props();

  const which = $derived(
    id.startsWith("action:")
      ? id.slice("action:".length)
      : kind === "app"
        ? "app"
        : "search",
  );

  const icon = $derived(LAUNCHER_ICONS[which] ?? Search);
</script>

<Icon {icon} size={16} strokeWidth={1.7} />
