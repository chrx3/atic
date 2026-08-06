<script lang="ts">
  /**
   * Ajustes.
   *
   * Reemplaza a un modal de 2.012 líneas donde las siete secciones estaban una
   * detrás de otra en el mismo archivo. Acá cada una es un componente que se
   * entiende solo, y el panel únicamente decide cuál se ve.
   *
   * Las siete están reescritas.
   */
  import { tabPanel } from "$lib/motion";
  import SettingsNav from "$patterns/SettingsNav.svelte";
  import type { IconId } from "$lib/ToolIcon.svelte";
  import AudioSection from "./AudioSection.svelte";
  import CapturesSection from "./CapturesSection.svelte";
  import DictationSection from "./DictationSection.svelte";
  import GeneralSection from "./GeneralSection.svelte";
  import MeetingsSection from "./MeetingsSection.svelte";
  import ShortcutsSection from "./ShortcutsSection.svelte";
  import SummarySection from "./SummarySection.svelte";

  type SectionId =
    | "general"
    | "meetings"
    | "dictation"
    | "captures"
    | "shortcuts"
    | "audio"
    | "summary";

  const SECTIONS: { value: SectionId; label: string; icon: IconId }[] = [
    { value: "general", label: "General", icon: "general" },
    { value: "meetings", label: "Reuniones", icon: "meetings" },
    { value: "dictation", label: "Dictado", icon: "dictation" },
    { value: "captures", label: "Capturas", icon: "captures" },
    { value: "shortcuts", label: "Atajos", icon: "shortcuts" },
    { value: "audio", label: "Audio", icon: "audio" },
    { value: "summary", label: "Resúmenes", icon: "summary" },
  ];

  let section = $state<SectionId>("general");
</script>

<!--
  `min-h-0 overflow-hidden` cierra la cadena de altura: sin eso el flex crece
  con el contenido y el scroll se va al modal entero. Siempre en fila: el
  container query vive acá y SettingsNav (descendiente) decide tabs vs sidebar.
  No poner `@md/settings:*` en este mismo nodo — un contenedor no se consulta
  a sí mismo.
-->
<div class="@container/settings flex h-full min-h-0 overflow-hidden">
  <SettingsNav bind:value={section} sections={SECTIONS} />

  <div class="settings-stage min-h-0 flex-1 overflow-y-auto p-4">
    {#key section}
      <div class="settings-pane" in:tabPanel|local out:tabPanel|local>
        {#if section === "general"}
          <GeneralSection />
        {:else if section === "meetings"}
          <MeetingsSection />
        {:else if section === "dictation"}
          <DictationSection />
        {:else if section === "captures"}
          <CapturesSection />
        {:else if section === "shortcuts"}
          <ShortcutsSection />
        {:else if section === "audio"}
          <AudioSection />
        {:else}
          <SummarySection />
        {/if}
      </div>
    {/key}
  </div>
</div>

<style>
  .settings-stage {
    position: relative;
    /* La altura la fija el modal (`fill`); acá solo scrollea el contenido. */
    height: 100%;
  }

  .settings-stage :global(.settings-pane) {
    transform-origin: 50% 0;
  }
</style>
