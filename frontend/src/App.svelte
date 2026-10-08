<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open, save, confirm } from "@tauri-apps/plugin-dialog";
  import Icon from "./lib/Icon.svelte";
  import CardTile from "./lib/CardTile.svelte";
  import CardPreview from "./lib/CardPreview.svelte";
  import ProgressPanel from "./lib/ProgressPanel.svelte";
  import DeckEditor from "./lib/DeckEditor.svelte";
  import ThemePicker from "./lib/ThemePicker.svelte";
  import SavedDecks from "./lib/SavedDecks.svelte";
  import ChangeArtwork from "./lib/ChangeArtwork.svelte";
  import {
    cachedArtworkDpi,
    rememberArtworkDpi,
    dpiLabel,
  } from "./lib/artwork";
  import {
    cardKey,
    displayName,
    filterCards,
    needsAttention,
    patchCards,
    statusLabels,
    type Card,
    type Face,
    type Filter,
    type OrderSummary,
    type Status,
  } from "./lib/order";

  let summary: OrderSummary | null = null;
  let homePage: "welcome" | "saved" = "welcome";
  let cards: Card[] = [];
  let phase = "";
  let busy = false;
  let connected = false;
  let error = "";
  let orderError = "";
  let previewError = "";
  let dpiError = "";
  let dpiGeneration = 0;
  let libraryWarning = "";
  let dragging = false;
  let query = "";
  let filter: Filter = "all";
  let selectedKey: string | null = null;
  let artworkCard: Card | null = null;
  let artworkApplying = false;
  let failedBackThumb: string | null = null;
  let downloadCompleted = 0;
  let downloadFailures = 0;
  let searchInput: HTMLInputElement;
  let editorMode: "edit" | "replace" | "create" | null = null;
  let resumeDraft = false;
  let draftText = "";
  let draftName = "New deck";
  let savedMessage = "";
  let running = false;
  let closing = false;
  let appBar: HTMLElement;
  let appBarHeight = 0;

  const filters: { value: Filter; label: string }[] = [
    { value: "all", label: "All cards" },
    { value: "back", label: "Backs" },
    { value: "attention", label: "Needs attention" },
  ];
  $: cardbacks = cards.filter((card) => card.cardback);
  $: deckImageCount = cards.length - cardbacks.length;
  $: attentionCount = cards.filter(needsAttention).length;
  $: visibleCards = filterCards(cards, filter, query);
  $: selectedCard = cards.find((card) => cardKey(card) === selectedKey) ?? null;

  // Coalesce event bursts without conflating a front's placement with its back.
  let pending: { driveId: string; change: Partial<Card>; face?: Face }[] = [];
  let frame: number | null = null;
  function flush() {
    frame = null;
    for (const update of pending)
      cards = patchCards(cards, update.driveId, update.change, update.face);
    pending = [];
  }
  function patch(driveId: string, change: Partial<Card>, face?: Face) {
    pending.push({ driveId, change, face });
    if (frame === null) frame = requestAnimationFrame(flush);
  }

  onMount(() => {
    let disposed = false;
    const unlisteners: UnlistenFn[] = [];
    const toolbarSize = new ResizeObserver(() => {
      appBarHeight = appBar.clientHeight;
    });
    toolbarSize.observe(appBar);
    async function register(subscription: Promise<UnlistenFn>) {
      const off = await subscription;
      if (disposed) off();
      else unlisteners.push(off);
    }
    async function connect() {
      try {
        await Promise.all([
          register(
            listen<string>("library-warning", ({ payload }) => {
              libraryWarning = payload;
            }),
          ),
          register(
            listen<{ drive_id: string; ok: boolean; error?: string }>(
              "download-progress",
              ({ payload }) => {
                downloadCompleted = Math.min(
                  cards.length,
                  downloadCompleted + 1,
                );
                if (!payload.ok) downloadFailures += 1;
                patch(payload.drive_id, {
                  status: payload.ok ? "downloaded" : "download-failed",
                  error: payload.ok ? undefined : payload.error,
                });
              },
            ),
          ),
          register(
            listen<{ phase: string }>("phase", ({ payload }) => {
              phase = payload.phase;
              if (phase === "downloading") {
                downloadCompleted = 0;
                downloadFailures = 0;
              }
            }),
          ),
          register(
            listen<{ drive_id: string; face: Face; status: Status }>(
              "card-status",
              ({ payload }) => {
                patch(
                  payload.drive_id,
                  { status: payload.status },
                  payload.face,
                );
              },
            ),
          ),
          register(
            listen<{ drive_id: string; path: string }>(
              "thumb-ready",
              ({ payload }) => {
                patch(payload.drive_id, {
                  thumb: convertFileSrc(payload.path),
                });
              },
            ),
          ),
          register(
            getCurrentWebview().onDragDropEvent(({ payload }) => {
              if (busy || editorMode || artworkCard) return;
              if (payload.type === "enter" || payload.type === "over")
                dragging = true;
              else dragging = false;
              if (payload.type === "drop") {
                if (
                  payload.paths.length !== 1 ||
                  !/\.(xml|txt|mpcdeck)$/i.test(payload.paths[0])
                ) {
                  orderError =
                    "Choose one .xml order, .mpcdeck draft, or .txt decklist at a time.";
                  return;
                }
                void openPath(payload.paths[0]);
              }
            }),
          ),
          register(
            getCurrentWindow().onCloseRequested(async (event) => {
              if (
                closing ||
                (!summary?.dirty && !editorMode && !artworkApplying)
              )
                return;
              event.preventDefault();
              if (
                await confirm(
                  "Close without saving your order or draft edits?",
                  { title: "Unsaved changes", kind: "warning" },
                )
              ) {
                closing = true;
                await getCurrentWindow().close();
              }
            }),
          ),
        ]);
        if (!disposed) connected = true;
      } catch (e) {
        if (!disposed)
          orderError = `Could not connect to the desktop app: ${String(e)}`;
      }
    }
    void connect();
    return () => {
      disposed = true;
      dpiGeneration++;
      toolbarSize.disconnect();
      unlisteners.forEach((off) => off());
      if (frame !== null) cancelAnimationFrame(frame);
    };
  });

  async function loadOrder(path: string) {
    if (busy || !connected) return;
    busy = true;
    orderError = "";
    libraryWarning = "";
    dragging = false;
    try {
      if (
        summary?.dirty &&
        !(await confirm(
          "Open another order and discard your unsaved changes?",
          { title: "Unsaved changes", kind: "warning" },
        ))
      )
        return;
      const loaded = await invoke<OrderSummary>("load_order", { path });
      if (frame !== null) cancelAnimationFrame(frame);
      frame = null;
      pending = [];
      summary = loaded;
      cards = loaded.cards.map((card) => ({ ...card, status: "pending" }));
      if (!loaded.draft) void loadDpi();
      phase = "";
      error = "";
      previewError = "";
      downloadCompleted = 0;
      downloadFailures = 0;
      query = "";
      filter = "all";
      selectedKey = null;
      savedMessage = loaded.draft ? "Draft saved" : "Deck saved";
      await tick();
      window.scrollTo({ top: 0 });
      if (!loaded.draft)
        invoke("thumbnails").catch(() => {
          // A thumbnail failure does not prevent full image downloads or autofill.
          if (summary === loaded)
            previewError =
              "Card previews could not be loaded. You can still start autofill.";
        });
    } catch (e) {
      orderError = `Could not open this order: ${String(e)}`;
    } finally {
      busy = false;
    }
  }

  function pasteDecklist() {
    draftText = "";
    draftName = "New deck";
    resumeDraft = false;
    editorMode = "create";
  }

  function replaceWithDecklist(text = "") {
    draftText = text;
    resumeDraft = false;
    editorMode = "replace";
  }

  async function openPath(path: string) {
    if (!path.toLowerCase().endsWith(".txt")) return loadOrder(path);
    if (busy || !connected) return;
    busy = true;
    dragging = false;
    orderError = "";
    try {
      draftText = await invoke<string>("read_decklist", { path });
      draftName =
        path
          .split(/[\\/]/)
          .pop()
          ?.replace(/\.txt$/i, "") || "New deck";
      resumeDraft = false;
      editorMode = summary && !summary.draft ? "replace" : "create";
    } catch (e) {
      orderError = `Could not import this decklist: ${String(e)}`;
    } finally {
      busy = false;
    }
  }

  async function openOrder(textOnly = false) {
    if (busy || !connected) return;
    busy = true;
    orderError = "";
    try {
      const path = await open({
        multiple: false,
        filters: textOnly
          ? [{ name: "Text decklist", extensions: ["txt"] }]
          : summary
            ? [{ name: "Order or draft", extensions: ["xml", "mpcdeck"] }]
            : [{ name: "Order, draft, or decklist", extensions: ["xml", "mpcdeck", "txt"] }],
      });
      busy = false;
      if (typeof path === "string") await openPath(path);
    } catch (e) {
      orderError = `Could not open the file picker: ${String(e)}`;
    } finally {
      busy = false;
    }
  }

  async function fill() {
    if (busy || !summary || summary.draft || !connected) return;
    busy = true;
    running = true;
    error = "";
    phase = "downloading";
    downloadCompleted = 0;
    downloadFailures = 0;
    flush();
    cards = cards.map((card) => ({
      ...card,
      status: "pending",
      error: undefined,
    }));
    try {
      await invoke("fill");
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      running = false;
    }
  }

  function showAttention() {
    query = "";
    filter = "attention";
    searchInput?.focus();
  }

  async function saveOrder(saveAs = false) {
    if (busy || !summary || summary.draft) return;
    busy = true;
    orderError = "";
    savedMessage = "";
    libraryWarning = "";
    try {
      let path: string | null = null;
      if (saveAs || !summary.path) {
        let defaultPath = summary.path;
        if (!defaultPath) {
          const fileName = `${summary.name.replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_")}.xml`;
          try {
            defaultPath = await invoke<string>("default_deck_path", {
              fileName,
            });
          } catch {
            defaultPath = fileName;
            libraryWarning =
              "The default deck folder is unavailable. Choose another save location.";
          }
        }
        path = await save({
          defaultPath,
          filters: [{ name: "Order XML", extensions: ["xml"] }],
        });
        if (!path) return;
      }
      summary = await invoke<OrderSummary>("save_order", { path });
      savedMessage = "Order saved";
    } catch (e) {
      orderError = `Could not save this order: ${String(e)}`;
    } finally {
      busy = false;
    }
  }

  async function browseSavedDecks() {
    if (busy) return;
    busy = true;
    try {
      if (
        summary?.dirty &&
        !(await confirm(
          "Return to saved decks and discard your unsaved changes?",
          { title: "Unsaved changes", kind: "warning" },
        ))
      )
        return;
      if (frame !== null) cancelAnimationFrame(frame);
      frame = null;
      pending = [];
      summary = null;
      homePage = "saved";
      cards = [];
      selectedKey = null;
      orderError = "";
      libraryWarning = "";
      await tick();
      window.scrollTo({ top: 0 });
    } catch (e) {
      orderError = `Could not open saved decks: ${String(e)}`;
    } finally {
      busy = false;
    }
  }

  async function showWelcome() {
    homePage = "welcome";
    orderError = "";
    await tick();
    window.scrollTo({ top: 0 });
  }

  async function loadDpi() {
    const generation = ++dpiGeneration;
    dpiError = "";
    cards = cards.map((card) => ({
      ...card,
      dpi: cachedArtworkDpi(card.drive_id),
    }));
    const ids = [
      ...new Set(
        cards
          .filter((card) => card.dpi === undefined)
          .map((card) => card.drive_id),
      ),
    ];
    if (!ids.length) return;
    try {
      const values = await invoke<Record<string, number | null>>(
        "artwork_dpi",
        { driveIds: ids },
      );
      rememberArtworkDpi(values);
      if (generation !== dpiGeneration) return;
      cards = cards.map((card) => ({
        ...card,
        dpi: cachedArtworkDpi(card.drive_id) ?? null,
      }));
    } catch {
      if (generation !== dpiGeneration) return;
      cards = cards.map((card) => ({ ...card, dpi: card.dpi ?? null }));
      dpiError = "Some artwork DPI values could not be loaded.";
    }
  }

  function changeArtwork(card: Card) {
    if (busy || !connected) return;
    selectedKey = null;
    artworkCard = card;
  }

  async function applyArtworkChange(order: OrderSummary, previewId: string) {
    const face = artworkCard?.face;
    applyEditedOrder(order, true);
    artworkCard = null;
    // Reuse the gallery's cached image while the order thumbnails refresh.
    invoke<string>("artwork_thumbnail", { driveId: previewId })
      .then((path) => patch(previewId, { thumb: convertFileSrc(path) }))
      .catch(() => {});
    await tick();
    document
      .querySelector<HTMLButtonElement>(
        `:is(.card-change-art, .common-back-change)[data-card-key="${CSS.escape(`${face}:${previewId}`)}"]`,
      )
      ?.focus({ preventScroll: true });
  }

  function applyEditedOrder(edited: OrderSummary, keepView = false) {
    if (frame !== null) cancelAnimationFrame(frame);
    frame = null;
    pending = [];
    const thumbnails = new Map(
      cards.map((card) => [card.drive_id, card.thumb]),
    );
    summary = edited;
    cards = edited.cards.map((card) => ({
      ...card,
      thumb: thumbnails.get(card.drive_id),
      status: "pending",
    }));
    if (!edited.draft) void loadDpi();
    phase = "";
    error = "";
    orderError = "";
    savedMessage = edited.draft
      ? "Draft saved"
      : !edited.dirty && edited.path
        ? "Deck saved"
        : "";
    previewError = "";
    downloadCompleted = 0;
    downloadFailures = 0;
    if (!keepView) {
      query = "";
      filter = "all";
    }
    editorMode = null;
    if (!edited.draft)
      invoke("thumbnails").catch(
        () =>
          (previewError =
            "Card previews could not be loaded. You can still start autofill."),
      );
  }
