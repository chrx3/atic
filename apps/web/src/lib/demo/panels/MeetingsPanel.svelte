<script lang="ts">
  /**
   * Reuniones, en versión corta: grabar, ver la transcripción caer en vivo y
   * parar en un resumen listo para mandar.
   *
   * Es el corazón de la app y por eso está en el demo, aunque sea guionizado:
   * el audio real no sale del equipo en este demo — igual que en la app, donde
   * Whisper transcribe en local.
   */
  import Float from "../Float.svelte";
  import Icon from "$lib/atic/Icon.svelte";
  import { Check, Copy, Mic, Pause, Play, Square } from "$lib/atic/icons";
  import { copyText, demo } from "../state.svelte";

  let { onClose }: { onClose?: () => void } = $props();

  const CAPTIONS = [
    { who: "Yo", text: "Arrancamos con el estado del demo web." },
    { who: "Otros", text: "El sitio tiene que poder usarse sin la app instalada." },
    { who: "Yo", text: "La pill y la rueda quedan iguales; cada herramienta se puede probar." },
    { who: "Otros", text: "¿Y el video? Lo hacemos con Remotion y lo embebemos." },
    { who: "Yo", text: "Perfecto: primero el demo, después el video." },
    { who: "Otros", text: "De acuerdo. Mando el resumen por correo cuando terminemos." },
  ];

  const SUMMARY = [
    "El sitio muestra el demo interactivo; no reemplaza a la app.",
    "El video se produce con Remotion y se embebe en la sección de videos.",
    "El audio no sale del equipo salvo que se elija Groq para dictado o live.",
  ];

  type Phase = "idle" | "recording" | "transcribing" | "done";

  let phase = $state<Phase>("idle");
  let seconds = $state(0);
  let shown = $state(0);
  let timer: number | undefined;
  let captionTimer: number | undefined;

  const elapsed = $derived(
    `${String(Math.floor(seconds / 60)).padStart(2, "0")}:${String(seconds % 60).padStart(2, "0")}`,
  );

  function start() {
    phase = "recording";
    seconds = 0;
    shown = 0;
    timer = window.setInterval(() => (seconds += 1), 1000);
    captionTimer = window.setInterval(() => {
      if (shown < CAPTIONS.length) shown += 1;
    }, 1700);
  }

  function stop() {
    window.clearInterval(timer);
    window.clearInterval(captionTimer);
    phase = "transcribing";
    window.setTimeout(() => (phase = "done"), 1400);
  }

  function reset() {
    phase = "idle";
    seconds = 0;
    shown = 0;
  }

  async function copySummary() {
    await copyText(SUMMARY.map((line) => `· ${line}`).join("\n"), "Resumen copiado");
  }

  $effect(() => {
    return () => {
      window.clearInterval(timer);
      window.clearInterval(captionTimer);
    };
  });
</script>

