<script lang="ts">
  /**
   * Agentes en la ventana principal: solo configuración.
   *
   * Las consolas vivas no viven acá: están en la pill o en su ventana
   * dedicada. Esta tool es la puerta a todo lo configurable de los agentes
   * (los mismos controles de Ajustes → Agentes) más el botón que abre la
   * ventana de consolas.
   */
  import AgentsSection from "$features/settings/AgentsSection.svelte";
  import Button from "$ui/Button.svelte";
  import Icon from "$ui/Icon.svelte";
  import { SquareArrowOutUpRight } from "$lib/icons";
  import { agentsEnsureWindow } from "$ipc/agents";
  import { toastError } from "$domain/toasts.svelte";
  import { t } from "$domain/i18n.svelte";
</script>

<div class="host">
  <div class="open-row">
    <Button variant="soft" onclick={() => void agentsEnsureWindow().catch(toastError)}>
      {#snippet icon()}<Icon icon={SquareArrowOutUpRight} size={14} />{/snippet}
      {t("page.agents.openConsolesWindow")}
    </Button>
    <p>{t("page.agents.openConsolesWindowHint")}</p>
  </div>
  <div class="config-scroll">
    <AgentsSection />
  </div>
</div>

<style>
  .host {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .open-row {
    display: flex;
    flex: 0 0 auto;
    align-items: center;
    gap: 0.75rem;
    padding: 0.85rem 1rem 0.65rem;
  }

  .open-row p {
    margin: 0;
    min-width: 0;
    color: var(--muted);
    font-size: 0.75rem;
    line-height: 1.4;
  }

  .config-scroll {
    min-height: 0;
    flex: 1;
    overflow-y: auto;
    padding: 0 1rem 1.5rem;
  }
</style>
