import { test } from "node:test";
import assert from "node:assert/strict";
import { cardBackChanged, favoriteBacks, initialCardBack, toggleFavoriteBack } from "../src/lib/favoriteBacks.ts";

test("favorite backs persist in most recently added order", () => {
  const values = new Map();
  globalThis.localStorage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
  };
  globalThis.window = { dispatchEvent: () => {} };
  const choice = (id) => ({
    art: { drive_id: id, name: `${id}.png`, label: id },
    query: "",
    index: 0,
    count: 1,
  });
  toggleFavoriteBack(choice("first"));
  toggleFavoriteBack(choice("second"));
  assert.deepEqual(favoriteBacks().map((item) => item.art.drive_id), ["second", "first"]);
  toggleFavoriteBack(choice("second"));
  assert.deepEqual(favoriteBacks().map((item) => item.art.drive_id), ["first"]);
});

test("new decks use the newest favorite while resumed drafts keep their saved back", () => {
  const values = new Map();
  globalThis.localStorage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
  };
  globalThis.window = { dispatchEvent: () => {} };
  const choice = (id) => ({
    art: { drive_id: id, name: `${id}.png`, label: id },
    query: "",
    index: 0,
    count: 1,
  });
  toggleFavoriteBack(choice("favorite"));
  const selected = initialCardBack(null);
  assert.equal(selected?.art.drive_id, "favorite");
  assert.equal(cardBackChanged(selected, selected?.art.drive_id ?? null), false);
  assert.equal(cardBackChanged(choice("other"), selected?.art.drive_id ?? null), true);
  assert.equal(initialCardBack({ common_back: null }), null);
  assert.equal(
    initialCardBack({ common_back: choice("draft") })?.art.drive_id,
    "draft",
  );
});
