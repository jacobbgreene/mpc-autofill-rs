<script lang="ts">
  import { createEventDispatcher, onMount, onDestroy, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { open, confirm } from "@tauri-apps/plugin-dialog";
  import Icon from "./Icon.svelte";
  import ArtworkCard from "./ArtworkCard.svelte";
  import ArtworkGallery from "./ArtworkGallery.svelte";
  import ArtworkThumbnail from "./ArtworkThumbnail.svelte";
  import {
    createArtworkSearch,
    type ArtworkChoice,
    type ArtworkResults,
  } from "./artwork";
  import {
    createDraftController,
    imageKey,
    lineRange,
    retainedChoice,
    type DraftPreview,
    type DraftState,
  } from "./draft";
  import type { OrderSummary } from "./order";

  export let mode: "edit" | "replace" | "create";
  export let initialText = "";
  export let initialName = "New deck";
  export let currentOrder: OrderSummary | null = null;
  export let replacingUnsaved = false;
  export let dirty = false;
  const dispatch = createEventDispatcher<{
    close: void;
    applied: OrderSummary;
  }>();
  const artwork = createArtworkSearch((query, page, kind) =>
    invoke<ArtworkResults>("search_artwork", { query, page, kind }),
  );
  let dialog: HTMLDialogElement;
  let textarea: HTMLTextAreaElement;
  let gutter: HTMLDivElement;
  let matchAlert: HTMLDivElement;
  let text = initialText.replace(/\r\n?/g, "\n");
  let originalText = text;
  const existing = mode === "create" ? null : currentOrder;
  let orderName = existing?.name ?? "";
  let stock = existing?.stock.match(/S27|S30|S33|M31|P10/)?.[0] ?? "S30";
  let foil = existing?.foil ?? false;
  const originalStock = stock;
  const originalBack = existing?.cards.find((card) => card.cardback);
  let commonBack: ArtworkChoice | null = originalBack
    ? retainedChoice(originalBack)
    : null;
  let backPreviewFailed = false;
  let backPreviewRetry = 0;
  let busy = true;
  let error = "";
  let closingDraft = false;
  let disposed = false;
  let backGallery = false;
  let filter: "all" | "attention" = "all";
  let textRevision = 0;
  let focusImportRevision: number | null = text.trim() ? 0 : null;
  let pendingFocusRevision = -1;
  let pastingWholeDeck = false;
  let composing = false;
  let textPane: HTMLElement;
  let artworkPane: HTMLElement;
  let state: DraftState = {
    preview: null,
    images: [],
    updating: false,
    error: "",
  };
  $: effectiveName = orderName.trim() || initialName.trim() || "New deck";
  $: if (stock === "P10") foil = false;
  $: preview = state.preview;
  $: attention = state.images.filter((image) => image.status !== "selected");
  $: visibleImages = filter === "attention" ? attention : state.images;
  $: loading = state.images.filter((image) => image.busy).length;
  $: remaining = state.images.filter((image) => !image.choice).length;
  $: selections = state.images.flatMap(({ item, choice, modified }) =>
    choice && modified
      ? [
          {
            query: item.query,
            face: item.face,
            drive_id: choice.art.drive_id,
            name: choice.art.name,
          },
        ]
      : [],
  );
  $: needsBack = !!preview?.needs_common_back && !commonBack;
  $: hasBackItem = !!preview?.needs_common_back || !!preview?.common_back;
  $: showBackItem = hasBackItem && (filter === "all" || needsBack);
  $: issues = text.trim() ? (preview?.issues ?? []) : [];
  $: attentionCount =
    attention.length +
    Number(needsBack) +
    issues.length +
    Number(!!state.error);
  $: unmatched = state.images.filter(
    (image) => image.status === "no-match" && !image.choice,
  );
  $: problemLines = new Set([
    ...unmatched.flatMap((image) => image.item.source_lines),
    ...(preview?.issues.flatMap((issue) => (issue.line ? [issue.line] : [])) ??
      []),
  ]);
  $: autoFocusUnmatched(state, textRevision, busy || composing || backGallery);
  $: canApply =
    !!preview &&
    !state.updating &&
    !state.error &&
    !preview.issues.length &&
    preview.quantity > 0 &&
    !remaining &&
    !loading &&
    !needsBack &&
    !busy;
  $: dirty =
    !disposed &&
    (text !== originalText ||
      (mode !== "edit" && !!text.trim()) ||
      selections.length > 0 ||
      commonBack?.art.drive_id !== originalBack?.drive_id ||
      orderName !== (existing?.name ?? "") ||
      stock !== originalStock ||
      foil !== (existing?.foil ?? false));
  $: lineNumbers = Array.from(
    { length: text.split("\n").length },
    (_, i) => i + 1,
  );
  const draft = createDraftController({
    preview: (value) =>
      mode === "create"
        ? invoke<DraftPreview>("preview_new_decklist", {
            text: value,
            selections: [],
            options: { ...newOrderOptions(), cardback: null },
          })
        : invoke<DraftPreview>("preview_decklist", {
            text: value,
            selections: [],
          }),
    search: artwork.search,
    changed: (value) => (state = value),
  });
  onMount(() => {
    dialog.showModal();
    if (mode === "edit") {
      invoke<string>("get_decklist")
        .then((value) => {
          if (!disposed) {
            text = value.replace(/\r\n?/g, "\n");
            originalText = text;
            focusImportRevision = textRevision;
            draft.update(text, true);
          }
        })
        .catch((e) => {
          if (!disposed) error = String(e);
        })
        .finally(() => {
          if (!disposed) busy = false;
        });
    } else {
      busy = false;
      if (text.trim()) draft.update(text, true);
    }
  });
  onDestroy(() => {
    disposed = true;
    dirty = false;
    draft.dispose();
    artwork.dispose();
    dialog?.close();
  });
  function updateText(event: Event) {
    error = "";
    textRevision++;
    focusImportRevision =
      pastingWholeDeck && (event as InputEvent).inputType === "insertFromPaste"
        ? textRevision
        : null;
    pastingWholeDeck = false;
    draft.update(text);
  }
  function stopAutoFocus() {
    focusImportRevision = null;
  }
  function autoFocusUnmatched(
    value: DraftState,
    revision: number,
    blocked: boolean,
  ) {
    if (
      blocked ||
      value.updating ||
      focusImportRevision !== revision ||
      pendingFocusRevision === revision ||
      !dialog?.open
    )
      return;
    const first = value.images
      .filter((image) => image.status === "no-match" && !image.choice)
      .sort((a, b) => a.item.source_lines[0] - b.item.source_lines[0])[0];
    if (!first) return;
    // Guide a newly imported list once. Editing or moving the caret cancels
    // pending guidance; background validation must never interrupt corrections.
    pendingFocusRevision = revision;
    void focusLine(first.item.source_lines[0], revision).then((focused) => {
      if (focused && focusImportRevision === revision)
        focusImportRevision = null;
      if (pendingFocusRevision === revision) pendingFocusRevision = -1;
    });
  }
  async function focusLine(line: number, revision?: number) {
    if (revision === undefined) stopAutoFocus();
    await tick();
    if (disposed) return false;
    // Disabling Import while its native picker is open can leave focus on
    // the document body. That is still this editor's import, unless another
    // dialog has opened in the meantime.
    const activeElement = document.activeElement;
    const ownsFocus =
      activeElement?.closest("dialog") === dialog ||
      (activeElement === document.body &&
        !Array.from(document.querySelectorAll("dialog[open]")).some(
          (openDialog) => openDialog !== dialog,
        ));
    if (
      revision !== undefined &&
      (revision !== textRevision ||
        focusImportRevision !== revision ||
        state.updating ||
        composing ||
        !ownsFocus)
    )
      return false;
    const range = lineRange(text, line);
    textarea.focus({ preventScroll: true });
    textarea.setSelectionRange(range.start, range.end);
    textarea.scrollTop = Math.max(
      0,
      (line - 1) * parseFloat(getComputedStyle(textarea).lineHeight),
    );
    gutter.scrollTop = textarea.scrollTop;
    // At enlarged scaling, keeping the whole textarea in view can scroll its
    // error heading away. Show the alert and the selected line together.
    textPane.scrollIntoView({ block: "start" });
    textPane.scrollTop = 0;
    const panes = textPane.parentElement;
    if (panes && getComputedStyle(panes).display === "block") {
      const style = getComputedStyle(textarea);
      const lineBottom =
        textarea.getBoundingClientRect().top +
        parseFloat(style.paddingTop) +
        line * parseFloat(style.lineHeight) -
        textarea.scrollTop;
      panes.scrollTop += Math.max(
        0,
        lineBottom - panes.getBoundingClientRect().bottom + 2,
      );
    }
    return true;
  }
  async function close() {
    if (busy || closingDraft) return;
    closingDraft = true;
    error = "";
    try {
      if (
        dirty &&
        !(await confirm(
          "Discard this draft decklist? Your current order will stay unchanged.",
          {
            title: "Discard draft",
            kind: "warning",
            okLabel: "Discard draft",
            cancelLabel: "Keep editing",
          },
        ))
      )
        return;
      dispatch("close");
    } catch (e) {
      error = `Could not close this draft: ${String(e)}`;
    } finally {
      closingDraft = false;
    }
  }
  async function importText() {
    busy = true;
    error = "";
    try {
      const path = await open({
        multiple: false,
        filters: [{ name: "Text decklist", extensions: ["txt"] }],
      });
      if (typeof path !== "string") return;
      if (
        dirty &&
        !(await confirm(
          "Replace the text in this draft with the selected file?",
          { title: "Replace draft", kind: "warning" },
        ))
      )
        return;
      text = (await invoke<string>("read_decklist", { path })).replace(
        /\r\n?/g,
        "\n",
      );
      textRevision++;
      focusImportRevision = textRevision;
      draft.update(text, true);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
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
    if (!canApply) return;
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
          : await invoke<OrderSummary>("edit_decklist", {
              text,
              selections,
              options: newOrderOptions(),
            });
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
  class="editor-dialog deck-editor import-workspace"
  aria-labelledby="editor-title"
  on:cancel={(event) => {
    event.preventDefault();
    void close();
  }}
>
  <header class="editor-header">
    <div>
      <p class="eyebrow">TEXT & ARTWORK</p>
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
      disabled={busy || closingDraft}
      on:click={close}><Icon name="close" /></button
    >
  </header>
  <section
    class="workspace-print-settings"
    aria-labelledby="print-settings-title"
  >
    <h3 id="print-settings-title">Print settings</h3>
    <div class="new-order-settings">
      <label class="order-name-field"
        >Deck name<input
          bind:value={orderName}
          placeholder={initialName.trim() || "New deck"}
          maxlength="200"
          disabled={busy}
        /></label
      >
      <label
        >Cardstock<select
          aria-label="Cardstock"
          bind:value={stock}
          disabled={busy}
          ><option value="S27">S27 · Smooth</option><option value="S30"
            >S30 · Standard Smooth</option
          ><option value="S33">S33 · Superior Smooth</option><option value="M31"
            >M31 · Linen</option
          ><option value="P10">P10 · Plastic</option></select
        ></label
      >
      <label class="foil-option"
        ><input
          type="checkbox"
          bind:checked={foil}
          disabled={busy || stock === "P10"}
        />Foil finish</label
      >
    </div>
    {#if existing}<p class="settings-save-hint">
        Changes apply to this draft. Save writes the XML; a new deck name uses
        Save as.
      </p>{/if}
  </section>
  {#if error}<div class="notice error-notice workspace-error" role="alert">
      {error}
    </div>{/if}
  <nav class="workspace-jumps" aria-label="Jump to import pane">
    <button
      class="button secondary small"
      on:click={() => {
        textPane.scrollIntoView({ block: "start" });
        textarea.focus({ preventScroll: true });
      }}
      >Decklist{#if issues.length}
        ({issues.length}){/if}</button
    >
    <button
      class="button secondary small"
      on:click={() => {
        artworkPane.scrollIntoView({ block: "start" });
        artworkPane.focus({ preventScroll: true });
      }}>Artwork</button
    >
  </nav>
  <div class="workspace-panes">
    <section
      class="deck-text-pane"
      bind:this={textPane}
      aria-label="Decklist text and problems"
    >
      <div class="editor-label">
        <label for="decklist-text">Decklist</label><button
          class="button secondary small"
          disabled={busy}
          on:click={importText}
          ><Icon name="folder" size={14} />Import .txt</button
        >
      </div>
      <p id="decklist-help" class="editor-help">
        Edit names or quantities here. Artwork updates after you pause. Use <code
          >4 Island</code
        >
        or <code>Front | Back</code>; maximum 612 cards.
      </p>
      {#if unmatched.length && !state.updating}<div
          class="draft-match-alert"
          bind:this={matchAlert}
          id="decklist-match-alert"
        >
          <Icon name="warning" size={20} />
          <div>
            <strong
              >{unmatched.length === 1
                ? "Card not found"
                : `${unmatched.length} entries not found`}</strong
            >
            <p>
              Correct the highlighted {unmatched.length === 1
                ? "entry"
                : "entries"} in your decklist.
            </p>
            <div class="draft-match-links">
              {#each unmatched as image}<button
                  class="text-button"
                  on:click={() => focusLine(image.item.source_lines[0])}
                  >Line {image.item.source_lines[0]} · {image.item
                    .query}</button
                >{/each}
            </div>
          </div>
        </div>{/if}
      <div
        class="numbered-decklist"
        class:has-problems={problemLines.size > 0 && !state.updating}
      >
        <div class="decklist-gutter" bind:this={gutter} aria-hidden="true">
          {#each lineNumbers as line}<div
              class:problem-line={problemLines.has(line) && !state.updating}
            >
              {line}
            </div>{/each}
        </div>
        <textarea
          id="decklist-text"
          bind:this={textarea}
          bind:value={text}
          on:input={updateText}
          on:paste={() => {
            pastingWholeDeck =
              textarea.selectionStart === 0 &&
              textarea.selectionEnd === text.length;
          }}
          on:keydown={stopAutoFocus}
          on:pointerdown={stopAutoFocus}
          on:compositionstart={() => {
            composing = true;
            stopAutoFocus();
          }}
          on:compositionend={() => (composing = false)}
          on:scroll={() => (gutter.scrollTop = textarea.scrollTop)}
          disabled={busy}
          spellcheck="false"
          wrap="off"
          placeholder={"1 Lightning Bolt\n4 Island\n1 Delver of Secrets | Insectile Aberration"}
          aria-invalid={problemLines.size > 0 && !state.updating}
          aria-describedby={unmatched.length && !state.updating
            ? "decklist-help decklist-match-alert"
            : "decklist-help"}
        ></textarea>
      </div>
      <div
        class="draft-diagnostics"
        aria-label="Decklist problems"
        aria-busy={state.updating}
      >
        {#if state.error}<p role="alert">{state.error}</p>
          <button
            class="text-button"
            disabled={busy}
            on:click={() => draft.update(text, true)}>Retry preview</button
          >{/if}
        {#if !state.updating && text.trim()}
          {#each preview?.issues ?? [] as issue}<p class="draft-issue">
              {#if issue.line}<button
                  class="text-button"
                  on:click={() => focusLine(issue.line ?? 1)}
                  >Line {issue.line}</button
                >:
              {/if}{issue.message}
            </p>{/each}
          {#each attention.filter((image) => !image.busy && image.status !== "no-match") as image}<p
              class="draft-issue"
            >
              <button
                class="text-button"
                on:click={() => focusLine(image.item.source_lines[0])}
                >Line {image.item.source_lines[0]} · {image.item.query}</button
              >
              — Search failed
            </p>{/each}
        {/if}
      </div>
    </section>
    <section
      class="editor-body deck-art-pane"
      bind:this={artworkPane}
      tabindex="-1"
      aria-label="Deck artwork"
      aria-busy={state.updating}
    >
      <div class="deck-artwork-heading">
        <div>
          <strong
            >{preview
              ? `${preview.quantity} cards${preview.issues.length ? " · Preview incomplete" : " in your deck"}`
              : "Your artwork"}</strong
          >
          <p>
            {mode === "edit"
              ? "Your retained artwork is shown alongside new entries."
              : "The artwork shown is what will be used."}
          </p>
        </div>
      </div>
      <div
        class="draft-grid-filter"
        role="group"
        aria-label="Filter draft artwork"
      >
        <button
          class="button secondary small"
          aria-pressed={filter === "all"}
          on:click={() => (filter = "all")}
          >All ({state.images.length + Number(hasBackItem)})</button
        ><button
          class="button secondary small"
          aria-pressed={filter === "attention"}
          on:click={() => (filter = "attention")}
          >Needs attention ({attentionCount})</button
        >
      </div>
      {#if state.updating}<p class="editor-help">Updating preview…</p>{/if}
      {#if state.images.length}<p class="artwork-auto-note">
          New entries start with the first matching artwork. Edit the decklist
          to correct a name.
        </p>{/if}
      {#if filter === "attention" && !state.updating}
        {#if state.error}<div class="draft-match-alert">
            <div>
              <strong>Preview unavailable</strong>
              <p>{state.error}</p>
              <button
                class="text-button"
                on:click={() => draft.update(text, true)}>Retry preview</button
              >
            </div>
          </div>{/if}
        {#each issues as issue}<div class="draft-match-alert">
            <div>
              <strong>Decklist needs correction</strong>
              <p>{issue.message}</p>
              <button
                class="text-button"
                on:click={() => focusLine(issue.line ?? 1)}
                >{issue.line
                  ? `Edit line ${issue.line}`
                  : "Edit decklist"}</button
              >
            </div>
          </div>{/each}
      {/if}
      <div class="artwork-deck-grid">
        {#if showBackItem}<div class="cardback-row">
          <article
            class="artwork-card deck-cardback"
            aria-label="Common card back"
          >
            <header>
              <h3>Card back</h3>
              {#if commonBack}<span>Back</span>{/if}
            </header>
            <div class="artwork-carousel">
              <button
                class="artwork-card-preview cardback-choice-preview"
                aria-label={commonBack
                  ? "Change card-back artwork"
                  : "Browse card-back artwork"}
                disabled={busy || state.updating}
                on:click={() => (backGallery = true)}
              >
                {#if commonBack}{#key commonBack.art.drive_id}<ArtworkThumbnail
                      driveId={commonBack.art.drive_id}
                      label={commonBack.art.label}
                      retry={backPreviewRetry}
                      on:error={() => (backPreviewFailed = true)}
                      on:loaded={() => (backPreviewFailed = false)}
                    />{/key}
                {:else}<span class="artwork-placeholder cardback-placeholder">
                    <Icon name="cards" size={36} />
                    <span>Choose your card back</span>
                  </span>{/if}
              </button>
            </div>
            <div class="artwork-card-details">
              {#if commonBack}<p class="artwork-match-status">
                  Card back selected
                </p>{/if}
              <p class="cardback-help">
                Used for cards without their own reverse.
              </p>
              {#if backPreviewFailed}<button
                  class="text-button retry-preview"
                  disabled={busy || state.updating}
                  on:click={() => {
                    backPreviewFailed = false;
                    backPreviewRetry++;
                  }}>Retry preview</button
                >{/if}
              <button
                class:primary={needsBack}
                class:secondary={!needsBack}
                class="button small browse-artwork"
                disabled={busy || state.updating}
                on:click={() => (backGallery = true)}
                >{commonBack ? "Change card back" : "Choose card back"}</button
              >
            </div>
          </article>
        </div>{/if}
        {#each visibleImages as image (imageKey(image.item))}<ArtworkCard
            {image}
            disabled={busy || state.updating}
            search={artwork.search}
            on:choose={(event) => draft.choose(image.item, event.detail)}
            on:load={(event) => draft.load(image.item, event.detail)}
            on:retry={() => draft.retry(image.item)}
            on:edit={(event) => focusLine(event.detail)}
          />{/each}
      </div>
      {#if !visibleImages.length && !showBackItem && !needsBack && !state.updating && !issues.length && !state.error}<p
          class="editor-help"
        >
          {filter === "attention"
            ? "No artwork needs attention."
            : !text.trim()
              ? "Paste or import a decklist to see artwork here."
              : preview && !preview.issues.length
                ? "Your existing artwork is ready."
                : "Correct the highlighted problems to complete your preview."}
        </p>{/if}
    </section>
  </div>
  <footer class="editor-footer">
    <span
      >{state.updating
        ? "Updating preview…"
        : !text.trim()
          ? "Paste or import a decklist to begin."
          : state.error
            ? "Retry the preview or edit the text."
            : preview?.issues.length
              ? `${preview.issues.length} decklist problem(s) to correct.`
              : unmatched.length
                ? `${unmatched.length} ${unmatched.length === 1 ? "entry" : "entries"} not found. Correct the highlighted text.`
                : loading
                  ? `Finding artwork for ${loading} image(s)…`
                  : remaining
                    ? `${remaining} image(s) need artwork.`
                    : needsBack
                      ? "Choose your card back to continue."
                      : "Ready. Save your order as XML after applying."}</span
    >
    <button
      class="button secondary"
      disabled={busy || closingDraft}
      on:click={close}>Cancel</button
    >
    <button class="button primary" disabled={!canApply} on:click={apply}
      >{busy
        ? "Working…"
        : mode === "create"
          ? "Create deck"
          : "Apply changes"}<Icon name="check" size={16} /></button
    >
  </footer>
  <p class="sr-only" role="status" aria-atomic="true">
    {state.updating
      ? "Validating decklist."
      : loading
        ? "Finding artwork."
        : !text.trim()
          ? "Paste or import a decklist to begin."
          : attentionCount
            ? `${attentionCount} items need attention. Use the text links or Needs attention filter to review them.`
            : "Artwork ready. You can apply your deck."}
  </p>
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
      backPreviewFailed = false;
      backPreviewRetry = 0;
      backGallery = false;
    }}
  />{/if}
