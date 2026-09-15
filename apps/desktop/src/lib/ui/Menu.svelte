<script lang="ts" generics="T extends string">
  /**
   * Menú desplegable con su disparador.
   *
   * Es el patrón que macOS usa para todo lo que no merece estar a la vista:
   * cambiar de herramienta, el desborde de acciones de un ítem, filtrar. Un
   * `<select>` nativo no sirve acá porque el disparador no es un campo —es el
   * título de la ventana, un botón de icono, una pastilla— y porque el sistema
   * pinta el desplegable con su propio tema.
   *
   * Teclado: `↑`/`↓` recorren, `Inicio`/`Fin` saltan, `Esc` cierra y devuelve
   * el foco al disparador. El foco es real (no `aria-activedescendant`), así
   * que un lector de pantalla lee cada ítem al pasar.
   */
  import type { Snippet } from "svelte";
  import { tick } from "svelte";
  import Icon from "$ui/Icon.svelte";
  import type { MenuItem } from "$ui/menu";
  import { Check } from "$lib/icons";

  let {
    items,
    onpick,
    label,
    trigger,
    align = "start",
    triggerClass = "",
    triggerLabel,
    pressed = false,
  }: {
    items: MenuItem<T>[];
    onpick: (id: T) => void;
    /** Nombre del menú para lectores de pantalla. */
    label: string;
    /** Contenido del disparador: icono, texto, chevron. */
    trigger: Snippet;
    align?: "start" | "end";
    /** Clases del disparador (cada uso lo viste distinto). */
    triggerClass?: string;
    /** Se suma al `label` del menú cuando el trigger es solo un icono. */
    triggerLabel?: string;
    /** El menú deja algo encendido (p. ej. un filtro activo). */
    pressed?: boolean;
  } = $props();

  const id = $props.id();
  let open = $state(false);
  let triggerEl = $state<HTMLButtonElement | null>(null);
  let menuEl = $state<HTMLDivElement | null>(null);

  /** Los ítems con foco posible, en orden de lectura. */
  function enabled(): HTMLElement[] {
    return [
      ...(menuEl?.querySelectorAll<HTMLElement>("[data-item]:not(:disabled)") ?? []),
    ];
  }

  async function openMenu() {
    open = true;
    await tick();
    enabled()[0]?.focus();
  }

  function close(focusTrigger = true) {
    open = false;
    if (focusTrigger) triggerEl?.focus();
  }

  function pick(item: MenuItem<T>) {
    if (item.disabled) return;
    close();
    onpick(item.id);
  }

  function onMenuKeydown(event: KeyboardEvent) {
    const nodes = enabled();
    const at = nodes.indexOf(document.activeElement as HTMLElement);
    if (event.key === "ArrowDown") {
      event.preventDefault();
      nodes[(at + 1) % nodes.length]?.focus();
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      nodes[(at - 1 + nodes.length) % nodes.length]?.focus();
    } else if (event.key === "Home") {
      event.preventDefault();
      nodes[0]?.focus();
    } else if (event.key === "End") {
      event.preventDefault();
      nodes.at(-1)?.focus();
    } else if (event.key === "Tab") {
      close(false);
    } else if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  }

  // Cerrar al tocar afuera. El `<select>` nativo lo hace solo; acá es a mano
  // porque el menú es un popover, no un control del sistema.
  $effect(() => {
    if (!open) return;
    const onOutside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!menuEl?.contains(target) && !triggerEl?.contains(target)) open = false;
    };
    window.addEventListener("pointerdown", onOutside);
    return () => window.removeEventListener("pointerdown", onOutside);
  });
</script>

<div class="relative">
  <button
    bind:this={triggerEl}
    type="button"
    aria-haspopup="menu"
    aria-expanded={open}
    aria-controls={open ? id : undefined}
    aria-pressed={pressed ? "true" : undefined}
    aria-label={triggerLabel}
    title={triggerLabel}
    onclick={() => (open ? close() : void openMenu())}
    class={triggerClass}
  >
    {@render trigger()}
  </button>

  {#if open}
    <div
      bind:this={menuEl}
      {id}
      role="menu"
      aria-label={label}
      tabindex="-1"
      onkeydown={onMenuKeydown}
      class="absolute top-[calc(100%+4px)] z-(--z-popover) min-w-48 rounded-sm border
             border-line bg-elevated p-1 shadow-pop
             {align === 'end' ? 'right-0' : 'left-0'}"
    >
      {#each items as item (item.id)}
        <button
          type="button"
          role="menuitem"
          data-item
          disabled={item.disabled}
          onclick={() => pick(item)}
          class="flex w-full items-center gap-2 rounded-xs px-2 py-1 text-left text-sm
                 whitespace-nowrap transition-colors duration-(--duration-quick) ease-calm
                 hover:bg-surface-2 focus-visible:bg-surface-2
                 focus-visible:[outline:2px_solid_var(--accent)]
                 focus-visible:outline-offset-[-2px]
                 disabled:pointer-events-none disabled:opacity-45
                 {item.danger ? 'text-danger' : 'text-text'}"
        >
          {#if item.icon}
            <span
              class="grid size-3.5 shrink-0 place-items-center text-muted"
              aria-hidden="true"
            >
              <Icon icon={item.icon} size={13} />
            </span>
          {/if}
          <span class="min-w-0 flex-1 truncate">{item.label}</span>
          {#if item.hint}
            <span class="text-micro text-faint">{item.hint}</span>
          {/if}
          {#if item.checked}
            <Icon icon={Check} size={12} class="shrink-0 text-text" />
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>
