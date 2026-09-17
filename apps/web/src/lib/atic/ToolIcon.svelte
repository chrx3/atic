<script lang="ts">
  /**
   * Iconos de herramientas — mismos datos que `ToolIcon.svelte` de la app.
   */
  import Icon from "./Icon.svelte";
  import { TOOL_ICONS, type AppIconId } from "./icons";

  let {
    id,
    size = 20,
    strokeWidth = 1.75,
  }: {
    id: AppIconId;
    size?: number;
    strokeWidth?: number;
  } = $props();
</script>

<Icon icon={TOOL_ICONS[id]} {size} {strokeWidth} />
