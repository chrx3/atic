<script lang="ts">
  /**
   * Íconos del compositor de agentes (Lucide).
   *
   * El modo de permisos se lee por silueta: check = guardia completa,
   * shield = parcial, minus = plan, off = sin guardia (ámbar).
   */
  import type { ToolKind } from "$lib/types";
  import Icon from "$ui/Icon.svelte";
  import { AGENT_ICONS } from "$lib/icons";

  let {
    name,
    size = 11,
  }: {
    name:
      | "shield-manual"
      | "shield-edits"
      | "shield-plan"
      | "shield-open"
      | "folder"
      | "mic"
      | "clip"
      | "camera"
      | "mcp"
      | "history"
      | ToolKind;
    size?: number;
  } = $props();

  const icon = $derived(
    AGENT_ICONS[name as keyof typeof AGENT_ICONS] ?? AGENT_ICONS.other,
  );
  const color = $derived(name === "shield-open" ? "#d4a24c" : undefined);
</script>

<Icon {icon} {size} strokeWidth={1.8} {color} />
