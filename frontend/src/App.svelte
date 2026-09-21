<script lang="ts">
  import { onMount } from "svelte";
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";

  type Status =
    | "pending"
    | "downloaded"
    | "download-failed"
    | "uploading"
    | "inserted"
    | "skipped";

  type Card = {
    face: "front" | "back";
    drive_id: string;
    name: string;
    slots: number[];
    cardback: boolean;
    thumb?: string;
    status: Status;
    error?: string;
  };

  type OrderSummary = {
    name: string;
    quantity: number;
    stock: string;
    foil: boolean;
    cards: Omit<Card, "status">[];
  };

  let summary: OrderSummary | null = null;
  let cards: Card[] = [];
  let phase = "";
  let busy = false;
  let error = "";
  // Download events do not include a total, but the loaded order does. Keep
  // this separately from card status because later upload events replace it.
  let downloadCompleted = 0;
  let downloadFailures = 0;

  const PHASE_LABELS: Record<string, string> = {
    downloading: "Downloading card images…",
    "awaiting-sign-in": "Waiting for you to sign in to MakePlayingCards…",
    configuring: "Configuring the project…",
    "uploading-fronts": "Uploading card fronts…",
    "paging-to-backs": "Moving to card backs…",
    "uploading-backs": "Uploading card backs…",
    "paging-to-review": "Moving to project review…",
    done: "Done — review your project in the browser.",
  };

  $: cardbacks = cards.filter((c) => c.cardback);
  $: mainCards = cards.filter((c) => !c.cardback);
  $: downloaded = cards.filter((c) => c.status !== "pending" && c.status !== "download-failed").length;
  $: failed = cards.filter((c) => c.status === "download-failed").length;

  // Progress events can arrive in a burst (one per image); coalesce them and
  // rebuild `cards` at most once per animation frame so the grid never thrashes.
  let pending = new Map<string, Partial<Card>>();
  let flushScheduled = false;

  function flush() {
    flushScheduled = false;
    const changes = pending;
    pending = new Map();
    cards = cards.map((c) => {
      const change = changes.get(c.drive_id);
      return change ? { ...c, ...change } : c;
    });
  }

  // A Drive id can appear on both a front and a back, so a change applies to
  // every card that shares it.
  function patch(driveId: string, change: Partial<Card>) {
    const prev = pending.get(driveId);
    pending.set(driveId, prev ? { ...prev, ...change } : change);
    if (!flushScheduled) {
      flushScheduled = true;
      requestAnimationFrame(flush);
    }
  }

  onMount(() => {
    const unlisten = [
      listen<{ drive_id: string; ok: boolean; error?: string }>(
        "download-progress",
        (event) => {
          const { drive_id, ok, error } = event.payload;
          downloadCompleted = Math.min(cards.length, downloadCompleted + 1);
          if (!ok) downloadFailures += 1;
          patch(drive_id, {
            status: ok ? "downloaded" : "download-failed",
            error: ok ? undefined : error,
          });
        },
      ),
      listen<{ phase: string }>("phase", (event) => {
        phase = event.payload.phase;
        if (phase === "downloading") {
          downloadCompleted = 0;
          downloadFailures = 0;
        }
      }),
      listen<{ drive_id: string; status: Status }>("card-status", (event) => {
        patch(event.payload.drive_id, { status: event.payload.status });
      }),
      listen<{ drive_id: string; path: string }>("thumb-ready", (event) => {
        patch(event.payload.drive_id, { thumb: convertFileSrc(event.payload.path) });
      }),
    ];
    return () => unlisten.forEach((p) => p.then((off) => off()));
  });

  async function openOrder() {
    error = "";
    const path = await open({
      multiple: false,
      filters: [{ name: "Order XML", extensions: ["xml"] }],
    });
    if (typeof path !== "string") return;

    try {
      busy = true;
      summary = await invoke<OrderSummary>("load_order", { path });
      cards = summary.cards.map((c) => ({ ...c, status: "pending" }));
      phase = "";
      downloadCompleted = 0;
      downloadFailures = 0;
      // Fire-and-forget: backend fetches + caches thumbnails and streams them
      // back via "thumb-ready" events.
      invoke("thumbnails").catch(() => {});
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function fill() {
    try {
      busy = true;
      error = "";
      // Show an indeterminate state immediately; the backend's phase event
      // follows before it starts emitting per-image download progress.
      phase = "downloading";
      downloadCompleted = 0;
      downloadFailures = 0;
      await invoke("fill");
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<main class="app">
  <header>
    <div class="title">
      <h1>MPC Autofill</h1>
      {#if summary}
        <p class="meta">
          <strong>{summary.name}</strong> · {summary.quantity} cards · {summary.stock}{summary.foil
            ? " · foil"
            : ""}
        </p>
      {/if}
    </div>
    <div class="actions">
      <button on:click={openOrder} disabled={busy}>Open order…</button>
      <button class="primary" on:click={fill} disabled={busy || !summary}>
        Download images & fill order
      </button>
    </div>
  </header>

  {#if error}
    <p class="banner error">{error}</p>
  {:else if phase === "downloading"}
    <section class="banner download-progress" aria-live="polite">
      <div class="download-progress-copy">
        <span>{PHASE_LABELS.downloading}</span>
        {#if cards.length}
          <strong>{downloadCompleted}/{cards.length} images processed</strong>
        {:else}
          <strong>No images need downloading</strong>
        {/if}
      </div>
      {#if cards.length}
        {#if downloadCompleted === 0}
          <progress
            class="download-progress-bar"
            aria-label="Preparing image downloads"
          />
        {:else}
          <progress
            class="download-progress-bar"
            value={downloadCompleted}
            max={cards.length}
            aria-label={`${downloadCompleted} of ${cards.length} images processed`}
          />
        {/if}
      {/if}
      {#if downloadFailures}
        <p class="download-failures">
          {downloadFailures} download attempt{downloadFailures === 1 ? "" : "s"} failed.
        </p>
      {/if}
    </section>
  {:else if phase}
    <p class="banner">
      {PHASE_LABELS[phase] ?? phase}{downloadFailures
        ? ` · ${downloadFailures} download attempt${downloadFailures === 1 ? "" : "s"} failed`
        : ""}
    </p>
  {:else if summary}
    <p class="banner subtle">
      {downloaded}/{cards.length} images ready{failed ? ` · ${failed} failed` : ""}
    </p>
  {/if}

  {#if !summary}
    <div class="empty">
      <p>Open an order XML exported from MPC Autofill to begin.</p>
    </div>
  {:else}
    {#if cardbacks.length}
      <section class="section">
        <h2 class="section-title">Common cardback</h2>
        <div class="grid">
          {#each cardbacks as card (card.face + ":" + card.drive_id)}
            <figure class="card cardback {card.status}" title={card.error ?? card.name}>
              <span class="status-dot" />
              <div class="thumb">
                {#if card.thumb}
                  <img src={card.thumb} alt={card.name} loading="lazy" />
                {:else}
                  <span class="placeholder">↺</span>
                {/if}
                <span class="badge">×{card.slots.length}</span>
              </div>
              <figcaption>{card.name}</figcaption>
            </figure>
          {/each}
        </div>
      </section>
    {/if}

    <section class="section">
      <h2 class="section-title">Cards</h2>
      <div class="grid">
        {#each mainCards as card (card.face + ":" + card.drive_id)}
          <figure class="card {card.status}" title={card.error ?? card.name}>
            <span class="status-dot" />
            <div class="thumb">
              {#if card.thumb}
                <img src={card.thumb} alt={card.name} loading="lazy" />
              {:else}
                <span class="placeholder">{card.face === "front" ? "F" : "B"}</span>
              {/if}
              <span class="badge">{card.slots.length}</span>
            </div>
            <figcaption>{card.name}</figcaption>
          </figure>
        {/each}
      </div>
    </section>
  {/if}
</main>
