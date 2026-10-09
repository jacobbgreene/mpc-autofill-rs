<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { open, confirm } from "@tauri-apps/plugin-dialog";
  import Icon from "./Icon.svelte";
  import ArtworkCard from "./ArtworkCard.svelte";
  import ArtworkGallery from "./ArtworkGallery.svelte";
  import ArtworkThumbnail from "./ArtworkThumbnail.svelte";
  import { cardBackChanged, initialCardBack } from "./favoriteBacks";
  import {
    createArtworkSearch,
    type ArtworkChoice,
    type ArtworkResults,
  } from "./artwork";
  import type { DeckDraft, OrderSummary } from "./order";

  export let mode: "edit" | "replace" | "create";
  export let initialText = "";
  export let initialName = "New deck";
  export let replacingUnsaved = false;
  export let initialDraft: DeckDraft | null = null;
  const dispatch = createEventDispatcher<{
    close: void;
    applied: OrderSummary;
  }>();
  type Missing = { query: string; face: "front" | "back" };
  type Preview = {
    quantity: number;
    unresolved: Missing[];
    needs_common_back: boolean;
  };
  const newArtworkSearch = () =>
    createArtworkSearch((query, page, kind) =>
      invoke<ArtworkResults>("search_artwork", { query, page, kind }),
    );
  let artwork = newArtworkSearch();
  const key = (item: Missing) =>
    JSON.stringify([item.query.toLowerCase(), item.face]);
  let dialog: HTMLDialogElement;
  let body: HTMLDivElement;
  let grid: HTMLDivElement;
  let text = initialDraft?.text ?? initialText;
  let originalText = initialDraft?.text ?? initialText;
  let orderName = initialDraft?.name ?? "";
  $: effectiveName = orderName.trim() || initialName.trim() || "New deck";
  let stock = initialDraft?.stock ?? "S30";
  let foil = initialDraft?.foil ?? false;
  let commonBack: ArtworkChoice | null =
    mode === "create" ? initialCardBack(initialDraft) : null;
  const startingCommonBackId = commonBack?.art.drive_id ?? null;
  let busy = true;
  let error = "";
  let preview: Preview | null = null;
  let choices: Record<string, ArtworkChoice> = initialDraft?.choices ?? {};
  let loading = new Set<string>();
  let showText = true;
  let backGallery = false;
  $: selections = (preview?.unresolved ?? []).flatMap((item) => {
    const choice = choices[key(item)];
    return choice
      ? [{ ...item, drive_id: choice.art.drive_id, name: choice.art.name }]
      : [];
  });
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
    artwork.dispose();
  });

  function invalidate() {
    preview = null;
    loading = new Set();
    artwork.dispose();
    artwork = newArtworkSearch();
    error = "";
  }
  async function close() {
    if (busy) return;
    const changed = initialDraft
      ? JSON.stringify({ text, name: effectiveName, stock, foil, choices, commonBack }) !==
        JSON.stringify({
          text: initialDraft.text,
          name: initialDraft.name,
          stock: initialDraft.stock,
          foil: initialDraft.foil,
          choices: initialDraft.choices,
          commonBack: initialDraft.common_back,
        })
      : text !== originalText ||
        selections.length > 0 ||
        cardBackChanged(commonBack, startingCommonBackId) ||
        (mode === "create" && (orderName.trim() || stock !== "S30" || foil));
    if (
      changed &&
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
      await review();
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
              selections: [],
              options: { ...newOrderOptions(), cardback: null },
            })
          : await invoke<Preview>("preview_decklist", { text, selections: [] });
      showText = false;
      loading = new Set(
        preview.unresolved.filter((item) => !choices[key(item)]).map(key),
      );
      await tick();
      if (window.innerHeight <= 660 && grid) {
        body.scrollTop +=
          grid.getBoundingClientRect().top -
          body.getBoundingClientRect().top -
          12;
      }
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
  function setLoading(item: Missing, value: boolean) {
    if (value) loading.add(key(item));
    else loading.delete(key(item));
    loading = new Set(loading);
  }
  function newOrderOptions() {
    return {
      name: effectiveName,
      stock,
      foil,
      cardback: commonBack
        ? { drive_id: commonBack.art.drive_id, name: commonBack.art.name }
        : null,
    };
  }
  async function apply() {
    if (busy) return;
    if (mode === "create" ? !text.trim() : !preview || !!remaining.length || needsBack || !!loading.size) return;
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
              draft: {
                version: 1,
                text,
                name: effectiveName,
                stock,
                foil,
                choices,
                common_back: commonBack,
              } satisfies DeckDraft,
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
  class="editor-dialog deck-editor"
  class:has-artwork={!!preview}
  aria-labelledby="editor-title"
  on:cancel={(event) => {
    event.preventDefault();
    void close();
  }}
>
  <header class="editor-header">
    <div>
      <p class="eyebrow">
        {preview ? "MAKE IT YOUR DECK" : "YOUR ORDER, YOUR WAY"}
      </p>
      <h2 id="editor-title">
        {preview
          ? "Choose your artwork"
          : mode === "create"
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
  <div class="editor-body" bind:this={body}>
    {#if error}<div class="notice error-notice" role="alert">
        <Icon name="warning" />
        <p>{error}</p>
      </div>{/if}
    {#if preview}
      <div class="deck-artwork-heading">
        <div>
          <strong>{preview.quantity} cards in your deck</strong>
          <p>
            Use the arrows or enter an artwork number. Open the gallery to
            compare options.
          </p>
        </div>
        <button
          class="button secondary small"
          disabled={busy}
          on:click={() => (showText = !showText)}
          >{showText ? "Hide decklist" : "Edit decklist"}</button
        >
      </div>
    {/if}
    {#if showText}
      <div class="decklist-input-panel">
        {#if !preview}<p class="editor-description">
            {mode === "create"
              ? "Paste your card list or import a .txt file to start choosing artwork."
              : mode === "edit"
                ? "Change quantities, remove cards, or add something new. Matching cards keep their artwork."
                : "Paste another decklist or import a .txt file. Cards matching this order reuse their artwork; new cards need artwork choices."}
          </p>{/if}
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
          placeholder={"1 Lightning Bolt\n4 Island\n1 Delver of Secrets // Insectile Aberration"}
          aria-describedby="decklist-help"
        />
        <p id="decklist-help" class="editor-help">
          One card per line: <code>4 Island</code> or <code>4x Island</code>.
          Use <code>Front name // Back name</code> or <code>Front name | Back name</code> for double-faced cards. Blank
          lines and whole-line # or // comments are ignored, as are headings like
          <code>CREATURES (20)</code> or <code>Sideboard</code>. Maximum 612 cards.
        </p>
      </div>
    {/if}
    {#if mode === "create"}
      <details class="deck-print-settings" open={!preview}>
        <summary
          >Print settings <span
            >{effectiveName} · {stock}{foil ? " · Foil" : ""}</span
          ></summary
        >
        <div class="new-order-settings">
          <label class="order-name-field"
            >Deck name<input
              bind:value={orderName}
              placeholder={initialName.trim() || "New deck"}
              maxlength="200"
              disabled={busy}
            /><small>Saved automatically as XML in Saved decks.</small></label
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
      </details>
    {/if}
    {#if preview}
      {#if preview.unresolved.length}
        <div class="artwork-deck-grid" bind:this={grid}>
          {#each preview.unresolved as item (key(item))}
            <ArtworkCard
              name={item.query}
              face={item.face}
              search={artwork.search}
              initial={choices[key(item)] ?? null}
              disabled={busy}
              on:choose={(event) =>
                (choices = { ...choices, [key(item)]: event.detail })}
              on:loading={(event) => setLoading(item, event.detail)}
            />
          {/each}
        </div>
        <p class="artwork-auto-note">
          The first match is selected to get you started. The artwork shown on
          each card is what will be used.
        </p>
      {:else}<div class="draft-summary">
          <Icon name="check" />
          <div>
            <strong>Your existing artwork is ready</strong>
            <p>All cards matched images already in this order.</p>
          </div>
        </div>{/if}
      {#if mode === "create" && preview.needs_common_back}
        <div class="new-order-cardback">
          {#if commonBack}<span class="chosen-back-preview"
              ><ArtworkThumbnail
                driveId={commonBack.art.drive_id}
                label={commonBack.art.label}
              /></span
            >{:else}<span class="back-mini"><Icon size={24} /></span>{/if}
          <div>
            <strong
              >{commonBack
                ? commonBack.art.label
                : "Choose a common card back"}</strong
            >
            <p>Used for cards without a custom reverse.</p>
          </div>
          <button
            class="button secondary small"
            disabled={busy}
            on:click={() => (backGallery = true)}
            >{commonBack ? "Change card back" : "Choose card back"}<Icon
              name="grid"
              size={14}
            /></button
          >
        </div>
      {/if}
    {/if}
  </div>
  <footer class="editor-footer">
    <span
      ><span role="status"
        >{mode === "create"
          ? !preview
            ? "Create deck will save an XML draft if this list needs fixes."
            : loading.size || remaining.length || needsBack
              ? "Create deck saves an XML draft. Finish choosing artwork and a back before autofill."
              : "Ready to create and save this XML deck in Saved decks."
          : !preview
            ? "Your card previews will appear in the next step."
            : loading.size
              ? `Finding artwork for ${loading.size} image${loading.size === 1 ? "" : "s"}…`
              : remaining.length
                ? `${remaining.length} image${remaining.length === 1 ? " needs" : "s need"} artwork. Open a gallery to search.`
                : "Ready to apply. Save your order to keep these changes."}</span
      >
      {#if needsBack}<button
          class="text-button footer-back-link"
          disabled={busy}
          on:click={() => (backGallery = true)}
          >Browse card backs<Icon name="arrow" size={12} /></button
        >{/if}
    </span>
    <button class="button secondary" disabled={busy} on:click={close}
      >Cancel</button
    >
    {#if mode === "create"}
      {#if !preview}<button
          class="button secondary"
          disabled={busy || !text.trim()}
          on:click={review}>Choose artwork</button
        >{/if}
      <button
        class="button primary"
        disabled={busy || !text.trim()}
        on:click={apply}>Create deck<Icon name="check" size={16} /></button
      >
    {:else if preview}<button
        class="button primary"
        disabled={busy || !!loading.size || !!remaining.length || needsBack}
        on:click={apply}
        >{busy ? "Applying…" : "Apply changes"}<Icon name="check" size={16} /></button
      >
    {:else}<button
        class="button primary"
        disabled={busy || !text.trim()}
        on:click={review}
        >{busy ? "Loading cards…" : "Choose artwork"}<Icon
          name="arrow"
          size={16}
        /></button
      >{/if}
  </footer>
</dialog>
{#if backGallery}<ArtworkGallery
    search={artwork.search}
    title="Choose a card back"
    query={commonBack?.query ?? ""}
    kind="cardback"
    selected={commonBack}
    on:close={() => (backGallery = false)}
    on:choose={(event) => {
      commonBack = event.detail;
      backGallery = false;
    }}
  />{/if}
