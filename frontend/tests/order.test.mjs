import { test } from "node:test";
import assert from "node:assert/strict";
import {
  patchCards,
  filterCards,
  placementCounts,
  artworkQuery,
} from "../src/lib/order.ts";

const card = (face, status = "pending", extra = {}) => ({
  face,
  status,
  drive_id: "shared",
  name: "Island",
  slots: [0],
  cardback: face === "back",
  ...extra,
});

test("front placement never marks a shared back image as inserted", () => {
  const cards = patchCards([card("front"), card("back")], "shared", {
    status: "downloaded",
  });
  const updated = patchCards(cards, "shared", { status: "inserted" }, "front");
  assert.equal(updated[0].status, "inserted");
  assert.equal(updated[1].status, "downloaded");
  assert.deepEqual(placementCounts(updated, "back"), {
    total: 1,
    inserted: 0,
    skipped: 0,
    attention: 0,
  });
});

test("skipped cards remain discoverable in needs-attention after completion", () => {
  const cards = [
    card("front", "inserted"),
    card("back", "skipped", { error: "Image unavailable" }),
  ];
  assert.equal(filterCards(cards, "attention", "").length, 1);
  assert.equal(placementCounts(cards).skipped, 1);
  assert.equal(placementCounts(cards).attention, 1);
});

test("the grid searches fronts and individual backs; common backs stay in the overview", () => {
  const cards = [
    card("front"),
    card("back"),
    card("back", "pending", { cardback: false, name: "Insectile Aberration" }),
    card("front", "pending", { name: "Lightning Bolt", drive_id: "bolt" }),
  ];
  assert.equal(filterCards(cards, "back", "  ISLAND ").length, 0);
  assert.equal(filterCards(cards, "back", "insectile").length, 1);
  assert.equal(filterCards(cards, "all", "bolt").length, 1);
  assert.equal(filterCards(cards, "all", "island").length, 1);
  assert.equal(filterCards(cards, "back", "bolt").length, 0);
});

test("artwork search uses the original card query and browses common backs", () => {
  assert.equal(
    artworkQuery(
      card("front", "pending", {
        name: "Island (Variant).png",
        query: "Island",
      }),
    ),
    "Island",
  );
  assert.equal(
    artworkQuery(
      card("front", "pending", { name: "Island (shared).jpg", query: " " }),
    ),
    "Island",
  );
  assert.equal(artworkQuery(card("back")), "");
  assert.equal(
    artworkQuery(
      card("back", "pending", {
        cardback: false,
        query: "Insectile Aberration",
      }),
    ),
    "Insectile Aberration",
  );
});
