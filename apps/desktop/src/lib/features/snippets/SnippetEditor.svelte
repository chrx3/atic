<script lang="ts">
  /**
   * El editor de un texto guardado.
   *
   * Es un componente aparte y no un bloque dentro de la herramienta por una
   * razón concreta: recibe el texto ya comprobado como no nulo, así que puede
   * hacer `bind:` sobre sus campos. Dentro de un `{#if}` sobre estado, el
   * estrechamiento no llega hasta la expresión del `bind` y hay que repetir la
   * comprobación en cada campo.
   */
  import type { Snippet as SnippetItem } from "$core/types";
  import Button from "$ui/Button.svelte";
  import Field from "$ui/Field.svelte";
  import Input from "$ui/Input.svelte";
  import TextArea from "$ui/TextArea.svelte";
  import { t } from "$domain/i18n.svelte";

  let {
    item = $bindable(),
    saving = false,
    onSave,
    onPaste,
    onDelete,
    onClose,
  }: {
    item: SnippetItem;
    saving?: boolean;
    onSave: () => void;
    onPaste: (id: string) => void;
    onDelete: () => void;
    onClose: () => void;
  } = $props();
</script>

<div class="flex flex-col gap-3 p-4">
  <Field label={t("page.snippets.name")} required>
    {#snippet children({ id })}
      <Input {id} bind:value={item.name} placeholder={t("page.snippets.namePlaceholder")} />
    {/snippet}
  </Field>

  <Field label={t("page.snippets.body")}>
    {#snippet children({ id })}
      <TextArea {id} bind:value={item.body} rows={8} />
    {/snippet}
  </Field>

  <div class="flex flex-wrap gap-1.5">
    <Button
      variant="primary"
      size="sm"
      loading={saving}
      disabled={!item.name.trim()}
      onclick={onSave}
    >
      {t("page.snippets.save")}
    </Button>

    <!-- Solo si ya existe en disco: pegar o borrar algo sin guardar no
         significa nada. -->
    {#if item.id}
      <Button variant="soft" size="sm" onclick={() => onPaste(item.id)}
        >{t("pill.paste")}</Button
      >
      <Button variant="danger" size="sm" onclick={onDelete}
        >{t("page.common.delete")}</Button
      >
    {/if}

    <Button variant="ghost" size="sm" onclick={onClose}>{t("page.common.close")}</Button>
  </div>
</div>
