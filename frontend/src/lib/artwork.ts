export const ARTWORK_PAGE_SIZE = 24;

export type Artwork = {
  drive_id: string;
  name: string;
  label: string;
  source: string;
  dpi: number;
  thumb: string;
};
export type ArtworkResults = {
  cards: Artwork[];
  count: number;
  next_page: number | null;
};
export type ArtworkKind = "card" | "cardback";
export type ArtworkSearch = (
  query: string,
  page: number,
  kind?: ArtworkKind,
  priority?: "interactive" | "background",
) => Promise<ArtworkResults>;
export type ArtworkChoice = {
  art: Artwork;
  query: string;
  index: number;
  count: number;
};

// One queue per editor keeps large decks from flooding the artwork service.
// Tiles and the gallery share cached pages, including requests already in flight.
export function createArtworkSearch(load: ArtworkSearch, concurrency = 4) {
  const cache = new Map<string, Promise<ArtworkResults>>();
  const queue: { run: () => void; cancel: () => void; background: boolean }[] =
    [];
  let running = 0;
  let disposed = false;
  function pump() {
    while (!disposed && running < concurrency && queue.length)
      queue.shift()!.run();
  }
  const search: ArtworkSearch = (
    query,
    page,
    kind = "card",
    priority = "interactive",
  ) => {
    if (disposed) return Promise.reject(new Error("Artwork search closed"));
    query = query.trim();
    const key = JSON.stringify([query, page, kind]);
    const existing = cache.get(key);
    if (existing) return existing;
    const request = new Promise<ArtworkResults>((resolve, reject) => {
      const job = {
        background: priority === "background",
        cancel: () => reject(new Error("Artwork search closed")),
        run: () => {
          running++;
          Promise.resolve()
            .then(() => load(query, page, kind))
            .then(resolve, reject)
            .finally(() => {
              running--;
              pump();
            });
        },
      };
      // A user's next artwork or gallery page should not wait behind the rest
      // of a large deck's automatic previews.
      const firstBackground = queue.findIndex((entry) => entry.background);
      if (!job.background && firstBackground >= 0)
        queue.splice(firstBackground, 0, job);
      else queue.push(job);
    });
    cache.set(key, request);
    // Failed pages must be retryable.
    void request.catch(() => cache.delete(key));
    pump();
    return request;
  };
  return {
    search,
    dispose() {
      disposed = true;
      queue.splice(0).forEach((job) => job.cancel());
      cache.clear();
    },
  };
}
