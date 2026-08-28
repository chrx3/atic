<script lang="ts">
  /**
   * Ajustes.
   *
   * Reemplaza a un modal de 2.012 líneas donde las secciones estaban una
   * detrás de otra en el mismo archivo. Acá cada una es un componente que se
   * entiende solo, y el panel únicamente decide cuál se ve.
   */
  import { untrack } from "svelte";
  import { tabPanel } from "$lib/motion";
  import SettingsNav from "$patterns/SettingsNav.svelte";
  import type { IconId } from "$lib/ToolIcon.svelte";
  import AgentsSection from "./AgentsSection.svelte";
  import AudioSection from "./AudioSection.svelte";
  import AboutSection from "./AboutSection.svelte";
  import CapturesSection from "./CapturesSection.svelte";
  import DictationSection from "./DictationSection.svelte";
  import GeneralSection from "./GeneralSection.svelte";
  import LauncherSection from "./LauncherSection.svelte";
  import MeetingsSection from "./MeetingsSection.svelte";
  import PillSection from "./PillSection.svelte";
  import ShortcutsSection from "./ShortcutsSection.svelte";
  import SummarySection from "./SummarySection.svelte";
  import type { SettingsSectionId } from "./settingsSections";
  import { AGENTS_ENABLED, AGENT_PAGER_ENABLED } from "$core/tools";
  import { t } from "$domain/i18n.svelte";

  type SectionId = SettingsSectionId;

  const SHOW_AGENTS = AGENTS_ENABLED || AGENT_PAGER_ENABLED;

  const sections = $derived([
    { value: "general" as const, label: t("settings.nav.general"), icon: "general" as IconId },
    { value: "meetings" as const, label: t("settings.nav.meetings"), icon: "meetings" as IconId },
    { value: "dictation" as const, label: t("settings.nav.dictation"), icon: "dictation" as IconId },
    { value: "captures" as const, label: t("settings.nav.captures"), icon: "captures" as IconId },
    { value: "shortcuts" as const, label: t("settings.nav.shortcuts"), icon: "shortcuts" as IconId },
    { value: "pill" as const, label: t("settings.nav.pill"), icon: "pill" as IconId },
    { value: "launcher" as const, label: t("settings.nav.launcher"), icon: "launcher" as IconId },
    { value: "audio" as const, label: t("settings.nav.audio"), icon: "audio" as IconId },
    { value: "summary" as const, label: t("settings.nav.summary"), icon: "summary" as IconId },
    ...(SHOW_AGENTS
      ? [{ value: "agents" as const, label: t("settings.nav.agents"), icon: "agents" as IconId }]
      : []),
    { value: "about" as const, label: t("settings.nav.about"), icon: "about" as IconId },
  ]);

  let {
    initialSection = "general",
  }: {
    /** Sección inicial (p. ej. deep-link desde la consola de agentes). */
    initialSection?: SectionId;
  } = $props();

  // Solo seed: MainSurface remonta con `{#key settingsSection}` al deep-linkear.
  let section = $state<SectionId>(
    untrack(() =>
      !SHOW_AGENTS && initialSection === "agents" ? "general" : initialSection,
    ),
  );
</script>

<!--
  `min-h-0 overflow-hidden` cierra la cadena de altura: sin eso el flex crece
  con el contenido y el scroll se va al modal entero. Siempre en fila: el
  container query vive acá y SettingsNav (descendiente) decide tabs vs sidebar.
  No poner `@md/settings:*` en este mismo nodo — un contenedor no se consulta
  a sí mismo.
-->
<div class="@container/settings flex h-full min-h-0 overflow-hidden">
  <SettingsNav bind:value={section} sections={sections} />

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
        {:else if section === "pill"}
          <PillSection />
        {:else if section === "launcher"}
          <LauncherSection />
        {:else if section === "audio"}
          <AudioSection />
        {:else if section === "summary"}
          <SummarySection />
        {:else if SHOW_AGENTS && section === "agents"}
          <AgentsSection />
        {:else if section === "about"}
          <AboutSection />
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
