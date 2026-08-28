<script lang="ts">
  /**
   * Las cuatro perillas del tema personalizado.
   *
   * No hay vista previa: la app ENTERA es la vista previa. Por eso el `input`
   * pinta en el acto —24 propiedades en el root, sale gratis— y el `change`
   * persiste. Si guardara en cada `input`, arrastrar un slider escribiría el
   * archivo de config cien veces.
   */
  import Select from "$ui/Select.svelte";
  import Button from "$ui/Button.svelte";
  import { t } from "$domain/i18n.svelte";
  import {
    BASE_THEMES,
    readPalette,
    seedKnobs,
    setCustomKnobs,
    type ThemeKnobs,
  } from "$lib/theme";

  let {
    knobs,
    onchange,
  }: {
    knobs: ThemeKnobs;
    /** Al soltar: persistir en config. */
    onchange: (knobs: ThemeKnobs) => void;
  } = $props();

  const baseOptions = BASE_THEMES.map((value) => ({
    value: value as string,
    label: t(`settings.appearance.${value}`),
  }));

  /** Pinta ya; el `change` de cada control es el que guarda. */
  function preview(patch: Partial<ThemeKnobs>) {
    knobs = setCustomKnobs({ ...knobs, ...patch });
  }

  function commit(patch: Partial<ThemeKnobs> = {}) {
    preview(patch);
    onchange(knobs);
  }

  /**
   * Cambiar de base recarga las perillas con las de ESE tema.
   *
   * Mantener el papel y la temperatura anteriores sobre otro base daría un
   * salto sin sentido: lo que el usuario espera es empezar de nuevo desde el
   * tema que acaba de elegir.
   */
  function setBase(base: string) {
    commit(seedKnobs(base, readPalette(base)));
  }

  const number = (event: Event) =>
    Number((event.currentTarget as HTMLInputElement).value);
</script>

<div class="flex flex-col gap-3 rounded-sm border border-line bg-surface-2 p-3">
  <label class="flex items-center justify-between gap-3">
    <span class="text-xs text-muted">{t("settings.appearance.editor.base")}</span>
    <span class="w-40 shrink-0">
      <Select
        value={knobs.base}
        options={baseOptions}
        aria-label={t("settings.appearance.editor.base")}
        onchange={(event: Event) =>
          setBase((event.currentTarget as HTMLSelectElement).value)}
      />
    </span>
  </label>

  {#each [{ key: "paper", min: -100, max: 100 }, { key: "ink", min: 0, max: 100 }, { key: "warmth", min: -100, max: 100 }] as knob (knob.key)}
    <label class="flex items-center gap-3">
      <span class="w-20 shrink-0 text-xs text-muted">
        {t(`settings.appearance.editor.${knob.key}`)}
      </span>
      <input
        type="range"
        class="theme-range min-w-0 flex-1"
        min={knob.min}
        max={knob.max}
        value={knobs[knob.key as "paper" | "ink" | "warmth"]}
        oninput={(event) => preview({ [knob.key]: number(event) })}
        onchange={(event) => commit({ [knob.key]: number(event) })}
      />
      <output class="w-8 shrink-0 text-right text-xs text-faint">
        {knobs[knob.key as "paper" | "ink" | "warmth"]}
      </output>
    </label>
  {/each}

  <label class="flex items-center gap-3">
    <span class="w-20 shrink-0 text-xs text-muted">
      {t("settings.appearance.editor.accent")}
    </span>
    <input
      type="color"
      class="theme-color"
      value={knobs.accent}
      oninput={(event) =>
        preview({ accent: (event.currentTarget as HTMLInputElement).value })}
      onchange={(event) =>
        commit({ accent: (event.currentTarget as HTMLInputElement).value })}
    />
    <output class="font-mono text-xs text-faint">{knobs.accent}</output>
    <span class="ml-auto">
      <Button variant="ghost" size="sm" onclick={() => setBase(knobs.base)}>
        {t("settings.appearance.editor.reset")}
      </Button>
    </span>
  </label>
</div>

<style>
  /*
   * El `range` nativo no se puede estilar sin `appearance: none`, y a partir de
   * ahí hay que dibujar riel y pulgar a mano. Los dos motores de WebView2 usan
   * el pseudo-elemento `-webkit-`.
   */
  .theme-range {
    appearance: none;
    height: 18px;
    background: transparent;
    cursor: pointer;
  }

  .theme-range::-webkit-slider-runnable-track {
    height: 3px;
    border-radius: var(--radius-pill);
    background: var(--line-strong);
  }

  .theme-range::-webkit-slider-thumb {
    appearance: none;
    width: 12px;
    height: 12px;
    margin-top: -4.5px;
    border-radius: var(--radius-pill);
    background: var(--accent);
    box-shadow: var(--shadow-card);
  }

  .theme-range:focus-visible::-webkit-slider-thumb {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  /* El color picker nativo trae borde y padding propios en cada plataforma. */
  .theme-color {
    width: 28px;
    height: 22px;
    padding: 0;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-xs);
    background: transparent;
    cursor: pointer;
  }

  .theme-color::-webkit-color-swatch-wrapper {
    padding: 2px;
  }

  .theme-color::-webkit-color-swatch {
    border: none;
    border-radius: 3px;
  }
</style>
