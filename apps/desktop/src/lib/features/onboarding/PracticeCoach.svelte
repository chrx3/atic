<script lang="ts">
  /**
   * Coach de primer uso, junto a la pill.
   *
   * El modal de setup ya se cerró: esta ventana no atrapa el foco y los atajos
   * globales siguen vivos. Cada paso espera el evento real —rueda, dictado,
   * historial— antes de seguir. Saltar un paso solo aparece si se traba.
   */
  import { formatShortcut } from "$core/format";
  import type { ToolId } from "$core/tools";
  import { config } from "$domain/config.svelte";
  import { sessionEffect } from "$domain/session";
  import { toastError } from "$domain/toasts.svelte";
  import { subscribe } from "$ipc/events";
  import { overlayWorkAreas, workAreaOf } from "$ipc/overlay";
  import { liveArea, surfaces } from "$surfaces/overlay/surfaces.svelte";
  import Button from "$ui/Button.svelte";
  import Kbd from "$ui/Kbd.svelte";
  import { PRACTICE_SKIP_AFTER_MS, PRACTICE_STEPS } from "./practice";
  import { t } from "$domain/i18n.svelte";

  const cfg = $derived(config.current);
  const active = $derived(
    Boolean(cfg?.onboarding_done && !cfg.onboarding_practice_done),
  );

  let host = $state<HTMLElement | null>(null);
  let stepIndex = $state(0);
  let canSkip = $state(false);
  let statusText = $state<string | null>(null);
  let statusError = $state(false);
  let fallbackPlace = $state({ x: 24, y: 24 });

  const step = $derived(PRACTICE_STEPS[stepIndex] ?? PRACTICE_STEPS[0]);
  const shortcut = $derived(cfg ? formatShortcut(cfg[step.shortcutKey]) : "");
  const pillRect = $derived(surfaces.live["pill"] ?? surfaces.live["pill-skin"]);
  const place = $derived(
    pillRect
      ? placeNear(pillRect.x, pillRect.y, pillRect.w, pillRect.h)
      : fallbackPlace,
  );

  $effect(() => sessionEffect(["config"]));

  $effect(() => {
    if (active) stepIndex = 0;
  });

  $effect(() => (host && active ? liveArea("onboarding-coach", host) : undefined));

  $effect(() => {
    if (!active || pillRect) return;
    let cancelled = false;
    void overlayWorkAreas()
      .then((areas) => {
        if (cancelled || areas.length === 0) return;
        const work = workAreaOf(areas[0]);
        fallbackPlace = {
          x: Math.round(work.x + work.w / 2 - 176),
          y: Math.round(work.y + work.h - 280),
        };
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  });

  $effect(() => {
    if (!active) return;
    void stepIndex;
    canSkip = false;
    statusText = null;
    statusError = false;
    const timer = setTimeout(() => (canSkip = true), PRACTICE_SKIP_AFTER_MS);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    if (!active) return;
    const current = step.id;
    let stop: (() => void) | undefined;
    void subscribe({
      "pill-radial-press": () => {
        if (current === "wheel") advance();
      },
      "activate-tool-slot": (tool: ToolId) => {
        if (current === "clipboard" && tool === "clipboard") advance();
      },
      "dictation-status": (payload) => {
        if (current !== "dictation") return;
        if (payload.phase === "listening") {
          statusText = t("onboarding.coach.listening");
          statusError = false;
          return;
        }
        if (payload.phase === "transcribing") {
          statusText = t("onboarding.coach.transcribing");
          statusError = false;
          return;
        }
        if (payload.phase === "error") {
          statusText = payload.message ?? t("onboarding.coach.failed");
          statusError = true;
          canSkip = true;
          return;
        }
        if (payload.phase === "pasted") advance();
      },
    }).then((un) => {
      stop = un;
    });
    return () => stop?.();
  });

  function placeNear(x: number, y: number, w: number, h: number) {
    const cardW = 352;
    const cardH = 220;
    const gap = 16;
    const left = Math.max(12, Math.round(x + w / 2 - cardW / 2));
    if (y > cardH + gap + 12) {
      return { x: left, y: Math.round(y - cardH - gap) };
    }
    return { x: left, y: Math.round(y + h + gap) };
  }

  function advance() {
    if (stepIndex >= PRACTICE_STEPS.length - 1) {
      void finish();
      return;
    }
    stepIndex += 1;
  }

  async function finish() {
    try {
      await config.patch({ onboarding_practice_done: true });
    } catch (error) {
      toastError(error);
    }
  }
</script>

{#if active}
  <div
    bind:this={host}
    class="coach"
    style:left="{place.x}px"
    style:top="{place.y}px"
    role="dialog"
    aria-labelledby="onboarding-coach-title"
    aria-live="polite"
  >
    <p class="text-micro text-faint uppercase">
      {t("onboarding.coach.progress", {
        current: stepIndex + 1,
        total: PRACTICE_STEPS.length,
      })}
    </p>
    <h2 id="onboarding-coach-title" class="text-sm font-medium text-text">
      {t(`onboarding.coach.${step.id}Title`)}
    </h2>
    <p class="max-w-[36ch] text-sm leading-relaxed text-muted">
      {t(`onboarding.coach.${step.id}Body`)}
    </p>
    <Kbd combo={shortcut} separator="+" />

    {#if statusText}
      <p class="text-xs {statusError ? 'text-danger' : 'text-faint'}">
        {statusText}
      </p>
    {/if}

    <div class="flex justify-end gap-2">
      <Button variant="ghost" size="sm" onclick={() => void finish()}>
        {t("onboarding.coach.close")}
      </Button>
      {#if canSkip}
        <Button variant="ghost" size="sm" onclick={() => advance()}>
          {stepIndex >= PRACTICE_STEPS.length - 1
            ? t("onboarding.coach.done")
            : t("onboarding.coach.skip")}
        </Button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .coach {
    position: absolute;
    z-index: var(--z-overlay-float);
    box-sizing: border-box;
    width: 22rem;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.85rem 1rem;
    border-radius: 12px;
    border: 1px solid var(--line);
    background: var(--elevated);
    color: var(--text);
    box-shadow: var(--shadow-card);
    pointer-events: auto;
    -webkit-font-smoothing: antialiased;
  }
</style>
