<script lang="ts">
  /** Captura de atajo por teclas o botones laterales del mouse. */

  let {
    value = "CmdOrCtrl+Shift+R",
    defaultValue = "CmdOrCtrl+Shift+R",
    compact = false,
    ariaLabel = "Cambiar atajo",
    onChange,
  }: {
    value?: string;
    defaultValue?: string;
    compact?: boolean;
    ariaLabel?: string;
    onChange: (shortcut: string) => void | Promise<void>;
  } = $props();

  let capturing = $state(false);
  /** Aviso breve (p. ej. tecla Windows reservada por el SO). */
  let rejectHint = $state<string | null>(null);
  let rejectTimer: ReturnType<typeof setTimeout> | null = null;

  function showReject(msg: string) {
    rejectHint = msg;
    if (rejectTimer) clearTimeout(rejectTimer);
    rejectTimer = setTimeout(() => {
      rejectHint = null;
      rejectTimer = null;
    }, 3200);
  }

  function displayParts(raw: string): string[] {
    if (raw === "MouseX1") return ["Mouse atrás"];
    if (raw === "MouseX2") return ["Mouse adelante"];
    return raw
      .replace(/CmdOrCtrl/gi, "Ctrl")
      .replace(/CommandOrControl/gi, "Ctrl")
      .replace(/Super/gi, "Win")
      .split("+")
      .map((p) => p.trim())
      .filter(Boolean);
  }

  function keyEventToShortcut(e: KeyboardEvent): string | null {
    if (["Control", "Shift", "Alt", "Meta", "OS"].includes(e.key)) return null;

    // Win la reserva el SO (Inicio, Win+D, etc.). No registrar atajos con Super.
    if (e.metaKey) {
      showReject(
        "Win la usa Windows; prueba Ctrl/Alt/Shift o un botón lateral.",
      );
      return null;
    }

    const out: string[] = [];
    if (e.ctrlKey) out.push("CmdOrCtrl");
    if (e.altKey) out.push("Alt");
    if (e.shiftKey) out.push("Shift");

    let key = e.key;
    if (key === " ") key = "Space";
    else if (key.length === 1) key = key.toUpperCase();
    else if (/^F\d{1,2}$/i.test(key)) key = key.toUpperCase();

    if (out.length === 0 && !/^F\d{1,2}$/i.test(key)) return null;
    out.push(key);
    return out.join("+");
  }

  /** Botones laterales: 3 = atrás (X1), 4 = adelante (X2). */
  function mouseEventToShortcut(e: MouseEvent): string | null {
    if (e.button === 3) return "MouseX1";
    if (e.button === 4) return "MouseX2";
    return null;
  }

  function commit(sc: string) {
    capturing = false;
    rejectHint = null;
    // Diferir bindings de mouse: el mismo pulsado que completa la captura no
    // debe activar set_config → set_bindings en el mismo DOWN/UP del lateral.
    const isMouse = sc === "MouseX1" || sc === "MouseX2";
    if (isMouse) {
      setTimeout(() => void onChange(sc), 120);
    } else {
      void onChange(sc);
    }
  }

  function onKey(e: KeyboardEvent) {
    if (!capturing) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      capturing = false;
      return;
    }
    const sc = keyEventToShortcut(e);
    if (!sc) return;
    commit(sc);
  }

  function onMouse(e: MouseEvent) {
    if (!capturing) return;
    const sc = mouseEventToShortcut(e);
    if (!sc) return;
    // Solo consumir laterales; nunca interferir con clic izquierdo/derecho.
    e.preventDefault();
    e.stopPropagation();
    commit(sc);
  }

  function startCapture() {
    rejectHint = null;
    capturing = true;
  }

  function cancel() {
    capturing = false;
  }

  $effect(() => {
    if (!capturing) return;
    const onBlur = () => {
      capturing = false;
    };
    window.addEventListener("keydown", onKey, true);
    // mousedown + mouseup: algunos hosts solo entregan uno de los dos para X1/X2.
    window.addEventListener("mousedown", onMouse, true);
    window.addEventListener("mouseup", onMouse, true);
    window.addEventListener("auxclick", onMouse, true);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("mousedown", onMouse, true);
      window.removeEventListener("mouseup", onMouse, true);
      window.removeEventListener("auxclick", onMouse, true);
      window.removeEventListener("blur", onBlur);
    };
  });

  const keys = $derived(displayParts(value));
</script>

<div class="flex flex-wrap items-center gap-1.5" class:gap-2={!compact}>
  <button
    type="button"
    class="rb-hotkey {capturing ? 'is-capturing' : ''} {compact ? 'is-compact' : ''}"
    onclick={() => (capturing ? cancel() : startCapture())}
    aria-label={ariaLabel}
  >
    {#if capturing}
      <span class="rb-hotkey-pulse"
        >{compact ? "…" : "Tecla o botón lateral…"}</span
      >
      {#if !compact}
        <span class="rb-hint !text-[10px]">Esc cancela</span>
      {/if}
    {:else}
      <span class="rb-kbd inline-flex">
        {#each keys as key, i (i)}
          {#if i > 0}<span class="opacity-40">+</span>{/if}
          <kbd>{key}</kbd>
        {/each}
      </span>
      {#if !compact}
        <span class="rb-hint !text-[10px]">Clic para cambiar</span>
      {/if}
    {/if}
  </button>
  {#if value !== defaultValue && !capturing && !compact}
    <button
      type="button"
      class="rb-btn rb-btn-ghost !text-xs"
      onclick={() => onChange(defaultValue)}
    >
      Restablecer
    </button>
  {/if}
</div>
{#if rejectHint}
  <p class="rb-hint mt-1 w-full" style="color: var(--rb-warn)" role="status">
    {rejectHint}
  </p>
{/if}
