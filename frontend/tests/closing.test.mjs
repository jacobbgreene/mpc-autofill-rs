import { test } from "node:test";
import assert from "node:assert/strict";
import { createCloseController } from "../src/lib/closing.ts";

function setup(context = { dirty: false, running: false }, overrides = {}) {
  const events = [];
  const controller = createCloseController({
    context: () => context,
    confirm: async (message, labels) => {
      events.push({ message, labels });
      return true;
    },
    destroy: async () => {
      events.push("destroy");
    },
    changed: (state, error) => events.push({ state, error }),
    ...overrides,
  });
  return { events, controller };
}
test("clean windows close without a confirmation", async () => {
  const { events, controller } = setup();
  await controller.request();
  assert.equal(events.filter((e) => e.message).length, 0);
  assert.equal(events.at(-1), "destroy");
});
test("cancel keeps unsaved edits and allows a later close", async () => {
  let accepted = false;
  const { events, controller } = setup(
    { dirty: true, running: false },
    { confirm: async () => accepted },
  );
  await controller.request();
  assert.deepEqual(events.at(-1), { state: "idle", error: "" });
  assert.ok(!events.includes("destroy"));
  accepted = true;
  await controller.request();
  assert.equal(events.at(-1), "destroy");
});
test("repeated close requests share one confirmation and one destruction", async () => {
  let finish;
  let prompts = 0;
  const { controller, events } = setup(
    { dirty: true, running: false },
    {
      confirm: () => {
        prompts++;
        return new Promise((resolve) => {
          finish = resolve;
        });
      },
    },
  );
  const first = controller.request();
  await controller.request();
  finish(true);
  await first;
  await controller.request();
  assert.equal(prompts, 1);
  assert.equal(events.filter((e) => e === "destroy").length, 1);
});
test("autofill closing explains both stopping work and discarding edits", async () => {
  const { controller, events } = setup({ dirty: true, running: true });
  await controller.request();
  const prompt = events.find((e) => e.message);
  assert.match(prompt.message, /stops autofill/);
  assert.match(prompt.message, /discarded/);
  assert.equal(prompt.labels.okLabel, "Stop autofill and close");
});
for (const failure of ["confirm", "destroy"])
  test(`${failure} failure is visible and retryable`, async () => {
    let fail = true;
    const { controller, events } = setup(
      { dirty: true, running: false },
      {
        [failure]: async () => {
          if (fail) throw new Error("Unavailable");
          return true;
        },
      },
    );
    await controller.request();
    assert.equal(events.at(-1).state, "idle");
    assert.match(events.at(-1).error, /Unavailable/);
    fail = false;
    await controller.request();
    assert.ok(events.some((e) => e.state === "closing"));
  });
