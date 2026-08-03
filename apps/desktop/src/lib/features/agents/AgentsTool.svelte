<script lang="ts">
  /**
   * Agentes en la ventana principal: un acceso, no una consola.
   *
   * La consola vive en su propia ventana porque hace falta tenerla abierta al
   * lado de lo que estés haciendo. Duplicarla acá daría dos vistas del mismo
   * proceso compitiendo por ser «la» sesión.
   *
   * Usa todavía el store viejo de sesiones: es el mismo que mueve la consola
   * del overlay, y separarlo antes de reescribirla dejaría dos verdades.
   */
  import { agents } from "$lib/agentSessions.svelte";
  import { showAgentsWindow } from "$ipc/agents";
  import ToolPage from "$patterns/ToolPage.svelte";
  import Button from "$ui/Button.svelte";
  import Chip from "$ui/Chip.svelte";

  $effect(() => {
    void agents.init();
  });

  /** Qué le pasa a cada sesión, de lo más urgente a lo más tranquilo. */
  function state(session: (typeof agents.sessions)[number]) {
    if (session.pending.length > 0)
      return { tone: "warn", label: "espera permiso" } as const;
    if (session.status === "working")
      return { tone: "info", label: "trabajando" } as const;
    if (session.unread > 0) {
      return { tone: "info", label: `${session.unread} sin leer` } as const;
    }
    return { tone: "neutral", label: "lista" } as const;
  }
</script>

<ToolPage
  title="Agentes"
  blurb="Conversá con agentes de consola desde una interfaz, sin perder sus herramientas."
  kicker="Consola con interfaz"
>
  {#snippet meta()}
    {#if agents.sessions.length > 0}
      <Chip>{agents.sessions.length} sesiones</Chip>
    {/if}
  {/snippet}

  <div class="flex max-w-128 flex-col items-start gap-4 p-4">
    <p class="text-sm leading-relaxed text-muted">
      Atic no reemplaza a tu agente: lanza el que ya tenés instalado, con tu sesión, tus
      herramientas y tus skills. Solo le pone una interfaz con permisos, contexto a la
      vista y avisos en la pill.
    </p>

    {#if agents.sessions.length > 0}
      <ul class="flex w-full list-none flex-col gap-1">
        {#each agents.sessions as session (session.id)}
          {@const now = state(session)}
          <li
            class="flex items-center justify-between gap-3 rounded-sm border border-line
                   px-3 py-2"
          >
            <span class="truncate text-sm text-text">{session.backendName}</span>
            <Chip tone={now.tone}>{now.label}</Chip>
          </li>
        {/each}
      </ul>
    {/if}

    <Button variant="primary" onclick={() => void showAgentsWindow()}>
      Abrir la consola
    </Button>
  </div>
</ToolPage>
