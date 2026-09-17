<script lang="ts">
  /**
   * El pedido de permiso de un agente, resuelto desde la ventana.
   *
   * La pill muestra su tarjeta solo cuando la consola NO está abierta —«consola
   * abierta: el permiso se decide ahí», dice su propio código—, así que este es
   * el otro extremo del mismo contrato. Sin esto, con la ventana a la vista y un
   * agente esperando, no había dónde contestar.
   *
   * Va arriba del contenido y con el tono de aviso: es un estado, y el agente
   * está detenido hasta que alguien decida.
   */
  import type { PendingPermission } from "$lib/agentSessions.svelte";
  import type { PermissionDecision } from "$core/types";
  import { t } from "$domain/i18n.svelte";
  import Button from "$ui/Button.svelte";

  let {
    permission,
    sessionLabel = "",
    busy = false,
    onDecide,
  }: {
    permission: PendingPermission;
    /** Quién pide: backend y nombre de la sesión, si los tiene. */
    sessionLabel?: string;
    busy?: boolean;
    onDecide: (decision: PermissionDecision) => void;
  } = $props();
</script>

<section class="bar" aria-label={t("page.agents.permission.title")}>
  <p class="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-1.5 text-sm text-text">
    <span class="font-medium">{t("page.agents.permission.title")}</span>
    <span class="min-w-0 truncate text-muted">
      {permission.tool}{#if permission.description?.trim()}
        · {permission.description.trim()}{/if}
    </span>
  </p>

  {#if sessionLabel}
    <span class="shrink-0 font-mono text-xs text-faint" data-numeric
      >{sessionLabel}</span
    >
  {/if}

  <!-- La decisión más probable va al final, como en una barra del sistema. -->
  <div class="flex shrink-0 items-center gap-1.5">
    <Button variant="soft" size="md" disabled={busy} onclick={() => onDecide("deny")}>
      {t("page.agents.permission.deny")}
    </Button>
    <Button
      variant="soft"
      size="md"
      disabled={busy}
      onclick={() => onDecide("allowAlways")}
    >
      {t("page.agents.permission.allowAlways")}
    </Button>
    <Button
      variant="primary"
      size="md"
      disabled={busy}
      onclick={() => onDecide("allow")}
    >
      {t("page.agents.permission.allow")}
    </Button>
  </div>
</section>

<style>
  .bar {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 0.75rem;
    border-bottom: 1px solid var(--line);
    background: var(--warn-soft);
    padding: 0.5rem 0.75rem;
  }
</style>
