<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import { displayName, statusLabels, type Card } from "./order";
  export let card: Card | null;
  const dispatch = createEventDispatcher<{ close: void }>();
  let dialog: HTMLDialogElement;
  let broken = false;
  $: if (card?.thumb) broken = false;
  $: if (dialog && card && !dialog.open) dialog.showModal();
  $: if (dialog && !card && dialog.open) dialog.close();
</script>

<dialog
  bind:this={dialog}
  class="card-dialog"
  aria-labelledby="preview-title"
  on:close={() => dispatch("close")}
>
  {#if card}
    <form method="dialog" class="dialog-close">
      <button class="icon-button" aria-label="Close preview"
        ><Icon name="close" /></button
      >
    </form>
    <div class="preview-art">
      {#if card.thumb && !broken}
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
      <details>
        <summary>Card slots</summary>
        <p class="slot-list">
          {card.slots.map((slot) => slot + 1).join(", ") || "No slots assigned"}
        </p>
      </details>
      {#if card.error}<details class="error-details" open>
          <summary>Error details</summary>
          <p>{card.error}</p>
        </details>{/if}
    </div>
  {/if}
</dialog>
