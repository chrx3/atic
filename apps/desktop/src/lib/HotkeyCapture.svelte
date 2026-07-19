<script lang="ts">
  /** Captura de atajo por teclas (no textbox). */

  let {
    value = "CmdOrCtrl+Shift+R",
    defaultValue = "CmdOrCtrl+Shift+R",
    compact = false,
    ariaLabel = "Cambiar atajo",
    onChange,
  }: {
    value?: string;
    defaultValue?: string;
    compact?: boolean;
    ariaLabel?: string;
    onChange: (shortcut: string) => void | Promise<void>;
  } = $props();

  let capturing = $state(false);

  function parts(raw: string): string[] {
    return raw
      .replace(/CmdOrCtrl/gi, "Ctrl")
      .replace(/CommandOrControl/gi, "Ctrl")
      .split("+")
      .map((p) => p.trim())
      .filter(Boolean);
  }

  function keyEventToShortcut(e: KeyboardEvent): string | null {
    if (["Control", "Shift", "Alt", "Meta", "OS"].includes(e.key)) return null;
    const out: string[] = [];
    if (e.ctrlKey || e.metaKey) out.push("CmdOrCtrl");
    if (e.altKey) out.push("Alt");
    if (e.shiftKey) out.push("Shift");

    let key = e.key;
    if (key === " ") key = "Space";
    else if (key.length === 1) key = key.toUpperCase();
    else if (/^F\d{1,2}$/i.test(key)) key = key.toUpperCase();

    if (out.length === 0 && !/^F\d{1,2}$/i.test(key)) return null;
    out.push(key);
    return out.join("+");
  }

  function onKey(e: KeyboardEvent) {
    if (!capturing) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      capturing = false;
      return;
    }
    const sc = keyEventToShortcut(e);
    if (!sc) return;
    capturing = false;
    void onChange(sc);
  }

  function startCapture() {
    capturing = true;
  }

  function cancel() {
    capturing = false;
  }

  $effect(() => {
    if (!capturing) return;
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  const keys = $derived(parts(value));
</script>

<div class="flex flex-wrap items-center gap-1.5" class:gap-2={!compact}>
  <button
    type="button"
    class="rb-hotkey {capturing ? 'is-capturing' : ''} {compact ? 'is-compact' : ''}"
    onclick={() => (capturing ? cancel() : startCapture())}
    aria-label={ariaLabel}
  >
    {#if capturing}
      <span class="rb-hotkey-pulse">{compact ? "…" : "Pulsa la combinación…"}</span>
      {#if !compact}
        <span class="rb-hint !text-[10px]">Esc cancela</span>
      {/if}
    {:else}
      <span class="rb-kbd inline-flex">
        {#each keys as key, i (i)}
          {#if i > 0}<span class="opacity-40">+</span>{/if}
          <kbd>{key}</kbd>
        {/each}
      </span>
      {#if !compact}
        <span class="rb-hint !text-[10px]">Clic para cambiar</span>
      {/if}
    {/if}
  </button>
  {#if value !== defaultValue && !capturing && !compact}
    <button
      type="button"
      class="rb-btn rb-btn-ghost !text-xs"
      onclick={() => onChange(defaultValue)}
    >
      Restablecer
    </button>
  {/if}
</div>
