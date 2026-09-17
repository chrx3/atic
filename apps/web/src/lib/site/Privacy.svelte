<script lang="ts">
  /**
   * Privacidad: lo que la app promete y cumple por arquitectura — nada sale
   * del equipo si no se elige explícitamente.
   */
  import Icon from "$lib/atic/Icon.svelte";
  import { Lock, ShieldCheck, Mic, FileText } from "$lib/atic/icons";

  const POINTS = [
    {
      icon: ShieldCheck,
      title: "Todo vive en tu disco",
      text: "Grabaciones, transcripciones, historial, conversaciones y logs quedan en la carpeta de datos de la app. No hay servidor de Atic.",
    },
    {
      icon: Lock,
      title: "Claves en el llavero",
      text: "Las API keys y la contraseña SMTP van al llavero del sistema, no a un archivo de configuración.",
    },
    {
      icon: Mic,
      title: "Whisper transcribe en local",
      text: "El audio se procesa en tu equipo. Solo sale a una API si tú eliges Groq para dictado o transcripción en vivo.",
    },
    {
      icon: FileText,
      title: "Los resúmenes son BYOK",
      text: "Cuando resumes con la nube, viaja el texto —nunca el audio— y la clave es tuya. Ollama mantiene la etapa local.",
    },
  ];
</script>

<section class="privacy" id="privacidad">
  <div class="wrap">
    <header class="head">
      <span class="eyebrow">Privacidad</span>
      <h2>Local primero, y se nota</h2>
      <p>
        Atic no autentica agentes: usa el CLI que ya instalaste y logueaste. No hay
        cuenta Atic ni telemetría de producto.
      </p>
    </header>

    <ul class="grid">
      {#each POINTS as point (point.title)}
        <li class="card">
          <span class="card-icon"><Icon icon={point.icon} size={16} /></span>
          <h3>{point.title}</h3>
          <p>{point.text}</p>
        </li>
      {/each}
    </ul>

    <p class="legal">
      Grabar a otras personas puede requerir consentimiento según el país. Atic nunca
      graba solo: hace falta una acción tuya. La detección de llamadas solo sugiere.
    </p>
  </div>
</section>

<style>
  .wrap {
    max-width: 1080px;
    margin: 0 auto;
    padding: clamp(48px, 8vh, 84px) 20px 0;
  }

  .head {
    max-width: 620px;
    margin-bottom: 26px;
  }

  .eyebrow {
    color: var(--faint);
    font-size: var(--text-micro);
    font-weight: var(--font-weight-semibold);
    letter-spacing: var(--tracking-caps);
    text-transform: uppercase;
  }

  h2 {
    margin: 10px 0 10px;
    font-family: var(--font-display);
    font-size: clamp(1.5rem, 3.4vw, 2.125rem);
    font-weight: var(--font-weight-semibold);
    letter-spacing: -0.025em;
  }

  .head p {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-md);
    line-height: 1.6;
  }

  .grid {
    display: grid;
    margin: 0;
    padding: 0;
    gap: 10px;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    list-style: none;
  }

  .card {
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    padding: 16px;
    background: color-mix(in srgb, var(--surface) 55%, transparent);
  }

  .card-icon {
    display: grid;
    width: 32px;
    height: 32px;
    place-items: center;
    margin-bottom: 10px;
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background: var(--skin);
    color: var(--ok);
  }

  .card h3 {
    margin: 0 0 6px;
    font-size: var(--text-md);
    font-weight: var(--font-weight-semibold);
  }

  .card p {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.55;
  }

  .legal {
    margin: 20px 0 0;
    border-left: 2px solid var(--warn);
    padding: 2px 0 2px 12px;
    color: var(--faint);
    font-size: var(--text-xs);
    line-height: 1.6;
  }
</style>
