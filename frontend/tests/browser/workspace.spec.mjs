import { test, expect } from "@playwright/test";

async function launch(page) {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.addInitScript(() => {
    const callbacks = new Map();
    const listeners = new Map();
    let id = 0;
    const png =
      "data:image/svg+xml," +
      encodeURIComponent(
        '<svg xmlns="http://www.w3.org/2000/svg" width="200" height="280"><rect width="200" height="280" fill="#436b60"/><text x="100" y="145" fill="white" text-anchor="middle">Test artwork</text></svg>',
      );
    window.__test = {
      calls: [],
      accept: false,
      failDestroy: false,
      failures: {},
      delay: {},
      imported: "1 Island\n1 Typo",
      emit: async (event, payload = null) => {
        for (const handler of listeners.get(event) || [])
          await callbacks.get(handler)({ event, id: handler, payload });
      },
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
    window.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
      },
      transformCallback: (callback) => {
        callbacks.set(++id, callback);
        return id;
      },
      unregisterCallback: (key) => callbacks.delete(key),
      convertFileSrc: () => png,
      invoke: async (command, args = {}) => {
        window.__test.calls.push({ command, args });
        if (command === "plugin:event|listen") {
          listeners.set(args.event, [
            ...(listeners.get(args.event) || []),
            args.handler,
          ]);
          return args.handler;
        }
        if (command === "plugin:event|unlisten") return;
        if (command === "plugin:dialog|message")
          return window.__test.accept
            ? (args.buttons?.OkCancelCustom?.[0] ?? "Ok")
            : "Cancel";
        if (command === "plugin:window|destroy") {
          if (window.__test.failDestroy) throw new Error("Window unavailable");
          return;
        }
        if (command === "plugin:dialog|open") {
          if (window.__test.blurOnFileDialog) document.activeElement?.blur();
          return window.__test.path ?? "C:/fixtures/deck.txt";
        }
        if (command === "plugin:dialog|save")
          return (
            window.__test.savePath ??
            args.options?.defaultPath ??
            args.defaultPath ??
            null
          );
        if (command === "save_order") {
          if (window.__test.failSave) throw new Error("Permission denied");
          window.__test.order = {
            ...window.__test.order,
            path: args.path || window.__test.order.path,
            dirty: false,
          };
          window.__test.persistedOrder = structuredClone(window.__test.order);
          return window.__test.order;
        }
        if (command === "approve_local_image" || command === "replace_local_image") {
          // Native consent/file picking stays in Rust. The UI sees only the
          // backend's resulting summary (unchanged when the user cancels).
          window.__test.order = structuredClone(window.__test.localReviewResult ?? window.__test.order);
          return window.__test.order;
        }
        if (command === "load_order")
          return (
            window.__test.order ?? {
              name: "Saved deck",
              quantity: 1,
              stock: "S30",
              foil: false,
              cards: [],
              path: "C:/fixtures/deck.xml",
              dirty: false,
            }
          );
        if (command === "download" || command === "fill")
          return new Promise((_resolve, reject) => {
            window.__test.stopBrowser = () =>
              reject(
                new Error("browser/closed: The automation browser was closed."),
              );
          });
        if (command === "read_decklist") return window.__test.imported;
        if (
          command === "preview_new_decklist" ||
          command === "preview_decklist"
        ) {
          const unresolved = [];
          const issues = [];
          let quantity = 0;
          for (const [index, line] of args.text.split("\n").entries()) {
            if (!line.trim() || line.startsWith("#")) continue;
            const match = line.match(/^(\d+) (.+)$/);
            if (!match || Number(match[1]) === 0) {
              issues.push({
                code: "invalid_line",
                message: "Enter a quantity and card name",
                line: index + 1,
              });
              continue;
            }
            quantity += Number(match[1]);
            const found = unresolved.find(
              (item) => item.query.toLowerCase() === match[2].toLowerCase(),
            );
            if (found) found.source_lines.push(index + 1);
            else
              unresolved.push({
                query: match[2],
                face: "front",
                source_lines: [index + 1],
              });
          }
          const retained = [];
          if (command === "preview_decklist" && window.__test.order) {
            for (let i = unresolved.length - 1; i >= 0; i--) {
              const item = unresolved[i];
              const card = window.__test.order.cards.find(
                (c) =>
                  c.query?.toLowerCase() === item.query.toLowerCase() &&
                  c.face === item.face,
              );
              if (card) {
                retained.unshift({ ...item, card });
                unresolved.splice(i, 1);
              }
            }
          }
          return {
            quantity,
            unresolved,
            retained,
            common_back:
              command === "preview_decklist"
                ? (window.__test.order?.cards.find((c) => c.cardback) ?? null)
                : null,
            issues,
            needs_common_back:
              quantity > 0 && command === "preview_new_decklist",
          };
        }
        if (command === "search_artwork") {
          if (window.__test.delay[args.query])
            await new Promise((resolve) =>
              setTimeout(resolve, window.__test.delay[args.query]),
            );
          if (window.__test.failures[args.query] > 0) {
            window.__test.failures[args.query]--;
            throw new Error("Offline");
          }
          if (["Typo", "Other typo"].includes(args.query))
            return { cards: [], count: 0, next_page: null };
          const cards = [0, 1].map((index) => ({
            drive_id: `${args.query}-${index}`,
            name: `${args.query} ${index}.png`,
            label: `${args.query} art ${index}`,
            source: "Test source",
            dpi: 1200,
            thumb: png,
          }));
          return { cards, count: 2, next_page: null };
        }
        if (command === "artwork_thumbnail") {
          if (window.__test.thumbFailure) throw new Error("Image offline");
          return "test.jpg";
        }
        if (command === "artwork_dpi") return window.__test.dpiValues ?? {};
        if (command === "edit_decklist" && window.__test.order) {
          window.__test.order = {
            ...window.__test.order,
            ...args.options,
            dirty: true,
          };
          return window.__test.order;
        }
        if (command === "create_order" || command === "edit_decklist")
          return {
            name: "Test deck",
            quantity: 2,
            stock: "S30",
            foil: false,
            cards: [],
            path: "",
            dirty: true,
          };
        if (command === "get_decklist")
          return window.__test.decklist ?? "1 Island";
        if (command === "thumbnails") {
          if (window.__test.manualThumbnails) return;
          for (const card of window.__test.order?.cards ?? []) {
            if (card.source === "LocalFile" && !card.local_approved) continue;
            await window.__test.emit("thumb-ready", {
              drive_id: card.drive_id,
              session_id: window.__test.order.session_id,
              source: card.source ?? "GoogleDrive",
              path: window.__test.thumbFailure ? null : "test.jpg",
              error: window.__test.thumbFailure ? "Image offline" : null,
            });
          }
          return;
        }
        throw new Error(`Unexpected command: ${command}`);
      },
    };
  });
  await page.goto("/");
  await expect(
    page.getByRole("button", { name: "Paste a decklist", exact: true }),
  ).toBeEnabled();
  return errors;
}
async function paste(page, text = "1 Island\n1 Typo") {
  await page
    .getByRole("button", { name: "Paste a decklist", exact: true })
    .click();
  const editor = page.getByLabel("Decklist", { exact: true });
  await editor.focus();
  // Exercise paste intent without changing the user's system clipboard.
  await editor.evaluate((el, value) => {
    el.setSelectionRange(0, el.value.length);
    const clipboardData = new DataTransfer();
    clipboardData.setData("text/plain", value);
    el.dispatchEvent(
      new ClipboardEvent("paste", { clipboardData, bubbles: true }),
    );
    el.setRangeText(value, 0, el.value.length, "end");
    el.dispatchEvent(
      new InputEvent("input", {
        inputType: "insertFromPaste",
        data: value,
        bubbles: true,
      }),
    );
  }, text);
  await expect(page.locator(".artwork-card:not(.deck-cardback)")).toHaveCount(
    text.includes("\n") ? 2 : 1,
  );
}
const tile = (page, name) =>
  page.getByRole("article", { name: `${name} front artwork`, exact: true });

