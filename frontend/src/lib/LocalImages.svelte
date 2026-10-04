<script lang="ts">
  import { createEventDispatcher, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { displayName, type Card, type OrderSummary } from "./order";
  export let cards: Card[];
  const dispatch = createEventDispatcher<{ close: void; applied: OrderSummary; busy: boolean }>();
  let dialog: HTMLDialogElement;
  let busy = false;
  let error = "";
  $: local = [...new Map(cards.filter(card => card.source === "LocalFile").map(card => [card.drive_id, card])).values()];
  onMount(() => dialog.showModal());
  async function choose(card: Card, replacement = false) {
    if (busy) return;
    busy = true;
    dispatch("busy", true);
    error = "";
    try {
      const order = await invoke<OrderSummary>(replacement ? "replace_local_image" : "approve_local_image", { driveId: card.drive_id });
      dispatch("applied", order);
    } catch (e) { error = String(e); }
    finally { busy = false; dispatch("busy", false); }
  }
</script>

<dialog bind:this={dialog} class="editor-dialog local-images" aria-labelledby="local-images-title"
  on:cancel={(event) => { if (busy) event.preventDefault(); }} on:close={() => dispatch("close")}>
  <header>
    <h2 id="local-images-title">Review local images</h2>
    <p>This deck refers to files on your computer. Allow each image before previewing or uploading it to MakePlayingCards. A private copy is used for this session.</p>
  </header>
  <div class="local-image-list">
    {#each local as card (card.drive_id)}
      <section aria-label={displayName(card)}>
        <strong>{displayName(card)}</strong>
        <code>{card.drive_id}</code>
        <p>{card.local_approved ? "Image approved for this session" : "Not read or approved"}</p>
        <div class="local-image-actions">
          {#if !card.local_approved}<button class="button primary small" disabled={busy} on:click={() => choose(card)}>Allow image…</button>{/if}
          <button class="button secondary small" disabled={busy} on:click={() => choose(card, true)}>Choose replacement…</button>
        </div>
      </section>
    {/each}
  </div>
  <footer>
    {#if error}<p class="notice" role="alert">{error}</p>{/if}
    <p>Replacing a file updates the draft. Save to keep the new reference in your XML.</p>
    <button class="button secondary" disabled={busy} on:click={() => dialog.close()}>Done</button>
  </footer>
</dialog>

<style>
  .local-images { width: min(640px, calc(100vw - 32px)); max-height: calc(100dvh - 32px); padding: 0; }
  .local-images[open] { display: flex; flex-direction: column; }
  header, footer { padding: 20px; flex-shrink: 0; }
  h2 { margin-top: 0; }
  p { line-height: 1.5; }
  .local-image-list { min-height: 0; flex: 1; overflow-y: auto; max-height: 45dvh; padding: 0 20px; }
  section { border: 1px solid var(--border); border-radius: 10px; padding: 16px; margin-bottom: 12px; }
  code { display: block; overflow-wrap: anywhere; margin-top: 8px; }
  .local-image-actions { display: flex; flex-wrap: wrap; gap: 8px; }
</style>
