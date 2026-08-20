<script lang="ts">
  /**
   * Lo que se está diciendo, mientras se graba.
   *
   * Solo aparece con la transcripción en vivo activada, y solo mientras dura la
   * grabación: es una confirmación de que el audio entra, no un documento. Lo
   * que queda guardado es la transcripción del archivo, que se hace después y
   * con más contexto.
   */
  import { capture } from "$domain/capture.svelte";
  import Chip from "$ui/Chip.svelte";
  import { t } from "$domain/i18n.svelte";

  /**
   * Sigue al último renglón.
   *
   * Se ancla abajo porque lo último dicho es lo único que importa acá; sin esto
   * hay que arrastrar la lista cada pocos segundos. Va con `$effect` y no con
   * `queueMicrotask` en cada evento para que el desplazamiento ocurra una sola
   * vez por cambio, ya renderizado.
   */
  function follow(node: HTMLElement) {
    $effect(() => {
      // Se leen para que el efecto vuelva a correr cuando llega texto nuevo.
      void capture.segments.length;
      void capture.partial;
      node.scrollTo({ top: node.scrollHeight, behavior: "smooth" });
    });
  }

  function who(speaker: "me" | "others"): string {
    return speaker === "me" ? t("page.meetings.me") : t("page.meetings.others");
  }
</script>

{#if capture.active && (capture.segments.length > 0 || capture.partial || capture.liveError)}
  <!-- El espaciado va acá y no en un envoltorio: la herramienta no tiene forma
       de saber si esto dibuja algo, y un `<div>` con padding y nada adentro deja
       un hueco sin explicación. -->
  <section class="flex flex-col gap-1.5 px-4 pt-3" aria-label={t("page.meetings.live")}>
    <div class="flex items-center gap-2">
      <span class="text-micro text-faint uppercase">{t("page.meetings.liveChip")}</span>
      {#if capture.liveError}
        <Chip tone="warn">{t("page.meetings.liveFail")}</Chip>
      {:else}
        <Chip tone="ok">{t("page.meetings.liveOk")}</Chip>
      {/if}
    </div>

    <ul
      class="flex max-h-28 list-none flex-col gap-1 overflow-y-auto rounded-sm
             bg-surface-2 px-3 py-2"
      aria-live="polite"
      {@attach follow}
    >
      {#each capture.segments as segment, index (`${segment.speaker}-${segment.start_ms}-${index}`)}
        <li class="flex gap-2 text-sm">
          <span
            class="w-10 shrink-0 text-micro uppercase
                   {segment.speaker === 'me' ? 'text-mic' : 'text-sys'}"
          >
            {who(segment.speaker)}
          </span>
          <span class="min-w-0 text-text">{segment.text}</span>
        </li>
      {/each}

      {#if capture.partial}
        <!-- Lo parcial se atenúa: Whisper todavía puede corregirlo, y verlo
             cambiar sin aviso se lee como un error. -->
        <li class="flex gap-2 text-sm opacity-60">
          <span
            class="w-10 shrink-0 text-micro uppercase
                   {capture.partial.speaker === 'me' ? 'text-mic' : 'text-sys'}"
          >
            {who(capture.partial.speaker)}
          </span>
          <span class="min-w-0 text-muted">{capture.partial.text}</span>
        </li>
      {/if}
    </ul>
  </section>
{/if}
