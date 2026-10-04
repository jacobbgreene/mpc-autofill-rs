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
  export let editableEntry = false;
  const dispatch = createEventDispatcher<{
    close: void;
    choose: ArtworkChoice;
    edit: void;
  }>();
  let dialog: HTMLDialogElement;
  let body: HTMLDivElement;
  let inputQuery = query;
  let results: ArtworkResults | null = null;
  let choices: ArtworkChoice[] = [];
  let page =
    kind === "card" && selected
      ? Math.floor(selected.index / ARTWORK_PAGE_SIZE)
      : 0;
  let pageInput: number | undefined = page + 1;
  let busy = false;
  let loadingMore = false;
  let error = "";
  let requested = { page, query, append: false };
  let generation = 0;
  let failedPreviews = new Set<string>();
  let previewRetry = 0;
  $: locked = (busy && !loadingMore) || applying;
  $: currentId = selected?.art.drive_id ?? selectedId;
  $: pages = Math.max(1, Math.ceil((results?.count ?? 0) / ARTWORK_PAGE_SIZE));
  $: resultLabel = kind === "cardback" ? "card backs" : "artwork";

  onMount(() => {
    dialog.showModal();
    void load(page);
  });
  onDestroy(() => {
    generation++;
    dialog?.close();
  });
  async function load(nextPage: number, nextQuery = query, append = false) {
    if (applying || (append && busy)) return;
    const token = ++generation;
    busy = true;
    loadingMore = append;
    error = "";
    requested = { page: nextPage, query: nextQuery, append };
    if (!append) {
      page = nextPage;
      pageInput = nextPage + 1;
      results = null;
      choices = [];
      failedPreviews = new Set();
      body.scrollTop = 0;
    }
    try {
      const found = await search(nextQuery, nextPage, kind);
      if (token !== generation) return;
      results = {
        ...found,
        next_page:
          found.cards.length &&
          found.next_page !== null &&
          found.next_page > nextPage
            ? found.next_page
            : null,
      };
      const nextChoices = found.cards.map((art, i) => ({
        art,
        query: nextQuery,
        index: nextPage * ARTWORK_PAGE_SIZE + i,
        count: found.count,
      }));
      if (append) {
        const seen = new Set(choices.map((choice) => choice.art.drive_id));
        choices = [
          ...choices,
          ...nextChoices.filter((choice) => {
            if (seen.has(choice.art.drive_id)) return false;
            seen.add(choice.art.drive_id);
            return true;
          }),
        ];
      } else {
        choices = nextChoices;
        failedPreviews = new Set();
        body.scrollTop = 0;
      }
      query = nextQuery;
      page = nextPage;
      pageInput = page + 1;
    } catch (e) {
      if (token === generation)
        error = `Search failed for “${nextQuery || "all card backs"}”: ${String(e)}`;
    } finally {
      if (token === generation) {
        busy = false;
        loadingMore = false;
      }
    }
  }
  function loadMore() {
    if (!busy && !applying && !error && results?.next_page != null)
      void load(results.next_page, query, true);
  }
  function observeMore(node: HTMLElement) {
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) loadMore();
      },
      { root: body, rootMargin: "300px 0px" },
    );
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
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
  <label class="gallery-search-label" for="gallery-search"
    >{kind === "cardback"
      ? "Search card backs"
      : "Search replacement artwork"}</label
  >
  <form
    class="gallery-toolbar"
    on:submit|preventDefault={() => load(0, inputQuery.trim())}
  >
    <label class="search-field"
      ><Icon name="search" size={16} /><input
        id="gallery-search"
        bind:value={inputQuery}
        disabled={applying}
        aria-label={kind === "cardback"
          ? "Search card backs"
          : "Search replacement artwork"}
        placeholder={kind === "cardback"
          ? "Search card backs"
          : "Search replacement artwork"}
      /></label
    >
    <button
      class="button secondary small"
      disabled={applying || (kind === "card" && !inputQuery.trim())}
      >Search</button
    >
  </form>
  <div class="gallery-context">
    {#if kind === "card"}<p class="editor-help gallery-search-context">
        Choosing artwork for <strong>{title}</strong>. Selecting an image
        changes its artwork; edit the decklist to change its name.
      </p>{/if}
    {#if editableEntry}<button
        class="text-button"
        disabled={applying}
        on:click={() => dispatch("edit")}>Edit entry</button
      >{/if}
  </div>
  <div class="editor-body" bind:this={body} aria-busy={busy || applying}>
    {#if applying}<p class="gallery-status" role="status">
        Updating artwork…
      </p>{/if}
    {#if applyError}<p class="notice error-notice" role="alert">
        {applyError}
      </p>{/if}
    {#if description}<p class="editor-help">{description}</p>{/if}
    {#if busy && !loadingMore}<p class="gallery-status" role="status">
        Searching MPC Autofill…
      </p>{/if}
    {#if error && !requested.append}<div
        class="notice error-notice"
        role="alert"
      >
        <p>{error}</p>
        <button
          class="button secondary small"
          disabled={locked}
          on:click={() =>
            load(requested.page, requested.query, requested.append)}
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
      <div class="artwork-grid" class:is-loading={busy && !loadingMore}>
        {#each choices as choice (choice.art.drive_id)}
          {@const art = choice.art}
          <button
            class="artwork-choice"
            class:is-selected={currentId === art.drive_id}
            aria-pressed={currentId === art.drive_id}
            disabled={locked}
            on:click={() => dispatch("choose", choice)}
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
                : `Use artwork ${choice.index + 1}`}</span
            >
          </button>
        {/each}
      </div>
      {#if !choices.length}<p class="no-artwork">
          {kind === "cardback"
            ? "No matching card backs. Try another search or clear it to browse all backs."
            : "No matching artwork. Try the card’s full name or a different spelling."}
        </p>{/if}
      {#if results.next_page !== null}
        {#key results.next_page}
          <div class="gallery-load-more" use:observeMore>
            {#if error && requested.append}
              <div class="notice error-notice" role="alert">
                <p>Could not load more {resultLabel}: {error}</p>
                <button
                  class="button secondary small"
                  disabled={busy || applying}
                  on:click={() => load(requested.page, requested.query, true)}
                  >Retry loading</button
                >
              </div>
            {:else if loadingMore}
              <p class="gallery-status" role="status">
                Loading more {resultLabel}…
              </p>
            {:else}
              <button
                class="button secondary small"
                disabled={busy || applying}
                on:click={loadMore}>Load more {resultLabel}</button
              >
            {/if}
          </div>
        {/key}
      {/if}
    {/if}
  </div>
  <footer class="editor-footer gallery-footer">
    {#if kind === "cardback"}
      <span class="gallery-loaded-count" role="status">
        {#if results}
          {choices.length} of {results.count} card backs{results.next_page ===
          null
            ? " · End of results"
            : " · Scroll for more"}
        {:else}Browse card backs{/if}
      </span>
    {:else}
      <button
        class="button secondary small"
        disabled={locked || !results || page === 0}
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
    {/if}
  </footer>
</dialog>
