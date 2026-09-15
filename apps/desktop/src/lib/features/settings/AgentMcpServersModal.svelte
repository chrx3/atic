<script lang="ts">
  /**
   * Servidores MCP que Atic le suma al agente.
   *
   * Son herramientas **para el agente**, no para Atic: al abrir una sesión se
   * pasan a Claude por `--mcp-config`, a Codex por `-c` y a los ACP en el
   * `mcp_servers` del `session/new`. Atic solo los guarda y decide cuáles van.
   *
   * El JSON se edita crudo a propósito. Cada servidor documenta su bloque y lo
   * normal es pegarlo tal cual; un formulario con campos fijos (comando, args,
   * env) obligaría a traducir a mano y se quedaría corto con cada variante
   * nueva —HTTP, SSE, headers— que aparezca.
   *
   * Lo que no sea stdio o no tenga traducción para un backend se saltea al
   * arrancar con un aviso en el log; acá no se bloquea, porque para Claude
   * puede ser perfectamente válido.
   */
  import { untrack } from "svelte";
  import type { McpServerConfig } from "$core/types";
  import { config } from "$domain/config.svelte";
  import { t } from "$domain/i18n.svelte";
  import { Trash2 } from "$lib/icons";
  import Button from "$ui/Button.svelte";
  import Field from "$ui/Field.svelte";
  import Icon from "$ui/Icon.svelte";
  import IconButton from "$ui/IconButton.svelte";
  import Input from "$ui/Input.svelte";
  import Modal from "$ui/Modal.svelte";
  import Switch from "$ui/Switch.svelte";
  import TextArea from "$ui/TextArea.svelte";

  let { onClose }: { onClose: () => void } = $props();

  const EXAMPLE = `{
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:/ruta"]
}`;

  function parse(raw: string | undefined): McpServerConfig[] {
    try {
      const v = JSON.parse(raw ?? "[]");
      return Array.isArray(v) ? v : [];
    } catch {
      return [];
    }
  }

  // Copia editable: se toma una vez y a partir de ahí manda el borrador.
  // Seguir la config haría que un guardado externo pisara lo que estás
  // escribiendo.
  let draft = $state<McpServerConfig[]>(
    untrack(() => parse(config.current?.agent_mcp_servers).map((s) => ({ ...s }))),
  );
  let saving = $state(false);
  let error = $state<string | null>(null);

  /** Qué entradas tienen el JSON roto. Se avisa al editar, no al arrancar. */
  const broken = $derived(
    draft
      .map((s, i) => ({ i, ok: isValid(s.json) }))
      .filter((x) => !x.ok)
      .map((x) => x.i),
  );

  function isValid(json: string): boolean {
    if (!json.trim()) return false;
    try {
      const parsed = JSON.parse(json);
      return typeof parsed === "object" && parsed !== null;
    } catch {
      return false;
    }
  }

  function add() {
    draft = [...draft, { name: "", json: EXAMPLE, enabled: true }];
  }

  function remove(index: number) {
    draft = draft.filter((_, i) => i !== index);
  }

  async function save() {
    saving = true;
    error = null;
    try {
      // Se guardan también los que están rotos: perder lo que alguien estaba
      // escribiendo por un JSON a medias sería peor que arrancar sin ellos.
      // Al iniciar una sesión se saltan solos.
      await config.patch({ agent_mcp_servers: JSON.stringify(draft) });
      onClose();
    } catch (err) {
      error = String(err);
    } finally {
      saving = false;
    }
  }
</script>

<Modal title={t("settings.agents.mcpTitle")} size="lg" {onClose}>
  <div class="flex flex-col gap-3">
    <p class="m-0 text-xs text-faint">{t("settings.agents.mcpBody")}</p>

    {#each draft as server, i (i)}
      <div
        class="flex flex-col gap-2 rounded-md border p-2.5"
        class:border-danger={broken.includes(i)}
        class:border-line={!broken.includes(i)}
      >
        <div class="flex items-end gap-2">
          <div class="min-w-0 flex-1">
            <Field label={t("settings.agents.mcpName")}>
              {#snippet children({ id })}
                <Input {id} bind:value={server.name} placeholder="filesystem" />
              {/snippet}
            </Field>
          </div>
          <div class="pb-1.5">
            <Switch
              bind:checked={server.enabled}
              label={t("settings.agents.mcpActive")}
            />
          </div>
          <div class="pb-0.5">
            <IconButton
              label={t("settings.agents.mcpRemove")}
              variant="danger"
              size="sm"
              onclick={() => remove(i)}
            >
              <Icon icon={Trash2} size={14} />
            </IconButton>
          </div>
        </div>
        <TextArea
          bind:value={server.json}
          rows={5}
          invalid={broken.includes(i)}
          spellcheck="false"
          class="font-mono text-[11px] leading-snug"
          aria-label={t("settings.agents.mcpJsonAria")}
        />
        {#if broken.includes(i)}
          <p class="m-0 text-xs text-danger" role="alert">
            {t("settings.agents.mcpInvalid")}
          </p>
        {/if}
      </div>
    {/each}

    {#if draft.length === 0}
      <p class="m-0 py-2 text-xs text-faint">
        {t("settings.agents.mcpEmpty")}
      </p>
    {/if}

    <p class="m-0 text-xs text-faint">{t("settings.agents.mcpAticNote")}</p>

    {#if error}
      <p class="m-0 text-xs text-danger" role="alert">{error}</p>
    {/if}

    <div>
      <Button variant="soft" size="sm" onclick={add}>
        {t("settings.agents.mcpAdd")}
      </Button>
    </div>
  </div>

  {#snippet actions()}
    <Button variant="soft" onclick={onClose}>
      {t("settings.agents.mcpCancel")}
    </Button>
    <Button
      variant="primary"
      disabled={saving}
      loading={saving}
      onclick={() => void save()}
    >
      {saving ? t("settings.agents.mcpSaving") : t("settings.agents.mcpSave")}
    </Button>
  {/snippet}
</Modal>
