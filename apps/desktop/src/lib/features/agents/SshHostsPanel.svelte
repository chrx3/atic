<script lang="ts">
  /**
   * CRUD de hosts SSH + test de conexión + passphrase en keyring.
   * Los secretos nunca se muestran de vuelta: solo flags has_*.
   */
  import { onMount } from "svelte";
  import {
    sshDeleteHostSecrets,
    sshHostSecretsStatus,
    sshSetHostSecret,
    sshTestHost,
  } from "$ipc/agents";
  import { config as appConfig } from "$domain/config.svelte";
  import { t } from "$domain/i18n.svelte";
  import { getConfig, setConfig } from "$ipc/config";
  import { pickSshIdentityFile } from "$ipc/dialogs";
  import type { AppConfig, SshHost, SshHostSecretFlags } from "$lib/types";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Button from "$ui/Button.svelte";
  import ConfirmDialog from "$ui/ConfirmDialog.svelte";
  import Input from "$ui/Input.svelte";
  import Select from "$ui/Select.svelte";

  let {
    config = $bindable(),
    onToast,
  }: {
    config: AppConfig;
    onToast: (msg: string) => void;
  } = $props();

  let secretFlags = $state<SshHostSecretFlags[]>([]);
  let editingId = $state<string | null>(null);
  let draft = $state<SshHost | null>(null);
  let passphrase = $state("");
  let testingId = $state<string | null>(null);
  let testMessage = $state<string | null>(null);

  const hosts = $derived(config.ssh_hosts ?? []);

  function flagsFor(id: string): SshHostSecretFlags | undefined {
    return secretFlags.find((f) => f.hostId === id);
  }

  async function refreshFlags() {
    try {
      secretFlags = await sshHostSecretsStatus();
    } catch (e) {
      onToast(String(e));
    }
  }

  /** Persiste solo `ssh_hosts` sin pisar otros cambios locales de Ajustes. */
  async function persistHosts(list: SshHost[]) {
    const latest = await getConfig();
    const next = { ...latest, ssh_hosts: list };
    await setConfig(next);
    config = { ...config, ssh_hosts: list };
    // SettingsPanel usa el store de dominio; mantenerlo alineado tras el write.
    if (appConfig.current) {
      appConfig.current = { ...appConfig.current, ssh_hosts: list };
    }
  }

  onMount(() => {
    void refreshFlags();
  });

  function emptyHost(): SshHost {
    return {
      id: crypto.randomUUID(),
      label: "",
      user: "",
      host: "",
      // 0 = omitir -p (ideal para alias de ~/.ssh/config).
      port: 0,
      auth: "agent",
      identity_file: null,
      default_remote_cwd: null,
      remote_agent_bin: null,
      last_test_ok: null,
      last_test_at: null,
    };
  }

  function destinationLabel(h: SshHost): string {
    const host = h.host.trim();
    const user = h.user.trim();
    const base = user ? `${user}@${host}` : host;
    return h.port > 0 ? `${base}:${h.port}` : base;
  }

  function startCreate() {
    draft = emptyHost();
    editingId = draft.id;
    passphrase = "";
    testMessage = null;
  }

  function startEdit(h: SshHost) {
    draft = { ...h };
    editingId = h.id;
    passphrase = "";
    testMessage = null;
  }

  function cancelEdit() {
    draft = null;
    editingId = null;
    passphrase = "";
  }

  async function pickIdentity() {
    if (!draft) return;
    const picked = await pickSshIdentityFile();
    if (picked) {
      draft = { ...draft, identity_file: picked };
    }
  }

  async function saveDraft() {
    if (!draft) return;
    const user = draft.user.trim();
    const host = draft.host.trim();
    if (!host) {
      onToast(t("page.agents.ssh.hostRequired"));
      return;
    }
    if (host.includes("@") || /\s/.test(host)) {
      onToast(t("page.agents.ssh.hostOnlyName"));
      return;
    }
    // host:22 pegado por error (permitir IPv6 con varios ':').
    if (/^[^:]+:\d+$/.test(host)) {
      onToast(t("page.agents.ssh.noPortInHost"));
      return;
    }
    if (user.includes("@") || /\s/.test(user)) {
      onToast(t("page.agents.ssh.invalidUser"));
      return;
    }
    const label = draft.label.trim() || (user ? `${user}@${host}` : host);
    if (draft.auth === "key" && !draft.identity_file?.trim()) {
      onToast(t("page.agents.ssh.keyAuthNeedsFile"));
      return;
    }
    const next: SshHost = {
      ...draft,
      label,
      user,
      host,
      // 0 = no pasar -p (ssh_config / default OpenSSH).
      port: draft.port > 0 ? draft.port : 0,
      auth: draft.auth === "key" ? "key" : "agent",
      identity_file: draft.auth === "key" ? draft.identity_file?.trim() || null : null,
      default_remote_cwd: draft.default_remote_cwd?.trim() || null,
      remote_agent_bin: draft.remote_agent_bin?.trim() || null,
    };
    const list = [...(config.ssh_hosts ?? [])];
    const idx = list.findIndex((h) => h.id === next.id);
    if (idx >= 0) list[idx] = next;
    else list.push(next);
    try {
      await persistHosts(list);
      if (passphrase.trim()) {
        await sshSetHostSecret(next.id, "passphrase", passphrase.trim());
        passphrase = "";
      }
      await refreshFlags();
    } catch (e) {
      onToast(String(e));
      return;
    }
    draft = null;
    editingId = null;
    onToast(t("page.agents.ssh.saved"));
  }

  /** Host esperando confirmación: borra el host y sus secretos del llavero. */
  let removing = $state<SshHost | null>(null);

  async function removeHost(id: string) {
    const list = (config.ssh_hosts ?? []).filter((h) => h.id !== id);
    try {
      await persistHosts(list);
      await sshDeleteHostSecrets(id);
      await refreshFlags();
      onToast(t("page.agents.ssh.deleted"));
    } catch (e) {
      onToast(String(e));
    }
    if (editingId === id) cancelEdit();
    removing = null;
  }

  async function clearPassphrase(id: string) {
    try {
      await sshSetHostSecret(id, "passphrase", "");
      await refreshFlags();
      onToast(t("page.agents.ssh.passphraseCleared"));
    } catch (e) {
      onToast(String(e));
    }
  }

  async function test(h: SshHost) {
    testingId = h.id;
    testMessage = null;
    try {
      const result = await sshTestHost(h);
      testMessage = result.message;
      const list = (config.ssh_hosts ?? []).map((row) =>
        row.id === h.id
          ? {
              ...row,
              last_test_ok: result.ok,
              last_test_at: result.checkedAt,
            }
          : row,
      );
      config = { ...config, ssh_hosts: list };
      onToast(
        result.ok ? t("page.agents.ssh.testOk") : t("page.agents.ssh.testFailed"),
      );
    } catch (e) {
      testMessage = String(e);
      onToast(String(e));
    } finally {
      testingId = null;
    }
  }

  function statusDot(h: SshHost): "ok" | "bad" | "unknown" {
    if (h.last_test_ok === true) return "ok";
    if (h.last_test_ok === false) return "bad";
    return "unknown";
  }
