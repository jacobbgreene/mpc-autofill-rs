<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy } from "svelte";
  import Icon from "./Icon.svelte";
  import ArtworkThumbnail from "./ArtworkThumbnail.svelte";
  import {
    ARTWORK_PAGE_SIZE,
    type ArtworkSearch,
    type ArtworkResults,
    type ArtworkChoice,
    type ArtworkKind,
  } from "./artwork";

  export let search: ArtworkSearch;
  export let title: string;
  export let query: string;
  export let kind: ArtworkKind = "card";
  export let selected: ArtworkChoice | null = null;
  export let selectedId = "";
  export let applying = false;
  export let applyError = "";
  export let description = "";
  const dispatch = createEventDispatcher<{
    close: void;
    choose: ArtworkChoice;
  }>();
  let dialog: HTMLDialogElement;
  let body: HTMLDivElement;
  let inputQuery = query;
  let results: ArtworkResults | null = null;
  let page = selected ? Math.floor(selected.index / ARTWORK_PAGE_SIZE) : 0;
  let pageInput: number | undefined = page + 1;
  let busy = false;
  let error = "";
  let requested = { page, query };
  let generation = 0;
  let failedPreviews = new Set<string>();
  let previewRetry = 0;
  $: locked = busy || applying;
  $: currentId = selected?.art.drive_id ?? selectedId;
  $: pages = Math.max(1, Math.ceil((results?.count ?? 0) / ARTWORK_PAGE_SIZE));

  onMount(() => {
    dialog.showModal();
    void load(page);
  });
  onDestroy(() => {
    generation++;
  });
  async function load(nextPage: number, nextQuery = query) {
    if (applying) return;
    const token = ++generation;
    busy = true;
    error = "";
    requested = { page: nextPage, query: nextQuery };
    try {
      const found = await search(nextQuery, nextPage, kind);
      if (token !== generation) return;
      results = found;
      failedPreviews = new Set();
      query = nextQuery;
      page = nextPage;
      pageInput = page + 1;
      body.scrollTop = 0;
    } catch (e) {
      if (token === generation) error = String(e);
    } finally {
      if (token === generation) busy = false;
    }
  }
  function jumpPage() {
    const next = Number(pageInput);
    if (!Number.isInteger(next) || next < 1 || next > pages) {
      pageInput = page + 1;
      return;
    }
    void load(next - 1);
  }
  function close() {
    if (!applying) dispatch("close");
  }
</script>

<svelte:window
  on:keydown={(event) => {
    if (event.key === "Escape" && dialog?.open) {
      event.preventDefault();
      event.stopPropagation();
      close();
    }
  }}
/>

<dialog
  bind:this={dialog}
  class="editor-dialog artwork-gallery"
  aria-labelledby="gallery-title"
  on:cancel={(event) => {
    event.preventDefault();
    event.stopPropagation();
    close();
  }}
>
  <header class="editor-header">
    <div>
      <p class="eyebrow">MPC AUTOFILL · ARTWORK GALLERY</p>
      <h2 id="gallery-title">{title}</h2>
    </div>
    <button
      class="icon-button"
      aria-label="Close artwork gallery"
      disabled={applying}
      on:click={close}><Icon name="close" /></button
    >
  </header>
  <form
    class="gallery-toolbar"
    on:submit|preventDefault={() => load(0, inputQuery.trim())}
  >
    <label class="search-field"
      ><Icon name="search" size={16} /><input
        bind:value={inputQuery}
        disabled={applying}
        aria-label="Artwork search name"
        placeholder={kind === "cardback"
          ? "Search card backs"
          : "Search by card name"}
      /></label
    >
    <button
      class="button secondary small"
      disabled={locked || (kind === "card" && !inputQuery.trim())}
      >Search</button
    >
  </form>
  <div class="editor-body" bind:this={body} aria-busy={locked}>
    {#if applying}<p class="gallery-status" role="status">
        Updating artwork…
      </p>{/if}
    {#if applyError}<p class="notice error-notice" role="alert">
        {applyError}
      </p>{/if}
    {#if description}<p class="editor-help">{description}</p>{/if}
    {#if busy}<p class="gallery-status" role="status">
        Searching MPC Autofill…
      </p>{/if}
    {#if error}<div class="notice error-notice" role="alert">
        <p>{error}</p>
        <button
          class="button secondary small"
          disabled={locked}
          on:click={() => load(requested.page, requested.query)}
          >Retry search</button
        >
      </div>{/if}
    {#if results}
      <p class="editor-help">
        {results.count} options · Choose an image to use it in your deck.
      </p>
      {#if failedPreviews.size}<button
          class="text-button retry-preview"
          on:click={() => {
            failedPreviews = new Set();
            previewRetry++;
          }}>Retry failed previews</button
        >{/if}
      <div class="artwork-grid" class:is-loading={busy}>
        {#each results.cards as art, i (art.drive_id)}
          <button
            class="artwork-choice"
            class:is-selected={currentId === art.drive_id}
            aria-pressed={currentId === art.drive_id}
            disabled={locked}
            on:click={() =>
              dispatch("choose", {
                art,
                query,
                index: page * ARTWORK_PAGE_SIZE + i,
                count: results?.count ?? 0,
              })}
          >
            <span class="artwork-thumb"
              ><ArtworkThumbnail
                driveId={art.drive_id}
                label={art.label}
                retry={previewRetry}
                on:error={() =>
                  (failedPreviews = new Set([...failedPreviews, art.drive_id]))}
              /></span
            >
            <strong>{art.label}</strong><span>{art.source} · {art.dpi} DPI</span
            >
            <span class="artwork-select-label"
              >{currentId === art.drive_id
                ? "✓ Selected"
                : `Use artwork ${page * ARTWORK_PAGE_SIZE + i + 1}`}</span
            >
          </button>
        {/each}
      </div>
      {#if !results.cards.length}<p class="no-artwork">
          No matching artwork. Try the card’s full name or a different spelling.
        </p>{/if}
    {/if}
  </div>
  <footer class="editor-footer gallery-footer">
    <button
      class="button secondary small"
      disabled={locked || page === 0}
      on:click={() => load(page - 1)}
      ><Icon name="chevron-left" size={16} />Previous</button
    >
    <form class="artwork-page-field" on:submit|preventDefault={jumpPage}>
      <label
        >Page <input
          type="number"
          min="1"
          max={pages}
          step="1"
          bind:value={pageInput}
          on:change={jumpPage}
          disabled={locked || !results?.count}
          aria-label="Gallery page"
        /></label
      ><span>of {pages}</span>
    </form>
    <button
      class="button secondary small"
      disabled={locked || !results || results.next_page === null}
      on:click={() => results?.next_page != null && load(results.next_page)}
      >Next<Icon name="chevron-right" size={16} /></button
    >
  </footer>
</dialog>
