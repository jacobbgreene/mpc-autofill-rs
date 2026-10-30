import { test } from "node:test";
import assert from "node:assert/strict";
import {
  createDraftController,
  imageKey,
  lineRange,
} from "../src/lib/draft.ts";

const flush = () => new Promise((resolve) => setImmediate(resolve));

test("resumed incomplete decks preserve saved choices without searching again", async () => {
  const choice = { art: results("saved-art").cards[0], query: "Island", index: 1, count: 2 };
  const app = setup({ initialChoices: { [imageKey(item("Island"))]: choice } });
  app.controller.update("Island", true);
  await flush();
  assert.equal(app.state.images[0].choice.art.drive_id, "saved-art");
  assert.equal(app.state.images[0].modified, true);
  assert.deepEqual(app.calls, []);
  app.controller.update("Island\nMountain", true);
  await flush();
  assert.equal(app.state.images[0].choice.art.drive_id, "saved-art");
  assert.deepEqual(app.calls, ["Mountain"]);
  app.controller.dispose();
});

test("saved artwork is displayed without searches or dirtying the draft, and survives unrelated edits", async () => {
  const retained = {
    ...item("Island"),
    card: {
      drive_id: "saved-id",
      name: "Selected Island.png",
      source: "GoogleDrive",
    },
  };
  const app = setup({
    preview: async (text) => ({
      ...preview(...(text === "rename" ? [item("Mountain")] : [])),
      retained: text === "rename" ? [] : [retained],
    }),
  });
  app.controller.update("original", true);
  await flush();
  assert.equal(app.state.images[0].choice.art.drive_id, "saved-id");
  assert.equal(app.state.images[0].modified, false);
  assert.deepEqual(app.calls, []);
  const choice = {
    art: results("replacement").cards[0],
    query: "Island",
    index: 0,
    count: 1,
  };
  app.controller.choose(item("Island"), choice);
  app.controller.update("quantity changed", true);
  await flush();
  assert.equal(app.state.images[0].choice.art.drive_id, "replacement");
  assert.equal(app.state.images[0].modified, true);
  app.controller.update("rename", true);
  await flush();
  assert.equal(app.state.images[0].choice.art.drive_id, "Mountain");
  assert.deepEqual(app.calls, ["Mountain"]);
  app.controller.dispose();
});
const item = (query, source_lines = [1]) => ({
  query,
  face: "front",
  source_lines,
});
const preview = (...items) => ({
  quantity: items.length,
  unresolved: items,
  needs_common_back: false,
  issues: [],
});
const results = (name) => ({
  cards: [
    { drive_id: name, name, label: name, dpi: 1200, source: "Test", thumb: "" },
  ],
  count: 1,
  next_page: null,
});
function setup(overrides = {}) {
  let state;
  const calls = [];
  const controller = createDraftController({
    preview: async (text) =>
      preview(...text.split("\n").map((name, i) => item(name, [i + 1]))),
    search: async (query) => {
      calls.push(query);
      return results(query);
    },
    changed: (value) => {
      state = value;
    },
    ...overrides,
  });
  return {
    controller,
    calls,
    get state() {
      return state;
    },
  };
}
test("typing marks the preview stale immediately and debounces 350 ms", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const parsed = [];
  const app = setup({
    preview: async (text) => {
      parsed.push(text);
      return preview(item(text));
    },
  });
  app.controller.update("Is");
  assert.equal(app.state.updating, true);
  t.mock.timers.tick(200);
  app.controller.update("Island");
  t.mock.timers.tick(349);
  assert.deepEqual(parsed, []);
  t.mock.timers.tick(1);
  await flush();
  assert.deepEqual(parsed, ["Island"]);
  assert.equal(app.state.updating, false);
  app.controller.dispose();
});
test("older previews cannot replace the latest text", async () => {
  const pending = [];
  const app = setup({
    preview: () => new Promise((resolve) => pending.push(resolve)),
  });
  app.controller.update("Old", true);
  app.controller.update("New", true);
  pending[1](preview(item("New")));
  await flush();
  pending[0](preview(item("Old")));
  await flush();
  assert.equal(app.state.images[0].item.query, "New");
  assert.deepEqual(app.calls, ["New"]);
});
test("reordering and quantity/source-line changes retain artwork; renaming does not", async () => {
  const app = setup();
  app.controller.update("Island\nBolt", true);
  await flush();
  const choice = {
    art: results("Chosen").cards[0],
    query: "Alternate search",
    index: 0,
    count: 1,
  };
  app.controller.choose(item("Island"), choice);
  app.controller.update("Bolt\nIsland", true);
  await flush();
  assert.equal(app.state.images[1].choice, choice);
  assert.deepEqual(app.state.images[1].item.source_lines, [2]);
  assert.deepEqual(app.calls, ["Island", "Bolt"]);
  app.controller.update("Mountain\nBolt", true);
  await flush();
  assert.equal(app.state.images[0].choice.art.drive_id, "Mountain");
  assert.deepEqual(app.calls, ["Island", "Bolt", "Mountain"]);
});
test("late artwork cannot overwrite an explicit gallery selection or a renamed entry", async () => {
  let finish;
  const app = setup({
    search: () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  });
  app.controller.update("Old", true);
  await flush();
  const choice = {
    art: results("Manual").cards[0],
    query: "Manual",
    count: 1,
    index: 0,
  };
  app.controller.choose(item("Old"), choice);
  finish(results("Old"));
  await flush();
  assert.equal(app.state.images[0].choice, choice);
  app.controller.update("New", true);
  await flush();
  assert.equal(app.state.images[0].choice, null);
  app.controller.dispose();
  finish(results("New"));
  await flush();
});
test("no-match and transport failures have distinct states and can be retried", async () => {
  let attempt = 0;
  const app = setup({
    search: async () => {
      attempt++;
      if (attempt === 1) return { cards: [], count: 0, next_page: null };
      if (attempt === 2) throw new Error("Offline");
      return results("Fixed");
    },
  });
  app.controller.update("Typo", true);
  await flush();
  assert.equal(app.state.images[0].status, "no-match");
  await app.controller.load(item("Typo"));
  assert.equal(app.state.images[0].status, "failed");
  await app.controller.load(item("Typo"));
  assert.equal(app.state.images[0].status, "selected");
});
test("line navigation and name grouping match Rust's semantics", () => {
  assert.deepEqual(lineRange("# heading\n1 Island\n1 Bolt", 3), {
    start: 19,
    end: 25,
  });
  assert.equal(imageKey(item("Island")), imageKey(item("island")));
  assert.notEqual(imageKey(item("Æther")), imageKey(item("æther")));
  assert.notEqual(
    imageKey(item("Island")),
    imageKey({ ...item("Island"), face: "back" }),
  );
});

test("retry requests the failed artwork index while preserving the existing selection", async () => {
  const calls = [];
  let fail = true;
  const app = setup({
    search: async (...args) => {
      calls.push(args);
      if (args[1] === 1 && fail) throw new Error("Offline");
      return { ...results("Island"), count: 25 };
    },
  });
  app.controller.update("Island", true);
  await flush();
  const previous = app.state.images[0].choice;
  await app.controller.load(item("Island"), 24);
  assert.equal(app.state.images[0].choice, previous);
  fail = false;
  await app.controller.retry(item("Island"));
  assert.equal(calls.at(-1)[1], 1);
  assert.equal(calls.at(-1)[4], true);
  assert.equal(app.state.images[0].choice.index, 24);
});
