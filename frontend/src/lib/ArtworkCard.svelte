<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy } from "svelte";
  import Icon from "./Icon.svelte";
  import ArtworkGallery from "./ArtworkGallery.svelte";
  import ArtworkThumbnail from "./ArtworkThumbnail.svelte";
  import {
    ARTWORK_PAGE_SIZE,
    type ArtworkSearch,
    type ArtworkChoice,
  } from "./artwork";

  export let name: string;
  export let face: string;
  export let search: ArtworkSearch;
  export let initial: ArtworkChoice | null = null;
  export let disabled = false;
  const dispatch = createEventDispatcher<{
    choose: ArtworkChoice;
    loading: boolean;
  }>();
  let selected = initial;
  let query = initial?.query ?? name;
  let count = initial?.count ?? 0;
  let variant: number | undefined = initial ? initial.index + 1 : 1;
  let busy = !initial;
  let error = "";
  let gallery = false;
  let generation = 0;
  let previewFailed = false;
  let previewRetry = 0;
  onMount(() => {
    if (!selected) void load(0, true);
  });
  onDestroy(() => {
    generation++;
  });

  async function load(index: number, background = false) {
    const token = ++generation;
    busy = true;
    error = "";
    dispatch("loading", true);
    try {
      const found = await search(
        query,
        Math.floor(index / ARTWORK_PAGE_SIZE),
        "card",
        background ? "background" : "interactive",
      );
      if (token !== generation) return;
      const art = found.cards[index % ARTWORK_PAGE_SIZE];
      if (!art) {
        if (!selected) count = found.count;
        error = found.count
          ? "This option is no longer available. Open the gallery to choose another."
          : "No artwork found. Open the gallery to try another name.";
      } else choose({ art, query, index, count: found.count });
    } catch (e) {
      if (token === generation) error = String(e);
    } finally {
      if (token === generation) {
        busy = false;
        variant = selected ? selected.index + 1 : 1;
        dispatch("loading", false);
      }
    }
  }
  function choose(choice: ArtworkChoice) {
    error = "";
    selected = choice;
    query = choice.query;
    count = choice.count;
    variant = choice.index + 1;
    previewFailed = false;
    previewRetry = 0;
    dispatch("choose", choice);
  }
  function jump() {
    const next = Number(variant);
    if (!Number.isInteger(next) || next < 1 || next > count) {
      variant = selected ? selected.index + 1 : 1;
      return;
    }
    void load(next - 1);
  }
</script>

<article
  class="artwork-card"
  aria-label={`${name} ${face} artwork`}
  aria-busy={busy}
>
  <header>
    <h3 title={name}>{name}</h3>
    <span>{face === "back" ? "Reverse" : "Front"}</span>
  </header>
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
      aria-label={`Browse all artwork for ${name} (${face})`}
      disabled={busy || disabled}
      on:click={() => (gallery = true)}
    >
      {#if selected}{#key selected.art.drive_id}<ArtworkThumbnail
            driveId={selected.art.drive_id}
            label={selected.art.label}
            retry={previewRetry}
            on:error={() => (previewFailed = true)}
            on:loaded={() => (previewFailed = false)}
          />{/key}
      {:else}<span class="artwork-placeholder"
          ><Icon name="image" size={30} /><span
            >{busy
              ? "Finding artwork…"
              : selected
                ? "Preview unavailable"
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
    {#if busy && selected}<span class="variant-loading" role="status"
        >Loading…</span
      >{/if}
  </div>
  <div class="artwork-card-details">
    <form class="variant-field" on:submit|preventDefault={jump}>
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
    </form>
    <p
      class="artwork-credit"
      title={selected ? `${selected.art.source} · ${selected.art.dpi} DPI` : ""}
    >
      {selected
        ? `${selected.art.source} · ${selected.art.dpi} DPI`
        : busy
          ? "Searching MPC Autofill"
          : "No artwork selected"}
    </p>
    {#if error}<p class="artwork-card-error" role="alert">{error}</p>
      <button
        class="text-button"
        disabled={busy || disabled}
        on:click={() => load(selected?.index ?? 0)}>Retry</button
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
      disabled={busy || disabled}
      on:click={() => (gallery = true)}
      ><Icon name="grid" size={14} />Browse all artwork</button
    >
  </div>
</article>
{#if gallery}<ArtworkGallery
    {search}
    title={name}
    {query}
    {selected}
    on:close={() => (gallery = false)}
    on:choose={(event) => {
      choose(event.detail);
      gallery = false;
    }}
  />{/if}
