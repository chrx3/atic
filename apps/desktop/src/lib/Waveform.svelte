<script lang="ts">
  /** Waveform de barras que reacciona al nivel RMS (0–1+). */

  let {
    level = 0,
    color = "mic",
    bars = 12,
    /** Si se pasan ambos, una sola visualización intercalada mic/sys. */
    mic,
    system,
    /** quiet = monocromo nativo (pill); vivid = mic/sys coloreados */
    variant = "vivid",
  }: {
    level?: number;
    color?: "mic" | "sys";
    bars?: number;
    mic?: number;
    system?: number;
    variant?: "vivid" | "quiet";
  } = $props();

  const mixed = $derived(mic !== undefined && system !== undefined);

  function barHeight(amp: number, i: number, count: number): number {
    const t = count <= 1 ? 0.5 : i / (count - 1);
    const envelope = 0.35 + 0.65 * Math.sin(Math.PI * t);
    const jitter = 0.55 + 0.45 * Math.abs(Math.sin(i * 1.7 + amp * 8));
    return Math.max(0.14, envelope * jitter * (0.18 + amp * 0.82));
  }

  const items = $derived.by(() => {
    if (mixed) {
      const micAmp = Math.min(1, Math.max(0, (mic ?? 0) * 2.8));
      const sysAmp = Math.min(1, Math.max(0, (system ?? 0) * 2.8));
      const out: { h: number; kind: "mic" | "sys" }[] = [];
      for (let i = 0; i < bars; i++) {
        const kind = i % 2 === 0 ? "mic" : "sys";
        const amp = kind === "mic" ? micAmp : sysAmp;
        out.push({ h: barHeight(amp, i, bars), kind });
      }
      return out;
    }

    const amp = Math.min(1, Math.max(0, level * 2.8));
    return Array.from({ length: bars }, (_, i) => ({
      h: barHeight(amp, i, bars),
      kind: color as "mic" | "sys",
    }));
  });
</script>

<div
  class="rb-wave"
  class:rb-wave-mic={variant === "vivid" && !mixed && color === "mic"}
  class:rb-wave-sys={variant === "vivid" && !mixed && color === "sys"}
  class:rb-wave-mixed={variant === "vivid" && mixed}
  class:rb-wave-quiet={variant === "quiet"}
  data-tauri-drag-region
  aria-hidden="true"
>
  {#each items as item, i (i)}
    <span
      class="rb-wave-bar"
      class:rb-wave-bar-mic={variant === "vivid" && item.kind === "mic"}
      class:rb-wave-bar-sys={variant === "vivid" && item.kind === "sys"}
      class:rb-wave-bar-quiet={variant === "quiet"}
      data-tauri-drag-region
      style="height: {Math.round(item.h * 100)}%"
    ></span>
  {/each}
</div>
