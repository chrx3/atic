<script lang="ts">
  /**
   * Un aviso persistente dentro de la pantalla. Reemplaza a `.rb-setup-notice`
   * y `.rb-banner`, que aparecen seis veces solo en la ventana principal.
   *
   * A diferencia de un toast, esto no se va solo: describe una condición que
   * sigue siendo verdad —falta un modelo, falta una clave— y normalmente trae
   * la acción que la resuelve.
   */
  import type { Snippet } from "svelte";

  type Tone = "info" | "warn" | "danger";

  let {
    tone = "info",
    title,
    action,
    children,
  }: {
    tone?: Tone;
    title: string;
    action?: Snippet;
    children?: Snippet;
  } = $props();

  const TONES: Record<Tone, string> = {
    info: "bg-info-soft text-info",
    warn: "bg-warn-soft text-warn",
    danger: "bg-danger-soft text-danger",
  };
</script>

<div
  class="flex items-start gap-3 rounded-sm px-3 py-2 {TONES[tone]}"
  role={tone === "danger" ? "alert" : "status"}
>
  <div class="flex min-w-0 flex-1 flex-col gap-0.5">
    <p class="text-sm font-medium">{title}</p>
    {#if children}
      <div class="text-xs opacity-80">{@render children()}</div>
    {/if}
  </div>
  {#if action}
    <div class="shrink-0">{@render action()}</div>
  {/if}
</div>
