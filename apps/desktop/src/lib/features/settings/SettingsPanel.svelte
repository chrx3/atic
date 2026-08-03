<script lang="ts">
  /**
   * Ajustes.
   *
   * Reemplaza a un modal de 2.012 líneas donde las siete secciones estaban una
   * detrás de otra en el mismo archivo. Acá cada una es un componente que se
   * entiende solo, y el panel únicamente decide cuál se ve.
   *
   * Tres secciones siguen en la UI vieja y se dicen a la cara: Atajos necesita
   * la captura de combinaciones, Audio la enumeración de dispositivos y
   * Resúmenes el manejo de claves de proveedor. Prometer una pantalla vacía
   * sería peor que mandar a la que funciona.
   */
  import SettingsNav from "$patterns/SettingsNav.svelte";
  import Banner from "$ui/Banner.svelte";
  import CapturesSection from "./CapturesSection.svelte";
  import DictationSection from "./DictationSection.svelte";
  import GeneralSection from "./GeneralSection.svelte";
  import MeetingsSection from "./MeetingsSection.svelte";

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

  const PENDING: Record<string, string> = {
    shortcuts: "La captura de combinaciones de teclas todavía no está reescrita.",
    audio: "La lista de micrófonos y salidas todavía no está reescrita.",
    summary: "El manejo de claves de los proveedores todavía no está reescrito.",
  };

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
    {:else}
      <Banner tone="info" title={PENDING[section]}>
        Volvé a la interfaz anterior con Ctrl+Alt+M para cambiarlo.
      </Banner>
    {/if}
  </div>
</div>
