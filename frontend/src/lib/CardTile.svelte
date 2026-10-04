<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import { dpiLabel } from "./artwork";
  import {
    displayName,
    cardKey,
    needsAttention,
    statusLabels,
    type Card,
  } from "./order";
  export let card: Card;
  export let disabled = false;
  const dispatch = createEventDispatcher<{
    changeArtwork: Card;
    retry: Card;
    previewFailed: Card;
  }>();
  let broken = false;
  $: if (card.thumb) broken = false;
</script>

<div class="card-slot">
  <button
    class="card-tile"
    class:needs-attention={needsAttention(card)}
    on:click
    aria-label={`Preview ${card.name}, ${card.face}, ${statusLabels[card.status]}`}
  >
    <span class="card-art">
      {#if card.thumb && card.thumbStatus !== "failed" && !broken}
        <img
          src={card.thumb}
          alt=""
          loading="lazy"
          decoding="async"
          on:error={() => {
            broken = true;
            dispatch("previewFailed", card);
          }}
        />
      {:else}
        <span class="card-placeholder"
          ><Icon name="image" size={28} /><span
            >{card.source === "LocalFile"
              ? card.local_approved ? "Couldn’t open this local image" : "Approve this local image to see its artwork"
              : card.thumbStatus === "failed" || broken
                ? "Couldn’t load card artwork. Retry below."
                : "Loading card artwork…"}</span
          ></span
        >
      {/if}
      <span class="face-label"
        >{card.cardback
          ? "Common back"
          : card.face === "front"
            ? "Front"
            : "Back"}</span
      >
      {#if card.slots.length > 1}<span class="quantity-badge"
          >×{card.slots.length}</span
        >{/if}
      <span class="preview-hint">View card <Icon name="arrow" size={14} /></span
      >
    </span>
    <span class="card-name" title={displayName(card)}>{displayName(card)}</span>
    <span
      class="card-dpi"
      title="Original artwork resolution reported by MPC Autofill"
      >{dpiLabel(card.dpi)}</span
    >
    <span class="card-status {card.status}">
      <Icon
        name={needsAttention(card)
          ? "warning"
          : card.status === "inserted"
            ? "check"
            : card.status === "uploading"
              ? "upload"
              : card.status === "downloaded"
                ? "import"
                : "clock"}
        size={13}
      />
      {statusLabels[card.status]}
    </span>
  </button>
  {#if (broken || card.thumbStatus === "failed") && card.source !== "LocalFile"}<button
      class="text-button retry-card-preview"
      aria-label={`Retry preview for ${displayName(card)}`}
      title={card.thumbError}
      on:click={() => {
        broken = false;
        dispatch("retry", card);
      }}>Retry preview</button
    >{/if}
  <button
    class="card-change-art"
    data-card-key={cardKey(card)}
    {disabled}
    aria-label={`Change artwork for ${displayName(card)}, ${card.face}`}
    on:click={() => dispatch("changeArtwork", card)}
    ><Icon name="image" size={14} />{card.cardback
      ? "Change back"
      : "Change artwork"}</button
  >
</div>
