<script lang="ts">
  /**
   * Todas las primitivas, en las tres paletas, en la misma página.
   *
   * No es una galería: es la prueba de que la capa de tokens funciona. Tailwind
   * genera utilidades bajo demanda, así que hasta que algo no escribe
   * `bg-surface` no hay forma de saber si `@theme inline` resuelve donde tiene
   * que resolver. Y el modo más difícil —una isla con paleta propia dentro de
   * otra— solo se ve poniendo las tres juntas.
   *
   * Si una columna se ve con los colores de otra, el `inline` no está haciendo
   * su trabajo y la cadena quedó anclada a `:root`.
   */
  import Banner from "$ui/Banner.svelte";
  import Button from "$ui/Button.svelte";
  import Chip from "$ui/Chip.svelte";
  import EmptyState from "$ui/EmptyState.svelte";
  import Field from "$ui/Field.svelte";
  import Input from "$ui/Input.svelte";
  import Kbd from "$ui/Kbd.svelte";
  import Modal from "$ui/Modal.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";
  import Switch from "$ui/Switch.svelte";

  const PALETTES = [
    { name: "oscuro", attrs: { "data-theme": "dark" } },
    { name: "claro", attrs: { "data-theme": "light" } },
    { name: "consola", attrs: { "data-palette": "console" } },
  ];

  let text = $state("Reunión de equipo");
  let empty = $state("");
  let on = $state(true);
  let off = $state(false);
  let seg = $state<"todos" | "míos" | "otros">("todos");
  let modalOpen = $state(false);
</script>

<svelte:head><title>kitchen sink</title></svelte:head>

<div class="atic-root min-h-screen p-6">
  <header class="mb-6 flex items-baseline gap-3">
    <h1 class="text-xl font-semibold">kitchen sink</h1>
    <p class="text-xs text-faint">
      las tres paletas, lado a lado. si una columna se ve con los colores de otra, el
      <code class="font-mono">@theme inline</code> no está resolviendo en el elemento.
    </p>
  </header>

  <div class="grid gap-4 lg:grid-cols-3">
    {#each PALETTES as palette (palette.name)}
      <section
        {...palette.attrs}
        class="atic-root flex flex-col gap-5 rounded-md border border-line bg-surface p-4"
      >
        <h2 class="text-micro text-faint uppercase">{palette.name}</h2>

        <!-- Superficies: tienen que verse tres escalones distintos. -->
        <div class="flex gap-1.5">
          {#each ["bg-bg", "bg-surface", "bg-surface-2", "bg-elevated"] as swatch (swatch)}
            <div class="h-8 flex-1 rounded-xs border border-line {swatch}"></div>
          {/each}
        </div>

        <!-- Tinta: tres niveles y ninguno más. -->
        <div class="flex flex-col gap-0.5">
          <p class="text-sm text-text">Texto principal</p>
          <p class="text-sm text-muted">Texto secundario</p>
          <p class="text-sm text-faint">Texto tenue</p>
        </div>

        <div class="flex flex-wrap items-center gap-1.5">
          <Button variant="primary" size="sm">Grabar</Button>
          <Button variant="soft" size="sm">Importar</Button>
          <Button variant="ghost" size="sm">Cancelar</Button>
          <Button variant="danger" size="sm">Borrar</Button>
        </div>

        <div class="flex flex-wrap items-center gap-1.5">
          <Button variant="primary">Acción</Button>
          <Button variant="danger-solid">Borrar todo</Button>
          <Button variant="soft" disabled>Deshabilitado</Button>
          <Button variant="soft" loading>Cargando</Button>
        </div>

        <!-- Estado: lo único que tiene derecho a color. -->
        <div class="flex flex-wrap gap-1.5">
          <Chip>neutral</Chip>
          <Chip tone="ok">transcrita</Chip>
          <Chip tone="warn">pendiente</Chip>
          <Chip tone="danger">error</Chip>
          <Chip tone="rec">grabando</Chip>
          <Chip tone="info">nuevo</Chip>
        </div>

        <div class="flex flex-col gap-2">
          <Field label="Título" hint="Se usa en la lista y en el resumen.">
            {#snippet children({ id, describedBy })}
              <Input {id} aria-describedby={describedBy} bind:value={text} />
            {/snippet}
          </Field>

          <Field label="Carpeta" error="No se pudo escribir ahí." required>
            {#snippet children({ id, describedBy })}
              <Input
                {id}
                aria-describedby={describedBy}
                bind:value={empty}
                invalid
                mono
              />
            {/snippet}
          </Field>
        </div>

        <div class="flex flex-col gap-2">
          <Switch bind:checked={on} label="Transcribir al terminar" />
          <Switch
            bind:checked={off}
            label="Vista en vivo"
            hint="Experimental. Consume más CPU."
          />
          <Switch checked={false} label="No disponible" disabled />
        </div>

        <SegmentedControl
          bind:value={seg}
          label="Filtrar hablantes"
          options={[
            { value: "todos", label: "Todos" },
            { value: "míos", label: "Míos" },
            { value: "otros", label: "Otros" },
          ]}
          full
        />

        <div class="flex items-center gap-2 text-xs text-muted">
          <span>Buscar</span>
          <Kbd combo="Ctrl+K" />
          <span>Dictar</span>
          <Kbd combo="Ctrl+Shift+D" />
        </div>

        <Banner tone="warn" title="Falta descargar un modelo">
          {#snippet action()}
            <Button variant="soft" size="sm">Descargar</Button>
          {/snippet}
          Sin él no se puede transcribir.
        </Banner>

        <Banner tone="danger" title="No se pudo abrir el micrófono" />

        <div class="rounded-sm border border-line">
          <EmptyState
            title="Todavía no hay grabaciones"
            hint="Empezá una desde la pill."
          >
            {#snippet action()}
              <Button variant="soft" size="sm">Grabar ahora</Button>
            {/snippet}
          </EmptyState>
        </div>

        <!-- Los números no saltan de ancho: por eso van en mono y tabulares. -->
        <p class="font-mono text-sm text-muted" data-numeric>
          00:14:32 · 48.2 MB · 1 128 palabras
        </p>
      </section>
    {/each}
  </div>

  <div class="mt-6">
    <Button variant="soft" onclick={() => (modalOpen = true)}>Abrir un modal</Button>
  </div>

  {#if modalOpen}
    <Modal
      title="Borrar la grabación"
      subtitle="Reunión de equipo · 14 min"
      size="sm"
      onClose={() => (modalOpen = false)}
    >
      {#snippet actions()}
        <Button variant="ghost" onclick={() => (modalOpen = false)}>Cancelar</Button>
        <Button variant="danger-solid" onclick={() => (modalOpen = false)}
          >Borrar</Button
        >
      {/snippet}
      <p class="text-sm text-muted">
        Se borran el audio, la transcripción y el resumen. No se puede deshacer.
      </p>
    </Modal>
  {/if}
</div>
