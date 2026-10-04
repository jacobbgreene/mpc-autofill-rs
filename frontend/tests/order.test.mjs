import { test } from "node:test";
import assert from "node:assert/strict";
import {
  patchCards,
  filterCards,
  placementCounts,
  artworkQuery,
  cardKey,
  needsAttention,
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

test("local consent remains actionable independently of download and placement status", () => {
  const pending = card("front", "pending", { source: "LocalFile", local_approved: false });
  assert.equal(needsAttention(pending), true);
  assert.equal(needsAttention({ ...pending, local_approved: undefined }), true);
  assert.deepEqual(filterCards([pending, card("back")], "attention", ""), [pending]);
  const approved = { ...pending, local_approved: true };
  assert.equal(needsAttention(approved), false);
  assert.equal(needsAttention({ ...approved, status: "download-failed" }), true);
});

test("local and remote images with identical IDs keep separate cache and progress identities", () => {
  const remote = card("front", "pending", { source: "GoogleDrive" });
  const local = card("front", "pending", { source: "LocalFile", local_approved: true });
  assert.notEqual(cardKey(remote), cardKey(local));
  const updated = patchCards([remote, local], "shared", { status: "downloaded" }, undefined, "GoogleDrive");
  assert.equal(updated[0].status, "downloaded");
  assert.equal(updated[1].status, "pending");
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

test("all artwork and backs include the common card back", () => {
  const cards = [
    card("front"),
    card("back"),
    card("back", "pending", { cardback: false, name: "Insectile Aberration" }),
    card("front", "pending", { name: "Lightning Bolt", drive_id: "bolt" }),
  ];
  assert.equal(filterCards(cards, "back", "  ISLAND ").length, 1);
  assert.equal(filterCards(cards, "back", "insectile").length, 1);
  assert.equal(filterCards(cards, "all", "bolt").length, 1);
  assert.equal(filterCards(cards, "all", "island").length, 2);
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
