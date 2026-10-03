type Watcher = {
  observer: IntersectionObserver;
  callbacks: Map<Element, () => void>;
};

const watchers = new Map<Element | null, Watcher>();

// Share one observer per scrolling surface. Observing the window instead of a
// dialog's scroll container clips its load-ahead margin at the dialog boundary.
export function observePreview(node: Element, ready: () => void) {
  const body = node.closest(".editor-body");
  // A nested card preview is in the top layer, outside its parent's scroll area.
  const root = body?.closest("dialog") === node.closest("dialog") ? body : null;
  let watcher = watchers.get(root);
  if (!watcher) {
    const callbacks = new Map<Element, () => void>();
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          const callback = callbacks.get(entry.target);
          callbacks.delete(entry.target);
          observer.unobserve(entry.target);
          callback?.();
        }
        if (!callbacks.size) {
          observer.disconnect();
          if (watchers.get(root)?.observer === observer) watchers.delete(root);
        }
      },
      { root, rootMargin: "600px 0px" },
    );
    watcher = { observer, callbacks };
    watchers.set(root, watcher);
  }
  const current = watcher;
  current.callbacks.set(node, ready);
  current.observer.observe(node);
  return () => {
    current.callbacks.delete(node);
    current.observer.unobserve(node);
    if (!current.callbacks.size) {
      current.observer.disconnect();
      if (watchers.get(root) === current) watchers.delete(root);
    }
  };
}
