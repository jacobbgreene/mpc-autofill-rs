<script lang="ts">
  import { createEventDispatcher, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import ArtworkGallery from "./ArtworkGallery.svelte";
  import {
    createArtworkSearch,
    type ArtworkChoice,
    type ArtworkResults,
  } from "./artwork";
  import {
    artworkQuery,
    displayName,
    type Card,
    type OrderSummary,
  } from "./order";

  export let card: Card;
  export let applying = false;
  const dispatch = createEventDispatcher<{
    close: void;
    applied: { order: OrderSummary; previewId: string };
  }>();
  const artwork = createArtworkSearch((query, page, kind) =>
    invoke<ArtworkResults>("search_artwork", { query, page, kind }),
  );
  let error = "";
  onDestroy(() => artwork.dispose());

  async function choose(choice: ArtworkChoice) {
    if (applying) return;
    if (choice.art.drive_id === card.drive_id) {
      dispatch("close");
      return;
    }
    applying = true;
    error = "";
    try {
      const edited = await invoke<OrderSummary>("change_artwork", {
        driveId: card.drive_id,
        selection: {
          face: card.face,
          query: choice.query,
          drive_id: choice.art.drive_id,
          name: choice.art.name,
        },
      });
      dispatch("applied", { order: edited, previewId: choice.art.drive_id });
    } catch (e) {
      error = `Could not change this artwork: ${String(e)}`;
    } finally {
      applying = false;
    }
  }
</script>

<ArtworkGallery
  search={artwork.search}
  title={card.cardback
    ? "Change common cardback"
    : `Change artwork · ${displayName(card)}`}
  query={artworkQuery(card)}
  kind={card.cardback ? "cardback" : "card"}
  selectedId={card.drive_id}
  {applying}
  applyError={error}
  description={`Applies to ${card.slots.length} ${card.face === "front" ? "front" : "back"} image${card.slots.length === 1 ? "" : "s"} using this artwork. Save your order to keep the change.`}
  on:choose={(event) => choose(event.detail)}
  on:close={() => dispatch("close")}
/>
