<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import ArtworkThumbnail from "./ArtworkThumbnail.svelte";
  import Icon from "./Icon.svelte";

  type SavedDeck = {
    path: string;
    name: string;
    quantity: number;
    stock: string;
    foil: boolean;
    modified: number;
    preview_id: string | null;
  };
  type Listing = { decks: SavedDeck[]; folder: string; warning: string | null };

  export let disabled = false;
  const dispatch = createEventDispatcher<{
    select: string;
    open: boolean;
    paste: void;
    home: void;
  }>();
  let decks: SavedDeck[] = [];
  let folder = "";
  let warning = "";
  let error = "";
  let loading = true;
  let disposed = false;
  let query = "";
  let sort = "modified";
  $: search = query.trim().toLocaleLowerCase();
  $: visible = decks
    .filter((deck) =>
      `${deck.name} ${deck.path}`.toLocaleLowerCase().includes(search),
    )
    .sort((a, b) =>
      sort === "name"
        ? a.name.localeCompare(b.name)
        : b.modified - a.modified || a.name.localeCompare(b.name),
    );

  onMount(() => {
    void refresh();
  });
  onDestroy(() => {
    disposed = true;
  });
  async function refresh() {
    loading = true;
    error = "";
    try {
      const result = await invoke<Listing>("list_saved_decks");
      if (disposed) return;
      decks = result.decks;
      folder = result.folder;
      warning = result.warning || "";
    } catch (e) {
      if (!disposed) error = `Could not load saved decks: ${String(e)}`;
    } finally {
      if (!disposed) loading = false;
    }
  }
  function date(value: number) {
    return value
      ? new Date(value).toLocaleDateString(undefined, {
          month: "short",
          day: "numeric",
          year: "numeric",
        })
      : "Date unavailable";
  }
</script>

<section class="saved-decks" aria-label="Saved decks">
  <button
    class="text-button saved-decks-back"
    {disabled}
    on:click={() => dispatch("home")}
    ><Icon name="chevron-left" size={16} />Back to start</button
  >
  <div class="saved-decks-heading">
    <div>
      <p class="eyebrow">YOUR LIBRARY</p>
      <h1>Saved decks <span class="count-badge">{decks.length}</span></h1>
      <p class="muted">Pick up where you left off, or start a new deck.</p>
    </div>
    <div class="saved-decks-actions">
      <button
        class="button secondary small"
        {disabled}
        on:click={() => dispatch("open", false)}
        ><Icon name="folder" size={16} />Open order</button
      >
      <button
        class="button primary small"
        {disabled}
        on:click={() => dispatch("open", true)}
        ><Icon name="import" size={16} />Import .txt decklist</button
      >
      <button
        class="button secondary small"
        {disabled}
        on:click={() => dispatch("paste")}>Paste a decklist</button
      >
    </div>
  </div>
  {#if error}
    <div class="notice error-notice" role="alert">
      <Icon name="warning" />
      <p>{error}</p>
      <button
        class="button secondary small"
        disabled={loading}
        on:click={refresh}>Retry</button
      >
    </div>
  {/if}
  {#if warning}<p class="notice" role="status">{warning}</p>{/if}
  {#if loading && !decks.length}
    <p class="saved-decks-loading" role="status">
      <Icon name="clock" />Finding saved decks…
    </p>
  {:else if decks.length}
    <div class="saved-decks-controls">
      <label class="search-field"
        ><Icon name="search" size={17} /><input
          type="search"
          bind:value={query}
          placeholder="Find a saved deck…"
          aria-label="Search saved decks"
        /></label
      >
      <label class="saved-decks-sort"
        >Sort by <select bind:value={sort} aria-label="Sort saved decks">
          <option value="modified">Recently saved</option><option value="name"
            >Name</option
          >
        </select></label
      >
      <button
        class="text-button"
        disabled={loading || disabled}
        on:click={refresh}>{loading ? "Refreshing…" : "Refresh"}</button
      >
    </div>
    {#if visible.length}
      <div class="saved-deck-grid">
        {#each visible as deck (deck.path)}
          <button
            class="saved-deck"
            {disabled}
            on:click={() => dispatch("select", deck.path)}
            aria-label={`Open ${deck.name}, ${deck.quantity} cards`}
          >
            <span class="saved-deck-art">
              {#if deck.preview_id}<ArtworkThumbnail
                  driveId={deck.preview_id}
                  label={`${deck.name} artwork`}
                />
              {:else}<Icon size={32} />{/if}
            </span>
            <span class="saved-deck-details">
              <strong>{deck.name}</strong>
              <span class="saved-deck-count"
                >{deck.quantity} card{deck.quantity === 1 ? "" : "s"}{deck.foil
                  ? " · Foil"
                  : ""}</span
              >
              <span>{deck.stock}</span>
              <span class="saved-deck-date">Saved {date(deck.modified)}</span>
              <span class="saved-deck-path" title={deck.path}>{deck.path}</span>
              <span class="saved-deck-open"
                >Open deck <Icon name="arrow" size={14} /></span
              >
            </span>
          </button>
        {/each}
      </div>
    {:else}
      <div class="no-results">
        <Icon name="search" size={28} />
        <h3>No matching decks</h3>
        <p>Try a different name or folder.</p>
        <button class="button secondary small" on:click={() => (query = "")}
          >Show all decks</button
        >
      </div>
    {/if}
  {:else if !error}
    <div class="no-results saved-decks-empty">
      <Icon name="folder" size={32} />
      <h3>No saved decks available yet</h3>
      <p>Save a deck, or open an existing XML order to add it here.</p>
    </div>
  {/if}
  {#if !loading || decks.length}
    <div class="saved-decks-footer">
      {#if folder}<p>
          Default save folder <span title={folder}>{folder}</span>
        </p>{/if}
      {#if !decks.length}<button
          class="text-button"
          disabled={loading || disabled}
          on:click={refresh}>Refresh saved decks</button
        >{/if}
    </div>
  {/if}
</section>
