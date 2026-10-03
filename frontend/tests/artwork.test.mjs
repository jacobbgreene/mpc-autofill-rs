import { test } from "node:test";
import assert from "node:assert/strict";
import { createArtworkSearch } from "../src/lib/artwork.ts";

const result = { cards: [], count: 0, next_page: null };
const flush = () => new Promise((resolve) => setImmediate(resolve));

test("a large deck limits concurrent searches and shares pages with its gallery", async () => {
  const started = [];
  let active = 0;
  let peak = 0;
  const pending = [];
  const client = createArtworkSearch((query, page, kind) => {
    started.push([query, page, kind]);
    peak = Math.max(peak, ++active);
    return new Promise((resolve) =>
      pending.push(() => {
        active--;
        resolve(result);
      }),
    );
  });
  const requests = Array.from({ length: 12 }, (_, i) =>
    client.search(`Card ${i}`, 0),
  );
  const gallery = client.search(" Card 0 ", 0);
  assert.equal(gallery, requests[0], "same page uses the in-flight request");
  await flush();
  assert.equal(started.length, 4);
  while (pending.length) {
    pending.splice(0).forEach((finish) => finish());
    await flush();
  }
  await Promise.all(requests);
  assert.equal(peak, 4);
  assert.equal(started.length, 12);
  assert.equal(await client.search("Card 0", 0), result);
  assert.equal(started.length, 12, "returning from the gallery uses the cache");
  client.dispose();
});

test("failed searches can be retried and pages and card backs use separate cache entries", async () => {
  const calls = [];
  const client = createArtworkSearch(async (query, page, kind) => {
    calls.push([query, page, kind]);
    if (calls.length === 1) throw new Error("Network unavailable");
    return result;
  });
  await assert.rejects(client.search("Island", 0), /Network unavailable/);
  assert.equal(await client.search("Island", 0), result);
  await client.search("Island", 1);
  await client.search("Island", 0, "cardback");
  assert.deepEqual(calls, [
    ["Island", 0, "card"],
    ["Island", 0, "card"],
    ["Island", 1, "card"],
    ["Island", 0, "cardback"],
  ]);
  client.dispose();
});

test("closing or replacing a draft cancels queued searches without starting them", async () => {
  let finish;
  let started = 0;
  const client = createArtworkSearch(() => {
    started++;
    return new Promise((resolve) => {
      finish = resolve;
    });
  }, 1);
  const first = client.search("First", 0);
  const queued = client.search("Queued", 0);
  const rejected = assert.rejects(queued, /closed/);
  await flush();
  client.dispose();
  await rejected;
  finish(result);
  assert.equal(await first, result);
  await flush();
  assert.equal(started, 1);
  await assert.rejects(client.search("Later", 0), /closed/);
});

test("a gallery page takes priority over queued automatic previews", async () => {
  const started = [];
  const pending = [];
  const client = createArtworkSearch((query) => {
    started.push(query);
    return new Promise((resolve) => pending.push(() => resolve(result)));
  }, 1);
  const first = client.search("First preview", 0, "card", "background");
  const queued = client.search("Later preview", 0, "card", "background");
  const gallery = client.search("Gallery page", 1);
  await flush();
  pending.shift()();
  await first;
  await flush();
  assert.deepEqual(started, ["First preview", "Gallery page"]);
  pending.shift()();
  await gallery;
  await flush();
  assert.deepEqual(started, ["First preview", "Gallery page", "Later preview"]);
  pending.shift()();
  await queued;
  client.dispose();
});
