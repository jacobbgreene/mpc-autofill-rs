<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { open, confirm } from "@tauri-apps/plugin-dialog";
  import Icon from "./Icon.svelte";
  import type { OrderSummary } from "./order";

  export let mode: "edit" | "replace" | "create";
  export let initialText = "";
  export let initialName = "New deck";
  export let replacingUnsaved = false;
  const dispatch = createEventDispatcher<{
    close: void;
    applied: OrderSummary;
  }>();
  type Missing = { query: string; face: "front" | "back" };
  type SearchTarget = Missing | { query: string; face: "common" };
  type Selection = Missing & { drive_id: string; name: string };
  type Preview = {
    quantity: number;
    unresolved: Missing[];
    needs_common_back: boolean;
  };
  type Artwork = {
    drive_id: string;
    name: string;
    label: string;
    source: string;
    dpi: number;
    thumb: string;
  };
  type Results = { cards: Artwork[]; count: number; next_page: number | null };
  let dialog: HTMLDialogElement;
  let text = initialText;
  let originalText = initialText;
  let orderName = initialName;
  let stock = "S30";
  let foil = false;
  let commonBack: Artwork | null = null;
  let busy = true;
  let error = "";
  let preview: Preview | null = null;
  let selections: Selection[] = [];
  let active: SearchTarget | null = null;
  let artworkQuery = "";
  let results: Results | null = null;
  let searchError = "";
  let searching = false;
  let page = 0;
  let generation = 0;
  let disposed = false;
  let artworkPanel: HTMLElement;
  $: remaining =
    preview?.unresolved.filter(
      (item) =>
        !selections.some((s) => s.query === item.query && s.face === item.face),
    ) ?? [];
  $: needsBack =
    mode === "create" && !!preview?.needs_common_back && !commonBack;
  $: if (stock === "P10") foil = false;

  onMount(() => {
    dialog.showModal();
    if (mode === "edit") {
      invoke<string>("get_decklist")
        .then((value) => {
          text = value;
          originalText = value;
        })
        .catch((e) => (error = String(e)))
        .finally(() => (busy = false));
    } else {
      busy = false;
      if (mode === "create" && text.trim()) void review();
    }
  });
  onDestroy(() => {
    disposed = true;
    generation += 1;
  });

  function invalidate() {
    preview = null;
    selections = [];
    active = null;
    results = null;
    searching = false;
    error = "";
    generation += 1;
  }
  async function close() {
    if (busy) return;
    if (
      (text !== originalText ||
        selections.length > 0 ||
        commonBack ||
        (mode === "create" &&
          (orderName !== initialName || stock !== "S30" || foil))) &&
      !(await confirm(
        "Discard this draft decklist? Your current order will stay unchanged.",
        { title: "Discard draft", kind: "warning" },
      ))
    )
      return;
    dispatch("close");
  }
  async function importText() {
    busy = true;
    try {
      const path = await open({
        multiple: false,
        filters: [{ name: "Text decklist", extensions: ["txt"] }],
      });
      if (typeof path !== "string") return;
      if (
        text !== originalText &&
        !(await confirm(
          "Replace the text in this draft with the selected file?",
          { title: "Replace draft", kind: "warning" },
        ))
      )
        return;
      const imported = await invoke<string>("read_decklist", { path });
      text = imported;
      invalidate();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
  async function review() {
    busy = true;
    error = "";
    try {
      preview =
        mode === "create"
          ? await invoke<Preview>("preview_new_decklist", {
              text,
              selections,
              options: { ...newOrderOptions(), cardback: null },
            })
          : await invoke<Preview>("preview_decklist", { text, selections });
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
  async function search(target: SearchTarget, nextPage = 0) {
    const token = ++generation;
    active = target;
    searching = true;
    searchError = "";
    results = null;
    page = nextPage;
    await tick();
    artworkPanel?.scrollIntoView({ block: "start", behavior: "auto" });
    try {
      const found = await invoke<Results>("search_artwork", {
        query: artworkQuery,
        page,
        kind: target.face === "common" ? "cardback" : "card",
      });
      if (!disposed && token === generation) {
        results = found;
        await tick();
        artworkPanel?.scrollIntoView({ block: "start", behavior: "auto" });
      }
    } catch (e) {
      if (!disposed && token === generation) searchError = String(e);
    } finally {
      if (!disposed && token === generation) searching = false;
    }
  }
  function choose(art: Artwork) {
    if (!active) return;
    if (active.face === "common") commonBack = art;
    else
      selections = [
        ...selections.filter(
          (s) => s.query !== active?.query || s.face !== active?.face,
        ),
        { ...active, drive_id: art.drive_id, name: art.name },
      ];
    active = null;
    results = null;
  }
  function newOrderOptions() {
    return {
      name: orderName,
      stock,
      foil,
      cardback: commonBack
        ? { drive_id: commonBack.drive_id, name: commonBack.name }
        : null,
    };
  }
  function browseBacks() {
    artworkQuery = "";
    void search({ query: "Common card back", face: "common" });
  }
  function nextPage() {
    if (active && results?.next_page != null)
      void search(active, results.next_page);
  }
  async function apply() {
    if (!preview || remaining.length || needsBack || busy) return;
    busy = true;
    error = "";
    try {
      if (
        mode === "create" &&
        replacingUnsaved &&
        !(await confirm(
          "Create this deck and discard the unsaved changes in your current order?",
          { title: "Unsaved changes", kind: "warning" },
        ))
      )
        return;
      const summary =
        mode === "create"
          ? await invoke<OrderSummary>("create_order", {
              text,
              selections,
              options: newOrderOptions(),
            })
          : await invoke<OrderSummary>("edit_decklist", { text, selections });
      dispatch("applied", summary);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<dialog
  bind:this={dialog}
  class="editor-dialog"
  aria-labelledby="editor-title"
  on:cancel={(event) => {
    event.preventDefault();
    void close();
  }}
>
  <header class="editor-header">
    <div>
      <p class="eyebrow">YOUR ORDER, YOUR WAY</p>
      <h2 id="editor-title">
        {mode === "create"
          ? "Create a deck from text"
          : mode === "edit"
            ? "Edit your decklist"
            : "Replace your decklist"}
      </h2>
    </div>
    <button
      class="icon-button"
      aria-label="Close decklist editor"
      disabled={busy}
      on:click={close}><Icon name="close" /></button
    >
  </header>
  <div class="editor-body">
    <p class="editor-description">
      {mode === "create"
        ? "Import or paste your card list, then choose the artwork and card backs you want to print."
        : mode === "edit"
          ? "Change quantities, remove cards, or add something new."
          : "Paste another decklist or import a text file to replace the cards in this order."}
      {#if mode !== "create"}Matching cards keep their artwork. Stock, finish,
        and the common back stay the same.{/if}
    </p>
    {#if mode === "create"}
      <div class="new-order-settings">
        <label class="order-name-field"
          >Order name<input
            bind:value={orderName}
            maxlength="200"
            disabled={busy}
          /></label
        >
        <label
          >Cardstock<select bind:value={stock} disabled={busy}>
            <option value="S27">S27 · Smooth</option><option value="S30"
              >S30 · Standard Smooth</option
            ><option value="S33">S33 · Superior Smooth</option><option
              value="M31">M31 · Linen</option
            ><option value="P10">P10 · Plastic</option>
          </select></label
        >
        <label class="foil-option"
          ><input
            type="checkbox"
            bind:checked={foil}
            disabled={busy || stock === "P10"}
          />Foil finish</label
        >
      </div>
      <div class="new-order-cardback">
        {#if commonBack}<img
            src={commonBack.thumb}
            alt={commonBack.label}
          />{:else}<span class="back-mini"><Icon size={24} /></span>{/if}
        <div>
          <strong
            >{commonBack
              ? commonBack.label
              : "Choose a common card back"}</strong
          >
          <p>Used for cards without a custom reverse.</p>
        </div>
        <button
          class="button secondary small"
          disabled={busy || searching}
          on:click={browseBacks}
          >{commonBack ? "Change card back" : "Choose card back"}<Icon
            name="image"
            size={14}
          /></button
        >
      </div>
    {/if}
    <div class="editor-label">
      <label for="decklist-text">Decklist</label><button
        class="button secondary small"
        disabled={busy}
        on:click={importText}
        ><Icon name="folder" size={14} />Import .txt</button
      >
    </div>
    <textarea
      id="decklist-text"
      bind:value={text}
      on:input={invalidate}
      disabled={busy}
      spellcheck="false"
      placeholder={"1 Lightning Bolt\n4 Island\n1 Delver of Secrets | Insectile Aberration"}
      aria-describedby="decklist-help"
    />
    <p id="decklist-help" class="editor-help">
      One card per line: <code>4 Island</code> or <code>4x Island</code>. Use
      <code>Front name | Back name</code> for double-faced cards. Blank lines and
      # comments are ignored. Maximum 612 cards.
    </p>
    {#if error}<div class="notice error-notice" role="alert">
        <Icon name="warning" />
        <p>{error}</p>
      </div>{/if}
    {#if preview}
      <div class="draft-summary" role="status">
        <Icon name={remaining.length || needsBack ? "image" : "check"} />
        <div>
          <strong>{preview.quantity} cards in this decklist</strong>
          <p>
            {remaining.length
              ? `Choose artwork for ${remaining.length} new image${remaining.length === 1 ? "" : "s"} before applying.`
              : needsBack
                ? "Choose a common card back to finish your deck."
                : mode === "create"
                  ? "Your deck is ready to create."
                  : "Artwork is resolved. Your changes are ready to apply."}
          </p>
        </div>
      </div>
      {#if preview.unresolved.length}
        <ul class="artwork-requests">
          {#each preview.unresolved as item}<li>
              <div>
                <strong>{item.query}</strong><span>{item.face} image</span>
              </div>
              <button
                class="button secondary small"
                disabled={busy || searching}
                on:click={() => {
                  artworkQuery = item.query;
                  void search(item);
                }}
                >{selections.some(
                  (s) => s.query === item.query && s.face === item.face,
                )
                  ? "Change artwork"
                  : "Choose artwork"}<Icon name="image" size={14} /></button
              >{#if selections.some((s) => s.query === item.query && s.face === item.face)}<span
                  class="art-selected"
                  ><Icon name="check" size={14} />Selected</span
                >{/if}
            </li>{/each}
        </ul>
      {/if}
    {/if}
    {#if active}
      <section
        bind:this={artworkPanel}
        class="artwork-picker"
        aria-label={`Choose artwork for ${active.query}`}
      >
        <div class="artwork-picker-heading">
          <div>
            <p class="eyebrow">MPC AUTOFILL ARTWORK</p>
            <h3>{active.query}</h3>
          </div>
          <button
            class="icon-button"
            aria-label="Close artwork search"
            on:click={() => {
              active = null;
              generation += 1;
              searching = false;
            }}><Icon name="close" size={16} /></button
          >
        </div>
        <form
          class="artwork-search"
          on:submit|preventDefault={() => active && search(active)}
        >
          <label class="search-field"
            ><Icon name="search" size={16} /><input
              bind:value={artworkQuery}
              aria-label="Artwork search name"
            /></label
          ><button
            class="button secondary small"
            disabled={searching ||
              (active.face !== "common" && !artworkQuery.trim())}>Search</button
          >
        </form>
        {#if searching}<p class="editor-help" role="status">
            Searching MPC Autofill…
          </p>{/if}
        {#if searchError}<p class="notice error-notice" role="alert">
            Search failed: {searchError}. You can try again; your draft is still
            here.
          </p>{/if}
        {#if results}
          <p class="editor-help">
            {results.count} matching artworks · select an image to use it.
          </p>
          <div class="artwork-grid">
            {#each results.cards as art}<button
                class="artwork-choice"
                on:click={() => choose(art)}
                ><span class="artwork-thumb"
                  ><img src={art.thumb} alt={art.label} loading="lazy" /></span
                ><strong>{art.label}</strong><span
                  >{art.source} · {art.dpi} DPI</span
                ><span class="artwork-select-label">Use this artwork</span
                ></button
              >{/each}
          </div>
          {#if !results.cards.length}<p class="no-artwork">
              No matching artwork. Try the card’s full name or a different
              spelling.
            </p>{/if}
          <div class="artwork-pagination">
            <button
              class="button secondary small"
              disabled={page === 0 || searching}
              on:click={() => active && search(active, page - 1)}
              >Previous</button
            ><span>Page {page + 1}</span><button
              class="button secondary small"
              disabled={results.next_page === null || searching}
              on:click={nextPage}>Next</button
            >
          </div>
        {/if}
      </section>
    {/if}
  </div>
  <footer class="editor-footer">
    <span
      >{mode === "create"
        ? "Create your deck, then save it as an XML order."
        : "Apply changes, then save your order as XML."}</span
    ><button class="button secondary" disabled={busy} on:click={close}
      >Cancel</button
    >{#if preview && !remaining.length}<button
        class="button primary"
        disabled={busy || needsBack || (mode === "create" && !orderName.trim())}
        on:click={apply}
        >{busy
          ? "Applying…"
          : mode === "create"
            ? "Create deck"
            : "Apply changes"}<Icon name="check" size={16} /></button
      >{:else}<button
        class="button primary"
        disabled={busy || !text.trim()}
        on:click={review}
        >{busy ? "Checking…" : "Review decklist"}<Icon
          name="arrow"
          size={16}
        /></button
      >{/if}
  </footer>
</dialog>
