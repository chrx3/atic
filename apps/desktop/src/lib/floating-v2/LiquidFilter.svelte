<script lang="ts">
  let { id = "atic-liquid-v2" }: { id?: string } = $props();
</script>

<svg class="filter-definitions" aria-hidden="true">
  <defs>
    <filter id={id} x="-45%" y="-45%" width="190%" height="190%" color-interpolation-filters="sRGB">
      <feGaussianBlur in="SourceGraphic" stdDeviation="8" result="blur" />
      <feColorMatrix
        in="blur"
        mode="matrix"
        values="
          1 0 0 0 0
          0 1 0 0 0
          0 0 1 0 0
          0 0 0 22 -9"
        result="liquid"
      />
      <feComposite in="SourceGraphic" in2="liquid" operator="atop" />
    </filter>
  </defs>
</svg>

<style>
  .filter-definitions {
    position: absolute;
    width: 0;
    height: 0;
    overflow: hidden;
    pointer-events: none;
  }
</style>
