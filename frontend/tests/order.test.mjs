import { test } from "node:test";
import assert from "node:assert/strict";
import { patchCards, filterCards, placementCounts } from "../src/lib/order.ts";

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

test("search and face filters work together, including common cardbacks", () => {
  const cards = [
    card("front"),
    card("back"),
    card("front", "pending", { name: "Lightning Bolt", drive_id: "bolt" }),
  ];
  assert.equal(filterCards(cards, "back", "  ISLAND ").length, 1);
  assert.equal(filterCards(cards, "front", "bolt").length, 1);
  assert.equal(filterCards(cards, "back", "bolt").length, 0);
});
