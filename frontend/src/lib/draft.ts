import {
  ARTWORK_PAGE_SIZE,
  type ArtworkChoice,
  type ArtworkSearch,
} from "./artwork.ts";

export type DraftImage = {
  query: string;
  face: "front" | "back";
  source_lines: number[];
};
export type DraftIssue = { code: string; message: string; line?: number };
export type DraftPreview = {
  quantity: number;
  unresolved: DraftImage[];
  retained?: (DraftImage & {
    card: { drive_id: string; name: string | null; source: string };
  })[];
  common_back?: { drive_id: string; name: string | null } | null;
  needs_common_back: boolean;
  issues: DraftIssue[];
};
export type ImageState = {
  item: DraftImage;
  choice: ArtworkChoice | null;
  status: "searching" | "no-match" | "failed" | "selected";
  busy: boolean;
  error: string;
  request: number;
  requestedIndex: number;
  originalId?: string;
  modified: boolean;
};
export type DraftState = {
  preview: DraftPreview | null;
  images: ImageState[];
  updating: boolean;
  error: string;
};

// The Rust parser uses eq_ignore_ascii_case; Unicode lowercasing differs.
export function imageKey(item: Pick<DraftImage, "query" | "face">) {
  return JSON.stringify([
    item.query.replace(/[A-Z]/g, (letter) => letter.toLowerCase()),
    item.face,
  ]);
}

export function lineRange(text: string, line: number) {
  const lines = text.split("\n");
  const index = Math.max(0, Math.min(line - 1, lines.length - 1));
  const start = lines
    .slice(0, index)
    .reduce((offset, value) => offset + value.length + 1, 0);
  return { start, end: start + lines[index].length };
}

export function createDraftController(options: {
  preview: (text: string) => Promise<DraftPreview>;
  search: ArtworkSearch;
  changed: (state: DraftState) => void;
  delay?: number;
  initialChoices?: Record<string, ArtworkChoice>;
}) {
  let revision = 0;
  let disposed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let text = "";
  const cache = new Map<string, ImageState>();
  let state: DraftState = {
    preview: null,
    images: [],
    updating: false,
    error: "",
  };
  function publish() {
    if (!disposed) options.changed({ ...state, images: [...state.images] });
  }
  function choose(item: DraftImage, choice: ArtworkChoice) {
    const entry = cache.get(imageKey(item));
    if (!entry || disposed || state.updating) return;
    entry.request++;
    entry.requestedIndex = choice.index;
    entry.choice = choice;
    entry.modified = choice.art.drive_id !== entry.originalId;
    entry.busy = false;
    entry.status = "selected";
    entry.error = "";
    publish();
  }
  async function load(
    item: DraftImage,
    index = 0,
    background = false,
    refresh = false,
  ) {
    const entry = cache.get(imageKey(item));
    if (!entry || disposed) return;
    const request = ++entry.request;
    entry.requestedIndex = index;
    const query = entry.choice?.query ?? item.query;
    entry.busy = true;
    entry.status = "searching";
    entry.error = "";
    publish();
    try {
      const found = await options.search(
        query,
        Math.floor(index / ARTWORK_PAGE_SIZE),
        "card",
        background ? "background" : "interactive",
        refresh,
      );
      if (disposed || entry.request !== request) return;
      const art = found.cards[index % ARTWORK_PAGE_SIZE];
      if (art) {
        entry.choice = { art, query, index, count: found.count };
        entry.modified = art.drive_id !== entry.originalId;
        entry.status = "selected";
      } else {
        entry.status = entry.choice ? "selected" : "no-match";
        entry.error = found.count
          ? "This artwork is no longer available. Choose another image."
          : "No matches. Edit the entry or search for artwork using another name.";
      }
    } catch (error) {
      if (disposed || entry.request !== request) return;
      entry.status = "failed";
      entry.error = `Search failed for “${query}”: ${String(error)}`;
    } finally {
      if (!disposed && entry.request === request) {
        entry.busy = false;
        publish();
      }
    }
  }
  async function refresh(token: number) {
    try {
      const preview = await options.preview(text);
      if (disposed || token !== revision) return;
      const fresh: DraftImage[] = [];
      const items = [...(preview.retained ?? []), ...preview.unresolved].sort(
        (a, b) =>
          a.source_lines[0] - b.source_lines[0] || Number(a.face === "back") - Number(b.face === "back"),
      );
      const images = items.map((item) => {
        const key = imageKey(item);
        let entry = cache.get(key);
        if (!entry) {
          const retained =
            "card" in item
              ? (item.card as NonNullable<
                  DraftPreview["retained"]
                >[number]["card"])
              : null;
          const saved = options.initialChoices?.[key];
          const choice = saved ?? (retained ? retainedChoice(retained, item.query) : null);
          entry = {
            item,
            choice,
            originalId: retained?.drive_id,
            modified: !!saved && saved.art.drive_id !== retained?.drive_id,
            status: choice ? "selected" : "searching",
            busy: !choice,
            error: "",
            request: 0,
            requestedIndex: 0,
          };
          cache.set(key, entry);
          if (!choice) fresh.push(item);
        }
        entry.item = item;
        return entry;
      });
      state = { preview, images, updating: false, error: "" };
      publish();
      fresh.forEach((item) => void load(item, 0, true));
    } catch (error) {
      if (disposed || token !== revision) return;
      state = {
        ...state,
        updating: false,
        preview: null,
        error: `Could not preview the decklist: ${String(error)}`,
      };
      publish();
    }
  }
  return {
    update(value: string, immediate = false) {
      if (disposed) return;
      text = value;
      const token = ++revision;
      clearTimeout(timer);
      state = { ...state, updating: true, error: "" };
      publish();
      if (immediate) void refresh(token);
      else timer = setTimeout(() => void refresh(token), options.delay ?? 350);
    },
    choose,
    load,
    retry(item: DraftImage) {
      const entry = cache.get(imageKey(item));
      if (entry && !entry.busy)
        return load(item, entry.requestedIndex, false, true);
    },
    dispose() {
      disposed = true;
      revision++;
      clearTimeout(timer);
      cache.clear();
    },
  };
}

export function retainedChoice(
  card: { drive_id: string; name: string | null },
  query = "",
): ArtworkChoice {
  const name = card.name || card.drive_id;
  return {
    art: {
      drive_id: card.drive_id,
      name,
      label: name.replace(/\.(png|jpe?g|webp)$/i, ""),
      source: "Retained artwork",
      dpi: 0,
      thumb: "",
    },
    query,
    index: 0,
    count: 0,
  };
}
