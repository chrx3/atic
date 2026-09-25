<script lang="ts">
  /**
   * Un plan que el agente pide aprobar antes de ejecutarlo.
   *
   * Claude lo pide con `ExitPlanMode` y Cursor con `cursor/create_plan`; el
   * adaptador de Cursor lo trae con la misma forma, así que es una tarjeta
   * para los dos. Antes se veía como el JSON del permiso —el de Claude— o se
   * aceptaba solo —el de Cursor—: aprobar a ciegas lo único que se quería leer.
   */
  import AgentMessage from "$lib/AgentMessage.svelte";
  import type { PermissionDecision } from "$core/types";
  import { t } from "$domain/i18n.svelte";

  let {
    input,
    agentName,
    busy = false,
    onDecide,
  }: {
    input: unknown;
    agentName: string;
    busy?: boolean;
    onDecide: (decision: PermissionDecision) => void;
  } = $props();

  type Todo = { content: string; status: string };

  const fields = $derived.by(() => {
    const o =
      input && typeof input === "object" ? (input as Record<string, unknown>) : {};
    const text = (key: string) =>
      typeof o[key] === "string" ? (o[key] as string) : "";
    const todos = Array.isArray(o.todos)
      ? o.todos.flatMap((todo): Todo[] => {
          if (!todo || typeof todo !== "object") return [];
          const { content, status } = todo as Record<string, unknown>;
          return typeof content === "string" && content.trim()
            ? [{ content, status: typeof status === "string" ? status : "pending" }]
            : [];
        })
      : [];
    return {
      name: text("name"),
      overview: text("overview"),
      plan: text("plan"),
      todos,
    };
  });
</script>

<section class="plan" aria-label={t("page.agents.chat.planTitle", { name: agentName })}>
  <p class="title">
    {fields.name || t("page.agents.chat.planTitle", { name: agentName })}
  </p>
  {#if fields.overview}
    <p class="overview">{fields.overview}</p>
  {/if}

  {#if fields.plan.trim()}
    <div class="body">
      <AgentMessage text={fields.plan} />
    </div>
  {/if}

  {#if fields.todos.length > 0}
    <ol class="todos">
      {#each fields.todos as todo, i (i)}
        <li data-s={todo.status}>{todo.content}</li>
      {/each}
    </ol>
  {/if}

  <div class="actions">
    <button type="button" class="btn" disabled={busy} onclick={() => onDecide("deny")}>
      {t("page.agents.chat.planReject")}
    </button>
    <button
      type="button"
      class="btn is-primary"
      disabled={busy}
      onclick={() => onDecide("allow")}
    >
      {t("page.agents.chat.planApprove")}
    </button>
  </div>
</section>

<style>
  .plan {
    display: flex;
    flex-direction: column;
    gap: 8px;
    border-radius: 14px;
    padding: 12px;
    background: color-mix(in sRGB, var(--accent) 7%, var(--rb-surface));
    box-shadow:
      inset 0 0 0 1px color-mix(in sRGB, var(--accent) 28%, transparent),
      0 8px 24px -16px rgb(0 0 0 / 50%);
  }

  .title {
    margin: 0;
    font-size: 13.5px;
    font-weight: 600;
    text-wrap: balance;
  }

  .overview {
    margin: 0;
    color: var(--rb-muted);
    font-size: 12.5px;
    text-wrap: pretty;
  }

  /* El plan puede ser largo: se lee acá adentro sin empujar el composer. */
  .body {
    max-height: 320px;
    overflow-y: auto;
    border-radius: 10px;
    padding: 8px 12px;
    background: var(--rb-bg0);
    overscroll-behavior: contain;
  }

  .todos {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin: 0;
    padding-left: 20px;
    color: var(--rb-muted);
    font-size: 12.5px;
  }

  .todos li[data-s="in_progress"] {
    color: var(--rb-text);
  }

  .todos li[data-s="completed"] {
    color: var(--rb-faint);
    text-decoration: line-through;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }

  .btn {
    min-height: 30px;
    border: 0;
    border-radius: 8px;
    padding: 0 12px;
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
    color: var(--rb-text);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition:
      background-color 120ms ease,
      scale 120ms ease;
  }

  .btn:active:not(:disabled) {
    scale: 0.96;
  }

  .btn.is-primary {
    background: var(--accent);
    color: var(--rb-on-accent, var(--rb-bg0));
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
