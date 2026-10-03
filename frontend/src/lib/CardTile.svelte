<script lang="ts">
  import Icon from "./Icon.svelte";
  import {
    displayName,
    needsAttention,
    statusLabels,
    type Card,
  } from "./order";
  export let card: Card;
  let broken = false;
  $: if (card.thumb) broken = false;
</script>

<button
  class="card-tile"
  class:needs-attention={needsAttention(card)}
  on:click
  aria-label={`Preview ${card.name}, ${card.face}, ${statusLabels[card.status]}`}
>
  <span class="card-art">
    {#if card.thumb && !broken}
      <img
        src={card.thumb}
        alt=""
        loading="lazy"
        decoding="async"
        on:error={() => (broken = true)}
      />
    {:else}
      <span class="card-placeholder"
        ><Icon name="image" size={28} /><span>Preview unavailable</span></span
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
    <span class="preview-hint">View card <Icon name="arrow" size={14} /></span>
  </span>
  <span class="card-name" title={displayName(card)}>{displayName(card)}</span>
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