</script>

<svelte:window
  on:keydown={(event) => {
    if (
      (event.ctrlKey || event.metaKey) &&
      event.key.toLowerCase() === "s" &&
      summary &&
      !summary.draft &&
      !editorMode &&
      !artworkCard &&
      !selectedCard
    ) {
      event.preventDefault();
      void saveOrder(event.shiftKey);
    }
    if (
      (event.ctrlKey || event.metaKey) &&
      event.key.toLowerCase() === "f" &&
      summary &&
      !summary.draft &&
      !selectedCard &&
      !artworkCard &&
      !editorMode
    ) {
      event.preventDefault();
      searchInput?.focus();
      searchInput?.scrollIntoView({ block: "center" });
    }
  }}
/>

<div class="app-shell" style:--app-bar-height={`${appBarHeight}px`}>
  <header class="app-bar" class:has-order={!!summary} bind:this={appBar}>
    <div class="brand">
      <span class="brand-mark"><Icon size={20} /></span><span
        >MPC <strong>Autofill</strong></span
      >
    </div>
    {#if !summary}<span class="app-tagline"
        >A little less work. A lot more cards.</span
      >{/if}
    {#if summary}<div class="file-actions">
        <span class="save-status" class:unsaved={summary.dirty} role="status"
          >{summary.dirty ? "Unsaved changes" : savedMessage}</span
        ><button
          class="button secondary small"
          disabled={busy}
          on:click={browseSavedDecks}
          ><Icon name="grid" size={16} />Saved decks</button
        >{#if summary.draft}<button
          class="button primary small"
          disabled={busy}
          on:click={() => {
            resumeDraft = true;
            editorMode = "create";
          }}>Continue import</button
        >{:else}<button
          class="button secondary small"
          disabled={busy}
          on:click={() => (editorMode = "edit")}>Edit list</button
        ><button
          class="button secondary small"
          disabled={busy}
          on:click={() => replaceWithDecklist()}>Replace deck</button
        ><button
          class="button secondary small"
          disabled={busy || !connected}
          on:click={pasteDecklist}>New deck</button
        ><button
          class="button secondary small"
          on:click={() => saveOrder()}
          disabled={busy || !connected}>Save</button
        ><button
          class="button secondary small"
          on:click={() => saveOrder(true)}
          disabled={busy || !connected}>Save as…</button
        >{/if}<button
          class="button secondary small"
          on:click={() => openOrder()}
          disabled={busy || !connected}
          ><Icon name="folder" size={16} />Open deck…</button
        >
      </div>{/if}
    <ThemePicker />
  </header>

  <main class:welcome-main={!summary}>
    {#if orderError}
      <div class="notice error-notice" role="alert">
        <Icon name="warning" />
        <div>
          <strong>Order unavailable</strong>
          <p>{orderError}</p>
        </div>
        <button
          class="icon-button"
          aria-label="Dismiss order error"
          on:click={() => (orderError = "")}
          ><Icon name="close" size={16} /></button
        >
      </div>
    {/if}

    {#if libraryWarning}<p class="notice" role="status">
        {libraryWarning}
      </p>{/if}
    {#if !summary}
      {#if homePage === "saved"}
        <SavedDecks
          disabled={busy || !connected}
          on:select={(event) => loadOrder(event.detail)}
          on:open={(event) => openOrder(event.detail)}
          on:paste={pasteDecklist}
          on:home={showWelcome}
        />
      {:else}
        <section class="welcome" aria-labelledby="welcome-title">
          <div class="welcome-art" aria-hidden="true">
            <span class="decor-card card-left"></span><span
              class="decor-card card-right"
            ></span><span class="decor-card card-center"
              ><Icon size={46} /></span
            >
          </div>
          <p class="eyebrow">YOUR NEXT DECK STARTS HERE</p>
          <h1 id="welcome-title">
            Great cards.<br /><span>Less clicking.</span>
          </h1>
          <p class="welcome-description">
            Bring your decklist. We’ll prepare the images and<br
              class="wide-only"
            /> build your MakePlayingCards project for you.
          </p>
          <div class="drop-zone" class:dragging>
            <span class="drop-icon"><Icon name="import" size={24} /></span>
            <div>
              <h2>
                {dragging
                  ? "Drop your file here"
                  : "Drop a decklist or order here"}
              </h2>
              <p>Text decklist (.txt), saved draft (.mpcdeck), or MPC Autofill order (.xml)</p>
            </div>
            <button
              class="button secondary"
              on:click={() => openOrder()}
              disabled={busy || !connected}
              >{busy ? "Opening…" : "Open deck…"}<Icon
                name="arrow"
                size={16}
              /></button
            >
          </div>
          <div class="welcome-import-actions">
            <button
              class="button primary"
              disabled={busy || !connected}
              on:click={() => openOrder(true)}
              ><Icon name="import" size={17} />Import .txt decklist</button
            >
            <button
              class="button secondary"
              disabled={busy || !connected}
              on:click={pasteDecklist}>Paste a decklist</button
            >
            <button
              class="button secondary"
              disabled={busy || !connected}
              on:click={browseSavedDecks}
              ><Icon name="grid" size={17} />Browse saved decks</button
            >
          </div>
          <ol class="welcome-steps">
            <li><span>01</span>Preview your cards</li>
            <li><span>02</span>Start autofill</li>
            <li><span>03</span>Review in Chrome</li>
          </ol>
        </section>
      {/if}
    {:else}
      {#if summary.draft}
        <section class="draft-review" aria-labelledby="draft-title">
          <p class="eyebrow">SAVED DRAFT</p>
          <h1 id="draft-title">{summary.name}</h1>
          <p>
            Your imported decklist is saved. Continue importing to fix its issues and
            create an order for autofill.
          </p>
          {#if summary.draft_error}<div class="notice error-notice" role="alert">
              <Icon name="warning" />
              <p>{summary.draft_error}</p>
            </div>{:else}<p class="notice" role="status">
              This draft is ready to create. Continue import to review it.
            </p>{/if}
          <div class="draft-actions">
            <button
              class="button primary"
              disabled={busy || !connected}
              on:click={() => {
                resumeDraft = true;
                editorMode = "create";
              }}>Continue import</button
            >
            <button
              class="button secondary"
              disabled={busy}
              on:click={browseSavedDecks}>Saved decks</button
            >
          </div>
          <h2>Imported decklist</h2>
          <pre class="draft-text">{summary.draft.text}</pre>
        </section>
      {:else}
      <section class="order-overview" aria-labelledby="order-title">
        <div class="order-heading">
          <p class="eyebrow">YOUR ORDER</p>
          <h1 id="order-title">{summary.name}</h1>
          <div class="order-meta">
            <span><Icon size={14} />{summary.quantity} cards</span><span
              >{summary.stock}</span
            ><span>{summary.foil ? "Foil finish" : "Non-foil"}</span>
          </div>
        </div>
        <div class="decklist-actions">
          <button class="button primary small" disabled={busy || !connected}
            on:click={() => replaceWithDecklist()}>Paste replacement decklist</button>
          <button class="button secondary small" disabled={busy || !connected}
            on:click={() => openOrder(true)}>Import replacement .txt</button>
          <span>Matching cards can reuse artwork from this deck.</span>
        </div>
        <div class="order-actions">
          {#each cardbacks as card (cardKey(card))}
            <div
              class="common-back"
              class:needs-attention={needsAttention(card)}
            >
              <button
                class="back-preview"
                on:click={() => (selectedKey = cardKey(card))}
                aria-label={`Preview common cardback, ${card.name}`}
                title="Preview common back"
              >
                <span class="back-mini"
                  >{#if card.thumb && card.thumb !== failedBackThumb}<img
                      src={card.thumb}
                      alt=""
                      on:error={() => (failedBackThumb = card.thumb ?? null)}
                    />{:else}<Icon size={22} />{/if}</span
                >
                <span class="back-preview-hint">Preview</span>
              </button>
              <div class="common-back-info">
                <span class="eyebrow">COMMON BACK</span><strong
                  title={displayName(card)}>{displayName(card)}</strong
                ><span class="muted"
                  >Used on {card.slots.length} card{card.slots.length === 1
                    ? ""
                    : "s"}</span
                >
                <span
                  class="card-dpi"
                  title="Original artwork resolution reported by MPC Autofill"
                  >{dpiLabel(card.dpi)}</span
                >
                {#if needsAttention(card)}<span
                    class="common-back-warning"
                    title={card.error}
                  >
                    <Icon name="warning" size={13} />{statusLabels[card.status]}
                  </span>{/if}
                <button
                  class="button secondary small common-back-change"
                  data-card-key={cardKey(card)}
                  disabled={busy || !connected}
                  on:click={() => changeArtwork(card)}
                  ><Icon name="image" size={14} />Change back</button
                >
              </div>
            </div>
          {/each}
          <div class="start-action">
            <button
              class="button primary"
              on:click={fill}
              disabled={busy || !connected}
              ><Icon name={running ? "clock" : "arrow"} size={17} />{running
                ? "Autofill in progress"
                : phase === "done"
                  ? "Start new project"
                  : error
                    ? "Start over"
                    : "Start autofill"}</button
            ><span
              >{phase === "done" || error
                ? "Creates a new project in Chrome"
                : "Opens MakePlayingCards in Chrome"}</span
            >
          </div>
        </div>
      </section>

      <ProgressPanel
        {cards}
        {phase}
        busy={running}
        {error}
        {downloadCompleted}
        {downloadFailures}
        on:click={showAttention}
      />
      {#if error}<details class="notice error-details">
          <summary>Technical error details</summary>
          <p>{error}</p>
        </details>{/if}
      {#if previewError}<p class="notice">{previewError}</p>{/if}
      {#if dpiError}<div class="notice" role="status">
          <p>{dpiError}</p>
          <button class="text-button" on:click={() => loadDpi()}
            >Retry DPI</button
          >
        </div>{/if}

      <section class="library" aria-labelledby="library-title">
        <div class="library-heading">
          <div>
            <h2 id="library-title">
              Your cards <span class="count-badge">{deckImageCount}</span>
            </h2>
            <p>Preview your cards or change their artwork.</p>
          </div>
          <label class="search-field"
            ><Icon name="search" size={17} /><input
              bind:this={searchInput}
              bind:value={query}
              type="search"
              placeholder="Find a card…"
              aria-label="Search cards by name"
            /></label
          >
        </div>
        <div class="library-controls">
          <div class="filters" aria-label="Filter cards">
            {#each filters as item}<button
                class:active={filter === item.value}
                aria-pressed={filter === item.value}
                on:click={() => (filter = item.value)}
                >{item.label}{#if item.value === "attention" && attentionCount}<span
                    class="filter-count">{attentionCount}</span
                  >{/if}</button
              >{/each}
          </div>
          <span class="result-count" aria-live="polite"
            >{visibleCards.length} images</span
          >
        </div>
        {#if visibleCards.length}
          <div class="card-grid">
            {#each visibleCards as card (cardKey(card))}<CardTile
                {card}
                disabled={busy || !connected}
                on:click={() => (selectedKey = cardKey(card))}
                on:changeArtwork={(event) => changeArtwork(event.detail)}
              />{/each}
          </div>
        {:else}
          <div class="no-results">
            <Icon
              name={filter === "attention" && !query ? "check" : "search"}
              size={28}
            />
            <h3>
              {filter === "attention" && !query
                ? "Nothing needs attention"
                : "No matching cards"}
            </h3>
            <p>
              {filter === "attention" && !query
                ? "Images with download or placement issues will appear here."
                : "Try a different name or show all cards."}
            </p>
            <button
              class="button secondary small"
              on:click={() => {
                query = "";
                filter = "all";
              }}>Show all cards</button
            >
          </div>
        {/if}
      </section>
      <footer class="library-footer">
        <Icon size={14} /><span
          >{summary.quantity} cards · {cards.length} front and back images</span
        ><span>Made for your next deck.</span>
      </footer>
      {/if}
    {/if}
  </main>
</div>

{#if dragging && summary}<div class="drop-overlay" role="status">
    <Icon name="import" size={40} />
    <h2>Drop a .txt decklist, .mpcdeck draft, or .xml order</h2>
    <p>{summary.draft
        ? "Text lists start a new deck; saved drafts can be resumed."
        : "Text lists replace the open deck and can reuse its artwork; saved drafts can be resumed."}</p>
  </div>{/if}
<CardPreview
  card={selectedCard}
  editable
  disabled={busy || !connected}
  on:close={() => (selectedKey = null)}
  on:changeArtwork={(event) => changeArtwork(event.detail)}
/>
{#if artworkCard}<ChangeArtwork
    card={artworkCard}
    bind:applying={artworkApplying}
    on:close={() => (artworkCard = null)}
    on:applied={(event) =>
      applyArtworkChange(event.detail.order, event.detail.previewId)}
  />{/if}
{#if editorMode}<DeckEditor
    mode={editorMode}
    initialText={editorMode !== "edit" && !resumeDraft ? draftText : ""}
    initialName={resumeDraft && summary ? summary.name : draftName}
    initialDraft={editorMode === "create" && resumeDraft ? summary?.draft ?? null : null}
    draftPath={editorMode === "create" && resumeDraft && summary?.draft ? summary.path : ""}
    replacingUnsaved={summary?.dirty ?? false}
    on:close={() => (editorMode = null)}
    on:applied={(event) => applyEditedOrder(event.detail)}
  />{/if}
