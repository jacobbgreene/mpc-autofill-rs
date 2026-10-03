import { test } from "node:test";
import assert from "node:assert/strict";
import { observePreview } from "../src/lib/previewVisibility.ts";

class Observer {
  static instances = [];
  targets = new Set();
  disconnected = false;
  constructor(callback, options) {
    this.callback = callback;
    this.options = options;
    Observer.instances.push(this);
  }
  observe(node) {
    this.targets.add(node);
  }
  unobserve(node) {
    this.targets.delete(node);
  }
  disconnect() {
    this.disconnected = true;
    this.targets.clear();
  }
  deliver(node, visible = true) {
    this.callback([{ target: node, isIntersecting: visible }]);
  }
}
globalThis.IntersectionObserver = Observer;

const element = (body, dialog) => ({
  closest: (selector) => (selector === ".editor-body" ? body : dialog),
});

test("a deck shares one observer rooted in its scroll area and loads ahead once", () => {
  const dialog = {};
  const body = { closest: () => dialog };
  const first = element(body, dialog);
  const second = element(body, dialog);
  let loads = 0;
  const before = Observer.instances.length;
  const stopFirst = observePreview(first, () => loads++);
  const stopSecond = observePreview(second, () => loads++);
  assert.equal(Observer.instances.length, before + 1);
  const observer = Observer.instances.at(-1);
  assert.equal(observer.options.root, body);
  assert.equal(observer.options.rootMargin, "600px 0px");
  observer.deliver(first, false);
  assert.equal(loads, 0);
  observer.deliver(first);
  observer.deliver(first);
  assert.equal(loads, 1, "a thumbnail is fetched only once");
  assert.equal(observer.targets.has(first), false);
  stopSecond();
  observer.deliver(second);
  assert.equal(loads, 1, "destroyed cards cannot start a pending fetch");
  assert.equal(observer.disconnected, true);
  stopFirst();
});

test("an enlarged preview escapes its parent dialog's scroll clipping", () => {
  const parent = {};
  const enlarged = {};
  const body = { closest: () => parent };
  const node = element(body, enlarged);
  const stop = observePreview(node, () => {});
  assert.equal(Observer.instances.at(-1).options.root, null);
  stop();
});

test("separate scroll surfaces keep their visibility observers independent", () => {
  const dialog = {};
  const gallery = {};
  const deckBody = { closest: () => dialog };
  const galleryBody = { closest: () => gallery };
  const before = Observer.instances.length;
  const stopDeck = observePreview(element(deckBody, dialog), () => {});
  const stopGallery = observePreview(element(galleryBody, gallery), () => {});
  assert.equal(Observer.instances.length, before + 2);
  stopDeck();
  assert.equal(Observer.instances.at(-1).disconnected, false);
  stopGallery();
});
