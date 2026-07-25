<script lang="ts">
  /** Waveform de barras que reacciona al nivel RMS (0–1+). */
  import { prefersReducedMotion } from "$lib/motion";

  let {
    level = 0,
    color = "mic",
    bars = 12,
    /** Si se pasan ambos, una sola visualización intercalada mic/sys. */
    mic,
    system,
    /** quiet = monocromo nativo (pill); vivid = mic/sys coloreados;
     *  voice = dictado, con motor propio (ver abajo). */
    variant = "vivid",
    /** Anima por reloj. Solo para `voice`: cuesta un rAF, así que se enciende
     *  únicamente mientras el micrófono está tomando. */
    live = false,
  }: {
    level?: number;
    color?: "mic" | "sys";
    bars?: number;
    mic?: number;
    system?: number;
    variant?: "vivid" | "quiet" | "voice";
    live?: boolean;
  } = $props();

  const mixed = $derived(mic !== undefined && system !== undefined);

  /* ─── Motor de voz ──────────────────────────────────────────────────────
   *
   * El waveform de abajo es una función pura de (barra, amplitud). Sin término
   * temporal, una vocal sostenida lo deja CONGELADO; y cuando el RMS cambia
   * salta, porque tampoco hay transición. Se ve muerto y nervioso a la vez.
   *
   * Acá el nivel se suaviza con ataque rápido y caída lenta, como un vúmetro
   * real: las consonantes entran al instante y la cola se apaga sola. Y la
   * fase ATRASA de izquierda a derecha, así el envión de la voz se ve *viajar*
   * por la tira en lugar de rebotar en el lugar. Hablar es ráfagas y pausas,
   * no un ecualizador de equipo de música.
   */

  /** rad/s del envión que cruza la tira. */
  const TRAVEL_SPEED = 3.4;
  /** Atraso de fase entre la primera barra y la última. Esto es "viajar". */
  const TRAVEL_LAG = 2.4;
  /** rad/s de la respiración en reposo. Lenta: es un latido, no un parpadeo. */
  const IDLE_SPEED = 1.15;
  /** Suavizado por frame: sube rápido, baja lento. */
  const ATTACK = 0.4;
  const RELEASE = 0.075;

  /* El RMS de voz medido en esta app va de ~0.000 a ~0.02: hablando fuerte
   * apenas llega a 0.016. Con una ganancia lineal (el `* 2.8` de antes) eso da
   * una amplitud de 0.05 y la tira queda PLANA por más que grites.
   *
   * El oído es logarítmico, así que la escala también. Piso y techo en dB
   * acotan justo el rango donde vive el habla, y el resultado usa todo el alto
   * disponible en vez del 5% de abajo. */
  const FLOOR_DB = -60;
  const CEIL_DB = -20;

  /** RMS lineal → 0–1 perceptual. */
  function normalize(rms: number): number {
    if (rms <= 0) return 0;
    const db = 20 * Math.log10(rms);
    return Math.min(1, Math.max(0, (db - FLOOR_DB) / (CEIL_DB - FLOOR_DB)));
  }

  let phase = $state(0);
  let smooth = $state(0);

  const raw = $derived(normalize(mic ?? level));
  const animated = $derived(
    variant === "voice" && live && !prefersReducedMotion(),
  );

  $effect(() => {
    if (!animated) return;
    let frame = 0;
    let last = performance.now();
    const tick = (now: number) => {
      // dt acotado: si el SO congela la ventana, no queremos un salto de fase.
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      phase += dt;
      const target = raw;
      smooth += (target - smooth) * (target > smooth ? ATTACK : RELEASE);
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  });

  /** Altura de una barra de voz, en 0–1. */
  function voiceBar(i: number, count: number, amp: number, t: number): number {
    const x = count <= 1 ? 0.5 : i / (count - 1);
    // Campana: los extremos siempre más bajos, la tira respira desde el centro.
    const envelope = 0.42 + 0.58 * Math.sin(Math.PI * x);
    const travel = 0.55 + 0.45 * Math.sin(t * TRAVEL_SPEED - x * TRAVEL_LAG);
    // En reposo nunca se aplana. Una tira plana parece "colgado"; el dictado
    // necesita comunicar "escuchando".
    const idle = 0.09 + 0.045 * Math.sin(t * IDLE_SPEED - x * 1.3);
    return Math.min(1, idle + amp * travel * envelope);
  }

  /** Amplitud que ve el CSS. El color codifica nivel; no decora. */
  const heat = $derived(variant === "voice" ? (animated ? smooth : raw) : 0);

  function barHeight(amp: number, i: number, count: number): number {
    const t = count <= 1 ? 0.5 : i / (count - 1);
    const envelope = 0.35 + 0.65 * Math.sin(Math.PI * t);
    const jitter = 0.55 + 0.45 * Math.abs(Math.sin(i * 1.7 + amp * 8));
    return Math.max(0.14, envelope * jitter * (0.18 + amp * 0.82));
  }

  const items = $derived.by(() => {
    if (variant === "voice") {
      const amp = animated ? smooth : raw;
      const t = animated ? phase : 0;
      return Array.from({ length: bars }, (_, i) => ({
        h: voiceBar(i, bars, amp, t),
        kind: "mic" as const,
      }));
    }
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
  class:rb-wave-voice={variant === "voice"}
  style="--rb-wave-amp: {heat.toFixed(3)}"
  data-tauri-drag-region
  aria-hidden="true"
>
  {#each items as item, i (i)}
    <span
      class="rb-wave-bar"
      class:rb-wave-bar-mic={variant === "vivid" && item.kind === "mic"}
      class:rb-wave-bar-sys={variant === "vivid" && item.kind === "sys"}
      class:rb-wave-bar-quiet={variant === "quiet"}
      class:rb-wave-bar-voice={variant === "voice"}
      data-tauri-drag-region
      style="height: {Math.round(item.h * 100)}%"
    ></span>
  {/each}
</div>
