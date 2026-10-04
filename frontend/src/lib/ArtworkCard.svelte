<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import ArtworkGallery from "./ArtworkGallery.svelte";
  import ArtworkThumbnail from "./ArtworkThumbnail.svelte";
  import CardPreview from "./CardPreview.svelte";
  import type { ArtworkSearch, ArtworkChoice } from "./artwork";
  import type { ImageState } from "./draft";
  export let image: ImageState;
  export let search: ArtworkSearch;
  export let disabled = false;
  const dispatch = createEventDispatcher<{
    choose: ArtworkChoice;
    load: number;
    edit: number;
    retry: void;
  }>();
  $: name = image.item.query;
  $: face = image.item.face;
  $: selected = image.choice;
  $: query = selected?.query ?? name;
  $: count = selected?.count ?? 0;
  $: busy = image.busy;
  $: error = image.error;
  $: notFound = image.status === "no-match" && !selected;
  let variant: number | undefined = 1;
  let previousChoice: ArtworkChoice | null = null;
  let gallery = false;
  let previewOpen = false;
  let previewFailed = false;
  let previewRetry = 0;
  $: if (selected !== previousChoice) {
    previousChoice = selected;
    variant = selected ? selected.index + 1 : 1;
    previewFailed = false;
    previewRetry = 0;
  }
  function load(index: number) {
    dispatch("load", index);
  }
  function choose(choice: ArtworkChoice) {
    dispatch("choose", choice);
  }
  function jump() {
    const next = Number(variant);
    if (!Number.isInteger(next) || next < 1 || next > count) {
      variant = selected ? selected.index + 1 : 1;
      return;
    }
    load(next - 1);
  }
</script>

<article
  class="artwork-card"
  class:artwork-not-found={notFound}
  aria-label={`${name} ${face} artwork`}
  aria-busy={busy}
>
  <header>
    <h3 title={name}>{name}</h3>
    <span>{face === "back" ? "Reverse" : "Front"}</span>
  </header>
  {#if notFound}<div class="artwork-not-found-label">
      <Icon name="warning" size={18} /><strong>Card not found</strong>
    </div>{/if}
  <div class="artwork-entry-links">
    <button
      class="text-button"
      {disabled}
      on:click={() => dispatch("edit", image.item.source_lines[0])}
      >Edit entry</button
    >
    {#each image.item.source_lines as line}<button
        class="text-button"
        {disabled}
        on:click={() => dispatch("edit", line)}>Line {line}</button
      >{/each}
  </div>
  <div class="artwork-carousel">
    <button
      class="variant-arrow previous"
      aria-label={`Previous artwork for ${name} (${face})`}
      disabled={busy || disabled || !selected || selected.index === 0}
      on:click={() => selected && load(selected.index - 1)}
      ><Icon name="chevron-left" /></button
    >
    <button
      class="artwork-card-preview"
      aria-label={`Preview artwork for ${name} (${face})`}
      title="Preview artwork"
      disabled={busy || disabled || !selected}
      on:click={() => (previewOpen = true)}
    >
      {#if selected}{#key selected.art.drive_id}<ArtworkThumbnail
            driveId={selected.art.drive_id}
            label={selected.art.label}
            retry={previewRetry}
            on:error={() => (previewFailed = true)}
            on:loaded={() => (previewFailed = false)}
          />{/key}
      {:else}<span class="artwork-placeholder"
          ><Icon name={notFound ? "warning" : "image"} size={30} /><span
            >{busy
              ? "Finding artwork…"
              : notFound
                ? "Check the name in your decklist"
                : "Find artwork"}</span
          ></span
        >{/if}
    </button>
    <button
      class="variant-arrow next"
      aria-label={`Next artwork for ${name} (${face})`}
      disabled={busy || disabled || !selected || selected.index + 1 >= count}
      on:click={() => selected && load(selected.index + 1)}
      ><Icon name="chevron-right" /></button
    >
    {#if busy && selected}<span class="variant-loading">Loading…</span>{/if}
  </div>
  <div class="artwork-card-details">
    {#if !notFound}<p class="artwork-match-status">
        {image.status === "searching"
          ? "Searching…"
          : image.status === "no-match"
            ? "No matches"
            : image.status === "failed"
              ? "Search failed"
              : "Artwork selected"}
      </p>{/if}
    {#if count || !selected}<form
        class="variant-field"
        on:submit|preventDefault={jump}
      >
        <label
          >Art <input
            type="number"
            min="1"
            max={Math.max(count, 1)}
            step="1"
            bind:value={variant}
            on:change={jump}
            aria-label={`Artwork number for ${name} (${face})`}
            disabled={busy || disabled || !count}
          /></label
        ><span>of {count || "—"}</span>
        {#if selected}<Icon name="check" size={14} />{/if}
      </form>{/if}
    <p
      class="artwork-credit"
      title={selected ? `${selected.art.source} · ${selected.art.dpi} DPI` : ""}
    >
      {selected
        ? `${selected.art.source}${selected.art.dpi ? ` · ${selected.art.dpi} DPI` : ""}`
        : busy
          ? "Searching MPC Autofill"
          : "No artwork selected"}
    </p>
    {#if error}<p class="artwork-card-error">{error}</p>
      <button
        class="text-button"
        disabled={busy || disabled}
        on:click={() => dispatch("retry")}>Retry</button
      >{/if}
    {#if previewFailed}<button
        class="text-button retry-preview"
        on:click={() => {
          previewFailed = false;
          previewRetry++;
        }}>Retry preview</button
      >{/if}
    <button
      class="button secondary small browse-artwork"
      {disabled}
      on:click={() => (gallery = true)}
      ><Icon name="grid" size={14} />{selected
        ? "Choose different artwork"
        : "Find artwork"}</button
    >
  </div>
</article>
{#if previewOpen && selected}<CardPreview
    artwork={selected.art}
    on:close={() => (previewOpen = false)}
  />{/if}
{#if gallery}<ArtworkGallery
    {search}
    title={name}
    {query}
    {selected}
    editableEntry
    on:edit={() => {
      gallery = false;
      dispatch("edit", image.item.source_lines[0]);
    }}
    on:close={() => (gallery = false)}
    on:choose={(event) => {
      choose(event.detail);
      gallery = false;
    }}
  />{/if}
