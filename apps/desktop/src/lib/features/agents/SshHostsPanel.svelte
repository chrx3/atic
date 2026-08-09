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
  import { getConfig, setConfig } from "$ipc/config";
  import { pickSshIdentityFile } from "$ipc/dialogs";
  import type { AppConfig, SshHost, SshHostSecretFlags } from "$lib/types";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import SettingsRow from "$patterns/SettingsRow.svelte";
  import Button from "$ui/Button.svelte";
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
      onToast("Host es obligatorio (IP, hostname o alias de ssh_config).");
      return;
    }
    if (host.includes("@") || /\s/.test(host)) {
      onToast(
        "Host debe ser solo el nombre/IP/alias. Usuario y puerto van aparte (o vacíos con alias).",
      );
      return;
    }
    // host:22 pegado por error (permitir IPv6 con varios ':').
    if (/^[^:]+:\d+$/.test(host)) {
      onToast(
        "No pongas el puerto en Host. Usá el campo Puerto, o vacío con alias de ssh_config.",
      );
      return;
    }
    if (user.includes("@") || /\s/.test(user)) {
      onToast("Usuario inválido. Con un alias de ssh_config, dejalo vacío.");
      return;
    }
    const label =
      draft.label.trim() || (user ? `${user}@${host}` : host);
    if (draft.auth === "key" && !draft.identity_file?.trim()) {
      onToast("Auth por clave: elegí un identity file.");
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
      identity_file:
        draft.auth === "key" ? (draft.identity_file?.trim() || null) : null,
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
    onToast("Host SSH guardado.");
  }

  async function removeHost(id: string) {
    const list = (config.ssh_hosts ?? []).filter((h) => h.id !== id);
    try {
      await persistHosts(list);
      await sshDeleteHostSecrets(id);
      await refreshFlags();
    } catch (e) {
      onToast(String(e));
    }
    if (editingId === id) cancelEdit();
  }

  async function clearPassphrase(id: string) {
    try {
      await sshSetHostSecret(id, "passphrase", "");
      await refreshFlags();
      onToast("Passphrase eliminada del llavero.");
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
      onToast(result.ok ? "Conexión SSH OK" : "Falló el test SSH");
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

<SettingsGroup
  title="Hosts SSH"
  hint="Podés usar un alias de ~/.ssh/config (Host = contabo, Usuario/Puerto vacíos). Preferí ssh-agent. Hace falta el cliente OpenSSH."
>
  {#if hosts.length === 0 && !draft}
    <p class="py-2 text-xs text-faint">
      Todavía no hay hosts. Agregá uno para empezar.
    </p>
  {/if}

  {#if hosts.length > 0}
    <ul class="flex flex-col gap-2 py-2">
      {#each hosts as h (h.id)}
        <li
          class="flex min-w-0 items-center gap-2.5 rounded-md bg-surface-2 px-2.5 py-2"
        >
          <span
            class="ssh-dot shrink-0"
            data-status={statusDot(h)}
            aria-hidden="true"
          ></span>
          <div class="min-w-0 flex-1">
            <div class="truncate text-sm font-medium text-text">{h.label}</div>
            <div class="truncate text-xs text-faint">
              {destinationLabel(h)}
              · {h.auth === "key" ? "clave" : "agent"}
              {#if flagsFor(h.id)?.hasPassphrase}
                · passphrase
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
              {testingId === h.id ? "Probando…" : "Probar"}
            </Button>
            <Button variant="ghost" size="sm" onclick={() => startEdit(h)}>
              Editar
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onclick={() => void removeHost(h.id)}
            >
              Borrar
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
        Agregar host
      </Button>
    </div>
  {:else}
    {@render hostForm(draft)}
  {/if}
</SettingsGroup>

{#snippet hostForm(d: SshHost)}
  <SettingsRow label="Etiqueta">
    {#snippet control({ id })}
      <Input {id} bind:value={d.label} placeholder="prod-api" />
    {/snippet}
  </SettingsRow>
  <SettingsRow
    label="Usuario"
    hint="Opcional con alias de ssh_config (vacío = lo define el config)."
  >
    {#snippet control({ id })}
      <Input {id} bind:value={d.user} placeholder="root (o vacío)" />
    {/snippet}
  </SettingsRow>
  <SettingsRow
    label="Host"
    hint="IP, hostname, o alias Host de ~/.ssh/config (p.ej. contabo)."
  >
    {#snippet control({ id })}
      <Input
        {id}
        bind:value={d.host}
        placeholder="contabo o 10.0.0.5"
      />
    {/snippet}
  </SettingsRow>
  <SettingsRow
    label="Puerto"
    hint="Vacío = no pasar -p (usa ssh_config o el default 22)."
  >
    {#snippet control({ id })}
      <Input
        {id}
        type="number"
        min="0"
        max="65535"
        value={d.port > 0 ? String(d.port) : ""}
        placeholder="auto"
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
  <SettingsRow label="Autenticación">
    {#snippet control({ id })}
      <Select
        {id}
        value={d.auth}
        options={[
          { value: "agent", label: "ssh-agent (recomendado)" },
          { value: "key", label: "Identity file" },
        ]}
        onchange={(e: Event) => {
          d.auth = (e.currentTarget as HTMLSelectElement).value;
        }}
      />
    {/snippet}
  </SettingsRow>
  {#if d.auth === "key"}
    <SettingsRow label="Identity file" hint="Ruta al .pem / id_ed25519">
      {#snippet control()}
        <div class="flex min-w-0 flex-col gap-1">
          <Input
            readonly
            mono
            value={d.identity_file ?? ""}
            placeholder="Sin archivo"
          />
          <Button
            variant="ghost"
            size="sm"
            full
            onclick={() => void pickIdentity()}
          >
            Elegir…
          </Button>
        </div>
      {/snippet}
    </SettingsRow>
    <SettingsRow
      label="Passphrase"
      hint={flagsFor(d.id)?.hasPassphrase
        ? "Guardada en el llavero; dejá vacío para no cambiar."
        : "Opcional; va al llavero."}
    >
      {#snippet control({ id })}
        <div class="flex min-w-0 flex-col gap-1">
          <Input
            {id}
            type="password"
            autocomplete="new-password"
            placeholder={flagsFor(d.id)?.hasPassphrase
              ? "••••••••"
              : "Opcional"}
            bind:value={passphrase}
          />
          {#if flagsFor(d.id)?.hasPassphrase}
            <Button
              variant="ghost"
              size="sm"
              full
              onclick={() => void clearPassphrase(d.id)}
            >
              Borrar del llavero
            </Button>
          {/if}
        </div>
      {/snippet}
    </SettingsRow>
  {/if}
  <SettingsRow label="Cwd remoto" hint="Directorio por defecto en el host.">
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
  <SettingsRow label="Binario remoto" hint="Comando del agente en el PATH remoto.">
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
    <Button variant="ghost" size="sm" onclick={cancelEdit}>Cancelar</Button>
    <Button variant="primary" size="sm" onclick={() => void saveDraft()}>
      Listo
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