<Float title="Reuniones" icon="meetings" onClose={onClose}>
  <div class="meetings">
    {#if phase === "idle"}
      <div class="idle">
        <span class="big-icon"><Icon icon={Mic} size={22} /></span>
        <p class="idle-title">Detectamos una llamada.</p>
        <p class="idle-sub">Micrófono y audio del sistema en pistas separadas; transcripción local.</p>
        <button type="button" class="btn is-rec" onclick={start}>
          <span class="rec-dot"></span> Grabar esta reunión
        </button>
        <p class="idle-note">Atic nunca graba solo: hace falta una acción tuya.</p>
        <p class="idle-note">
          En muchas jurisdicciones grabar requiere el consentimiento de los
          participantes. Úsalo solo cuando esté permitido y, si hace falta, avisa.
        </p>
      </div>
    {:else if phase === "recording"}
      <div class="recording">
        <div class="rec-head">
          <span class="rec-pill"><span class="rec-dot"></span> {elapsed}</span>
          <span class="tracks">
            <i class="track is-mic"></i> mic
            <i class="track is-sys"></i> sistema
          </span>
          <button type="button" class="btn" onclick={stop}><Icon icon={Square} size={11} /> Detener</button>
        </div>
        <div class="wave" aria-hidden="true" title="Nivel de mic y sistema">
          {#each Array(22) as _, i (i)}
            <i class={i % 2 === 0 ? "is-mic" : "is-sys"} style="--i: {i}"></i>
          {/each}
        </div>
        <p class="live-note">Transcripción en vivo (experimental en la app).</p>
        <ul class="captions">
          {#each CAPTIONS.slice(0, shown) as line, i (i)}
            <li class="caption">
              <span class="who" class:is-other={line.who === "Otros"}>{line.who}</span>
              <span class="caption-text">{line.text}</span>
            </li>
          {/each}
          {#if shown === 0}
            <li class="caption is-empty">Escuchando…</li>
          {/if}
        </ul>
      </div>
    {:else if phase === "transcribing"}
      <div class="idle">
        <span class="big-icon is-spin"><Icon icon={Pause} size={22} /></span>
        <p class="idle-title">Transcribiendo en local…</p>
        <div class="progress"><i></i></div>
        <p class="idle-note">Whisper corre en tu equipo. El audio no viaja.</p>
      </div>
    {:else}
      <div class="done">
        <h4 class="done-title"><Icon icon={Check} size={14} /> Resumen listo</h4>
        <ul class="summary">
          {#each SUMMARY as line (line)}
            <li>{line}</li>
          {/each}
        </ul>
        <div class="done-actions">
          <button type="button" class="btn is-primary" onclick={() => demo.toast("Borrador abierto en tu correo")}>
            Enviar por correo
          </button>
          <button type="button" class="btn" onclick={copySummary}><Icon icon={Copy} size={12} /> Copiar</button>
          <button type="button" class="btn" onclick={reset}><Icon icon={Play} size={12} /> Otra</button>
        </div>
        <p class="done-notes">
          Vista simplificada: en la app el resumen tiene 4 plantillas y se edita
          antes de enviar, por SMTP o como borrador mailto. La grabación queda en
          la biblioteca, con reproductor y transcripción editable.
        </p>
      </div>
    {/if}
  </div>
</Float>

<style>
  .meetings {
    min-height: 260px;
  }

  .idle {
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: center;
    padding: 30px 22px;
    text-align: center;
  }

  .big-icon {
    display: grid;
    width: 44px;
    height: 44px;
    place-items: center;
    border-radius: 50%;
    background: color-mix(in srgb, var(--text) 7%, transparent);
    color: var(--muted);
  }

  .big-icon.is-spin {
    animation: spin 1.4s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .idle-title {
    margin: 4px 0 0;
    color: var(--text);
    font-size: var(--text-md);
  }

  .idle-sub {
    margin: 0;
    max-width: 30ch;
    color: var(--faint);
    font-size: var(--text-xs);
    line-height: 1.5;
  }

  .idle-note {
    margin: 6px 0 0;
    color: var(--faint);
    font-size: var(--text-micro);
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    padding: 5px 10px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--text-micro);
    cursor: pointer;
  }

  .btn:hover {
    border-color: var(--line-strong);
    color: var(--text);
  }

  .btn.is-primary {
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    background: var(--accent);
    color: var(--on-accent);
  }

  .btn.is-rec {
    border-color: color-mix(in srgb, var(--rec) 45%, transparent);
    color: var(--rec);
  }

  .rec-dot {
    width: 8px;
    height: 8px;
    background: var(--rec);
    animation: pulse 1.4s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  .recording {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
  }

  .rec-head {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .rec-pill {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    border: 1px solid color-mix(in srgb, var(--rec) 45%, transparent);
    border-radius: var(--radius-pill);
    padding: 4px 10px;
    color: var(--rec);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
  }

  .tracks {
    display: inline-flex;
    flex: 1;
    align-items: center;
    gap: 6px;
    color: var(--faint);
    font-size: var(--text-micro);
  }

  .track {
    width: 14px;
    height: 3px;
    border-radius: 999px;
  }

  .track.is-mic {
    background: var(--mic);
  }

  .track.is-sys {
    background: var(--sys);
  }

  .wave {
    display: flex;
    height: 34px;
    align-items: center;
    gap: 2px;
    justify-content: center;
  }

  .wave i {
    width: 3px;
    border-radius: 999px;
    background: var(--mic);
    animation: wave 0.9s ease-in-out infinite;
    animation-delay: calc(var(--i) * -0.07s);
  }

  .wave i.is-sys {
    background: var(--sys);
  }

  .live-note {
    margin: 0;
    color: var(--faint);
    font-size: var(--text-micro);
    text-align: center;
  }

  @keyframes wave {
    0%,
    100% {
      height: 5px;
      opacity: 0.5;
    }
    50% {
      height: 26px;
      opacity: 1;
    }
  }

  .captions {
    display: flex;
    margin: 0;
    flex-direction: column;
    gap: 7px;
    padding: 0;
    list-style: none;
    max-height: 150px;
    overflow: auto;
  }

  .caption {
    display: flex;
    gap: 8px;
    font-size: var(--text-sm);
    line-height: 1.45;
  }

  .who {
    flex-shrink: 0;
    width: 3.2rem;
    color: var(--mic);
    font-size: var(--text-micro);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .who.is-other {
    color: var(--sys);
  }

  .caption-text {
    color: var(--muted);
  }

  .caption.is-empty {
    color: var(--faint);
    font-style: italic;
  }

  .progress {
    width: 200px;
    height: 4px;
    overflow: hidden;
    border-radius: 999px;
    background: color-mix(in srgb, var(--text) 10%, transparent);
  }

  .progress i {
    display: block;
    width: 40%;
    height: 100%;
    border-radius: 999px;
    background: var(--accent);
    animation: slide 1.1s ease-in-out infinite;
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(350%);
    }
  }

  .done {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 16px;
  }

  .done-title {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 0;
    color: var(--ok);
    font-size: var(--text-sm);
    font-weight: var(--font-weight-semibold);
  }

  .summary {
    margin: 0;
    padding: 0 0 0 2px;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .summary li {
    position: relative;
    padding-left: 14px;
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.5;
  }

  .summary li::before {
    content: "";
    position: absolute;
    top: 0.55em;
    left: 2px;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--faint);
  }

  .done-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding-top: 4px;
  }

  .done-notes {
    margin: 2px 0 0;
    color: var(--faint);
    font-size: var(--text-micro);
    line-height: 1.6;
  }
</style>
