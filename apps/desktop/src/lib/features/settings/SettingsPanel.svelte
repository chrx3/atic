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
  import SettingsNav from "$patterns/SettingsNav.svelte";
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

  const SECTIONS: { value: SectionId; label: string }[] = [
    { value: "general", label: "General" },
    { value: "meetings", label: "Reuniones" },
    { value: "dictation", label: "Dictado" },
    { value: "captures", label: "Capturas" },
    { value: "shortcuts", label: "Atajos" },
    { value: "audio", label: "Audio" },
    { value: "summary", label: "Resúmenes" },
  ];

  let section = $state<SectionId>("general");
</script>

<div class="@container/settings flex h-full">
  <SettingsNav bind:value={section} sections={SECTIONS} />

  <div class="min-h-0 flex-1 overflow-y-auto p-4">
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
</div>