async function openSavedDeck(page) {
  await page.evaluate(() => {
    window.__test.path = "C:/fixtures/deck.xml";
    window.__test.decklist = "4 Island\n1 Mountain";
    window.__test.order = {
      name: "deck",
      quantity: 5,
      stock: "(S30) Standard Smooth",
      foil: false,
      path: "C:/fixtures/deck.xml",
      dirty: false,
      cards: [
        {
          face: "front",
          drive_id: "saved-island",
          name: "Island.png",
          query: "Island",
          slots: [0, 1, 2, 3],
          cardback: false,
        },
        {
          face: "front",
          drive_id: "saved-mountain",
          name: "Mountain.png",
          query: "Mountain",
          slots: [4],
          cardback: false,
        },
        {
          face: "back",
          drive_id: "saved-back",
          name: "Common back.png",
          query: null,
          slots: [0, 1, 2, 3, 4],
          cardback: true,
        },
      ],
    };
  });
  await page.getByRole("button", { name: "Open order", exact: true }).click();
}

async function openLocalDeck(page) {
  await page.evaluate(() => {
    window.__test.path = "C:/fixtures/local-deck.xml";
    window.__test.order = {
      name: "local-deck", quantity: 1, stock: "S30", foil: false,
      path: window.__test.path, dirty: false,
      cards: [
        { face: "front", drive_id: "art/original.png", name: "Local card.png", slots: [0], cardback: false, source: "LocalFile", local_approved: false },
        { face: "back", drive_id: "saved-back", name: "Common back.png", slots: [0], cardback: true, source: "GoogleDrive", local_approved: false },
      ],
    };
    window.__test.persistedOrder = structuredClone(window.__test.order);
  });
  await page.getByRole("button", { name: "Open order", exact: true }).click();
}

test("local images stay blocked after cancelled consent and become ready only after native approval", async ({ page }) => {
  const errors = await launch(page);
  await openLocalDeck(page);
  const start = page.getByRole("button", { name: "Start autofill", exact: true });
  await expect(start).toBeDisabled();
  await page.getByRole("button", { name: /Needs attention\s*1/ }).click();
  const localCard = page.getByRole("button", { name: /Preview Local card.png/ });
  await expect(localCard).toBeVisible();
  await expect(localCard.locator("img")).toHaveCount(0);
  expect(await page.evaluate(() => window.__test.calls.filter(call => /approve_local|replace_local|^fill$/.test(call.command)))).toEqual([]);

  await page.getByRole("button", { name: "Review local images", exact: true }).click();
  const review = page.getByRole("dialog", { name: "Review local images" });
  await review.getByRole("button", { name: "Allow image…", exact: true }).click();
  await expect(review.getByText("Not read or approved", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => window.__test.calls.filter(call => call.command === "approve_local_image"))).toEqual([
    { command: "approve_local_image", args: { driveId: "art/original.png" } },
  ]);
  await review.getByRole("button", { name: "Done", exact: true }).click();
  await expect(start).toBeDisabled();
  await expect(page.getByRole("button", { name: /Needs attention\s*1/ })).toBeVisible();
  await expect(page.locator(".order-save-state")).toHaveText(/Saved.*local-deck.xml/);

  await page.evaluate(() => {
    window.__test.localReviewResult = structuredClone(window.__test.order);
    window.__test.localReviewResult.cards[0].local_approved = true;
  });
  await page.getByRole("button", { name: "Review local images", exact: true }).click();
  await review.getByRole("button", { name: "Allow image…", exact: true }).click();
  await expect(review.getByText("Image approved for this session", { exact: true })).toBeVisible();
  await review.getByRole("button", { name: "Done", exact: true }).click();
  await expect(start).toBeEnabled();
  await expect(page.getByRole("button", { name: "Needs attention", exact: true })).toBeVisible();
  await expect(page.locator(".order-save-state")).toHaveText(/Saved.*local-deck.xml/);
  expect(errors).toEqual([]);
});

test("local replacement sends only the existing entry ID and waits for explicit Save", async ({ page }) => {
  const errors = await launch(page);
  await openLocalDeck(page);
  await page.getByRole("button", { name: "Review local images", exact: true }).click();
  const review = page.getByRole("dialog", { name: "Review local images" });
  await review.getByRole("button", { name: "Choose replacement…", exact: true }).click();
  await expect(review.getByText("art/original.png", { exact: true })).toBeVisible();
  await expect(review.getByText("Not read or approved", { exact: true })).toBeVisible();

  await page.evaluate(() => {
    window.__test.localReviewResult = structuredClone(window.__test.order);
    window.__test.localReviewResult.cards[0].drive_id = "C:/fixtures/replacement.png";
    window.__test.localReviewResult.cards[0].local_approved = true;
    window.__test.localReviewResult.dirty = true;
  });
  await review.getByRole("button", { name: "Choose replacement…", exact: true }).click();
  await expect(review.getByText("C:/fixtures/replacement.png", { exact: true })).toBeVisible();
  await expect(review.getByText("art/original.png", { exact: true })).toHaveCount(0);
  expect(await page.evaluate(() => window.__test.calls.filter(call => call.command === "replace_local_image"))).toEqual([
    { command: "replace_local_image", args: { driveId: "art/original.png" } },
    { command: "replace_local_image", args: { driveId: "art/original.png" } },
  ]);
  await review.getByRole("button", { name: "Done", exact: true }).click();
  await expect(page.locator(".order-save-state")).toHaveText(/Unsaved changes.*local-deck.xml/);
  expect(await page.evaluate(() => window.__test.persistedOrder.cards[0].drive_id)).toBe("art/original.png");
  expect(await page.evaluate(() => window.__test.calls.filter(call => call.command === "save_order"))).toEqual([]);
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.locator(".order-save-state")).toHaveText(/Saved.*local-deck.xml/);
  expect(await page.evaluate(() => window.__test.persistedOrder.cards[0].drive_id)).toBe("C:/fixtures/replacement.png");
  expect(errors).toEqual([]);
});

