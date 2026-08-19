<script lang="ts">
  /**
   * Aviso de actualización: una gota azul abajo a la derecha.
   *
   * No se funde con el picker: no sale de él. Es una gota suelta del mismo
   * material líquido, con un segundo lóbulo para que se lea viscosa y no como
   * un botón redondo.
   */
  import { appUpdate } from "$domain/appUpdate.svelte";
  import { Download } from "$lib/icons";
  import { pillShape } from "$liquid/geometry";
  import Skin from "$liquid/Skin.svelte";
  import Icon from "$ui/Icon.svelte";

  const BODY = 56;
  const LOBE = 26;

  const shapes = [
    pillShape({ x: 10, y: 6, w: BODY, h: BODY }),
    pillShape({ x: 38, y: 40, w: LOBE, h: LOBE }),
  ];

  const label = $derived(
    appUpdate.downloading
      ? `Descargando ${appUpdate.version ?? ""}`
      : `Actualizar a ${appUpdate.version ?? ""}`,
  );
</script>

{#if appUpdate.pending}
  <div class="dock">
    <Skin {shapes} color="var(--info)" shadow="var(--shadow-goo)" />
    <button
      type="button"
      class="hit"
      aria-label={label}
      title={label}
      disabled={appUpdate.downloading}
      onclick={() => void appUpdate.install()}
    >
      {#if appUpdate.downloading}
        <span class="pct" data-numeric>
          {appUpdate.percent == null ? "…" : `${appUpdate.percent}%`}
        </span>
      {:else}
        <Icon icon={Download} size={22} strokeWidth={2} />
      {/if}
    </button>
  </div>
{/if}

<style>
  .dock {
    position: absolute;
    right: 18px;
    bottom: 16px;
    z-index: 4;
    width: 76px;
    height: 76px;
    pointer-events: none;
  }

  .hit {
    position: absolute;
    left: 10px;
    top: 6px;
    width: 56px;
    height: 56px;
    display: grid;
    place-items: center;
    padding: 0;
    border: 0;
    border-radius: 999px;
    background: transparent;
    color: var(--bg);
    cursor: pointer;
    pointer-events: auto;
    -webkit-tap-highlight-color: transparent;
  }

  .hit:disabled {
    cursor: progress;
  }

  .hit:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 3px;
  }

  .pct {
    font-size: 11px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.02em;
  }

  @media (prefers-reduced-motion: no-preference) {
    .dock {
      animation: rise var(--duration-enter, 420ms) var(--ease-smooth-out, ease) both;
    }
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(10px) scale(0.92);
    }

    to {
      opacity: 1;
      transform: none;
    }
  }
</style>
