<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import ArtworkThumbnail from "./ArtworkThumbnail.svelte";
  import type { Artwork } from "./artwork";
  import { displayName, statusLabels, type Card } from "./order";
  export let card: Card | null = null;
  export let artwork: Artwork | null = null;
  export let editable = false;
  export let disabled = false;
  const dispatch = createEventDispatcher<{
    close: void;
    changeArtwork: Card;
  }>();
  let dialog: HTMLDialogElement;
  let broken = false;
  let retry = 0;
  $: if (card?.thumb) broken = false;
  $: if (dialog && (card || artwork) && !dialog.open) dialog.showModal();
  $: if (dialog && !card && !artwork && dialog.open) dialog.close();
</script>

<svelte:window
  on:keydown={(event) => {
    if (event.key === "Escape" && dialog?.open) {
      event.preventDefault();
      event.stopPropagation();
      dialog.close();
    }
  }}
/>

<dialog
  bind:this={dialog}
  class="card-dialog"
  class:artwork-detail={!!artwork}
  aria-labelledby="preview-title"
  on:cancel={(event) => {
    event.preventDefault();
    event.stopPropagation();
    dialog.close();
  }}
  on:close={() => dispatch("close")}
>
  {#if card || artwork}
    <form method="dialog" class="dialog-close">
      <button class="icon-button" aria-label="Close preview"
        ><Icon name="close" /></button
      >
    </form>
    <div class="preview-art">
      {#if artwork}<ArtworkThumbnail
          driveId={artwork.drive_id}
          label={artwork.label}
          {retry}
          on:error={() => (broken = true)}
          on:loaded={() => (broken = false)}
        />
      {:else if card?.thumb && !broken}
        <img
          src={card.thumb}
          alt={card.name}
          on:error={() => (broken = true)}
        />
      {:else}
        <div class="card-placeholder">
          <Icon name="image" size={44} /><span
            >No preview available for this image.</span
          >
        </div>
      {/if}
    </div>
    <div class="preview-info">
      {#if artwork}
        <span class="eyebrow">ARTWORK PREVIEW</span>
        <h2 id="preview-title">{artwork.label}</h2>
        <p>{artwork.source} · {artwork.dpi} DPI</p>
        {#if broken}<button
            class="button secondary small"
            on:click={() => {
              broken = false;
              retry++;
            }}>Retry preview</button
          >{/if}
      {:else if card}
        <span class="eyebrow"
          >{card.cardback ? "Common cardback" : `${card.face} image`}</span
        >
        <h2 id="preview-title">{displayName(card)}</h2>
        <p class="card-status {card.status}">{statusLabels[card.status]}</p>
        <p>
          Used in {card.slots.length} card slot{card.slots.length === 1
            ? ""
            : "s"}.
        </p>
        {#if editable}<button
            class="button secondary preview-change-art"
            {disabled}
            on:click={() => card && dispatch("changeArtwork", card)}
            ><Icon name="image" size={16} />{card.cardback
              ? "Change back"
              : "Change artwork"}</button
          >{/if}
        <details>
          <summary>Card slots</summary>
          <p class="slot-list">
            {card.slots.map((slot) => slot + 1).join(", ") ||
              "No slots assigned"}
          </p>
        </details>
        {#if card.error}<details class="error-details" open>
            <summary>Error details</summary>
            <p>{card.error}</p>
          </details>{/if}
      {/if}
    </div>
  {/if}
</dialog>