</script>

<SettingsGroup title={t("page.agents.ssh.title")} hint={t("page.agents.ssh.hint")}>
  {#if hosts.length === 0 && !draft}
    <p class="py-2 text-xs text-faint">
      {t("page.agents.ssh.empty")}
    </p>
  {/if}

  {#if hosts.length > 0}
    <ul class="flex flex-col gap-2 py-2">
      {#each hosts as h (h.id)}
        <li
          class="flex min-w-0 items-center gap-2.5 rounded-md bg-surface-2 px-2.5 py-2"
        >
          <span class="ssh-dot shrink-0" data-status={statusDot(h)} aria-hidden="true"
          ></span>
          <div class="min-w-0 flex-1">
            <div class="truncate text-sm font-medium text-text">{h.label}</div>
            <div class="truncate text-xs text-faint">
              {destinationLabel(h)}
              · {h.auth === "key"
                ? t("page.agents.ssh.authKeyTag")
                : t("page.agents.ssh.authAgentTag")}
              {#if flagsFor(h.id)?.hasPassphrase}
                · {t("page.agents.ssh.passphraseTag")}
              {/if}
            </div>
          </div>
          <div class="flex shrink-0 flex-wrap justify-end gap-0.5">
            <Button
              variant="ghost"
              size="sm"
              disabled={testingId === h.id}
              onclick={() => void test(h)}
            >
              {testingId === h.id
                ? t("page.agents.ssh.testBusy")
                : t("page.agents.ssh.test")}
            </Button>
            <Button variant="ghost" size="sm" onclick={() => startEdit(h)}>
              {t("page.agents.ssh.edit")}
            </Button>
            <Button variant="ghost" size="sm" onclick={() => (removing = h)}>
              {t("page.agents.ssh.remove")}
            </Button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}

  {#if testMessage}
    <p class="py-1 text-xs text-faint">{testMessage}</p>
  {/if}

  {#if !draft}
    <div class="py-2">
      <Button variant="soft" size="sm" onclick={startCreate}>
        {t("page.agents.ssh.add")}
      </Button>
    </div>
  {:else}
    {@render hostForm(draft)}
  {/if}
</SettingsGroup>

{#if removing}
  <ConfirmDialog
    title={t("page.agents.ssh.deleteTitle", { label: removing.label })}
    body={t("page.agents.ssh.deleteBody")}
    confirmLabel={t("page.agents.ssh.remove")}
    tone="danger"
    onConfirm={() => void removeHost(removing!.id)}
    onCancel={() => (removing = null)}
  />
{/if}

{#snippet hostForm(d: SshHost)}
  <SettingsRow label={t("page.agents.ssh.labelField")}>
    {#snippet control({ id })}
      <Input {id} bind:value={d.label} placeholder="prod-api" />
    {/snippet}
  </SettingsRow>
  <SettingsRow
    label={t("page.agents.ssh.userField")}
    hint={t("page.agents.ssh.userHint")}
  >
    {#snippet control({ id })}
      <Input
        {id}
        bind:value={d.user}
        placeholder={t("page.agents.ssh.userPlaceholder")}
      />
    {/snippet}
  </SettingsRow>
  <SettingsRow
    label={t("page.agents.ssh.hostField")}
    hint={t("page.agents.ssh.hostHint")}
  >
    {#snippet control({ id })}
      <Input {id} bind:value={d.host} placeholder="contabo o 10.0.0.5" />
    {/snippet}
  </SettingsRow>
  <SettingsRow
    label={t("page.agents.ssh.portField")}
    hint={t("page.agents.ssh.portHint")}
  >
    {#snippet control({ id })}
      <Input
        {id}
        type="number"
        min="0"
        max="65535"
        value={d.port > 0 ? String(d.port) : ""}
        placeholder={t("page.agents.ssh.portPlaceholder")}
        oninput={(e: Event) => {
          const raw = (e.currentTarget as HTMLInputElement).value.trim();
          if (!raw) {
            d.port = 0;
            return;
          }
          const n = Number(raw);
          d.port = n > 0 ? n : 0;
        }}
      />
    {/snippet}
  </SettingsRow>
  <SettingsRow label={t("page.agents.ssh.authField")}>
    {#snippet control({ id })}
      <Select
        {id}
        value={d.auth}
        options={[
          { value: "agent", label: t("page.agents.ssh.authAgentOption") },
          { value: "key", label: t("page.agents.ssh.identityFile") },
        ]}
        onchange={(e: Event) => {
          d.auth = (e.currentTarget as HTMLSelectElement).value;
        }}
      />
    {/snippet}
  </SettingsRow>
  {#if d.auth === "key"}
    <SettingsRow
      label={t("page.agents.ssh.identityFile")}
      hint={t("page.agents.ssh.identityHint")}
    >
      {#snippet control()}
        <div class="flex min-w-0 flex-col gap-1">
          <Input
            readonly
            mono
            value={d.identity_file ?? ""}
            placeholder={t("page.agents.ssh.noFilePlaceholder")}
          />
          <Button variant="ghost" size="sm" full onclick={() => void pickIdentity()}>
            {t("page.agents.ssh.choose")}
          </Button>
        </div>
      {/snippet}
    </SettingsRow>
    <SettingsRow
      label={t("page.agents.ssh.passphrase")}
      hint={flagsFor(d.id)?.hasPassphrase
        ? t("page.agents.ssh.passphraseSavedHint")
        : t("page.agents.ssh.passphraseHint")}
    >
      {#snippet control({ id })}
        <div class="flex min-w-0 flex-col gap-1">
          <Input
            {id}
            type="password"
            autocomplete="new-password"
            placeholder={flagsFor(d.id)?.hasPassphrase
              ? "••••••••"
              : t("page.agents.ssh.optionalPlaceholder")}
            bind:value={passphrase}
          />
          {#if flagsFor(d.id)?.hasPassphrase}
            <Button
              variant="ghost"
              size="sm"
              full
              onclick={() => void clearPassphrase(d.id)}
            >
              {t("page.agents.ssh.clearPassphrase")}
            </Button>
          {/if}
        </div>
      {/snippet}
    </SettingsRow>
  {/if}
  <SettingsRow label={t("page.agents.ssh.cwd")} hint={t("page.agents.ssh.cwdHint")}>
    {#snippet control({ id })}
      <Input
        {id}
        mono
        placeholder="/home/deploy/app"
        value={d.default_remote_cwd ?? ""}
        oninput={(e: Event) => {
          const v = (e.currentTarget as HTMLInputElement).value.trim();
          d.default_remote_cwd = v || null;
        }}
      />
    {/snippet}
  </SettingsRow>
  <SettingsRow
    label={t("page.agents.ssh.remoteBin")}
    hint={t("page.agents.ssh.remoteBinHint")}
  >
    {#snippet control({ id })}
      <Input
        {id}
        mono
        placeholder="claude"
        value={d.remote_agent_bin ?? ""}
        oninput={(e: Event) => {
          const v = (e.currentTarget as HTMLInputElement).value.trim();
          d.remote_agent_bin = v || null;
        }}
      />
    {/snippet}
  </SettingsRow>
  <div class="flex justify-end gap-2 py-2">
    <Button variant="ghost" size="sm" onclick={cancelEdit}>
      {t("page.agents.ssh.cancel")}
    </Button>
    <Button variant="primary" size="sm" onclick={() => void saveDraft()}>
      {t("page.agents.ssh.done")}
    </Button>
  </div>
{/snippet}

<style>
  .ssh-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #9ca3af;
  }

  .ssh-dot[data-status="ok"] {
    background: #22a06b;
  }

  .ssh-dot[data-status="bad"] {
    background: #e34935;
  }
</style>
