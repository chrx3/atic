<script lang="ts">
  /**
   * Iconos de herramientas y ajustes — Lucide vía Morphicons.
   */
  import Icon from "$ui/Icon.svelte";
  import { TOOL_ICONS, type AppIconId } from "$lib/icons";

  export type SettingsIconId =
    | "general"
    | "shortcuts"
    | "audio"
    | "summary"
    | "settings"
    | "about";

  export type IconId = AppIconId;

  let {
    id,
    size = 20,
    strokeWidth = 1.75,
  }: {
    id: IconId;
    size?: number;
    strokeWidth?: number;
  } = $props();

  const icon = $derived(TOOL_ICONS[id]);
</script>

<Icon {icon} {size} {strokeWidth} />