test("DPI requests exclude local filenames and keep local and remote copies of one ID separate", async ({ page }) => {
  const errors = await launch(page);
  await page.evaluate(() => {
    window.__test.path = "C:/fixtures/dpi-boundaries.xml";
    window.__test.dpiValues = { sharedDriveId: 1200, remoteBack: 600 };
    window.__test.order = {
      name: "dpi-boundaries", quantity: 3, stock: "S30", foil: false,
      path: window.__test.path, dirty: false, session_id: 2,
      cards: [
        { face: "front", drive_id: "privateImage123", name: "Private image.png", slots: [0], cardback: false, source: "LocalFile", local_approved: false },
        { face: "front", drive_id: "sharedDriveId", name: "Local twin.png", slots: [1], cardback: false, source: "LocalFile", local_approved: false },
        { face: "front", drive_id: "sharedDriveId", name: "Remote twin.png", slots: [2], cardback: false, source: "GoogleDrive" },
        { face: "back", drive_id: "remoteBack", name: "Remote back.png", slots: [0, 1, 2], cardback: true, source: "GoogleDrive" },
      ],
    };
  });
  await page.getByRole("button", { name: "Open order", exact: true }).click();
  const privateImage = page.getByRole("button", { name: /Preview Private image.png/ });
  const localTwin = page.getByRole("button", { name: /Preview Local twin.png/ });
  const remoteTwin = page.getByRole("button", { name: /Preview Remote twin.png/ });
  await expect(remoteTwin.getByText("1200 DPI", { exact: true })).toBeVisible();
  await expect(localTwin.getByText("DPI unavailable", { exact: true })).toBeVisible();
  await expect(privateImage.getByText("DPI unavailable", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => window.__test.calls.filter(call => call.command === "artwork_dpi"))).toEqual([
    { command: "artwork_dpi", args: { driveIds: ["sharedDriveId", "remoteBack"] } },
  ]);
  await expect(remoteTwin.locator("img")).toHaveCount(1);
  await expect(localTwin.locator("img")).toHaveCount(0);
  await expect(privateImage.locator("img")).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("thumbnail events from an old session cannot cross a queued order replacement", async ({ page }) => {
  const errors = await launch(page);
  await page.evaluate(() => {
    window.__test.path = "C:/fixtures/epoch-one.xml";
    window.__test.manualThumbnails = true;
    window.__test.order = {
      name: "Epoch one", quantity: 1, stock: "S30", foil: false,
      path: window.__test.path, dirty: false, session_id: 1,
      cards: [{ face: "front", drive_id: "same-image", name: "Epoch card.png", slots: [0], cardback: false, source: "GoogleDrive" }],
    };
  });
  await page.getByRole("button", { name: "Open order", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Epoch one", exact: true })).toBeVisible();
  await page.evaluate(async () => {
    // Hold the actual event batching callbacks so an old update remains queued
    // while the next load completes, without timing sleeps or mocked app logic.
    const frames = new Map();
    let nextFrame = 100000;
    const originalFrame = window.requestAnimationFrame;
    const originalCancel = window.cancelAnimationFrame;
    window.requestAnimationFrame = callback => {
      const id = ++nextFrame;
      frames.set(id, callback);
      return id;
    };
    window.cancelAnimationFrame = id => { frames.delete(id); originalCancel(id); };
    window.__test.flushHeldFrames = () => {
      const callbacks = [...frames.values()];
      frames.clear();
      callbacks.forEach(callback => callback(performance.now()));
    };
    window.__test.restoreFrames = () => {
      window.requestAnimationFrame = originalFrame;
      window.cancelAnimationFrame = originalCancel;
    };
    await window.__test.emit("thumb-ready", { drive_id: "same-image", source: "GoogleDrive", session_id: 1, path: "old-before-load.jpg" });
    window.__test.oldFrameWasQueued = frames.size > 0;
    window.__test.path = "C:/fixtures/epoch-two.xml";
    window.__test.order = { ...structuredClone(window.__test.order), name: "Epoch two", path: window.__test.path, session_id: 2 };
    document.querySelector(".more-actions").open = true;
    [...document.querySelectorAll(".more-actions button")].find(button => button.textContent.trim() === "Open order").click();
  });
  await expect(page.getByRole("heading", { name: "Epoch two", exact: true })).toBeVisible();
  await page.evaluate(async () => {
    await window.__test.emit("thumb-ready", { drive_id: "same-image", source: "GoogleDrive", session_id: 1, path: "old-after-load.jpg" });
    window.__test.flushHeldFrames();
    window.__test.restoreFrames();
  });
  expect(await page.evaluate(() => window.__test.oldFrameWasQueued)).toBe(true);
  const card = page.getByRole("button", { name: /Preview Epoch card.png/ });
  await expect(card.locator("img")).toHaveCount(0);
  await page.evaluate(() => window.__test.emit("thumb-ready", { drive_id: "same-image", source: "GoogleDrive", session_id: 2, path: "current.jpg" }));
  await expect(card.locator("img")).toHaveCount(1);
  await page.evaluate(async () => {
    await window.__test.emit("thumb-ready", { drive_id: "same-image", source: "GoogleDrive", session_id: 1, path: null, error: "Stale preview failure" });
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  });
  await expect(card.locator("img")).toHaveCount(1);
  await expect(page.getByRole("button", { name: "Retry preview for Epoch card", exact: true })).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("saved artwork is complete in Edit list without a lookup or discard warning", async ({
  page,
}) => {
  await launch(page);
  await openSavedDeck(page);
  await expect(page.locator(".order-save-state")).toHaveText(/Saved.*deck.xml/);
  await page.getByRole("button", { name: "Edit list", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "All (3)", exact: true }),
  ).toBeVisible();
  await expect(
    tile(page, "Island").getByText("Artwork selected", { exact: true }),
  ).toBeVisible();
  await expect(page.getByLabel("Decklist", { exact: true })).toHaveValue(
    "4 Island\n1 Mountain",
  );
  expect(
    await page.evaluate(() =>
      window.__test.calls.filter((c) => c.command === "search_artwork"),
    ),
  ).toEqual([]);
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(
    page.getByRole("dialog", { name: "Edit your decklist" }),
  ).toHaveCount(0);
  expect(
    await page.evaluate(() =>
      window.__test.calls.filter((c) => c.command === "plugin:dialog|message"),
    ),
  ).toEqual([]);
});

test("print settings apply as unsaved changes and a renamed deck suggests a new XML filename", async ({
  page,
}) => {
  await launch(page);
  await openSavedDeck(page);
  await page.getByRole("button", { name: "Edit print settings" }).click();
  await page.getByLabel("Deck name", { exact: true }).fill("Revised deck");
  await page.getByLabel("Cardstock", { exact: true }).selectOption("M31");
  await page.getByLabel("Foil finish", { exact: true }).check();
  await page
    .getByRole("button", { name: "Apply changes", exact: true })
    .click();
  await expect(page.locator(".order-save-state")).toHaveText(
    /Unsaved changes.*deck.xml/,
  );
  const applied = await page.evaluate(
    () => window.__test.calls.find((c) => c.command === "edit_decklist").args,
  );
  expect(applied.options).toMatchObject({
    name: "Revised deck",
    stock: "M31",
    foil: true,
  });
  expect(applied.selections).toEqual([]);
  expect(
    await page.evaluate(() =>
      window.__test.calls.some((c) => c.command === "save_order"),
    ),
  ).toBe(false);
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.locator(".order-save-state")).toHaveText(
    /Saved.*Revised deck.xml/,
  );
  expect(
    await page.evaluate(
      () =>
        window.__test.calls.find((c) => c.command === "plugin:dialog|save")
          .args,
    ),
  ).toMatchObject({ options: { defaultPath: "C:/fixtures/Revised deck.xml" } });
});

test("syntax problems count in attention and links navigate only when activated", async ({
  page,
}) => {
  await launch(page);
  await openSavedDeck(page);
  await page.getByRole("button", { name: "Edit list", exact: true }).click();
  const editor = page.getByLabel("Decklist", { exact: true });
  await editor.fill("0 Broken\n4 Island");
  await expect(
    page.getByRole("button", { name: "Needs attention (1)", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Needs attention (1)", exact: true })
    .click();
  await expect(page.getByText("No artwork needs attention.")).toHaveCount(0);
  await page.getByRole("button", { name: "Edit line 1", exact: true }).click();
  await expect(editor).toBeFocused();
  expect(
    await editor.evaluate((el) =>
      el.value.slice(el.selectionStart, el.selectionEnd),
    ),
  ).toBe("0 Broken");
  await editor.fill("1 Island\n4 Island");
  await expect(
    page.getByRole("button", { name: "Needs attention (0)", exact: true }),
  ).toBeVisible();
  await expect(editor).toBeFocused();
});

test("save and thumbnail failures stay specific, retryable, and preserve the deck", async ({
  page,
}) => {
  await launch(page);
  await page.evaluate(() => {
    window.__test.thumbFailure = true;
  });
  await openSavedDeck(page);
  await expect(
    page.getByRole("button", { name: "Retry preview for Island", exact: true }),
  ).toBeVisible();
  await page.evaluate(() => {
    window.__test.thumbFailure = false;
    window.__test.failSave = true;
  });
  await page
    .getByRole("button", { name: "Retry preview for Island", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Retry preview for Island", exact: true }),
  ).toHaveCount(0);
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(
    page.getByRole("alert").getByText("Couldn’t save", { exact: true }),
  ).toBeVisible();
  await expect(page.locator(".card-tile")).toHaveCount(3);
  await page.evaluate(() => {
    window.__test.failSave = false;
  });
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("closing the automation browser restores editing and allows another run", async ({
  page,
}) => {
  await launch(page);
  await openSavedDeck(page);
  await page
    .getByRole("button", { name: "Start autofill", exact: true })
    .click();
  await page.evaluate(() =>
    window.__test.emit("phase", { phase: "uploading-fronts" }),
  );
  await page.evaluate(() =>
    window.__test.emit("card-status", {
      drive_id: "saved-island",
      face: "front",
      status: "uploading",
    }),
  );
  await expect(
    page.getByRole("button", { name: "Edit list", exact: true }),
  ).toBeDisabled();
  await page.evaluate(() => window.__test.stopBrowser());
  await expect(
    page.getByText("Autofill stopped", { exact: true }),
  ).toBeVisible();
  await expect(page.locator(".progress-copy")).toContainText(
    "Your deck is unchanged",
  );
  await expect(
    page.getByRole("button", { name: "Edit list", exact: true }),
  ).toBeEnabled();
  await expect(page.locator(".card-tile")).toHaveCount(3);
  await expect(
    page.getByText("Placement not confirmed", { exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Start over", exact: true }).click();
  expect(
    await page.evaluate(
      () => window.__test.calls.filter((c) => c.command === "fill").length,
    ),
  ).toBe(2);
});

for (const theme of ["light", "dark", "zero"])
  test(`compact saved deck shows artwork at 720x560 in ${theme}`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize({ width: 720, height: 560 });
    await launch(page);
    await page
      .getByRole("combobox", { name: "Appearance" })
      .selectOption(theme);
    await openSavedDeck(page);
    await expect(page.locator(".library-heading")).toContainText(
      "5 cards · 3 artwork images",
    );
    await expect(page.locator(".card-tile").first()).toBeInViewport();
    const bounds = await page.locator(".card-tile").first().boundingBox();
    expect(bounds.y).toBeLessThan(390);
    await page.getByRole("button", { name: "Backs", exact: true }).click();
    await expect(page.locator(".card-tile")).toHaveCount(1);
    await expect(page.locator(".card-tile")).toContainText("Common back");
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > window.innerWidth,
      ),
    ).toBe(false);
    await page.screenshot({ path: testInfo.outputPath("compact-deck.png") });
  });

test("stacked pane jump controls keep text navigation reachable", async ({
  page,
}) => {
  await page.setViewportSize({ width: 550, height: 410 });
  await launch(page);
  await paste(page);
  const jumps = page.getByRole("navigation", { name: "Jump to import pane" });
  await jumps.getByRole("button", { name: "Artwork", exact: true }).click();
  await expect(
    page.getByRole("region", { name: "Deck artwork", exact: true }),
  ).toBeFocused();
  await jumps.getByRole("button", { name: "Decklist", exact: true }).click();
  await expect(page.getByLabel("Decklist", { exact: true })).toBeFocused();
});

test("corrects a source line without losing another card's artwork", async ({
  page,
}) => {
  const errors = await launch(page);
  await paste(page);
  await expect(
    tile(page, "Typo").getByText("Card not found", { exact: true }),
  ).toBeVisible();
  await tile(page, "Island")
    .getByLabel("Next artwork for Island (front)")
    .click();
  await expect(
    tile(page, "Island").getByLabel("Artwork number for Island (front)"),
  ).toHaveValue("2");
  await tile(page, "Typo")
    .getByRole("button", { name: "Edit entry", exact: true })
    .click();
  const text = page.getByLabel("Decklist", { exact: true });
  await expect(text).toBeFocused();
  expect(
    await text.evaluate((el) =>
      el.value.slice(el.selectionStart, el.selectionEnd),
    ),
  ).toBe("1 Typo");
  await text.press("Backspace");
  await text.pressSequentially("1 Mountain");
  await expect(
    tile(page, "Mountain").getByText("Artwork selected", { exact: true }),
  ).toBeVisible();
  await expect(
    tile(page, "Island").getByLabel("Artwork number for Island (front)"),
  ).toHaveValue("2");
  await page
    .getByRole("button", { name: "Choose card back", exact: true })
    .click();
  await page
    .getByRole("dialog", { name: "Choose a card back" })
    .getByRole("button", { name: /Use artwork 1/ })
    .click();
  await expect(
    page.getByRole("button", { name: "Create deck", exact: true }),
  ).toBeEnabled();
  const settings = page.getByRole("region", { name: "Print settings" });
  await settings.getByLabel("Deck name").fill("My printed deck");
  await settings.getByLabel("Foil finish").check();
  await settings.getByLabel("Cardstock").selectOption("P10");
  await expect(settings.getByLabel("Foil finish")).not.toBeChecked();
  await expect(settings.getByLabel("Foil finish")).toBeDisabled();
  await settings.getByLabel("Cardstock").selectOption("S33");
  await settings.getByLabel("Foil finish").check();
  await page.getByRole("button", { name: "Create deck", exact: true }).click();
  expect(
    await page.evaluate(
      () =>
        window.__test.calls.find((c) => c.command === "create_order").args
          .options,
    ),
  ).toMatchObject({ name: "My printed deck", stock: "S33", foil: true });
  expect(errors).toEqual([]);
});

test("card-back setup appears first and remains included in attention counts", async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 720, height: 560 });
  await launch(page);
  await paste(page, "1 Island");
  await expect(
    tile(page, "Island").getByText("Artwork selected", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "All (2)", exact: true }),
  ).toBeVisible();
  const attention = page.getByRole("button", {
    name: "Needs attention (1)",
    exact: true,
  });
  const requirement = page.getByRole("article", {
    name: "Common card back",
    exact: true,
  });
  await expect(
    requirement.getByText("Choose your card back", { exact: true }),
  ).toBeInViewport();
  await expect(
    page.getByText("Card back required", { exact: false }),
  ).toHaveCount(0);
  const pickerBounds = await requirement.boundingBox();
  const artworkBounds = await tile(page, "Island").boundingBox();
  expect(pickerBounds.height).toBeGreaterThan(pickerBounds.width);
  expect(pickerBounds.width).toBeCloseTo(artworkBounds.width, 0);
  expect(pickerBounds.x).toBeCloseTo(artworkBounds.x, 0);
  expect(pickerBounds.y + pickerBounds.height).toBeLessThan(artworkBounds.y);
  await expect(
    requirement.getByRole("button", { name: "Browse card-back artwork" }),
  ).toBeInViewport();
  await expect(
    page.getByRole("button", { name: "Create deck", exact: true }),
  ).toBeDisabled();
  await attention.click();
  await expect(tile(page, "Island")).toHaveCount(0);
  await expect(requirement).toBeVisible();
  const filteredPickerBounds = await requirement.boundingBox();
  expect(filteredPickerBounds.width).toBeCloseTo(pickerBounds.width, 0);
  expect(filteredPickerBounds.height).toBeCloseTo(pickerBounds.height, 0);
  await expect(
    page.getByText("No artwork needs attention.", { exact: true }),
  ).toHaveCount(0);
  await page.screenshot({ path: testInfo.outputPath("card-back-setup.png") });
  await requirement
    .getByRole("button", { name: "Browse card-back artwork" })
    .focus();
  await page.keyboard.press("Enter");
  await page
    .getByRole("dialog", { name: "Choose a card back" })
    .getByRole("button", { name: /Use artwork 1/ })
    .click();
  await expect(
    page.getByRole("button", { name: "Needs attention (0)", exact: true }),
  ).toBeVisible();
  await expect(requirement).toHaveCount(0);
  await expect(
    page.getByText("No artwork needs attention.", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Create deck", exact: true }),
  ).toBeEnabled();
  await page.getByRole("button", { name: "All (2)", exact: true }).click();
  await expect(
    requirement.getByText("Card back selected", { exact: true }),
  ).toBeVisible();
  await expect(
    requirement.getByRole("button", { name: "Change card back" }),
  ).toBeEnabled();
  await expect(requirement.locator("img")).toBeVisible();
  await requirement
    .getByRole("button", { name: "Change card-back artwork" })
    .click();
  const gallery = page.getByRole("dialog", { name: "Choose a card back" });
  await expect(gallery).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(gallery).toHaveCount(0);
  await expect(
    requirement.getByRole("button", { name: "Change card-back artwork" }),
  ).toBeFocused();
  await expect(
    page.getByRole("button", { name: "Needs attention (0)", exact: true }),
  ).toBeVisible();
});

test("unmatched cards automatically select their source line and show prominent errors", async ({
  page,
}) => {
  await launch(page);
  await paste(page);
  const text = page.getByLabel("Decklist", { exact: true });
  await expect(
    page
      .locator(".draft-match-alert")
      .getByText("Card not found", { exact: true }),
  ).toBeVisible();
  await expect(text).toBeFocused();
  await expect
    .poll(() =>
      text.evaluate((el) => el.value.slice(el.selectionStart, el.selectionEnd)),
    )
    .toBe("1 Typo");
  await expect(text).toHaveAttribute("aria-invalid", "true");
  await expect(page.locator(".decklist-gutter .problem-line")).toHaveText("2");
  await expect(tile(page, "Typo")).toHaveClass(/artwork-not-found/);
  await expect(
    page.getByRole("button", { name: "Needs attention (2)", exact: true }),
  ).toBeVisible();
  await text.pressSequentially("1 Mountain");
  await expect(
    tile(page, "Mountain").getByText("Artwork selected", { exact: true }),
  ).toBeVisible();
  await expect(page.locator(".draft-match-alert")).toHaveCount(0);
  await expect(text).toHaveAttribute("aria-invalid", "false");
  await expect(
    page.getByRole("button", { name: "Needs attention (1)", exact: true }),
  ).toBeVisible();
});

test("late failures do not repeatedly move the caret", async ({ page }) => {
  await launch(page);
  await page.evaluate(() => {
    window.__test.delay["Other typo"] = 1000;
  });
  await paste(page, "1 Typo\n1 Other typo");
  const text = page.getByLabel("Decklist", { exact: true });
  await expect
    .poll(() =>
      text.evaluate((el) => el.value.slice(el.selectionStart, el.selectionEnd)),
    )
    .toBe("1 Typo");
  await text.press("ArrowRight");
  await expect(
    page
      .locator(".draft-match-alert")
      .getByText("2 entries not found", { exact: true }),
  ).toBeVisible();
  expect(
    await text.evaluate((el) => el.selectionStart === el.selectionEnd),
  ).toBe(true);
});

test("paused edits do not reselect an unresolved entry", async ({ page }) => {
  await launch(page);
  await paste(page);
  const text = page.getByLabel("Decklist", { exact: true });
  await expect
    .poll(() =>
      text.evaluate((el) => el.value.slice(el.selectionStart, el.selectionEnd)),
    )
    .toBe("1 Typo");
  await text.press("Control+Home");
  await text.press("End");
  // Each pause is longer than the preview debounce, with another error still present.
  await text.pressSequentially("sss", { delay: 500 });
  await expect(
    tile(page, "Islandsss").getByText("Artwork selected", { exact: true }),
  ).toBeVisible();
  await expect(text).toHaveValue("1 Islandsss\n1 Typo");
  expect(
    await text.evaluate((el) => [el.selectionStart, el.selectionEnd]),
  ).toEqual([11, 11]);
  await expect(page.locator(".draft-match-alert")).toBeVisible();
  await page
    .locator(".draft-match-alert")
    .getByRole("button", { name: "Line 2 · Typo" })
    .click();
  expect(
    await text.evaluate((el) =>
      el.value.slice(el.selectionStart, el.selectionEnd),
    ),
  ).toBe("1 Typo");
});

test("moving the caret cancels pending import focus", async ({ page }) => {
  await launch(page);
  await page.evaluate(() => {
    window.__test.delay.Typo = 1000;
  });
  await paste(page);
  const text = page.getByLabel("Decklist", { exact: true });
  await text.press("Control+Home");
  await expect(page.locator(".draft-match-alert")).toBeVisible();
  expect(
    await text.evaluate((el) => [el.selectionStart, el.selectionEnd]),
  ).toEqual([0, 0]);
  await expect(text).toBeFocused();
});

test("typing before a pending match and clicking the editor both keep the caret", async ({
  page,
}) => {
  await launch(page);
  await page.evaluate(() => {
    window.__test.delay.Typo = 1000;
  });
  await paste(page);
  const text = page.getByLabel("Decklist", { exact: true });
  await text.press("Control+Home");
  await text.pressSequentially("# Comment\n");
  await expect(page.locator(".draft-match-alert")).toBeVisible();
  expect(
    await text.evaluate((el) => [el.selectionStart, el.selectionEnd]),
  ).toEqual([10, 10]);
  // Import another list, then deliberately place the caret while its search is pending.
  await page.evaluate(() => {
    window.__test.accept = true;
    window.__test.imported = "1 Island\n1 Other typo";
    window.__test.delay["Other typo"] = 1000;
  });
  await page.getByRole("button", { name: "Import .txt", exact: true }).click();
  await expect(text).toHaveValue("1 Island\n1 Other typo");
  await text.click({ position: { x: 14, y: 16 } });
  const caret = await text.evaluate((el) => [
    el.selectionStart,
    el.selectionEnd,
  ]);
  await expect(page.locator(".draft-match-alert")).toBeVisible();
  expect(
    await text.evaluate((el) => [el.selectionStart, el.selectionEnd]),
  ).toEqual(caret);
});

test("a fresh file import offers one initial error navigation", async ({
  page,
}) => {
  await launch(page);
  await paste(page, "1 Island");
  await page.evaluate(() => {
    window.__test.accept = true;
    window.__test.blurOnFileDialog = true;
  });
  await page.getByRole("button", { name: "Import .txt", exact: true }).click();
  const text = page.getByLabel("Decklist", { exact: true });
  await expect
    .poll(() =>
      text.evaluate((el) => el.value.slice(el.selectionStart, el.selectionEnd)),
    )
    .toBe("1 Typo");
  await expect(text).toBeFocused();
});

test("an unmatched result waits until the card-back gallery closes to focus text", async ({
  page,
}) => {
  await launch(page);
  await page.evaluate(() => {
    window.__test.delay.Typo = 1000;
  });
  await paste(page, "1 Typo");
  await page
    .getByRole("button", { name: "Choose card back", exact: true })
    .click();
  const gallery = page.getByRole("dialog", { name: "Choose a card back" });
  await expect(gallery).toBeVisible();
  await expect(gallery.getByLabel("Search card backs")).toBeEnabled();
  await gallery.getByLabel("Search card backs").focus();
  await page.waitForTimeout(1100);
  await expect(gallery.getByLabel("Search card backs")).toBeFocused();
  await gallery.getByRole("button", { name: "Close artwork gallery" }).click();
  const text = page.getByLabel("Decklist", { exact: true });
  await expect(text).toBeFocused();
  expect(
    await text.evaluate((el) =>
      el.value.slice(el.selectionStart, el.selectionEnd),
    ),
  ).toBe("1 Typo");
});

test("a superseded unmatched response does not select text being corrected", async ({
  page,
}) => {
  await launch(page);
  await page.evaluate(() => {
    window.__test.delay.Typo = 1000;
  });
  await paste(page, "1 Typo");
  const text = page.getByLabel("Decklist", { exact: true });
  await text.fill("1 Mountain");
  await expect(
    tile(page, "Mountain").getByText("Artwork selected", { exact: true }),
  ).toBeVisible();
  await page.waitForTimeout(1100);
  expect(
    await text.evaluate((el) => [el.selectionStart, el.selectionEnd]),
  ).toEqual([10, 10]);
  await expect(page.locator(".draft-match-alert")).toHaveCount(0);
});
test("gallery searches replace artwork only and clear stale results on error", async ({
  page,
}) => {
  await launch(page);
  await paste(page, "1 Island");
  await tile(page, "Island")
    .getByRole("button", { name: "Choose different artwork" })
    .click();
  const gallery = page.getByRole("dialog", { name: "Island", exact: true });
  await expect(
    gallery.getByRole("button", { name: /Island art 0/ }),
  ).toBeVisible();
  await page.evaluate(() => {
    window.__test.failures.Mountain = 1;
  });
  await gallery.getByLabel("Search replacement artwork").fill("Mountain");
  await gallery.getByLabel("Search replacement artwork").press("Enter");
  await expect(gallery.getByText(/Search failed for “Mountain”/)).toBeVisible();
  await expect(gallery.locator(".artwork-choice")).toHaveCount(0);
  await gallery
    .getByRole("button", { name: "Retry search", exact: true })
    .click();
  await gallery.getByRole("button", { name: /Mountain art 1/ }).click();
  await expect(page.getByLabel("Decklist", { exact: true })).toHaveValue(
    "1 Island",
  );
  await expect(
    tile(page, "Island").getByLabel("Artwork number for Island (front)"),
  ).toHaveValue("2");
  await tile(page, "Island")
    .getByRole("button", { name: "Choose different artwork" })
    .click();
  await gallery
    .getByRole("button", { name: "Edit entry", exact: true })
    .click();
  await expect(page.getByLabel("Decklist", { exact: true })).toBeFocused();
});
test("filters during loading without restarting work and shows syntax errors", async ({
  page,
}) => {
  await launch(page);
  await page.evaluate(() => {
    window.__test.delay.Island = 900;
  });
  await paste(page);
  await page.getByRole("button", { name: /Needs attention \(/ }).click();
  await expect(tile(page, "Island")).toHaveCount(0);
  await page.getByRole("button", { name: /All \(/ }).click();
  await expect(
    tile(page, "Island").getByText("Artwork selected", { exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () =>
        window.__test.calls.filter(
          (c) => c.command === "search_artwork" && c.args.query === "Island",
        ).length,
    ),
  ).toBe(1);
  await page.getByLabel("Decklist", { exact: true }).fill("0 Broken\n1 Island");
  await expect(page.getByText("1 cards · Preview incomplete")).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Create deck", exact: true }),
  ).toBeDisabled();
  await page
    .locator(".draft-diagnostics")
    .getByRole("button", { name: "Line 1", exact: true })
    .click();
  await expect(page.getByLabel("Decklist", { exact: true })).toBeFocused();
});
test("imports text immediately and links every occurrence of a grouped name", async ({
  page,
}) => {
  await launch(page);
  await page.evaluate(() => {
    window.__test.imported = "1 Island\n2 island";
  });
  await page
    .getByRole("button", { name: "Import .txt decklist", exact: true })
    .click();
  await expect(
    tile(page, "Island").getByText("Artwork selected", { exact: true }),
  ).toBeVisible();
  await tile(page, "Island")
    .getByRole("button", { name: "Line 2", exact: true })
    .click();
  expect(
    await page
      .getByLabel("Decklist", { exact: true })
      .evaluate((el) => el.value.slice(el.selectionStart, el.selectionEnd)),
  ).toBe("2 island");
});
test("native close events respect dirty drafts, cancellation and retry", async ({
  page,
}) => {
  const errors = await launch(page);
  await paste(page, "1 Island");
  await page.evaluate(() => window.__test.emit("tauri://close-requested"));
  expect(
    await page.evaluate(
      () =>
        window.__test.calls.filter((c) => c.command === "plugin:window|destroy")
          .length,
    ),
  ).toBe(0);
  await page.evaluate(() => {
    window.__test.accept = true;
    window.__test.failDestroy = true;
  });
  await page.evaluate(() => window.__test.emit("tauri://close-requested"));
  await expect(
    page.getByRole("dialog", { name: "The app could not close" }),
  ).toBeVisible();
  await page.evaluate(() => {
    window.__test.failDestroy = false;
  });
  await page.getByRole("button", { name: "Retry close" }).click();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.__test.calls.filter(
            (c) => c.command === "plugin:window|destroy",
          ).length,
      ),
    )
    .toBe(2);
  expect(errors).toEqual([]);
});
test("an untouched empty editor closes without a discard prompt", async ({
  page,
}) => {
  await launch(page);
  await page
    .getByRole("button", { name: "Paste a decklist", exact: true })
    .click();
  await page.evaluate(() => window.__test.emit("tauri://close-requested"));
  expect(
    await page.evaluate(() =>
      window.__test.calls.some((c) => c.command === "plugin:dialog|message"),
    ),
  ).toBe(false);
  expect(
    await page.evaluate(() =>
      window.__test.calls.some((c) => c.command === "plugin:window|destroy"),
    ),
  ).toBe(true);
});

test("gallery ignores superseded searches and Escape returns focus to the draft", async ({
  page,
}) => {
  await launch(page);
  await paste(page, "1 Island");
  const browse = tile(page, "Island").getByRole("button", {
    name: "Choose different artwork",
  });
  await browse.click();
  const gallery = page.getByRole("dialog", { name: "Island", exact: true });
  await page.evaluate(() => {
    window.__test.delay.Slow = 1000;
  });
  const search = gallery.getByLabel("Search replacement artwork");
  await search.fill("Slow");
  await search.press("Enter");
  await search.fill("Fast");
  await search.press("Enter");
  await expect(
    gallery.getByRole("button", { name: /Fast art 0/ }),
  ).toBeVisible();
  await page.waitForTimeout(1100);
  await expect(gallery.getByRole("button", { name: /Slow art 0/ })).toHaveCount(
    0,
  );
  await search.press("Escape");
  await expect(gallery).toHaveCount(0);
  await expect(browse).toBeFocused();
  await expect(page.getByLabel("Decklist", { exact: true })).toHaveValue(
    "1 Island",
  );
});

test("editing and replacing a saved deck use the same synchronized workspace", async ({
  page,
}) => {
  const errors = await launch(page);
  await page.evaluate(() => {
    window.__test.path = "C:/fixtures/deck.xml";
  });
  await page.getByRole("button", { name: "Open order", exact: true }).click();
  await page.getByRole("button", { name: "Edit list", exact: true }).click();
  await expect(page.getByLabel("Decklist", { exact: true })).toHaveValue(
    "1 Island",
  );
  await page.getByLabel("Decklist", { exact: true }).fill("2 Mountain");
  await expect(
    tile(page, "Mountain").getByText("Artwork selected", { exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Apply changes", exact: true })
    .click();
  expect(
    await page.evaluate(
      () =>
        window.__test.calls.find((c) => c.command === "edit_decklist").args
          .text,
    ),
  ).toBe("2 Mountain");
  await page.locator(".more-actions > summary").click();
  await page.getByRole("button", { name: "Replace deck", exact: true }).click();
  await expect(page.getByLabel("Decklist", { exact: true })).toHaveValue("");
  await page.getByLabel("Decklist", { exact: true }).fill("1 Typo");
  await expect(
    tile(page, "Typo").getByText("Card not found", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Apply changes", exact: true }),
  ).toBeDisabled();
  expect(errors).toEqual([]);
});

test("saved decks close immediately and active autofill can be kept running or stopped", async ({
  page,
}) => {
  await launch(page);
  await page.evaluate(() => {
    window.__test.path = "C:/fixtures/deck.xml";
  });
  await page.getByRole("button", { name: "Open order", exact: true }).click();
  await page.evaluate(() => window.__test.emit("tauri://close-requested"));
  expect(
    await page.evaluate(() =>
      window.__test.calls.some((c) => c.command === "plugin:dialog|message"),
    ),
  ).toBe(false);
  await page.reload();
  await page.evaluate(() => {
    window.__test.path = "C:/fixtures/deck.xml";
  });
  await page.getByRole("button", { name: "Open order", exact: true }).click();
  await page
    .getByRole("button", { name: "Start autofill", exact: true })
    .click();
  await page.evaluate(() => window.__test.emit("tauri://close-requested"));
  expect(
    await page.evaluate(
      () =>
        window.__test.calls.find((c) => c.command === "plugin:dialog|message")
          .args.message,
    ),
  ).toContain("Closing stops autofill");
  expect(
    await page.evaluate(() =>
      window.__test.calls.some((c) => c.command === "plugin:window|destroy"),
    ),
  ).toBe(false);
  await page.evaluate(() => {
    window.__test.accept = true;
  });
  await page.evaluate(() => window.__test.emit("tauri://close-requested"));
  expect(
    await page.evaluate(() =>
      window.__test.calls.some((c) => c.command === "plugin:window|destroy"),
    ),
  ).toBe(true);
});
test("appearance defaults to Light independently of device settings and migrates old System choices", async ({
  page,
}) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await launch(page);
  const picker = page.getByRole("combobox", { name: "Appearance" });
  await expect(picker.locator("option")).toHaveText([
    "Light (Default)",
    "Dark",
    "Zero",
  ]);
  await expect(picker).toHaveValue("light");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  for (const previous of ["system", "unknown-theme"]) {
    await page.evaluate(
      (value) => localStorage.setItem("mpc-autofill-theme", value),
      previous,
    );
    await page.reload();
    await expect(picker).toHaveValue("light");
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  }
});

test("each appearance persists across reloads and ignores device theme changes", async ({
  page,
}, testInfo) => {
  const errors = await launch(page);
  const picker = page.getByRole("combobox", { name: "Appearance" });
  for (const theme of ["dark", "light", "zero"]) {
    await picker.selectOption(theme);
    await page.reload();
    await expect(picker).toHaveValue(theme);
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    await page.emulateMedia({ colorScheme: "dark" });
    await page.emulateMedia({ colorScheme: "light" });
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
  }
  const colors = await page.evaluate(() => {
    const style = getComputedStyle(document.documentElement);
    return Object.fromEntries(
      [
        "--text",
        "--bg",
        "--muted",
        "--panel",
        "--accent",
        "--accent-soft",
        "--button-text",
        "--button-bg",
        "--fail",
        "--fail-soft",
        "--warning",
        "--warning-soft",
      ].map((key) => [key, style.getPropertyValue(key).trim()]),
    );
  });
  const luminance = (hex) => {
    const rgb = hex
      .slice(1)
      .match(/../g)
      .map((channel) => {
        const value = parseInt(channel, 16) / 255;
        return value <= 0.04045
          ? value / 12.92
          : ((value + 0.055) / 1.055) ** 2.4;
      });
    return 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
  };
  for (const [foreground, background] of [
    ["--text", "--bg"],
    ["--muted", "--panel"],
    ["--accent", "--accent-soft"],
    ["--button-text", "--button-bg"],
    ["--fail", "--fail-soft"],
    ["--warning", "--warning-soft"],
  ]) {
    const values = [
      luminance(colors[foreground]),
      luminance(colors[background]),
    ].sort((a, b) => b - a);
    expect(
      (values[0] + 0.05) / (values[1] + 0.05),
      `${foreground} on ${background}`,
    ).toBeGreaterThanOrEqual(4.5);
  }
  await expect(
    page.getByRole("button", { name: "Import .txt decklist", exact: true }),
  ).toHaveCSS("background-color", "rgb(54, 224, 126)");
  await page.screenshot({ path: testInfo.outputPath("zero-welcome.png") });
  expect(errors).toEqual([]);
});

test("appearance can still change when preference storage is unavailable", async ({
  page,
}) => {
  await page.addInitScript(() => {
    Storage.prototype.getItem = () => {
      throw new Error("Storage unavailable");
    };
    Storage.prototype.setItem = () => {
      throw new Error("Storage unavailable");
    };
  });
  const errors = await launch(page);
  const picker = page.getByRole("combobox", { name: "Appearance" });
  await expect(picker).toHaveValue("light");
  await picker.selectOption("zero");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "zero");
  expect(errors).toEqual([]);
});

for (const [width, height] of [
  [1100, 820],
  [720, 560],
  [550, 410],
])
  for (const theme of ["light", "dark", "zero"]) {
    test(`workspace remains usable at ${width}x${height} in ${theme}`, async ({
      page,
    }, testInfo) => {
      await page.setViewportSize({ width, height });
      await launch(page);
      await page
        .getByRole("combobox", { name: "Appearance" })
        .selectOption(theme);
      await paste(page);
      await expect(
        tile(page, "Typo").getByText("Card not found", { exact: true }),
      ).toBeVisible();
      await expect(
        page
          .locator(".draft-match-alert")
          .getByText("Card not found", { exact: true }),
      ).toBeInViewport();
      const footer = await page
        .locator(".import-workspace > footer")
        .boundingBox();
      expect(footer.y + footer.height).toBeLessThanOrEqual(height);
      const selectedLineBottom = await page
        .getByLabel("Decklist", { exact: true })
        .evaluate((el) => {
          const style = getComputedStyle(el);
          const line = el.value.slice(0, el.selectionStart).split("\n").length;
          return (
            el.getBoundingClientRect().top +
            parseFloat(style.paddingTop) +
            line * parseFloat(style.lineHeight) -
            el.scrollTop
          );
        });
      expect(selectedLineBottom).toBeLessThanOrEqual(footer.y);
      const settings = page.getByRole("region", { name: "Print settings" });
      await expect(settings.getByLabel("Deck name")).toBeInViewport();
      await expect(settings.getByLabel("Cardstock")).toBeInViewport();
      await expect(settings.getByLabel("Foil finish")).toBeInViewport();
      const settingsBounds = await settings.boundingBox();
      const panesBounds = await page.locator(".workspace-panes").boundingBox();
      expect(settingsBounds.y + settingsBounds.height).toBeLessThanOrEqual(
        panesBounds.y,
      );
      const overflow = await page
        .locator(".import-workspace")
        .evaluate((el) => el.scrollWidth > el.clientWidth);
      expect(overflow).toBe(false);
      if (width >= 720) {
        const left = await page.locator(".deck-text-pane").boundingBox();
        const right = await page.locator(".deck-art-pane").boundingBox();
        expect(right.x).toBeGreaterThanOrEqual(left.x + left.width - 1);
      }
      await page.screenshot({ path: testInfo.outputPath("workspace.png") });
    });
  }
