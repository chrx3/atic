<script lang="ts">
  /**
   * Capturar un atajo apretándolo.
   *
   * La lógica viene tal cual de la versión anterior porque cada rama de acá se
   * ganó contra un caso real: los botones laterales del mouse, la tecla Windows
   * que el SO se reserva, y el pulsado que completa la captura y que no puede
   * ser el mismo que dispare el guardado.
   */
  import Kbd from "./Kbd.svelte";

  let {
    value = "",
    defaultValue = "",
    ariaLabel = "Cambiar atajo",
    onChange,
  }: {
    value?: string;
    /** Para el botón de restablecer. Si es igual al valor, no se ofrece. */
    defaultValue?: string;
    ariaLabel?: string;
    onChange: (shortcut: string) => void | Promise<void>;
  } = $props();

  let capturing = $state(false);
  /** Aviso breve, p. ej. una tecla que el SO no deja tomar. */
  let rejected = $state<string | null>(null);
  let rejectTimer: ReturnType<typeof setTimeout> | null = null;

  function showReject(msg: string) {
    rejected = msg;
    if (rejectTimer) clearTimeout(rejectTimer);
    rejectTimer = setTimeout(() => {
      rejected = null;
      rejectTimer = null;
    }, 3200);
  }

  /** Cómo se muestra. Los botones del mouse no son teclas y se nombran. */
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
    // Un modificador solo no es un atajo: se espera a la tecla que lo acompaña.
    if (["Control", "Shift", "Alt", "Meta", "OS"].includes(e.key)) return null;

    // En Windows la tecla Win la reserva el SO y el registro global falla sin
    // decir por qué. Mejor rechazarla acá y explicarlo.
    const isWindows =
      typeof navigator !== "undefined" && /Win/i.test(navigator.userAgent);
    if (isWindows && e.metaKey) {
      showReject("Win la usa Windows. Probá con Ctrl, Alt o un botón lateral.");
      return null;
    }

    const out: string[] = [];
    if (e.ctrlKey || e.metaKey) out.push("CmdOrCtrl");
    if (e.altKey) out.push("Alt");
    if (e.shiftKey) out.push("Shift");

    let key = e.key;
    if (key === " ") key = "Space";
    else if (key.length === 1) key = key.toUpperCase();
    else if (/^F\d{1,2}$/i.test(key)) key = key.toUpperCase();

    // Sin modificador no hay atajo global posible, salvo las F.
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

  function commit(shortcut: string) {
    capturing = false;
    rejected = null;
    // Los del mouse se difieren: el mismo pulsado que completa la captura no
    // puede además disparar el registro del binding en el mismo DOWN/UP, o el
    // botón queda tomado a mitad del gesto.
    if (shortcut === "MouseX1" || shortcut === "MouseX2") {
      setTimeout(() => void onChange(shortcut), 120);
    } else {
      void onChange(shortcut);
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
    if (sc) commit(sc);
  }

  function onMouse(e: MouseEvent) {
    if (!capturing) return;
    const sc = mouseEventToShortcut(e);
    if (!sc) return;
    // Solo se consumen los laterales: interceptar el izquierdo dejaría la app
    // sin forma de cancelar.
    e.preventDefault();
    e.stopPropagation();
    commit(sc);
  }

  $effect(() => {
    if (!capturing) return;
    const onBlur = () => (capturing = false);
    // En captura, todo pasa por el window y en fase de captura: si no, el
    // atajo se lo come el control que tenga el foco.
    window.addEventListener("keydown", onKey, true);
    // Los tres, porque según el host llega uno u otro para X1/X2.
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

  const parts = $derived(displayParts(value));
  const canReset = $derived(Boolean(defaultValue) && value !== defaultValue);
</script>

<div class="flex flex-col gap-1">
  <div class="flex flex-wrap items-center gap-1.5">
    <button
      type="button"
      aria-label={ariaLabel}
      onclick={() => (capturing = !capturing)}
      class="inline-flex h-8 min-w-0 flex-1 items-center justify-center gap-1 rounded-sm
             border px-2
             transition-colors duration-(--duration-quick) ease-calm
             {capturing
        ? 'border-accent bg-surface-2'
        : 'border-line bg-surface-2 hover:bg-elevated'}"
    >
      {#if capturing}
        <span class="text-xs text-muted">Apretá la combinación…</span>
      {:else if parts.length === 0}
        <span class="text-xs text-faint">Sin asignar</span>
      {:else}
        <Kbd combo={parts.join("+")} />
      {/if}
    </button>

    {#if canReset && !capturing}
      <button
        type="button"
        class="text-xs text-muted underline-offset-2 hover:text-text hover:underline"
        onclick={() => void onChange(defaultValue)}
      >
        Restablecer
      </button>
    {/if}
  </div>

  {#if capturing}
    <p class="text-xs text-faint">
      Esc cancela. También sirve un botón lateral del mouse.
    </p>
  {/if}
  {#if rejected}
    <p class="text-xs text-warn" role="status">{rejected}</p>
  {/if}
</div>
