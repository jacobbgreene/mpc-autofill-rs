export type Face = "front" | "back";
export type Status =
  | "pending"
  | "downloaded"
  | "download-failed"
  | "uploading"
  | "interrupted"
  | "inserted"
  | "skipped";
export type Filter = "all" | "back" | "attention";

export type Card = {
  face: Face;
  drive_id: string;
  name: string;
  query?: string | null;
  slots: number[];
  cardback: boolean;
  thumb?: string;
  thumbStatus?: "loading" | "ready" | "failed";
  thumbError?: string;
  source?: "GoogleDrive" | "LocalFile";
  dpi?: number | null;
  status: Status;
  error?: string;
};

export type OrderSummary = {
  name: string;
  quantity: number;
  stock: string;
  foil: boolean;
  cards: Omit<Card, "status">[];
  path: string;
  dirty: boolean;
};

export const statusLabels: Record<Status, string> = {
  pending: "Not started",
  downloaded: "Image ready",
  "download-failed": "Download failed",
  uploading: "Uploading",
  interrupted: "Placement not confirmed",
  inserted: "Added to project",
  skipped: "Skipped",
};

export const cardKey = (card: Pick<Card, "face" | "drive_id">) =>
  `${card.face}:${card.drive_id}`;
export const needsAttention = (card: Card) =>
  card.status === "download-failed" ||
  card.status === "skipped" ||
  card.status === "interrupted";

export function displayName(
  card: Pick<Card, "name" | "drive_id" | "cardback">,
): string {
  if (card.cardback && card.name === card.drive_id) return "Common cardback";
  const name = card.name.replace(/\.(png|jpe?g|webp)$/i, "");
  const idSuffix = ` (${card.drive_id})`;
  return name.endsWith(idSuffix) ? name.slice(0, -idSuffix.length) : name;
}

export function filterCards(
  cards: Card[],
  filter: Filter,
  query: string,
): Card[] {
  const search = query.trim().toLocaleLowerCase();
  return cards.filter(
    (card) =>
      (filter === "all" ||
        (filter === "attention"
          ? needsAttention(card)
          : card.face === filter)) &&
      (!search || card.name.toLocaleLowerCase().includes(search)),
  );
}

export function autofillRecovery(phase: string, error: string): string {
  if (error.includes("browser/closed"))
    return "The automation browser was closed. Your deck is unchanged. Start over to create a new browser project; an unfinished project may not have been saved by MakePlayingCards.";
  const stage =
    (
      {
        downloading: "preparing images",
        "starting-browser": "opening Chrome",
        "awaiting-sign-in": "signing in",
        configuring: "setting up the project",
        "uploading-fronts": "adding card fronts",
        "paging-to-backs": "opening card backs",
        "uploading-backs": "adding card backs",
        "paging-to-review": "opening review",
      } as Record<string, string>
    )[phase] ?? "starting Chrome";
  return `Autofill stopped while ${stage}. Your deck is unchanged. ${phase === "downloading" ? "Check your connection and retry." : "Check the Chrome window and your connection, then start over to create a new project. Any partial project remains in Chrome while that window is open."}`;
}

export function artworkQuery(card: Card): string {
  return card.cardback ? "" : card.query?.trim() || displayName(card);
}

// Downloads and thumbnails are shared by image; placement belongs to one face.
export function patchCards(
  cards: Card[],
  driveId: string,
  change: Partial<Card>,
  face?: Face,
): Card[] {
  return cards.map((card) =>
    card.drive_id === driveId && (!face || card.face === face)
      ? { ...card, ...change }
      : card,
  );
}

export function phaseIndex(phase: string): number {
  switch (phase) {
    case "downloading":
      return 0;
    case "awaiting-sign-in":
    case "starting-browser":
    case "configuring":
      return 1;
    case "uploading-fronts":
      return 2;
    case "paging-to-backs":
    case "uploading-backs":
      return 3;
    case "paging-to-review":
    case "done":
      return 4;
    default:
      return -1;
  }
}

export function placementCounts(cards: Card[], face?: Face) {
  const relevant = face ? cards.filter((card) => card.face === face) : cards;
  return {
    total: relevant.length,
    inserted: relevant.filter((card) => card.status === "inserted").length,
    skipped: relevant.filter((card) => card.status === "skipped").length,
    attention: relevant.filter(needsAttention).length,
  };
}
