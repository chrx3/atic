<script lang="ts">
  /**
   * Qué herramientas viven en la pill (Ajustes → Pill).
   *
   * Tres cubos y no dos interruptores: en una rueda radial el costo de una
   * herramienta no es que ocupe sitio, es que le achica el ángulo a todas las
   * demás. Por eso hay un escalón intermedio —«Más», un segundo anillo— entre
   * tenerla a un golpe y no tenerla: lo que se usa poco deja de competir por
   * ángulo sin tener que desaparecer.
   *
   * El orden importa y por eso se puede cambiar: la posición de un gajo es lo
   * que se aprende con la mano, no su icono.
   */
  import { pillLayout } from "$core/pillTools";
  import type { ToolDef } from "$core/tools";
  import { config } from "$domain/config.svelte";
  import { localizeTool, t } from "$domain/i18n.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import { ChevronDown, ChevronUp } from "$lib/icons";
  import ToolIcon from "$lib/ToolIcon.svelte";
  import SettingsGroup from "$patterns/SettingsGroup.svelte";
  import Button from "$ui/Button.svelte";
  import Icon from "$ui/Icon.svelte";
  import IconButton from "$ui/IconButton.svelte";
  import Select from "$ui/Select.svelte";

  type Bucket = "ring" | "more" | "hidden";

  const cfg = $derived(config.current);
  const layout = $derived(pillLayout(cfg?.pill_tools, cfg?.pill_more_tools));

  /**
   * La rueda no puede quedar vacía.
   *
   * `pillLayout` ya se defiende de una config así —sube el submenú, o vuelve
   * al catálogo entero—, pero eso desharía en silencio lo que el usuario acaba
   * de hacer. Es mejor que el último gajo no se pueda mover.
   */
  const ringLocked = $derived(layout.ring.length <= 1);

  const places = $derived([
    { value: "ring" as const, label: t("settings.pill.placeRing") },
    { value: "more" as const, label: t("settings.pill.placeMore") },
    { value: "hidden" as const, label: t("settings.pill.placeHidden") },
  ]);

  const sections = $derived([
    {
      bucket: "ring" as const,
      title: t("settings.pill.ring"),
      hint: t("settings.pill.ringHint"),
      tools: layout.ring,
    },
    {
      bucket: "more" as const,
      title: t("settings.pill.more"),
      hint: t("settings.pill.moreHint"),
      tools: layout.more,
    },
    {
      bucket: "hidden" as const,
      title: t("settings.pill.hidden"),
      hint: t("settings.pill.hiddenHint"),
      tools: layout.hidden,
    },
  ]);

  async function save(ring: ToolDef[], more: ToolDef[]) {
    try {
      await config.patch({
        pill_tools: ring.map((tool) => tool.id),
        pill_more_tools: more.map((tool) => tool.id),
      });
    } catch (error) {
      toastError(error);
    }
  }

  function without(tools: ToolDef[], id: string): ToolDef[] {
    return tools.filter((tool) => tool.id !== id);
  }

  function moveTo(tool: ToolDef, to: Bucket) {
    const ring = without(layout.ring, tool.id);
    const more = without(layout.more, tool.id);
    if (to === "ring") void save([...ring, tool], more);
    else if (to === "more") void save(ring, [...more, tool]);
    else void save(ring, more);
  }

  /** Un paso arriba o abajo dentro de su propio cubo. */
  function reorder(bucket: Exclude<Bucket, "hidden">, index: number, delta: number) {
    const list = bucket === "ring" ? [...layout.ring] : [...layout.more];
    const next = index + delta;
    if (next < 0 || next >= list.length) return;
    [list[index], list[next]] = [list[next], list[index]];
    if (bucket === "ring") void save(list, layout.more);
    else void save(layout.ring, list);
  }
</script>

{#if cfg}
  <div class="flex flex-col gap-5">
    <SettingsGroup title={t("settings.pill.title")} hint={t("settings.pill.hint")}>
      {#each sections as section (section.bucket)}
        <section class="bucket">
          <header class="bucket-head">
            <h4 class="text-sm font-medium text-text">
              {section.title}
              <span class="ml-1 text-xs text-faint tabular-nums">
                {section.tools.length}
              </span>
            </h4>
            <p class="text-xs text-faint">{section.hint}</p>
          </header>

          {#if section.tools.length === 0}
            <p class="py-1 text-xs text-faint">{t("settings.pill.none")}</p>
          {:else}
            <ul class="flex flex-col divide-y divide-line">
              {#each section.tools as tool, index (tool.id)}
                {@const shown = localizeTool(tool)}
                {@const locked = section.bucket === "ring" && ringLocked}
                <li class="row">
                  <span class="row-ico" aria-hidden="true">
                    <ToolIcon id={tool.id} size={15} strokeWidth={1.5} />
                  </span>
                  <span class="min-w-0 flex-1 truncate text-sm text-text">
                    {shown.label}
                  </span>

                  {#if section.bucket !== "hidden"}
                    <span class="flex shrink-0 items-center gap-0.5">
                      <IconButton
                        label={t("settings.pill.up", { label: shown.label })}
                        size="sm"
                        disabled={index === 0}
                        onclick={() => reorder(section.bucket, index, -1)}
                      >
                        <Icon icon={ChevronUp} size={12} />
                      </IconButton>
                      <IconButton
                        label={t("settings.pill.down", { label: shown.label })}
                        size="sm"
                        disabled={index === section.tools.length - 1}
                        onclick={() => reorder(section.bucket, index, 1)}
                      >
                        <Icon icon={ChevronDown} size={12} />
                      </IconButton>
                    </span>
                  {/if}

                  <span class="w-40 shrink-0">
                    <Select
                      value={section.bucket}
                      options={places}
                      disabled={locked}
                      aria-label={t("settings.pill.place", { label: shown.label })}
                      onchange={(event: Event) =>
                        moveTo(
                          tool,
                          (event.currentTarget as HTMLSelectElement).value as Bucket,
                        )}
                    />
                  </span>
                </li>
              {/each}
            </ul>
          {/if}
        </section>
      {/each}

      {#if ringLocked}
        <p class="pt-2 text-xs text-warn">{t("settings.pill.lastOne")}</p>
      {/if}

      <p class="pt-2 text-xs text-faint">{t("settings.pill.stripNote")}</p>

      <div class="pt-2">
        <Button variant="soft" size="sm" onclick={() => void save([], [])}>
          {t("settings.pill.reset")}
        </Button>
      </div>
    </SettingsGroup>
  </div>
{/if}

<style>
  .bucket {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    padding: 0.5rem 0;
  }

  .bucket-head {
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.375rem 0;
  }

  .row-ico {
    display: grid;
    place-items: center;
    width: 1.75rem;
    height: 1.75rem;
    flex-shrink: 0;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--muted);
  }
</style>
