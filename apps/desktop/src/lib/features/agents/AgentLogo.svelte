<script lang="ts">
  import Icon from "$ui/Icon.svelte";
  import { SquareTerminal } from "$lib/icons";

  let { agent, size = 22 }: { agent?: string | null; size?: number } = $props();

  type LogoDef = { path: string; colored?: boolean };
  const LOGOS: Record<string, LogoDef> = {
    claude: { path: "/agents/claude.svg", colored: true },
    "claude-code": { path: "/agents/claude.svg", colored: true },
    opencode: { path: "/agents/opencode.svg" },
    codex: { path: "/agents/openai.svg" },
    openai: { path: "/agents/openai.svg" },
    cursor: { path: "/agents/cursor.svg" },
    "cursor-agent": { path: "/agents/cursor.svg" },
    gemini: { path: "/agents/gemini.svg" },
    agy: { path: "/agents/antigravity.svg" },
    grok: { path: "/agents/grok.svg" },
    xai: { path: "/agents/grok.svg" },
  };

  const logo = $derived(LOGOS[agent?.trim().toLowerCase() ?? ""] ?? null);
</script>

<span class="agent-logo" style:--agent-logo-size={`${size}px`} aria-hidden="true">
  {#if logo?.colored}
    <img src={logo.path} alt="" draggable="false" />
  {:else if logo}
    <span class="agent-logo-mask" style={`--agent-logo-image: url("${logo.path}")`}
    ></span>
  {:else}
    <Icon icon={SquareTerminal} {size} />
  {/if}
</span>

<style>
  .agent-logo {
    display: grid;
    width: var(--agent-logo-size);
    height: var(--agent-logo-size);
    flex: 0 0 auto;
    place-items: center;
    color: currentColor;
  }

  .agent-logo img,
  .agent-logo-mask {
    display: block;
    width: 100%;
    height: 100%;
  }

  .agent-logo-mask {
    background: currentColor;
    mask: var(--agent-logo-image) center / contain no-repeat;
  }
</style>
